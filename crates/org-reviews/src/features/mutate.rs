//! Review lifecycle mutations: `review.route`, `review.resubmit`, and `review.close`.

use std::collections::BTreeSet;

use org_proto::reviews::{
    ReviewCloseParams, ReviewMutationResult, ReviewResubmitParams, ReviewRoundResult,
    ReviewRouteParams, ReviewRouteResult, ReviewSessionMode, ReviewStatus, ReviewTargetKind,
    REVIEW_SUMMARY_MAX_CHARS,
};
use org_proto::ErrorCode;
use org_store::{Change, Id, Refusal};
use serde_json::json;

use crate::time::format_rfc3339_ms;

use super::super::ports::{ReviewContextRead, ReviewsRead};
use super::super::records::{
    compute_review_status, has_open_blocker, reviewers_to_views, round_to_view, ResolvedCodePin,
    ReviewReviewerRecord, ReviewRoundRecord,
};
use super::super::validation::{
    bad_req, check_resolved_code_pin, effective_session_mode, refuse, resolve_routing,
    storage_refusal, unauth, validate_close_reason, validate_oid, validate_text_limit,
    ResolvedRouting,
};
use super::{build_event, commit_doc, load_mutable_review, LoadedReview};

fn apply_round_routing(
    loaded: &mut LoadedReview,
    res: &ResolvedRouting,
    mode: ReviewSessionMode,
    actor: &Id,
    at: &str,
) {
    let rn = loaded.review.current_round;
    let next: BTreeSet<&Id> = res.reviewer_ids.iter().collect();
    for ex in loaded.reviewers.iter_mut().filter(|r| r.round_number == rn) {
        if !next.contains(&ex.reviewer_id) {
            ex.removed = true;
        }
    }
    for rid in &res.reviewer_ids {
        if let Some(row) = loaded
            .reviewers
            .iter_mut()
            .find(|r| r.round_number == rn && r.reviewer_id == *rid)
        {
            let was_removed = row.removed;
            (
                row.removed,
                row.routing,
                row.team_id,
                row.session_mode,
                row.assigned_by,
                row.assigned_at,
            ) = (
                false,
                res.mode,
                res.team_id.clone(),
                mode,
                actor.clone(),
                at.to_owned(),
            );
            if was_removed {
                row.verdict = None;
                row.summary_markdown.clear();
                row.finding_ids.clear();
                row.attachment_ids.clear();
                row.verdict_at = None;
            }
        } else {
            loaded.reviewers.push(
                ReviewReviewerRecord::assigned(
                    loaded.review.id.clone(),
                    rn,
                    rid.clone(),
                    res.mode,
                    mode,
                    actor.clone(),
                    at.to_owned(),
                )
                .with_context(res.team_id.clone(), None),
            );
        }
    }
}

pub fn decide_route<V: ReviewsRead + ReviewContextRead>(
    view: &V,
    now_ms: u64,
    caller_id: &str,
    p: &ReviewRouteParams,
) -> Result<Change, Refusal> {
    let (caller, mut loaded) = load_mutable_review(
        view,
        caller_id,
        &p.command_id,
        &p.review,
        Some(p.expected_revision),
        ErrorCode::InvalidRequest,
        "cannot route a closed review",
        false,
    )?;
    let project = loaded
        .review
        .project_id
        .as_ref()
        .map(|pid| view.project_info(pid))
        .transpose()
        .map_err(storage_refusal)?
        .flatten();
    let mode = effective_session_mode(
        loaded.review.independent,
        p.session_mode,
        loaded.review.session_mode,
    )?;
    let res = resolve_routing(
        view,
        Some(&p.routing),
        project.as_ref(),
        &loaded.review.created_by,
        loaded.review.independent,
        loaded.review.number.saturating_add(loaded.review.revision),
    )?;
    let (at, rn) = (format_rfc3339_ms(now_ms), loaded.review.current_round);
    apply_round_routing(&mut loaded, &res, mode, &caller.id, &at);
    let active: Vec<_> = loaded
        .reviewers
        .iter()
        .filter(|r| r.round_number == rn && !r.removed)
        .collect();
    loaded.review.status = compute_review_status(
        loaded.review.status,
        &active,
        has_open_blocker(&loaded.findings),
    );
    loaded.review.session_mode = mode;
    loaded.bump(&at);
    let payload = json!({
        "round_number": rn, "routing": res.mode.as_str(),
        "team_id": res.team_id.as_ref().map(Id::as_str), "session_mode": mode.as_str(),
        "reviewer_ids": res.reviewer_ids.iter().map(Id::as_str).collect::<Vec<_>>(),
    });
    let reply = ReviewRouteResult {
        review: loaded.view(),
        reviewers: reviewers_to_views(rn, &loaded.reviewers),
    };
    Ok(loaded.commit(
        "review_routed",
        &caller.id,
        &p.command_id,
        &payload,
        &at,
        &reply,
    ))
}

