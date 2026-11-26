//! Every office-private tree is masked from agent sandboxes (`TASK-203` P0-1, P0-2): each
//! companion store `org-office` keeps beside its journal, the durable `blobs/` and
//! `checkpoints/`, a restore's backups beside the data roots, and anything unexpected beside
//! `agents/` or the roots (a legacy `durable.before-restore-*`), in both the deny globs every
//! runtime gets and the folders a Linux `bwrap` sandbox pre-creates.

use std::path::{Path, PathBuf};

use org_office::companion_paths::{companion_store_paths, CompanionKind, AGENTS_DIR_NAME};

use super::*;

fn manager(office: &Arc<Office>) -> SessionManager {
    SessionManager::new(
        Arc::clone(office),
        SessionManagerConfig::production(office.durable_root().join("sock")),
    )
}

fn denies(mgr: &SessionManager, me: &str) -> Vec<String> {
    mgr.denied_office_globs_for(Some(me))
        .into_iter()
        .map(|g| g.as_str().to_owned())
        .collect()
}

/// The folder whose `/**` glob masks `path` (itself or an ancestor below `durable`), if any.
fn masking_folder(denied: &[String], durable: &Path, path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .take_while(|a| *a != durable && a.starts_with(durable))
        .find(|a| denied.contains(&format!("{}/**", a.display())))
        .map(Path::to_path_buf)
}

fn first_employee(office: &Office) -> String {
    office.list_employees().unwrap()[0].id.as_str().to_owned()
}

#[test]
fn every_companion_store_and_durable_family_is_denied_and_every_folder_is_precreated() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let mgr = manager(&office);
    let me = first_employee(&office);
    let durable = office.durable_root().canonicalize().unwrap();
    let office_id = office.info().office_id.clone();
    let office_local = durable.join("local/offices").join(&office_id);
    let stores = companion_store_paths(&office_id);
    assert!(stores.len() >= 20, "{stores:?}");

    let denied = denies(&mgr, &me);
    for store in &stores {
        let path = durable.join(&store.path);
        if store.path.ends_with(&format!("/{AGENTS_DIR_NAME}")) {
            // Only the other agents' folders are masked, never the folder that holds them.
            assert!(
                !denied.contains(&format!("{}/**", path.display())),
                "{denied:?}"
            );
            continue;
        }
        let covered = masking_folder(&denied, &durable, &path).is_some()
            || (store.kind == CompanionKind::File && denied.contains(&path.display().to_string()));
        assert!(covered, "{} is not masked: {denied:?}", store.path);
    }
    for family in ["blobs", "checkpoints", "private", "state", "recovery"] {
        let glob = format!("{}/**", durable.join(family).display());
        assert!(denied.contains(&glob), "{glob} missing: {denied:?}");
    }
    // The durable root and the runtime root are never masked whole: the agent's own home and
    // worktrees live below them.
    for root in [&durable, &office.runtime_root().canonicalize().unwrap()] {
        assert!(
            !denied.contains(&format!("{}/**", root.display())),
            "{denied:?}"
        );
    }

    crate::manager::ensure_bwrap_deny_dirs(&mgr, &me).unwrap();
    let denied = denies(&mgr, &me);
    for store in &stores {
        let path = durable.join(&store.path);
        match store.kind {
            CompanionKind::Folder => {
                let folder = masking_folder(&denied, &durable, &path)
                    .or_else(|| store.path.ends_with(AGENTS_DIR_NAME).then(|| path.clone()))
                    .unwrap_or_else(|| panic!("{} is not masked", store.path));
                assert!(folder.is_dir(), "{} is not pre-created", folder.display());
                let mode = std::fs::metadata(&folder).unwrap().permissions().mode() & 0o777;
                assert_eq!(mode, 0o700, "{}", folder.display());
            }
            // A one-file store is masked where it exists; a folder is never made in its place.
            CompanionKind::File => assert!(!path.is_dir(), "{} made a folder", store.path),
        }
    }
    for family in ["blobs", "checkpoints"] {
        assert!(durable.join(family).is_dir(), "{family} not pre-created");
    }
    assert!(!office_local.join("hq-owner.v1.json").exists());
}

