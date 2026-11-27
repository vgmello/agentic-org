//! Durable file layout and engine-independent projector (`ReviewFiles`) for universal reviews.

use std::collections::BTreeSet;

use org_proto::reviews::{
    FindingStatus, ReviewSessionMode, ReviewStatus, ReviewTargetKind, RoutingMode,
};
use org_store::{DurablePath, Engine, Id, ProjectionError, Projector};

use super::ports::{ReviewListQuery, ReviewsRead, ReviewsWrite};
use super::records::{
    self, decode_counter, decode_event, decode_versioned, ReviewDocument, ReviewRecord,
    ReviewRoundRecord,
};
use super::validation::validate_oid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewFileKind {
    Review(Id),
    Round(Id, u32),
    Event(Id, u64),
}

fn dpath(s: String) -> DurablePath {
    DurablePath::parse(&s).expect("valid review path")
}
pub fn review_path(id: &Id) -> DurablePath {
    dpath(format!("state/reviews/{id}/review.json"))
}
pub fn round_path(id: &Id, round: u32) -> DurablePath {
    dpath(format!("state/reviews/{id}/rounds/{round:06}.json"))
}
pub fn event_path(id: &Id, rev: u64) -> DurablePath {
    dpath(format!("state/reviews/{id}/events/{rev:010}.json"))
}
pub fn counter_path(office_id: &Id) -> DurablePath {
    dpath(format!("local/offices/{office_id}/review-counter.json"))
}

fn parse_uuid(raw: &str) -> Option<Id> {
    org_proto::is_uuid_v7(raw)
        .then(|| Id::parse(raw).ok())
        .flatten()
}

fn parse_padded(name: &str, width: usize) -> Option<u64> {
    let stem = name.strip_suffix(".json")?;
    if stem.len() != width || !stem.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let v = stem.parse::<u64>().ok()?;
    (v >= 1 && v <= i64::MAX as u64).then_some(v)
}

pub fn parse_path(path: &DurablePath) -> Option<ReviewFileKind> {
    let parts: Vec<&str> = path.components().collect();
    match parts.as_slice() {
        ["state", "reviews", rid, "review.json"] => Some(ReviewFileKind::Review(parse_uuid(rid)?)),
        ["state", "reviews", rid, "rounds", f] => Some(ReviewFileKind::Round(
            parse_uuid(rid)?,
            u32::try_from(parse_padded(f, 6)?).ok()?,
        )),
        ["state", "reviews", rid, "events", f] => Some(ReviewFileKind::Event(
            parse_uuid(rid)?,
            parse_padded(f, 10)?,
        )),
        _ => None,
    }
}

pub struct ReviewFiles {
    office_id: Option<Id>,
}

impl ReviewFiles {
    pub fn new(office_id: Id) -> Self {
        Self {
            office_id: Some(office_id),
        }
    }
    pub fn shared() -> Self {
        Self { office_id: None }
    }
    fn owns_counter(&self, path: &DurablePath) -> bool {
        self.office_id
            .as_ref()
            .is_some_and(|o| *path == counter_path(o))
    }
}

fn refuse(path: &DurablePath, reason: impl Into<String>) -> ProjectionError {
    ProjectionError::new(path, reason)
}

fn check_doc(path: &DurablePath, rid: &Id, doc: &ReviewDocument) -> Result<(), ProjectionError> {
    let hdr_ok = doc.review.id == *rid
        && records::is_valid_office_code(&doc.review.office_code)
        && doc.review.number > 0
        && doc.review.current_round > 0
        && doc.review.revision > 0;
    let children_ok = doc
        .reviewers
        .iter()
        .all(|r| r.review_id == *rid && r.round_number > 0)
        && doc
            .findings
            .iter()
            .all(|f| f.review_id == *rid && f.number > 0 && f.round_number > 0)
        && doc
            .replies
            .iter()
            .all(|r| r.review_id == *rid && r.round_number > 0);
    if !hdr_ok || !children_ok {
        return Err(refuse(path, "invalid review header or child identity"));
    }
    Ok(())
}

