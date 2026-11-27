//! Tests for inline `agentic-org hr hire ... --bind-workspace <dir>` (`TASK-211`).

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

struct HireAndBindTransport {
    calls: Vec<(String, Value)>,
    fail_hire: Option<Refusal>,
    fail_bind: Option<Refusal>,
}

impl OfficeTransport for HireAndBindTransport {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        self.calls.push((method.to_owned(), params));
        match method {
            "hr.hire" | "hr.hire.confirm" => {
                if let Some(refusal) = &self.fail_hire {
                    return Err(refusal.clone());
                }
                Ok(test_hire_reply("0193a5c0-0000-7000-8000-000000000051"))
            }
            "member.external_bind" => {
                if let Some(refusal) = &self.fail_bind {
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
            other => panic!("unexpected method {other}"),
        }
    }
}

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn split_bind_workspace_extracts_flag_and_ignores_values_of_other_flags() {
    let (ws, rest) = split_bind_workspace(&words(&[
        "hr",
        "hire",
        "--team",
        "T1",
        "--name",
        "Ada",
        "--handle",
        "ada",
        "--reason",
        "--bind-workspace",
        "--bind-workspace",
        "/tmp/my-repo",
    ]))
    .unwrap();
    assert_eq!(ws, Some(PathBuf::from("/tmp/my-repo")));
    assert_eq!(
        rest,
        words(&[
            "hr",
            "hire",
            "--team",
            "T1",
            "--name",
            "Ada",
            "--handle",
            "ada",
            "--reason",
            "--bind-workspace"
        ])
    );

    let (ws, rest) = split_bind_workspace(&words(&[
        "hr",
        "hire",
        "--team",
        "T1",
        "--bind-workspace=/tmp/eq-repo",
    ]))
    .unwrap();
    assert_eq!(ws, Some(PathBuf::from("/tmp/eq-repo")));
    assert_eq!(rest, words(&["hr", "hire", "--team", "T1"]));

    // Non-hr-hire commands and help are untouched.
    let (ws, _) = split_bind_workspace(&words(&["hr", "list"])).unwrap();
    assert_eq!(ws, None);
    let (ws, _) = split_bind_workspace(&words(&["hr", "hire", "--help"])).unwrap();
    assert_eq!(ws, None);

    // Missing value, empty value, and duplicates fail with exit 2.
    for bad in [
        &["hr", "hire", "--team", "T1", "--bind-workspace"][..],
        &["hr", "hire", "--team", "T1", "--bind-workspace="],
        &[
            "hr",
            "hire",
            "--bind-workspace",
            "/a",
            "--bind-workspace",
            "/b",
        ],
    ] {
        let err = split_bind_workspace(&words(bad)).unwrap_err();
        assert_eq!(err.exit_code(), 2, "{bad:?}");
    }
}

#[test]
fn execute_hire_and_bind_hires_and_binds_in_one_step_without_leaking_secrets() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("repo");
    std::fs::create_dir(&ws).unwrap();
    let state_root = tmp.path().join("state");

    let mut transport = HireAndBindTransport {
        calls: Vec::new(),
        fail_hire: None,
        fail_bind: None,
    };
    let (mut ctx, out) = captured(
        Modes {
            json: true,
            ..Modes::default()
        },
        Terminal::piped(),
        &[],
    );
    execute_hire_and_bind(
        words(&[
            "hr",
            "hire",
            "--team",
            "0193a5c0-0000-7000-8000-000000000001",
            "--name",
            "Ada",
            "--handle",
            "ada",
            "--reason",
            "BYOA engineer",
        ]),
        &ws,
        Shown::Json,
        &mut ctx,
        &mut transport,
        Some(&state_root),
    )
    .unwrap();

    assert_eq!(transport.calls.len(), 2);
    assert_eq!(transport.calls[0].0, "hr.hire");
    assert_eq!(transport.calls[1].0, "member.external_bind");
    assert_eq!(
        transport.calls[1].1["employee_id"],
        "0193a5c0-0000-7000-8000-000000000051"
    );

    let env: Value = serde_json::from_str(&out.stdout.text()).unwrap();
    assert_eq!(env["ok"], true);
    assert_eq!(env["data"]["state"], "hired");
    assert_eq!(
        env["data"]["employee_id"],
        "0193a5c0-0000-7000-8000-000000000051"
    );
    assert_eq!(
        env["data"]["workspace_binding"]["session_id"],
        "ext-0193a5c0-0000-7000-8000-000000000070"
    );
    assert_eq!(
        env["data"]["workspace_binding"]["workspace_root"],
        ws.canonicalize().unwrap().display().to_string()
    );
    assert!(env["data"]["workspace_binding"].get("token").is_none());

