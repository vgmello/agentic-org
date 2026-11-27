//! `review.request`: create a review and immutable round 1, allocating `RV-<OFFICE_CODE><N>`.

use org_proto::reviews::{
    ReviewRequestParams, ReviewRoundResult, ReviewSessionMode, ReviewStatus, ReviewTargetKind,
    REVIEW_BODY_MAX_CHARS, REVIEW_CRITERION_MAX_CHARS, REVIEW_GUIDANCE_REF_MAX_CHARS,
    REVIEW_MAX_CRITERIA, REVIEW_MAX_GUIDANCE_REFS,
};
use org_proto::ErrorCode;
use org_store::{Change, Id, Refusal};
use serde_json::json;

use crate::time::format_rfc3339_ms;

use super::super::files::counter_path;
use super::super::ports::{ReviewActorInfo, ReviewContextRead, ReviewProjectInfo, ReviewsRead};
use super::super::records::{
    encode_counter, review_to_view, round_to_view, OfficeReviewCounterRecord, ResolvedCodePin,
    ReviewDocument, ReviewRecord, ReviewReviewerRecord, ReviewRoundRecord,
};
use super::super::validation::{
    bad_req, check_resolved_code_pin, effective_session_mode, may_see_project, not_found, refuse,
    require_active_caller, require_project_scope, resolve_routing, storage_refusal, unauth,
    validate_oid, validate_ref_token, validate_slug, validate_string_list, validate_text_limit,
    validate_title, validate_uuid,
};
use super::{build_event, commit_doc, require_org_id};

struct ResolvedTarget {
    project: Option<ReviewProjectInfo>,
    task_id: Option<Id>,
    repository_id: Option<Id>,
    branch: Option<String>,
    target_ref: Option<String>,
    base_oid: Option<String>,
    head_oid: Option<String>,
}

fn resolve_scope<C: ReviewContextRead>(
    ctx: &C,
    caller: &ReviewActorInfo,
    p: &ReviewRequestParams,
) -> Result<(Option<ReviewProjectInfo>, Option<Id>), Refusal> {
    let mut project = p
        .project_id
        .as_deref()
        .map(|r| require_project_scope(ctx, caller, r, true))
        .transpose()?;
    let Some(tref) = p.task_id.as_deref() else {
        return Ok((project, None));
    };
    let task = ctx
        .resolve_task_info(tref)
        .map_err(storage_refusal)?
        .ok_or_else(|| not_found("task not found"))?;
    match task.project_id.as_ref() {
        Some(pid) => {
            let proj = ctx
                .project_info(pid)
                .map_err(storage_refusal)?
                .ok_or_else(|| not_found("task not found"))?;
            if !may_see_project(ctx, caller, &proj)? {
                return Err(not_found("task not found"));
            }
            if let Some(exp) = project.as_ref() {
                if exp.id != *pid {
                    return Err(bad_req("task does not belong to project"));
                }
            } else {
                if proj.archived {
                    return Err(unauth("project is archived"));
                }
                project = Some(proj);
            }
        }
        None if project.is_some() => return Err(bad_req("task does not belong to project")),
        None => {}
    }
    Ok((project, Some(task.id)))
}

