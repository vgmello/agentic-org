//! Every office-private tree is masked from agent sandboxes (`TASK-203` P0-1, P0-2): each
//! companion store `org-office` keeps beside its journal, the durable `blobs/` and
//! `checkpoints/`, a restore's backups beside the data roots, and anything unexpected beside
//! `agents/` or the roots (a legacy `durable.before-restore-*`), in both the deny globs every
//! runtime gets and the folders a Linux `bwrap` sandbox pre-creates. So is every neighbour on a
//! host serving several organizations and offices (`TASK-204`): every other organization,
//! every other office of the organization (co-hosted satellites, another office's folders in
//! the same roots) and every office folder in the office home, with the masks reaching every
//! runtime and the agent's own trees left alone.

use std::path::{Path, PathBuf};

use org_agent_runtime::ToolCapability;
use org_office::companion_paths::{companion_store_paths, CompanionKind, AGENTS_DIR_NAME};

use super::*;
use crate::instructions::StandingInstructions;
use crate::manager::{build_session_spec, ModelChoice};
use crate::preset::decide_escalation_with_roots;

/// Another organization on the test office's host, and one of its offices.
const ORG_B: &str = "0195a000-0000-7000-8000-00000000b001";
const OFFICE_B: &str = "0195a000-0000-7000-8000-00000000b0b0";

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

/// Whether a deny pattern masks `path`: a folder glob holding it, or `path` exactly.
fn is_masked(denied: &[String], path: &Path) -> bool {
    denied.iter().any(|d| match d.strip_suffix("/**") {
        Some(base) => path.starts_with(base),
        None => path == Path::new(d),
    })
}

/// No deny pattern names `path` or a folder holding it.
fn assert_reachable(denied: &[String], path: &Path) {
    for d in denied {
        let base = d.strip_suffix("/**").unwrap_or(d);
        assert!(
            !path.starts_with(base),
            "{} is masked by {d}",
            path.display()
        );
    }
}

fn assert_masked_whole(denied: &[String], folder: &Path) {
    for needle in [
        folder.display().to_string(),
        format!("{}/**", folder.display()),
    ] {
        assert!(denied.contains(&needle), "{needle} missing: {denied:?}");
    }
}

fn write_secret(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b"secret").unwrap();
}

/// The host's data root the test office sits in, where the host puts an HQ:
/// `<data-root>/<org-id>/{durable,runtime}`.
fn data_root(office: &Office) -> PathBuf {
    let durable = office.durable_root().canonicalize().unwrap();
    let org_root = durable.parent().unwrap();
    assert_eq!(
        org_root.file_name().and_then(|n| n.to_str()),
        Some(office.info().org_id.as_str())
    );
    org_root.parent().unwrap().to_path_buf()
}

