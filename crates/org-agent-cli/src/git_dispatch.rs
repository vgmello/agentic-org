//! Dispatch for C7 `repo` and `worktree` commands over [`OfficeTransport`] (`TASK-51`,
//! `TASK-173`).

use org_command::{Ack, Answer, Refusal, Request, Status};
use org_proto::git::{
    GitPush, GitPushParams, GitPushResult, RepoAttach, RepoAttachParams, RepoAttachResult,
    RepoFetch, RepoFetchParams, RepoFetchResult, RepoList, RepoListParams, RepoListResult,
    RepoRegister, RepoRegisterParams, RepoRegisterResult, RepoStatus, RepoStatusParams,
    RepoStatusResult, RepositoryKindWire, WorktreeList, WorktreeListParams, WorktreeListResult,
    WorktreePrepare, WorktreePrepareParams, WorktreePrepareResult,
};
use org_proto::{Call, ErrorCode};
use serde_json::{Map, Value};

use crate::keys::try_new_uuid_v7;
use crate::transport::OfficeTransport;

pub fn try_execute<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    match request.command.resource {
        "repo" => match request.command.verb {
            "register" => Some(write_repo_register(transport, request)),
            "list" => Some(read_repo_list(transport, request)),
            "attach" => Some(write_repo_attach(transport, request)),
            "status" => Some(read_repo_status(transport, request)),
            "fetch" => Some(write_repo_fetch(transport, request)),
            "push" => Some(write_repo_push(transport, request)),
            _ => None,
        },
        "worktree" => match request.command.verb {
            "list" => Some(read_worktree_list(transport, request)),
            "prepare" => Some(write_worktree_prepare(transport, request)),
            _ => None,
        },
        _ => None,
    }
}

fn reject_dry_run(request: &Request) -> Result<(), Refusal> {
    if request.dry_run {
        return Err(Refusal::new(
            ErrorCode::Unsupported,
            "`--dry-run` is not supported on repository or worktree write commands",
            "Re-run without `--dry-run`.",
        ));
    }
    Ok(())
}

fn cmd_id(request: &Request) -> Result<String, Refusal> {
    reject_dry_run(request)?;
    if let Some(key) = request.idempotency_key.as_deref() {
        if org_proto::is_uuid_v7(key) {
            return Ok(key.to_owned());
        }
        return Ok(crate::transport::derive_uuid_v7("git-command", key));
    }
    fresh_id()
}

fn entity_id(request: &Request, domain: &str) -> Result<String, Refusal> {
    if let Some(key) = request.idempotency_key.as_deref() {
        return Ok(crate::transport::derive_uuid_v7(domain, key));
    }
    fresh_id()
}

fn fresh_id() -> Result<String, Refusal> {
    try_new_uuid_v7().ok_or_else(|| {
        Refusal::new(
            ErrorCode::StorageError,
            "could not mint a UUIDv7 identifier",
            "Retry the command.",
        )
    })
}

fn req_str<'a>(input: &'a Map<String, Value>, key: &str) -> Result<&'a str, Refusal> {
    input.get(key).and_then(Value::as_str).ok_or_else(|| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            format!("`{key}` is required"),
            format!("Provide `{key}`."),
        )
    })
}

fn opt_str(input: &Map<String, Value>, key: &str) -> Option<String> {
    input.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn call_rpc<C: Call, T: OfficeTransport + ?Sized>(
    transport: &mut T,
    params: &C::Params,
) -> Result<C::Result, Refusal> {
    let raw_params = serde_json::to_value(params).map_err(|_| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            "failed to encode request parameters",
            "Check the command arguments.",
        )
    })?;
    let raw = transport.call_method(C::METHOD, raw_params)?;
    serde_json::from_value(raw).map_err(|_| {
        Refusal::new(
            ErrorCode::Unavailable,
            format!("office returned an unreadable `{}` reply", C::METHOD),
            "Retry the command or check the office version.",
        )
    })
}

fn write_answer(request: &Request, data: Value, revision: Option<u64>) -> Answer {
    Answer {
        data,
        status: Status::Completed,
        operation_id: request.idempotency_key.clone(),
        ack: Some(Ack::Committed),
        revision,
        next_cursor: None,
        waiting_messages: None,
    }
}

