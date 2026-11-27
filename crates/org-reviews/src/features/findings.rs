//! Anchored findings, append-only finding replies, and atomic round-pinned verdicts.

use std::collections::BTreeSet;

use org_proto::reviews::{
    FindingSeverity, FindingStatus, ReviewFindingDraft, ReviewFindingOp, ReviewFindingParams,
    ReviewFindingResult, ReviewFindingView, ReviewStatus, ReviewTargetKind, ReviewVerdict,
    ReviewVerdictParams, ReviewVerdictResult, REVIEW_MAX_FINDINGS_PER_VERDICT,
    REVIEW_SUMMARY_MAX_CHARS,
};
use org_proto::ErrorCode;
use org_store::{Change, Id, Refusal};
use serde_json::json;

use crate::time::format_rfc3339_ms;

use super::super::ports::{ReviewContextRead, ReviewsRead};
use super::super::records::{
    compute_review_status, finding_to_view, has_open_blocker, reviewer_verdict_to_view,
    ReviewFindingRecord, ReviewFindingReplyRecord,
};
use super::super::validation::{
    bad_req, conflict, not_found, refuse, storage_refusal, too_large, unauth,
    validate_attachment_ids, validate_finding_draft, validate_oid, validate_reply_body,
    validate_text_limit, validate_uuid,
};
use super::{load_mutable_review, LoadedReview};

fn append_findings<V: ReviewsRead>(
    view: &V,
    loaded: &mut LoadedReview,
    actor: &Id,
    drafts: &[ReviewFindingDraft],
    at: &str,
) -> Result<(Vec<Id>, Vec<ReviewFindingView>), Refusal> {
    if drafts.len() > REVIEW_MAX_FINDINGS_PER_VERDICT {
        return Err(too_large(format!(
            "verdict exceeds {REVIEW_MAX_FINDINGS_PER_VERDICT} findings"
        )));
    }
    let mut seen = BTreeSet::new();
    let mut parsed = Vec::with_capacity(drafts.len());
    for d in drafts {
        let (fid, atts) = validate_finding_draft(d)?;
        if !seen.insert(fid.clone()) {
            return Err(bad_req("duplicate finding_id in batch"));
        }
        if view
            .review_finding_by_id(&fid)
            .map_err(storage_refusal)?
            .is_some()
        {
            return Err(refuse(
                ErrorCode::AlreadyExists,
                "finding_id already exists",
            ));
        }
        parsed.push((fid, atts));
    }
    let mut next_num = loaded.findings.iter().map(|f| f.number).max().unwrap_or(0);
    let (mut ids, mut views) = (
        Vec::with_capacity(parsed.len()),
        Vec::with_capacity(parsed.len()),
    );
    for (d, (fid, atts)) in drafts.iter().zip(parsed) {
        next_num = next_num.saturating_add(1);
        ids.push(fid.clone());
        let rec = ReviewFindingRecord {
            id: fid,
            review_id: loaded.review.id.clone(),
            number: next_num,
            round_number: loaded.review.current_round,
            author_id: actor.clone(),
            severity: d.severity,
            status: FindingStatus::Open,
            anchor: d.anchor.clone(),
            body_markdown: d.body_markdown.clone(),
            attachment_ids: atts,
            resolved_by: None,
            resolved_at: None,
            resolved_in_round: None,
            revision: 1,
            created_at: at.to_owned(),
            updated_at: at.to_owned(),
        };
        views.push(finding_to_view(&rec, &[]));
        loaded.findings.push(rec);
    }
    Ok((ids, views))
}

fn is_review_submitter(loaded: &LoadedReview, actor: &Id, finding_round: u32) -> bool {
    *actor == loaded.review.created_by
        || loaded
            .current_round()
            .is_some_and(|r| r.submitted_by == *actor)
        || loaded
            .rounds
            .iter()
            .find(|r| r.round_number == finding_round)
            .is_some_and(|r| r.submitted_by == *actor)
}

fn downgrade_approved_if_blocked(loaded: &mut LoadedReview) {
    if loaded.review.status == ReviewStatus::Approved && has_open_blocker(&loaded.findings) {
        loaded.review.status = ReviewStatus::Open;
    }
}

