//! Tests of the workspace state store: where it lives, owner-only modes, symbolic-link refusal,
//! walk-up resolution and token staleness.

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use super::*;

fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
    let owned: Vec<(String, String)> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    move |name| {
        owned
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| OsString::from(v))
    }
}

fn binding(root: &Path) -> WorkspaceBinding {
    WorkspaceBinding {
        version: 1,
        workspace_root: root.display().to_string(),
        office_id: "0193a5c0-0000-7000-8000-000000000001".into(),
        socket: Some("/tmp/office.sock".into()),
        runtime_dir: None,
        member_id: "0193a5c0-0000-7000-8000-000000000051".into(),
        handle: Some("ada".into()),
        session_id: "ext-1".into(),
        lease_expires_at: "2026-11-27T00:00:00.000Z".into(),
    }
}

fn mode(path: &Path) -> u32 {
    std::fs::symlink_metadata(path)
        .unwrap()
        .permissions()
        .mode()
        & 0o777
}

#[test]
fn the_state_root_honours_development_overrides_only_in_development_builds() {
    let dev = state_root_from(&env_of(&[(STATE_DIR_ENV, "/tmp/aos")]), true).unwrap();
    assert_eq!(dev, PathBuf::from("/tmp/aos"));
    let base = state_root_from(&env_of(&[(DEV_ACCOUNT_BASE_ENV, "/tmp/base")]), true).unwrap();
    assert_eq!(base, PathBuf::from("/tmp/base/agent-os"));
    assert!(state_root_from(&env_of(&[(STATE_DIR_ENV, "/tmp/aos")]), false).is_err());
    assert!(state_root_from(&env_of(&[(DEV_ACCOUNT_BASE_ENV, "/tmp/b")]), false).is_err());
    assert!(state_root_from(&env_of(&[(STATE_DIR_ENV, "relative")]), true).is_err());
    assert!(state_root_from(&env_of(&[("HOME", "relative")]), true).is_err());
    let home = state_root_from(&env_of(&[("HOME", "/home/someone")]), false).unwrap();
    assert!(home.ends_with("agent-os"), "{}", home.display());
    assert!(home.starts_with("/home/someone"), "{}", home.display());
}

#[test]
fn workspace_keys_are_sha256_hex_of_the_canonical_root() {
    let key = workspace_key(Path::new("/src/app"));
    assert_eq!(key.len(), 64);
    assert!(key.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(key, workspace_key(Path::new("/src/app")));
    assert_ne!(key, workspace_key(Path::new("/src/app2")));
}

#[test]
fn state_files_are_owner_only_and_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("state");
    let ws = tmp.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let state = WorkspaceState::for_root(&root, &ws.canonicalize().unwrap());
    state.ensure().unwrap();
    assert_eq!(mode(state.dir()), 0o700);
    let b = binding(&ws);
    state.write_binding(&b).unwrap();
    state.write_key(b"pkcs8-bytes").unwrap();
    let token = TokenFile {
        version: 1,
        session_id: "ext-1".into(),
        token: "tok-secret".into(),
        expires_at: "2026-11-27T00:00:00.000Z".into(),
    };
    state.write_token(&token).unwrap();
    for name in ["binding.v1.json", "key.ed25519.pkcs8", "token.v1"] {
        assert_eq!(mode(&state.dir().join(name)), 0o600, "{name}");
    }
    assert_eq!(state.read_binding().unwrap().unwrap(), b);
    assert_eq!(
        state.read_key().unwrap().unwrap().as_slice(),
        b"pkcs8-bytes"
    );
    assert_eq!(state.read_token().unwrap().unwrap(), token);
    assert!(!format!("{token:?}").contains("tok-secret"));
    let raw = std::fs::read_to_string(state.dir().join("binding.v1.json")).unwrap();
    assert!(
        !raw.contains("tok-secret"),
        "the binding never holds the token"
    );
    // No temporary file is left behind.
    let names: Vec<_> = std::fs::read_dir(state.dir())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names.len(), 3, "{names:?}");
    state.remove().unwrap();
    assert!(!state.dir().exists());
}

