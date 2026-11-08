//! The orphan-key sweep: deleting the keys of offices that are gone, never on first sight.
//!
//! A key whose office has nothing left in this home can never open anything: its databases are
//! gone, or never existed (a create whose `--create` and `--discard` servers couldn't even
//! start). An office has something left when a folder is named after it in `offices/` or
//! `offices/.refused/` (`<id>` or `<id>-<unix ms>`), or when an `office.json` in any folder
//! there names it (a folder renamed by hand, moved aside at startup with its data). The host
//! sweeps once at startup, after the startup discards and before any create can store a key for
//! a folder not made yet.
//!
//! **Never on first sight.** Deleting a key can't be undone, so the sweep only *marks* an
//! orphaned key the first time it finds it, in the orphan record, and deletes it only at a host
//! start [`ORPHAN_GRACE`] (7 days) or more after it was first marked, if it is still orphaned
//! then. A key whose office came back is unmarked. The decision is [`decide`], a pure function.
//!
//! **The orphan record** is `<home>/orphan-keys.json` ([`ORPHAN_RECORD_FILE_NAME`]), next to
//! `host.lock`: owner-only (0600), written atomically (a temp file, fsync, rename, fsync of the
//! home), holding office ids and first-seen times only, never a key:
//! ```json
//! {"v":1,"first_seen_ms":{"01ABC…":1794096000000}}
//! ```
//! `first_seen_ms` maps each marked office id to when it was first found orphaned, in Unix
//! milliseconds (system time). A first-seen time in the future (the clock went back) isn't 7
//! days old: it is marked again from now. A record that can't be read (not a regular file of
//! this user, too large, not this shape or version, an id that isn't plain) is logged and
//! treated as empty: nothing is deleted that run, and the orphans found then are marked from
//! then (written as a fresh record when there are any).
//!
//! A clock that is plainly wrong, before [`CLOCK_FLOOR_MS`] (when this build's code was
//! written), skips the sweep, logged: marks taken at such a time would make the next start, on a
//! corrected clock, see them as long past.
//!
//! This is the one exception to "a key is deleted only after `discarded`", and it is safe only
//! when the key store belongs to this office home alone: `Host::start` decides whether it runs
//! (`may_sweep_orphan_keys`: release builds, on the account's own office home).
//!
//! It fails safe: if `offices/` or `offices/.refused/` can't be read, an `office.json` in them
//! can't be read, or the store can't list, nothing is marked, unmarked or deleted; a folder that
//! can't be checked keeps its key. Every mark, unmark and deletion is logged, with ids only.

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use org_office_launch::{is_plain_id, OFFICES_DIR_NAME, OFFICE_FILE_NAME, REFUSED_DIR_NAME};
use serde::{Deserialize, Serialize};

use super::KeyStore;
use crate::host_log;

/// The orphan record's file name, in the office home.
pub(crate) const ORPHAN_RECORD_FILE_NAME: &str = "orphan-keys.json";

/// How long a key must have stayed orphaned before a host start deletes it.
pub(crate) const ORPHAN_GRACE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// The orphan record's version.
const RECORD_VERSION: u32 = 1;

/// The largest orphan record read.
const MAX_RECORD_BYTES: u64 = 1024 * 1024;

/// 2026-01-01T00:00:00Z in Unix ms: a system time before it is wrong, and no sweep runs on it.
pub(crate) const CLOCK_FLOOR_MS: u64 = 1_767_225_600_000;

/// The marked orphans: each office id with when it was first found orphaned (Unix ms).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct OrphanRecord {
    pub(crate) first_seen_ms: BTreeMap<String, u64>,
}

/// The orphan record as stored.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordFile {
    v: u32,
    first_seen_ms: BTreeMap<String, u64>,
}

