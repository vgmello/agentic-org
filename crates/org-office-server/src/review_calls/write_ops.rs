//! Write handlers for `review.request`, `review.route`, `review.finding`, `review.verdict`,
//! `review.resubmit`, and `review.close` (`TASK-92.1`).

use std::collections::BTreeSet;

use org_office::reviews::features::ReviewInspectSections;
use org_office::Office;
use org_office_sessions::SessionManager;
use org_proto::reviews::{
    FindingSeverity, FindingStatus, ReviewClose, ReviewCloseParams, ReviewFinding,
    ReviewFindingDraft, ReviewFindingOp, ReviewFindingParams, ReviewInspectParams,
    ReviewInspectResult, ReviewRequest, ReviewRequestParams, ReviewResubmit, ReviewResubmitParams,
    ReviewRoute, ReviewRouteParams, ReviewStatus, ReviewTargetKind, ReviewVerdict,
    ReviewVerdictCall, ReviewVerdictParams, REVIEW_FINDING_BODY_MAX_CHARS,
    REVIEW_MAX_FINDINGS_PER_VERDICT, REVIEW_REPLY_BODY_MAX_CHARS, REVIEW_SUMMARY_MAX_CHARS,
};
use org_proto::{ApiError, Call, ErrorCode, Response};

use crate::live::LiveEvents;

use super::attachments::{resolve_review_attachment_batches, resolve_review_attachments};
use super::git::{CodePinQuery, ReviewGit};
use super::scope::verify_active_and_request_scope;
use super::wakes;

pub(super) struct WriteDeps<'a> {
    pub(super) office: &'a Office,
    pub(super) live: &'a LiveEvents,
    pub(super) git: &'a ReviewGit,
    pub(super) sessions: Option<&'a SessionManager>,
    pub(super) sync_wake: Option<&'a (dyn Fn() + Send + Sync)>,
}

impl WriteDeps<'_> {
    fn notify_mutation(&self) {
        self.live.record_mailbox_change();
        if let Some(wake) = self.sync_wake {
            wake();
        }
    }
}

pub(super) fn handle_request(
    deps: &WriteDeps<'_>,
    request_id: u64,
    member: &str,
    params: ReviewRequestParams,
) -> Response {
    let pin = if params.target_kind == ReviewTargetKind::Code {
        if let Err(refusal) = verify_active_and_request_scope(
            deps.office,
            member,
            params.project_id.as_deref(),
            params.task_id.as_deref(),
            params.repository_id.as_deref(),
        ) {
            return Response::error(request_id, refusal);
        }
        let (Some(repo_id), Some(branch)) =
            (params.repository_id.as_deref(), params.branch.as_deref())
        else {
            return Response::error(
                request_id,
                ApiError::new(
                    ErrorCode::InvalidRequest,
                    "code review requires repository_id and branch",
                ),
            );
        };
        match deps.git.resolve_code_pin(
            deps.office,
            &CodePinQuery {
                caller_id: member,
                repository_id: repo_id.trim(),
                branch: branch.trim(),
                task_id: params.task_id.as_deref(),
                requested_base_oid: params.base_oid.as_deref(),
                expected_head_oid: params.expected_head_oid.as_deref(),
            },
        ) {
            Ok(pin) => Some(pin),
            Err(refusal) => return Response::error(request_id, refusal),
        }
    } else {
        None
    };

    let result = match deps.office.review_request_with_pin(member, params, pin) {
        Ok(Ok(res)) => res,
        Ok(Err(refusal)) => return Response::error(request_id, refusal),
        Err(err) => {
            crate::logged(ReviewRequest::METHOD, &err);
            return storage_err(request_id);
        }
    };
    deps.notify_mutation();
    if let Some(sessions) = deps.sessions {
        if let Ok(inspected) =
            inspect_for_service(deps.office, member, &result.review.id, false, true, false)
        {
            wakes::on_review_requested(
                deps.office,
                sessions,
                member,
                &result.review,
                &result.round,
                &inspected.reviewers,
            );
        }
    }
    Response::ok::<ReviewRequest>(request_id, &result).unwrap_or_else(|_| storage_err(request_id))
}

