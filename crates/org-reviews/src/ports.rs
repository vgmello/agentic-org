//! Engine-independent read/write ports for universal reviews (`ENG-1`) and the consumer-owned
//! context port keeping `crate::reviews` decoupled from sibling domains (`Ce = 0`).

use org_proto::reviews::{ReviewStatus, ReviewTargetKind};
use org_store::{Engine, EngineError, Id};

use super::records::{
    parse_review_key, OfficeReviewCounterRecord, ReviewDocument, ReviewEventRecord,
    ReviewFindingRecord, ReviewFindingReplyRecord, ReviewRecord, ReviewReviewerRecord,
    ReviewRoundRecord,
};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ReviewListQuery {
    pub project_id: Option<Id>,
    pub task_id: Option<Id>,
    pub status: Option<ReviewStatus>,
    pub target_kind: Option<ReviewTargetKind>,
    pub reviewer_id: Option<Id>,
    pub created_by: Option<Id>,
}

pub trait ReviewsRead {
    fn review(&self, id: &Id) -> Result<Option<ReviewRecord>, EngineError>;
    fn review_by_key(
        &self,
        office_code: &str,
        number: u64,
    ) -> Result<Option<ReviewRecord>, EngineError>;
    fn list_reviews(&self, query: &ReviewListQuery) -> Result<Vec<ReviewRecord>, EngineError>;
    fn max_review_number(&self, office_code: &str) -> Result<u64, EngineError>;
    fn office_review_counter(
        &self,
        office_code: &str,
    ) -> Result<Option<OfficeReviewCounterRecord>, EngineError>;
    fn office_review_counters(&self) -> Result<Vec<OfficeReviewCounterRecord>, EngineError>;
    fn review_rounds(&self, review_id: &Id) -> Result<Vec<ReviewRoundRecord>, EngineError>;
    fn review_reviewers(&self, review_id: &Id) -> Result<Vec<ReviewReviewerRecord>, EngineError>;
    fn review_findings(&self, review_id: &Id) -> Result<Vec<ReviewFindingRecord>, EngineError>;
    fn review_finding_by_id(&self, id: &Id) -> Result<Option<ReviewFindingRecord>, EngineError>;
    fn review_finding_replies(
        &self,
        review_id: &Id,
    ) -> Result<Vec<ReviewFindingReplyRecord>, EngineError>;
    fn review_finding_reply_by_id(
        &self,
        id: &Id,
    ) -> Result<Option<ReviewFindingReplyRecord>, EngineError>;
    fn review_events(&self, review_id: &Id) -> Result<Vec<ReviewEventRecord>, EngineError>;
    fn review_child_parent_ids(&self) -> Result<Vec<Id>, EngineError>;

    fn resolve_review(&self, reference: &str) -> Result<Option<ReviewRecord>, EngineError> {
        let s = reference.trim();
        if s.is_empty() {
            return Ok(None);
        }
        if org_proto::is_uuid_v7(s) {
            if let Ok(id) = Id::parse(s) {
                if let Some(found) = self.review(&id)? {
                    return Ok(Some(found));
                }
            }
        }
        match parse_review_key(s) {
            Some((code, num)) => self.review_by_key(&code, num),
            None => Ok(None),
        }
    }
}

pub trait ReviewsWrite: ReviewsRead {
    fn put_review_document(&mut self, doc: &ReviewDocument) -> Result<(), EngineError>;
    fn put_review_round(&mut self, record: &ReviewRoundRecord) -> Result<(), EngineError>;
    fn put_review_event(&mut self, record: &ReviewEventRecord) -> Result<(), EngineError>;
    fn put_office_review_counter(
        &mut self,
        record: &OfficeReviewCounterRecord,
    ) -> Result<(), EngineError>;
    fn remove_office_review_counter(&mut self, office_code: &str) -> Result<(), EngineError>;
}

pub trait ReviewsEngine: Engine<Tx: ReviewsWrite, Snapshot: ReviewsRead> {}
impl<E> ReviewsEngine for E where E: Engine<Tx: ReviewsWrite, Snapshot: ReviewsRead> {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewActorInfo {
    pub id: Id,
    pub is_person: bool,
    pub active: bool,
    pub home_team_id: Id,
    pub is_builtin: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewProjectInfo {
    pub id: Id,
    pub owner_team_id: Id,
    pub contributing_team_ids: Vec<Id>,
    pub archived: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewTaskInfo {
    pub id: Id,
    pub project_id: Option<Id>,
}

/// Consumer-owned read port for sibling-domain facts, implemented at `store::open`.
pub trait ReviewContextRead {
    fn organization_id(&self) -> Result<Option<Id>, EngineError>;
    fn active_office_code(&self, office_id: &Id) -> Result<Option<String>, EngineError>;
    fn employee_info(&self, id: &Id) -> Result<Option<ReviewActorInfo>, EngineError>;
    fn all_employees(&self) -> Result<Vec<ReviewActorInfo>, EngineError>;
    fn team_exists_active(&self, team_id: &Id) -> Result<bool, EngineError>;
    fn team_lead(&self, team_id: &Id) -> Result<Option<Id>, EngineError>;
    fn team_chain(&self, team_id: &Id) -> Result<Option<Vec<Id>>, EngineError>;
    fn resolve_project_info(
        &self,
        reference: &str,
    ) -> Result<Option<ReviewProjectInfo>, EngineError>;
    fn project_info(&self, id: &Id) -> Result<Option<ReviewProjectInfo>, EngineError>;
    fn resolve_task_info(&self, reference: &str) -> Result<Option<ReviewTaskInfo>, EngineError>;
    fn repository_exists(&self, repo_id: &Id) -> Result<bool, EngineError>;
}