fn resolve_resubmit_pin(
    kind: ReviewTargetKind,
    base0: Option<&str>,
    p: &ReviewResubmitParams,
    pin: Option<&ResolvedCodePin>,
) -> Result<(Option<String>, Option<String>), Refusal> {
    if kind != ReviewTargetKind::Code {
        if p.expected_head_oid.is_some() || pin.is_some() {
            return Err(bad_req("expected_head_oid is only for code reviews"));
        }
        return Ok((None, None));
    }
    let (base, head) = match pin {
        Some(cp) => check_resolved_code_pin(cp, p.expected_head_oid.as_deref())?,
        None => {
            let b = base0
                .ok_or_else(|| refuse(ErrorCode::StorageError, "code review missing base_oid"))?
                .to_owned();
            let h = validate_oid(
                p.expected_head_oid
                    .as_deref()
                    .ok_or_else(|| bad_req("code resubmit requires expected_head_oid"))?,
                "expected_head_oid",
            )?;
            (b, h)
        }
    };
    Ok((Some(base), Some(head)))
}

fn carry_reviewers(loaded: &mut LoadedReview, prev: u32, next: u32, mode: ReviewSessionMode) {
    let carried: Vec<_> = loaded
        .reviewers
        .iter()
        .filter(|r| r.round_number == prev && !r.removed)
        .map(|old| {
            let sid = (mode == ReviewSessionMode::Warm)
                .then(|| old.runtime_session_id.clone())
                .flatten();
            ReviewReviewerRecord::assigned(
                loaded.review.id.clone(),
                next,
                old.reviewer_id.clone(),
                old.routing,
                mode,
                old.assigned_by.clone(),
                old.assigned_at.clone(),
            )
            .with_context(old.team_id.clone(), sid)
        })
        .collect();
    loaded.reviewers.extend(carried);
}

pub fn decide_resubmit<V: ReviewsRead + ReviewContextRead>(
    view: &V,
    now_ms: u64,
    caller_id: &str,
    p: &ReviewResubmitParams,
    pin: Option<&ResolvedCodePin>,
) -> Result<Change, Refusal> {
    let (caller, mut loaded) = load_mutable_review(
        view,
        caller_id,
        &p.command_id,
        &p.review,
        Some(p.expected_revision),
        ErrorCode::InvalidRequest,
        "cannot resubmit a closed review",
        false,
    )?;
    let prev = loaded.review.current_round;
    if loaded.review.independent
        && loaded
            .reviewers
            .iter()
            .any(|r| r.round_number == prev && !r.removed && r.reviewer_id == caller.id)
    {
        return Err(unauth(
            "assigned reviewer cannot resubmit an independent review",
        ));
    }
    let mode = effective_session_mode(
        loaded.review.independent,
        p.requested_session_mode().map_err(bad_req)?,
        loaded.review.session_mode,
    )?;
    let note_markdown =
        validate_text_limit(&p.note_markdown, REVIEW_SUMMARY_MAX_CHARS, "note_markdown")?;
    let base_prev = loaded
        .current_round()
        .or_else(|| loaded.rounds.last())
        .and_then(|r| r.base_oid.as_deref())
        .map(str::to_owned);
    let (base_oid, head_oid) =
        resolve_resubmit_pin(loaded.review.target_kind, base_prev.as_deref(), p, pin)?;
    let next = prev
        .checked_add(1)
        .ok_or_else(|| bad_req("round_number overflow"))?;
    let at = format_rfc3339_ms(now_ms);
    let round = ReviewRoundRecord {
        review_id: loaded.review.id.clone(),
        round_number: next,
        base_oid: base_oid.clone(),
        head_oid: head_oid.clone(),
        session_mode: mode,
        submitted_by: caller.id.clone(),
        submitted_at: at.clone(),
        note_markdown,
    };
    carry_reviewers(&mut loaded, prev, next, mode);
    loaded.review.current_round = next;
    loaded.review.status = ReviewStatus::Open;
    loaded.review.session_mode = mode;
    loaded.bump(&at);
    let payload = json!({ "round_number": next, "session_mode": mode.as_str(), "base_oid": base_oid, "head_oid": head_oid });
    let ev = build_event(
        &loaded.review.id,
        loaded.review.revision,
        "review_resubmitted",
        &caller.id,
        Some(&p.command_id),
        &payload,
        &at,
    );
    let reply = ReviewRoundResult {
        review: loaded.view(),
        round: round_to_view(&round, &loaded.reviewers),
    };
    Ok(commit_doc(
        &loaded.into_document(),
        Some(&round),
        &ev,
        &reply,
    ))
}

pub fn decide_close<V: ReviewsRead + ReviewContextRead>(
    view: &V,
    now_ms: u64,
    caller_id: &str,
    p: &ReviewCloseParams,
) -> Result<Change, Refusal> {
    let (caller, mut loaded) = load_mutable_review(
        view,
        caller_id,
        &p.command_id,
        &p.review,
        Some(p.expected_revision),
        ErrorCode::Conflict,
        "review is already closed",
        true,
    )?;
    let reason = validate_close_reason(p.reason.as_deref())?;
    let at = format_rfc3339_ms(now_ms);
    loaded.review.status = ReviewStatus::Closed;
    loaded.review.closed_reason = reason.clone();
    loaded.review.closed_at = Some(at.clone());
    loaded.bump(&at);
    let reply = ReviewMutationResult {
        review: loaded.view(),
    };
    Ok(loaded.commit(
        "review_closed",
        &caller.id,
        &p.command_id,
        &json!({ "reason": reason }),
        &at,
        &reply,
    ))
}