pub(super) fn handle_route(
    deps: &WriteDeps<'_>,
    request_id: u64,
    member: &str,
    params: ReviewRouteParams,
) -> Response {
    let result = match deps.office.review_route(member, params) {
        Ok(Ok(res)) => res,
        Ok(Err(refusal)) => return Response::error(request_id, refusal),
        Err(err) => {
            crate::logged(ReviewRoute::METHOD, &err);
            return storage_err(request_id);
        }
    };
    deps.notify_mutation();
    if let Some(sessions) = deps.sessions {
        wakes::on_review_routed(
            deps.office,
            sessions,
            member,
            &result.review,
            &result.reviewers,
        );
    }
    Response::ok::<ReviewRoute>(request_id, &result).unwrap_or_else(|_| storage_err(request_id))
}

pub(super) fn handle_finding(
    deps: &WriteDeps<'_>,
    request_id: u64,
    member: &str,
    mut params: ReviewFindingParams,
) -> Response {
    let need_findings = matches!(params.op, ReviewFindingOp::Reply { .. });
    let inspected = match inspect_for_service(
        deps.office,
        member,
        &params.review,
        true,
        true,
        need_findings,
    ) {
        Ok(res) => res,
        Err(refusal) => return Response::error(request_id, refusal),
    };
    if let Err(refusal) = precheck_finding_before_bind(deps, member, &inspected, &mut params) {
        return Response::error(request_id, refusal);
    }
    let (event_id, preview, wake_atts) = match &mut params.op {
        ReviewFindingOp::Add(draft) => {
            match resolve_review_attachments(
                deps.office,
                member,
                &params.command_id,
                &inspected.review,
                &draft.attachment_ids,
            ) {
                Ok(bound) => draft.attachment_ids = bound,
                Err(refusal) => return Response::error(request_id, refusal),
            }
            (
                draft.finding_id.clone(),
                draft.body_markdown.clone(),
                draft.attachment_ids.clone(),
            )
        }
        ReviewFindingOp::Reply {
            reply_id,
            body_markdown,
            attachment_ids,
            ..
        } => {
            match resolve_review_attachments(
                deps.office,
                member,
                &params.command_id,
                &inspected.review,
                attachment_ids,
            ) {
                Ok(bound) => *attachment_ids = bound,
                Err(refusal) => return Response::error(request_id, refusal),
            }
            (
                reply_id.clone(),
                body_markdown.clone(),
                attachment_ids.clone(),
            )
        }
    };
    let result = match deps.office.review_finding(member, params) {
        Ok(Ok(res)) => res,
        Ok(Err(refusal)) => return Response::error(request_id, refusal),
        Err(err) => {
            crate::logged(ReviewFinding::METHOD, &err);
            return storage_err(request_id);
        }
    };
    deps.notify_mutation();
    if let Some(sessions) = deps.sessions {
        wakes::on_review_finding_activity(
            deps.office,
            sessions,
            &wakes::FindingWake {
                actor_id: member,
                review: &result.review,
                reviewers: &inspected.reviewers,
                finding: &result.finding,
                event_id: &event_id,
                preview: &preview,
                attachment_ids: &wake_atts,
            },
        );
    }
    Response::ok::<ReviewFinding>(request_id, &result).unwrap_or_else(|_| storage_err(request_id))
}

pub(super) fn handle_verdict(
    deps: &WriteDeps<'_>,
    request_id: u64,
    member: &str,
    mut params: ReviewVerdictParams,
) -> Response {
    let inspected = match inspect_for_service(deps.office, member, &params.review, true, true, true)
    {
        Ok(res) => res,
        Err(refusal) => return Response::error(request_id, refusal),
    };
    if let Err(refusal) = precheck_verdict_before_bind(deps, member, &inspected, &mut params) {
        return Response::error(request_id, refusal);
    }
    let mut batches: Vec<&[String]> = Vec::with_capacity(1 + params.findings.len());
    batches.push(&params.attachment_ids);
    for draft in &params.findings {
        batches.push(&draft.attachment_ids);
    }
    let mut bound_batches = match resolve_review_attachment_batches(
        deps.office,
        member,
        &params.command_id,
        &inspected.review,
        &batches,
    ) {
        Ok(bound) => bound.into_iter(),
        Err(refusal) => return Response::error(request_id, refusal),
    };
    if let Some(verdict_atts) = bound_batches.next() {
        params.attachment_ids = verdict_atts;
    }
    for (draft, draft_atts) in params.findings.iter_mut().zip(bound_batches) {
        draft.attachment_ids = draft_atts;
    }
    let runtime_session_id = deps
        .sessions
        .and_then(|s| s.current_runtime_session_id(member));
    let result =
        match deps
            .office
            .review_verdict_with_session(member, params, None, runtime_session_id)
        {
            Ok(Ok(res)) => res,
            Ok(Err(refusal)) => return Response::error(request_id, refusal),
            Err(err) => {
                crate::logged(ReviewVerdictCall::METHOD, &err);
                return storage_err(request_id);
            }
        };
    deps.notify_mutation();
    if let Some(sessions) = deps.sessions {
        let submitted_by = inspected
            .rounds
            .iter()
            .find(|r| r.round_number == result.verdict.round_number)
            .map(|r| r.submitted_by.as_str());
        wakes::on_review_verdict(
            deps.office,
            sessions,
            member,
            &result.review,
            submitted_by,
            &result.verdict,
            result.findings.len(),
        );
    }
    Response::ok::<ReviewVerdictCall>(request_id, &result)
        .unwrap_or_else(|_| storage_err(request_id))
}

