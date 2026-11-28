//! Tests for `agentic-org agent sessions [--member @<handle>] [--pending]` grammar, flags,
//! pending table/JSON/quiet output, workspace familiarity, and refusal handling.

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
        if method == "member.external_pending" {
            return Ok(json!({
                "pending": [{
                    "request_id": "req-0193a5c0-0000-7000-8000-000000000081",
                    "handle": "@ada",
                    "employee_id": "0193a5c0-0000-7000-8000-000000000051",
                    "office_id": "0193a5c0-0000-7000-8000-000000000001",
                    "office_name": "HQ",
                    "hostname": "dev-box",
                    "workspace_path": "/home/ada/repo",
                    "workspace_label": "~/repo",
                    "pubkey_fingerprint": "3f9c2a7d10b4e8f1",
                    "managed_turn_active": false,
                    "requested_at": "2026-11-27T09:00:00.000Z",
                    "expires_at": "2026-11-27T09:10:00.000Z",
                    "code_prefix": "K7QF-••••",
                    "workspace_familiarity": "returning_known_key",
                    "last_approved_by": "0193a5c0-0000-7000-8000-000000000002",
                    "last_approved_at": "2026-11-26T10:00:00.000Z"
                }]
            }));
        }
        Ok(json!({
            "employee_id": "0193a5c0-0000-7000-8000-000000000051",
            "active": {
                "session_id": "ext-0193a5c0-0000-7000-8000-000000000070",
                "employee_id": "0193a5c0-0000-7000-8000-000000000051",
                "handle": "ada",
                "workspace_label": "~/repo",
                "pubkey_fingerprint": "3f9c2a7d10b4e8f1",
                "approved_by": "0193a5c0-0000-7000-8000-000000000002",
                "approved_at": "2026-11-27T00:00:00.000Z",
                "expires_at": "2026-11-28T00:00:00.000Z",
                "active": true
            },
            "history": [{
                "at": "2026-11-27T00:00:00.000Z",
                "event": "approved",
                "session_id": "ext-0193a5c0-0000-7000-8000-000000000070",
                "actor_id": "0193a5c0-0000-7000-8000-000000000002"
            }]
        }))
    }
}

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn sessions_grammar_is_consistent_and_help_has_use_when_and_examples() {
    command().debug_assert();
    let help = command().render_long_help().to_string();
    assert!(help.contains("Use when:"), "{help}");
    assert!(help.contains("Examples:"), "{help}");
    assert!(help.contains("--pending"), "{help}");
    assert!(help.contains("--member"), "{help}");
    assert!(help.contains("--limit"), "{help}");
}

#[test]
fn sessions_pending_lists_office_wide_and_filtered_in_json_tty_and_quiet_modes() {
    let mut transport = FakeTransport {
        calls: Vec::new(),
        refuse: None,
    };

    // 1. Office-wide --pending --json
    let args = SessionsArgs::try_parse_from(["agentic-org agent sessions", "--pending"]).unwrap();
    let (mut ctx, out) = captured(
        Modes {
            json: true,
            ..Modes::default()
        },
        Terminal::piped(),
        &[],
    );
    execute_with_transport(&args, &mut transport, &mut ctx).unwrap();
    assert_eq!(transport.calls[0].0, "member.external_pending");
    assert_eq!(transport.calls[0].1, json!({}));
    let parsed: Value = serde_json::from_str(&out.stdout.text()).unwrap();
    let item = &parsed["pending"][0];
    assert_eq!(item["code_prefix"], "K7QF-••••");
    assert_eq!(item["workspace_familiarity"], "returning_known_key");
    assert_eq!(item["handle"], "@ada");

    // 2. Filtered by --member @ada in TTY table mode
    let args = SessionsArgs::try_parse_from([
        "agentic-org agent sessions",
        "--pending",
        "--member",
        "@ada",
    ])
    .unwrap();
    let (mut ctx, out) = captured(Modes::default(), tty(Some(180)), &[("NO_COLOR", "1")]);
    execute_with_transport(&args, &mut transport, &mut ctx).unwrap();
    assert_eq!(transport.calls[1].1, json!({"employee_id": "@ada"}));
    let shown = out.stdout.text();
    assert!(shown.contains("K7QF-••••"), "{shown}");
    assert!(shown.contains("returning_known_key"), "{shown}");
    assert!(shown.contains("dev-box"), "{shown}");

    // 3. Quiet mode prints only request_id
    let (mut ctx, out) = captured(
        Modes {
            quiet: true,
            ..Modes::default()
        },
        tty(Some(80)),
        &[],
    );
    execute_with_transport(&args, &mut transport, &mut ctx).unwrap();
    assert_eq!(
        out.stdout.text(),
        "req-0193a5c0-0000-7000-8000-000000000081\n"
    );
}