impl<E: Engine> Projector<E> for ReviewFiles
where
    E::Tx: ReviewsWrite,
{
    fn owns(&self, path: &DurablePath) -> bool {
        parse_path(path).is_some() || self.owns_counter(path)
    }

    fn apply(
        &self,
        tx: &mut E::Tx,
        path: &DurablePath,
        bytes: Option<&[u8]>,
    ) -> Result<(), ProjectionError> {
        if self.owns_counter(path) {
            return apply_counter(tx, path, self.office_id.as_ref(), bytes);
        }
        let kind = parse_path(path).ok_or_else(|| refuse(path, "not a review file"))?;
        let raw = bytes.ok_or_else(|| refuse(path, "review files are retained and immutable"))?;
        match kind {
            ReviewFileKind::Review(rid) => {
                let doc: ReviewDocument =
                    decode_versioned(raw, "review.json").map_err(|e| refuse(path, e))?;
                check_doc(path, &rid, &doc)?;
                tx.put_review_document(&doc)?;
            }
            ReviewFileKind::Round(rid, num) => {
                let round: ReviewRoundRecord =
                    decode_versioned(raw, "round file").map_err(|e| refuse(path, e))?;
                if round.review_id != rid || round.round_number != num {
                    return Err(refuse(path, "round identity mismatch"));
                }
                tx.put_review_round(&round)?;
            }
            ReviewFileKind::Event(rid, rev) => {
                let ev = decode_event(raw).map_err(|e| refuse(path, e))?;
                if ev.review_id != rid || ev.revision != rev {
                    return Err(refuse(path, "event identity mismatch"));
                }
                tx.put_review_event(&ev)?;
            }
        }
        Ok(())
    }

    fn validate(&self, tx: &E::Tx) -> Result<(), ProjectionError> {
        validate_reviews(tx)
    }
}

fn apply_counter<Tx: ReviewsWrite>(
    tx: &mut Tx,
    path: &DurablePath,
    office: Option<&Id>,
    bytes: Option<&[u8]>,
) -> Result<(), ProjectionError> {
    let Some(raw) = bytes else {
        for old in tx.office_review_counters()? {
            if Some(&old.office_id) == office {
                tx.remove_office_review_counter(&old.office_code)?;
            }
        }
        return Ok(());
    };
    let counter = decode_counter(raw).map_err(|e| refuse(path, e))?;
    if Some(&counter.office_id) != office {
        return Err(refuse(path, "counter office_id mismatch"));
    }
    for old in tx.office_review_counters()? {
        if old.office_id == counter.office_id && old.office_code != counter.office_code {
            tx.remove_office_review_counter(&old.office_code)?;
        }
    }
    tx.put_office_review_counter(&counter)?;
    Ok(())
}

fn target_fields_valid(r: &ReviewRecord) -> bool {
    let has_code = (r.repository_id.is_some(), r.branch.is_some());
    match r.target_kind {
        ReviewTargetKind::Code => has_code == (true, true) && r.target_ref.is_none(),
        ReviewTargetKind::Task => {
            r.task_id.is_some() && has_code == (false, false) && r.target_ref.is_none()
        }
        ReviewTargetKind::Doc | ReviewTargetKind::Artifact => {
            has_code == (false, false) && r.target_ref.is_some()
        }
    }
}

fn validate_header(path: &DurablePath, r: &ReviewRecord) -> Result<(), ProjectionError> {
    let closed_ok = (r.status == ReviewStatus::Closed) == r.closed_at.is_some()
        && (r.status == ReviewStatus::Closed || r.closed_reason.is_none());
    let mode_ok = !r.independent || r.session_mode == ReviewSessionMode::Fresh;
    if !closed_ok || !mode_ok || !target_fields_valid(r) {
        return Err(refuse(path, "invalid review header invariants"));
    }
    Ok(())
}