pub(super) fn handle_resubmit(
    deps: &WriteDeps<'_>,
    request_id: u64,
    member: &str,
    params: ReviewResubmitParams,
) -> Response {
    let inspected =
        match inspect_for_service(deps.office, member, &params.review, true, false, false) {
            Ok(res) => res,
            Err(refusal) => return Response::error(request_id, refusal),
        };
    let prev_round = inspected
        .rounds
        .iter()
        .find(|r| r.round_number == inspected.review.current_round);
    let pin = if inspected.review.target_kind == ReviewTargetKind::Code {
        let (Some(repo_id), Some(branch)) = (
            inspected.review.repository_id.as_deref(),
            inspected.review.branch.as_deref(),
        ) else {
            return Response::error(
                request_id,
                ApiError::new(
                    ErrorCode::InvalidRequest,
                    "code review is missing repository_id or branch",
                ),
            );
        };
        let base_oid = prev_round.and_then(|r| r.base_oid.as_deref());
        match deps.git.resolve_code_pin(
            deps.office,
            &CodePinQuery {
                caller_id: member,
                repository_id: repo_id,
                branch,
                task_id: inspected.review.task_id.as_deref(),
                requested_base_oid: base_oid,
                expected_head_oid: params.expected_head_oid.as_deref(),
            },
        ) {
            Ok(pin) => Some(pin),
            Err(refusal) => return Response::error(request_id, refusal),
        }
    } else {
        None
    };

    let result = match deps.office.review_resubmit_with_pin(member, params, pin) {
        Ok(Ok(res)) => res,
        Ok(Err(refusal)) => return Response::error(request_id, refusal),
        Err(err) => {
            crate::logged(ReviewResubmit::METHOD, &err);
            return storage_err(request_id);
        }
    };
    deps.notify_mutation();
    if let Some(sessions) = deps.sessions {
        let inter_round_stat = match (
            inspected.review.repository_id.as_deref(),
            prev_round.and_then(|r| r.head_oid.as_deref()),
            result.round.head_oid.as_deref(),
        ) {
            (Some(repo_id), Some(prev_head), Some(new_head)) => {
                deps.git
                    .format_inter_round_stat(deps.office, repo_id, prev_head, new_head)
            }
            _ => None,
        };
        if let Ok(Ok(records)) = deps.office.review_reviewer_records(&result.review.id) {
            wakes::on_review_resubmitted(
                deps.office,
                sessions,
                member,
                &result.review,
                &result.round,
                &records,
                inter_round_stat.as_deref(),
            );
        }
    }
    Response::ok::<ReviewResubmit>(request_id, &result).unwrap_or_else(|_| storage_err(request_id))
}

pub(super) fn handle_close(
    deps: &WriteDeps<'_>,
    request_id: u64,
    member: &str,
    params: ReviewCloseParams,
) -> Response {
    let result = match deps.office.review_close(member, params) {
        Ok(Ok(res)) => res,
        Ok(Err(refusal)) => return Response::error(request_id, refusal),
        Err(err) => {
            crate::logged(ReviewClose::METHOD, &err);
            return storage_err(request_id);
        }
    };
    deps.notify_mutation();
    Response::ok::<ReviewClose>(request_id, &result).unwrap_or_else(|_| storage_err(request_id))
}

