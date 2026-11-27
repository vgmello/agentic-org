//! Engine-independent `ReadOp` and `StoreOp` wrappers for universal reviews (`TASK-92.1`).

use org_proto::attachments::AttachmentTargetKind;
use org_proto::reviews::{
    ReviewCloseParams, ReviewFindingParams, ReviewFindingResult, ReviewInspectParams,
    ReviewInspectResult, ReviewListParams, ReviewListResult, ReviewMutationResult,
    ReviewRequestParams, ReviewResubmitParams, ReviewRoundResult, ReviewRouteParams,
    ReviewRouteResult, ReviewVerdictParams, ReviewVerdictResult,
};
use org_proto::{ApiError, ErrorCode};
use org_store::{
    Actor, Change, EngineError, Id, Operation, Outcome, Refusal, Sha256, Store, StoreError,
    UnitOfWork,
};
use serde::de::DeserializeOwned;

use crate::organization::records::{Employee, EmployeeKind, EmployeeStatus, TeamStatus};
use crate::projects::records::Project;
use crate::reviews::features::{
    decide_close, decide_finding, decide_request, decide_resubmit, decide_route,
    decide_verdict_with_session, decode_refusal, inspect_review, list_events, list_reviews,
    ReviewInspectSections,
};
use crate::reviews::ports::{
    ReviewActorInfo, ReviewAttachmentInfo, ReviewContextRead, ReviewListQuery, ReviewProjectInfo,
    ReviewTaskInfo, ReviewsRead,
};
use crate::reviews::records::{
    OfficeReviewCounterRecord, ResolvedCodePin, ReviewEventRecord, ReviewFindingRecord,
    ReviewFindingReplyRecord, ReviewRecord, ReviewReviewerRecord, ReviewRoundRecord,
};
use crate::tasks::ports::TasksRead;

use super::{OfficeEngine, ReadOp, StoreOp};

type Answer<T> = Result<Result<T, ApiError>, StoreError>;

fn to_actor(e: Employee) -> ReviewActorInfo {
    ReviewActorInfo {
        id: e.id,
        is_person: e.kind == EmployeeKind::Person,
        active: e.status == EmployeeStatus::Active,
        home_team_id: e.home_team_id,
        is_builtin: e.builtin_key.is_some(),
    }
}

fn to_project(p: Project) -> ReviewProjectInfo {
    ReviewProjectInfo {
        id: p.id,
        owner_team_id: p.owner_team_id,
        contributing_team_ids: p.contributing_team_ids,
        repository_ids: p
            .repositories
            .into_iter()
            .map(|r| r.repository_id)
            .collect(),
        archived: p.archived,
    }
}

struct ReviewView<'a, S: ?Sized>(&'a S);

macro_rules! fwd_reviews_read {
    ($($name:ident($($arg:ident: $ty:ty),*) -> $ret:ty;)+) => {
        impl<S: ReviewsRead + ?Sized> ReviewsRead for ReviewView<'_, S> {
            $(fn $name(&self, $($arg: $ty),*) -> Result<$ret, EngineError> { self.0.$name($($arg),*) })+
        }
    };
}

fwd_reviews_read! {
    review(id: &Id) -> Option<ReviewRecord>;
    review_by_key(c: &str, n: u64) -> Option<ReviewRecord>;
    list_reviews(q: &ReviewListQuery) -> Vec<ReviewRecord>;
    max_review_number(c: &str) -> u64;
    office_review_counter(c: &str) -> Option<OfficeReviewCounterRecord>;
    office_review_counters() -> Vec<OfficeReviewCounterRecord>;
    review_rounds(id: &Id) -> Vec<ReviewRoundRecord>;
    review_reviewers(id: &Id) -> Vec<ReviewReviewerRecord>;
    review_findings(id: &Id) -> Vec<ReviewFindingRecord>;
    review_finding_by_id(id: &Id) -> Option<ReviewFindingRecord>;
    review_finding_replies(id: &Id) -> Vec<ReviewFindingReplyRecord>;
    review_finding_reply_by_id(id: &Id) -> Option<ReviewFindingReplyRecord>;
    review_events(id: &Id) -> Vec<ReviewEventRecord>;
    review_child_parent_ids() -> Vec<Id>;
}

