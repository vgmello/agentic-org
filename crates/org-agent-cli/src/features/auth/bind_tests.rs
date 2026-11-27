//! Unit tests for one-step workspace binding (`TASK-211`).

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use org_command::Refusal;
use org_proto::ErrorCode;
use serde_json::{json, Value};

use super::*;
use crate::features::auth::store::{TokenFile, WorkspaceState};
use crate::runtime::transport::OfficeTransport;

struct FakeBindTransport {
    calls: Vec<(String, Value)>,
    office_socket: Option<String>,
    fail_with: Option<Refusal>,
}

impl FakeBindTransport {
    fn new(office_socket: Option<&str>) -> Self {
        Self {
            calls: Vec::new(),
            office_socket: office_socket.map(str::to_owned),
            fail_with: None,
        }
    }
}

impl OfficeTransport for FakeBindTransport {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        self.calls.push((method.to_owned(), params));
        if let Some(refusal) = &self.fail_with {
            return Err(refusal.clone());
        }
        Ok(json!({
            "office_id": "0193a5c0-0000-7000-8000-000000000001",
            "office_socket": self.office_socket,
            "session_id": "ext-0193a5c0-0000-7000-8000-000000000070",
            "employee_id": "0193a5c0-0000-7000-8000-000000000051",
            "handle": "ada",
            "workspace_label": "~/src/app",
            "pubkey_fingerprint": "3f9c2a7d10b4e8f1",
            "approved_by": "0193a5c0-0000-7000-8000-000000000002",
            "approved_at": "2026-11-27T00:00:00.000Z",
            "expires_at": "2026-11-28T00:00:00.000Z",
            "active": true
        }))
    }
}

fn mode_of(path: &Path) -> u32 {
    std::fs::symlink_metadata(path)
        .unwrap()
        .permissions()
        .mode()
        & 0o777
}

#[test]
fn bind_workspace_writes_owner_only_credentials_outside_workspace_and_clears_stale_token() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("repo");
    std::fs::create_dir(&ws).unwrap();
    let canonical = ws.canonicalize().unwrap();
    let state_root = tmp.path().join("state");

    // Pre-seed a stale token from a prior binding of this workspace.
    let state = WorkspaceState::for_root(&state_root, &canonical);
    state.ensure().unwrap();
    state
        .write_token(&TokenFile {
            version: 1,
            session_id: "ext-stale".into(),
            token: String::from("stale-secret-token").into(),
            expires_at: "2099-01-01T00:00:00.000Z".into(),
        })
        .unwrap();

    let sock = "/tmp/rt/o/000000000001/s.sock";
    let mut transport = FakeBindTransport::new(Some(sock));
    let bound = bind_workspace(
        &mut transport,
        &WorkspaceBindRequest {
            member: "@ada",
            workspace_dir: &ws,
            label: Some("my-repo"),
            ttl_secs: Some(3600),
            takeover: true,
            command_id: Some("cmd-1"),
            state_root: Some(&state_root),
        },
    )
    .unwrap();

    assert_eq!(transport.calls.len(), 1);
    assert_eq!(transport.calls[0].0, "member.external_bind");
    assert_eq!(transport.calls[0].1["employee_id"], "@ada");
    assert_eq!(transport.calls[0].1["workspace_label"], "my-repo");
    assert_eq!(transport.calls[0].1["ttl_secs"], 3600);
    assert_eq!(transport.calls[0].1["takeover"], true);
    let sent_pubkey = transport.calls[0].1["pubkey_ed25519_b64"].as_str().unwrap();

    assert_eq!(bound.workspace_root, canonical.display().to_string());
    assert_eq!(
        bound.session.session_id,
        "ext-0193a5c0-0000-7000-8000-000000000070"
    );
    assert_eq!(mode_of(state.dir()), 0o700);
    assert_eq!(mode_of(&state.dir().join("binding.v1.json")), 0o600);
    assert_eq!(mode_of(&state.dir().join("key.ed25519.pkcs8")), 0o600);
    assert!(
        state.read_token().unwrap().is_none(),
        "stale token.v1 must be removed on re-bind"
    );
    let saved_key = state.read_key().unwrap().unwrap();
    assert_eq!(client::public_key_b64(&saved_key).unwrap(), sent_pubkey);
    let saved_binding = state.read_binding().unwrap().unwrap();
    assert_eq!(saved_binding.socket.as_deref(), Some(sock));
    assert_eq!(saved_binding.runtime_dir.as_deref(), Some("/tmp/rt"));
    assert_eq!(saved_binding.session_id, bound.session.session_id);
    // Workspace folder itself stays completely untouched.
    assert_eq!(std::fs::read_dir(&ws).unwrap().count(), 0);
}

