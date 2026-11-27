//! Read handlers for `review.list`, `review.inspect` (with `--fields` pruning, `pin_stale`, and
//! `anchor_stale`), and `review.diff` (bare-cache bounded diffs).

use org_git::{DEFAULT_DIFF_CONTEXT_LINES, MAX_DIFF_CONTEXT_LINES};
use org_office::reviews::features::ReviewInspectSections;
use org_office::Office;
use org_proto::reviews::{
    ReviewDiff, ReviewDiffParams, ReviewInspect, ReviewInspectParams, ReviewInspectResult,
    ReviewInspectSelection, ReviewList, ReviewListParams, ReviewTargetKind,
};
use org_proto::{ApiError, Call, ErrorCode, Outcome, Response};

use super::fields::prune_review_inspect;
use super::git::{fit_diff_frame, DiffQuery, ReviewGit};

pub(super) fn handle_list(
    office: &Office,
    git: &ReviewGit,
    request_id: u64,
    member: &str,
    params: ReviewListParams,
) -> Response {
    let mut result = match office.review_list(member, params) {
        Ok(Ok(res)) => res,
        Ok(Err(refusal)) => return Response::error(request_id, refusal),
        Err(err) => {
            crate::logged(ReviewList::METHOD, &err);
            return storage_err(request_id);
        }
    };
    let has_code = result
        .reviews
        .iter()
        .any(|r| r.target_kind == ReviewTargetKind::Code);
    if has_code {
        let state = git.load_state(office);
        for review in &mut result.reviews {
            if review.target_kind != ReviewTargetKind::Code {
                continue;
            }
            if let Some(head) = current_round_head(office, member, &review.id, review.current_round)
            {
                review.pin_stale = git.is_pin_stale(office, &state, review, &head);
            }
        }
    }
    Response::ok::<ReviewList>(request_id, &result).unwrap_or_else(|_| storage_err(request_id))
}

pub(super) fn handle_inspect(
    office: &Office,
    git: &ReviewGit,
    request_id: u64,
    member: &str,
    params: ReviewInspectParams,
) -> Response {
    let selection = match ReviewInspectSelection::try_from_fields(params.fields.as_deref()) {
        Ok(sel) => sel,
        Err(msg) => {
            return Response::error(request_id, ApiError::new(ErrorCode::InvalidRequest, msg))
        }
    };
    let want_pruned = params.fields.is_some();
    // Load rounds internally for `pin_stale` / `anchor_stale` computation on code reviews, then
    // drop unrequested sections via `prune_review_inspect`.
    let sections = ReviewInspectSections {
        rounds: true,
        reviewers: selection.reviewers,
        findings: selection.findings,
    };
    let mut result = match office.review_inspect_sections(member, params, sections) {
        Ok(Ok(res)) => res,
        Ok(Err(refusal)) => return Response::error(request_id, refusal),
        Err(err) => {
            crate::logged(ReviewInspect::METHOD, &err);
            return storage_err(request_id);
        }
    };
    enrich_inspect_staleness(office, git, &mut result);
    if !want_pruned {
        return Response::ok::<ReviewInspect>(request_id, &result)
            .unwrap_or_else(|_| storage_err(request_id));
    }
    match serde_json::to_value(&result) {
        Ok(val) => Response {
            id: request_id,
            outcome: Outcome::Result(prune_review_inspect(val, selection)),
            waiting_messages: None,
        },
        Err(_) => storage_err(request_id),
    }
}

pub(super) fn handle_diff(
    office: &Office,
    git: &ReviewGit,
    request_id: u64,
    member: &str,
    params: ReviewDiffParams,
) -> Response {
    // Visibility & active-employee check happens BEFORE touching git-state.json or bare caches!
    let inspected = match office.review_inspect_sections(
        member,
        ReviewInspectParams {
            review: params.review.clone(),
            fields: None,
        },
        ReviewInspectSections {
            rounds: true,
            reviewers: false,
            findings: false,
        },
    ) {
        Ok(Ok(res)) => res,
        Ok(Err(refusal)) => return Response::error(request_id, refusal),
        Err(err) => {
            crate::logged(ReviewDiff::METHOD, &err);
            return storage_err(request_id);
        }
    };
    let diff = match run_review_diff(office, git, request_id, &inspected, &params) {
        Ok(res) => res,
        Err(refusal) => return Response::error(request_id, refusal),
    };
    Response::ok::<ReviewDiff>(request_id, &diff).unwrap_or_else(|_| storage_err(request_id))
}