impl<S: TasksRead + ?Sized> ReviewContextRead for ReviewView<'_, S> {
    fn organization_id(&self) -> Result<Option<Id>, EngineError> {
        Ok(self.0.organization()?.map(|o| o.id))
    }
    fn active_office_code(&self, id: &Id) -> Result<Option<String>, EngineError> {
        Ok(self.0.office_code(id)?.map(|c| c.code))
    }
    fn employee_info(&self, id: &Id) -> Result<Option<ReviewActorInfo>, EngineError> {
        Ok(self.0.employee(id)?.map(to_actor))
    }
    fn all_employees(&self) -> Result<Vec<ReviewActorInfo>, EngineError> {
        Ok(self.0.employees()?.into_iter().map(to_actor).collect())
    }
    fn team_exists_active(&self, id: &Id) -> Result<bool, EngineError> {
        Ok(self
            .0
            .team(id)?
            .is_some_and(|t| t.status == TeamStatus::Active))
    }
    fn team_lead(&self, id: &Id) -> Result<Option<Id>, EngineError> {
        Ok(self.0.team(id)?.and_then(|t| t.lead_employee_id))
    }
    fn team_chain(&self, id: &Id) -> Result<Option<Vec<Id>>, EngineError> {
        self.0.team_chain(id)
    }
    fn resolve_project_info(&self, r: &str) -> Result<Option<ReviewProjectInfo>, EngineError> {
        Ok(self.0.resolve_project(r)?.map(to_project))
    }
    fn project_info(&self, id: &Id) -> Result<Option<ReviewProjectInfo>, EngineError> {
        Ok(self.0.project(id)?.map(to_project))
    }
    fn resolve_task_info(&self, r: &str) -> Result<Option<ReviewTaskInfo>, EngineError> {
        Ok(self.0.resolve_task(r)?.map(|t| ReviewTaskInfo {
            id: t.id,
            project_id: t.project_id,
        }))
    }
    fn repository_exists(&self, id: &Id) -> Result<bool, EngineError> {
        Ok(self.0.repositories()?.iter().any(|r| r.id == *id))
    }
    fn repository_project_ids(&self, id: &Id) -> Result<Vec<Id>, EngineError> {
        let has = |p: &Project| p.repositories.iter().any(|r| r.repository_id == *id);
        Ok(self
            .0
            .projects()?
            .into_iter()
            .filter(has)
            .map(|p| p.id)
            .collect())
    }
    fn attachment_info(&self, id: &Id) -> Result<Option<ReviewAttachmentInfo>, EngineError> {
        let Some(a) = self.0.attachment(id)? else {
            return Ok(None);
        };
        let target_id = match (a.target_kind, a.target_id) {
            (AttachmentTargetKind::TaskComment, Some(c)) => {
                self.0.task_comment_by_id(&c)?.map(|x| x.task_id)
            }
            (_, tid) => tid,
        };
        Ok(Some(ReviewAttachmentInfo {
            target_kind: a.target_kind,
            target_id,
        }))
    }
}

fn run_write<E: OfficeEngine, T: DeserializeOwned>(
    store: &Store<E>,
    kind: &'static str,
    cmd: &str,
    caller: &str,
    fp: &[u8],
    decide: impl FnMut(&mut UnitOfWork<'_, E>) -> Result<Change, Refusal> + Send + 'static,
) -> Answer<T> {
    let Some(id) = org_proto::is_uuid_v7(cmd)
        .then(|| Id::parse(cmd).ok())
        .flatten()
    else {
        return Ok(Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "command_id: a lowercase UUIDv7",
        )));
    };
    let op = Operation {
        id,
        kind,
        actor: Id::parse(caller).map_or(Actor::System, Actor::Employee),
        fingerprint: Sha256::of_parts(&[kind.as_bytes(), b"\0", fp]),
    };
    let reply = match store.submit(Some(op), false, decide) {
        Ok(r) => r,
        Err(StoreError::Conflict) => {
            return Ok(Err(ApiError::new(
                ErrorCode::OperationReused,
                "command_id reused with a different request",
            )));
        }
        Err(e) => return Err(e),
    };
    if reply.outcome == Outcome::Rejected {
        return Ok(Err(decode_refusal(&Refusal::Rejected(reply.bytes))));
    }
    serde_json::from_slice(&reply.bytes)
        .map(Ok)
        .map_err(|e| StoreError::Invalid(format!("corrupt review reply: {e}")))
}

