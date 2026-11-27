//! `repo register|list|attach|status|fetch|push`: the organization's Git repositories, their
//! project links, the office's cache of each origin, and publishing a task branch.

use org_command::{Ack, Answer, Command, Example, Field, IfMatch, Kind, Refusal, Request, Type};
use org_proto::git::{
    GitPush, GitPushParams, GitPushResult, RepoAttach, RepoAttachParams, RepoAttachResult,
    RepoFetch, RepoFetchParams, RepoFetchResult, RepoList, RepoListParams, RepoListResult,
    RepoRegister, RepoRegisterParams, RepoRegisterResult, RepoStatus, RepoStatusParams,
    RepoStatusResult, RepositoryKindWire,
};
use org_proto::ErrorCode;
use serde_json::Value;

use super::{cmd_id, FETCH_ERRORS, READ_ERRORS, STRING_TYPE, WRITE_ERRORS};
use crate::runtime::domain::CommandCue;
use crate::runtime::input::{
    call_rpc, entity_id, opt_string, reject_dry_run, require_str, write_answer,
};
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport};

const REPO_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Repository UUIDv7"),
    Field::optional("key", Type::String, "Repository key"),
    Field::required("kind", Type::Enum(&["remote", "local"]), "Repository kind"),
    Field::optional("remote_url", Type::String, "Remote URL"),
    Field::optional(
        "integration_branch",
        Type::String,
        "Project integration branch",
    ),
    Field::required(
        "cache_state",
        Type::Enum(&["unprepared", "cloning", "ready", "failed"]),
        "Office cache state",
    ),
    Field::optional("cache_reason", Type::String, "Cache failure reason"),
    Field::required("revision", Type::Integer, "Record revision"),
];

const REPO_REGISTER_OUTPUT: &[Field] = &[Field::required(
    "repository",
    Type::Object(REPO_FIELDS),
    "Registered repository",
)];

const REPO_ATTACH_LINK_FIELDS: &[Field] = &[
    Field::required("project_id", Type::String, "Project UUIDv7"),
    Field::required("project_key", Type::String, "Project key"),
    Field::required("repository_id", Type::String, "Repository UUIDv7"),
    Field::optional("repository_key", Type::String, "Repository key"),
    Field::required("integration_branch", Type::String, "Integration branch"),
    Field::required("revision", Type::Integer, "Project revision"),
];

const REPO_ATTACH_OUTPUT: &[Field] = &[Field::required(
    "link",
    Type::Object(REPO_ATTACH_LINK_FIELDS),
    "Attached project repository link",
)];

const REPO_STATUS_OUTPUT: &[Field] = &[
    Field::required("office_id", Type::String, "Office UUIDv7"),
    Field::required("repository_id", Type::String, "Repository UUIDv7"),
    Field::required(
        "state",
        Type::Enum(&["unprepared", "cloning", "ready", "failed"]),
        "Cache state",
    ),
    Field::optional("bare_path", Type::String, "Bare cache path"),
    Field::optional("reason", Type::String, "Failure reason"),
];

const REPO_FETCH_OUTPUT: &[Field] = &[
    Field::required("office_id", Type::String, "Office UUIDv7"),
    Field::required("repository_id", Type::String, "Repository UUIDv7"),
    Field::optional("repository_key", Type::String, "Repository key"),
    Field::required(
        "state",
        Type::Enum(&["unprepared", "cloning", "ready", "failed"]),
        "Cache state",
    ),
    Field::optional("default_branch", Type::String, "Origin HEAD branch"),
    Field::optional("head_oid", Type::String, "Default branch commit OID"),
    Field::required("branch_count", Type::Integer, "Origin branches cached"),
    Field::required("added", Type::List(&STRING_TYPE), "New branches (max 50)"),
    Field::required("added_count", Type::Integer, "New branch count"),
    Field::required(
        "updated",
        Type::List(&STRING_TYPE),
        "Moved branches (max 50)",
    ),
    Field::required("updated_count", Type::Integer, "Moved branch count"),
    Field::required(
        "pruned",
        Type::List(&STRING_TYPE),
        "Removed branches (max 50)",
    ),
    Field::required("pruned_count", Type::Integer, "Removed branch count"),
    Field::required("fetched_at", Type::String, "Fetch completion time"),
];

const REPO_PUSH_OUTPUT: &[Field] = &[
    Field::required("task_id", Type::String, "Task UUIDv7"),
    Field::required("repository_id", Type::String, "Repository UUIDv7"),
    Field::required("branch", Type::String, "Pushed task branch"),
    Field::required("commit", Type::String, "Pushed commit OID"),
];

