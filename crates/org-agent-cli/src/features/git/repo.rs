//! `repo register|list|attach|status|fetch|push`: the organization's Git repositories, their
//! project links, the office's cache of each origin, and publishing a task branch.

use org_command::{Answer, Refusal, Request};
use org_proto::git::{
    GitPush, GitPushParams, GitPushResult, RepoAttach, RepoAttachParams, RepoAttachResult,
    RepoFetch, RepoFetchParams, RepoFetchResult, RepoList, RepoListParams, RepoListResult,
    RepoRegister, RepoRegisterParams, RepoRegisterResult, RepoStatus, RepoStatusParams,
    RepoStatusResult, RepositoryKindWire,
};
use org_proto::ErrorCode;
use serde_json::Value;

use super::cmd_id;
use crate::runtime::domain::CommandCue;
use crate::runtime::input::{
    call_rpc, entity_id, opt_string, reject_dry_run, require_str, write_answer,
};
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport};

/// `Use when:` guidance of each `repo` command.
pub(super) const CUES: &[CommandCue] = &[
    ("repo", "attach", "linking a registered repository and verified integration branch to a project."),
    ("repo", "fetch", "seeing branches pushed to the origin after your worktree was prepared: the office refreshes `origin/*` with its own credentials and never moves your checkout."),
    ("repo", "list", "listing registered Git repositories and their office bare-cache state (optionally by `--project`)."),
    ("repo", "push", "pushing your committed task worktree branch to the repository origin via the office."),
    ("repo", "register", "registering a new remote or local Git repository in the organization."),
    ("repo", "status", "checking whether an office's bare Git cache for a repository is ready or failed before preparing a worktree."),
];

/// Runs a `repo` command.
pub(super) fn dispatch<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    Some(match request.command.verb {
        "register" => write_repo_register(transport, request),
        "list" => read_repo_list(transport, request),
        "attach" => write_repo_attach(transport, request),
        "status" => read_repo_status(transport, request),
        "fetch" => write_repo_fetch(transport, request),
        "push" => write_repo_push(transport, request),
        _ => return None,
    })
}

fn write_repo_register<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let command_id = cmd_id(request)?;
    let kind_raw = require_str(&request.input, "kind")?;
    let kind = RepositoryKindWire::parse(kind_raw).ok_or_else(|| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            "kind must be `remote` or `local`",
            "Pass `--kind remote` or `--kind local`.",
        )
    })?;
    let repository_id = match opt_string(&request.input, "repository_id") {
        Some(id) => id,
        None => entity_id(request, None, "repo-entity")?,
    };
    let params = RepoRegisterParams {
        command_id,
        repository_id,
        key: opt_string(&request.input, "key"),
        kind,
        remote_url: opt_string(&request.input, "remote_url"),
        origin_path: opt_string(&request.input, "origin_path"),
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
    // A bad cursor is refused before the office is asked.
    parse_cursor(request.cursor.as_deref())?;
    let params = RepoListParams {
        project: opt_string(&request.input, "project"),
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
        project: require_str(&request.input, "project")?.to_owned(),
        repository: require_str(&request.input, "repo")?.to_owned(),
        integration_branch: require_str(&request.input, "branch")?.to_owned(),
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
        repository: require_str(&request.input, "repo")?.to_owned(),
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
        repository: require_str(&request.input, "repo")?.to_owned(),
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
        task: require_str(&request.input, "task")?.to_owned(),
        repository: opt_string(&request.input, "repo"),
    };
    let res: GitPushResult = call_rpc::<GitPush, _>(transport, &params)?;
    let data = serde_json::to_value(res).unwrap_or(Value::Null);
    Ok(write_answer(request, data, None))
}
