//! Canonical domain records, wire projections, and deterministic JSON codecs for universal reviews.

use std::collections::BTreeMap;

use org_proto::reviews::{
    FindingAnchor, FindingSeverity, FindingStatus, ReviewFindingReplyView, ReviewFindingView,
    ReviewReviewerView, ReviewRoundView, ReviewSessionMode, ReviewStatus, ReviewTargetKind,
    ReviewVerdict, ReviewVerdictView, ReviewView, RoutingMode, REVIEW_KEY_PREFIX,
};
use org_store::{Id, Sha256};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;

pub const SCHEMA_VERSION: u64 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedCodePin {
    pub base_oid: String,
    pub head_oid: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeReviewCounterRecord {
    pub office_id: Id,
    pub office_code: String,
    pub next_number: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewRecord {
    pub id: Id,
    pub office_id: Id,
    pub office_code: String,
    pub number: u64,
    pub project_id: Option<Id>,
    pub task_id: Option<Id>,
    pub target_kind: ReviewTargetKind,
    pub repository_id: Option<Id>,
    pub branch: Option<String>,
    pub target_ref: Option<String>,
    pub kind_slug: String,
    pub category_slug: Option<String>,
    pub title: String,
    pub body_markdown: String,
    pub independent: bool,
    pub status: ReviewStatus,
    pub current_round: u32,
    pub session_mode: ReviewSessionMode,
    pub guidance_refs: Vec<String>,
    pub criteria: Vec<String>,
    pub closed_reason: Option<String>,
    pub revision: u64,
    pub created_by: Id,
    pub created_at: String,
    pub updated_at: String,
    pub closed_at: Option<String>,
}

impl ReviewRecord {
    pub fn key(&self) -> String {
        format!("{REVIEW_KEY_PREFIX}{}{}", self.office_code, self.number)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewRoundRecord {
    pub review_id: Id,
    pub round_number: u32,
    pub base_oid: Option<String>,
    pub head_oid: Option<String>,
    pub session_mode: ReviewSessionMode,
    pub submitted_by: Id,
    pub submitted_at: String,
    pub note_markdown: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewReviewerRecord {
    pub review_id: Id,
    pub round_number: u32,
    pub reviewer_id: Id,
    pub routing: RoutingMode,
    pub team_id: Option<Id>,
    pub session_mode: ReviewSessionMode,
    pub assigned_by: Id,
    pub assigned_at: String,
    pub removed: bool,
    pub verdict: Option<ReviewVerdict>,
    pub summary_markdown: String,
    pub finding_ids: Vec<Id>,
    pub attachment_ids: Vec<String>,
    pub verdict_at: Option<String>,
    pub runtime_session_id: Option<String>,
}

impl ReviewReviewerRecord {
    pub fn assigned(
        review_id: Id,
        round_number: u32,
        reviewer_id: Id,
        routing: RoutingMode,
        session_mode: ReviewSessionMode,
        assigned_by: Id,
        assigned_at: String,
    ) -> Self {
        Self {
            review_id,
            round_number,
            reviewer_id,
            routing,
            team_id: None,
            session_mode,
            assigned_by,
            assigned_at,
            removed: false,
            verdict: None,
            summary_markdown: String::new(),
            finding_ids: Vec::new(),
            attachment_ids: Vec::new(),
            verdict_at: None,
            runtime_session_id: None,
        }
    }

    pub fn with_context(mut self, team_id: Option<Id>, sid: Option<String>) -> Self {
        self.team_id = team_id;
        self.runtime_session_id = sid;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewFindingRecord {
    pub id: Id,
    pub review_id: Id,
    pub number: u64,
    pub round_number: u32,
    pub author_id: Id,
    pub severity: FindingSeverity,
    pub status: FindingStatus,
    pub anchor: FindingAnchor,
    pub body_markdown: String,
    pub attachment_ids: Vec<String>,
    pub resolved_by: Option<Id>,
    pub resolved_at: Option<String>,
    pub resolved_in_round: Option<u32>,
    pub revision: u64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewFindingReplyRecord {
    pub id: Id,
    pub finding_id: Id,
    pub review_id: Id,
    pub round_number: u32,
    pub author_id: Id,
    pub body_markdown: String,
    pub set_status: Option<FindingStatus>,
    pub attachment_ids: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewEventRecord {
    pub review_id: Id,
    pub revision: u64,
    pub event_type: String,
    pub actor_id: Id,
    pub operation_id: Option<String>,
    pub payload_json: String,
    pub payload_sha256: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewDocument {
    pub review: ReviewRecord,
    #[serde(default)]
    pub reviewers: Vec<ReviewReviewerRecord>,
    #[serde(default)]
    pub findings: Vec<ReviewFindingRecord>,
    #[serde(default)]
    pub replies: Vec<ReviewFindingReplyRecord>,
}

pub fn is_valid_office_code(code: &str) -> bool {
    let b = code.as_bytes();
    (1..=2).contains(&b.len())
        && b.iter()
            .all(|c| matches!(c, b'A'..=b'H' | b'J'..=b'N' | b'P'..=b'Z'))
}

pub fn parse_review_key(raw: &str) -> Option<(String, u64)> {
    let s = raw.trim();
    if s.len() <= REVIEW_KEY_PREFIX.len()
        || !s[..REVIEW_KEY_PREFIX.len()].eq_ignore_ascii_case(REVIEW_KEY_PREFIX)
    {
        return None;
    }
    let rest = &s[REVIEW_KEY_PREFIX.len()..];
    let split = rest.find(|c: char| c.is_ascii_digit())?;
    let code = rest[..split].to_ascii_uppercase();
    let num = &rest[split..];
    if !is_valid_office_code(&code)
        || num.is_empty()
        || num.starts_with('0')
        || !num.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let n = num.parse::<u64>().ok()?;
    (n > 0 && n <= i64::MAX as u64).then_some((code, n))
}

pub fn compute_review_status(cur: ReviewStatus, active: &[&ReviewReviewerRecord]) -> ReviewStatus {
    if cur == ReviewStatus::Closed {
        ReviewStatus::Closed
    } else if active
        .iter()
        .any(|r| r.verdict == Some(ReviewVerdict::ChangesRequested))
    {
        ReviewStatus::ChangesRequested
    } else if !active.is_empty()
        && active
            .iter()
            .all(|r| r.verdict == Some(ReviewVerdict::Approve))
    {
        ReviewStatus::Approved
    } else {
        ReviewStatus::Open
    }
}

fn opt_id(id: Option<&Id>) -> Option<String> {
    id.map(|v| v.as_str().to_owned())
}

pub fn review_to_view(org_id: &Id, r: &ReviewRecord, open_findings: u64) -> ReviewView {
    ReviewView {
        id: r.id.as_str().to_owned(),
        key: r.key(),
        office_code: r.office_code.clone(),
        number: r.number,
        organization_id: org_id.as_str().to_owned(),
        project_id: opt_id(r.project_id.as_ref()),
        task_id: opt_id(r.task_id.as_ref()),
        target_kind: r.target_kind,
        repository_id: opt_id(r.repository_id.as_ref()),
        branch: r.branch.clone(),
        target_ref: r.target_ref.clone(),
        kind_slug: r.kind_slug.clone(),
        category_slug: r.category_slug.clone(),
        title: r.title.clone(),
        body_markdown: r.body_markdown.clone(),
        independent: r.independent,
        status: r.status,
        current_round: r.current_round,
        pin_stale: false,
        guidance_refs: r.guidance_refs.clone(),
        criteria: r.criteria.clone(),
        open_findings,
        closed_reason: r.closed_reason.clone(),
        revision: r.revision,
        created_by: r.created_by.as_str().to_owned(),
        created_at: r.created_at.clone(),
        updated_at: r.updated_at.clone(),
    }
}

pub fn reviewer_verdict_to_view(r: &ReviewReviewerRecord) -> Option<ReviewVerdictView> {
    Some(ReviewVerdictView {
        reviewer_id: r.reviewer_id.as_str().to_owned(),
        round_number: r.round_number,
        verdict: r.verdict?,
        summary_markdown: r.summary_markdown.clone(),
        finding_ids: r
            .finding_ids
            .iter()
            .map(|i| i.as_str().to_owned())
            .collect(),
        attachment_ids: r.attachment_ids.clone(),
        submitted_at: r.verdict_at.clone()?,
    })
}

pub fn round_to_view(rd: &ReviewRoundRecord, rrs: &[ReviewReviewerRecord]) -> ReviewRoundView {
    let mut verdicts: Vec<_> = rrs
        .iter()
        .filter(|r| r.round_number == rd.round_number)
        .filter_map(reviewer_verdict_to_view)
        .collect();
    verdicts.sort_by(|a, b| {
        a.submitted_at
            .cmp(&b.submitted_at)
            .then(a.reviewer_id.cmp(&b.reviewer_id))
    });
    ReviewRoundView {
        round_number: rd.round_number,
        base_oid: rd.base_oid.clone(),
        head_oid: rd.head_oid.clone(),
        session_mode: rd.session_mode,
        submitted_by: rd.submitted_by.as_str().to_owned(),
        submitted_at: rd.submitted_at.clone(),
        note_markdown: rd.note_markdown.clone(),
        verdicts,
    }
}

pub fn reviewers_to_views(cur_round: u32, rrs: &[ReviewReviewerRecord]) -> Vec<ReviewReviewerView> {
    let mut grouped: BTreeMap<&Id, Vec<&ReviewReviewerRecord>> = BTreeMap::new();
    for row in rrs {
        grouped.entry(&row.reviewer_id).or_default().push(row);
    }
    let mut views: Vec<_> = grouped
        .into_values()
        .map(|mut rows| {
            rows.sort_by_key(|r| r.round_number);
            let last = rows.last().expect("non-empty group");
            let v = rows.iter().rev().find(|r| r.verdict.is_some());
            ReviewReviewerView {
                reviewer_id: last.reviewer_id.as_str().to_owned(),
                routing: last.routing,
                team_id: opt_id(last.team_id.as_ref()),
                session_mode: last.session_mode,
                assigned_by: last.assigned_by.as_str().to_owned(),
                assigned_at: last.assigned_at.clone(),
                last_verdict: v.and_then(|r| r.verdict),
                last_verdict_round: v.map(|r| r.round_number),
                removed: last.removed || last.round_number < cur_round,
            }
        })
        .collect();
    views.sort_by(|a, b| {
        a.removed
            .cmp(&b.removed)
            .then(a.assigned_at.cmp(&b.assigned_at))
            .then(a.reviewer_id.cmp(&b.reviewer_id))
    });
    views
}

pub fn finding_to_view(
    f: &ReviewFindingRecord,
    reps: &[ReviewFindingReplyRecord],
) -> ReviewFindingView {
    let mut rviews: Vec<_> = reps
        .iter()
        .filter(|rep| rep.finding_id == f.id)
        .map(|rep| ReviewFindingReplyView {
            id: rep.id.as_str().to_owned(),
            finding_id: rep.finding_id.as_str().to_owned(),
            author_id: rep.author_id.as_str().to_owned(),
            body_markdown: rep.body_markdown.clone(),
            set_status: rep.set_status,
            attachment_ids: rep.attachment_ids.clone(),
            created_at: rep.created_at.clone(),
        })
        .collect();
    rviews.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
    ReviewFindingView {
        id: f.id.as_str().to_owned(),
        review_id: f.review_id.as_str().to_owned(),
        number: f.number,
        round_number: f.round_number,
        severity: f.severity,
        status: f.status,
        anchor: f.anchor.clone(),
        anchor_stale: false,
        body_markdown: f.body_markdown.clone(),
        author_id: f.author_id.as_str().to_owned(),
        attachment_ids: f.attachment_ids.clone(),
        resolved_by: opt_id(f.resolved_by.as_ref()),
        resolved_at: f.resolved_at.clone(),
        resolved_in_round: f.resolved_in_round,
        replies: rviews,
        revision: f.revision,
        created_at: f.created_at.clone(),
        updated_at: f.updated_at.clone(),
    }
}

/// Canonical compact JSON (`serde_json::Map` is `BTreeMap` without `preserve_order`).
pub fn canonical_json(value: &Value) -> String {
    serde_json::to_string(value).expect("JSON serializes")
}

#[derive(Serialize, Deserialize)]
struct Versioned<T> {
    schema_version: u64,
    #[serde(flatten)]
    data: T,
}

pub fn encode_versioned<T: Serialize>(data: &T) -> Vec<u8> {
    let val = serde_json::to_value(Versioned {
        schema_version: SCHEMA_VERSION,
        data,
    })
    .expect("review record serializes");
    let mut s = canonical_json(&val);
    s.push('\n');
    s.into_bytes()
}

pub fn decode_versioned<T: DeserializeOwned>(bytes: &[u8], label: &str) -> Result<T, String> {
    let v: Versioned<T> =
        serde_json::from_slice(bytes).map_err(|e| format!("invalid {label}: {e}"))?;
    if v.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "unsupported {label} schema_version {}",
            v.schema_version
        ));
    }
    Ok(v.data)
}

pub fn encode_review_doc(doc: &ReviewDocument) -> Vec<u8> {
    let mut d = doc.clone();
    d.reviewers.sort_by(|a, b| {
        a.round_number
            .cmp(&b.round_number)
            .then(a.reviewer_id.cmp(&b.reviewer_id))
    });
    d.findings.sort_by_key(|f| f.number);
    d.replies
        .sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
    encode_versioned(&d)
}

pub fn encode_counter(counter: &OfficeReviewCounterRecord) -> Vec<u8> {
    encode_versioned(counter)
}

pub fn decode_counter(bytes: &[u8]) -> Result<OfficeReviewCounterRecord, String> {
    let c: OfficeReviewCounterRecord = decode_versioned(bytes, "review-counter.json")?;
    if !is_valid_office_code(&c.office_code)
        || c.next_number == 0
        || c.next_number > i64::MAX as u64
    {
        return Err("invalid office_code or next_number in review-counter.json".into());
    }
    Ok(c)
}

pub fn compute_event_sha256(
    review_id: &Id,
    revision: u64,
    event_type: &str,
    actor_id: &Id,
    operation_id: Option<&str>,
    payload: &Value,
    created_at: &str,
) -> String {
    let unsigned = serde_json::json!({
        "schema_version": SCHEMA_VERSION, "review_id": review_id.as_str(), "revision": revision,
        "event_type": event_type, "actor_id": actor_id.as_str(), "operation_id": operation_id,
        "payload": payload, "created_at": created_at,
    });
    Sha256::of(canonical_json(&unsigned).as_bytes()).to_hex()
}

pub fn encode_event(ev: &ReviewEventRecord) -> Vec<u8> {
    encode_versioned(ev)
}

pub fn decode_event(bytes: &[u8]) -> Result<ReviewEventRecord, String> {
    let mut ev: ReviewEventRecord = decode_versioned(bytes, "event file")?;
    let payload: Value = serde_json::from_str(&ev.payload_json)
        .map_err(|e| format!("invalid event payload: {e}"))?;
    if !payload.is_object() {
        return Err("event payload must be a JSON object".into());
    }
    let expected = compute_event_sha256(
        &ev.review_id,
        ev.revision,
        &ev.event_type,
        &ev.actor_id,
        ev.operation_id.as_deref(),
        &payload,
        &ev.created_at,
    );
    if ev.payload_sha256 != expected {
        return Err("event payload_sha256 mismatch".into());
    }
    ev.payload_json = canonical_json(&payload);
    Ok(ev)
}