#[test]
fn sessions_without_pending_requires_member_and_reads_active_lease_and_history() {
    let mut transport = FakeTransport {
        calls: Vec::new(),
        refuse: None,
    };
    let (mut ctx, _) = captured(Modes::default(), Terminal::piped(), &[]);
    let missing = SessionsArgs::try_parse_from(["agentic-org agent sessions"]).unwrap();
    let err = execute_with_transport(&missing, &mut transport, &mut ctx).unwrap_err();
    assert_eq!(err.exit_code(), 2);

    let limit_with_pending =
        SessionsArgs::try_parse_from(["agentic-org agent sessions", "--pending", "--limit", "10"])
            .unwrap();
    let err = execute_with_transport(&limit_with_pending, &mut transport, &mut ctx).unwrap_err();
    assert_eq!(err.exit_code(), 2);
    assert!(transport.calls.is_empty());

    let member_args =
        SessionsArgs::try_parse_from(["agentic-org agent sessions", "@ada", "--limit", "50"])
            .unwrap();
    let (mut ctx, out) = captured(
        Modes {
            quiet: true,
            ..Modes::default()
        },
        Terminal::piped(),
        &[],
    );
    execute_with_transport(&member_args, &mut transport, &mut ctx).unwrap();
    assert_eq!(transport.calls[0].0, "member.external_sessions");
    assert_eq!(
        transport.calls[0].1,
        json!({"employee_id": "@ada", "limit": 50})
    );
    assert_eq!(
        out.stdout.text(),
        "ext-0193a5c0-0000-7000-8000-000000000070\n"
    );

    let (mut ctx, out) = captured(Modes::default(), Terminal::piped(), &[]);
    assert!(parse_sessions_args(&words(&["--help"]), &mut ctx)
        .unwrap()
        .is_none());
    assert!(out.stdout.text().contains("agentic-org agent sessions"));
}

#[test]
fn sessions_surfaces_not_authorized_refusal() {
    let args = SessionsArgs::try_parse_from(["agentic-org agent sessions", "--pending"]).unwrap();
    let mut transport = FakeTransport {
        calls: Vec::new(),
        refuse: Some(Refusal::new(
            ErrorCode::NotAuthorized,
            "human-only",
            "sign in as a person",
        )),
    };
    let (mut ctx, _) = captured(Modes::default(), Terminal::piped(), &[]);
    let err = execute_with_transport(&args, &mut transport, &mut ctx).unwrap_err();
    assert_eq!(err.exit_code(), 1);
    assert_eq!(err.code(), "office::not_authorized");
}

