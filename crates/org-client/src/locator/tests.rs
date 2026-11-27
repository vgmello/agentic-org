//! Unit tests for `locator/instances.json` -> `/o/<suffix>/s.sock` auto-discovery (`TASK-248`).

use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};

use org_office_launch::{OfficeFile, OFFICES_DIR_NAME, OFFICE_FILE_NAME};
use org_proto::OfficeRole;
use serde_json::json;

use super::*;

const OFFICE_A: &str = "0193a5c0-0000-7000-8000-0000000000a1";
const OFFICE_B: &str = "0193a5c0-0000-7000-8000-0000000000b2";
const INSTANCE_A: &str = "0193a5c0-0000-7000-8000-000000001001";
const INSTANCE_B: &str = "0193a5c0-0000-7000-8000-000000001002";

struct FakeOffice {
    _listener: UnixListener,
    socket: PathBuf,
    runtime_dir: PathBuf,
    root: PathBuf,
}

fn spawn_listening_office(
    base: &Path,
    tag: &str,
    office_id: &str,
    office_name: Option<&str>,
) -> FakeOffice {
    let runtime_dir = base.join(format!("rt-{tag}"));
    let handle = &office_id[office_id.len() - 12..];
    let sock_dir = runtime_dir.join("o").join(handle);
    fs::create_dir_all(&sock_dir).unwrap();
    let socket = sock_dir.join("s.sock");
    let listener = UnixListener::bind(&socket).unwrap();

    let root = base.join(format!("inst-{tag}"));
    fs::create_dir_all(&root).unwrap();
    if let Some(name) = office_name {
        let dir = root
            .join("office-state")
            .join(OFFICES_DIR_NAME)
            .join(office_id);
        fs::create_dir_all(&dir).unwrap();
        let file = OfficeFile::selected(
            office_id,
            name,
            OfficeRole::Hq,
            "0193a5c0-0000-7000-8000-000000009001",
            "Acme",
            "2026-11-27T00:00:00.000Z",
        )
        .unwrap();
        fs::write(dir.join(OFFICE_FILE_NAME), file.to_vec().unwrap()).unwrap();
    }
    FakeOffice {
        _listener: listener,
        socket,
        runtime_dir,
        root,
    }
}

fn write_locator(path: &Path, entries: serde_json::Value) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let body = json!({
        "version": 1,
        "user_temp": "/tmp",
        "entries": entries,
    });
    fs::write(path, serde_json::to_vec(&body).unwrap()).unwrap();
}

#[test]
fn single_active_office_auto_selects_and_matches_uuid_suffix_and_name() {
    let tmp = tempfile::Builder::new().prefix("loc1").tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let a = spawn_listening_office(&base, "a", OFFICE_A, Some("HQ Mac"));
    let locator = base.join("locator").join("instances.json");
    write_locator(
        &locator,
        json!([{
            "phase": "ready",
            "binding": {
                "name": "default",
                "id": INSTANCE_A,
                "root": a.root,
                "identity": { "device": 1, "inode": 1 },
                "temp_handle": "rta",
                "temp_folder": a.runtime_dir
            }
        }]),
    );

    // Bare auto-select finds the single active office.
    let found = resolve_local_office_from(Some(&locator), None, None, None).unwrap();
    assert_eq!(found.socket, a.socket);
    assert_eq!(found.office_id.as_deref(), Some(OFFICE_A));
    assert_eq!(found.office_name.as_deref(), Some("HQ Mac"));

    // Explicit --office <full-uuid>, unique suffix, and office name all resolve cleanly.
    for selector in [OFFICE_A, "0000000000a1", "hq mac", "default"] {
        let hit = resolve_local_office_from(Some(&locator), None, None, Some(selector)).unwrap();
        assert_eq!(hit.socket, a.socket, "selector `{selector}`");
    }
}

#[test]
fn explicit_uuid_matches_socket_suffix_even_without_office_json() {
    let tmp = tempfile::Builder::new().prefix("loc2").tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let a = spawn_listening_office(&base, "a", OFFICE_A, None);
    let locator = base.join("locator").join("instances.json");
    write_locator(
        &locator,
        json!([{
            "phase": "ready",
            "binding": {
                "name": "default",
                "id": INSTANCE_A,
                "root": a.root,
                "identity": { "device": 1, "inode": 1 },
                "temp_handle": "rta",
                "temp_folder": a.runtime_dir
            }
        }]),
    );

    let hit = resolve_local_office_from(Some(&locator), None, None, Some(OFFICE_A)).unwrap();
    assert_eq!(hit.socket, a.socket);
    assert_eq!(hit.socket_handle, "0000000000a1");
}

