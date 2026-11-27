//! Engine-independent domain features and read queries for universal reviews (`TASK-92.1`).

pub mod findings;
pub mod mutate;
pub mod request;

use org_proto::reviews::{
    FindingStatus, ReviewInspectParams, ReviewInspectResult, ReviewListParams, ReviewListResult,
    ReviewStatus, ReviewView, REVIEW_LIST_MAX_LIMIT,
};
use org_proto::{ApiError, ErrorCode};
use org_store::{Ack, Change, Id, Refusal};
use serde::Serialize;
use serde_json::Value;

use crate::error::decode_api_refusal;

use super::files::{event_path, review_path, round_path};
use super::ports::{ReviewActorInfo, ReviewContextRead, ReviewListQuery, ReviewsRead};
use super::records::{
    canonical_json, compute_event_sha256, encode_event, encode_review_doc, encode_versioned,
    finding_to_view, review_to_view, reviewers_to_views, round_to_view, ReviewDocument,
    ReviewEventRecord, ReviewFindingRecord, ReviewFindingReplyRecord, ReviewRecord,
    ReviewReviewerRecord, ReviewRoundRecord,
};
use super::validation::{
    bad_req, conflict_revision, may_see_project, may_see_review, not_found, refuse,
    require_active_caller, require_project_scope, storage_refusal, validate_uuid, REVIEW_NOT_FOUND,
};

pub use self::findings::{decide_finding, decide_verdict};
pub use self::mutate::{decide_close, decide_resubmit, decide_route};
pub use self::request::decide_request;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReviewInspectSections {
    pub rounds: bool,
    pub reviewers: bool,
    pub findings: bool,
}

impl Default for ReviewInspectSections {
    fn default() -> Self {
        Self::ALL
    }
}

impl ReviewInspectSections {
    pub const ALL: Self = Self {
        rounds: true,
        reviewers: true,
        findings: true,
    };
}

pub(crate) struct LoadedReview {
    pub org_id: Id,
    pub review: ReviewRecord,
    pub rounds: Vec<ReviewRoundRecord>,
    pub reviewers: Vec<ReviewReviewerRecord>,
    pub findings: Vec<ReviewFindingRecord>,
    pub replies: Vec<ReviewFindingReplyRecord>,
}

impl LoadedReview {
    pub(crate) fn view(&self) -> ReviewView {
        review_to_view(
            &self.org_id,
            &self.review,
            open_findings_count(&self.findings),
        )
    }

    pub(crate) fn current_round(&self) -> Option<&ReviewRoundRecord> {
        self.rounds
            .iter()
            .find(|r| r.round_number == self.review.current_round)
    }

    pub(crate) fn bump(&mut self, at: &str) {
        self.review.revision = self.review.revision.saturating_add(1);
        self.review.updated_at = at.to_owned();
    }

    pub(crate) fn into_document(self) -> ReviewDocument {
        ReviewDocument {
            review: self.review,
            reviewers: self.reviewers,
            findings: self.findings,
            replies: self.replies,
        }
    }

    pub(crate) fn commit<T: Serialize>(
        self,
        event_type: &str,
        actor: &Id,
        cmd_id: &str,
        payload: &Value,
        at: &str,
        reply: &T,
    ) -> Change {
        let ev = build_event(
            &self.review.id,
            self.review.revision,
            event_type,
            actor,
            Some(cmd_id),
            payload,
            at,
        );
        commit_doc(&self.into_document(), None, &ev, reply)
    }
}

pub fn decode_refusal(refusal: &Refusal) -> ApiError {
    decode_api_refusal(refusal, "review operation refused")
}

pub(crate) fn open_findings_count(findings: &[ReviewFindingRecord]) -> u64 {
    findings
        .iter()
        .filter(|f| f.status == FindingStatus::Open)
        .count() as u64
}