fn decide_reply<V: ReviewsRead>(
    view: &V,
    mut loaded: LoadedReview,
    actor: &Id,
    p: &ReviewFindingParams,
    at: &str,
) -> Result<Change, Refusal> {
    let ReviewFindingOp::Reply {
        finding_id,
        reply_id,
        body_markdown,
        set_status,
        attachment_ids,
    } = &p.op
    else {
        unreachable!()
    };
    let (fid, rid) = (
        validate_uuid(finding_id, "finding_id")?,
        validate_uuid(reply_id, "reply_id")?,
    );
    let body = validate_reply_body(body_markdown, set_status.is_some())?;
    let atts = validate_attachment_ids(attachment_ids)?;
    if view
        .review_finding_reply_by_id(&rid)
        .map_err(storage_refusal)?
        .is_some()
    {
        return Err(refuse(ErrorCode::AlreadyExists, "reply_id already exists"));
    }
    let idx = loaded
        .findings
        .iter()
        .position(|f| f.id == fid)
        .ok_or_else(|| not_found("finding not found"))?;
    let is_blocker = loaded.findings[idx].severity == FindingSeverity::Blocker;
    let finding_round = loaded.findings[idx].round_number;
    let closes_finding = matches!(
        *set_status,
        Some(FindingStatus::Resolved | FindingStatus::WontFix)
    );
    if loaded.review.independent
        && is_blocker
        && closes_finding
        && is_review_submitter(&loaded, actor, finding_round)
    {
        return Err(unauth(
            "author or round submitter cannot resolve or wont_fix a blocker finding on an independent review",
        ));
    }
    let rn = loaded.review.current_round;
    loaded.replies.push(ReviewFindingReplyRecord {
        id: rid.clone(),
        finding_id: fid.clone(),
        review_id: loaded.review.id.clone(),
        round_number: rn,
        author_id: actor.clone(),
        body_markdown: body,
        set_status: *set_status,
        attachment_ids: atts,
        created_at: at.to_owned(),
    });
    let f = &mut loaded.findings[idx];
    if let Some(st) = *set_status {
        let res = st != FindingStatus::Open;
        (f.status, f.resolved_by, f.resolved_at, f.resolved_in_round) = (
            st,
            res.then(|| actor.clone()),
            res.then(|| at.to_owned()),
            res.then_some(rn),
        );
    }
    f.revision = f.revision.saturating_add(1);
    f.updated_at = at.to_owned();
    let finding = finding_to_view(&loaded.findings[idx], &loaded.replies);
    downgrade_approved_if_blocked(&mut loaded);
    loaded.bump(at);
    let payload = json!({ "finding_id": fid.as_str(), "reply_id": rid.as_str(), "round_number": rn, "set_status": set_status.map(FindingStatus::as_str) });
    let reply = ReviewFindingResult {
        review: loaded.view(),
        finding,
    };
    Ok(loaded.commit(
        "finding_replied",
        actor,
        &p.command_id,
        &payload,
        at,
        &reply,
    ))
}

pub fn decide_finding<V: ReviewsRead + ReviewContextRead>(
    view: &V,
    now_ms: u64,
    caller_id: &str,
    p: &ReviewFindingParams,
) -> Result<Change, Refusal> {
    let (caller, mut loaded) = load_mutable_review(
        view,
        caller_id,
        &p.command_id,
        &p.review,
        None,
        ErrorCode::InvalidRequest,
        "cannot modify findings on a closed review",
        false,
    )?;
    let at = format_rfc3339_ms(now_ms);
    match &p.op {
        ReviewFindingOp::Add(draft) => {
            let (_, mut added) = append_findings(
                view,
                &mut loaded,
                &caller.id,
                std::slice::from_ref(draft),
                &at,
            )?;
            let finding = added.pop().expect("one finding added");
            downgrade_approved_if_blocked(&mut loaded);
            loaded.bump(&at);
            let payload = json!({
                "finding_id": &finding.id, "number": finding.number,
                "round_number": loaded.review.current_round,
                "severity": draft.severity.as_str(), "anchor_kind": draft.anchor.kind.as_str(),
            });
            let reply = ReviewFindingResult {
                review: loaded.view(),
                finding,
            };
            Ok(loaded.commit(
                "finding_added",
                &caller.id,
                &p.command_id,
                &payload,
                &at,
                &reply,
            ))
        }
        ReviewFindingOp::Reply { .. } => decide_reply(view, loaded, &caller.id, p, &at),
    }
}

