//! The catalog: this Mac's offices, as the folders under `<home>/offices/` that hold a valid
//! `office.json`, plus the rules for adding one (the name is unique among this Mac's
//! organizations ignoring case, and the office id's socket handle is unique ignoring case) and
//! for finding unfinished creates.
//!
//! The catalog holds no process state; status comes from supervision.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use org_office_launch::{
    office_dir, OfficeFile, MAX_OFFICE_FILE_BYTES, OFFICES_DIR_NAME, OFFICE_FILE_NAME,
};
use org_proto::{OfficeRole, OrganizationName};

use crate::host_log;
use crate::ids::handle_of;

/// The folder under `offices/` that the folders of refused creates are moved to (see
/// `org_office_launch`, "A refused create"). It isn't a plain id, so the catalog never lists,
/// starts or discards anything in it.
pub(crate) const REFUSED_DIR_NAME: &str = ".refused";

/// The name of every HQ office the host creates.
pub(crate) const HQ_OFFICE_NAME: &str = "HQ";

/// How many fresh office ids to try before giving up on a unique handle. A clash of 60 random
/// bits is already all but impossible; this only bounds a broken random source.
const MAX_ID_ATTEMPTS: usize = 16;

/// The offices on this Mac.
#[derive(Debug, Default)]
pub(crate) struct Catalog {
    offices: Vec<OfficeFile>,
    /// Folders whose `office.json` names another office: never listed, started or discarded;
    /// the host moves them to `offices/.refused/`.
    misplaced: Vec<String>,
    /// Creates in flight: the office id each one planned (for its handle) and its name's key.
    /// A reservation ends when its create fails ([`Catalog::release`]) or its office is in the
    /// catalog ([`Catalog::insert`]).
    reserved: Vec<Reservation>,
}

#[derive(Debug)]
struct Reservation {
    office_id: String,
    name_key: String,
}

/// One organization and its offices on this Mac, HQ first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OrganizationEntry<'a> {
    pub(crate) id: &'a str,
    pub(crate) name: &'a str,
    pub(crate) created_at: &'a str,
    pub(crate) offices: Vec<&'a OfficeFile>,
}

/// A new organization and its HQ office, checked against the catalog: what the create message
/// carries, apart from the app token, which supervision adds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CreatePlan {
    pub(crate) org_id: String,
    pub(crate) org_name: String,
    pub(crate) office_id: String,
    pub(crate) office_name: String,
    pub(crate) role: OfficeRole,
}

/// Why a create can't go ahead.
#[derive(Debug)]
pub(crate) enum PlanError {
    /// An organization with that name (ignoring case) is already on this Mac.
    NameTaken,
    /// No id could be made.
    Ids(io::Error),
}

impl Catalog {
    /// Reads every `<home>/offices/<id>/office.json`. Folders that aren't an office (no or a
    /// bad `office.json`, an unsupported version, a folder name that isn't its `office_id`, a
    /// socket handle that clashes with an earlier office) are skipped and logged, never
    /// removed. Those whose valid `office.json` names another office are also noted as
    /// [`Catalog::misplaced`]. Unfinished creates are listed by [`unfinished_folders`].
    pub(crate) fn scan(home: &Path) -> io::Result<Self> {
        let mut catalog = Self::default();
        for (folder, path) in office_folders(home)? {
            let file = path.join(OFFICE_FILE_NAME);
            let office = match read_office_file(&file) {
                Ok(Some(office)) => office,
                Ok(None) => continue, // unfinished: no office.json
                Err(why) => {
                    host_log::log(&format!("skipping office folder {folder}: {why}"));
                    continue;
                }
            };
            if office.office_id != folder {
                host_log::log(&format!(
                    "skipping office folder {folder}: its office.json is for office {}",
                    office.office_id
                ));
                catalog.misplaced.push(folder);
                continue;
            }
            if catalog.handle_taken(&office.office_id) {
                host_log::log(&format!(
                    "skipping office folder {folder}: its socket handle clashes with another office"
                ));
                continue;
            }
            catalog.offices.push(office);
        }
        Ok(catalog)
    }

    pub(crate) fn offices(&self) -> &[OfficeFile] {
        &self.offices
    }

    /// The folders the scan found holding another office's `office.json`.
    pub(crate) fn misplaced(&self) -> &[String] {
        &self.misplaced
    }

