//! Leaf input/error parsing helpers for the `review` CLI slice (`TASK-92.1`).

use org_command::{Refusal, Request};
use org_proto::reviews::{
    FindingSeverity, FindingStatus, ReviewSessionMode, ReviewStatus, ReviewTargetKind,
    ReviewVerdict,
};
use org_proto::ErrorCode;
use serde_json::{Map, Value};

use crate::runtime::input::{command_id, is_set, opt_string, opt_u64};

pub(super) fn cmd_id(request: &Request) -> Result<String, Refusal> {
    command_id(request, "review-command")
}

pub(super) fn invalid(message: impl Into<String>, hint: &str) -> Refusal {
    Refusal::new(ErrorCode::InvalidRequest, message, hint)
}

pub(super) fn unreadable(e: serde_json::Error) -> Refusal {
    Refusal::new(
        ErrorCode::Unavailable,
        format!("The office returned a review answer this agent-os cannot read: {e}."),
        "Retry the command; if it persists, the office and agent-os versions differ.",
    )
}

pub(super) fn opt_u32(input: &Map<String, Value>, field: &str) -> Result<Option<u32>, Refusal> {
    let Some(val) = opt_u64(input, field)? else {
        return Ok(None);
    };
    u32::try_from(val).map(Some).map_err(|_| {
        invalid(
            format!("`--{}` is out of range.", field.replace('_', "-")),
            "Pass a smaller non-negative integer.",
        )
    })
}

pub(super) fn parse_target_kind(raw: &str) -> Result<ReviewTargetKind, Refusal> {
    ReviewTargetKind::parse(raw).ok_or_else(|| {
        invalid(
            format!("invalid target_kind `{raw}`"),
            "Use code, doc, task, or artifact.",
        )
    })
}

pub(super) fn parse_status(raw: &str) -> Result<ReviewStatus, Refusal> {
    ReviewStatus::parse(raw).ok_or_else(|| {
        invalid(
            format!("invalid status `{raw}`"),
            "Use open, changes_requested, approved, or closed.",
        )
    })
}

pub(super) fn parse_severity(raw: &str) -> Result<FindingSeverity, Refusal> {
    FindingSeverity::parse(raw).ok_or_else(|| {
        invalid(
            format!("invalid severity `{raw}`"),
            "Use blocker, major, minor, nit, or question.",
        )
    })
}

pub(super) fn parse_finding_status(raw: &str) -> Result<FindingStatus, Refusal> {
    FindingStatus::parse(raw).ok_or_else(|| {
        invalid(
            format!("invalid set_status `{raw}`"),
            "Use open, resolved, or wont_fix.",
        )
    })
}

pub(super) fn parse_verdict(raw: &str) -> Result<ReviewVerdict, Refusal> {
    ReviewVerdict::parse(raw).ok_or_else(|| {
        invalid(
            format!("invalid verdict `{raw}`"),
            "Use approve, changes_requested, comment, or incomplete.",
        )
    })
}

pub(super) fn parse_session_mode(
    input: &Map<String, Value>,
) -> Result<(Option<ReviewSessionMode>, bool), Refusal> {
    let fresh = is_set(input, "fresh");
    let mode = opt_string(input, "session_mode")
        .map(|s| {
            ReviewSessionMode::parse(&s)
                .ok_or_else(|| invalid(format!("invalid session_mode `{s}`"), "Use warm or fresh."))
        })
        .transpose()?;
    if fresh && mode == Some(ReviewSessionMode::Warm) {
        return Err(invalid(
            "--fresh cannot be combined with --session-mode warm.",
            "Pass either --fresh or --session-mode warm.",
        ));
    }
    let effective = if fresh {
        Some(ReviewSessionMode::Fresh)
    } else {
        mode
    };
    Ok((effective, fresh))
}
