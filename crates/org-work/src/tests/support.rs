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
/// An area of `PROJECT` led by riley: bugs routed there don't wait on the owner.
pub const AREA_PAY: &str = "ar-pay";
/// An area of `PROJECT` with a team and no lead (and, at first, no team lead).
pub const AREA_UI: &str = "ar-ui";
/// `PROJECT`'s General area: its team has no lead, so the owner triages its bugs.
pub const GENERAL: &str = "p-chk-general";

pub fn app() -> Caller {
    Caller::app(OWNER)
}

pub fn agent() -> Caller {
    Caller::agent("ada")
}

/// A person who reports bugs through the app.
pub fn pat() -> Caller {
    Caller::app("pat")
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
        .with_project(ARCHIVED, "old", &[])
        .with_area(AREA_PAY, PROJECT, Some(TEAM), Some("riley"))
        .with_area(AREA_UI, PROJECT, Some(TEAM), None);
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

// ---- bugs ----------------------------------------------------------------------------------

impl World {
    pub fn report_params(&self, project: &str) -> BugReportParams {
        BugReportParams {
            change_id: self.id(),
            item_id: self.id(),
            project_id: project.into(),
            title: "Coupon total is off".into(),
            what_happened: "The total ignores the coupon.".into(),
            severity: Severity::Major,
            area_id: None,
            expected: None,
            steps: None,
            environment: None,
            from: None,
            affects_feature_id: None,
            checklist: vec![],
        }
    }

    /// Reports a bug in `PROJECT`, in `area` (default General), as `pat`; returns its id.
    pub fn bug_in(&self, area: Option<&str>) -> String {
        self.bug_with(|p| p.area_id = area.map(str::to_owned))
    }

    pub fn bug_with(&self, f: impl FnOnce(&mut BugReportParams)) -> String {
        let mut p = self.report_params(PROJECT);
        f(&mut p);
        let id = p.item_id.clone();
        self.ok(&pat(), WorkCall::BugReport(p));
        id
    }

    /// A new bug in the area led by riley (no waiting on the owner).
    pub fn bug(&self) -> String {
        self.bug_in(Some(AREA_PAY))
    }

    pub fn triage_params(&self, id: &str, area: &str) -> BugTriageParams {
        BugTriageParams {
            change_id: self.id(),
            item_id: id.into(),
            expected_version: self.version(id),
            area_id: area.into(),
            severity: Severity::Major,
            priority: Priority::High,
            assignee_id: None,
        }
    }

    pub fn triage(&self, id: &str) -> WorkAccepted {
        let area = self.item(id).bug.area_id.expect("a bug has an area");
        let p = self.triage_params(id, &area);
        self.ok(&app(), WorkCall::BugTriage(p))
    }

    /// A triaged bug in the area led by riley.
    pub fn triaged(&self) -> String {
        let id = self.bug();
        self.triage(&id);
        id
    }

    /// A task that fixes `bug`, made in `PROJECT`.
    pub fn fix_task(&self, bug: &str) -> String {
        self.task_with(|p| {
            p.fixes = vec![NewFixLink {
                link_id: self.id(),
                bug_id: bug.into(),
            }]
        })
    }

    pub fn fixed_params(&self, id: &str) -> BugMarkFixedParams {
        BugMarkFixedParams {
            change_id: self.id(),
            item_id: id.into(),
            expected_version: self.version(id),
            note: "Rounded the total".into(),
            anyway_reason: None,
        }
    }

    pub fn verify_params(&self, id: &str, outcome: VerifyOutcome) -> BugVerifyParams {
        BugVerifyParams {
            change_id: self.id(),
            item_id: id.into(),
            expected_version: self.version(id),
            outcome,
            note: (outcome == VerifyOutcome::StillHappens).then(|| "Still off by a cent".into()),
        }
    }

    pub fn wont_fix_params(&self, id: &str) -> BugCloseParams {
        BugCloseParams {
            change_id: self.id(),
            item_id: id.into(),
            expected_version: self.version(id),
            duplicate_of: None,
            resolution: Some(Resolution::WontFix),
            reason: Some("By design".into()),
        }
    }

    pub fn duplicate_params(&self, id: &str, original: &str) -> BugCloseParams {
        BugCloseParams {
            change_id: self.id(),
            item_id: id.into(),
            expected_version: self.version(id),
            duplicate_of: Some(original.into()),
            resolution: None,
            reason: None,
        }
    }

    pub fn reopen_params(&self, id: &str) -> BugReopenParams {
        BugReopenParams {
            change_id: self.id(),
            item_id: id.into(),
            expected_version: self.version(id),
            note: "It came back".into(),
        }
    }

    pub fn bug_update(&self, id: &str) -> BugUpdateParams {
        BugUpdateParams {
            change_id: self.id(),
            item_id: id.into(),
            expected_version: self.version(id),
            title: None,
            what_happened: None,
            expected: None,
            clear_expected: false,
            steps: None,
            clear_steps: false,
            environment: None,
            clear_environment: false,
            severity: None,
            priority: None,
            area_id: None,
            affects_feature_id: None,
            clear_affects_feature: false,
        }
    }

    pub fn bug_assign(&self, id: &str, to: Option<&str>) -> BugAssignParams {
        BugAssignParams {
            change_id: self.id(),
            item_id: id.into(),
            expected_version: self.version(id),
            assignee_id: to.map(str::to_owned),
        }
    }

    /// Drives a fresh bug (area led by riley) to `status`: `New`, `Triaged`, `InProgress`
    /// (through a fix task), `Fixed` (by hand) or `Closed` (won't fix).
    pub fn bug_at(&self, status: ItemStatus) -> String {
        let id = self.bug();
        if status == ItemStatus::New {
            return id;
        }
        self.triage(&id);
        match status {
            ItemStatus::Triaged => {}
            ItemStatus::InProgress => {
                let task = self.fix_task(&id);
                self.set(&task, ItemStatus::InProgress);
            }
            ItemStatus::Fixed => {
                let p = self.fixed_params(&id);
                self.ok(&app(), WorkCall::BugMarkFixed(p));
            }
            ItemStatus::Closed => {
                let p = self.wont_fix_params(&id);
                self.ok(&app(), WorkCall::BugClose(p));
            }
            other => panic!("not a bug status: {other:?}"),
        }
        assert_eq!(self.item(&id).status, status);
        id
    }

    /// The kinds of the item's events from `after_version` on, as short names.
    pub fn kinds_after(&self, id: &str, after_version: u64) -> Vec<String> {
        self.events(id)
            .into_iter()
            .filter(|e| e.version > after_version)
            .map(|e| {
                let v = serde_json::to_value(&e.change).expect("encodes");
                let kind = v["type"].as_str().expect("tagged").to_owned();
                if e.auto {
                    format!("auto:{kind}")
                } else {
                    kind
                }
            })
            .collect()
    }
}