fn write_repo_register<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let command_id = cmd_id(request)?;
    let kind_raw = req_str(&request.input, "kind")?;
    let kind = RepositoryKindWire::parse(kind_raw).ok_or_else(|| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            "kind must be `remote` or `local`",
            "Pass `--kind remote` or `--kind local`.",
        )
    })?;
    let repository_id = match opt_str(&request.input, "repository_id") {
        Some(id) => id,
        None => entity_id(request, "repo-entity")?,
    };
    let params = RepoRegisterParams {
        command_id,
        repository_id,
        key: opt_str(&request.input, "key"),
        kind,
        remote_url: opt_str(&request.input, "remote_url"),
        origin_path: opt_str(&request.input, "origin_path"),
    };
    let res: RepoRegisterResult = call_rpc::<RepoRegister, _>(transport, &params)?;
    let rev = Some(res.repository.revision);
    let data = serde_json::to_value(res).unwrap_or(Value::Null);
    Ok(write_answer(request, data, rev))
}

fn paginate_items(request: &Request, items: Vec<Value>) -> Result<Answer, Refusal> {
    let offset = match request.cursor.as_deref() {
        None => 0usize,
        Some(raw) => raw.parse::<usize>().map_err(|_| {
            Refusal::new(
                ErrorCode::InvalidRequest,
                "invalid pagination cursor",
                "Pass the `next_cursor` returned by the previous page.",
            )
        })?,
    };
    let total = items.len();
    let limit = request.limit.unwrap_or(org_command::DEFAULT_LIMIT) as usize;
    let page: Vec<Value> = items.into_iter().skip(offset).take(limit).collect();
    let next = offset.saturating_add(page.len());
    let mut answer = Answer::completed(Value::Array(page));
    if next < total {
        answer.next_cursor = Some(next.to_string());
    }
    Ok(answer)
}

fn read_repo_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = RepoListParams {
        project: opt_str(&request.input, "project"),
    };
    let res: RepoListResult = call_rpc::<RepoList, _>(transport, &params)?;
    let items: Vec<Value> = res
        .repositories
        .into_iter()
        .map(|r| serde_json::to_value(r).unwrap_or(Value::Null))
        .collect();
    paginate_items(request, items)
}

fn write_repo_attach<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let command_id = cmd_id(request)?;
    let params = RepoAttachParams {
        command_id,
        project: req_str(&request.input, "project")?.to_owned(),
        repository: req_str(&request.input, "repo")?.to_owned(),
        integration_branch: req_str(&request.input, "branch")?.to_owned(),
    };
    let res: RepoAttachResult = call_rpc::<RepoAttach, _>(transport, &params)?;
    let rev = Some(res.link.revision);
    let data = serde_json::to_value(res).unwrap_or(Value::Null);
    Ok(write_answer(request, data, rev))
}

fn read_repo_status<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = RepoStatusParams {
        repository: req_str(&request.input, "repo")?.to_owned(),
    };
    let res: RepoStatusResult = call_rpc::<RepoStatus, _>(transport, &params)?;
    Ok(Answer::completed(
        serde_json::to_value(res).unwrap_or(Value::Null),
    ))
}

/// `repo fetch`: the office refreshes its cache of the origin; the caller never needs Git
/// credentials, and no worktree's checkout moves.
fn write_repo_fetch<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    reject_dry_run(request)?;
    let params = RepoFetchParams {
        repository: req_str(&request.input, "repo")?.to_owned(),
    };
    let res: RepoFetchResult = call_rpc::<RepoFetch, _>(transport, &params)?;
    let data = serde_json::to_value(res).unwrap_or(Value::Null);
    Ok(write_answer(request, data, None))
}

fn write_repo_push<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    reject_dry_run(request)?;
    let params = GitPushParams {
        task: req_str(&request.input, "task")?.to_owned(),
        repository: opt_str(&request.input, "repo"),
    };
    let res: GitPushResult = call_rpc::<GitPush, _>(transport, &params)?;
    let data = serde_json::to_value(res).unwrap_or(Value::Null);
    Ok(write_answer(request, data, None))
}

fn read_worktree_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = WorktreeListParams {
        task: opt_str(&request.input, "task"),
    };
    let res: WorktreeListResult = call_rpc::<WorktreeList, _>(transport, &params)?;
    let items: Vec<Value> = res
        .worktrees
        .into_iter()
        .map(|w| serde_json::to_value(w).unwrap_or(Value::Null))
        .collect();
    paginate_items(request, items)
}

fn write_worktree_prepare<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let command_id = cmd_id(request)?;
    let params = WorktreePrepareParams {
        command_id,
        task: req_str(&request.input, "task")?.to_owned(),
        repository: req_str(&request.input, "repo")?.to_owned(),
        base_branch: opt_str(&request.input, "base_branch"),
    };
    let res: WorktreePrepareResult = call_rpc::<WorktreePrepare, _>(transport, &params)?;
    let data = serde_json::to_value(res).unwrap_or(Value::Null);
    Ok(write_answer(request, data, None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::TransportOffice;
    use serde_json::json;

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
    }
}
