//! `worktree list|prepare`: isolated per-task Git worktrees, prepared before code changes.

use org_command::{Ack, Answer, Command, Example, Field, IfMatch, Kind, Refusal, Request, Type};
use org_proto::git::{
    WorktreeList, WorktreeListParams, WorktreeListResult, WorktreePrepare, WorktreePrepareParams,
    WorktreePrepareResult,
};
use serde_json::Value;

use super::{cmd_id, READ_ERRORS, STRING_TYPE, WRITE_ERRORS};
use crate::runtime::domain::CommandCue;
use crate::runtime::input::{call_rpc, opt_string, require_str, write_answer};
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport};

const WORKTREE_FIELDS: &[Field] = &[
    Field::required("office_id", Type::String, "Office UUIDv7"),
    Field::required("employee_id", Type::String, "Owner employee UUIDv7"),
    Field::required("task_id", Type::String, "Task UUIDv7"),
    Field::required("task_key", Type::String, "Task key"),
    Field::required("repository_id", Type::String, "Repository UUIDv7"),
    Field::optional("repository_key", Type::String, "Repository key"),
    Field::required("branch", Type::String, "Task branch"),
    Field::required("worktree_path", Type::String, "Worktree directory path"),
    Field::required("base_commit", Type::String, "Base commit OID"),
    Field::optional("pushed_head", Type::String, "Last pushed commit OID"),
    Field::required("state", Type::String, "Workspace state"),
];

const WORKTREE_PREPARE_OUTPUT: &[Field] = &[
    Field::required(
        "worktree",
        Type::Object(WORKTREE_FIELDS),
        "Prepared worktree",
    ),
    Field::required(
        "writable_roots",
        Type::List(&STRING_TYPE),
        "Sandbox writable roots",
    ),
    Field::required(
        "fixed_write_denies",
        Type::List(&STRING_TYPE),
        "Sandbox write-denied Git metadata paths",
    ),
];

const WORKTREE_LIST_INPUT: &[Field] = &[Field::optional(
    "task",
    Type::String,
    "Filter by task ID or key",
)];

const WORKTREE_PREPARE_INPUT: &[Field] = &[
    Field::required("task", Type::String, "Task ID or T-<CODE><N> key"),
    Field::required("repo", Type::String, "Repository ID or key"),
    Field::optional(
        "base_branch",
        Type::String,
        "Base branch for projectless tasks",
    ),
];

/// The `worktree` rows.
pub(super) const COMMANDS: &[Command] = &[
    Command {
        resource: "worktree",
        verb: "list",
        summary: "List task worktrees",
        call: "worktree.list",
        kind: Kind::List,
        input: WORKTREE_LIST_INPUT,
        content_field: None,
        output: WORKTREE_FIELDS,
        errors: READ_ERRORS,
        example: Example {
            input: "{}",
            output: r#"[{"office_id":"01950000-0000-7000-8000-000000000001","employee_id":"01950000-0000-7000-8000-000000000002","task_id":"01950000-0000-7000-8000-000000000401","task_key":"T-A1","repository_id":"01950000-0000-7000-8000-000000000301","branch":"task/T-A1-fix-bug","worktree_path":"/tmp/wt","base_commit":"0123456789abcdef0123456789abcdef01234567","state":"active"}]"#,
        },
    },
    Command {
        resource: "worktree",
        verb: "prepare",
        summary: "Prepare task worktree",
        call: "worktree.prepare",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: WORKTREE_PREPARE_INPUT,
        content_field: None,
        output: WORKTREE_PREPARE_OUTPUT,
        errors: WRITE_ERRORS,
        example: Example {
            input: r#"{"task":"T-A1","repo":"api"}"#,
            output: r#"{"worktree":{"office_id":"01950000-0000-7000-8000-000000000001","employee_id":"01950000-0000-7000-8000-000000000002","task_id":"01950000-0000-7000-8000-000000000401","task_key":"T-A1","repository_id":"01950000-0000-7000-8000-000000000301","branch":"task/T-A1-fix-bug","worktree_path":"/tmp/wt","base_commit":"0123456789abcdef0123456789abcdef01234567","state":"active"},"writable_roots":["/tmp/wt"],"fixed_write_denies":["/tmp/wt/.git"]}"#,
        },
    },
];

/// `Use when:` guidance of each `worktree` command.
pub(super) const CUES: &[CommandCue] = &[
    ("worktree", "list", "listing existing task Git worktrees, branches, and paths (optionally filtered by `--task`)."),
    ("worktree", "prepare", "creating or reusing an isolated per-task Git worktree and branch before editing repository code."),
];

/// Runs a `worktree` command.
pub(super) fn dispatch<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Option<Result<Answer, Refusal>> {
    Some(match request.command.verb {
        "list" => read_worktree_list(transport, request),
        "prepare" => write_worktree_prepare(transport, request),
        _ => return None,
    })
}

fn read_worktree_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    // A bad cursor is refused before the office is asked.
    parse_cursor(request.cursor.as_deref())?;
    let params = WorktreeListParams {
        task: opt_string(&request.input, "task"),
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
        task: require_str(&request.input, "task")?.to_owned(),
        repository: require_str(&request.input, "repo")?.to_owned(),
        base_branch: opt_string(&request.input, "base_branch"),
    };
    let res: WorktreePrepareResult = call_rpc::<WorktreePrepare, _>(transport, &params)?;
    let data = serde_json::to_value(res).unwrap_or(Value::Null);
    Ok(write_answer(request, data, None))
}
