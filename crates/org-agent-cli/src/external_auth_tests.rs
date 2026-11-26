//! Tests of the external lifecycle's argument handling, envelopes and operator rows.

use org_command::Refusal;
use org_proto::ErrorCode;
use serde_json::{json, Value};

use super::client::parse_ttl;
use super::operator::rewrite_positional;
use super::{failure, run_lifecycle, success, Flags};
use crate::transport::{OfficeTransport, TransportOffice};

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn flags_take_values_switches_and_a_command_after_the_separator() {
    let parsed = Flags::parse(
        &words(&[
            "--as",
            "@ada",
            "--ttl=8h",
            "--takeover",
            "--",
            "claude",
            "--flag",
        ]),
        &["as", "ttl"],
        &["takeover"],
        "usage",
    )
    .unwrap();
    assert_eq!(parsed.value("as"), Some("@ada"));
    assert_eq!(parsed.value("ttl"), Some("8h"));
    assert!(parsed.switch("takeover"));
    assert_eq!(parsed.rest, words(&["claude", "--flag"]));
    for bad in [
        &["bare"][..],
        &["--unknown"],
        &["--as"],
        &["--as", "a", "--as", "b"],
        &["--takeover=yes"],
    ] {
        let err = Flags::parse(&words(bad), &["as"], &["takeover"], "u").err();
        let err = err.unwrap_or_else(|| panic!("{bad:?} should be refused"));
        assert_eq!(err.code, ErrorCode::InvalidRequest);
        assert_eq!(failure(&err).exit_code, 2, "{bad:?}");
    }
}