const REPO_REGISTER_INPUT: &[Field] = &[
    Field::required("kind", Type::Enum(&["remote", "local"]), "remote or local"),
    Field::optional("key", Type::String, "Optional unique key"),
    Field::optional("remote_url", Type::String, "Remote HTTPS or SSH URL"),
    Field::optional("origin_path", Type::String, "Local origin directory path"),
    Field::optional("repository_id", Type::String, "Optional repository UUIDv7"),
];

const REPO_LIST_INPUT: &[Field] = &[Field::optional(
    "project",
    Type::String,
    "Filter by project ID, P-<n>, or slug",
)];

const REPO_ATTACH_INPUT: &[Field] = &[
    Field::required("project", Type::String, "Project ID, P-<n>, or slug"),
    Field::required("repo", Type::String, "Repository ID or key"),
    Field::required("branch", Type::String, "Verified integration branch"),
];

const REPO_STATUS_INPUT: &[Field] = &[Field::required(
    "repo",
    Type::String,
    "Repository ID or key",
)];

const REPO_FETCH_INPUT: &[Field] = &[Field::required(
    "repo",
    Type::String,
    "Repository ID or key",
)];

const REPO_PUSH_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task ID or T-<CODE><N> key"),
    Field::optional("repo", Type::String, "Repository ID or key"),
];

/// The `repo` rows.
pub(super) const COMMANDS: &[Command] = &[
    Command {
        resource: "repo",
        verb: "register",
        summary: "Register a repository",
        call: "repo.register",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: REPO_REGISTER_INPUT,
        content_field: None,
        output: REPO_REGISTER_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"kind":"remote","key":"api","remote_url":"https://github.com/acme/api.git"}"#,
            output: r#"{"repository":{"id":"01950000-0000-7000-8000-000000000301","key":"api","kind":"remote","remote_url":"https://github.com/acme/api.git","cache_state":"unprepared","revision":1}}"#,
        },
    },
    Command {
        resource: "repo",
        verb: "list",
        summary: "List repositories",
        call: "repo.list",
        kind: Kind::List,
        input: REPO_LIST_INPUT,
        content_field: None,
        output: REPO_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: "{}",
            output: r#"[{"id":"01950000-0000-7000-8000-000000000301","key":"api","kind":"remote","remote_url":"https://github.com/acme/api.git","cache_state":"ready","revision":1}]"#,
        },
    },
    Command {
        resource: "repo",
        verb: "attach",
        summary: "Attach repo to project",
        call: "repo.attach",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::No,
        },
        input: REPO_ATTACH_INPUT,
        content_field: None,
        output: REPO_ATTACH_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"project":"P-1","repo":"api","branch":"main"}"#,
            output: r#"{"link":{"project_id":"01950000-0000-7000-8000-000000000201","project_key":"P-1","repository_id":"01950000-0000-7000-8000-000000000301","repository_key":"api","integration_branch":"main","revision":2}}"#,
        },
    },
    Command {
        resource: "repo",
        verb: "status",
        summary: "Show repo cache status",
        call: "repo.status",
        kind: Kind::Read,
        input: REPO_STATUS_INPUT,
        content_field: None,
        output: REPO_STATUS_OUTPUT,
        errors: READ_ERRORS,
        example: Example {
            input: r#"{"repo":"api"}"#,
            output: r#"{"office_id":"01950000-0000-7000-8000-000000000001","repository_id":"01950000-0000-7000-8000-000000000301","state":"ready"}"#,
        },
    },
    Command {
        resource: "repo",
        verb: "fetch",
        summary: "Fetch origin branches",
        call: "repo.fetch",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: REPO_FETCH_INPUT,
        content_field: None,
        output: REPO_FETCH_OUTPUT,
        errors: FETCH_ERRORS,
        example: Example {
            input: r#"{"repo":"api"}"#,
            output: r#"{"office_id":"01950000-0000-7000-8000-000000000001","repository_id":"01950000-0000-7000-8000-000000000301","repository_key":"api","state":"ready","default_branch":"main","head_oid":"0123456789abcdef0123456789abcdef01234567","branch_count":3,"added":["feature/x"],"added_count":1,"updated":["main"],"updated_count":1,"pruned":[],"pruned_count":0,"fetched_at":"2026-11-26T12:00:00.000Z"}"#,
        },
    },
    Command {
        resource: "repo",
        verb: "push",
        summary: "Push task branch",
        call: "git.push",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: REPO_PUSH_INPUT,
        content_field: None,
        output: REPO_PUSH_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1"}"#,
            output: r#"{"task_id":"01950000-0000-7000-8000-000000000401","repository_id":"01950000-0000-7000-8000-000000000301","branch":"task/T-A1-fix-bug","commit":"0123456789abcdef0123456789abcdef01234567"}"#,
        },
    },
];

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
