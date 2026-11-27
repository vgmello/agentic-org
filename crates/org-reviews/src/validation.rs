//! Single authorization choke-point and domain validators for universal reviews (`ADR-0026`, `ADR-0031`).

use std::collections::BTreeSet;

use org_proto::reviews::{
    ReviewFindingDraft, ReviewRoutingSpec, ReviewSessionMode, RoutingMode,
    REVIEW_CLOSE_REASON_MAX_CHARS, REVIEW_FINDING_BODY_MAX_CHARS, REVIEW_MAX_ATTACHMENTS,
    REVIEW_PATH_MAX_LEN, REVIEW_REPLY_BODY_MAX_CHARS, REVIEW_SLUG_MAX_LEN, REVIEW_TITLE_MAX_CHARS,
};
use org_proto::{ApiError, ConflictDetail, ErrorCode};
use org_store::{Id, Refusal};

use crate::error::{api_refusal, parse_uuid_v7_refusal, storage_api_refusal};

use super::ports::{ReviewActorInfo, ReviewContextRead, ReviewProjectInfo};
use super::records::{ResolvedCodePin, ReviewRecord, ReviewReviewerRecord};

pub const REVIEW_NOT_FOUND: &str = "review not found";
pub const PROJECT_NOT_FOUND: &str = "project not found";

pub(crate) fn refuse(c: ErrorCode, m: impl Into<String>) -> Refusal {
    api_refusal(c, m)
}
pub(crate) fn bad_req(m: impl Into<String>) -> Refusal {
    api_refusal(ErrorCode::InvalidRequest, m)
}
pub(crate) fn too_large(m: impl Into<String>) -> Refusal {
    api_refusal(ErrorCode::TooLarge, m)
}
pub(crate) fn not_found(m: impl Into<String>) -> Refusal {
    api_refusal(ErrorCode::NotFound, m)
}
pub(crate) fn unauth(m: impl Into<String>) -> Refusal {
    api_refusal(ErrorCode::NotAuthorized, m)
}
pub(crate) fn conflict(m: impl Into<String>) -> Refusal {
    api_refusal(ErrorCode::Conflict, m)
}
pub(crate) fn storage_refusal(e: impl std::fmt::Display) -> Refusal {
    storage_api_refusal("review storage error", e)
}

pub(crate) fn conflict_revision(expected: u64, current: u64) -> Refusal {
    let detail = ConflictDetail {
        expected_revision: expected,
        current_revision: current,
        changed_sections: Vec::new(),
        events_since: Vec::new(),
        events_truncated: false,
        omitted_events: 0,
        reread_recommended: true,
        current_summary: None,
    };
    let msg = format!("stale expected_revision {expected}: review is at revision {current}");
    Refusal::rejected(
        serde_json::to_vec(&ApiError::conflict(msg, detail)).expect("ApiError encodes"),
    )
}

pub fn require_active_caller<C: ReviewContextRead>(
    ctx: &C,
    caller_id: &str,
) -> Result<ReviewActorInfo, Refusal> {
    let id = Id::parse(caller_id).map_err(|_| unauth("invalid caller employee id"))?;
    let emp = ctx
        .employee_info(&id)
        .map_err(storage_refusal)?
        .ok_or_else(|| unauth("unknown employee"))?;
    if !emp.active {
        return Err(unauth("inactive employee"));
    }
    Ok(emp)
}

pub fn may_see_project<C: ReviewContextRead>(
    ctx: &C,
    caller: &ReviewActorInfo,
    p: &ReviewProjectInfo,
) -> Result<bool, Refusal> {
    if caller.is_person {
        return Ok(true);
    }
    let Some(chain) = ctx
        .team_chain(&caller.home_team_id)
        .map_err(storage_refusal)?
    else {
        return Ok(false);
    };
    Ok(chain
        .iter()
        .any(|t| *t == p.owner_team_id || p.contributing_team_ids.contains(t)))
}