pub(crate) fn require_org_id<C: ReviewContextRead>(ctx: &C) -> Result<Id, Refusal> {
    ctx.organization_id()
        .map_err(storage_refusal)?
        .ok_or_else(|| refuse(ErrorCode::Unavailable, "organization is not initialized"))
}

pub(crate) fn load_visible_review<V: ReviewsRead + ReviewContextRead>(
    view: &V,
    caller: &ReviewActorInfo,
    review_ref: &str,
) -> Result<LoadedReview, Refusal> {
    let org_id = require_org_id(view)?;
    let review = view
        .resolve_review(review_ref)
        .map_err(storage_refusal)?
        .ok_or_else(|| not_found(REVIEW_NOT_FOUND))?;
    let reviewers = view.review_reviewers(&review.id).map_err(storage_refusal)?;
    if !may_see_review(view, caller, &review, &reviewers)? {
        return Err(not_found(REVIEW_NOT_FOUND));
    }
    Ok(LoadedReview {
        org_id,
        rounds: view.review_rounds(&review.id).map_err(storage_refusal)?,
        findings: view.review_findings(&review.id).map_err(storage_refusal)?,
        replies: view
            .review_finding_replies(&review.id)
            .map_err(storage_refusal)?,
        review,
        reviewers,
    })
}

pub(crate) fn load_mutable_review<V: ReviewsRead + ReviewContextRead>(
    view: &V,
    caller_id: &str,
    command_id: &str,
    review_ref: &str,
    expected_revision: Option<u64>,
    closed_code: ErrorCode,
    closed_msg: &str,
) -> Result<(ReviewActorInfo, LoadedReview), Refusal> {
    let caller = require_active_caller(view, caller_id)?;
    let _ = validate_uuid(command_id, "command_id")?;
    let loaded = load_visible_review(view, &caller, review_ref)?;
    if loaded.review.status == ReviewStatus::Closed {
        return Err(refuse(closed_code, closed_msg));
    }
    if let Some(exp) = expected_revision {
        if loaded.review.revision != exp {
            return Err(conflict_revision(exp, loaded.review.revision));
        }
    }
    Ok((caller, loaded))
}

pub(crate) fn build_event(
    review_id: &Id,
    revision: u64,
    event_type: &str,
    actor_id: &Id,
    operation_id: Option<&str>,
    payload: &Value,
    at: &str,
) -> ReviewEventRecord {
    ReviewEventRecord {
        payload_sha256: compute_event_sha256(
            review_id,
            revision,
            event_type,
            actor_id,
            operation_id,
            payload,
            at,
        ),
        review_id: review_id.clone(),
        revision,
        event_type: event_type.to_owned(),
        actor_id: actor_id.clone(),
        operation_id: operation_id.map(str::to_owned),
        payload_json: canonical_json(payload),
        created_at: at.to_owned(),
    }
}

pub(crate) fn commit_doc<T: Serialize>(
    doc: &ReviewDocument,
    round: Option<&ReviewRoundRecord>,
    event: &ReviewEventRecord,
    reply: &T,
) -> Change {
    let mut c = Change::new(Ack::Durable);
    if let Some(rd) = round {
        c = c.create(
            round_path(&rd.review_id, rd.round_number),
            encode_versioned(rd),
        );
    }
    c.put(review_path(&doc.review.id), encode_review_doc(doc))
        .create(
            event_path(&event.review_id, event.revision),
            encode_event(event),
        )
        .reply(serde_json::to_vec(reply).expect("review reply serializes"))
}

fn resolve_task_filter<C: ReviewContextRead>(
    ctx: &C,
    caller: &ReviewActorInfo,
    raw: &str,
) -> Result<Option<Id>, Refusal> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(bad_req("task_id cannot be empty"));
    }
    let Some(task) = ctx.resolve_task_info(s).map_err(storage_refusal)? else {
        return Ok(None);
    };
    if let Some(pid) = task.project_id.as_ref() {
        let vis = ctx
            .project_info(pid)
            .map_err(storage_refusal)?
            .map(|p| may_see_project(ctx, caller, &p))
            .transpose()?
            .unwrap_or(false);
        if !vis {
            return Ok(None);
        }
    }
    Ok(Some(task.id))
}