fn check_verdict_guards(
    loaded: &LoadedReview,
    caller_id: &Id,
    p: &ReviewVerdictParams,
) -> Result<usize, Refusal> {
    if p.round_number != loaded.review.current_round {
        return Err(conflict(format!(
            "stale round {}: review is on round {}",
            p.round_number, loaded.review.current_round
        )));
    }
    let round = loaded
        .current_round()
        .ok_or_else(|| refuse(ErrorCode::StorageError, "current round missing"))?;
    match loaded.review.target_kind {
        ReviewTargetKind::Code => {
            let head = validate_oid(
                p.head_oid
                    .as_deref()
                    .ok_or_else(|| bad_req("code verdict requires head_oid"))?,
                "head_oid",
            )?;
            if round.head_oid.as_deref() != Some(head.as_str()) {
                return Err(conflict("head_oid mismatch"));
            }
        }
        _ if p.head_oid.is_some() => {
            return Err(bad_req("head_oid is only valid for code reviews"))
        }
        _ => {}
    }
    let ridx = loaded
        .reviewers
        .iter()
        .position(|r| {
            r.round_number == loaded.review.current_round
                && !r.removed
                && r.reviewer_id == *caller_id
        })
        .ok_or_else(|| unauth("only an assigned reviewer may submit a verdict"))?;
    if loaded.review.independent
        && (*caller_id == loaded.review.created_by || *caller_id == round.submitted_by)
    {
        return Err(unauth(
            "author or round submitter cannot submit a verdict on an independent review",
        ));
    }
    let blocker = has_open_blocker(&loaded.findings)
        || p.findings
            .iter()
            .any(|d| d.severity == FindingSeverity::Blocker);
    if p.verdict == ReviewVerdict::Approve && blocker {
        return Err(conflict(
            "cannot approve while open blocker findings remain",
        ));
    }
    Ok(ridx)
}

pub fn decide_verdict<V: ReviewsRead + ReviewContextRead>(
    view: &V,
    now_ms: u64,
    caller_id: &str,
    p: &ReviewVerdictParams,
    expected_revision: Option<u64>,
) -> Result<Change, Refusal> {
    decide_verdict_with_session(view, now_ms, caller_id, p, expected_revision, None)
}

pub fn decide_verdict_with_session<V: ReviewsRead + ReviewContextRead>(
    view: &V,
    now_ms: u64,
    caller_id: &str,
    p: &ReviewVerdictParams,
    expected_revision: Option<u64>,
    runtime_session_id: Option<&str>,
) -> Result<Change, Refusal> {
    let (caller, mut loaded) = load_mutable_review(
        view,
        caller_id,
        &p.command_id,
        &p.review,
        expected_revision,
        ErrorCode::InvalidRequest,
        "cannot submit a verdict on a closed review",
        false,
    )?;
    let ridx = check_verdict_guards(&loaded, &caller.id, p)?;
    let summary = validate_text_limit(
        &p.summary_markdown,
        REVIEW_SUMMARY_MAX_CHARS,
        "summary_markdown",
    )?;
    let atts = validate_attachment_ids(&p.attachment_ids)?;
    let at = format_rfc3339_ms(now_ms);
    let (batch_ids, findings) = append_findings(view, &mut loaded, &caller.id, &p.findings, &at)?;
    let row = &mut loaded.reviewers[ridx];
    (
        row.verdict,
        row.summary_markdown,
        row.finding_ids,
        row.attachment_ids,
        row.verdict_at,
    ) = (
        Some(p.verdict),
        summary,
        batch_ids.clone(),
        atts,
        Some(at.clone()),
    );
    if let Some(sid) = runtime_session_id.map(str::trim).filter(|s| !s.is_empty()) {
        row.runtime_session_id = Some(sid.to_owned());
    }
    let verdict = reviewer_verdict_to_view(row).expect("verdict set");
    let rn = loaded.review.current_round;
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
    loaded.bump(&at);
    let payload = json!({
        "round_number": rn, "verdict": p.verdict.as_str(),
        "status": loaded.review.status.as_str(),
        "finding_ids": batch_ids.iter().map(Id::as_str).collect::<Vec<_>>(),
    });
    let reply = ReviewVerdictResult {
        review: loaded.view(),
        verdict,
        findings,
    };
    Ok(loaded.commit(
        "review_verdict",
        &caller.id,
        &p.command_id,
        &payload,
        &at,
        &reply,
    ))
}