fn precheck_open_and_unarchived(
    office: &Office,
    command_id: &str,
    inspected: &ReviewInspectResult,
    closed_msg: &str,
) -> Result<(), ApiError> {
    if !org_proto::is_uuid_v7(command_id.trim()) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "command_id: a lowercase UUIDv7",
        ));
    }
    if inspected.review.status == ReviewStatus::Closed {
        return Err(ApiError::new(ErrorCode::InvalidRequest, closed_msg));
    }
    if let Some(pid) = inspected.review.project_id.as_deref() {
        let proj = office
            .inspect_project(pid)
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "the office could not answer"))??;
        if proj.archived {
            return Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "project is archived",
            ));
        }
    }
    Ok(())
}

fn precheck_draft_and_anchor(
    deps: &WriteDeps<'_>,
    inspected: &ReviewInspectResult,
    draft: &mut ReviewFindingDraft,
) -> Result<(), ApiError> {
    if !org_proto::is_uuid_v7(draft.finding_id.trim()) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "finding_id: a lowercase UUIDv7",
        ));
    }
    if draft.body_markdown.trim().is_empty() {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "finding body cannot be empty",
        ));
    }
    if draft.body_markdown.chars().count() > REVIEW_FINDING_BODY_MAX_CHARS {
        return Err(ApiError::new(
            ErrorCode::TooLarge,
            format!("body_markdown exceeds {REVIEW_FINDING_BODY_MAX_CHARS} chars"),
        ));
    }
    draft
        .anchor
        .check()
        .map_err(|m| ApiError::new(ErrorCode::InvalidRequest, m))?;
    if inspected.review.target_kind == ReviewTargetKind::Code {
        if let (Some(repo_id), Some(head_oid)) = (
            inspected.review.repository_id.as_deref(),
            inspected
                .rounds
                .iter()
                .find(|r| r.round_number == inspected.review.current_round)
                .and_then(|r| r.head_oid.as_deref()),
        ) {
            deps.git
                .validate_and_hash_anchor(deps.office, repo_id, head_oid, &mut draft.anchor)?;
        }
    }
    Ok(())
}

fn precheck_finding_before_bind(
    deps: &WriteDeps<'_>,
    member: &str,
    inspected: &ReviewInspectResult,
    params: &mut ReviewFindingParams,
) -> Result<(), ApiError> {
    precheck_open_and_unarchived(
        deps.office,
        &params.command_id,
        inspected,
        "cannot modify findings on a closed review",
    )?;
    match &mut params.op {
        ReviewFindingOp::Add(draft) => precheck_draft_and_anchor(deps, inspected, draft),
        ReviewFindingOp::Reply {
            finding_id,
            reply_id,
            body_markdown,
            set_status,
            ..
        } => {
            if !org_proto::is_uuid_v7(finding_id.trim()) {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "finding_id: a lowercase UUIDv7",
                ));
            }
            if !org_proto::is_uuid_v7(reply_id.trim()) {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "reply_id: a lowercase UUIDv7",
                ));
            }
            if body_markdown.chars().count() > REVIEW_REPLY_BODY_MAX_CHARS {
                return Err(ApiError::new(
                    ErrorCode::TooLarge,
                    format!("body_markdown exceeds {REVIEW_REPLY_BODY_MAX_CHARS} chars"),
                ));
            }
            if body_markdown.trim().is_empty() && set_status.is_none() {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "reply requires body or set_status",
                ));
            }
            let finding = inspected
                .findings
                .iter()
                .find(|f| f.id == finding_id.trim())
                .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "finding not found"))?;
            let closes_finding = matches!(
                *set_status,
                Some(FindingStatus::Resolved | FindingStatus::WontFix)
            );
            let is_submitter = member == inspected.review.created_by
                || inspected
                    .rounds
                    .iter()
                    .find(|r| r.round_number == inspected.review.current_round)
                    .is_some_and(|r| r.submitted_by == member)
                || inspected
                    .rounds
                    .iter()
                    .find(|r| r.round_number == finding.round_number)
                    .is_some_and(|r| r.submitted_by == member);
            if inspected.review.independent
                && finding.severity == FindingSeverity::Blocker
                && closes_finding
                && is_submitter
            {
                return Err(ApiError::new(
                    ErrorCode::NotAuthorized,
                    "author or round submitter cannot resolve or wont_fix a blocker finding on an independent review",
                ));
            }
            Ok(())
        }
    }
}

