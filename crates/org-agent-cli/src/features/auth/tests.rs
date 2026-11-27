//! Tests of the external lifecycle's argument handling, exit codes and envelopes.

use org_command::Refusal;
use org_proto::ErrorCode;
use serde_json::{json, Value};

use super::{
    failure, operator_approve_args, run_lifecycle, success, Flags, LifecycleError, LoginHooks,
};
use crate::runtime::transport::{OfficeTransport, TransportOffice};

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

fn no_briefing(_: &mut dyn OfficeTransport) -> Result<Value, Refusal> {
    Ok(Value::Null)
}

const HOOKS: LoginHooks = LoginHooks {
    briefing: no_briefing,
};

fn run(args: &[String]) -> Option<(String, u8)> {
    run_lifecycle(args, &HOOKS)
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
        assert!(matches!(err, LifecycleError::Usage(_)), "{bad:?}");
        assert_eq!(err.refusal().code, ErrorCode::InvalidRequest);
        assert_eq!(failure(&err).exit_code, 2, "{bad:?}");
    }
}

/// `agent-os exec` approves through `agentic-org` with named flags only.
#[test]
fn exec_approves_with_named_flags_only() {
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
        let (stdout, code) = run(&words(&[verb, "--help"])).unwrap();
        assert_eq!(code, 0, "{verb}");
        assert!(stdout.starts_with("Usage: agent-os "), "{verb}: {stdout}");
        assert!(stdout.contains("Use when:"), "{verb}: {stdout}");
    }
    assert!(run(&words(&["task", "list"])).is_none());
    let (stdout, code) = run(&words(&["login"])).unwrap();
    assert_eq!(code, 2);
    let env: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(env["ok"], false);
    assert_eq!(env["error"]["code"], "invalid_request");
    let (_, code) = run(&words(&["exec", "--as", "@ada"])).unwrap();
    assert_eq!(code, 2, "exec without a command is a usage error");

    // P1-5: bare `--json` before OR after lifecycle verbs works cleanly (and `--json` after `--`
    // in `exec` stays untouched for the wrapped child command).
    for verb in ["login", "whoami", "logout", "exec"] {
        for line in [
            vec!["--json", verb, "--help"],
            vec![verb, "--json", "--help"],
        ] {
            let (stdout, code) = run(&words(&line)).unwrap();
            assert_eq!(code, 0, "{line:?}: {stdout}");
            assert!(stdout.starts_with("Usage: agent-os "), "{line:?}: {stdout}");
        }
    }
    let parsed_exec = Flags::parse(
        &words(&["--as", "@ada", "--json", "--", "claude", "--json"]),
        &["as", "ttl", "office", "socket"],
        &["takeover", "json"],
        "usage",
    )
    .unwrap();
    assert!(parsed_exec.switch("json"));
    assert_eq!(parsed_exec.rest, words(&["claude", "--json"]));

    // Duplicate `--json` before `--` or `--json=<val>` is refused with exit 2.
    for bad in [
        &["--json", "whoami", "--json"][..],
        &["whoami", "--json", "--json"],
        &["whoami", "--json=true"],
        &["--json", "logout", "--json"],
        &["--json", "login", "--as", "@ada", "--json"],
        &["--json", "exec", "--as", "@ada", "--json", "--", "true"],
    ] {
        let (stdout, code) = run(&words(bad)).unwrap();
        assert_eq!(code, 2, "{bad:?}: {stdout}");
        let env: Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(env["ok"], false, "{bad:?}");
        assert_eq!(env["error"]["code"], "invalid_request", "{bad:?}");
    }
}

#[test]
fn envelopes_keep_the_shared_field_order() {
    let ok = success(&json!({"member_id": "m"}));
    assert_eq!(ok.exit_code, 0);
    assert!(ok.stdout.starts_with("{\"ok\":true,\"data\":{"));
    let parsed: Value = serde_json::from_str(&ok.stdout).unwrap();
    assert_eq!(parsed["data"]["member_id"], "m");
    let refused = failure(&LifecycleError::Failed(Refusal::new(
        ErrorCode::NotAuthorized,
        "no",
        "hint",
    )));
    assert_eq!(refused.exit_code, 1);
}

