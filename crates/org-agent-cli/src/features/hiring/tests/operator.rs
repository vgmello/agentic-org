//! The operator rows `agent approve|sessions|revoke`: their positional shorthand and the calls
//! they send.

use org_command::Refusal;
use serde_json::{json, Value};

use crate::runtime::transport::{OfficeTransport, TransportOffice};

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

fn rewrite(args: &[String]) -> Option<Vec<String>> {
    super::super::rewrite_positional(crate::registry::runtime_table(), args)
}

#[test]
fn operator_shorthand_rewrites_to_named_flags_only_for_the_external_verbs() {
    assert_eq!(
        rewrite(&words(&["agent", "approve", "ABCD-EFGH", "--ttl", "8h"])),
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
        rewrite(&words(&["agent", "sessions", "@ada"])),
        Some(words(&["agent", "sessions", "--member", "@ada"]))
    );
    assert_eq!(
        rewrite(&words(&["agent", "revoke", "@ada", "ext-1"])),
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
        assert_eq!(rewrite(&words(unchanged)), None, "{unchanged:?}");
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
        assert_eq!(rewrite(&words(line)), Some(words(expected)), "{line:?}");
    }
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
