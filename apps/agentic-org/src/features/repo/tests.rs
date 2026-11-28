//! Unit tests for operator-local `repo` credential commands (`TASK-135`).

use std::io::Cursor;

use clap::Parser;
use org_agent_cli::OfficeTransport;
use org_cli_shell::context::testing::captured;
use org_cli_shell::context::Modes;
use org_cli_shell::terminal::Terminal;
use org_command::Refusal;
use serde_json::{json, Value};
use zeroize::Zeroizing;

use super::credential_status::execute_status;
use super::github::{self, GitHubAction};
use super::revoke_credential::execute_revoke;
use super::set_credential::{execute_set, read_token, read_token_from_reader, SetCredentialArgs};
use super::{is_local_verb, reads_a_secret};

struct StubTransport {
    calls: Vec<(String, Value)>,
    reply: Value,
}

impl OfficeTransport for StubTransport {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        self.calls.push((method.to_owned(), params));
        Ok(self.reply.clone())
    }
}

#[test]
fn local_verb_detection_and_secret_prompt_flag_match_credential_verbs() {
    let s = |v: &[&str]| v.iter().map(|x| (*x).to_owned()).collect::<Vec<_>>();
    assert!(is_local_verb(&s(&["set-credential", "--repo", "app"])));
    assert!(is_local_verb(&s(&["credential-status", "--repo", "app"])));
    assert!(is_local_verb(&s(&["revoke-credential", "--repo", "app"])));
    assert!(!is_local_verb(&s(&["register", "--key", "app"])));
    assert!(!is_local_verb(&s(&["fetch", "--repo", "app"])));
    assert!(reads_a_secret(&s(&["set-credential"])));
    assert!(!reads_a_secret(&s(&["credential-status"])));
    assert!(!reads_a_secret(&s(&["revoke-credential"])));
}

#[test]
fn set_credential_reads_regular_token_file_and_refuses_symlinks_and_no_input() {
    let dir = tempfile::tempdir().unwrap();
    let token_file = dir.path().join("tok.txt");
    std::fs::write(&token_file, "ghp_task135_secret_token_999\n").unwrap();
    let link_file = dir.path().join("tok.link");
    org_platform::fs::create_symlink(&token_file, &link_file).unwrap();

    let (ctx, _) = captured(Modes::default(), Terminal::piped(), &[]);
    let ok_args = SetCredentialArgs::try_parse_from([
        "agentic-org repo set-credential",
        "--repo",
        "app",
        "--token-file",
        token_file.to_str().unwrap(),
    ])
    .unwrap();
    let tok = read_token(&ok_args, &ctx).unwrap();
    assert_eq!(tok.as_str(), "ghp_task135_secret_token_999");

    let link_args = SetCredentialArgs::try_parse_from([
        "agentic-org repo set-credential",
        "--repo",
        "app",
        "--token-file",
        link_file.to_str().unwrap(),
    ])
    .unwrap();
    let err = read_token(&link_args, &ctx).unwrap_err();
    assert!(err.to_string().contains("symlinks are refused"), "{err}");

    let no_source_args =
        SetCredentialArgs::try_parse_from(["agentic-org repo set-credential", "--repo", "app"])
            .unwrap();
    let err = read_token(&no_source_args, &ctx).unwrap_err();
    assert!(err.to_string().contains("required"), "{err}");

    // Stdin reader rejects empty, control characters, and >4096-byte inputs.
    assert!(read_token_from_reader(Cursor::new(b"\n"), "--token-stdin").is_err());
    assert!(read_token_from_reader(Cursor::new(b"abc\0def"), "--token-stdin").is_err());
    assert!(read_token_from_reader(Cursor::new(vec![b'a'; 4097]), "--token-stdin").is_err());
    assert_eq!(
        read_token_from_reader(Cursor::new(b"tok_ok\r\n"), "--token-stdin")
            .unwrap()
            .as_str(),
        "tok_ok"
    );
}