fn precheck_verdict_round_head<'a>(
    inspected: &'a ReviewInspectResult,
    params: &ReviewVerdictParams,
) -> Result<&'a org_proto::reviews::ReviewRoundView, ApiError> {
    if params.round_number != inspected.review.current_round {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            format!(
                "stale round {}: review is on round {}",
                params.round_number, inspected.review.current_round
            ),
        ));
    }
    let round = inspected
        .rounds
        .iter()
        .find(|r| r.round_number == inspected.review.current_round)
        .ok_or_else(|| ApiError::new(ErrorCode::StorageError, "current round missing"))?;
    if inspected.review.target_kind == ReviewTargetKind::Code {
        let head = params.head_oid.as_deref().map(str::trim).ok_or_else(|| {
            ApiError::new(ErrorCode::InvalidRequest, "code verdict requires head_oid")
        })?;
        let valid_hex = (head.len() == 40 || head.len() == 64)
            && head.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        if !valid_hex {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "head_oid must be a 40- or 64-char hex OID",
            ));
        }
        if round.head_oid.as_deref() != Some(head) {
            return Err(ApiError::new(ErrorCode::Conflict, "head_oid mismatch"));
        }
    } else if params.head_oid.is_some() {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "head_oid is only valid for code reviews",
        ));
    }
    Ok(round)
}

fn precheck_verdict_authz_and_blockers(
    member: &str,
    inspected: &ReviewInspectResult,
    round: &org_proto::reviews::ReviewRoundView,
    params: &ReviewVerdictParams,
) -> Result<(), ApiError> {
    let is_assigned = inspected
        .reviewers
        .iter()
        .any(|r| !r.removed && r.reviewer_id == member);
    if !is_assigned {
        return Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "only an assigned reviewer may submit a verdict",
        ));
    }
    if inspected.review.independent
        && (member == inspected.review.created_by || member == round.submitted_by)
    {
        return Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "author or round submitter cannot submit a verdict on an independent review",
        ));
    }
    let has_blocker = inspected
        .findings
        .iter()
        .any(|f| f.severity == FindingSeverity::Blocker && f.status == FindingStatus::Open)
        || params
            .findings
            .iter()
            .any(|d| d.severity == FindingSeverity::Blocker);
    if params.verdict == ReviewVerdict::Approve && has_blocker {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            "cannot approve while open blocker findings remain",
        ));
    }
    Ok(())
}

fn precheck_verdict_findings_batch(
    deps: &WriteDeps<'_>,
    inspected: &ReviewInspectResult,
    params: &mut ReviewVerdictParams,
) -> Result<(), ApiError> {
    if params.summary_markdown.chars().count() > REVIEW_SUMMARY_MAX_CHARS {
        return Err(ApiError::new(
            ErrorCode::TooLarge,
            format!("summary_markdown exceeds {REVIEW_SUMMARY_MAX_CHARS} chars"),
        ));
    }
    if params.findings.len() > REVIEW_MAX_FINDINGS_PER_VERDICT {
        return Err(ApiError::new(
            ErrorCode::TooLarge,
            format!("verdict exceeds {REVIEW_MAX_FINDINGS_PER_VERDICT} findings"),
        ));
    }
    let mut seen = BTreeSet::new();
    for draft in &mut params.findings {
        precheck_draft_and_anchor(deps, inspected, draft)?;
        if !seen.insert(draft.finding_id.trim().to_owned()) {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "duplicate finding_id in batch",
            ));
        }
    }
    Ok(())
}

fn precheck_verdict_before_bind(
    deps: &WriteDeps<'_>,
    member: &str,
    inspected: &ReviewInspectResult,
    params: &mut ReviewVerdictParams,
) -> Result<(), ApiError> {
    precheck_open_and_unarchived(
        deps.office,
        &params.command_id,
        inspected,
        "cannot submit a verdict on a closed review",
    )?;
    let round = precheck_verdict_round_head(inspected, params)?;
    precheck_verdict_authz_and_blockers(member, inspected, round, params)?;
    precheck_verdict_findings_batch(deps, inspected, params)
}

fn inspect_for_service(
    office: &Office,
    member: &str,
    review_ref: &str,
    rounds: bool,
    reviewers: bool,
    findings: bool,
) -> Result<ReviewInspectResult, ApiError> {
    office
        .review_inspect_sections(
            member,
            ReviewInspectParams {
                review: review_ref.to_owned(),
                fields: None,
            },
            ReviewInspectSections {
                rounds,
                reviewers,
                findings,
            },
        )
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "the office could not answer"))?
}

fn storage_err(id: u64) -> Response {
    Response::error(
        id,
        ApiError::new(ErrorCode::StorageError, "the office could not answer"),
    )
}
