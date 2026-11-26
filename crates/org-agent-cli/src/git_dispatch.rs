//! Dispatch for C7 `repo` and `worktree` commands over [`OfficeTransport`] (`TASK-51`,
//! `TASK-173`).

use org_command::{Answer, Refusal, Request, Status};
use org_proto::git::{
    GitPush, GitPushParams, GitPushResult, RepoAttach, RepoAttachParams, RepoAttachResult,
    RepoFetch, RepoFetchParams, RepoFetchResult, RepoList, RepoListParams, RepoListResult,
    RepoRegister, RepoRegisterParams, RepoRegisterResult, RepoStatus, RepoStatusParams,
    RepoStatusResult, RepositoryKindWire, WorktreeList, WorktreeListParams, WorktreeListResult,
    WorktreePrepare, WorktreePrepareParams, WorktreePrepareResult,
};
use org_proto::{Call, ErrorCode};
use serde_json::{Map, Value};

use crate::transport::{command_id, paginate, write_ack, OfficeTransport};

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
    command_id(request, "git-command")
}

fn entity_id(request: &Request, domain: &str) -> Result<String, Refusal> {
    crate::transport::entity_id(request, None, domain)
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
        ack: Some(write_ack(request)),
        revision,
        next_cursor: None,
        waiting_messages: None,
        total: None,
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
    paginate(request, items)
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
    paginate(request, items)
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
mod tests;