fn resolve_target<C: ReviewContextRead>(
    ctx: &C,
    caller: &ReviewActorInfo,
    p: &ReviewRequestParams,
    pin: Option<&ResolvedCodePin>,
) -> Result<ResolvedTarget, Refusal> {
    let (project, task_id) = resolve_scope(ctx, caller, p)?;
    let has_code = p.repository_id.is_some()
        || p.branch.is_some()
        || p.base_oid.is_some()
        || p.expected_head_oid.is_some();
    let mut t = ResolvedTarget {
        project,
        task_id,
        repository_id: None,
        branch: None,
        target_ref: None,
        base_oid: None,
        head_oid: None,
    };
    match p.target_kind {
        ReviewTargetKind::Code => {
            if p.target_ref.is_some() {
                return Err(bad_req("code review forbids target_ref"));
            }
            let repo_raw = p
                .repository_id
                .as_deref()
                .ok_or_else(|| bad_req("code review requires repository_id"))?;
            let rid = validate_uuid(repo_raw, "repository_id")?;
            if !ctx.repository_exists(&rid).map_err(storage_refusal)? {
                return Err(not_found("repository not found"));
            }
            let branch_raw = p
                .branch
                .as_deref()
                .ok_or_else(|| bad_req("code review requires branch"))?;
            let branch = validate_ref_token(branch_raw, true, "branch")?;
            let (base, head) = match pin {
                Some(cp) => {
                    let (b, h) = check_resolved_code_pin(cp, p.expected_head_oid.as_deref())?;
                    if let Some(raw_base) = p.base_oid.as_deref() {
                        if validate_oid(raw_base, "base_oid")? != b {
                            return Err(refuse(ErrorCode::Conflict, "base_oid mismatch"));
                        }
                    }
                    (b, h)
                }
                None => {
                    let need = || bad_req("code review requires base_oid and expected_head_oid");
                    (
                        validate_oid(p.base_oid.as_deref().ok_or_else(need)?, "base_oid")?,
                        validate_oid(
                            p.expected_head_oid.as_deref().ok_or_else(need)?,
                            "expected_head_oid",
                        )?,
                    )
                }
            };
            (t.repository_id, t.branch, t.base_oid, t.head_oid) =
                (Some(rid), Some(branch), Some(base), Some(head));
        }
        ReviewTargetKind::Task => {
            if t.task_id.is_none() || has_code || p.target_ref.is_some() {
                return Err(bad_req("invalid task review target"));
            }
        }
        ReviewTargetKind::Doc | ReviewTargetKind::Artifact => {
            if has_code {
                return Err(bad_req("doc/artifact review forbids code fields"));
            }
            let raw = p
                .target_ref
                .as_deref()
                .ok_or_else(|| bad_req("target_ref is required"))?;
            t.target_ref = Some(validate_ref_token(raw, false, "target_ref")?);
        }
    }
    Ok(t)
}

fn allocate_number<V: ReviewsRead + ReviewContextRead>(
    view: &V,
    office_id: &Id,
) -> Result<(String, u64, OfficeReviewCounterRecord), Refusal> {
    let code = view
        .active_office_code(office_id)
        .map_err(storage_refusal)?
        .ok_or_else(|| refuse(ErrorCode::Unavailable, "no active office code"))?;
    let prev = view.office_review_counter(&code).map_err(storage_refusal)?;
    let issued = view.max_review_number(&code).map_err(storage_refusal)?;
    if prev
        .as_ref()
        .map_or(issued > 0, |c| c.next_number <= issued)
    {
        return Err(refuse(
            ErrorCode::Unavailable,
            format!("review counter for `{code}` behind RV-{code}{issued}"),
        ));
    }
    let num = prev.map_or(1, |c| c.next_number);
    let next_number = num
        .checked_add(1)
        .filter(|n| *n <= i64::MAX as u64)
        .ok_or_else(|| refuse(ErrorCode::Unavailable, "counter overflow"))?;
    Ok((
        code.clone(),
        num,
        OfficeReviewCounterRecord {
            office_id: office_id.clone(),
            office_code: code,
            next_number,
        },
    ))
}