pub fn require_project_scope<C: ReviewContextRead>(
    ctx: &C,
    caller: &ReviewActorInfo,
    pref: &str,
    write: bool,
) -> Result<ReviewProjectInfo, Refusal> {
    let s = pref.trim();
    if s.is_empty() {
        return Err(bad_req("project_id cannot be empty"));
    }
    let proj = ctx
        .resolve_project_info(s)
        .map_err(storage_refusal)?
        .ok_or_else(|| not_found(PROJECT_NOT_FOUND))?;
    if !may_see_project(ctx, caller, &proj)? {
        return Err(not_found(PROJECT_NOT_FOUND));
    }
    if write && proj.archived {
        return Err(unauth("project is archived"));
    }
    Ok(proj)
}

pub fn may_see_review<C: ReviewContextRead>(
    ctx: &C,
    caller: &ReviewActorInfo,
    r: &ReviewRecord,
    rrs: &[ReviewReviewerRecord],
) -> Result<bool, Refusal> {
    let Some(pid) = r.project_id.as_ref() else {
        return Ok(true);
    };
    if caller.is_person
        || r.created_by == caller.id
        || rrs.iter().any(|rr| {
            rr.round_number == r.current_round && !rr.removed && rr.reviewer_id == caller.id
        })
    {
        return Ok(true);
    }
    Ok(ctx
        .project_info(pid)
        .map_err(storage_refusal)?
        .map(|p| may_see_project(ctx, caller, &p))
        .transpose()?
        .unwrap_or(false))
}

pub fn validate_uuid(raw: &str, field: &str) -> Result<Id, Refusal> {
    parse_uuid_v7_refusal(raw, field)
}

pub fn validate_text_limit(raw: &str, max: usize, field: &str) -> Result<String, Refusal> {
    if raw.chars().count() > max {
        return Err(too_large(format!("{field} exceeds {max} chars")));
    }
    Ok(raw.to_owned())
}

pub fn validate_title(raw: &str) -> Result<String, Refusal> {
    let s = raw.trim();
    if s.is_empty() || s.contains(['\n', '\r']) {
        return Err(bad_req("title must be non-empty single line"));
    }
    validate_text_limit(s, REVIEW_TITLE_MAX_CHARS, "title")
}

pub fn validate_close_reason(raw: Option<&str>) -> Result<Option<String>, Refusal> {
    let Some(t) = raw else { return Ok(None) };
    let s = t.trim();
    if s.is_empty() {
        return Err(bad_req("close reason cannot be blank"));
    }
    validate_text_limit(s, REVIEW_CLOSE_REASON_MAX_CHARS, "reason").map(Some)
}

pub fn validate_slug(raw: &str, field: &str) -> Result<String, Refusal> {
    let s = raw.trim();
    let b = s.as_bytes();
    let edge = |c: u8| c.is_ascii_lowercase() || c.is_ascii_digit();
    if b.is_empty()
        || b.len() > REVIEW_SLUG_MAX_LEN
        || !edge(b[0])
        || !edge(b[b.len() - 1])
        || !b.iter().all(|&c| edge(c) || c == b'-')
        || s.contains("--")
    {
        return Err(bad_req(format!("{field} must be a lowercase slug")));
    }
    Ok(s.to_owned())
}

pub fn validate_oid(raw: &str, field: &str) -> Result<String, Refusal> {
    let s = raw.trim();
    if !(s.len() == 40 || s.len() == 64)
        || !s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(bad_req(format!("{field} must be a 40- or 64-char hex OID")));
    }
    Ok(s.to_owned())
}

pub fn validate_ref_token(raw: &str, no_space: bool, label: &str) -> Result<String, Refusal> {
    let s = raw.trim();
    let bad_branch = no_space
        && (s.starts_with('-')
            || s.contains("@{")
            || s.chars()
                .any(|c| matches!(c, '\\' | '~' | '^' | ':' | '?' | '*' | '[')));
    if s.is_empty()
        || s.len() > REVIEW_PATH_MAX_LEN
        || s.contains("..")
        || bad_branch
        || s.chars()
            .any(|c| c.is_control() || (no_space && c.is_whitespace()))
    {
        return Err(bad_req(format!("invalid {label}")));
    }
    Ok(s.to_owned())
}