#[test]
fn bind_workspace_refuses_bad_paths_bad_ttl_unsafe_state_and_unsafe_sockets_closed() {
    let tmp = tempfile::tempdir().unwrap();
    let state_root = tmp.path().join("state");
    let file_not_dir = tmp.path().join("plain.txt");
    std::fs::write(&file_not_dir, b"hi").unwrap();
    let ws = tmp.path().join("ws");
    std::fs::create_dir(&ws).unwrap();

    // Missing directory, file instead of directory, out-of-range TTL, control-char label: zero RPCs.
    let mut transport = FakeBindTransport::new(Some("/tmp/office.sock"));
    for (member, dir, label, ttl) in [
        ("@ada", tmp.path().join("missing"), None, None),
        ("@ada", file_not_dir, None, None),
        ("@ada", ws.clone(), None, Some(60)),
        ("@ada", ws.clone(), Some("bad\nlabel"), None),
        ("", ws.clone(), None, None),
    ] {
        let err = bind_workspace(
            &mut transport,
            &WorkspaceBindRequest {
                member,
                workspace_dir: &dir,
                label,
                ttl_secs: ttl,
                takeover: false,
                command_id: None,
                state_root: Some(&state_root),
            },
        )
        .unwrap_err();
        assert!(
            matches!(err.code, ErrorCode::InvalidRequest | ErrorCode::NotFound),
            "{err:?}"
        );
    }
    assert!(
        transport.calls.is_empty(),
        "invalid inputs never reach the office"
    );

    // Unsafe (symlinked) existing state folder is refused before calling the office.
    let canonical = ws.canonicalize().unwrap();
    let ws_state = WorkspaceState::for_root(&state_root, &canonical);
    let real_target = tmp.path().join("real-target");
    std::fs::create_dir_all(&real_target).unwrap();
    org_platform::fs::create_owner_only_dir_all(ws_state.dir().parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&real_target, ws_state.dir()).unwrap();
    let err = bind_workspace(
        &mut transport,
        &WorkspaceBindRequest {
            member: "@ada",
            workspace_dir: &ws,
            label: None,
            ttl_secs: None,
            takeover: false,
            command_id: None,
            state_root: Some(&state_root),
        },
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::Unavailable);
    assert!(transport.calls.is_empty());

    // Unsafe office_socket returned by server (relative or traversal) fails closed without writing.
    let ws2 = tmp.path().join("ws2");
    std::fs::create_dir(&ws2).unwrap();
    let state2 = WorkspaceState::for_root(&state_root, &ws2.canonicalize().unwrap());
    for bad_socket in ["relative.sock", "/tmp/../etc/passwd"] {
        let mut t = FakeBindTransport::new(Some(bad_socket));
        let err = bind_workspace(
            &mut t,
            &WorkspaceBindRequest {
                member: "@ada",
                workspace_dir: &ws2,
                label: None,
                ttl_secs: None,
                takeover: false,
                command_id: None,
                state_root: Some(&state_root),
            },
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::Unavailable, "{bad_socket}");
        assert_eq!(t.calls.len(), 1, "bad socket must be checked after RPC");
        assert!(!state2.dir().exists(), "{bad_socket}");
    }
}

