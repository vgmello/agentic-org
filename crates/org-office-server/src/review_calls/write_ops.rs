//! Write handlers for `review.request`, `review.route`, `review.finding`, `review.verdict`,
//! `review.resubmit`, and `review.close` (`TASK-92.1`).

use org_office::reviews::features::ReviewInspectSections;
use org_office::Office;
use org_office_sessions::SessionManager;
use org_proto::reviews::{
    ReviewClose, ReviewCloseParams, ReviewFinding, ReviewFindingOp, ReviewFindingParams,
    ReviewInspectParams, ReviewInspectResult, ReviewRequest, ReviewRequestParams, ReviewResubmit,
    ReviewResubmitParams, ReviewRoute, ReviewRouteParams, ReviewTargetKind, ReviewVerdictCall,
    ReviewVerdictParams,
};
use org_proto::{ApiError, Call, ErrorCode, Response};

use crate::live::LiveEvents;

use super::attachments::resolve_review_attachments;
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
                repository_id: repo_id,
                branch,
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
            inspect_for_service(deps.office, member, &result.review.id, false, true)
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
    let inspected = match inspect_for_service(deps.office, member, &params.review, true, true) {
        Ok(res) => res,
        Err(refusal) => return Response::error(request_id, refusal),
    };
    let (event_id, preview, wake_atts) = match &mut params.op {
        ReviewFindingOp::Add(draft) => {
            if let Err(refusal) = validate_draft_anchor_and_attachments(
                deps,
                member,
                &params.command_id,
                &inspected,
                draft,
            ) {
                return Response::error(request_id, refusal);
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
    let inspected = match inspect_for_service(deps.office, member, &params.review, true, false) {
        Ok(res) => res,
        Err(refusal) => return Response::error(request_id, refusal),
    };
    match resolve_review_attachments(
        deps.office,
        member,
        &params.command_id,
        &inspected.review,
        &params.attachment_ids,
    ) {
        Ok(bound) => params.attachment_ids = bound,
        Err(refusal) => return Response::error(request_id, refusal),
    }
    for draft in &mut params.findings {
        if let Err(refusal) = validate_draft_anchor_and_attachments(
            deps,
            member,
            &params.command_id,
            &inspected,
            draft,
        ) {
            return Response::error(request_id, refusal);
        }
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
    let inspected = match inspect_for_service(deps.office, member, &params.review, true, false) {
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

fn validate_draft_anchor_and_attachments(
    deps: &WriteDeps<'_>,
    member: &str,
    command_id: &str,
    inspected: &ReviewInspectResult,
    draft: &mut org_proto::reviews::ReviewFindingDraft,
) -> Result<(), ApiError> {
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
    draft.attachment_ids = resolve_review_attachments(
        deps.office,
        member,
        command_id,
        &inspected.review,
        &draft.attachment_ids,
    )?;
    Ok(())
}

fn inspect_for_service(
    office: &Office,
    member: &str,
    review_ref: &str,
    rounds: bool,
    reviewers: bool,
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
                findings: false,
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