fn validate_rounds<T: ReviewsRead>(tx: &T, r: &ReviewRecord) -> Result<(), ProjectionError> {
    let rounds = tx.review_rounds(&r.id)?;
    let contiguous = rounds.len() == r.current_round as usize
        && rounds
            .iter()
            .enumerate()
            .all(|(i, rd)| rd.round_number == (i as u32) + 1);
    if !contiguous {
        return Err(refuse(&review_path(&r.id), "review rounds non-contiguous"));
    }
    for rd in &rounds {
        let pin_ok = match r.target_kind {
            ReviewTargetKind::Code => rd
                .base_oid
                .as_deref()
                .zip(rd.head_oid.as_deref())
                .is_some_and(|(b, h)| {
                    validate_oid(b, "base_oid").is_ok() && validate_oid(h, "head_oid").is_ok()
                }),
            _ => rd.base_oid.is_none() && rd.head_oid.is_none(),
        };
        if !pin_ok || (r.independent && rd.session_mode != ReviewSessionMode::Fresh) {
            return Err(refuse(
                &round_path(&r.id, rd.round_number),
                "invalid round snapshot",
            ));
        }
    }
    Ok(())
}

fn validate_reviewers<T: ReviewsRead>(
    tx: &T,
    r: &ReviewRecord,
    path: &DurablePath,
) -> Result<(), ProjectionError> {
    for rr in tx.review_reviewers(&r.id)? {
        let routing_ok = (rr.routing == RoutingMode::Pool) == rr.team_id.is_some();
        let verdict_ok = rr.verdict.is_some() == rr.verdict_at.is_some()
            && (rr.verdict.is_some()
                || (rr.summary_markdown.is_empty()
                    && rr.finding_ids.is_empty()
                    && rr.attachment_ids.is_empty()));
        let indep_ok = !(r.independent && !rr.removed && rr.reviewer_id == r.created_by);
        if rr.round_number > r.current_round || !routing_ok || !verdict_ok || !indep_ok {
            return Err(refuse(path, "invalid reviewer assignment"));
        }
    }
    Ok(())
}

fn validate_findings_and_replies<T: ReviewsRead>(
    tx: &T,
    r: &ReviewRecord,
    path: &DurablePath,
) -> Result<(), ProjectionError> {
    let mut fids = BTreeSet::new();
    for (idx, f) in tx.review_findings(&r.id)?.into_iter().enumerate() {
        let res_ok = match f.status {
            FindingStatus::Open => {
                f.resolved_by.is_none() && f.resolved_at.is_none() && f.resolved_in_round.is_none()
            }
            FindingStatus::Resolved | FindingStatus::WontFix => {
                f.resolved_by.is_some() && f.resolved_at.is_some() && f.resolved_in_round.is_some()
            }
        };
        if f.number != (idx as u64) + 1
            || f.round_number > r.current_round
            || f.body_markdown.is_empty()
            || !res_ok
        {
            return Err(refuse(path, "invalid finding invariants"));
        }
        f.anchor.check().map_err(|msg| refuse(path, msg))?;
        fids.insert(f.id);
    }
    for rep in tx.review_finding_replies(&r.id)? {
        if !fids.contains(&rep.finding_id) || rep.round_number > r.current_round {
            return Err(refuse(path, "invalid finding reply"));
        }
    }
    Ok(())
}

fn validate_one_review<T: ReviewsRead>(tx: &T, r: &ReviewRecord) -> Result<(), ProjectionError> {
    let path = review_path(&r.id);
    validate_header(&path, r)?;
    validate_rounds(tx, r)?;
    validate_reviewers(tx, r, &path)?;
    validate_findings_and_replies(tx, r, &path)?;
    let events = tx.review_events(&r.id)?;
    let events_ok = events.len() as u64 == r.revision
        && events
            .iter()
            .enumerate()
            .all(|(i, e)| e.revision == (i as u64) + 1);
    if !events_ok {
        return Err(refuse(&path, "review events non-contiguous"));
    }
    Ok(())
}

fn validate_reviews<T: ReviewsRead>(tx: &T) -> Result<(), ProjectionError> {
    let (mut ids, mut keys) = (BTreeSet::new(), BTreeSet::new());
    for r in tx.list_reviews(&ReviewListQuery::default())? {
        ids.insert(r.id.clone());
        if !keys.insert((r.office_code.clone(), r.number)) {
            return Err(refuse(&review_path(&r.id), "duplicate review key"));
        }
        validate_one_review(tx, &r)?;
    }
    for parent in tx.review_child_parent_ids()? {
        if !ids.contains(&parent) {
            return Err(refuse(&review_path(&parent), "orphaned review child"));
        }
    }
    Ok(())
}