    let state = WorkspaceState::for_root(&state_root, &ws.canonicalize().unwrap());
    let mode = std::fs::symlink_metadata(state.dir())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o700);
    assert!(state.read_key().unwrap().is_some());
    assert!(state.read_binding().unwrap().is_some());
    assert_eq!(std::fs::read_dir(&ws).unwrap().count(), 0);
}

#[test]
fn invalid_flag_combinations_or_missing_workspace_refuse_before_hiring() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("repo");
    std::fs::create_dir(&ws).unwrap();
    let state_root = tmp.path().join("state");

    for (args, dir, dry_run) in [
        (
            vec![
                "hr", "hire", "--draft", "--team", "T1", "--name", "A", "--handle", "a",
                "--reason", "r",
            ],
            ws.clone(),
            false,
        ),
        (
            vec![
                "hr",
                "hire",
                "--id",
                "req-1",
                "--if-match",
                "1",
                "--withdraw",
                "--closed-reason",
                "no",
            ],
            ws.clone(),
            false,
        ),
        (
            vec![
                "hr", "hire", "--team", "T1", "--name", "A", "--handle", "a", "--reason", "r",
            ],
            ws.clone(),
            true,
        ),
        (
            vec![
                "hr", "hire", "--team", "T1", "--name", "A", "--handle", "a", "--reason", "r",
            ],
            tmp.path().join("missing-dir"),
            false,
        ),
    ] {
        let mut transport = HireAndBindTransport {
            calls: Vec::new(),
            fail_hire: None,
            fail_bind: None,
        };
        let (mut ctx, _) = captured(
            Modes {
                dry_run,
                ..Modes::default()
            },
            Terminal::piped(),
            &[],
        );
        assert!(execute_hire_and_bind(
            words(&args),
            &dir,
            Shown::Compact,
            &mut ctx,
            &mut transport,
            Some(&state_root),
        )
        .is_err());
        assert!(
            transport.calls.is_empty(),
            "{args:?}: nothing may be sent to the office before preflight passes"
        );
    }
}

#[test]
fn hire_failure_skips_bind_and_bind_failure_gives_recovery_command() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("repo");
    std::fs::create_dir(&ws).unwrap();
    let state_root = tmp.path().join("state");

    // Hire refused -> bind never called.
    let mut transport = HireAndBindTransport {
        calls: Vec::new(),
        fail_hire: Some(Refusal::new(
            ErrorCode::NotAuthorized,
            "cannot hire",
            "ask team lead",
        )),
        fail_bind: None,
    };
    let (mut ctx, _) = captured(Modes::default(), tty(Some(80)), &[]);
    let err = execute_hire_and_bind(
        words(&[
            "hr", "hire", "--team", "T1", "--name", "Ada", "--handle", "ada", "--reason", "r",
        ]),
        &ws,
        Shown::Rendered,
        &mut ctx,
        &mut transport,
        Some(&state_root),
    )
    .unwrap_err();
    assert_eq!(err.exit_code(), 1);
    assert_eq!(transport.calls.len(), 1);
    assert_eq!(transport.calls[0].0, "hr.hire");

    // Hire succeeded but bind refused -> actionable hint names `agentic-org agent bind`.
    let mut transport = HireAndBindTransport {
        calls: Vec::new(),
        fail_hire: None,
        fail_bind: Some(Refusal::new(
            ErrorCode::NotAuthorized,
            "cannot bind",
            "check role",
        )),
    };
    let (mut ctx, _) = captured(Modes::default(), Terminal::piped(), &[]);
    let err = execute_hire_and_bind(
        words(&[
            "hr", "hire", "--team", "T1", "--name", "Ada", "--handle", "ada", "--reason", "r",
        ]),
        &ws,
        Shown::Compact,
        &mut ctx,
        &mut transport,
        Some(&state_root),
    )
    .unwrap_err();
    assert_eq!(err.exit_code(), 1);
    assert!(
        err.help()
            .as_deref()
            .is_some_and(|h| h
                .contains("agentic-org agent bind --member 0193a5c0-0000-7000-8000-000000000051")),
        "{:?}",
        err.help()
    );
}
