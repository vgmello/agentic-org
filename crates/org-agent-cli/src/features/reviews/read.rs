//! Read handlers for `review list|inspect|diff` (`TASK-92.1`).

use org_command::{Answer, Refusal, Request};
use org_proto::reviews::{
    ReviewDiff, ReviewDiffParams, ReviewDiffResult, ReviewInspect, ReviewInspectParams,
    ReviewInspectResult, ReviewList, ReviewListParams, ReviewListResult, ReviewView,
};
use org_proto::Call;
use serde_json::{json, Value};

use crate::runtime::domain::CommandCue;
use crate::runtime::input::{encode, is_set, opt_string, require_str};
use crate::runtime::transport::{parse_cursor, OfficeTransport};

use super::parse::{opt_u32, parse_status, parse_target_kind, unreadable};

/// `Use when:` guidance of each read `review` verb.
pub(super) const CUES: &[CommandCue] = &[
    ("review", "diff", "reading the bounded bare-cache diff of a code review round or `--since-round` inter-round delta."),
    ("review", "inspect", "inspecting a review's rounds, reviewers, findings, `pin_stale`, and `anchor_stale` (use `--fields` to prune unrequested sections server-side)."),
    ("review", "list", "listing reviews filtered by project, task, status, target kind, reviewer, or creator."),
];

pub(super) fn read_review_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let offset = parse_cursor(request.cursor.as_deref())?;
    let i = &request.input;
    let status = opt_string(i, "status")
        .map(|s| parse_status(&s))
        .transpose()?;
    let target_kind = opt_string(i, "target_kind")
        .map(|s| parse_target_kind(&s))
        .transpose()?;
    let limit = request.limit.unwrap_or(org_command::DEFAULT_LIMIT);
    let params = ReviewListParams {
        project_id: opt_string(i, "project_id"),
        task_id: opt_string(i, "task_id"),
        status,
        target_kind,
        reviewer_id: opt_string(i, "reviewer_id"),
        created_by: opt_string(i, "created_by"),
        limit: Some(limit),
        offset: (offset > 0).then_some(offset as u64),
    };
    let raw = transport.call_method(ReviewList::METHOD, json!(params))?;
    let res: ReviewListResult = serde_json::from_value(raw).map_err(unreadable)?;
    let max_items = limit as usize;
    let items: Vec<Value> = res
        .reviews
        .iter()
        .take(max_items)
        .map(|r| encode(r, ReviewList::METHOD))
        .collect::<Result<_, _>>()?;
    let returned = items.len();
    let next = offset.saturating_add(returned);
    let mut answer = Answer::completed(Value::Array(items));
    if max_items > 0 && returned > 0 && (next as u64) < res.total {
        answer.next_cursor = Some(next.to_string());
    }
    answer.total = Some(res.total);
    Ok(answer)
}

pub(super) fn read_review_inspect<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = ReviewInspectParams {
        review: require_str(&request.input, "review")?.to_owned(),
        fields: request.fields.clone(),
    };
    let raw = transport.call_method(ReviewInspect::METHOD, json!(params))?;
    if request.fields.is_some() {
        let review_val = raw.get("review").cloned().ok_or_else(|| {
            unreadable(serde_json::Error::io(std::io::Error::other(
                "missing review",
            )))
        })?;
        let _: ReviewView = serde_json::from_value(review_val).map_err(unreadable)?;
        return Ok(Answer::completed(raw));
    }
    let res: ReviewInspectResult = serde_json::from_value(raw).map_err(unreadable)?;
    Ok(Answer::completed(encode(&res, ReviewInspect::METHOD)?))
}

pub(super) fn read_review_diff<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let i = &request.input;
    let params = ReviewDiffParams {
        review: require_str(i, "review")?.to_owned(),
        round: opt_u32(i, "round")?,
        since_round: opt_u32(i, "since_round")?,
        path: opt_string(i, "path"),
        stat_only: is_set(i, "stat_only"),
        context_lines: opt_u32(i, "context_lines")?,
    };
    let raw = transport.call_method(ReviewDiff::METHOD, json!(params))?;
    let res: ReviewDiffResult = serde_json::from_value(raw).map_err(unreadable)?;
    Ok(Answer::completed(encode(&res, ReviewDiff::METHOD)?))
}