#[test]
fn sessions_yaml_emits_a_single_document_and_accepts_post_subcommand_format() {
    use org_cli_shell::context::OutputFormat;

    let mut transport = FakeTransport {
        calls: Vec::new(),
        refuse: None,
    };
    let member_args =
        SessionsArgs::try_parse_from(["agentic-org agent sessions", "@ada", "--format", "yaml"])
            .unwrap();
    assert_eq!(member_args.format, Some(FormatArg::Yaml));

    let (mut ctx, out) = captured(
        Modes {
            format: Some(OutputFormat::Yaml),
            ..Modes::default()
        },
        tty(Some(80)),
        &[],
    );
    execute_with_transport(&member_args, &mut transport, &mut ctx).unwrap();
    let yaml = out.stdout.text();
    let top_level_keys: Vec<&str> = yaml
        .lines()
        .filter(|line| !line.starts_with(' ') && !line.is_empty())
        .map(|line| line.split(':').next().unwrap_or_default())
        .collect();
    assert_eq!(
        top_level_keys,
        vec!["active", "employee_id", "history"],
        "YAML must be printed once as a single valid document (no duplicate copy):\n{yaml}"
    );
    assert!(
        yaml.contains("0193a5c0-0000-7000-8000-000000000051")
            && yaml.contains("ext-0193a5c0-0000-7000-8000-000000000070"),
        "{yaml}"
    );

    // Post-subcommand `--format` accepts all formats; `--json --format json` is idempotent;
    // `--json --format yaml` and `-q --format yaml` are refused (exit 2).
    for fmt in ["json", "yaml", "compact", "tsv", "table"] {
        let parsed = SessionsArgs::try_parse_from([
            "agentic-org agent sessions",
            "--pending",
            "--format",
            fmt,
        ])
        .unwrap();
        assert!(parsed.format.is_some(), "{fmt}");
    }
    let (mut ctx, _) = captured(Modes::default(), Terminal::piped(), &[]);
    assert!(parse_sessions_args(
        &words(&["--pending", "--json", "--format", "json"]),
        &mut ctx
    )
    .is_ok());
    let (mut ctx, _) = captured(Modes::default(), Terminal::piped(), &[]);
    let err = parse_sessions_args(
        &words(&["--pending", "--json", "--format", "yaml"]),
        &mut ctx,
    )
    .unwrap_err();
    assert_eq!(err.exit_code(), 2);
}

#[test]
fn sessions_member_uuid_succeeds_and_unauthorized_caller_preserves_accurate_error_and_hint() {
    let ada_id = "0193a5c0-0000-7000-8000-000000000051";
    let mut transport = FakeTransport {
        calls: Vec::new(),
        refuse: None,
    };
    let args =
        SessionsArgs::try_parse_from(["agentic-org agent sessions", "--member", ada_id, "--json"])
            .unwrap();
    let (mut ctx, out) = captured(
        Modes {
            json: true,
            ..Modes::default()
        },
        Terminal::piped(),
        &[],
    );
    execute_with_transport(&args, &mut transport, &mut ctx).unwrap();
    assert_eq!(transport.calls[0].0, "member.external_sessions");
    assert_eq!(transport.calls[0].1, json!({"employee_id": ada_id}));
    let parsed: Value = serde_json::from_str(&out.stdout.text()).unwrap();
    assert_eq!(parsed["employee_id"], ada_id);
    assert_eq!(
        parsed["active"]["session_id"],
        "ext-0193a5c0-0000-7000-8000-000000000070"
    );

    // Accurate authorization refusal + actionable hint flow through (never opaque host refusal).
    let mut denied_transport = FakeTransport {
        calls: Vec::new(),
        refuse: Some(Refusal::new(
            ErrorCode::NotAuthorized,
            "only the owner, a person who may hire, or the agent's manager or team lead may manage its external sessions",
            "Ask the organization owner, a person with hiring access, or the agent's manager or team lead to inspect or manage this agent's external sessions.",
        )),
    };
    let (mut ctx, _) = captured(Modes::default(), Terminal::piped(), &[]);
    let err = execute_with_transport(&args, &mut denied_transport, &mut ctx).unwrap_err();
    assert_eq!(err.exit_code(), 1);
    assert_eq!(err.code(), "office::not_authorized");
    let rendered = err.to_string();
    assert!(
        rendered.contains("owner") && rendered.contains("team lead"),
        "expected accurate authorization error, got: {rendered}"
    );
    assert!(
        !rendered.contains("request refused on host channel"),
        "must not collapse into opaque host channel refusal: {rendered}"
    );
    assert!(
        err.help()
            .is_some_and(|h| h.contains("organization owner") && h.contains("team lead")),
        "expected actionable hint, got: {:?}",
        err.help()
    );
}
