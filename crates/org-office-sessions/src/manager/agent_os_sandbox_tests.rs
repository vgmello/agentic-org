//! Managed sandboxes never expose `agent-os` external sign-in state (`TASK-137`): workspace
//! keys, fast tokens and `exec` token files under `<application base>/agent-os/` are denied to
//! every managed agent and masked by a Linux `bwrap` launch even before they exist.

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use org_agent_runtime::{PathPattern, SpecPath};
use org_office::organization_family::records::PermissionPreset;
use org_office::{Office, TestOffice};

use super::rust_toolchain::canonicalize_with_tail;
use super::sandbox_rules::agent_os_state_roots_from;
use super::spec::prune_nonexistent_bwrap_denies;
use super::{ensure_bwrap_deny_dirs, SessionManager, SessionManagerConfig};

fn env_of(pairs: &[(&str, &Path)]) -> impl Fn(&str) -> Option<OsString> {
    let owned: Vec<(String, OsString)> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.as_os_str().to_owned()))
        .collect();
    move |name| {
        owned
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    }
}

fn strings(patterns: &[PathPattern]) -> Vec<String> {
    patterns.iter().map(|p| p.as_str().to_owned()).collect()
}

#[test]
fn state_roots_follow_the_application_base_and_dev_overrides_only_in_development() {
    let home = Path::new("/home/ada");
    let base = org_platform::dirs::application_base(home).unwrap();
    let account = org_platform::dirs::account_application_base(home).unwrap();
    let release = agent_os_state_roots_from(&env_of(&[("HOME", home)]), false);
    assert!(release.contains(&base.join("agent-os")), "{release:?}");
    assert!(release.contains(&account.join("agent-os")), "{release:?}");
    assert!(release.len() <= 2, "no duplicates: {release:?}");

    let moved = Path::new("/srv/aos-state");
    let dev_base = Path::new("/srv/dev-account");
    let env = env_of(&[
        ("HOME", home),
        ("AGENT_OS_STATE_DIR", moved),
        ("ORG_DEV_ACCOUNT_BASE", dev_base),
    ]);
    let development = agent_os_state_roots_from(&env, true);
    assert!(development.contains(&moved.to_path_buf()));
    assert!(development.contains(&dev_base.join("agent-os")));
    // A release build never follows the development overrides (agent-os refuses them too).
    let release = agent_os_state_roots_from(&env, false);
    assert!(!release.contains(&moved.to_path_buf()));
    assert!(!release.contains(&dev_base.join("agent-os")));

    // Relative or empty values are no root; no HOME is no home root.
    let relative = env_of(&[
        ("HOME", Path::new("relative/home")),
        ("AGENT_OS_STATE_DIR", Path::new("rel")),
        ("ORG_DEV_ACCOUNT_BASE", Path::new("")),
    ]);
    assert!(agent_os_state_roots_from(&relative, true).is_empty());
    assert!(agent_os_state_roots_from(&|_| None, true).is_empty());
}

#[test]
fn every_managed_sandbox_denies_and_bwrap_masks_the_agent_os_state_roots() {
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let sock_dir = office.durable_root().join("run-sock");
    std::fs::create_dir_all(&sock_dir).unwrap();
    let home = tempfile::tempdir().unwrap();
    let home = home.path().canonicalize().unwrap();
    let moved = home.join("moved-state");
    let roots: Vec<PathBuf> = agent_os_state_roots_from(
        &env_of(&[("HOME", &home), ("AGENT_OS_STATE_DIR", &moved)]),
        true,
    );
    assert!(roots.len() >= 2, "{roots:?}");
    let mut config = SessionManagerConfig::production(sock_dir.join("office.sock"));
    config.agent_os_state_roots = roots.clone();
    let mgr = SessionManager::new(Arc::clone(&office), config);
    let me = office.list_employees().unwrap()[0].id.as_str().to_owned();

    // Before anything exists: the deny globs (seatbelt, landlock, policy) already cover them.
    for root in &roots {
        assert!(!root.exists(), "{root:?}");
    }
    let wanted: Vec<String> = roots
        .iter()
        .map(|r| format!("{}/**", canonicalize_with_tail(r).display()))
        .collect();
    let denied = strings(&mgr.denied_office_globs_for(Some(&me)));
    let generic = strings(&mgr.denied_office_globs_for(None));
    for glob in &wanted {
        assert!(denied.contains(glob), "{glob} not denied: {denied:?}");
        assert!(generic.contains(glob), "{glob} not denied: {generic:?}");
    }
    let workspace = home.join("ws");
    std::fs::create_dir_all(&workspace).unwrap();
    let ws = SpecPath::try_from(workspace.to_str().unwrap()).unwrap();
    for preset in PermissionPreset::ALL {
        let cfg = mgr
            .build_employee_preset_cfg(&me, preset, true, &ws, false)
            .unwrap();
        let deny_read = strings(&cfg.sandbox.deny_read);
        for glob in &wanted {
            assert!(
                deny_read.contains(glob),
                "{preset:?}: {glob} not in deny_read {deny_read:?}"
            );
        }
    }

    // Linux `bwrap`: the roots are pre-created owner-only, so the mask survives the pruning of
    // absent paths and a key written mid-session is never visible.
    ensure_bwrap_deny_dirs(&mgr, &me).unwrap();
    for root in &roots {
        let meta = std::fs::symlink_metadata(root).unwrap();
        assert!(meta.is_dir(), "{root:?}");
        assert_eq!(meta.permissions().mode() & 0o777, 0o700, "{root:?}");
    }
    let mut cfg = mgr
        .build_employee_preset_cfg(&me, PermissionPreset::AutonomousSandboxed, true, &ws, false)
        .unwrap();
    prune_nonexistent_bwrap_denies(&mut cfg.sandbox, &mut cfg.policy);
    let masked = strings(&cfg.sandbox.deny_read);
    for root in &roots {
        let concrete = root.canonicalize().unwrap().display().to_string();
        assert!(
            masked.contains(&concrete),
            "{concrete} not masked: {masked:?}"
        );
    }

    // A planted file where a root belongs fails the launch closed.
    std::fs::remove_dir(&roots[0]).unwrap();
    std::fs::write(&roots[0], b"not a folder").unwrap();
    assert!(ensure_bwrap_deny_dirs(&mgr, &me).is_err());
}