#[test]
fn durations_parse_with_units_and_refuse_nonsense() {
    assert_eq!(parse_ttl("90").unwrap(), 90);
    assert_eq!(parse_ttl("90s").unwrap(), 90);
    assert_eq!(parse_ttl("45m").unwrap(), 2_700);
    assert_eq!(parse_ttl("24h").unwrap(), 86_400);
    assert_eq!(parse_ttl("7D").unwrap(), 604_800);
    for bad in [
        "",
        "h",
        "0h",
        "-1h",
        "1w",
        "1.5h",
        "99999999999999999999d",
        "1hé",
    ] {
        assert!(parse_ttl(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn operator_shorthand_rewrites_to_named_flags_only_for_the_external_verbs() {
    assert_eq!(
        rewrite_positional(&words(&["agent", "approve", "ABCD-EFGH", "--ttl", "8h"])),
        Some(words(&[
            "agent",
            "approve",
            "--code",
            "ABCD-EFGH",
            "--ttl",
            "8h"
        ]))
    );
    assert_eq!(
        rewrite_positional(&words(&["agent", "sessions", "@ada"])),
        Some(words(&["agent", "sessions", "--member", "@ada"]))
    );
    assert_eq!(
        rewrite_positional(&words(&["agent", "revoke", "@ada", "ext-1"])),
        Some(words(&[
            "agent",
            "revoke",
            "--member",
            "@ada",
            "--session",
            "ext-1"
        ]))
    );
    for unchanged in [
        &["agent", "approve", "--code", "X"][..],
        &["agent", "pause", "@ada"],
        &["agent", "revoke", "a", "b", "c"],
        &["task", "approve", "x"],
        &["agent"],
        &["agent", "approve", "--format", "json"],
        &["agent", "approve", "--ttl", "8h"],
    ] {
        assert_eq!(rewrite_positional(&words(unchanged)), None, "{unchanged:?}");
    }
}

/// B1: `agentic-org` puts `--format json|compact` (and `--dry-run`) right after the command
/// path, before the caller's bare words. The shorthand still reads them, and never takes a
/// flag's value for a bare word.
#[test]
fn operator_shorthand_reads_bare_words_after_the_flags_a_caller_added() {
    for (line, expected) in [
        (
            &["agent", "approve", "--format", "json", "ABCD-EFGH"][..],
            &[
                "agent",
                "approve",
                "--code",
                "ABCD-EFGH",
                "--format",
                "json",
            ][..],
        ),
        (
            &[
                "agent",
                "approve",
                "--format",
                "compact",
                "ABCD-EFGH",
                "--ttl",
                "8h",
                "--takeover",
            ],
            &[
                "agent",
                "approve",
                "--code",
                "ABCD-EFGH",
                "--format",
                "compact",
                "--ttl",
                "8h",
                "--takeover",
            ],
        ),
        (
            &["agent", "approve", "--ttl", "8h", "K7QF-9XM2", "--dry-run"],
            &[
                "agent",
                "approve",
                "--code",
                "K7QF-9XM2",
                "--ttl",
                "8h",
                "--dry-run",
            ],
        ),
        (
            &[
                "agent", "sessions", "--format", "json", "@ada", "--limit", "50",
            ],
            &[
                "agent",
                "sessions",
                "--member",
                "@ada",
                "--format",
                "json",
                "--history-limit",
                "50",
            ],
        ),
        (
            &["agent", "sessions", "--member", "@ada", "--limit=5"],
            &["agent", "sessions", "--member", "@ada", "--history-limit=5"],
        ),
        (
            &["agent", "revoke", "--format", "compact", "@ada", "ext-1"],
            &[
                "agent",
                "revoke",
                "--member",
                "@ada",
                "--session",
                "ext-1",
                "--format",
                "compact",
            ],
        ),
        (
            &["agent", "revoke", "--fields", "revoked", "@ada"],
            &["agent", "revoke", "--member", "@ada", "--fields", "revoked"],
        ),
        // Nothing after `--` is read: it stays for the table to refuse as written.
        (
            &["agent", "approve", "X", "--", "Y"],
            &["agent", "approve", "--code", "X", "--", "Y"],
        ),
    ] {
        assert_eq!(
            rewrite_positional(&words(line)),
            Some(words(expected)),
            "{line:?}"
        );
    }
}

/// `agent-os exec` approves through `agentic-org` with named flags only.
#[test]
fn exec_approves_with_named_flags_only() {
    use super::operator_approve_args;
    assert_eq!(
        operator_approve_args("ABCD-EFGH", 3_600, false),
        words(&["agent", "approve", "--code", "ABCD-EFGH", "--ttl", "3600s"])
    );
    assert_eq!(
        operator_approve_args("ABCD-EFGH", 60, true)
            .last()
            .map(String::as_str),
        Some("--takeover")
    );
}

#[test]
fn lifecycle_help_runs_without_a_binding_and_other_verbs_pass_through() {
    for verb in ["login", "whoami", "logout", "exec"] {
        let (stdout, code) = run_lifecycle(&words(&[verb, "--help"])).unwrap();
        assert_eq!(code, 0, "{verb}");
        assert!(stdout.starts_with("Usage: agent-os "), "{verb}: {stdout}");
        assert!(stdout.contains("Use when:"), "{verb}: {stdout}");
    }
    assert!(run_lifecycle(&words(&["task", "list"])).is_none());
    let (stdout, code) = run_lifecycle(&words(&["login"])).unwrap();
    assert_eq!(code, 2);
    let env: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(env["ok"], false);
    assert_eq!(env["error"]["code"], "invalid_request");
    let (_, code) = run_lifecycle(&words(&["exec", "--as", "@ada"])).unwrap();
    assert_eq!(code, 2, "exec without a command is a usage error");
}

#[test]
fn envelopes_keep_the_shared_field_order() {
    let ok = success(&json!({"member_id": "m"}));
    assert_eq!(ok.exit_code, 0);
    assert!(ok.stdout.starts_with("{\"ok\":true,\"data\":{"));
    let parsed: Value = serde_json::from_str(&ok.stdout).unwrap();
    assert_eq!(parsed["data"]["member_id"], "m");
    let refused = failure(&Refusal::new(ErrorCode::NotAuthorized, "no", "hint"));
    assert_eq!(refused.exit_code, 1);
}

struct Recording {
    calls: Vec<(String, Value)>,
}

impl OfficeTransport for Recording {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        self.calls.push((method.to_owned(), params));
        Ok(match method {
            "member.external_sessions" => json!({"employee_id": "e", "history": []}),
            "member.external_revoke" => json!({"employee_id": "e", "revoked": true}),
            _ => json!({
                "session_id": "ext-1", "employee_id": "e", "workspace_label": "w",
                "pubkey_fingerprint": "f", "approved_by": "o", "approved_at": "a",
                "expires_at": "x", "active": true
            }),
        })
    }
}

#[test]
fn operator_rows_send_the_external_calls() {
    let mut office = TransportOffice(Recording { calls: Vec::new() });
    let out = crate::execute_with_office(
        "agentic-org",
        &words(&["agent", "approve", "abcd-efgh", "--ttl", "8h", "--takeover"]),
        &mut office,
    );
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    let (method, params) = &office.0.calls[0];
    assert_eq!(method, "member.external_approve");
    assert_eq!(params["code"], "abcd-efgh");
    assert_eq!(params["ttl_secs"], 28_800);
    assert_eq!(params["takeover"], true);
    assert!(params["command_id"].is_string());

    let out = crate::execute_with_office(
        "agentic-org",
        &words(&["agent", "sessions", "@ada"]),
        &mut office,
    );
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    assert_eq!(office.0.calls[1].1, json!({"employee_id": "@ada"}));

    let out = crate::execute_with_office(
        "agentic-org",
        &words(&["agent", "revoke", "@ada", "ext-9"]),
        &mut office,
    );
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    assert_eq!(office.0.calls[2].0, "member.external_revoke");
    assert_eq!(office.0.calls[2].1["session_id"], "ext-9");

    let bad = crate::execute_with_office(
        "agentic-org",
        &words(&["agent", "approve", "X", "--ttl", "forever"]),
        &mut office,
    );
    assert_ne!(bad.exit_code, 0);
    assert_eq!(
        office.0.calls.len(),
        3,
        "a bad --ttl never reaches the office"
    );
}

#[test]
fn top_level_help_lists_the_workspace_sign_in_verbs() {
    let out = crate::execute_with_office(
        "agent-os",
        &words(&["--help"]),
        &mut TransportOffice(Recording { calls: Vec::new() }),
    );
    for verb in ["login", "whoami", "logout", "exec"] {
        assert!(
            out.stdout.contains(&format!("\n  {verb} ")),
            "{verb}: {}",
            out.stdout
        );
    }
}
