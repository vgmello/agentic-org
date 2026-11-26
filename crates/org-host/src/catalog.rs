//! The catalog: this Mac's offices, as the folders under `<home>/offices/` that hold a valid
//! `office.json`, plus the rules for adding one (the name is unique among this Mac's
//! organizations ignoring case, and the office id's socket handle is unique ignoring case) and
//! for finding unfinished creates.
//!
//! The catalog holds no process state; status comes from supervision.

use std::collections::{BTreeMap, HashSet};
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

/// Why a create can't go ahead.
#[derive(Debug)]
pub(crate) enum PlanError {
    /// An organization with that name (ignoring case) is already on this Mac.
    NameTaken,
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

    /// Hold the exact selected reservation across incomplete attempts. The
    /// protected operation owns release/recovery; legacy failure cleanup does not.
    pub(crate) fn reserve_selected(
        &mut self,
        p: &org_proto::selected_bootstrap::PreparedV1,
    ) -> Result<(), PlanError> {
        if self.is_reserved(&p.office_id) {
            return Ok(());
        }
        let name = OrganizationName::parse(&p.plan.name).map_err(|_| PlanError::NameTaken)?;
        if self.has_organization_named(&name) || self.handle_taken(&p.office_id) {
            return Err(PlanError::NameTaken);
        }
        self.reserved.push(Reservation {
            office_id: p.office_id.clone(),
            name_key: name.key().into(),
        });
        Ok(())
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

/// Whether any office has a folder in this home, finished or not. A folder is made only
/// after its office's key was stored (the key and its fingerprint come first), so a home
/// with one is a home whose records say that keys exist.
pub(crate) fn any_office_folder(home: &Path) -> io::Result<bool> {
    Ok(!office_folders(home)?.is_empty())
}

/// The office folders with no `office.json`: creates that never finished. The host never
/// removes them itself; it has a server discard each one, with the office's key (see
/// `Offices::discard_unfinished`), because only the server's discard removes the folder under
/// the office lock, after checking that it holds nothing but the lock and the temporary files of
/// an unfinished publication. The host deletes the key after `discarded`.
///
/// Only a folder named by a UUIDv7 office id is listed: a server discards nothing else, so a
/// folder left by an older build under a plain id is skipped, logged in one line, and kept.
pub(crate) fn unfinished_folders(home: &Path) -> io::Result<Vec<String>> {
    let mut skipped = Vec::new();
    let mut unfinished = Vec::new();
    for (folder, path) in office_folders(home)? {
        if has_office_file(&path)
            || path.join("hq-creation.v1.json").exists()
            || holds_writer_custody(&path)
        {
            continue;
        }
        if org_office_launch::is_selected_entity_id(&folder) {
            unfinished.push(folder);
        } else {
            skipped.push(host_log::untrusted(&folder));
        }
    }
    if !skipped.is_empty() {
        host_log::log(&format!(
            "kept unfinished office folders not named by a UUIDv7 office id: {}; a server \
             can't discard them, so they are left for a person",
            skipped.join(", ")
        ));
    }
    Ok(unfinished)
}

/// Whether the host reserved writer custody in `folder` (`org_office_launch::writer::marker`):
/// its marker, or a staging file of one, which a host killed while it installed the marker
/// leaves. Such a folder is the reservation's: its protected operation finishes or keeps it,
/// and a worker's discard refuses it, so it is never "unfinished". A folder that can't be
/// read is kept as well.
fn holds_writer_custody(folder: &Path) -> bool {
    org_office_launch::writer::marker::holds_custody(folder).unwrap_or(true)
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

/// Of the offices `ids` (the ones with a key), those that have nothing left in `home`, or why
/// it can't tell (for the orphan-key sweep, `keys::sweep_orphans`). An office has something
/// left when a folder is named after it in `offices/` or `offices/.refused/` (`<id>` or
/// `<id>-<unix ms>`), or when an `office.json` in any folder there names it (a folder renamed by
/// hand, moved aside at startup with its data).
///
/// It refuses to answer (so nothing is marked or deleted) whenever the answer could be wrong:
/// - anything it must look at can't be read: `offices/`, `offices/.refused/`, any `office.json`
///   there, or an office's folder (one that can't be checked is never taken for an orphan);
/// - `offices/` is empty while there are keys: the home was likely moved, restored or re-created
///   elsewhere, and every key would look orphaned;
/// - more than half of the keys would be orphans at once (ada): the same mistake, partly.
pub(crate) fn orphans(home: &Path, ids: &[String]) -> Result<Vec<String>, String> {
    orphans_in(home, ids, |path| match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    })
}

/// [`orphans`], with `exists` saying whether an office's folder is there.
fn orphans_in(
    home: &Path,
    ids: &[String],
    exists: impl Fn(&Path) -> io::Result<bool>,
) -> Result<Vec<String>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let offices = home.join(OFFICES_DIR_NAME);
    let mut claimed = folders_in(&offices, false).map_err(|e| e.to_string())?;
    // `.refused/`, `.DS_Store` and the like aren't offices.
    if claimed.iter().all(|(name, _)| name.starts_with('.')) {
        return Err(format!(
            "offices/ is empty while the key store holds {} key(s): the office home may have \
             been moved, restored or re-created elsewhere",
            ids.len()
        ));
    }
    claimed.extend(folders_in(&offices.join(REFUSED_DIR_NAME), true).map_err(|e| e.to_string())?);
    let folder_names: Vec<&String> = claimed.iter().map(|(name, _)| name).collect();
    let named: HashSet<&String> = claimed.iter().filter_map(|(_, id)| id.as_ref()).collect();
    let mut orphans = Vec::new();
    for id in ids {
        match exists(&offices.join(id)) {
            Ok(true) => continue,
            Ok(false) => {}
            Err(e) => return Err(format!("the folder of office {id} can't be checked: {e}")),
        }
        if named.contains(id) || folder_names.iter().any(|name| is_refused_as(name, id)) {
            continue;
        }
        orphans.push(id.clone());
    }
    if orphans.len() * 2 > ids.len() {
        return Err(format!(
            "{} of the {} keys would be orphans at once (more than half): the office home may \
             not be the one these keys belong to",
            orphans.len(),
            ids.len()
        ));
    }
    Ok(orphans)
}

/// The entries in `dir`, each with the office id its `office.json` names, if it has one. With
/// `may_be_missing`, a `dir` that doesn't exist has none. An `office.json` that can't be read
/// (a symlink, a FIFO, someone else's, too large, invalid) is an error.
fn folders_in(dir: &Path, may_be_missing: bool) -> io::Result<Vec<(String, Option<String>)>> {
    let shown = dir.display();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if may_be_missing && e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(io::Error::new(
                e.kind(),
                format!("{shown} can't be read: {e}"),
            ))
        }
    };
    let mut folders = Vec::new();
    for entry in entries {
        let entry = entry?;
        let file = entry.path().join(OFFICE_FILE_NAME);
        let id = office_id_in(&file)?;
        folders.push((entry.file_name().to_string_lossy().into_owned(), id));
    }
    Ok(folders)
}

/// The office id `file` (an `office.json`) names, or `None` if there is no such file (or its
/// folder isn't a folder), read the catalog's careful way (`catalog::read_office_file`: no
/// symlink, never blocked by a FIFO, owned by this user, capped).
fn office_id_in(file: &Path) -> io::Result<Option<String>> {
    match read_office_file(file) {
        Ok(office) => Ok(office.map(|office| office.office_id)),
        // A plain file where a folder was expected has no office.json. (A folder that can't
        // even be looked at is an error, never "no office.json".)
        Err(why) => match file.parent().map(fs::metadata) {
            Some(Ok(meta)) if !meta.is_dir() => Ok(None),
            // A dangling symlink: nothing there.
            Some(Err(e)) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            _ => Err(io::Error::other(format!("{}: {why}", file.display()))),
        },
    }
}

/// Whether the folder `name` in `.refused/` is office `id`'s: `<id>`, or `<id>-<unix ms>` when
/// that name was taken (`catalog::move_refused`).
fn is_refused_as(name: &str, id: &str) -> bool {
    name == id
        || name
            .strip_prefix(id)
            .and_then(|rest| rest.strip_prefix('-'))
            .is_some_and(|ms| !ms.is_empty() && ms.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests;
