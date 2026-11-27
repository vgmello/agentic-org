//! `session resume` (`agent-os resume` / `agent-os me`, `TASK-220`): one-call wake/resume
//! dashboard returning identity, unread inbox, open assigned tasks with optional caller-owned
//! private working note pointer, open asks, active worktrees, and optional own budget status.

use org_command::{Answer, Refusal, Request};
use org_proto::resume::{AgentResume, AgentResumeParams};
use org_proto::{Call, ErrorCode};
use serde_json::json;

use crate::runtime::domain::CommandCue;
use crate::runtime::input::opt_u64;
use crate::runtime::transport::OfficeTransport;

/// `Use when:` guidance for `session resume` (`agent-os resume` / `agent-os me`).
pub(super) const CUES: &[CommandCue] = &[(
    "session",
    "resume",
    "waking up, resuming after context reset, or checking your unread inbox, active tasks (with your latest private working note), open asks, worktrees, and budget in one call (`agent-os resume` or `agent-os me`).",
)];

fn opt_u32(request: &Request, field: &str) -> Result<Option<u32>, Refusal> {
    let Some(val) = opt_u64(request, field)? else {
        return Ok(None);
    };
    u32::try_from(val).map(Some).map_err(|_| {
        let flag = field.replace('_', "-");
        Refusal::new(
            ErrorCode::InvalidRequest,
            format!("`--{flag}` is too large."),
            format!("Pass a small positive integer for `--{flag}`."),
        )
    })
}

pub(super) fn read_session_resume<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = AgentResumeParams {
        tasks_limit: opt_u32(request, "tasks_limit")?,
        inbox_limit: opt_u32(request, "inbox_limit")?,
        asks_limit: opt_u32(request, "asks_limit")?,
        worktrees_limit: opt_u32(request, "worktrees_limit")?,
    };
    let raw = transport.call_method(AgentResume::METHOD, json!(params))?;
    Ok(Answer::completed(raw))
}

#[cfg(test)]
#[path = "resume_tests.rs"]
mod tests;
