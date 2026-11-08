//! A small world to run calls in: an organization (owner, a person, agents in each state, two
//! projects and an archived one), an in-memory store and a clock.

use std::cell::Cell;
use std::sync::Arc;

use org_proto::org::AgentStatus;
use org_proto::work::*;
use org_proto::{ApiError, ErrorCode};

use crate::testkit::{MemOrg, MemStore, TestClock};
use crate::{ulid_from_parts, Caller, StoredItem, TxnOutput, WorkCall, WorkService};

/// 2026-10-27T…: a fixed start.
pub const T0: u64 = 1_793_000_000_000;
pub const HOUR: u64 = 3_600_000;
pub const MINUTE: u64 = 60_000;
pub const DAY: u64 = 24 * HOUR;

pub const OWNER: &str = "owner1";
pub const PROJECT: &str = "p-chk";
pub const OTHER_PROJECT: &str = "p-pay";
pub const ARCHIVED: &str = "p-old";
pub const TEAM: &str = "team-a";

pub fn app() -> Caller {
    Caller::app(OWNER)
}

pub fn agent() -> Caller {
    Caller::agent("ada")
}

pub fn org() -> MemOrg {
    let mut org = MemOrg::new(OWNER)
        .with_person("pat")
        .with_agent("ada", AgentStatus::Active)
        .with_agent("riley", AgentStatus::Active)
        .with_agent("pau", AgentStatus::Paused)
        .with_agent("off", AgentStatus::Offboarded)
        .with_project(PROJECT, "chk", &[TEAM])
        .with_project(OTHER_PROJECT, "pay", &[])
        .with_project(ARCHIVED, "old", &[]);
    org.set_archived(ARCHIVED, true);
    org
}

pub struct World {
    pub store: Arc<MemStore>,
    pub clock: Arc<TestClock>,
    pub svc: WorkService<Arc<MemStore>>,
    seq: Cell<u128>,
}

impl World {
    pub fn new() -> Self {
        let store = Arc::new(MemStore::new(org()));
        let clock = TestClock::at(T0);
        let svc = WorkService::new(Arc::clone(&store), clock.clone(), "hq");
        Self {
            store,
            clock,
            svc,
            seq: Cell::new(0),
        }
    }

    pub fn now(&self) -> u64 {
        use crate::Clock;
        self.clock.now_ms()
    }

    /// A fresh ULID dated now.
    pub fn id(&self) -> String {
        self.seq.set(self.seq.get() + 1);
        ulid_from_parts(self.now(), self.seq.get())
    }

    pub fn call(&self, caller: &Caller, call: WorkCall) -> TxnOutput {
        self.svc
            .call(caller.clone(), call)
            .expect("no storage error")
    }

    pub fn ok(&self, caller: &Caller, call: WorkCall) -> WorkAccepted {
        match self.call(caller, call) {
            TxnOutput::Committed { accepted, .. } => accepted,
            other => panic!("expected a commit, got {other:?}"),
        }
    }

    pub fn err(&self, caller: &Caller, call: WorkCall) -> ApiError {
        match self.call(caller, call) {
            TxnOutput::Refused(e) => e,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    pub fn code(&self, caller: &Caller, call: WorkCall) -> ErrorCode {
        self.err(caller, call).code
    }

    pub fn item(&self, id: &str) -> StoredItem {
        self.store.snapshot().items[id].clone()
    }

    pub fn version(&self, id: &str) -> u64 {
        self.item(id).version
    }

    pub fn events(&self, id: &str) -> Vec<WorkEvent> {
        self.store
            .snapshot()
            .events
            .get(id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn last_event(&self, id: &str) -> WorkEvent {
        self.events(id).pop().expect("an event")
    }

    // ---- params ------------------------------------------------------------------------

    pub fn create_params(&self, project: &str) -> TaskCreateParams {
        TaskCreateParams {
            change_id: self.id(),
            item_id: self.id(),
            project_id: project.into(),
            title: "Coupon edge cases".into(),
            description: None,
            definition_of_done: None,
            assignee_id: None,
            team_id: None,
            priority: None,
            feature_id: None,
            fixes: vec![],
            checklist: vec![],
        }
    }

    /// Creates a task in `PROJECT` as the app; returns its id.
    pub fn task(&self) -> String {
        self.task_with(|_| {})
    }

    pub fn task_with(&self, f: impl FnOnce(&mut TaskCreateParams)) -> String {
        let mut p = self.create_params(PROJECT);
        f(&mut p);
        let id = p.item_id.clone();
        self.ok(&app(), WorkCall::TaskCreate(p));
        id
    }

    /// A task with one step (`required`), returning (task id, step id).
    pub fn task_with_step(&self, required: bool) -> (String, String) {
        let step = self.id();
        let s = step.clone();
        let task = self.task_with(|p| {
            p.checklist = vec![NewStep {
                step_id: s,
                text: "Write the tests".into(),
                required,
            }]
        });
        (task, step)
    }

    pub fn status(&self, id: &str, status: ItemStatus) -> TaskSetStatusParams {
        TaskSetStatusParams {
            change_id: self.id(),
            item_id: id.into(),
            expected_version: self.version(id),
            status,
            blocked_reason: (status == ItemStatus::Blocked).then(|| "waiting on CI".into()),
            resolution: None,
            anyway_reason: None,
            shortcut: None,
        }
    }

    /// Moves a task to `status` as the app (with a reason if blocked).
    pub fn set(&self, id: &str, status: ItemStatus) -> WorkAccepted {
        let p = self.status(id, status);
        self.ok(&app(), WorkCall::TaskSetStatus(p))
    }

    pub fn update(&self, id: &str) -> TaskUpdateParams {
        TaskUpdateParams {
            change_id: self.id(),
            item_id: id.into(),
            expected_version: self.version(id),
            title: None,
            description: None,
            clear_description: false,
            definition_of_done: None,
            clear_definition_of_done: false,
            priority: None,
            team_id: None,
            clear_team: false,
            feature_id: None,
            clear_feature: false,
        }
    }

    pub fn comment(&self, id: &str, body: &str) -> WorkCommentParams {
        WorkCommentParams {
            change_id: self.id(),
            item_id: id.into(),
            comment_id: self.id(),
            body: body.into(),
            comment_kind: None,
            asks: None,
            answers: None,
            watch: false,
        }
    }

    pub fn check(&self, step: &str, done: bool) -> WorkCall {
        WorkCall::ChecklistCheck(ChecklistCheckParams {
            change_id: self.id(),
            step_id: step.into(),
            done,
        })
    }

    pub fn make_task(&self, step: &str) -> (String, WorkCall) {
        let task_id = self.id();
        let call = WorkCall::ChecklistMakeTask(ChecklistMakeTaskParams {
            change_id: self.id(),
            step_id: step.into(),
            task_id: task_id.clone(),
            link_id: self.id(),
            title: None,
        });
        (task_id, call)
    }

    pub fn step(&self, item: &str, step: &str) -> crate::StoredStep {
        self.item(item)
            .checklist
            .into_iter()
            .find(|s| s.id == step)
            .expect("the step")
    }

    pub fn delete(&self, id: &str) -> WorkCall {
        WorkCall::Delete(WorkDeleteParams {
            change_id: self.id(),
            item_id: id.into(),
            expected_version: self.version(id),
        })
    }
}