/// The policy, pure: given the keys orphaned now, the record and the time now (Unix ms), the
/// keys to delete (orphaned now, first marked [`ORPHAN_GRACE`] or more ago) and the record to
/// keep (every key orphaned now, with its first-seen time: kept, or now for a new one or one
/// marked in the future). A key no longer orphaned isn't in the new record (unmarked). The
/// caller removes from the new record the keys it did delete.
pub(crate) fn decide(
    orphans_now: &[String],
    record: &OrphanRecord,
    now_ms: u64,
) -> (Vec<String>, OrphanRecord) {
    let grace_ms = u64::try_from(ORPHAN_GRACE.as_millis()).unwrap_or(u64::MAX);
    let mut to_delete = Vec::new();
    let mut new = OrphanRecord::default();
    for id in orphans_now {
        let first_seen = match record.first_seen_ms.get(id) {
            Some(&at) if at <= now_ms => at,
            // New, or marked in the future (the clock went back): marked from now.
            _ => now_ms,
        };
        if now_ms - first_seen >= grace_ms {
            to_delete.push(id.clone());
        }
        new.first_seen_ms.insert(id.clone(), first_seen);
    }
    (to_delete, new)
}

/// Sweeps `store` for the offices of `home` at `now` (see the module docs): marks new orphans,
/// unmarks keys whose office came back, deletes keys orphaned for [`ORPHAN_GRACE`] or more, and
/// saves the record. Returns the ids it deleted.
pub(crate) fn sweep_orphans(store: &dyn KeyStore, home: &Path, now: SystemTime) -> Vec<String> {
    let now_ms = now.duration_since(UNIX_EPOCH).map_or(0, |since| {
        u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
    });
    if now_ms < CLOCK_FLOOR_MS {
        host_log::log("no orphan-key sweep: the system clock is before 2026, so it is wrong");
        return Vec::new();
    }
    let orphans = match find_orphans(store, home) {
        Ok(orphans) => orphans,
        Err(why) => {
            host_log::log(&format!("no orphan-key sweep: {why}"));
            return Vec::new();
        }
    };
    let path = home.join(ORPHAN_RECORD_FILE_NAME);
    let record = match load_record(&path) {
        Ok(record) => record,
        Err(why) => {
            // Empty: everything orphaned now is marked from now, so nothing is deleted.
            host_log::log(&format!(
                "the orphan-key record can't be used ({why}): no key is deleted this time, and \
                 the orphans found now are marked again from now"
            ));
            OrphanRecord::default()
        }
    };
    let (to_delete, mut new) = decide(&orphans, &record, now_ms);
    for id in record.first_seen_ms.keys() {
        if !new.first_seen_ms.contains_key(id) {
            host_log::log(&format!(
                "unmarked the key of office {id}: it is no longer orphaned"
            ));
        }
    }
    for id in new.first_seen_ms.keys() {
        if !record.first_seen_ms.contains_key(id) {
            host_log::log(&format!(
                "marked the key of office {id}: it has no folder in offices/; it is deleted at a \
                 host start 7 days or more from now if it still has none"
            ));
        }
    }
    let mut deleted = Vec::new();
    for id in to_delete {
        match store.delete(&id) {
            Ok(()) => {
                host_log::log(&format!(
                    "deleted the key of office {id}: it has had no folder in offices/ for 7 days \
                     or more"
                ));
                new.first_seen_ms.remove(&id);
                deleted.push(id);
            }
            // Still marked: tried again at the next start.
            Err(e) => host_log::log(&format!("couldn't delete the orphan key of {id}: {e}")),
        }
    }
    if new != record {
        match save_record(home, &path, &new) {
            Ok(Ok(())) => {}
            Ok(Err(e)) => host_log::log(&format!(
                "saved the orphan-key record, but couldn't sync the office home: {e}"
            )),
            Err(e) => host_log::log(&format!("couldn't save the orphan-key record: {e}")),
        }
    }
    deleted
}

/// The ids of the keys in `store` whose office has nothing left in `home` (see the module
/// docs), or why it can't tell.
fn find_orphans(store: &dyn KeyStore, home: &Path) -> Result<Vec<String>, String> {
    let offices = home.join(OFFICES_DIR_NAME);
    // Everything readable, or no answer: a key is never taken for an orphan for a folder not
    // looked at.
    let mut claimed = folders_in(&offices, false).map_err(|e| e.to_string())?;
    claimed.extend(folders_in(&offices.join(REFUSED_DIR_NAME), true).map_err(|e| e.to_string())?);
    let folder_names: Vec<&String> = claimed.iter().map(|(name, _)| name).collect();
    let named: HashSet<&String> = claimed.iter().filter_map(|(_, id)| id.as_ref()).collect();
    let ids = store
        .list()
        .map_err(|e| format!("the key store can't list its keys: {e}"))?;
    let mut orphans = Vec::new();
    for id in ids {
        match fs::symlink_metadata(offices.join(&id)) {
            Ok(_) => continue,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => {
                host_log::log(&format!(
                    "kept the key of office {id}: its folder can't be checked: {e}"
                ));
                continue;
            }
        }
        if named.contains(&id) || folder_names.iter().any(|name| is_refused_as(name, &id)) {
            continue;
        }
        orphans.push(id);
    }
    Ok(orphans)
}

