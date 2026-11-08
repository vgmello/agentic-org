//! The orphan-key sweep: deleting the keys of offices that are gone.
//!
//! A key whose office has nothing left in this home can never open anything: its databases are
//! gone, or never existed (a create whose `--create` and `--discard` servers couldn't even
//! start). An office has something left when a folder is named after it in `offices/` or
//! `offices/.refused/` (`<id>` or `<id>-<unix ms>`), or when an `office.json` in any folder
//! there names it (a folder renamed by hand, moved aside at startup with its data). The host
//! deletes the other keys once at startup, after the startup discards and before any create can
//! store a key for a folder not made yet.
//!
//! This is the one exception to "a key is deleted only after `discarded`", and it is safe only
//! when the key store belongs to this office home alone: `Host::start` decides whether it runs
//! (`may_sweep_orphan_keys`; today it never does).
//!
//! It fails safe: if `offices/` or `offices/.refused/` can't be read, an `office.json` in them
//! can't be read, or the store can't list, nothing is deleted; a folder that can't be checked
//! keeps its key.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;

use org_office_launch::{OFFICES_DIR_NAME, OFFICE_FILE_NAME, REFUSED_DIR_NAME};

use super::KeyStore;
use crate::host_log;

/// Deletes from `store` the keys of offices with no folder in `home` (see the module docs), and
/// returns their ids.
pub(crate) fn sweep_orphans(store: &dyn KeyStore, home: &Path) -> Vec<String> {
    let offices = home.join(OFFICES_DIR_NAME);
    // Everything readable, or nothing is swept: a key is never deleted for a folder not looked
    // at.
    let claimed = match folders_in(&offices, false).and_then(|mut claimed| {
        claimed.extend(folders_in(&offices.join(REFUSED_DIR_NAME), true)?);
        Ok(claimed)
    }) {
        Ok(claimed) => claimed,
        Err(e) => {
            host_log::log(&format!("no orphan-key sweep: {e}"));
            return Vec::new();
        }
    };
    let folder_names: Vec<&String> = claimed.iter().map(|(name, _)| name).collect();
    let named: HashSet<&String> = claimed.iter().filter_map(|(_, id)| id.as_ref()).collect();
    let ids = match store.list() {
        Ok(ids) => ids,
        Err(e) => {
            host_log::log(&format!(
                "no orphan-key sweep: the key store can't list its keys: {e}"
            ));
            return Vec::new();
        }
    };
    let mut deleted = Vec::new();
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
        match store.delete(&id) {
            Ok(()) => {
                host_log::log(&format!(
                    "deleted the key of office {id}: it has no folder in offices/"
                ));
                deleted.push(id);
            }
            Err(e) => host_log::log(&format!("couldn't delete the orphan key of {id}: {e}")),
        }
    }
    deleted
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
        assert_eq!(sweep_orphans(&keys, dir.path()), [ORPHAN]);
        assert!(keys.has(RENAMED) && keys.has(MISPLACED));

        // An office.json there that can't be read: nothing is swept.
        keys.create(ORPHAN, &DbKey::from_bytes([1; 32])).unwrap();
        fs::write(moved.join(OFFICE_FILE_NAME), b"{not an office").unwrap();
        assert!(sweep_orphans(&keys, dir.path()).is_empty());
        assert!(keys.has(ORPHAN));
    }

    #[test]
    fn only_the_keys_of_offices_with_no_folder_anywhere_are_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let keys = setup(dir.path());
        assert_eq!(sweep_orphans(&keys, dir.path()), [ORPHAN]);
        assert_eq!(
            keys.list().unwrap(),
            [REFUSED_AGAIN, REFUSED, STORED, UNFINISHED]
        );
        // Nothing more on a second run.
        assert!(sweep_orphans(&keys, dir.path()).is_empty());
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
    fn nothing_is_deleted_when_anything_cant_be_read() {
        // No offices/ at all.
        let dir = tempfile::tempdir().unwrap();
        let keys = MemoryKeys::default();
        keys.create(ORPHAN, &DbKey::from_bytes([1; 32])).unwrap();
        assert!(sweep_orphans(&keys, dir.path()).is_empty());
        assert!(keys.has(ORPHAN));

        // A store that can't list.
        let keys = setup(dir.path());
        keys.fail(Op::List, Fault::Os(-25308));
        assert!(sweep_orphans(&keys, dir.path()).is_empty());
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
        assert!(sweep_orphans(&keys, dir.path()).is_empty());
        assert!(keys.has(ORPHAN));
        fs::remove_dir_all(&fifo_folder).unwrap();

        // A .refused/ that can't be read (permissions don't stop root, so not as root).
        if crate::home::euid() == 0 {
            return;
        }
        let refused = dir.path().join(OFFICES_DIR_NAME).join(REFUSED_DIR_NAME);
        fs::set_permissions(&refused, fs::Permissions::from_mode(0o000)).unwrap();
        let swept = sweep_orphans(&keys, dir.path());
        fs::set_permissions(&refused, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(swept.is_empty());
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
        assert_eq!(sweep_orphans(&store, &home), [ORPHAN]);
        assert_eq!(
            store.list().unwrap(),
            [REFUSED_AGAIN, REFUSED, STORED, UNFINISHED]
        );
    }
}
