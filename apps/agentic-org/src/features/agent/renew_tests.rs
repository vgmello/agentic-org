//! Tests for `agentic-org agent renew` grammar, default 7d TTL, flags, dry-run, and refusals.

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
            "session_id": "ext-0193a5c0-0000-7000-8000-000000000070",
            "employee_id": "0193a5c0-0000-7000-8000-000000000051",
            "handle": "ada",
            "workspace_label": "~/repo",
            "pubkey_fingerprint": "3f9c2a7d10b4e8f1",
            "approved_by": "0193a5c0-0000-7000-8000-000000000002",
            "approved_at": "2026-11-27T00:00:00.000Z",
            "expires_at": "2026-12-04T00:00:00.000Z",
            "active": true
        }))
    }
}

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn renew_grammar_is_consistent_and_help_has_use_when_and_examples() {
    command().debug_assert();
    let help = command().render_long_help().to_string();
    assert!(help.contains("Use when:"), "{help}");
    assert!(help.contains("Examples:"), "{help}");
    assert!(help.contains("--member"), "{help}");
    assert!(help.contains("--ttl"), "{help}");
    assert!(help.contains("--session"), "{help}");
    assert!(help.contains("--json"), "{help}");
}

#[test]
fn renew_defaults_to_7d_ttl_and_accepts_explicit_ttl_and_session() {
    let args = RenewArgs::try_parse_from(["agentic-org agent renew", "--member", "@ada"]).unwrap();
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
    execute_with_transport(&args, &mut transport, &mut ctx).unwrap();
    assert_eq!(transport.calls.len(), 1);
    assert_eq!(transport.calls[0].0, "member.external_renew");
    assert_eq!(transport.calls[0].1["employee_id"], "@ada");
    assert_eq!(transport.calls[0].1["ttl_secs"], MAX_LEASE_TTL_SECS);
    let parsed: Value = serde_json::from_str(&out.stdout.text()).unwrap();
    assert_eq!(parsed["expires_at"], "2026-12-04T00:00:00.000Z");

    // Positional @handle + explicit --ttl 24h + --session + quiet mode.
    let args = RenewArgs::try_parse_from([
        "agentic-org agent renew",
        "@ada",
        "--ttl",
        "24h",
        "--session",
        "ext-0193a5c0-0000-7000-8000-000000000070",
    ])
    .unwrap();
    let (mut ctx, out) = captured(
        Modes {
            quiet: true,
            ..Modes::default()
        },
        tty(Some(80)),
        &[],
    );
    execute_with_transport(&args, &mut transport, &mut ctx).unwrap();
    assert_eq!(transport.calls[1].1["ttl_secs"], 86_400);
    assert_eq!(
        transport.calls[1].1["session_id"],
        "ext-0193a5c0-0000-7000-8000-000000000070"
    );
    assert_eq!(
        out.stdout.text(),
        "ext-0193a5c0-0000-7000-8000-000000000070\n"
    );
}

#[test]
fn renew_rejects_missing_member_and_invalid_ttl_before_rpc() {
    let mut transport = FakeTransport {
        calls: Vec::new(),
        refuse: None,
    };
    let (mut ctx, _) = captured(Modes::default(), Terminal::piped(), &[]);
    let missing = RenewArgs::try_parse_from(["agentic-org agent renew"]).unwrap();
    let err = execute_with_transport(&missing, &mut transport, &mut ctx).unwrap_err();
    assert_eq!(err.exit_code(), 2);

    for bad_ttl in ["60s", "8d", "forever"] {
        let args = RenewArgs::try_parse_from([
            "agentic-org agent renew",
            "--member",
            "@ada",
            "--ttl",
            bad_ttl,
        ])
        .unwrap();
        let err = execute_with_transport(&args, &mut transport, &mut ctx).unwrap_err();
        assert_eq!(err.exit_code(), 1, "{bad_ttl}");
    }
    assert!(transport.calls.is_empty());

    let (mut ctx, out) = captured(Modes::default(), Terminal::piped(), &[]);
    assert!(parse_renew_args(&words(&["--help"]), &mut ctx)
        .unwrap()
        .is_none());
    assert!(out.stdout.text().contains("agentic-org agent renew"));
}

#[test]
fn renew_surfaces_server_refusal_cleanly() {
    let args = RenewArgs::try_parse_from(["agentic-org agent renew", "--member", "@ada"]).unwrap();
    let mut transport = FakeTransport {
        calls: Vec::new(),
        refuse: Some(Refusal::new(
            ErrorCode::NotAuthorized,
            "not allowed",
            "sign in as the owner",
        )),
    };
    let (mut ctx, _) = captured(Modes::default(), Terminal::piped(), &[]);
    let err = execute_with_transport(&args, &mut transport, &mut ctx).unwrap_err();
    assert_eq!(err.exit_code(), 1);
    assert_eq!(err.code(), "office::not_authorized");
}