#[test]
fn repo_credential_verbs_call_expected_rpcs_and_never_echo_secret_tokens() {
    let secret = "ghp_SUPER_SECRET_NEVER_ECHO_12345";
    let reply = json!({
        "office_id": "0193a5c0-0000-7000-8000-000000000001",
        "repository_id": "0193a5c0-0000-7000-8000-000000000099",
        "repository_key": "app",
        "configured": true,
        "source": "vault",
        "scope_url": "https://github.com/acme/app.git",
        "username": "x-access-token",
        "updated_at": "2026-11-27T19:00:00Z"
    });
    let mut transport = StubTransport {
        calls: Vec::new(),
        reply,
    };
    let modes = Modes {
        json: true,
        ..Default::default()
    };
    let (mut ctx, streams) = captured(modes, Terminal::piped(), &[]);

    execute_set(
        "app",
        Some("x-access-token".to_owned()),
        Zeroizing::new(secret.to_owned()),
        &mut transport,
        &mut ctx,
    )
    .unwrap();
    execute_status("app", &mut transport, &mut ctx).unwrap();
    execute_revoke("app", &mut transport, &mut ctx).unwrap();

    let methods: Vec<&str> = transport.calls.iter().map(|(m, _)| m.as_str()).collect();
    assert_eq!(
        methods,
        [
            "repo.credential_set",
            "repo.credential_status",
            "repo.credential_revoke"
        ]
    );
    let out = streams.stdout.text();
    let err = streams.stderr.text();
    assert!(!out.contains(secret), "stdout must never echo token: {out}");
    assert!(!err.contains(secret), "stderr must never echo token: {err}");
}

#[test]
fn repo_github_dry_run_fails_closed_without_executing_live_mutations() {
    use org_cli_shell::error::CliDiagnostic;

    let s = |v: &[&str]| v.iter().map(|x| (*x).to_owned()).collect::<Vec<_>>();
    assert!(is_local_verb(&s(&["github", "status"])));
    assert!(reads_a_secret(&s(&["github", "configure"])));

    // 1. CLI entry point fails closed on `--dry-run` before opening any channel or touching files.
    for argv in [
        s(&[
            "configure",
            "--app-id",
            "123",
            "--installation-id",
            "456",
            "--private-key-stdin",
            "--dry-run",
        ]),
        s(&["revoke", "--dry-run"]),
        s(&[
            "bind",
            "--repo",
            "backend",
            "--owner",
            "acme",
            "--name",
            "backend",
            "--dry-run",
        ]),
        s(&["unbind", "--repo", "backend", "--dry-run"]),
    ] {
        let (mut ctx, _) = captured(Modes::default(), Terminal::piped(), &[]);
        let err = github::run(&argv, &mut ctx).unwrap_err();
        assert_eq!(err.code(), "office::unsupported", "{argv:?}: {err}");
        assert!(
            err.to_string().contains("does not support --dry-run"),
            "{argv:?}: {err}"
        );
    }

    // Status read also refuses --dry-run cleanly.
    let (mut ctx, _) = captured(Modes::default(), Terminal::piped(), &[]);
    let status_err = github::run(&s(&["status", "--dry-run"]), &mut ctx).unwrap_err();
    assert_eq!(status_err.code(), "cli::usage");

    // 2. Direct executor with pre-set `dry_run: true` never invokes the transport for any mutation.
    let mut transport = StubTransport {
        calls: Vec::new(),
        reply: json!({}),
    };
    let (mut ctx, _) = captured(
        Modes {
            dry_run: true,
            ..Default::default()
        },
        Terminal::piped(),
        &[],
    );
    for action in [
        GitHubAction::Configure {
            app_id: 1,
            installation_id: 2,
            api_base_url: None,
            private_key_file: None,
            private_key_stdin: true,
            webhook_secret_file: None,
            webhook_secret_prompt: false,
            account_hint: None,
        },
        GitHubAction::Revoke,
        GitHubAction::Bind {
            repo: "backend".into(),
            owner: "acme".into(),
            name: "backend".into(),
            installation_id: None,
        },
        GitHubAction::Unbind {
            repo: "backend".into(),
        },
    ] {
        let err = github::execute_action(action, &mut transport, &mut ctx).unwrap_err();
        assert_eq!(err.code(), "office::unsupported", "{err}");
    }
    assert!(
        transport.calls.is_empty(),
        "dry-run must never dispatch any live mutation RPC: {:?}",
        transport.calls
    );
}