/// The orphan record at `path` (empty if there is none), or why it can't be used.
fn load_record(path: &Path) -> Result<OrphanRecord, String> {
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(OrphanRecord::default()),
        Err(e) => return Err(format!("it can't be opened: {e}")),
    };
    let meta = file
        .metadata()
        .map_err(|e| format!("it can't be read: {e}"))?;
    if !meta.is_file() || meta.uid() != crate::home::euid() {
        return Err("it isn't a regular file of this user".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("it can't be read: {e}"))?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err("it is too large".into());
    }
    // Where and what kind, never serde's message: it can quote the file's text.
    let stored: RecordFile = serde_json::from_slice(&bytes).map_err(|e| {
        format!(
            "it isn't valid ({:?} at line {}, column {})",
            e.classify(),
            e.line(),
            e.column()
        )
    })?;
    if stored.v != RECORD_VERSION {
        return Err(format!("its version {} isn't {RECORD_VERSION}", stored.v));
    }
    if !stored.first_seen_ms.keys().all(|id| is_plain_id(id)) {
        return Err("it names an office id that isn't plain".into());
    }
    Ok(OrphanRecord {
        first_seen_ms: stored.first_seen_ms,
    })
}

/// Writes `record` to `path` atomically: an owner-only temp file in `home`, fsynced, renamed
/// over `path`, then `home` fsynced. The outer error: not saved; the inner one: saved, but
/// `home` couldn't be synced.
fn save_record(home: &Path, path: &Path, record: &OrphanRecord) -> io::Result<io::Result<()>> {
    let bytes = serde_json::to_vec(&RecordFile {
        v: RECORD_VERSION,
        first_seen_ms: record.first_seen_ms.clone(),
    })
    .map_err(io::Error::other)?;
    let temp = home.join(format!("{ORPHAN_RECORD_FILE_NAME}.tmp"));
    match fs::remove_file(&temp) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let written = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&temp)
        .and_then(|mut file| file.write_all(&bytes).and_then(|()| file.sync_all()))
        .and_then(|()| fs::rename(&temp, path));
    if let Err(e) = written {
        let _ = fs::remove_file(&temp);
        return Err(e);
    }
    Ok(File::open(home).and_then(|dir| dir.sync_all()))
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
    match crate::catalog::read_office_file(file) {
        Ok(office) => Ok(office.map(|office| office.office_id)),
        // A plain file where a folder was expected has no office.json.
        Err(_) if file.parent().is_some_and(|folder| !folder.is_dir()) => Ok(None),
        Err(why) => Err(io::Error::other(format!("{}: {why}", file.display()))),
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
mod tests {
    use super::*;
    use crate::keys::memory::{Fault, MemoryKeys, Op};
    use org_proto::DbKey;
    use std::os::unix::fs::PermissionsExt;

    const ORPHAN: &str = "01ORPHANORPHANORPHANORPHAN";
    const STORED: &str = "01STOREDSTOREDSTOREDSTORED";
    const UNFINISHED: &str = "01UNFINISHEDUNFINISHEDUNFI";
    const REFUSED: &str = "01REFUSEDREFUSEDREFUSEDREF";
    const REFUSED_AGAIN: &str = "01AGAINAGAINAGAINAGAINAGAI";

    const DAY: Duration = Duration::from_secs(24 * 60 * 60);
    const DAY_MS: u64 = 24 * 60 * 60 * 1000;

    /// A fixed start time for the tests (2026-11-08, after [`CLOCK_FLOOR_MS`]).
    fn t0() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_794_096_000)
    }

    fn ms(at: SystemTime) -> u64 {
        u64::try_from(at.duration_since(UNIX_EPOCH).unwrap().as_millis()).unwrap()
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|id| (*id).to_owned()).collect()
    }

    fn record(entries: &[(&str, u64)]) -> OrphanRecord {
        OrphanRecord {
            first_seen_ms: entries
                .iter()
                .map(|(id, at)| ((*id).to_owned(), *at))
                .collect(),
        }
    }

    /// A home with a stored office, an unfinished one and two refused ones, and a store with a
    /// key for each and for one office that has no folder.
    fn setup(dir: &Path) -> MemoryKeys {
        let offices = dir.join(OFFICES_DIR_NAME);
        fs::create_dir_all(offices.join(STORED)).unwrap();
        write_office(&offices.join(STORED), STORED);
        fs::create_dir_all(offices.join(UNFINISHED)).unwrap();
        fs::create_dir_all(offices.join(REFUSED_DIR_NAME).join(REFUSED)).unwrap();
        fs::create_dir_all(
            offices
                .join(REFUSED_DIR_NAME)
                .join(format!("{REFUSED_AGAIN}-1762560000000")),
        )
        .unwrap();
        let keys = MemoryKeys::default();
        for id in [ORPHAN, STORED, UNFINISHED, REFUSED, REFUSED_AGAIN] {
            keys.create(id, &DbKey::from_bytes([1; 32])).unwrap();
        }
        keys
    }

    fn write_office(folder: &Path, office_id: &str) {
        let office = org_office_launch::OfficeFile::new(
            office_id,
            "HQ",
            org_proto::OfficeRole::Hq,
            "01ORG",
            "Acme",
            "2026-11-08T00:00:00.000Z",
        );
        fs::write(folder.join(OFFICE_FILE_NAME), office.to_vec().unwrap()).unwrap();
    }

    /// Sweeps at t0 (which must only mark) and again 7 days later: what that second sweep
    /// deleted.
    fn sweep_a_week_apart(store: &dyn KeyStore, home: &Path) -> Vec<String> {
        assert!(
            sweep_orphans(store, home, t0()).is_empty(),
            "deleted on first sight"
        );
        sweep_orphans(store, home, t0() + 7 * DAY)
    }

    // The policy.

    #[test]
    fn first_sight_marks_and_deletes_nothing() {
        let now = ms(t0());
        let (to_delete, new) = decide(&ids(&[ORPHAN]), &OrphanRecord::default(), now);
        assert!(to_delete.is_empty());
        assert_eq!(new, record(&[(ORPHAN, now)]));
    }

    #[test]
    fn a_key_is_deleted_only_once_it_has_been_marked_7_days() {
        let first = ms(t0());
        let marked = record(&[(ORPHAN, first)]);
        for (after, deleted) in [
            (0, false),
            (6 * DAY_MS, false),
            (7 * DAY_MS - 1, false),
            (7 * DAY_MS, true),
            (30 * DAY_MS, true),
        ] {
            let (to_delete, new) = decide(&ids(&[ORPHAN]), &marked, first + after);
            assert_eq!(to_delete == ids(&[ORPHAN]), deleted, "after {after} ms");
            // The first-seen time is kept, never moved on.
            assert_eq!(new, marked, "after {after} ms");
        }
    }

    #[test]
    fn a_key_no_longer_orphaned_is_unmarked() {
        let first = ms(t0());
        let marked = record(&[(ORPHAN, first), (STORED, first)]);
        // STORED's folder came back.
        let (to_delete, new) = decide(&ids(&[ORPHAN]), &marked, first + 8 * DAY_MS);
        assert_eq!(to_delete, ids(&[ORPHAN]));
        assert_eq!(new, record(&[(ORPHAN, first)]));
        // If it goes again, its 7 days start over.
        let later = first + 9 * DAY_MS;
        let (to_delete, new) = decide(&ids(&[STORED]), &new, later);
        assert!(to_delete.is_empty());
        assert_eq!(new, record(&[(STORED, later)]));
    }

    #[test]
    fn a_first_seen_time_in_the_future_isnt_7_days_old() {
        let now = ms(t0());
        let future = record(&[(ORPHAN, now + 30 * DAY_MS)]);
        let (to_delete, new) = decide(&ids(&[ORPHAN]), &future, now);
        assert!(to_delete.is_empty());
        // Marked again from now.
        assert_eq!(new, record(&[(ORPHAN, now)]));
    }

    #[test]
    fn a_clock_before_the_floor_sweeps_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let keys = setup(dir.path());
        // A dead clock (2001), or one before 1970: nothing is marked at the wrong time, so the
        // corrected start minutes later only marks.
        let wrong = UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        assert!(sweep_orphans(&keys, dir.path(), wrong).is_empty());
        assert!(sweep_orphans(&keys, dir.path(), UNIX_EPOCH - DAY).is_empty());
        assert!(!dir.path().join(ORPHAN_RECORD_FILE_NAME).exists());
        assert!(sweep_orphans(&keys, dir.path(), t0()).is_empty());
        assert!(keys.has(ORPHAN));
        assert!(ms(t0()) > CLOCK_FLOOR_MS);
    }

    #[test]
    fn a_record_that_is_a_fifo_or_a_folder_is_refused_without_waiting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(ORPHAN_RECORD_FILE_NAME);
        fs::create_dir(&path).unwrap();
        assert!(load_record(&path).is_err());
        fs::remove_dir(&path).unwrap();
        let status = std::process::Command::new("/usr/bin/mkfifo")
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(load_record(&path).is_err());
    }

    #[test]
    fn a_records_error_never_quotes_its_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(ORPHAN_RECORD_FILE_NAME);
        fs::write(
            &path,
            b"{\"v\":1,\"first_seen_ms\":{\"A\":\"secret-looking text\"}}",
        )
        .unwrap();
        let why = load_record(&path).unwrap_err();
        assert!(!why.contains("secret-looking"), "{why}");
    }

    // The record.

    #[test]
    fn the_record_is_owner_only_and_holds_ids_and_times_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(ORPHAN_RECORD_FILE_NAME);
        assert_eq!(load_record(&path).unwrap(), OrphanRecord::default());
        let marked = record(&[(ORPHAN, 1_762_560_000_000), (STORED, 7)]);
        save_record(dir.path(), &path, &marked).unwrap().unwrap();
        assert_eq!(load_record(&path).unwrap(), marked);
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(
            text,
            format!("{{\"v\":1,\"first_seen_ms\":{{\"{ORPHAN}\":1762560000000,\"{STORED}\":7}}}}")
        );
        let meta = fs::metadata(&path).unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        // No temp file left behind.
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, [std::ffi::OsString::from(ORPHAN_RECORD_FILE_NAME)]);
    }

    #[test]
    fn a_record_that_cant_be_used_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(ORPHAN_RECORD_FILE_NAME);
        for bad in [
            "".to_owned(),
            "{not json".to_owned(),
            "{\"v\":2,\"first_seen_ms\":{}}".to_owned(),
            "{\"v\":1}".to_owned(),
            "{\"v\":1,\"first_seen_ms\":{\"../x\":1}}".to_owned(),
            "{\"v\":1,\"first_seen_ms\":{\"A\":-1}}".to_owned(),
            "{\"v\":1,\"first_seen_ms\":{},\"more\":1}".to_owned(),
            format!(
                "{{\"v\":1,\"first_seen_ms\":{{}}}}{}",
                " ".repeat(1024 * 1024)
            ),
        ] {
            fs::write(&path, &bad).unwrap();
            assert!(load_record(&path).is_err(), "{}", &bad[..bad.len().min(60)]);
        }
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(dir.path().join("elsewhere"), &path).unwrap();
        assert!(load_record(&path).is_err());
    }

    // End to end.

    #[test]
    fn a_key_orphaned_for_7_days_is_deleted_and_kept_until_then() {
        let dir = tempfile::tempdir().unwrap();
        let keys = setup(dir.path());
        assert!(sweep_orphans(&keys, dir.path(), t0()).is_empty());
        assert!(keys.has(ORPHAN));
        let path = dir.path().join(ORPHAN_RECORD_FILE_NAME);
        assert_eq!(load_record(&path).unwrap(), record(&[(ORPHAN, ms(t0()))]));
        // Six days on: still only marked (and marked from the first sight, not again).
        assert!(sweep_orphans(&keys, dir.path(), t0() + 6 * DAY).is_empty());
        assert!(keys.has(ORPHAN));
        assert_eq!(load_record(&path).unwrap(), record(&[(ORPHAN, ms(t0()))]));
        // Seven days on: deleted, and out of the record.
        assert_eq!(sweep_orphans(&keys, dir.path(), t0() + 7 * DAY), [ORPHAN]);
        assert_eq!(
            keys.list().unwrap(),
            [REFUSED_AGAIN, REFUSED, STORED, UNFINISHED]
        );
        assert_eq!(load_record(&path).unwrap(), OrphanRecord::default());
        assert!(sweep_orphans(&keys, dir.path(), t0() + 8 * DAY).is_empty());
    }

    #[test]
    fn a_key_whose_folder_came_back_is_unmarked_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        let keys = setup(dir.path());
        assert!(sweep_orphans(&keys, dir.path(), t0()).is_empty());
        fs::create_dir_all(dir.path().join(OFFICES_DIR_NAME).join(ORPHAN)).unwrap();
        assert!(sweep_orphans(&keys, dir.path(), t0() + 8 * DAY).is_empty());
        assert!(keys.has(ORPHAN));
        let path = dir.path().join(ORPHAN_RECORD_FILE_NAME);
        assert_eq!(load_record(&path).unwrap(), OrphanRecord::default());
    }

    #[test]
    fn a_bad_record_deletes_nothing_and_starts_over() {
        let dir = tempfile::tempdir().unwrap();
        let keys = setup(dir.path());
        let path = dir.path().join(ORPHAN_RECORD_FILE_NAME);
        // Marked long ago, then the record was damaged.
        fs::write(
            &path,
            b"{\"v\":1,\"first_seen_ms\":{\"01ORPHANORPHANORPHANORPHAN\":",
        )
        .unwrap();
        assert!(sweep_orphans(&keys, dir.path(), t0()).is_empty());
        assert!(keys.has(ORPHAN));
        // A fresh record, marked from now.
        assert_eq!(load_record(&path).unwrap(), record(&[(ORPHAN, ms(t0()))]));
        // One that marks the key in the future doesn't count either.
        save_record(
            dir.path(),
            &path,
            &record(&[(ORPHAN, ms(t0()) + 30 * DAY_MS)]),
        )
        .unwrap()
        .unwrap();
        assert!(sweep_orphans(&keys, dir.path(), t0() + DAY).is_empty());
        assert!(keys.has(ORPHAN));
    }

    #[test]
    fn a_key_that_cant_be_deleted_stays_marked() {
        let dir = tempfile::tempdir().unwrap();
        let keys = setup(dir.path());
        assert!(sweep_orphans(&keys, dir.path(), t0()).is_empty());
        keys.fail(Op::Delete, Fault::Os(-25308));
        assert!(sweep_orphans(&keys, dir.path(), t0() + 7 * DAY).is_empty());
        let path = dir.path().join(ORPHAN_RECORD_FILE_NAME);
        assert_eq!(load_record(&path).unwrap(), record(&[(ORPHAN, ms(t0()))]));
        keys.clear_fault(Op::Delete);
        assert_eq!(sweep_orphans(&keys, dir.path(), t0() + 8 * DAY), [ORPHAN]);
    }

    #[test]
    fn an_office_json_under_another_folder_name_keeps_its_key() {
        // Office RENAMED's folder was renamed by hand: at startup it's moved aside as
        // `.refused/<folder>` (or left in offices/ if the move failed), with its data.
        const RENAMED: &str = "01RENAMEDRENAMEDRENAMEDREN";
        const MISPLACED: &str = "01MISPLACEDMISPLACEDMISPLA";
        let dir = tempfile::tempdir().unwrap();
        let keys = setup(dir.path());
        let offices = dir.path().join(OFFICES_DIR_NAME);
        let moved = offices.join(REFUSED_DIR_NAME).join("Elsewhere");
        fs::create_dir_all(&moved).unwrap();
        write_office(&moved, RENAMED);
        let stayed = offices.join("Somewhere");
        fs::create_dir_all(&stayed).unwrap();
        write_office(&stayed, MISPLACED);
        for id in [RENAMED, MISPLACED] {
            keys.create(id, &DbKey::from_bytes([1; 32])).unwrap();
        }
        assert_eq!(sweep_a_week_apart(&keys, dir.path()), [ORPHAN]);
        assert!(keys.has(RENAMED) && keys.has(MISPLACED));

        // An office.json there that can't be read: nothing is swept, or marked.
        keys.create(ORPHAN, &DbKey::from_bytes([1; 32])).unwrap();
        fs::write(moved.join(OFFICE_FILE_NAME), b"{not an office").unwrap();
        assert!(sweep_orphans(&keys, dir.path(), t0() + 30 * DAY).is_empty());
        assert!(keys.has(ORPHAN));
    }

    #[test]
    fn a_refused_name_must_be_the_id_or_the_id_with_a_timestamp() {
        assert!(is_refused_as(REFUSED, REFUSED));
        assert!(is_refused_as(&format!("{REFUSED}-123"), REFUSED));
        for other in [
            format!("{REFUSED}-"),
            format!("{REFUSED}-x"),
            format!("{REFUSED}X"),
            format!("{REFUSED}-1-2"),
            REFUSED[1..].to_owned(),
        ] {
            assert!(!is_refused_as(&other, REFUSED), "{other}");
        }
    }

    #[test]
    fn nothing_is_deleted_or_marked_when_anything_cant_be_read() {
        let week_apart = |keys: &MemoryKeys, home: &Path| {
            let first = sweep_orphans(keys, home, t0());
            let second = sweep_orphans(keys, home, t0() + 8 * DAY);
            first.is_empty() && second.is_empty()
        };
        // No offices/ at all.
        let dir = tempfile::tempdir().unwrap();
        let keys = MemoryKeys::default();
        keys.create(ORPHAN, &DbKey::from_bytes([1; 32])).unwrap();
        assert!(week_apart(&keys, dir.path()));
        assert!(keys.has(ORPHAN));

        // A store that can't list.
        let keys = setup(dir.path());
        keys.fail(Op::List, Fault::Os(-25308));
        assert!(week_apart(&keys, dir.path()));
        assert!(keys.has(ORPHAN));
        keys.clear_fault(Op::List);

        // An office.json that's a FIFO: refused at once, never waited on.
        let fifo_folder = dir.path().join(OFFICES_DIR_NAME).join("Fifo");
        fs::create_dir_all(&fifo_folder).unwrap();
        let status = std::process::Command::new("/usr/bin/mkfifo")
            .arg(fifo_folder.join(OFFICE_FILE_NAME))
            .status()
            .unwrap();
        assert!(status.success());
        assert!(week_apart(&keys, dir.path()));
        assert!(keys.has(ORPHAN));
        fs::remove_dir_all(&fifo_folder).unwrap();
        // Nothing was marked meanwhile.
        assert!(!dir.path().join(ORPHAN_RECORD_FILE_NAME).exists());

        // A .refused/ that can't be read (permissions don't stop root, so not as root).
        if crate::home::euid() == 0 {
            return;
        }
        let refused = dir.path().join(OFFICES_DIR_NAME).join(REFUSED_DIR_NAME);
        fs::set_permissions(&refused, fs::Permissions::from_mode(0o000)).unwrap();
        let swept = week_apart(&keys, dir.path());
        fs::set_permissions(&refused, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(swept);
        assert!(keys.has(ORPHAN));
        assert!(!keys.calls().iter().any(|(op, _)| *op == Op::Delete));
    }

    #[cfg(debug_assertions)]
    #[test]
    fn the_development_store_is_swept_the_same_way() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("h");
        let memory = setup(&home);
        let store = crate::keys::dev::DevKeys::new(dir.path().join("k"));
        for id in memory.list().unwrap() {
            store.create(&id, &DbKey::from_bytes([2; 32])).unwrap();
        }
        assert_eq!(sweep_a_week_apart(&store, &home), [ORPHAN]);
        assert_eq!(
            store.list().unwrap(),
            [REFUSED_AGAIN, REFUSED, STORED, UNFINISHED]
        );
    }
}