    /// Adds an office the host has just created, ending its create's reservation.
    pub(crate) fn insert(&mut self, office: OfficeFile) {
        self.release(&office.office_id);
        self.offices.push(office);
    }

    /// Whether `office_id` is an office on this Mac.
    pub(crate) fn has_office(&self, office_id: &str) -> bool {
        self.offices.iter().any(|o| o.office_id == office_id)
    }

    /// The office `office_id`, if it's on this Mac.
    pub(crate) fn office(&self, office_id: &str) -> Option<&OfficeFile> {
        self.offices.iter().find(|o| o.office_id == office_id)
    }

    /// Whether a create in flight planned `office_id`.
    pub(crate) fn is_reserved(&self, office_id: &str) -> bool {
        self.reserved.iter().any(|r| r.office_id == office_id)
    }

    /// Ends the reservation of the create that planned `office_id` (if it still has one).
    pub(crate) fn release(&mut self, office_id: &str) {
        self.reserved.retain(|r| r.office_id != office_id);
    }

    /// Whether an organization on this Mac, or a create in flight, has this name, ignoring case.
    pub(crate) fn has_organization_named(&self, name: &OrganizationName) -> bool {
        let key = name.key();
        self.offices.iter().any(|o| name_key(&o.org_name) == key)
            || self.reserved.iter().any(|r| r.name_key == key)
    }

    /// Whether an office on this Mac, or a create in flight, has the same socket handle as
    /// `office_id`, ignoring case (macOS file systems usually are case-insensitive).
    pub(crate) fn handle_taken(&self, office_id: &str) -> bool {
        let handle = handle_of(office_id);
        let taken = |id: &str| handle_of(id).eq_ignore_ascii_case(handle);
        self.offices.iter().any(|o| taken(&o.office_id))
            || self.reserved.iter().any(|r| taken(&r.office_id))
    }

    /// The details of a new organization and its HQ, checked against this catalog and the
    /// creates in flight: the name must be free, and the office id's handle unique (re-rolled
    /// until it is). The name and handle are then reserved, until the create fails
    /// ([`Catalog::release`]) or its office is inserted. `new_id` makes ids (injected so the
    /// re-roll can be tested).
    pub(crate) fn plan_create(
        &mut self,
        name: &OrganizationName,
        mut new_id: impl FnMut() -> io::Result<String>,
    ) -> Result<CreatePlan, PlanError> {
        if self.has_organization_named(name) {
            return Err(PlanError::NameTaken);
        }
        let org_id = new_id().map_err(PlanError::Ids)?;
        for _ in 0..MAX_ID_ATTEMPTS {
            let office_id = new_id().map_err(PlanError::Ids)?;
            if !self.handle_taken(&office_id) {
                self.reserved.push(Reservation {
                    office_id: office_id.clone(),
                    name_key: name.key().to_owned(),
                });
                return Ok(CreatePlan {
                    org_id,
                    org_name: name.as_str().to_owned(),
                    office_id,
                    office_name: HQ_OFFICE_NAME.to_owned(),
                    role: OfficeRole::Hq,
                });
            }
        }
        Err(PlanError::Ids(io::Error::other(
            "no office id with a unique socket handle",
        )))
    }

    /// The organizations on this Mac, ordered by `created_at` then id, each with its offices
    /// here (HQ first, then by id). An organization's name and `created_at` come from its HQ,
    /// or from its first office when HQ isn't on this Mac.
    pub(crate) fn organizations(&self) -> Vec<OrganizationEntry<'_>> {
        let mut by_org: BTreeMap<&str, Vec<&OfficeFile>> = BTreeMap::new();
        for office in &self.offices {
            by_org.entry(&office.org_id).or_default().push(office);
        }
        let mut orgs: Vec<OrganizationEntry<'_>> = by_org
            .into_iter()
            .map(|(id, mut offices)| {
                offices.sort_by(|a, b| {
                    (a.role != OfficeRole::Hq, &a.office_id)
                        .cmp(&(b.role != OfficeRole::Hq, &b.office_id))
                });
                let first = offices[0];
                OrganizationEntry {
                    id,
                    name: &first.org_name,
                    created_at: &first.created_at,
                    offices,
                }
            })
            .collect();
        orgs.sort_by(|a, b| (a.created_at, a.id).cmp(&(b.created_at, b.id)));
        orgs
    }
}

