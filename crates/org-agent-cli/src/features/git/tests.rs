//! Repository and worktree dispatch: payloads, pages and acknowledgements.
use org_command::Refusal;
use org_proto::ErrorCode;
use serde_json::{json, Value};

use crate::runtime::transport::{OfficeTransport, TransportOffice};

struct FakeTransport {
    calls: Vec<(String, Value)>,
}

impl OfficeTransport for FakeTransport {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        self.calls.push((method.to_owned(), params.clone()));
        match method {
            "repo.register" => Ok(json!({
                "repository": {
                    "id": "01950000-0000-7000-8000-000000000301",
                    "key": "api",
                    "kind": "remote",
                    "remote_url": "https://github.com/acme/api.git",
                    "cache_state": "unprepared",
                    "revision": 1
                }
            })),
            "repo.list" => Ok(json!({
                "repositories": [
                    {"id": "01950000-0000-7000-8000-000000000301", "key": "a", "kind": "remote", "cache_state": "ready", "revision": 1},
                    {"id": "01950000-0000-7000-8000-000000000302", "key": "b", "kind": "remote", "cache_state": "ready", "revision": 1}
                ]
            })),
            "repo.fetch" if params["repository"] == "missing" => Err(Refusal::new(
                ErrorCode::NotFound,
                "repository not found",
                "Check the ID or name and retry.",
            )),
            "repo.fetch" => Ok(json!({
                "office_id": "01950000-0000-7000-8000-000000000001",
                "repository_id": "01950000-0000-7000-8000-000000000301",
                "repository_key": "api",
                "state": "ready",
                "default_branch": "main",
                "branch_count": 2,
                "added": ["feature/x"],
                "added_count": 1,
                "updated": [],
                "updated_count": 0,
                "pruned": [],
                "pruned_count": 0,
                "fetched_at": "2026-11-26T12:00:00.000Z"
            })),
            "git.push" => Ok(json!({
                "task_id": "01950000-0000-7000-8000-000000000401",
                "repository_id": "01950000-0000-7000-8000-000000000301",
                "branch": "task/T-A1-fix",
                "commit": "0123456789abcdef0123456789abcdef01234567"
            })),
            _ => Ok(json!({})),
        }
    }
}

#[test]
fn git_cli_rejects_dry_run_derives_deterministic_ids_and_paginates_list() {
    let mut office = TransportOffice(FakeTransport { calls: Vec::new() });

    // `--dry-run` refused before hitting transport.
    let dry = crate::execute_with_office(
        "agent-os",
        &[
            "repo".into(),
            "push".into(),
            "--task".into(),
            "T-A1".into(),
            "--dry-run".into(),
        ],
        &mut office,
    );
    assert_ne!(dry.exit_code, 0);
    assert!(office.0.calls.is_empty());

    // Same `--idempotency-key` derives deterministic `command_id` & `repository_id`.
    for _ in 0..2 {
        let out = crate::execute_with_office(
            "agent-os",
            &[
                "repo".into(),
                "register".into(),
                "--kind".into(),
                "remote".into(),
                "--key".into(),
                "api".into(),
                "--remote-url".into(),
                "https://github.com/acme/api.git".into(),
                "--idempotency-key".into(),
                "retry-key-1".into(),
            ],
            &mut office,
        );
        assert_eq!(out.exit_code, 0, "stdout: {}", out.stdout);
    }
    assert_eq!(
        office.0.calls[0].1["command_id"],
        office.0.calls[1].1["command_id"]
    );
    assert_eq!(
        office.0.calls[0].1["repository_id"],
        office.0.calls[1].1["repository_id"]
    );

    // `repo list --limit 1` returns page of 1 with `next_cursor: "1"`.
    let page = crate::execute_with_office(
        "agent-os",
        &["repo".into(), "list".into(), "--limit".into(), "1".into()],
        &mut office,
    );
    assert_eq!(page.exit_code, 0);
    let v: Value = serde_json::from_str(&page.stdout).unwrap();
    assert_eq!(v["data"].as_array().unwrap().len(), 1);
    assert_eq!(v["meta"]["next_cursor"].as_str(), Some("1"));
}

fn run(office: &mut TransportOffice<FakeTransport>, args: &[&str]) -> (u8, Value) {
    let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
    let out = crate::execute_with_office("agent-os", &args, office);
    let env = serde_json::from_str(&out.stdout).unwrap_or(Value::Null);
    (out.exit_code, env)
}

#[test]
fn repo_fetch_sends_the_repository_and_returns_the_office_summary() {
    let mut office = TransportOffice(FakeTransport { calls: Vec::new() });

    let (code, env) = run(&mut office, &["repo", "fetch", "--repo", "api"]);
    assert_eq!(code, 0, "{env}");
    assert_eq!(office.0.calls.len(), 1);
    assert_eq!(office.0.calls[0].0, "repo.fetch");
    assert_eq!(office.0.calls[0].1, json!({"repository": "api"}));
    assert_eq!(env["data"]["added"], json!(["feature/x"]));
    assert_eq!(env["data"]["default_branch"], "main");
    assert_eq!(env["meta"]["ack"], "committed");

    // The office's structured refusal reaches the caller unchanged.
    let (code, env) = run(&mut office, &["repo", "fetch", "--repo", "missing"]);
    assert_ne!(code, 0);
    assert_eq!(env["error"]["code"], "not_found", "{env}");

    // `--dry-run`, a missing `--repo` and a positional repository never reach the office.
    let before = office.0.calls.len();
    for args in [
        &["repo", "fetch", "--repo", "api", "--dry-run"][..],
        &["repo", "fetch"][..],
        &["repo", "fetch", "api"][..],
    ] {
        let (code, env) = run(&mut office, args);
        assert_ne!(code, 0, "{args:?}: {env}");
        assert_eq!(env["ok"], false, "{args:?}: {env}");
    }
    assert_eq!(office.0.calls.len(), before);

    // `--schema` declares every refusal code the command can answer.
    let (code, env) = run(&mut office, &["repo", "fetch", "--schema"]);
    assert_eq!(code, 0, "{env}");
    for declared in [
        "not_authorized",
        "not_bound",
        "not_found",
        "rate_limited",
        "storage_error",
        "unavailable",
        "unsupported",
    ] {
        assert!(
            env["data"]["errors"]
                .as_array()
                .is_some_and(|codes| codes.iter().any(|c| c == declared)),
            "{declared}: {env}"
        );
    }
    assert_eq!(office.0.calls.len(), before);
}