/// The agent's own trees: its private home and its worktree root (made here).
fn own_trees(mgr: &SessionManager, office: &Office, me: &str) -> Vec<PathBuf> {
    let (writable, _) = mgr.worktree_sandbox_grants(me, true);
    let worktree = PathBuf::from(writable.first().expect("a worktree root").as_str());
    let home = office
        .durable_root()
        .canonicalize()
        .unwrap()
        .join("local/offices")
        .join(&office.info().office_id)
        .join("agents")
        .join(me)
        .join("home");
    vec![home, worktree]
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
    // A link pointing to `/` (which overlaps own_roots) is omitted so sandboxes that dereference
    // deny entries never wipe the session; a link pointing outside stays masked as the link.
    let link_root = office_local.join("link-to-root");
    std::os::unix::fs::symlink("/", &link_root).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let link_outside = office_local.join("link-to-outside");
    std::os::unix::fs::symlink(outside.path(), &link_outside).unwrap();

    let denied = denies(&mgr, &me);
    for needle in [
        format!("{}/**", mystery.display()),
        mystery.display().to_string(),
        stray.display().to_string(),
        owner_file.display().to_string(),
        link_outside.display().to_string(),
    ] {
        assert!(denied.contains(&needle), "{needle} missing: {denied:?}");
    }
    assert!(
        !denied.contains(&link_root.display().to_string()),
        "link-to-root must be omitted: {denied:?}"
    );
    assert!(
        !denied.contains(&format!("{}/**", link_outside.display())),
        "{denied:?}"
    );
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

#[test]
fn attachment_staging_of_this_office_and_of_every_sibling_office_is_masked() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let mgr = manager(&office);
    let me = first_employee(&office);
    let durable = office.durable_root().canonicalize().unwrap();
    let offices = durable.join("local/offices");
    let own = offices
        .join(&office.info().office_id)
        .join(org_office::companion_paths::ATTACHMENT_STAGING_DIR_NAME);
    assert!(
        companion_store_paths(&office.info().office_id)
            .iter()
            .any(|store| store.path.ends_with("/attachment-staging")
                && store.kind == CompanionKind::Folder),
        "the staging area is a companion store"
    );
    // Another office's uploads under the same durable root, with bytes in them.
    let sibling = offices.join("0195a000-0000-7000-8000-0000000000aa");
    std::fs::create_dir_all(
        sibling.join("attachment-staging/0195a000-0000-7000-8000-0000000000ab"),
    )
    .unwrap();
    std::fs::write(
        sibling.join("attachment-staging/0195a000-0000-7000-8000-0000000000ab/data"),
        b"someone else's upload",
    )
    .unwrap();
    let empty_sibling = offices.join("0195a000-0000-7000-8000-0000000000bb");
    std::fs::create_dir_all(&empty_sibling).unwrap();

    let denied = denies(&mgr, &me);
    let glob = format!("{}/**", own.display());
    assert!(denied.contains(&glob), "{glob} missing: {denied:?}");
    // Another office's folder is masked whole (`TASK-204`), its staging area with it.
    for (office_folder, staging) in [
        (&sibling, sibling.join("attachment-staging")),
        (&empty_sibling, empty_sibling.join("attachment-staging")),
    ] {
        assert_masked_whole(&denied, office_folder);
        assert_eq!(
            masking_folder(&denied, &durable, &staging).as_deref(),
            Some(office_folder.as_path())
        );
    }
    assert!(!own.exists(), "denying never makes the folder");

    crate::manager::ensure_bwrap_deny_dirs(&mgr, &me).unwrap();
    assert!(
        own.is_dir(),
        "bwrap pre-creates the office's own staging area"
    );
    let mode = std::fs::metadata(&own).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
    // A sibling office's folder is masked, never made.
    assert!(!empty_sibling.join("attachment-staging").exists());
    let denied = denies(&mgr, &me);
    assert!(denied.contains(&format!("{}/**", own.display())));
    assert_masked_whole(&denied, &sibling);
}

#[test]
fn the_blob_outbox_of_this_and_every_sibling_office_and_the_blob_cache_are_masked() {
    use org_office::companion_paths::BLOB_OUTBOX_DIR_NAME;
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let mgr = manager(&office);
    let me = first_employee(&office);
    let durable = office.durable_root().canonicalize().unwrap();
    let offices = durable.join("local/offices");
    let office_id = office.info().office_id.clone();
    let own = offices.join(&office_id).join(BLOB_OUTBOX_DIR_NAME);
    assert!(
        companion_store_paths(&office_id).iter().any(
            |store| store.path.ends_with("/blob-outbox") && store.kind == CompanionKind::Folder
        ),
        "the blob outbox is a companion store"
    );
    // Another office's un-shipped files under the same durable root, with bytes in them.
    let sibling = offices.join("0195a000-0000-7000-8000-0000000000ca");
    std::fs::create_dir_all(sibling.join("blob-outbox/entries")).unwrap();
    std::fs::write(
        sibling.join("blob-outbox/entries/0195a000-0000-7000-8000-0000000000cb.yaml"),
        b"someone else's file record",
    )
    .unwrap();
    let empty_sibling = offices.join("0195a000-0000-7000-8000-0000000000cc");
    std::fs::create_dir_all(&empty_sibling).unwrap();

    let denied = denies(&mgr, &me);
    let glob = format!("{}/**", own.display());
    assert!(denied.contains(&glob), "{glob} missing: {denied:?}");
    // Another office's folder is masked whole (`TASK-204`), its outbox with it.
    for office_folder in [&sibling, &empty_sibling] {
        assert_masked_whole(&denied, office_folder);
        let outbox = office_folder.join(BLOB_OUTBOX_DIR_NAME);
        assert_eq!(
            masking_folder(&denied, &durable, &outbox).as_deref(),
            Some(office_folder.as_path())
        );
    }
    assert!(!own.exists(), "denying never makes the folder");

    crate::manager::ensure_bwrap_deny_dirs(&mgr, &me).unwrap();
    assert!(
        own.is_dir(),
        "bwrap pre-creates the office's own blob outbox"
    );
    let mode = std::fs::metadata(&own).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
    // A sibling office's folder is masked, never made.
    assert!(!empty_sibling.join(BLOB_OUTBOX_DIR_NAME).exists());
    let denied = denies(&mgr, &me);
    assert!(denied.contains(&format!("{}/**", own.display())));
    assert_masked_whole(&denied, &sibling);

    // The satellite's blob cache lives under the runtime's `protected/`, masked whole and
    // made owner-only before launch.
    let runtime = office.runtime_root().canonicalize().unwrap();
    let protected = runtime.join(&office_id).join("protected");
    let cache = runtime
        .join(&office_id)
        .join(org_office::attachments::cache::BLOB_CACHE_DIR);
    assert!(cache.starts_with(&protected));
    assert!(
        denied.contains(&format!("{}/**", protected.display())),
        "{denied:?}"
    );
    assert!(protected.is_dir());
    let mode = std::fs::metadata(&protected).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
}

