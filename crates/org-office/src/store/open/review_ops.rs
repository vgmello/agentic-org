//! Engine-independent `ReadOp` and `StoreOp` wrappers for universal reviews (`TASK-92.1`).

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
    decide_close, decide_finding, decide_request, decide_resubmit, decide_route, decide_verdict,
    decode_refusal, inspect_review, list_events, list_reviews, ReviewInspectSections,
};
use crate::reviews::ports::{
    ReviewActorInfo, ReviewContextRead, ReviewProjectInfo, ReviewTaskInfo, ReviewsRead,
};
use crate::reviews::records::{ResolvedCodePin, ReviewEventRecord};
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
        archived: p.archived,
    }
}

impl<S: TasksRead + ?Sized> ReviewContextRead for S {
    fn organization_id(&self) -> Result<Option<Id>, EngineError> {
        Ok(self.organization()?.map(|o| o.id))
    }
    fn active_office_code(&self, id: &Id) -> Result<Option<String>, EngineError> {
        Ok(self.office_code(id)?.map(|c| c.code))
    }
    fn employee_info(&self, id: &Id) -> Result<Option<ReviewActorInfo>, EngineError> {
        Ok(self.employee(id)?.map(to_actor))
    }
    fn all_employees(&self) -> Result<Vec<ReviewActorInfo>, EngineError> {
        Ok(self.employees()?.into_iter().map(to_actor).collect())
    }
    fn team_exists_active(&self, id: &Id) -> Result<bool, EngineError> {
        Ok(self
            .team(id)?
            .is_some_and(|t| t.status == TeamStatus::Active))
    }
    fn team_chain(&self, id: &Id) -> Result<Option<Vec<Id>>, EngineError> {
        self.team_chain(id)
    }
    fn resolve_project_info(&self, r: &str) -> Result<Option<ReviewProjectInfo>, EngineError> {
        Ok(self.resolve_project(r)?.map(to_project))
    }
    fn project_info(&self, id: &Id) -> Result<Option<ReviewProjectInfo>, EngineError> {
        Ok(self.project(id)?.map(to_project))
    }
    fn resolve_task_info(&self, r: &str) -> Result<Option<ReviewTaskInfo>, EngineError> {
        Ok(self.resolve_task(r)?.map(|t| ReviewTaskInfo {
            id: t.id,
            project_id: t.project_id,
        }))
    }
    fn repository_exists(&self, id: &Id) -> Result<bool, EngineError> {
        Ok(self.repositories()?.iter().any(|r| r.id == *id))
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
        ReviewListRead(c: String, p: ReviewListParams) -> ReviewListResult => |s| list_reviews(s, &c, &p);
        ReviewInspectRead(c: String, p: ReviewInspectParams, sec: ReviewInspectSections) -> ReviewInspectResult => |s| inspect_review(s, &c, &p, sec);
        ReviewEventsRead(c: String, r: String) -> Vec<ReviewEventRecord> => |s| list_events(s, &c, &r);
    }
    writes {
        ReviewRequestOp(oid: Id, c: String, p: ReviewRequestParams, pin: Option<ResolvedCodePin>) -> ReviewRoundResult, "review.request", p.command_id, &c, (&p, &pin) => |u| decide_request(u.view(), &oid, u.now().0, &c, &p, pin.as_ref());
        ReviewRouteOp(c: String, p: ReviewRouteParams) -> ReviewRouteResult, "review.route", p.command_id, &c, &p => |u| decide_route(u.view(), u.now().0, &c, &p);
        ReviewFindingOpCmd(c: String, p: ReviewFindingParams) -> ReviewFindingResult, "review.finding", p.command_id, &c, &p => |u| decide_finding(u.view(), u.now().0, &c, &p);
        ReviewVerdictOp(c: String, p: ReviewVerdictParams, rev: Option<u64>) -> ReviewVerdictResult, "review.verdict", p.command_id, &c, (&p, rev) => |u| decide_verdict(u.view(), u.now().0, &c, &p, rev);
        ReviewResubmitOp(c: String, p: ReviewResubmitParams, pin: Option<ResolvedCodePin>) -> ReviewRoundResult, "review.resubmit", p.command_id, &c, (&p, &pin) => |u| decide_resubmit(u.view(), u.now().0, &c, &p, pin.as_ref());
        ReviewCloseOp(c: String, p: ReviewCloseParams) -> ReviewMutationResult, "review.close", p.command_id, &c, &p => |u| decide_close(u.view(), u.now().0, &c, &p);
    }
}