/// The comparison key of a stored name. Stored names were checked by the server; one that no
/// longer parses still compares by its trimmed lowercase, so it can't be duplicated.
fn name_key(stored: &str) -> String {
    OrganizationName::parse(stored)
        .map(|n| n.key().to_owned())
        .unwrap_or_else(|_| stored.trim().to_lowercase())
}

/// Reads `office.json`: `Ok(None)` if there is none, an error message (for the log) if it's
/// unreadable, too large, a symlink, not a regular file owned by this user, or not a valid office
/// file of a supported version.
pub(crate) fn read_office_file(path: &Path) -> Result<Option<OfficeFile>, String> {
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{OFFICE_FILE_NAME} can't be opened: {e}")),
    };
    match file.metadata() {
        Ok(meta) if meta.is_file() && meta.uid() == crate::home::euid() => {}
        Ok(meta) if meta.is_file() => {
            return Err(format!("{OFFICE_FILE_NAME} isn't owned by this user"))
        }
        Ok(_) => return Err(format!("{OFFICE_FILE_NAME} isn't a regular file")),
        Err(e) => return Err(format!("{OFFICE_FILE_NAME} can't be read: {e}")),
    }
    let mut bytes = Vec::new();
    file.take(MAX_OFFICE_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{OFFICE_FILE_NAME} can't be read: {e}"))?;
    if bytes.len() > MAX_OFFICE_FILE_BYTES {
        return Err(format!(
            "{OFFICE_FILE_NAME} is larger than {MAX_OFFICE_FILE_BYTES} bytes"
        ));
    }
    OfficeFile::from_slice(&bytes)
        .map(Some)
        .map_err(|e| format!("{OFFICE_FILE_NAME} is invalid: {e}"))
}

/// The folders under `<home>/offices/` whose names are plain ids, sorted by name. Anything
/// else (files, symlinks, other names) isn't an office.
fn office_folders(home: &Path) -> io::Result<Vec<(String, PathBuf)>> {
    let mut folders = Vec::new();
    let offices = home.join(org_office_launch::OFFICES_DIR_NAME);
    for entry in fs::read_dir(offices)? {
        let entry = entry?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if !entry.file_type()?.is_dir() {
            continue;
        }
        if let Some(path) = office_dir(home, &name) {
            folders.push((name, path));
        }
    }
    folders.sort();
    Ok(folders)
}

/// The office folders with no `office.json`: creates that never finished. The host never
/// removes them itself; it has a server discard each one (see `Offices::discard_unfinished`),
/// because only the server can delete the Keychain key an unfinished create may have left.
pub(crate) fn unfinished_folders(home: &Path) -> io::Result<Vec<String>> {
    Ok(office_folders(home)?
        .into_iter()
        .filter(|(_, path)| !has_office_file(path))
        .map(|(folder, _)| folder)
        .collect())
}

/// Moves an office folder the host refuses (a refused create's, or one holding another office's
/// `office.json`), `offices/<office_id>`, to `offices/.refused/<office_id>` (`<office_id>-<unix ms>` if that name is taken), out of the
/// catalog's sight, and returns where it went. Makes `.refused/` owner-only if it's missing.
/// Follows no symlinks: the office folder and `.refused/` must be real folders, or nothing moves.
pub(crate) fn move_refused(home: &Path, office_id: &str) -> io::Result<PathBuf> {
    let dir =
        office_dir(home, office_id).ok_or_else(|| io::Error::other("the office id isn't plain"))?;
    if !fs::symlink_metadata(&dir)?.is_dir() {
        return Err(io::Error::other("the office folder isn't a folder"));
    }
    let refused = home.join(OFFICES_DIR_NAME).join(REFUSED_DIR_NAME);
    match fs::DirBuilder::new().mode(0o700).create(&refused) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    if !fs::symlink_metadata(&refused)?.is_dir() {
        return Err(io::Error::other(format!(
            "offices/{REFUSED_DIR_NAME} isn't a folder"
        )));
    }
    let mut target = refused.join(office_id);
    if fs::symlink_metadata(&target).is_ok() {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        target = refused.join(format!("{office_id}-{now_ms}"));
    }
    fs::rename(&dir, &target)?;
    Ok(target)
}

/// Whether the office `office_id` has a stored `office.json` (even an unreadable one): such a
/// folder is never discarded.
pub(crate) fn is_stored(home: &Path, office_id: &str) -> bool {
    office_dir(home, office_id).is_some_and(|dir| has_office_file(&dir))
}