#[test]
fn every_other_organization_on_the_host_is_masked_whole_and_the_host_and_own_trees_stay_open() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let mgr = manager(&office);
    let me = first_employee(&office);
    let data = data_root(&office);
    let org_b = data.join(ORG_B);
    let secrets = [
        org_b.join("durable/private/task-notes.db"),
        org_b
            .join("durable/local/offices")
            .join(OFFICE_B)
            .join("auth/token"),
        org_b
            .join("runtime")
            .join(OFFICE_B)
            .join("protected/blob-cache/x"),
        org_b.join("offices/0195a000-0000-7000-8000-00000000b0b1/durable/blobs/x"),
    ];
    for secret in &secrets {
        write_secret(secret);
    }
    // The host's own folders beside the organizations: the office home holds the office
    // sockets and every agent's temporary folder.
    let host_folders = [
        data.join("office-state/run/o"),
        data.join("office-state/run/t"),
        data.join("agents"),
    ];
    for folder in &host_folders {
        std::fs::create_dir_all(folder).unwrap();
    }

    let denied = denies(&mgr, &me);
    assert_masked_whole(&denied, &org_b);
    for secret in &secrets {
        assert!(is_masked(&denied, secret), "{}", secret.display());
    }
    let own_org = data.join(&office.info().org_id);
    for path in host_folders
        .iter()
        .chain(own_trees(&mgr, &office, &me).iter())
        .chain([&own_org, &data])
    {
        assert_reachable(&denied, path);
    }

    // An organization made after this launch is masked from the next one.
    let org_c = data.join("0195a000-0000-7000-8000-00000000c001");
    write_secret(&org_c.join("durable/private/task-notes.db"));
    assert!(!is_masked(&denied, &org_c));
    assert_masked_whole(&denies(&mgr, &me), &org_c);
}

#[test]
fn co_hosted_satellites_are_masked_from_hq_agents_and_their_folder_is_made_for_bwrap() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let mgr = manager(&office);
    let me = first_employee(&office);
    let satellites = data_root(&office)
        .join(&office.info().org_id)
        .join("offices");
    let denied = denies(&mgr, &me);
    assert!(
        denied.contains(&format!("{}/**", satellites.display())),
        "{denied:?}"
    );
    assert!(!satellites.exists(), "denying never makes the folder");

    crate::manager::ensure_bwrap_deny_dirs(&mgr, &me).unwrap();
    assert!(
        satellites.is_dir(),
        "bwrap makes it so its mask is a mount, which hides satellites that join later"
    );
    let mode = std::fs::metadata(&satellites).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
    // A satellite of the organization that joins this host after the launch.
    let sat_id = "0195a000-0000-7000-8000-00000000a0a1";
    let sat = satellites.join(sat_id);
    let secrets = [
        sat.join("durable/local/offices")
            .join(sat_id)
            .join("auth/token"),
        sat.join("durable/blobs/x"),
        sat.join("runtime").join(sat_id).join("protected/x"),
    ];
    for secret in &secrets {
        write_secret(secret);
        assert!(is_masked(&denied, secret), "{}", secret.display());
    }
    let next = denies(&mgr, &me);
    assert_masked_whole(&next, &satellites);
    for path in own_trees(&mgr, &office, &me) {
        assert_reachable(&next, &path);
    }
}

