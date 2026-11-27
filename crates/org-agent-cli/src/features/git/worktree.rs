//! `worktree list|prepare`: isolated per-task Git worktrees, prepared before code changes.

use org_command::{Answer, Refusal, Request};
use org_proto::git::{
    WorktreeList, WorktreeListParams, WorktreeListResult, WorktreePrepare, WorktreePrepareParams,
    WorktreePrepareResult,
};
use serde_json::Value;

use super::cmd_id;
use crate::runtime::domain::CommandCue;
use crate::runtime::input::{call_rpc, opt_string, require_str, write_answer};
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport};

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