fn run_review_diff(
    office: &Office,
    git: &ReviewGit,
    request_id: u64,
    inspected: &ReviewInspectResult,
    params: &ReviewDiffParams,
) -> Result<org_proto::reviews::ReviewDiffResult, ApiError> {
    if inspected.review.target_kind != ReviewTargetKind::Code {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "review.diff is only supported for code reviews",
        ));
    }
    let repo_id = inspected.review.repository_id.as_deref().ok_or_else(|| {
        ApiError::new(
            ErrorCode::InvalidRequest,
            "code review has no repository_id",
        )
    })?;
    let round_num = params.round.unwrap_or(inspected.review.current_round);
    let target_round = inspected
        .rounds
        .iter()
        .find(|r| r.round_number == round_num)
        .ok_or_else(|| {
            ApiError::new(
                ErrorCode::NotFound,
                format!("review round {round_num} not found"),
            )
        })?;
    let head_oid = target_round
        .head_oid
        .as_deref()
        .ok_or_else(|| ApiError::new(ErrorCode::InvalidRequest, "round has no pinned head_oid"))?;
    let base_oid = match params.since_round {
        Some(since) => {
            if since == 0 || since >= round_num {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "since_round must be at least 1 and strictly less than round",
                ));
            }
            inspected
                .rounds
                .iter()
                .find(|r| r.round_number == since)
                .and_then(|r| r.head_oid.as_deref())
                .ok_or_else(|| {
                    ApiError::new(
                        ErrorCode::NotFound,
                        format!("review round {since} not found"),
                    )
                })?
        }
        None => target_round.base_oid.as_deref().ok_or_else(|| {
            ApiError::new(ErrorCode::InvalidRequest, "round has no pinned base_oid")
        })?,
    };
    let context_lines = params.context_lines.unwrap_or(DEFAULT_DIFF_CONTEXT_LINES);
    if context_lines > MAX_DIFF_CONTEXT_LINES {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!("context_lines must be at most {MAX_DIFF_CONTEXT_LINES}"),
        ));
    }
    let raw = git.diff_review_round(
        office,
        &DiffQuery {
            review_id: &inspected.review.id,
            repository_id: repo_id,
            round_number: round_num,
            since_round: params.since_round,
            base_oid,
            head_oid,
            path_filter: params.path.as_deref(),
            stat_only: params.stat_only,
            context_lines,
        },
    )?;
    Ok(fit_diff_frame(request_id, raw))
}

fn enrich_inspect_staleness(office: &Office, git: &ReviewGit, result: &mut ReviewInspectResult) {
    if result.review.target_kind != ReviewTargetKind::Code {
        return;
    }
    let Some(current_head) = result
        .rounds
        .iter()
        .find(|r| r.round_number == result.review.current_round)
        .and_then(|r| r.head_oid.as_deref())
    else {
        return;
    };
    let state = git.load_state(office);
    result.review.pin_stale = git.is_pin_stale(office, &state, &result.review, current_head);
    if let Some(repo_id) = result.review.repository_id.as_deref() {
        for finding in &mut result.findings {
            finding.anchor_stale =
                git.is_anchor_stale(office, &state, repo_id, current_head, &finding.anchor);
        }
    }
}

fn current_round_head(
    office: &Office,
    member: &str,
    review_id: &str,
    current_round: u32,
) -> Option<String> {
    let inspected = office
        .review_inspect_sections(
            member,
            ReviewInspectParams {
                review: review_id.to_owned(),
                fields: None,
            },
            ReviewInspectSections {
                rounds: true,
                reviewers: false,
                findings: false,
            },
        )
        .ok()?
        .ok()?;
    inspected
        .rounds
        .into_iter()
        .find(|r| r.round_number == current_round)
        .and_then(|r| r.head_oid)
}

fn storage_err(id: u64) -> Response {
    Response::error(
        id,
        ApiError::new(ErrorCode::StorageError, "the office could not answer"),
    )
}