pub fn validate_string_list(
    items: &[String],
    max_n: usize,
    max_c: usize,
    field: &str,
    distinct: bool,
) -> Result<Vec<String>, Refusal> {
    if items.len() > max_n {
        return Err(too_large(format!("{field} exceeds {max_n}")));
    }
    let mut seen = BTreeSet::new();
    items
        .iter()
        .map(|i| {
            let s = i.trim();
            if s.is_empty()
                || s.chars().any(char::is_control)
                || (distinct && !seen.insert(s.to_owned()))
            {
                return Err(bad_req(format!("invalid {field} entry")));
            }
            validate_text_limit(s, max_c, field)
        })
        .collect()
}

pub fn validate_attachment_ids(items: &[String]) -> Result<Vec<String>, Refusal> {
    validate_string_list(
        items,
        REVIEW_MAX_ATTACHMENTS,
        REVIEW_PATH_MAX_LEN,
        "attachment_ids",
        true,
    )
}

pub fn validate_finding_draft(draft: &ReviewFindingDraft) -> Result<(Id, Vec<String>), Refusal> {
    let id = validate_uuid(&draft.finding_id, "finding_id")?;
    if draft.body_markdown.trim().is_empty() {
        return Err(bad_req("finding body cannot be empty"));
    }
    validate_text_limit(
        &draft.body_markdown,
        REVIEW_FINDING_BODY_MAX_CHARS,
        "body_markdown",
    )?;
    draft.anchor.check().map_err(bad_req)?;
    Ok((id, validate_attachment_ids(&draft.attachment_ids)?))
}

pub fn validate_reply_body(body: &str, has_status: bool) -> Result<String, Refusal> {
    validate_text_limit(body, REVIEW_REPLY_BODY_MAX_CHARS, "body_markdown")?;
    if body.trim().is_empty() && !has_status {
        return Err(bad_req("reply requires body or set_status"));
    }
    Ok(body.to_owned())
}

pub fn effective_session_mode(
    ind: bool,
    req: Option<ReviewSessionMode>,
    def: ReviewSessionMode,
) -> Result<ReviewSessionMode, Refusal> {
    if ind && req == Some(ReviewSessionMode::Warm) {
        return Err(bad_req("independent review requires fresh mode"));
    }
    Ok(if ind {
        ReviewSessionMode::Fresh
    } else {
        req.unwrap_or(def)
    })
}

pub fn check_resolved_code_pin(
    pin: &ResolvedCodePin,
    exp_head: Option<&str>,
) -> Result<(String, String), Refusal> {
    let (base, head) = (
        validate_oid(&pin.base_oid, "base_oid")?,
        validate_oid(&pin.head_oid, "head_oid")?,
    );
    if let Some(exp) = exp_head {
        if validate_oid(exp, "expected_head_oid")? != head {
            return Err(conflict("expected_head_oid mismatch"));
        }
    }
    Ok((base, head))
}

pub struct ResolvedRouting {
    pub mode: RoutingMode,
    pub team_id: Option<Id>,
    pub reviewer_ids: Vec<Id>,
}

fn pick_seed<T>(mut items: Vec<T>, seed: u64) -> Option<T> {
    (!items.is_empty()).then(|| items.swap_remove((seed.saturating_sub(1) as usize) % items.len()))
}

fn is_excluded_independent(id: &Id, excluded: &[&Id], ind: bool) -> bool {
    ind && excluded.iter().any(|ex| **ex == *id)
}

