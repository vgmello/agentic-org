//! `MemoryEngine` `FamilyTable` implementation for [`ReviewsRead`] and [`ReviewsWrite`] (`ENG-1`).

use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet},
};

use org_store::{
    testkit::{FamilyTable, MemorySnapshot, MemoryTx},
    EngineError, Id,
};

use super::ports::{ReviewListQuery, ReviewsRead, ReviewsWrite};
use super::records::{
    OfficeReviewCounterRecord, ReviewDocument, ReviewEventRecord, ReviewFindingRecord,
    ReviewFindingReplyRecord, ReviewRecord, ReviewReviewerRecord, ReviewRoundRecord,
};

#[derive(Clone, Default, Debug)]
pub struct ReviewsTables {
    counters: BTreeMap<String, OfficeReviewCounterRecord>,
    reviews: BTreeMap<Id, ReviewRecord>,
    rounds: BTreeMap<(Id, u32), ReviewRoundRecord>,
    reviewers: BTreeMap<(Id, u32, Id), ReviewReviewerRecord>,
    findings: BTreeMap<Id, ReviewFindingRecord>,
    replies: BTreeMap<Id, ReviewFindingReplyRecord>,
    events: BTreeMap<(Id, u64), ReviewEventRecord>,
}

impl FamilyTable for ReviewsTables {
    fn name(&self) -> &'static str {
        "reviews"
    }

    fn clone_box(&self) -> Box<dyn FamilyTable> {
        Box::new(self.clone())
    }

    fn dump(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for (code, c) in &self.counters {
            lines.push(format!(
                "office_review_counters {code} office={} next={}",
                c.office_id, c.next_number
            ));
        }
        for (id, r) in &self.reviews {
            lines.push(format!(
                "reviews {id} key={} rev={} round={} status={} closed={:?}",
                r.key(),
                r.revision,
                r.current_round,
                r.status.as_str(),
                r.closed_reason
            ));
        }
        for ((rid, rn), rd) in &self.rounds {
            lines.push(format!(
                "review_rounds {rid} r={rn} mode={} head={:?}",
                rd.session_mode.as_str(),
                rd.head_oid
            ));
        }
        for ((rid, rn, uid), rr) in &self.reviewers {
            lines.push(format!(
                "review_reviewers {rid} r={rn} u={uid} removed={} verdict={:?}",
                rr.removed, rr.verdict
            ));
        }
        for (fid, f) in &self.findings {
            lines.push(format!(
                "review_findings {fid} review={} num={} status={} rev={}",
                f.review_id,
                f.number,
                f.status.as_str(),
                f.revision
            ));
        }
        for (repid, rep) in &self.replies {
            lines.push(format!(
                "review_finding_replies {repid} finding={} status={:?}",
                rep.finding_id, rep.set_status
            ));
        }
        for ((rid, rev), ev) in &self.events {
            lines.push(format!(
                "review_events {rid} rev={rev} type={} sha={}",
                ev.event_type, ev.payload_sha256
            ));
        }
        lines.sort();
        lines
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

fn read<R: Default>(
    tables: Option<&ReviewsTables>,
    query: impl FnOnce(&ReviewsTables) -> R,
) -> Result<R, EngineError> {
    Ok(tables.map(query).unwrap_or_default())
}

fn matches_query(tables: &ReviewsTables, r: &ReviewRecord, q: &ReviewListQuery) -> bool {
    if q.project_id
        .as_ref()
        .is_some_and(|p| r.project_id.as_ref() != Some(p))
    {
        return false;
    }
    if q.task_id
        .as_ref()
        .is_some_and(|t| r.task_id.as_ref() != Some(t))
    {
        return false;
    }
    if q.status.is_some_and(|s| r.status != s) {
        return false;
    }
    if q.target_kind.is_some_and(|k| r.target_kind != k) {
        return false;
    }
    if q.created_by.as_ref().is_some_and(|c| r.created_by != *c) {
        return false;
    }
    if let Some(reviewer_id) = q.reviewer_id.as_ref() {
        let assigned = tables
            .reviewers
            .get(&(r.id.clone(), r.current_round, reviewer_id.clone()))
            .is_some_and(|rr| !rr.removed);
        if !assigned {
            return false;
        }
    }
    true
}

macro_rules! memory_reviews_read {
    ($($view:ty),+) => {$(
        impl ReviewsRead for $view {
            fn review(&self, id: &Id) -> Result<Option<ReviewRecord>, EngineError> {
                read(self.table(), |t| t.reviews.get(id).cloned())
            }

            fn review_by_key(
                &self,
                office_code: &str,
                number: u64,
            ) -> Result<Option<ReviewRecord>, EngineError> {
                read(self.table(), |t| {
                    t.reviews
                        .values()
                        .find(|r| r.office_code == office_code && r.number == number)
                        .cloned()
                })
            }

            fn list_reviews(&self, query: &ReviewListQuery) -> Result<Vec<ReviewRecord>, EngineError> {
                read(self.table(), |t| {
                    let mut out: Vec<ReviewRecord> = t
                        .reviews
                        .values()
                        .filter(|r| matches_query(t, r, query))
                        .cloned()
                        .collect();
                    out.sort_by(|a, b| {
                        b.updated_at
                            .cmp(&a.updated_at)
                            .then_with(|| b.id.cmp(&a.id))
                    });
                    out
                })
            }

            fn max_review_number(&self, office_code: &str) -> Result<u64, EngineError> {
                read(self.table(), |t| {
                    t.reviews
                        .values()
                        .filter(|r| r.office_code == office_code)
                        .map(|r| r.number)
                        .max()
                        .unwrap_or(0)
                })
            }

            fn office_review_counter(
                &self,
                office_code: &str,
            ) -> Result<Option<OfficeReviewCounterRecord>, EngineError> {
                read(self.table(), |t| t.counters.get(office_code).cloned())
            }

            fn office_review_counters(&self) -> Result<Vec<OfficeReviewCounterRecord>, EngineError> {
                read(self.table(), |t| t.counters.values().cloned().collect())
            }

            fn review_rounds(&self, review_id: &Id) -> Result<Vec<ReviewRoundRecord>, EngineError> {
                read(self.table(), |t| {
                    t.rounds
                        .iter()
                        .filter(|((rid, _), _)| rid == review_id)
                        .map(|(_, rd)| rd.clone())
                        .collect()
                })
            }

            fn review_reviewers(&self, review_id: &Id) -> Result<Vec<ReviewReviewerRecord>, EngineError> {
                read(self.table(), |t| {
                    t.reviewers
                        .iter()
                        .filter(|((rid, _, _), _)| rid == review_id)
                        .map(|(_, rr)| rr.clone())
                        .collect()
                })
            }

            fn review_findings(&self, review_id: &Id) -> Result<Vec<ReviewFindingRecord>, EngineError> {
                read(self.table(), |t| {
                    let mut out: Vec<ReviewFindingRecord> = t
                        .findings
                        .values()
                        .filter(|f| f.review_id == *review_id)
                        .cloned()
                        .collect();
                    out.sort_by_key(|f| f.number);
                    out
                })
            }

            fn review_finding_by_id(&self, id: &Id) -> Result<Option<ReviewFindingRecord>, EngineError> {
                read(self.table(), |t| t.findings.get(id).cloned())
            }

            fn review_finding_replies(
                &self,
                review_id: &Id,
            ) -> Result<Vec<ReviewFindingReplyRecord>, EngineError> {
                read(self.table(), |t| {
                    let mut out: Vec<ReviewFindingReplyRecord> = t
                        .replies
                        .values()
                        .filter(|r| r.review_id == *review_id)
                        .cloned()
                        .collect();
                    out.sort_by(|a, b| {
                        a.created_at
                            .cmp(&b.created_at)
                            .then_with(|| a.id.cmp(&b.id))
                    });
                    out
                })
            }

            fn review_finding_reply_by_id(
                &self,
                id: &Id,
            ) -> Result<Option<ReviewFindingReplyRecord>, EngineError> {
                read(self.table(), |t| t.replies.get(id).cloned())
            }

            fn review_events(&self, review_id: &Id) -> Result<Vec<ReviewEventRecord>, EngineError> {
                read(self.table(), |t| {
                    t.events
                        .iter()
                        .filter(|((rid, _), _)| rid == review_id)
                        .map(|(_, ev)| ev.clone())
                        .collect()
                })
            }

            fn review_child_parent_ids(&self) -> Result<Vec<Id>, EngineError> {
                read(self.table(), |t| {
                    let mut ids = BTreeSet::new();
                    for (rid, _) in t.rounds.keys() {
                        ids.insert(rid.clone());
                    }
                    for (rid, _, _) in t.reviewers.keys() {
                        ids.insert(rid.clone());
                    }
                    for f in t.findings.values() {
                        ids.insert(f.review_id.clone());
                    }
                    for rep in t.replies.values() {
                        ids.insert(rep.review_id.clone());
                    }
                    for (rid, _) in t.events.keys() {
                        ids.insert(rid.clone());
                    }
                    ids.into_iter().collect()
                })
            }
        }
    )+};
}

memory_reviews_read!(MemoryTx, MemorySnapshot);

impl ReviewsWrite for MemoryTx {
    fn put_review_document(&mut self, doc: &ReviewDocument) -> Result<(), EngineError> {
        let tables: &mut ReviewsTables = self.table_mut()?;
        let rid = doc.review.id.clone();
        tables.reviews.insert(rid.clone(), doc.review.clone());
        let next_reviewer_keys: BTreeSet<(Id, u32, Id)> = doc
            .reviewers
            .iter()
            .map(|r| (r.review_id.clone(), r.round_number, r.reviewer_id.clone()))
            .collect();
        tables
            .reviewers
            .retain(|k, _| k.0 != rid || next_reviewer_keys.contains(k));
        for rr in &doc.reviewers {
            tables.reviewers.insert(
                (
                    rr.review_id.clone(),
                    rr.round_number,
                    rr.reviewer_id.clone(),
                ),
                rr.clone(),
            );
        }
        for f in &doc.findings {
            tables.findings.insert(f.id.clone(), f.clone());
        }
        for rep in &doc.replies {
            if let Some(existing) = tables.replies.get(&rep.id) {
                if existing != rep {
                    return Err(EngineError::new(format!(
                        "immutable review_finding_reply conflict for reply {}",
                        rep.id
                    )));
                }
                continue;
            }
            tables.replies.insert(rep.id.clone(), rep.clone());
        }
        Ok(())
    }

    fn put_review_round(&mut self, record: &ReviewRoundRecord) -> Result<(), EngineError> {
        let tables: &mut ReviewsTables = self.table_mut()?;
        let key = (record.review_id.clone(), record.round_number);
        if let Some(existing) = tables.rounds.get(&key) {
            if existing == record {
                return Ok(());
            }
            return Err(EngineError::new(format!(
                "immutable review_round conflict for review {} round {}",
                record.review_id, record.round_number
            )));
        }
        tables.rounds.insert(key, record.clone());
        Ok(())
    }

    fn put_review_event(&mut self, record: &ReviewEventRecord) -> Result<(), EngineError> {
        let tables: &mut ReviewsTables = self.table_mut()?;
        let key = (record.review_id.clone(), record.revision);
        if let Some(existing) = tables.events.get(&key) {
            if existing.payload_sha256 == record.payload_sha256 {
                return Ok(());
            }
            return Err(EngineError::new(format!(
                "immutable review_event conflict for review {} revision {}",
                record.review_id, record.revision
            )));
        }
        tables.events.insert(key, record.clone());
        Ok(())
    }

    fn put_office_review_counter(
        &mut self,
        record: &OfficeReviewCounterRecord,
    ) -> Result<(), EngineError> {
        let tables: &mut ReviewsTables = self.table_mut()?;
        if let Some(existing) = tables.counters.get(&record.office_code) {
            if record.next_number < existing.next_number {
                return Err(EngineError::new("office review counter cannot decrease"));
            }
        }
        tables
            .counters
            .insert(record.office_code.clone(), record.clone());
        Ok(())
    }

    fn remove_office_review_counter(&mut self, office_code: &str) -> Result<(), EngineError> {
        let tables: &mut ReviewsTables = self.table_mut()?;
        tables.counters.remove(office_code);
        Ok(())
    }
}