pub fn list_reviews<V: ReviewsRead + ReviewContextRead>(
    view: &V,
    caller_id: &str,
    p: &ReviewListParams,
) -> Result<ReviewListResult, Refusal> {
    let caller = require_active_caller(view, caller_id)?;
    let org_id = require_org_id(view)?;
    let limit = match p.limit {
        None => 50,
        Some(n) if (1..=REVIEW_LIST_MAX_LIMIT).contains(&n) => n,
        Some(_) => {
            return Err(bad_req(format!(
                "limit must be 1..={REVIEW_LIST_MAX_LIMIT}"
            )))
        }
    };
    let project_id = p
        .project_id
        .as_deref()
        .map(|r| require_project_scope(view, &caller, r, false).map(|pr| pr.id))
        .transpose()?;
    let task_id = match p.task_id.as_deref() {
        Some(r) => match resolve_task_filter(view, &caller, r)? {
            Some(id) => Some(id),
            None => {
                return Ok(ReviewListResult {
                    reviews: Vec::new(),
                    total: 0,
                })
            }
        },
        None => None,
    };
    let q = ReviewListQuery {
        project_id,
        task_id,
        status: p.status,
        target_kind: p.target_kind,
        reviewer_id: p
            .reviewer_id
            .as_deref()
            .map(|s| validate_uuid(s, "reviewer_id"))
            .transpose()?,
        created_by: p
            .created_by
            .as_deref()
            .map(|s| validate_uuid(s, "created_by"))
            .transpose()?,
    };
    let mut visible = Vec::new();
    for r in view.list_reviews(&q).map_err(storage_refusal)? {
        if may_see_review(
            view,
            &caller,
            &r,
            &view.review_reviewers(&r.id).map_err(storage_refusal)?,
        )? {
            visible.push(r);
        }
    }
    let total = visible.len() as u64;
    let start = usize::try_from(p.offset.unwrap_or(0)).unwrap_or(usize::MAX);
    let mut reviews = Vec::new();
    for r in visible.into_iter().skip(start).take(limit as usize) {
        let open = open_findings_count(&view.review_findings(&r.id).map_err(storage_refusal)?);
        reviews.push(review_to_view(&org_id, &r, open));
    }
    Ok(ReviewListResult { reviews, total })
}

pub fn inspect_review<V: ReviewsRead + ReviewContextRead>(
    view: &V,
    caller_id: &str,
    p: &ReviewInspectParams,
    sec: ReviewInspectSections,
) -> Result<ReviewInspectResult, Refusal> {
    let caller = require_active_caller(view, caller_id)?;
    let l = load_visible_review(view, &caller, &p.review)?;
    Ok(ReviewInspectResult {
        review: l.view(),
        rounds: if sec.rounds {
            l.rounds
                .iter()
                .map(|rd| round_to_view(rd, &l.reviewers))
                .collect()
        } else {
            Vec::new()
        },
        reviewers: if sec.reviewers {
            reviewers_to_views(l.review.current_round, &l.reviewers)
        } else {
            Vec::new()
        },
        findings: if sec.findings {
            l.findings
                .iter()
                .map(|f| finding_to_view(f, &l.replies))
                .collect()
        } else {
            Vec::new()
        },
    })
}

pub fn list_events<V: ReviewsRead + ReviewContextRead>(
    view: &V,
    caller_id: &str,
    review_ref: &str,
) -> Result<Vec<ReviewEventRecord>, Refusal> {
    let caller = require_active_caller(view, caller_id)?;
    let l = load_visible_review(view, &caller, review_ref)?;
    view.review_events(&l.review.id).map_err(storage_refusal)
}