#[test]
fn multiple_active_offices_require_disambiguation_or_saved_context() {
    let tmp = tempfile::Builder::new().prefix("loc3").tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let a = spawn_listening_office(&base, "a", OFFICE_A, Some("HQ"));
    let b = spawn_listening_office(&base, "b", OFFICE_B, Some("EU Satellite"));
    let locator = base.join("locator").join("instances.json");
    write_locator(
        &locator,
        json!([
            {
                "phase": "ready",
                "binding": {
                    "name": "default",
                    "id": INSTANCE_A,
                    "root": a.root,
                    "identity": { "device": 1, "inode": 1 },
                    "temp_handle": "rta",
                    "temp_folder": a.runtime_dir
                }
            },
            {
                "phase": "ready",
                "binding": {
                    "name": "eu",
                    "id": INSTANCE_B,
                    "root": b.root,
                    "identity": { "device": 1, "inode": 2 },
                    "temp_handle": "rtb",
                    "temp_folder": b.runtime_dir
                }
            }
        ]),
    );

    // Without selector or saved context -> Ambiguous error listing both offices.
    let err = resolve_local_office_from(Some(&locator), None, None, None).unwrap_err();
    match &err {
        ResolveOfficeError::Ambiguous { candidates, .. } => assert_eq!(candidates.len(), 2),
        other => panic!("expected Ambiguous, got {other:?}"),
    }
    let msg = err.to_string();
    assert!(msg.contains(OFFICE_A) && msg.contains(OFFICE_B), "{msg}");

    // With explicit --office <uuid> -> resolves cleanly.
    let pick_b = resolve_local_office_from(Some(&locator), None, None, Some(OFFICE_B)).unwrap();
    assert_eq!(pick_b.socket, b.socket);

    // With saved cli-context.json -> resolves the saved office automatically!
    let saved = SavedContext {
        instance_name: Some("eu".into()),
        office_id: OFFICE_B.into(),
    };
    let via_saved = resolve_local_office_from(Some(&locator), None, Some(&saved), None).unwrap();
    assert_eq!(via_saved.socket, b.socket);
}

#[test]
fn stale_socket_reserved_phase_and_symlinks_are_ignored_fail_closed() {
    let tmp = tempfile::Builder::new().prefix("loc4").tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let dead = spawn_listening_office(&base, "dead", OFFICE_A, Some("Dead"));
    // Drop listener so socket file stays behind but refuses connections (stale).
    let dead_rt = dead.runtime_dir.clone();
    let dead_root = dead.root.clone();
    drop(dead);

    let reserved = spawn_listening_office(&base, "res", OFFICE_B, Some("Reserved"));
    let locator = base.join("locator").join("instances.json");
    write_locator(
        &locator,
        json!([
            {
                "phase": "ready",
                "binding": {
                    "name": "dead",
                    "id": INSTANCE_A,
                    "root": dead_root,
                    "identity": { "device": 1, "inode": 1 },
                    "temp_handle": "rtd",
                    "temp_folder": dead_rt
                }
            },
            {
                "phase": "reserved",
                "binding": {
                    "name": "res",
                    "id": INSTANCE_B,
                    "root": reserved.root,
                    "identity": { "device": 1, "inode": 2 },
                    "temp_handle": "rtr",
                    "temp_folder": reserved.runtime_dir
                }
            }
        ]),
    );
    assert!(matches!(
        resolve_local_office_from(Some(&locator), None, None, None),
        Err(ResolveOfficeError::NotFound { selector: None })
    ));

    // A symlinked locator file is refused fail-closed.
    let link = base.join("linked-instances.json");
    std::os::unix::fs::symlink(&locator, &link).unwrap();
    assert!(discover_from_locator(&link).is_empty());
}

#[test]
fn alias_and_symlinked_parent_paths_deduplicate_to_single_office() {
    let tmp = tempfile::Builder::new().prefix("loc5").tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let real_parent = base.join("real");
    let alias_parent = base.join("alias");
    let real_home = real_parent.join("home");
    let handle = &OFFICE_A[OFFICE_A.len() - 12..];
    let sock_dir = real_home.join("run").join("o").join(handle);
    fs::create_dir_all(&sock_dir).unwrap();
    let real_socket = sock_dir.join("s.sock");
    let _listener = UnixListener::bind(&real_socket).unwrap();

    // Symlinked parent directory (models macOS `/tmp` -> `/private/tmp` and `/var` -> `/private/var`).
    std::os::unix::fs::symlink(&real_parent, &alias_parent).unwrap();
    let alias_home = alias_parent.join("home");

    let locator = base.join("locator").join("instances.json");
    write_locator(
        &locator,
        json!([
            {
                "phase": "ready",
                "binding": {
                    "name": "default",
                    "id": INSTANCE_A,
                    "root": real_home,
                    "identity": { "device": 1, "inode": 1 },
                    "temp_handle": "rta",
                    "temp_folder": real_home.join("run")
                }
            },
            {
                "phase": "ready",
                "binding": {
                    "name": "alias-copy",
                    "id": INSTANCE_A,
                    "root": alias_home,
                    "identity": { "device": 1, "inode": 1 },
                    "temp_handle": "rta",
                    "temp_folder": alias_home.join("run")
                }
            }
        ]),
    );

    assert_eq!(discover_from_locator(&locator).len(), 1);
    let resolved = resolve_local_office_from(Some(&locator), Some(&alias_home), None, None)
        .expect("alias paths to the same socket must deduplicate instead of returning Ambiguous");
    assert_eq!(resolved.socket, real_socket);
    assert_eq!(resolved.instance_name, "default");
}