#[test]
fn symbolic_links_are_refused_for_files_and_for_the_state_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("state");
    let ws = tmp.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let state = WorkspaceState::for_root(&root, &ws);
    state.ensure().unwrap();
    let elsewhere = tmp.path().join("elsewhere.json");
    std::fs::write(&elsewhere, serde_json::to_vec(&binding(&ws)).unwrap()).unwrap();
    std::os::unix::fs::symlink(&elsewhere, state.dir().join("binding.v1.json")).unwrap();
    assert!(state.read_binding().is_err(), "a linked binding is refused");
    // A write never follows a planted link either: the link is replaced, its target untouched.
    let target = tmp.path().join("target-key");
    std::fs::write(&target, b"untouched").unwrap();
    std::os::unix::fs::symlink(&target, state.dir().join("key.ed25519.pkcs8")).unwrap();
    assert!(state.read_key().is_err());
    state.write_key(b"new").unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"untouched");

    // A state folder that is a symbolic link is refused.
    let real = tmp.path().join("real-state");
    std::fs::create_dir(&real).unwrap();
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o700)).unwrap();
    let linked = tmp.path().join("linked");
    std::os::unix::fs::symlink(&real, &linked).unwrap();
    assert!(WorkspaceState::at(linked).ensure().is_err());
    // A group-readable state folder is refused.
    let wide = tmp.path().join("wide");
    std::fs::create_dir(&wide).unwrap();
    std::fs::set_permissions(&wide, std::fs::Permissions::from_mode(0o750)).unwrap();
    assert!(WorkspaceState::at(wide).read_binding().is_err());
}

#[test]
fn oversized_state_files_are_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let state = WorkspaceState::for_root(&tmp.path().join("s"), tmp.path());
    state.ensure().unwrap();
    std::fs::write(state.dir().join("binding.v1.json"), vec![b' '; 64 * 1024]).unwrap();
    assert!(state.read_binding().is_err());
}

#[test]
fn resolution_walks_up_to_the_git_root_and_no_further() {
    let tmp = tempfile::tempdir().unwrap();
    let outer = tmp.path().canonicalize().unwrap();
    let repo = outer.join("repo");
    let deep = repo.join("a").join("b");
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::create_dir(repo.join(".git")).unwrap();
    let candidates = candidate_roots(&deep);
    assert_eq!(candidates, vec![deep.clone(), repo.join("a"), repo.clone()]);
    assert_eq!(default_workspace_root(&deep).unwrap(), repo);

    let state_root = outer.join("state");
    // A binding above the Git root is never picked up from inside it.
    let above = WorkspaceState::for_root(&state_root, &outer);
    above.ensure().unwrap();
    above.write_binding(&binding(&outer)).unwrap();
    assert!(find_workspace(&state_root, &deep).is_none());
    // The repository's own binding is.
    let own = WorkspaceState::for_root(&state_root, &repo);
    own.ensure().unwrap();
    own.write_binding(&binding(&repo)).unwrap();
    let (found, _) = find_workspace(&state_root, &deep).unwrap();
    assert_eq!(found, repo);
}

#[test]
fn tokens_are_stale_at_expiry_minus_the_margin_and_when_unreadable() {
    let token = TokenFile {
        version: 1,
        session_id: "s".into(),
        token: "t".into(),
        expires_at: "2026-11-27T00:00:00.000Z".into(),
    };
    let exp = org_proto::parse_rfc3339_ms(&token.expires_at).unwrap();
    assert!(!token.is_stale(exp - 120_000, 60_000));
    assert!(token.is_stale(exp - 60_000, 60_000));
    let broken = TokenFile {
        expires_at: "soon".into(),
        ..token
    };
    assert!(broken.is_stale(0, 0));
}

#[test]
fn labels_shorten_the_home_folder() {
    let home = Path::new("/home/ada");
    assert_eq!(
        tilde_path(Path::new("/home/ada/src/app"), Some(home)),
        "~/src/app"
    );
    assert_eq!(tilde_path(home, Some(home)), "~");
    assert_eq!(tilde_path(Path::new("/srv/app"), Some(home)), "/srv/app");
}