#[test]
fn another_office_in_the_same_roots_is_masked_whole_in_both_roots_and_never_written() {
    // A satellite agent beside its HQ's folders in one pair of roots: the HQ's local folder
    // (auth, TLS, its agents' homes, staged uploads) and its runtime folder.
    let sat = org_office::TestSatellite::new();
    let hq_id = sat.hq_office_id.clone();
    let office = Arc::new(sat.office);
    let mgr = manager(&office);
    let me = new_uuid_v7();
    let durable = office.durable_root().canonicalize().unwrap();
    let runtime = office.runtime_root().canonicalize().unwrap();
    let hq_local = durable.join("local/offices").join(&hq_id);
    let hq_runtime = runtime.join(&hq_id);
    let secrets = [
        hq_local.join("auth/token"),
        hq_local.join("tls/federation.v1.json"),
        hq_local.join("agents/0195a000-0000-7000-8000-00000000a0e1/home/.ssh/id_ed25519"),
        hq_local.join("attachment-staging/0195a000-0000-7000-8000-00000000a0e2/data"),
        hq_runtime.join("protected/blob-cache/x"),
        hq_runtime.join("teams/t1/agents/x/wt/file"),
    ];
    for secret in &secrets {
        write_secret(secret);
    }

    let denied = denies(&mgr, &me);
    for folder in [&hq_local, &hq_runtime] {
        assert_masked_whole(&denied, folder);
    }
    for secret in &secrets {
        assert!(is_masked(&denied, secret), "{}", secret.display());
    }
    for family in ["private", "blobs", "messaging", "journal", "state"] {
        let glob = format!("{}/**", durable.join(family).display());
        assert!(denied.contains(&glob), "{glob} missing: {denied:?}");
    }
    let own_id = &office.info().office_id;
    assert_reachable(
        &denied,
        &durable
            .join("local/offices")
            .join(own_id)
            .join("agents")
            .join(&me)
            .join("home"),
    );
    let own_runtime = runtime.join(own_id);
    assert!(!is_masked(&denied, &own_runtime), "{denied:?}");

    // A Linux `bwrap` launch makes nothing in another office's folder.
    let other = durable
        .join("local/offices")
        .join("0195a000-0000-7000-8000-00000000a0e3");
    std::fs::create_dir_all(&other).unwrap();
    crate::manager::ensure_bwrap_deny_dirs(&mgr, &me).unwrap();
    assert!(std::fs::read_dir(&other).unwrap().next().is_none());
    assert_masked_whole(&denies(&mgr, &me), &other);
}

#[test]
fn every_office_folder_in_the_office_home_is_masked_whole() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let mgr = manager(&office);
    let me = first_employee(&office);
    let folders = office.folder().parent().unwrap().to_path_buf();
    assert_eq!(
        folders.file_name().and_then(|n| n.to_str()),
        Some("offices")
    );
    let other = folders.join(OFFICE_B);
    let secrets = [
        other.join("git-credentials/store"),
        other.join("office.json"),
        folders.join(".refused/0195a000-0000-7000-8000-00000000b0b2/office.json"),
    ];
    for secret in &secrets {
        write_secret(secret);
    }
    let denied = denies(&mgr, &me);
    assert_masked_whole(&denied, &folders);
    for secret in &secrets {
        assert!(is_masked(&denied, secret), "{}", secret.display());
    }
    for path in own_trees(&mgr, &office, &me) {
        assert_reachable(&denied, &path);
    }
}