/// Whether an office folder holds an `office.json` (of any kind, even unreadable). Only a
/// definite "not found" counts as none: a folder that can't be checked is never taken for an
/// unfinished create, so it is never discarded.
fn has_office_file(dir: &Path) -> bool {
    match fs::symlink_metadata(dir.join(OFFICE_FILE_NAME)) {
        Ok(_) => true,
        Err(e) => e.kind() != io::ErrorKind::NotFound,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    const T: &str = "2026-11-08T00:00:00.000Z";

    fn office(id: &str, org: &str, name: &str, role: OfficeRole, at: &str) -> OfficeFile {
        OfficeFile::new(id, "HQ", role, org, name, at)
    }

    fn write(home: &Path, folder: &str, bytes: &[u8]) {
        let dir = home.join("offices").join(folder);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(OFFICE_FILE_NAME), bytes).unwrap();
    }

    #[test]
    fn names_are_unique_ignoring_case_and_handles_are_rerolled() {
        let mut catalog = Catalog::default();
        catalog.insert(office(
            "01AAAAAAAAAAAAAAAAAAAAAAAA",
            "01ORG",
            "Acme",
            OfficeRole::Hq,
            T,
        ));
        let acme = OrganizationName::parse(" ACME ").unwrap();
        assert!(matches!(
            catalog.plan_create(&acme, || unreachable!()),
            Err(PlanError::NameTaken)
        ));

        // The second id clashes with the existing handle (ignoring case), so it is re-rolled.
        let mut ids = vec![
            "01ORGNEW".to_owned(),
            "01BBBBBBBBBBBBAAAAAAAAAAAA".to_lowercase(),
            "01CCCCCCCCCCCCCCCCCCCCCCCC".to_owned(),
        ]
        .into_iter();
        let other = OrganizationName::parse("Other").unwrap();
        let plan = catalog
            .plan_create(&other, || Ok(ids.next().unwrap()))
            .unwrap();
        assert_eq!(plan.org_id, "01ORGNEW");
        assert_eq!(plan.office_id, "01CCCCCCCCCCCCCCCCCCCCCCCC");
        assert_eq!(plan.org_name, "Other");
        assert_eq!(plan.office_name, "HQ");
        assert_eq!(plan.role, OfficeRole::Hq);
    }

    #[test]
    fn a_planned_create_reserves_its_name_and_handle_until_released_or_inserted() {
        let mut catalog = Catalog::default();
        let acme = OrganizationName::parse("Acme").unwrap();
        let first = catalog
            .plan_create(&acme, {
                let mut ids = ["01ORG1", "01AAAAAAAAAAAAAAAAAAAAAAAA"].into_iter();
                move || Ok(ids.next().unwrap().to_owned())
            })
            .unwrap();
        // The name is taken while the first create is in flight, ignoring case.
        let upper = OrganizationName::parse("ACME").unwrap();
        assert!(matches!(
            catalog.plan_create(&upper, || unreachable!()),
            Err(PlanError::NameTaken)
        ));
        // So is its handle: another name's office id with the same handle is re-rolled.
        let other = OrganizationName::parse("Other").unwrap();
        let mut ids = [
            "01ORG2",
            "01BBBBBBBBBBBBAAAAAAAAAAAA",
            "01CCCCCCCCCCCCCCCCCCCCCCCC",
        ]
        .into_iter();
        let second = catalog
            .plan_create(&other, || Ok(ids.next().unwrap().to_owned()))
            .unwrap();
        assert_eq!(second.office_id, "01CCCCCCCCCCCCCCCCCCCCCCCC");
        // A failed create releases its reservation; inserting the office ends it too.
        catalog.release(&first.office_id);
        assert!(!catalog.has_organization_named(&upper));
        assert!(!catalog.handle_taken(&first.office_id));
        catalog.insert(office(
            &second.office_id,
            &second.org_id,
            &second.org_name,
            OfficeRole::Hq,
            T,
        ));
        assert!(catalog.reserved.is_empty());
        assert!(catalog.has_organization_named(&other));
    }

    #[test]
    fn organizations_are_ordered_with_hq_first() {
        let mut catalog = Catalog::default();
        let (early, late) = ("2026-11-08T01:00:00.000Z", "2026-11-08T02:00:00.000Z");
        catalog.insert(office("SAT1", "ORGB", "B", OfficeRole::Satellite, late));
        catalog.insert(office("HQB", "ORGB", "B", OfficeRole::Hq, late));
        catalog.insert(office("HQA", "ORGA", "A", OfficeRole::Hq, early));
        catalog.insert(office("HQC", "ORGC", "C", OfficeRole::Hq, early));
        let orgs = catalog.organizations();
        let ids: Vec<_> = orgs.iter().map(|o| o.id).collect();
        assert_eq!(ids, ["ORGA", "ORGC", "ORGB"]);
        let b: Vec<_> = orgs[2].offices.iter().map(|o| &o.office_id[..]).collect();
        assert_eq!(b, ["HQB", "SAT1"]);
    }

    #[test]
    fn scan_skips_bad_folders() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        fs::create_dir_all(home.join("offices")).unwrap();
        let good_id = "01GOODAAAAAAAAAAAAAAAAAAAA";
        let good = office(good_id, "ORG1", "Good", OfficeRole::Hq, T);
        write(home, good_id, &good.to_vec().unwrap());
        write(home, "BADJSON", b"{not json");
        write(
            home,
            "NEWER",
            br#"{"v":2,"office_id":"NEWER","office_name":"HQ","org_id":"O","org_name":"N","created_at":"t"}"#,
        );
        let other = office("SOMEONEELSE", "ORG2", "Other", OfficeRole::Hq, T);
        write(home, "MISMATCH", &other.to_vec().unwrap());
        let mut big = good.to_vec().unwrap();
        big.resize(MAX_OFFICE_FILE_BYTES + 10, b' ');
        write(home, "BIG", &big);
        // The same handle as the good office's, differing in case only; scanned after it.
        let clash_id = "02CLASHaaaaaaaaaaaaaaaaaaa";
        let clash = office(clash_id, "ORG3", "Clash", OfficeRole::Hq, T);
        write(home, clash_id, &clash.to_vec().unwrap());
        fs::create_dir_all(home.join("offices/UNFINISHED")).unwrap();
        fs::create_dir_all(home.join("offices/not-plain")).unwrap();
        // A symlinked office.json, and a symlinked office folder, both pointing at a valid
        // office: neither is followed.
        let linked_id = "01LINKEDBBBBBBBBBBBBBBBBBB";
        let elsewhere = home.join("elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        let linked = office(linked_id, "ORG5", "Linked", OfficeRole::Hq, T);
        fs::write(elsewhere.join(OFFICE_FILE_NAME), linked.to_vec().unwrap()).unwrap();
        fs::create_dir_all(home.join("offices").join(linked_id)).unwrap();
        std::os::unix::fs::symlink(
            elsewhere.join(OFFICE_FILE_NAME),
            home.join("offices").join(linked_id).join(OFFICE_FILE_NAME),
        )
        .unwrap();
        let linked_dir_id = "01LINKEDDIRCCCCCCCCCCCCCCC";
        let dir_target = home.join("dir-target");
        fs::create_dir_all(&dir_target).unwrap();
        let linked_dir = office(linked_dir_id, "ORG6", "LinkedDir", OfficeRole::Hq, T);
        fs::write(
            dir_target.join(OFFICE_FILE_NAME),
            linked_dir.to_vec().unwrap(),
        )
        .unwrap();
        std::os::unix::fs::symlink(&dir_target, home.join("offices").join(linked_dir_id)).unwrap();
        // A refused create, moved aside, with a valid office.json: not an office.
        let refused_id = "01REFUSEDAAAAAAAAAAAAAAAAA";
        let refused = office(refused_id, "ORG4", "Refused", OfficeRole::Hq, T);
        write(
            home,
            &format!("{REFUSED_DIR_NAME}/{refused_id}"),
            &refused.to_vec().unwrap(),
        );

        let catalog = Catalog::scan(home).unwrap();
        assert_eq!(catalog.offices(), [good]);
        assert_eq!(catalog.misplaced(), ["MISMATCH"]);
    }

    #[test]
    fn a_refused_folder_is_moved_aside_without_following_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let offices = home.join("offices");
        let refused = offices.join(REFUSED_DIR_NAME);
        // Moved, with what it holds, into an owner-only .refused/.
        write(home, "REFUSED", b"{}");
        let moved = move_refused(home, "REFUSED").unwrap();
        assert_eq!(moved, refused.join("REFUSED"));
        assert!(!offices.join("REFUSED").exists());
        assert_eq!(fs::read(moved.join(OFFICE_FILE_NAME)).unwrap(), b"{}");
        let mode = fs::metadata(&refused).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        // The same name again: moved under a new name, the first one kept.
        write(home, "REFUSED", b"{\"again\":1}");
        let again = move_refused(home, "REFUSED").unwrap();
        let name = again.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with("REFUSED-"), "{name}");
        assert!(name["REFUSED-".len()..].bytes().all(|b| b.is_ascii_digit()));
        assert_eq!(fs::read(moved.join(OFFICE_FILE_NAME)).unwrap(), b"{}");
        // No folder, or an id that isn't plain: an error.
        assert!(move_refused(home, "NOFOLDER").is_err());
        assert!(move_refused(home, "../REFUSED").is_err());
        // The folder is a symlink: it and its target stay.
        let elsewhere = home.join("elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, offices.join("LINKEDDIR")).unwrap();
        assert!(move_refused(home, "LINKEDDIR").is_err());
        assert!(fs::symlink_metadata(offices.join("LINKEDDIR")).is_ok());
        assert!(!refused.join("LINKEDDIR").exists());
        // .refused/ is a symlink: nothing moves through it.
        let other_home = tempfile::tempdir().unwrap();
        let other = other_home.path();
        write(other, "KEPT", b"{}");
        std::os::unix::fs::symlink(&elsewhere, other.join("offices").join(REFUSED_DIR_NAME))
            .unwrap();
        assert!(move_refused(other, "KEPT").is_err());
        assert!(other.join("offices/KEPT/office.json").exists());
        assert!(!elsewhere.join("KEPT").exists());
    }

    #[test]
    fn an_office_json_that_isnt_a_regular_file_is_refused_and_left_alone() {
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let offices = home.join("offices");
        // A directory named office.json, and a FIFO.
        let as_dir = "01DIRAAAAAAAAAAAAAAAAAAAAA";
        fs::create_dir_all(offices.join(as_dir).join(OFFICE_FILE_NAME)).unwrap();
        let as_fifo = "01FIFOBBBBBBBBBBBBBBBBBBBB";
        fs::create_dir_all(offices.join(as_fifo)).unwrap();
        let fifo = offices.join(as_fifo).join(OFFICE_FILE_NAME);
        let fifo_c = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        // SAFETY: a valid NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o600) }, 0);

        for id in [as_dir, as_fifo] {
            let err = read_office_file(&offices.join(id).join(OFFICE_FILE_NAME)).unwrap_err();
            assert!(err.contains("isn't a regular file"), "{id}: {err}");
            assert!(is_stored(home, id), "{id}");
        }
        // Never listed, never moved aside, never discarded.
        let catalog = Catalog::scan(home).unwrap();
        assert!(catalog.offices().is_empty());
        assert!(catalog.misplaced().is_empty());
        assert!(unfinished_folders(home).unwrap().is_empty());
    }

    #[test]
    fn unfinished_folders_are_those_without_office_json() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let offices = home.join("offices");
        fs::create_dir_all(offices.join("UNFINISHED")).unwrap();
        fs::create_dir_all(offices.join("not-plain")).unwrap();
        fs::write(offices.join("AFILE"), b"").unwrap();
        write(home, "STORED", b"{broken");
        // Refused creates, moved aside: neither unfinished nor stored.
        fs::create_dir_all(offices.join(REFUSED_DIR_NAME).join("REFUSED")).unwrap();
        assert_eq!(unfinished_folders(home).unwrap(), ["UNFINISHED"]);
        assert!(!is_stored(home, REFUSED_DIR_NAME));
        assert!(is_stored(home, "STORED"));
        assert!(!is_stored(home, "UNFINISHED"));
        assert!(!is_stored(home, "../STORED"));
        // A folder that can't be searched: its office.json can't be checked, so it is kept.
        // (Root can search it anyway, so this part needs a normal user.)
        if crate::home::euid() == 0 {
            return;
        }
        write(home, "LOCKED", b"{}");
        let locked = offices.join("LOCKED");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let can_check = fs::symlink_metadata(locked.join(OFFICE_FILE_NAME)).is_ok();
        let unfinished = unfinished_folders(home).unwrap();
        let locked_stored = is_stored(home, "LOCKED");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(
            !can_check,
            "running as root? the folder was still searchable"
        );
        assert!(locked_stored);
        assert_eq!(unfinished, ["UNFINISHED"]);
    }
}