macro_rules! review_ops {
    (
        reads { $($rname:ident($($rf:ident: $rty:ty),+) -> $rout:ty => |$snap:ident| $rbody:expr;)+ }
        writes { $($wname:ident($($wf:ident: $wty:ty),+) -> $wout:ty, $kind:literal, $cmd:expr, $actor:expr, $fp:expr => |$uow:ident| $wbody:expr;)+ }
    ) => {
        $(
            pub struct $rname($(pub $rty),+);
            impl ReadOp for $rname {
                type Out = Result<$rout, ApiError>;
                fn read<S: TasksRead + ReviewsRead>(self, $snap: &S) -> Answer<$rout> {
                    let Self($($rf),+) = self;
                    Ok($rbody.map_err(|r| decode_refusal(&r)))
                }
            }
        )+
        $(
            pub struct $wname($(pub $wty),+);
            impl StoreOp for $wname {
                type Out = Answer<$wout>;
                fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
                    let Self($($wf),+) = self;
                    let (cmd, actor, fp) = (($cmd).to_owned(), ($actor).to_owned(), serde_json::to_vec(&($fp)).expect("review params encode"));
                    run_write(store, $kind, &cmd, &actor, &fp, move |$uow| $wbody)
                }
            }
        )+
    };
}

review_ops! {
    reads {
        ReviewListRead(c: String, p: ReviewListParams) -> ReviewListResult => |s| list_reviews(&ReviewView(s), &c, &p);
        ReviewInspectRead(c: String, p: ReviewInspectParams, sec: ReviewInspectSections) -> ReviewInspectResult => |s| inspect_review(&ReviewView(s), &c, &p, sec);
        ReviewEventsRead(c: String, r: String) -> Vec<ReviewEventRecord> => |s| list_events(&ReviewView(s), &c, &r);
    }
    writes {
        ReviewRequestOp(oid: Id, c: String, p: ReviewRequestParams, pin: Option<ResolvedCodePin>) -> ReviewRoundResult, "review.request", p.command_id, &c, (&p, &pin) => |u| decide_request(&ReviewView(u.view()), &oid, u.now().0, &c, &p, pin.as_ref());
        ReviewRouteOp(c: String, p: ReviewRouteParams) -> ReviewRouteResult, "review.route", p.command_id, &c, &p => |u| decide_route(&ReviewView(u.view()), u.now().0, &c, &p);
        ReviewFindingOpCmd(c: String, p: ReviewFindingParams) -> ReviewFindingResult, "review.finding", p.command_id, &c, &p => |u| decide_finding(&ReviewView(u.view()), u.now().0, &c, &p);
        ReviewVerdictOp(c: String, p: ReviewVerdictParams, rev: Option<u64>, sid: Option<String>) -> ReviewVerdictResult, "review.verdict", p.command_id, &c, (&p, rev) => |u| decide_verdict_with_session(&ReviewView(u.view()), u.now().0, &c, &p, rev, sid.as_deref());
        ReviewResubmitOp(c: String, p: ReviewResubmitParams, pin: Option<ResolvedCodePin>) -> ReviewRoundResult, "review.resubmit", p.command_id, &c, (&p, &pin) => |u| decide_resubmit(&ReviewView(u.view()), u.now().0, &c, &p, pin.as_ref());
        ReviewCloseOp(c: String, p: ReviewCloseParams) -> ReviewMutationResult, "review.close", p.command_id, &c, &p => |u| decide_close(&ReviewView(u.view()), u.now().0, &c, &p);
    }
}

pub struct ReviewReviewerRecordsRead(pub Id);

impl ReadOp for ReviewReviewerRecordsRead {
    type Out = Result<Vec<ReviewReviewerRecord>, ApiError>;

    fn read<S: TasksRead + ReviewsRead>(self, snap: &S) -> Answer<Vec<ReviewReviewerRecord>> {
        Ok(Ok(snap.review_reviewers(&self.0)?))
    }
}