fn load_reviewer<C: ReviewContextRead>(
    ctx: &C,
    raw: &str,
    excluded: &[&Id],
    ind: bool,
    agent: bool,
) -> Result<Id, Refusal> {
    let id = validate_uuid(raw, "reviewer_id")?;
    let emp = ctx
        .employee_info(&id)
        .map_err(storage_refusal)?
        .ok_or_else(|| not_found("reviewer not found"))?;
    if !emp.active || (agent && emp.is_person) {
        return Err(bad_req("reviewer is not an eligible active employee"));
    }
    if is_excluded_independent(&emp.id, excluded, ind) {
        return Err(bad_req(
            "independent review forbids author or round submitter as reviewer",
        ));
    }
    Ok(emp.id)
}

fn pick_default_agent<C: ReviewContextRead>(
    ctx: &C,
    proj: Option<&ReviewProjectInfo>,
    excluded: &[&Id],
    ind: bool,
    seed: u64,
) -> Result<Option<Id>, Refusal> {
    let mut eligible: Vec<_> = ctx
        .all_employees()
        .map_err(storage_refusal)?
        .into_iter()
        .filter(|e| {
            e.active
                && !e.is_person
                && !e.is_builtin
                && !is_excluded_independent(&e.id, excluded, ind)
        })
        .collect();
    eligible.sort_by(|a, b| a.id.cmp(&b.id));
    if let Some(p) = proj {
        let mut scoped = Vec::new();
        for e in &eligible {
            if may_see_project(ctx, e, p)? {
                scoped.push(e.id.clone());
            }
        }
        return Ok(pick_seed(scoped, seed));
    }
    Ok(pick_seed(
        eligible.into_iter().map(|e| e.id).collect(),
        seed,
    ))
}

pub fn resolve_routing<C: ReviewContextRead>(
    ctx: &C,
    spec: Option<&ReviewRoutingSpec>,
    proj: Option<&ReviewProjectInfo>,
    excluded: &[&Id],
    ind: bool,
    seed: u64,
) -> Result<ResolvedRouting, Refusal> {
    let Some(spec) = spec else {
        let ids = pick_default_agent(ctx, proj, excluded, ind, seed)?
            .into_iter()
            .collect();
        return Ok(ResolvedRouting {
            mode: RoutingMode::SingleAgent,
            team_id: None,
            reviewer_ids: ids,
        });
    };
    spec.check().map_err(bad_req)?;
    let mut team_id = None;
    let reviewer_ids = match spec.mode {
        RoutingMode::Explicit => spec
            .reviewer_ids
            .iter()
            .map(|r| load_reviewer(ctx, r, excluded, ind, false))
            .collect::<Result<Vec<_>, _>>()?,
        RoutingMode::Pool => {
            let tid = validate_uuid(
                spec.team_id
                    .as_deref()
                    .ok_or_else(|| bad_req("pool routing needs team_id"))?,
                "team_id",
            )?;
            if !ctx.team_exists_active(&tid).map_err(storage_refusal)? {
                return Err(not_found("active team not found"));
            }
            let mut cands: Vec<Id> = ctx
                .all_employees()
                .map_err(storage_refusal)?
                .into_iter()
                .filter(|e| {
                    e.active
                        && !e.is_builtin
                        && e.home_team_id == tid
                        && !is_excluded_independent(&e.id, excluded, ind)
                })
                .map(|e| e.id)
                .collect();
            cands.sort();
            team_id = Some(tid);
            vec![pick_seed(cands, seed).ok_or_else(|| bad_req("no eligible reviewer in pool"))?]
        }
        RoutingMode::SingleAgent => vec![match spec.reviewer_ids.first() {
            Some(raw) => load_reviewer(ctx, raw, excluded, ind, true)?,
            None => pick_default_agent(ctx, proj, excluded, ind, seed)?
                .ok_or_else(|| bad_req("no eligible reviewer agent"))?,
        }],
    };
    Ok(ResolvedRouting {
        mode: spec.mode,
        team_id,
        reviewer_ids,
    })
}