#[test]
fn bind_workspace_refuses_symlinked_or_wide_parent_state_dirs_before_any_rpc_or_write() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let canonical = ws.canonicalize().unwrap();

    // 1. Symlinked `<state_root>/workspaces` while `<hash>` is absent -> refused BEFORE any RPC!
    let evil_target = tmp.path().join("evil-workspaces");
    std::fs::create_dir(&evil_target).unwrap();
    std::fs::set_permissions(&evil_target, std::fs::Permissions::from_mode(0o700)).unwrap();
    let state_root_1 = tmp.path().join("state-link-ws");
    std::fs::create_dir(&state_root_1).unwrap();
    std::os::unix::fs::symlink(&evil_target, state_root_1.join("workspaces")).unwrap();
    let mut transport = FakeBindTransport::new(Some("/tmp/office.sock"));
    let err = bind_workspace(
        &mut transport,
        &WorkspaceBindRequest {
            member: "@ada",
            workspace_dir: &ws,
            label: None,
            ttl_secs: None,
            takeover: false,
            command_id: None,
            state_root: Some(&state_root_1),
        },
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::Unavailable, "{err:?}");
    assert!(
        transport.calls.is_empty(),
        "symlinked <state_root>/workspaces must fail BEFORE member.external_bind RPC"
    );
    assert_eq!(
        std::fs::read_dir(&evil_target).unwrap().count(),
        0,
        "nothing may be created inside symlink target"
    );

    // 2. Symlinked `<state_root>` itself (even pointing to a 0700 dir) -> refused BEFORE any RPC!
    let real_root = tmp.path().join("real-state-root");
    std::fs::create_dir(&real_root).unwrap();
    std::fs::set_permissions(&real_root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let linked_root = tmp.path().join("linked-state-root");
    std::os::unix::fs::symlink(&real_root, &linked_root).unwrap();
    let err = bind_workspace(
        &mut transport,
        &WorkspaceBindRequest {
            member: "@ada",
            workspace_dir: &ws,
            label: None,
            ttl_secs: None,
            takeover: false,
            command_id: None,
            state_root: Some(&linked_root),
        },
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::Unavailable, "{err:?}");
    assert!(transport.calls.is_empty());
    assert_eq!(std::fs::read_dir(&real_root).unwrap().count(), 0);

    // 3. Group-readable `0755` `<state_root>/workspaces` while `<hash>` is absent -> refused!
    let state_root_wide = tmp.path().join("state-wide-ws");
    std::fs::create_dir_all(state_root_wide.join("workspaces")).unwrap();
    std::fs::set_permissions(
        state_root_wide.join("workspaces"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let err = bind_workspace(
        &mut transport,
        &WorkspaceBindRequest {
            member: "@ada",
            workspace_dir: &ws,
            label: None,
            ttl_secs: None,
            takeover: false,
            command_id: None,
            state_root: Some(&state_root_wide),
        },
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::Unavailable, "{err:?}");
    assert!(transport.calls.is_empty());

    // 4. `WorkspaceState::ensure()` and `write_key()` also refuse a symlinked `workspaces` dir.
    let state_via_at = WorkspaceState::at(
        state_root_1
            .join("workspaces")
            .join(store::workspace_key(&canonical)),
    );
    assert!(state_via_at.preflight().is_err());
    assert!(state_via_at.ensure().is_err());
    assert!(state_via_at.write_key(b"secret").is_err());
}

#[test]
fn default_label_truncates_multibyte_paths_on_utf8_boundary() {
    let long = format!("/tmp/{}", "é".repeat(120));
    let label = default_label_for(Path::new(&long));
    assert!(label.len() <= MAX_WORKSPACE_LABEL_BYTES, "{}", label.len());
    assert!(label.starts_with('…'));
}

#[test]
fn agentic_org_help_lists_agent_bind_and_hr_hire_bind_workspace_while_agent_os_omits_them() {
    let mut noop = crate::runtime::transport::TransportOffice(FakeBindTransport::new(None));
    let words = |s: &[&str]| s.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>();

    let op_agent =
        crate::execute_with_office("agentic-org", &words(&["agent", "--help"]), &mut noop);
    assert_eq!(op_agent.exit_code, 0);
    assert!(op_agent.stdout.contains("\n  bind "), "{}", op_agent.stdout);

    let ag_agent = crate::execute_with_office("agent-os", &words(&["agent", "--help"]), &mut noop);
    assert_eq!(ag_agent.exit_code, 0);
    assert!(
        !ag_agent.stdout.contains("\n  bind "),
        "{}",
        ag_agent.stdout
    );

    let op_hire =
        crate::execute_with_office("agentic-org", &words(&["hr", "hire", "--help"]), &mut noop);
    assert_eq!(op_hire.exit_code, 0);
    assert!(
        op_hire.stdout.contains("--bind-workspace"),
        "{}",
        op_hire.stdout
    );

    let ag_hire =
        crate::execute_with_office("agent-os", &words(&["hr", "hire", "--help"]), &mut noop);
    assert_eq!(ag_hire.exit_code, 0);
    assert!(
        !ag_hire.stdout.contains("--bind-workspace"),
        "{}",
        ag_hire.stdout
    );
}
