//! Tests for `agentic-org agent bind` grammar, flags, output modes, and secret redaction.

use std::os::unix::fs::PermissionsExt;

use org_agent_cli::external_auth::store::WorkspaceState;
use org_cli_shell::context::testing::{captured, tty};
use org_cli_shell::context::Modes;
use org_cli_shell::error::CliDiagnostic;
use org_cli_shell::terminal::Terminal;
use org_command::Refusal;
use org_proto::ErrorCode;
use serde_json::{json, Value};

use super::*;

struct FakeTransport {
    calls: Vec<(String, Value)>,
    refuse: Option<Refusal>,
}

impl OfficeTransport for FakeTransport {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        self.calls.push((method.to_owned(), params));
        if let Some(refusal) = &self.refuse {
            return Err(refusal.clone());
        }
        Ok(json!({
            "office_id": "0193a5c0-0000-7000-8000-000000000001",
            "office_socket": "/tmp/rt/o/000000000001/s.sock",
            "session_id": "ext-0193a5c0-0000-7000-8000-000000000070",
            "employee_id": "0193a5c0-0000-7000-8000-000000000051",
            "handle": "ada",
            "workspace_label": "~/repo",
            "pubkey_fingerprint": "3f9c2a7d10b4e8f1",
            "approved_by": "0193a5c0-0000-7000-8000-000000000002",
            "approved_at": "2026-11-27T00:00:00.000Z",
            "expires_at": "2026-11-28T00:00:00.000Z",
            "active": true
        }))
    }
}

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn bind_grammar_is_consistent_and_help_has_use_when_and_examples() {
    command().debug_assert();
    let help = command().render_long_help().to_string();
    assert!(help.contains("Use when:"), "{help}");
    assert!(help.contains("Examples:"), "{help}");
    assert!(help.contains("--member"), "{help}");
    assert!(help.contains("--workspace"), "{help}");
    assert!(help.contains("--takeover"), "{help}");
    assert!(help.contains("--json"), "{help}");
}

#[test]
fn parse_bind_args_accepts_help_and_rejects_missing_required_flags() {
    let (mut ctx, out) = captured(Modes::default(), Terminal::piped(), &[]);
    assert!(parse_bind_args(&words(&["--help"]), &mut ctx)
        .unwrap()
        .is_none());
    assert!(out.stdout.text().contains("agentic-org agent bind"));

    let (mut ctx, _) = captured(Modes::default(), Terminal::piped(), &[]);
    let err = parse_bind_args(&words(&["--member", "@ada"]), &mut ctx).unwrap_err();
    assert_eq!(err.exit_code(), 2);
    assert!(
        err.help()
            .as_deref()
            .is_some_and(|h| h.contains("--workspace")),
        "msg={err}, help={:?}",
        err.help()
    );

    let (mut ctx, _) = captured(
        Modes {
            json: true,
            ..Modes::default()
        },
        Terminal::piped(),
        &[],
    );
    let err = parse_bind_args(
        &words(&["--member", "@ada", "--workspace", "/tmp", "-q"]),
        &mut ctx,
    )
    .unwrap_err();
    assert_eq!(err.exit_code(), 2);
}

#[test]
fn execute_with_transport_binds_workspace_and_never_leaks_key_material() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("repo");
    std::fs::create_dir(&ws).unwrap();
    let state_root = tmp.path().join("state");

    let args = BindArgs::try_parse_from([
        "agentic-org agent bind",
        "--member",
        "@ada",
        "--workspace",
        ws.to_str().unwrap(),
        "--takeover",
        "--ttl",
        "8h",
    ])
    .unwrap();

    // JSON mode
    let mut transport = FakeTransport {
        calls: Vec::new(),
        refuse: None,
    };
    let (mut ctx, out) = captured(
        Modes {
            json: true,
            ..Modes::default()
        },
        Terminal::piped(),
        &[],
    );
    execute_with_transport(&args, &mut transport, Some(&state_root), &mut ctx).unwrap();
    assert_eq!(transport.calls.len(), 1);
    assert_eq!(transport.calls[0].0, "member.external_bind");
    assert_eq!(transport.calls[0].1["takeover"], true);
    assert_eq!(transport.calls[0].1["ttl_secs"], 28_800);

    let stdout = out.stdout.text();
    let parsed: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        parsed["session_id"],
        "ext-0193a5c0-0000-7000-8000-000000000070"
    );
    assert_eq!(
        parsed["employee_id"],
        "0193a5c0-0000-7000-8000-000000000051"
    );
    assert_eq!(
        parsed["workspace_root"],
        ws.canonicalize().unwrap().display().to_string()
    );
    assert!(parsed.get("token").is_none());
    assert!(parsed.get("pkcs8").is_none());
    assert!(parsed.get("private_key").is_none());

    // State saved outside workspace with 0700 / 0600 permissions.
    let state = WorkspaceState::for_root(&state_root, &ws.canonicalize().unwrap());
    let dir_mode = std::fs::symlink_metadata(state.dir())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(dir_mode, 0o700);
    let key_bytes = state.read_key().unwrap().unwrap();
    let key_b64 = org_agent_cli::external_auth::client::public_key_b64(&key_bytes).unwrap();
    assert!(!stdout.contains(&key_b64), "raw key material never printed");
    assert_eq!(std::fs::read_dir(&ws).unwrap().count(), 0);

    // Quiet mode prints only session_id.
    let (mut ctx, out) = captured(
        Modes {
            quiet: true,
            ..Modes::default()
        },
        tty(Some(80)),
        &[],
    );
    execute_with_transport(&args, &mut transport, Some(&state_root), &mut ctx).unwrap();
    assert_eq!(
        out.stdout.text(),
        "ext-0193a5c0-0000-7000-8000-000000000070\n"
    );
}

#[test]
fn unauthorized_refusal_leaves_no_local_state_behind() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("repo");
    std::fs::create_dir(&ws).unwrap();
    let state_root = tmp.path().join("state");
    let args = BindArgs::try_parse_from([
        "agentic-org agent bind",
        "--member",
        "@ada",
        "--workspace",
        ws.to_str().unwrap(),
    ])
    .unwrap();
    let mut transport = FakeTransport {
        calls: Vec::new(),
        refuse: Some(Refusal::new(
            ErrorCode::NotAuthorized,
            "not allowed",
            "sign in as the owner",
        )),
    };
    let (mut ctx, _) = captured(Modes::default(), Terminal::piped(), &[]);
    let err =
        execute_with_transport(&args, &mut transport, Some(&state_root), &mut ctx).unwrap_err();
    assert_eq!(err.exit_code(), 1);
    assert_eq!(err.code(), "office::not_authorized");
    let state = WorkspaceState::for_root(&state_root, &ws.canonicalize().unwrap());
    assert!(!state.dir().exists());
}