#[test]
fn co_hosted_masks_reach_claude_codex_agy_and_the_approval_bridge_and_the_workspace_stays() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let mgr = manager(&office);
    let me = first_employee(&office);
    let org_b = data_root(&office).join(ORG_B);
    let notes = org_b.join("durable/private/task-notes.db");
    write_secret(&notes);
    let (writable, _) = mgr.worktree_sandbox_grants(&me, true);
    let workspace = writable[0].clone();
    let preset = build_session_policy(
        PermissionPreset::AutonomousSandboxed,
        true,
        &workspace,
        None,
        &mgr.denied_office_globs_for(Some(&me)),
        1,
    )
    .unwrap();
    let employee = office.load_employee(&me).unwrap().unwrap();
    let spec = |runtime: &str| {
        build_session_spec(
            RuntimeId::try_from(runtime).unwrap(),
            &employee,
            ModelChoice::default(),
            workspace.clone(),
            Vec::new(),
            StandingInstructions {
                instructions: "Work.".into(),
                repository_instructions: String::new(),
            },
            &preset,
        )
        .unwrap()
    };
    let agent_dir = office
        .durable_root()
        .join("local/offices")
        .join(&office.info().office_id)
        .join("agents")
        .join(&me);
    let spec_path = |path: PathBuf| {
        std::fs::create_dir_all(&path).unwrap();
        SpecPath::try_from(path.to_str().unwrap()).unwrap()
    };
    let ctx = LaunchContext {
        runtime_path: PathBuf::from("/opt/bin/runtime"),
        runtime_version: None,
        runtime_dir: spec_path(agent_dir.join("runtime")),
        tmpdir: spec_path(agent_dir.join("tmp")),
        office_env_names: Vec::new(),
        new_session_id: Some(SessionId::try_from("0193a5c0-0000-4000-8000-000000000204").unwrap()),
        resume: None,
        config_dir: None,
    };
    let org_b_text = org_b.display().to_string();

    // Claude: every read tool is refused the other organization, and the Bash sandbox masks it
    // (`dir` on Linux `bwrap`, `dir/**` elsewhere) while the workspace stays writable.
    let (plan, _) = org_agent_runtime_claude::ClaudeRuntime::new()
        .plan_session(&spec("claude"), &ctx)
        .unwrap();
    let settings = &plan
        .files
        .iter()
        .find(|f| f.rel_path.as_str() == "claude-settings.json")
        .unwrap()
        .contents;
    let settings: serde_json::Value = serde_json::from_slice(settings).unwrap();
    let strings = |value: &serde_json::Value| -> Vec<String> {
        value
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect()
    };
    let perm_denies = strings(&settings["permissions"]["deny"]);
    assert!(
        perm_denies.contains(&format!("Read(/{org_b_text})")),
        "{perm_denies:?}"
    );
    let deny_read = strings(&settings["sandbox"]["filesystem"]["denyRead"]);
    assert!(
        deny_read
            .iter()
            .any(|d| *d == org_b_text || *d == format!("{org_b_text}/**")),
        "{deny_read:?}"
    );
    assert_reachable(&deny_read, Path::new(workspace.as_str()));
    let allow_write = strings(&settings["sandbox"]["filesystem"]["allowWrite"]);
    assert!(allow_write.iter().any(|w| w == workspace.as_str()));

    // Codex: the other organization is `none` in the session's filesystem profile.
    let (plan, _) = org_agent_runtime_codex::CodexRuntime::new()
        .plan_session(&spec("codex"), &ctx)
        .unwrap();
    let profile = plan
        .args
        .iter()
        .find(|a| a.starts_with("permissions.office.filesystem="))
        .expect("a filesystem profile");
    let entry = |path: &str, level: &str| format!("{}=\"{level}\"", serde_json::json!(path));
    assert!(profile.contains(&entry(&org_b_text, "none")), "{profile}");
    assert!(
        profile.contains(&entry(workspace.as_str(), "write")),
        "{profile}"
    );

    // Agy plans with the same deny rules and is confined by the approval bridge, which refuses
    // a read of the other organization and allows one in the workspace.
    org_agent_runtime_agy::AgyRuntime::new()
        .plan_session(&spec("agy"), &ctx)
        .unwrap();
    let read = |path: &Path| {
        let mut request = ApprovalRequest::new(
            ApprovalId::new("read-co-hosted"),
            ApprovalKind::Other,
            Escalation::None,
            "Read",
            serde_json::json!({ "file_path": path.display().to_string() }),
        );
        request.capability = Some(ToolCapability::Read);
        decide_escalation_with_roots(
            &request,
            &preset.policy,
            preset.preset,
            None,
            Some(&preset.sandbox.writable_roots),
        )
    };
    assert!(matches!(
        read(&notes),
        EscalationDisposition::AutoDeny { .. }
    ));
    assert!(matches!(
        read(&Path::new(workspace.as_str()).join("README.md")),
        EscalationDisposition::AllowByPolicy
    ));
}