#[test]
fn unexpected_entries_beside_agents_are_masked_and_a_one_file_store_is_masked_exactly() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let mgr = manager(&office);
    let me = first_employee(&office);
    let durable = office.durable_root().canonicalize().unwrap();
    let office_local = durable.join("local/offices").join(&office.info().office_id);
    let mystery = office_local.join("mystery-store");
    std::fs::create_dir_all(&mystery).unwrap();
    std::fs::write(mystery.join("secret"), b"token").unwrap();
    let stray = office_local.join("stray.v1.json");
    std::fs::write(&stray, b"{}").unwrap();
    let owner_file = office_local.join("hq-owner.v1.json");
    std::fs::write(&owner_file, b"{}").unwrap();
    // A link is masked as the link itself, never as what it points to.
    let link = office_local.join("link-to-root");
    std::os::unix::fs::symlink("/", &link).unwrap();

    let denied = denies(&mgr, &me);
    for needle in [
        format!("{}/**", mystery.display()),
        mystery.display().to_string(),
        stray.display().to_string(),
        owner_file.display().to_string(),
        link.display().to_string(),
    ] {
        assert!(denied.contains(&needle), "{needle} missing: {denied:?}");
    }
    assert!(!denied.iter().any(|d| d == "/**" || d == "/"), "{denied:?}");
    let own = office_local.join("agents").join(&me);
    assert!(
        !denied
            .iter()
            .any(|d| d.starts_with(&own.display().to_string())),
        "{denied:?}"
    );
}

#[test]
fn restore_backups_and_legacy_restore_leftovers_beside_the_roots_are_masked() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let mgr = manager(&office);
    let me = first_employee(&office);
    let durable = office.durable_root().canonicalize().unwrap();
    let base = durable.parent().unwrap().to_path_buf();
    // What a restore before `TASK-203` left beside the roots: a replaced root and a staging
    // folder, each a full copy of the office's secrets.
    let legacy = base.join("durable.before-restore-0011223344556677");
    std::fs::create_dir_all(legacy.join("private")).unwrap();
    std::fs::write(legacy.join("private/task-notes.db"), b"notes").unwrap();
    let staging = base.join(".restore-8899aabbccddeeff");
    std::fs::create_dir_all(staging.join("durable/auth")).unwrap();

    let denied = denies(&mgr, &me);
    let backups = base.join(org_office::backup::RESTORE_BACKUPS_DIR_NAME);
    for needle in [
        format!("{}/**", backups.display()),
        format!("{}/**", legacy.display()),
        legacy.display().to_string(),
        format!("{}/**", staging.display()),
    ] {
        assert!(denied.contains(&needle), "{needle} missing: {denied:?}");
    }
    assert!(!backups.exists(), "denying never makes the folder");

    crate::manager::ensure_bwrap_deny_dirs(&mgr, &me).unwrap();
    assert!(
        backups.is_dir(),
        "bwrap pre-creates the restore backups folder"
    );
    let mode = std::fs::metadata(&backups).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
    let denied = denies(&mgr, &me);
    assert!(denied.contains(&format!("{}/**", backups.display())));
    assert!(denied.contains(&format!("{}/**", legacy.display())));
}

#[test]
fn a_satellite_sandbox_precreates_companion_folders_but_never_private() {
    let sat = org_office::TestSatellite::new();
    let office = Arc::new(sat.office);
    let mgr = manager(&office);
    let me = new_uuid_v7();
    crate::manager::ensure_bwrap_deny_dirs(&mgr, &me).unwrap();
    let durable = office.durable_root();
    let office_local = durable.join("local/offices").join(&office.info().office_id);
    assert!(
        !durable.join("private").exists(),
        "ARCH-02: no private on a satellite"
    );
    for folder in [
        durable.join("blobs"),
        office_local.join("tls"),
        office_local.join("replication"),
    ] {
        assert!(folder.is_dir(), "{}", folder.display());
    }
    let denied = denies(&mgr, &me);
    let private = durable.canonicalize().unwrap().join("private");
    assert!(
        denied.contains(&format!("{}/**", private.display())),
        "{denied:?}"
    );
}