#[test]
fn default_context_path_fails_closed_on_relative_overrides() {
    let env_of = |pairs: &'static [(&'static str, &'static str)]| {
        move |name: &str| {
            pairs
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| OsString::from(v))
        }
    };

    // Relative AGENTIC_ORG_CONFIG_DIR fails closed instead of falling back to HOME or dev base.
    assert_eq!(
        default_context_path_from(
            &env_of(&[
                (CONFIG_DIR_ENV, "relative/cfg"),
                (DEV_ACCOUNT_BASE_ENV, "/tmp/dev"),
                ("HOME", "/home/ada"),
            ]),
            true,
        ),
        None
    );
    assert_eq!(
        default_context_path_from(
            &env_of(&[(CONFIG_DIR_ENV, "relative/cfg"), ("HOME", "/home/ada")]),
            false,
        ),
        None
    );

    // Absolute AGENTIC_ORG_CONFIG_DIR wins cleanly.
    assert_eq!(
        default_context_path_from(
            &env_of(&[(CONFIG_DIR_ENV, "/tmp/cfg"), ("HOME", "/home/ada")]),
            false,
        ),
        Some(PathBuf::from("/tmp/cfg/cli-context.json"))
    );

    // Relative ORG_DEV_ACCOUNT_BASE in development builds fails closed instead of falling back to HOME.
    assert_eq!(
        default_context_path_from(
            &env_of(&[(DEV_ACCOUNT_BASE_ENV, "rel/dev"), ("HOME", "/home/ada")]),
            true,
        ),
        None
    );
    assert_eq!(
        default_locator_path_from(
            &env_of(&[(DEV_ACCOUNT_BASE_ENV, "rel/dev"), ("HOME", "/home/ada")]),
            true,
        ),
        None
    );

    // Absolute ORG_DEV_ACCOUNT_BASE in dev mode works; unset falls back to HOME.
    assert_eq!(
        default_context_path_from(
            &env_of(&[(DEV_ACCOUNT_BASE_ENV, "/tmp/dev"), ("HOME", "/home/ada")]),
            true,
        ),
        Some(PathBuf::from("/tmp/dev/cli-context.json"))
    );
    assert_eq!(
        default_context_path_from(&env_of(&[("HOME", "/home/ada")]), false),
        Some(PathBuf::from("/home/ada/cli-context.json"))
    );
}

#[test]
fn symlinked_sockets_root_handle_dir_and_socket_file_are_refused() {
    let tmp = tempfile::Builder::new().prefix("loc6").tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let real = spawn_listening_office(&base, "real", OFFICE_A, Some("Real"));
    let handle = &OFFICE_A[OFFICE_A.len() - 12..];

    // 1. Symlinked `o/` directory under runtime_dir.
    let rt_link_o = base.join("rt-lo");
    fs::create_dir_all(&rt_link_o).unwrap();
    std::os::unix::fs::symlink(real.runtime_dir.join("o"), rt_link_o.join("o")).unwrap();

    // 2. Symlinked `<handle>/` directory under `o/`.
    let rt_link_handle = base.join("rt-lh");
    fs::create_dir_all(rt_link_handle.join("o")).unwrap();
    std::os::unix::fs::symlink(
        real.runtime_dir.join("o").join(handle),
        rt_link_handle.join("o").join(handle),
    )
    .unwrap();

    // 3. Symlinked `s.sock` inside real `o/<handle>/`.
    let rt_link_sock = base.join("rt-ls");
    let link_sock_dir = rt_link_sock.join("o").join(handle);
    fs::create_dir_all(&link_sock_dir).unwrap();
    std::os::unix::fs::symlink(&real.socket, link_sock_dir.join("s.sock")).unwrap();

    let locator = base.join("locator").join("instances.json");
    write_locator(
        &locator,
        json!([
            {
                "phase": "ready",
                "binding": {
                    "name": "link-o",
                    "id": INSTANCE_A,
                    "root": real.root,
                    "identity": { "device": 1, "inode": 1 },
                    "temp_handle": "rlo",
                    "temp_folder": rt_link_o
                }
            },
            {
                "phase": "ready",
                "binding": {
                    "name": "link-handle",
                    "id": INSTANCE_A,
                    "root": real.root,
                    "identity": { "device": 1, "inode": 2 },
                    "temp_handle": "rlh",
                    "temp_folder": rt_link_handle
                }
            },
            {
                "phase": "ready",
                "binding": {
                    "name": "link-sock",
                    "id": INSTANCE_A,
                    "root": real.root,
                    "identity": { "device": 1, "inode": 3 },
                    "temp_handle": "rls",
                    "temp_folder": rt_link_sock
                }
            }
        ]),
    );

    assert!(
        discover_from_locator(&locator).is_empty(),
        "symlinked o/, <handle>/, and s.sock must all be refused"
    );
    assert!(matches!(
        resolve_local_office_from(Some(&locator), None, None, None),
        Err(ResolveOfficeError::NotFound { selector: None })
    ));
}