fn build_initial_review(
    office_id: &Id,
    office_code: String,
    number: u64,
    caller_id: &Id,
    p: &ReviewRequestParams,
    target: &mut ResolvedTarget,
    at: &str,
) -> Result<ReviewRecord, Refusal> {
    Ok(ReviewRecord {
        id: validate_uuid(&p.review_id, "review_id")?,
        office_id: office_id.clone(),
        office_code,
        number,
        project_id: target.project.as_ref().map(|pr| pr.id.clone()),
        task_id: target.task_id.take(),
        target_kind: p.target_kind,
        repository_id: target.repository_id.take(),
        branch: target.branch.take(),
        target_ref: target.target_ref.take(),
        kind_slug: validate_slug(&p.kind_slug, "kind_slug")?,
        category_slug: p
            .category_slug
            .as_deref()
            .map(|s| validate_slug(s, "category_slug"))
            .transpose()?,
        title: validate_title(&p.title)?,
        body_markdown: validate_text_limit(
            &p.body_markdown,
            REVIEW_BODY_MAX_CHARS,
            "body_markdown",
        )?,
        independent: p.independent,
        status: ReviewStatus::Open,
        current_round: 1,
        session_mode: effective_session_mode(
            p.independent,
            p.session_mode,
            ReviewSessionMode::Warm,
        )?,
        guidance_refs: validate_string_list(
            &p.guidance_refs,
            REVIEW_MAX_GUIDANCE_REFS,
            REVIEW_GUIDANCE_REF_MAX_CHARS,
            "guidance_refs",
            false,
        )?,
        criteria: validate_string_list(
            &p.criteria,
            REVIEW_MAX_CRITERIA,
            REVIEW_CRITERION_MAX_CHARS,
            "criteria",
            false,
        )?,
        closed_reason: None,
        revision: 1,
        created_by: caller_id.clone(),
        created_at: at.to_owned(),
        updated_at: at.to_owned(),
        closed_at: None,
    })
}

pub fn decide_request<V: ReviewsRead + ReviewContextRead>(
    view: &V,
    office_id: &Id,
    now_ms: u64,
    caller_id: &str,
    p: &ReviewRequestParams,
    pin: Option<&ResolvedCodePin>,
) -> Result<Change, Refusal> {
    let caller = require_active_caller(view, caller_id)?;
    let org_id = require_org_id(view)?;
    let _ = validate_uuid(&p.command_id, "command_id")?;
    let rid = validate_uuid(&p.review_id, "review_id")?;
    let mut target = resolve_target(view, &caller, p, pin)?;
    if view.review(&rid).map_err(storage_refusal)?.is_some() {
        return Err(refuse(ErrorCode::AlreadyExists, "review_id already exists"));
    }
    let (code, number, counter) = allocate_number(view, office_id)?;
    let at = format_rfc3339_ms(now_ms);
    let review = build_initial_review(office_id, code, number, &caller.id, p, &mut target, &at)?;
    let routing = resolve_routing(
        view,
        p.routing.as_ref(),
        target.project.as_ref(),
        &[&caller.id],
        p.independent,
        number,
    )?;
    let mode = review.session_mode;
    let round = ReviewRoundRecord {
        review_id: rid.clone(),
        round_number: 1,
        base_oid: target.base_oid,
        head_oid: target.head_oid,
        session_mode: mode,
        submitted_by: caller.id.clone(),
        submitted_at: at.clone(),
        note_markdown: String::new(),
    };
    let reviewers: Vec<_> = routing
        .reviewer_ids
        .iter()
        .map(|id| {
            let r = ReviewReviewerRecord::assigned(
                rid.clone(),
                1,
                id.clone(),
                routing.mode,
                mode,
                caller.id.clone(),
                at.clone(),
            );
            r.with_context(routing.team_id.clone(), None)
        })
        .collect();
    let rids: Vec<&str> = routing.reviewer_ids.iter().map(Id::as_str).collect();
    let payload = json!({
        "key": review.key(), "target_kind": review.target_kind.as_str(), "kind_slug": &review.kind_slug,
        "independent": review.independent, "round_number": 1, "session_mode": mode.as_str(),
        "reviewer_ids": rids,
    });
    let ev = build_event(
        &rid,
        1,
        "review_requested",
        &caller.id,
        Some(&p.command_id),
        &payload,
        &at,
    );
    let reply = ReviewRoundResult {
        review: review_to_view(&org_id, &review, 0),
        round: round_to_view(&round, &reviewers),
    };
    let doc = ReviewDocument {
        review,
        reviewers,
        findings: Vec::new(),
        replies: Vec::new(),
    };
    Ok(commit_doc(&doc, Some(&round), &ev, &reply)
        .put(counter_path(office_id), encode_counter(&counter)))
}