#[test]
fn a_lifecycle_answer_escapes_controls_line_separators_and_bidi_overrides() {
    // CVE-2021-25743 class: a value the office or a workspace path carries can never reorder or
    // split the line a terminal or a line-reading harness shows.
    let hostile = "ws\u{7f}\u{2028}\u{202e}gpj.exe";
    let printed = success(&json!({ "workspace_root": hostile }));
    assert_eq!(printed.exit_code, 0);
    let line = printed.stdout.strip_suffix('\n').unwrap();
    assert!(!line.contains('\n'));
    for raw in ['\u{7f}', '\u{2028}', '\u{202e}'] {
        assert!(!line.contains(raw), "{raw:?} printed raw: {line}");
    }
    assert!(line.contains("ws\\u007f\\u2028\\u202egpj.exe"), "{line}");
    let parsed: Value = serde_json::from_str(line).unwrap();
    assert_eq!(parsed["data"]["workspace_root"], hostile);
    assert_eq!(
        printed.stdout,
        format!(
            "{}\n",
            org_command::success_envelope_json(&json!({ "workspace_root": hostile }))
        )
    );
}

#[test]
fn the_exit_code_follows_the_error_kind_never_its_message() {
    let usage = LifecycleError::usage("no Usage prefix here", "hint");
    assert_eq!(failure(&usage).exit_code, 2);
    let refused = LifecycleError::Failed(Refusal::new(
        ErrorCode::InvalidRequest,
        "Usage: a refusal that only reads like a usage error",
        "hint",
    ));
    assert_eq!(failure(&refused).exit_code, 1);
    let from: LifecycleError = Refusal::new(ErrorCode::Timeout, "slow", "wait").into();
    assert_eq!(from.exit_code(), 1);
    let (stdout, code) = run(&words(&["login", "--as", "@ada", "--wait", "soon"])).unwrap();
    assert_eq!(code, 2, "{stdout}");
    let (stdout, code) = run(&words(&[
        "exec", "--as", "@ada", "--ttl", "forever", "--", "x",
    ]))
    .unwrap();
    assert_eq!(
        code, 1,
        "a bad duration is a refusal, as it always was: {stdout}"
    );
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

#[test]
fn client_sanitizers_reject_bidi_line_separators_and_omit_oversized_or_unclean_workspace_paths() {
    use std::path::Path;

    use org_proto::external_auth::MAX_WORKSPACE_PATH_BYTES;

    use super::client::{optional_workspace_path, sanitize_hostname};

    assert_eq!(
        sanitize_hostname("workstation\n").as_deref(),
        Some("workstation")
    );
    assert_eq!(sanitize_hostname("  mac-1\r\n").as_deref(), Some("mac-1"));
    for ch in [
        '\u{2028}', '\u{2029}', '\u{200E}', '\u{200F}', '\u{202A}', '\u{202E}', '\u{2066}',
        '\u{2069}', '\x1b',
    ] {
        assert_eq!(sanitize_hostname(&format!("host{ch}name")), None);
        assert_eq!(sanitize_hostname(&format!("hostname{ch}")), None);
    }
    assert_eq!(sanitize_hostname("host\nname"), None);
    assert_eq!(sanitize_hostname(&"a".repeat(256)), None);

    assert_eq!(
        optional_workspace_path(Path::new("/")).as_deref(),
        Some("/")
    );
    assert_eq!(
        optional_workspace_path(Path::new("/Users/ada/repo")).as_deref(),
        Some("/Users/ada/repo")
    );
    let exact = format!("/{}", "a".repeat(MAX_WORKSPACE_PATH_BYTES - 1));
    assert_eq!(exact.len(), MAX_WORKSPACE_PATH_BYTES);
    assert_eq!(
        optional_workspace_path(Path::new(&exact)).as_deref(),
        Some(exact.as_str())
    );
    let too_long = format!("/{}", "a".repeat(MAX_WORKSPACE_PATH_BYTES));
    assert_eq!(optional_workspace_path(Path::new(&too_long)), None);
    let multibyte_too_long = format!("/{}", "é".repeat(MAX_WORKSPACE_PATH_BYTES / 2));
    assert!(multibyte_too_long.len() > MAX_WORKSPACE_PATH_BYTES);
    assert_eq!(
        optional_workspace_path(Path::new(&multibyte_too_long)),
        None
    );
    for bad in [
        "",
        "relative/repo",
        "./repo",
        "/Users/ada/a/../b",
        "/Users/ada/./repo",
        "/Users/ada/repo/",
        "//",
        "/Users//ada/repo",
        "/Users/ada/re\u{202E}po",
        "/Users/ada/repo\u{2028}",
    ] {
        assert_eq!(
            optional_workspace_path(Path::new(bad)),
            None,
            "workspace_path should be omitted: {bad:?}"
        );
    }
}

#[test]
fn not_signed_in_hint_mentions_both_agent_os_login_and_agentic_org() {
    let refusal = super::not_signed_in();
    assert_eq!(refusal.code, ErrorCode::NotAuthorized);
    assert!(
        refusal.hint.contains("agent-os login --as @<handle>"),
        "{}",
        refusal.hint
    );
    assert!(refusal.hint.contains("agentic-org"), "{}", refusal.hint);
}
