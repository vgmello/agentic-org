//! `TASK-84` AC #1, #2, #5: two-kind task blocking model across BOTH `MemoryEngine` and
//! `SqliteEngine` plus lossless `Delete+Rebuild`:
//!
//! - Derived dependency blocks from live `depends_on` edges without requiring `status = blocked`,
//!   surfacing cancelled upstream prerequisites (`cancelled: true`) and auto-clearing when
//!   prerequisites reach `done`.
//! - Explicit manual/external blocks (`status = blocked`): trimmed `1..=1000`-char plain-text
//!   `blocked_reason` (multiline allowed, control chars refused), opt-in `waiting_on_owner`
//!   (default `false`), in-place `blocked -> blocked` updates preserving continuous `blocked_since`
//!   and recording `previous_blocked_reason` in `status_changed` history, atomic clear on unblock,
//!   coexistence with derived dependency blocks, `blocked_reason` searchability (`tasks.list --query`
//!   + FTS5), and filtered `tasks.history` (`event_type` & `blocks_only`).

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use org_office::attachments::ports::AttachmentsRead;
use org_office::delivery::ports::DeliveryRead;
use org_office::search::features::query as search_query;
use org_office::search::ports::SearchRead;
use org_office::tasks::ops::TaskWriteAuthority;
use org_office::tasks::ports::TasksRead;
use org_office::testkit_support::store_open::task_ops::{
    TaskAddDependencyOp, TaskCreateOp, TaskHistoryRead, TaskInspectRead, TaskListRead,
    TaskTransitionOp,
};
use org_office::ReadOp;
use org_proto::format_rfc3339_ms;
use org_proto::search::{SearchEntityType, SearchQueryParams, SearchQueryResult};
use org_proto::tasks::{
    TaskStatus, TaskStatusScope, TaskView, TasksAddDependencyParams, TasksCreateParams,
    TasksHistoryParams, TasksListParams, TasksTransitionParams,
};
use org_proto::{ApiError, ErrorCode};
use org_store::{Clock, Id, Opening, Outcome, StoreError, Timestamp};

use super::*;

struct TestSearchRead {
    request: search_query::SearchRequest,
    owner: Id,
    caller: Id,
    now_utc: String,
}

impl ReadOp for TestSearchRead {
    type Out = SearchQueryResult;

    fn read<S: TasksRead + AttachmentsRead + DeliveryRead + SearchRead>(
        self,
        snapshot: &S,
    ) -> Result<Self::Out, StoreError> {
        let resolved = search_query::read_sources(
            snapshot,
            self.request,
            &self.owner,
            &self.caller,
            &self.now_utc,
        )?
        .expect("search ok");
        Ok(search_query::answer(resolved))
    }
}

struct StepClock(AtomicU64);

impl StepClock {
    fn new(start_ms: u64) -> Arc<Self> {
        Arc::new(Self(AtomicU64::new(start_ms)))
    }

    fn advance_ms(&self, delta_ms: u64) -> u64 {
        self.0.fetch_add(delta_ms, Ordering::SeqCst) + delta_ms
    }
}

impl Clock for StepClock {
    fn now(&self) -> Timestamp {
        Timestamp(self.0.load(Ordering::SeqCst))
    }
}

struct BlockingHarness {
    place: Place,
    engine: EngineChoice,
    clock: Arc<StepClock>,
    store: Option<OfficeStore>,
    next_id: u64,
}

impl BlockingHarness {
    fn new(engine: EngineChoice) -> Self {
        let place = Place::new();
        let clock = StepClock::new(CREATION_AT.0);
        let store = Self::open_with_clock(
            &place,
            engine.clone(),
            &clock,
            Opening::Create(AnchorFacts::default()),
        );
        assert_eq!(
            create_organization(&store, 1).unwrap().outcome,
            Outcome::Completed
        );
        Self {
            place,
            engine,
            clock,
            store: Some(store),
            next_id: 100,
        }
    }

    fn open_with_clock(
        place: &Place,
        engine: EngineChoice,
        clock: &Arc<StepClock>,
        opening: Opening,
    ) -> OfficeStore {
        let mut config = place.config(engine, opening);
        config.clock = Arc::clone(clock) as Arc<dyn Clock>;
        open_store(&config).expect("open store")
    }

    fn store(&self) -> &OfficeStore {
        self.store.as_ref().expect("store open")
    }

    fn uid(&mut self) -> String {
        self.next_id += 1;
        id(0x8400_0000 + self.next_id).as_str().to_owned()
    }

    fn create_task(&mut self, title: &str, depends_on: Vec<String>) -> TaskView {
        let cmd_id = self.uid();
        let task_id = self.uid();
        self.store()
            .run(TaskCreateOp {
                actor: id(CREATOR).as_str().to_owned(),
                here: identity(),
                numbering: self.store().task_numbering(),
                params: TasksCreateParams {
                    command_id: cmd_id,
                    task_id,
                    team_id: Some(id(TOP_TEAM).as_str().to_owned()),
                    project_id: None,
                    assigned_team_id: None,
                    assignee_id: None,
                    task_type: Some("task".into()),
                    work_type: None,
                    title: title.to_owned(),
                    body_markdown: String::new(),
                    priority: None,
                    effort: None,
                    severity: None,
                    rank_order: None,
                    labels: Vec::new(),
                    reference_files: Vec::new(),
                    modified_files: Vec::new(),
                    depends_on,
                    details: None,
                    dod: None,
                    attachment_ids: Vec::new(),
                },
                prepared_files: Default::default(),
            })
            .unwrap()
            .expect("task created")
            .task
    }

    fn transition(&mut self, params: TasksTransitionParams) -> Result<TaskView, ApiError> {
        self.store()
            .run(TaskTransitionOp {
                actor: id(CREATOR).as_str().to_owned(),
                params,
                authority: TaskWriteAuthority::Unrestricted,
            })
            .unwrap()
            .map(|o| o.task)
    }

    fn add_dep(&mut self, task: &str, depends_on: &str) -> TaskView {
        let cmd_id = self.uid();
        self.store()
            .run(TaskAddDependencyOp {
                actor: id(CREATOR).as_str().to_owned(),
                params: TasksAddDependencyParams {
                    command_id: cmd_id,
                    task: task.to_owned(),
                    depends_on_task: depends_on.to_owned(),
                },
                authority: TaskWriteAuthority::Unrestricted,
            })
            .unwrap()
            .expect("dep added")
            .task
    }

    fn list(&self, params: TasksListParams) -> Vec<TaskView> {
        self.store()
            .read(TaskListRead { params })
            .unwrap()
            .0
            .expect("list ok")
            .tasks
    }

    fn inspect(&self, task: &str) -> TaskView {
        self.store()
            .read(TaskInspectRead {
                task: task.to_owned(),
                fields: None,
            })
            .unwrap()
            .0
            .expect("inspect ok")
            .task
    }

    fn history(&self, params: TasksHistoryParams) -> Vec<org_proto::tasks::TaskEventWire> {
        self.store()
            .read(TaskHistoryRead { params })
            .unwrap()
            .0
            .expect("history ok")
            .events
    }

    fn delete_and_rebuild(&mut self) {
        if let Some(store) = self.store.take() {
            store.close().expect("close before wipe");
        }
        let runtime = self.place.runtime();
        if runtime.exists() {
            std::fs::remove_dir_all(&runtime).expect("wipe runtime projection");
        }
        self.store = Some(Self::open_with_clock(
            &self.place,
            self.engine.clone(),
            &self.clock,
            Opening::Existing,
        ));
    }

    fn tr_params(&mut self, task: &str, rev: u64, status: TaskStatus) -> TasksTransitionParams {
        TasksTransitionParams {
            command_id: self.uid(),
            task: task.to_owned(),
            expected_revision: rev,
            status,
            reason: None,
            blocked_reason: None,
            waiting_on_owner: None,
            resolution: None,
            note: None,
            done_anyway_reason: None,
            final_summary: None,
        }
    }
}

#[test]
fn task_84_two_kind_blocking_dual_engine_and_delete_rebuild() {
    for engine in engines() {
        let mut h = BlockingHarness::new(engine.clone());

        // --- Part 1: Derived dependency blocks (AC #1a) ---
        let t1 = h.create_task("Provision DNS zone", vec![]);
        let t2 = h.create_task("Deploy TLS ingress", vec![t1.key.clone()]);
        let t3 = h.create_task("Write release notes", vec![]);

        assert!(!t1.dependency_blocked);
        assert!(t1.unmet_blocking_dependencies.is_empty());
        assert_eq!(t2.status, TaskStatus::Todo);
        assert!(t2.dependency_blocked, "{engine:?}");
        assert_eq!(t2.unmet_blocking_dependencies.len(), 1);
        assert_eq!(t2.unmet_blocking_dependencies[0].key, t1.key);
        assert_eq!(t2.unmet_blocking_dependencies[0].status, TaskStatus::Todo);
        assert!(!t2.unmet_blocking_dependencies[0].cancelled);

        // `--ready` excludes dependency-blocked `t2`; `--dependency-blocked` returns only `t2`.
        let ready_keys: Vec<String> = h
            .list(TasksListParams {
                status_scope: Some(TaskStatusScope::Open),
                ready: true,
                ..TasksListParams::default()
            })
            .into_iter()
            .map(|t| t.key)
            .collect();
        assert_eq!(
            ready_keys,
            vec![t1.key.clone(), t3.key.clone()],
            "{engine:?}"
        );

        let dep_blocked_keys: Vec<String> = h
            .list(TasksListParams {
                status_scope: Some(TaskStatusScope::Open),
                dependency_blocked: true,
                ..TasksListParams::default()
            })
            .into_iter()
            .map(|t| t.key)
            .collect();
        assert_eq!(dep_blocked_keys, vec![t2.key.clone()], "{engine:?}");

        // Cancelled upstream dependency is surfaced clearly (`cancelled: true`) and still blocks.
        let mut cancel_t1 = h.tr_params(&t1.key, t1.revision, TaskStatus::Cancelled);
        cancel_t1.reason = Some("Superseded temporarily".into());
        let t1_cancelled = h.transition(cancel_t1).expect("cancel t1");
        let t2_after_cancel = h.inspect(&t2.key);
        assert!(t2_after_cancel.dependency_blocked, "{engine:?}");
        assert_eq!(t2_after_cancel.unmet_blocking_dependencies.len(), 1);
        assert_eq!(
            t2_after_cancel.unmet_blocking_dependencies[0].status,
            TaskStatus::Cancelled
        );
        assert!(t2_after_cancel.unmet_blocking_dependencies[0].cancelled);

        // Moving `t1` to `done` auto-clears `t2`'s derived dependency block immediately.
        let p_reopen = h.tr_params(&t1.key, t1_cancelled.revision, TaskStatus::InProgress);
        let t1_prog = h.transition(p_reopen).expect("reopen t1");
        let p_done = h.tr_params(&t1.key, t1_prog.revision, TaskStatus::Done);
        let _t1_done = h.transition(p_done).expect("complete t1");

        let t2_unblocked = h.inspect(&t2.key);
        assert!(!t2_unblocked.dependency_blocked, "{engine:?}");
        assert!(t2_unblocked.unmet_blocking_dependencies.is_empty());
        let ready_after: Vec<String> = h
            .list(TasksListParams {
                status_scope: Some(TaskStatusScope::Open),
                ready: true,
                ..TasksListParams::default()
            })
            .into_iter()
            .map(|t| t.key)
            .collect();
        assert_eq!(
            ready_after,
            vec![t2.key.clone(), t3.key.clone()],
            "{engine:?}"
        );

        // --- Part 2: Explicit block validation refusals (AC #1b) ---
        let mut p_empty = h.tr_params(&t2.key, t2_unblocked.revision, TaskStatus::Blocked);
        p_empty.reason = Some("  \n\t ".into());
        assert_eq!(
            h.transition(p_empty).unwrap_err().code,
            ErrorCode::InvalidRequest
        );

        let mut p_ctrl = h.tr_params(&t2.key, t2_unblocked.revision, TaskStatus::Blocked);
        p_ctrl.blocked_reason = Some("bad\x07bell".into());
        assert_eq!(
            h.transition(p_ctrl).unwrap_err().code,
            ErrorCode::InvalidRequest
        );

        let mut p_too_long = h.tr_params(&t2.key, t2_unblocked.revision, TaskStatus::Blocked);
        p_too_long.reason = Some("é".repeat(1001));
        assert_eq!(
            h.transition(p_too_long).unwrap_err().code,
            ErrorCode::TooLarge
        );

        // Refuses `blocked_reason`, `waiting_on_owner: Some(true|false)`, whitespace-only `reason`
        // on non-blocked transitions, and whitespace-only `note` used alone as blocked reason.
        let mut p_empty_note_block =
            h.tr_params(&t2.key, t2_unblocked.revision, TaskStatus::Blocked);
        p_empty_note_block.note = Some("  \t\n ".into());
        assert_eq!(
            h.transition(p_empty_note_block).unwrap_err().code,
            ErrorCode::InvalidRequest
        );

        let mut p_bad_nonblock =
            h.tr_params(&t2.key, t2_unblocked.revision, TaskStatus::InProgress);
        p_bad_nonblock.blocked_reason = Some("wrong status".into());
        assert_eq!(
            h.transition(p_bad_nonblock).unwrap_err().code,
            ErrorCode::InvalidRequest
        );

        for bad_wait in [true, false] {
            let mut p_bad_wait =
                h.tr_params(&t2.key, t2_unblocked.revision, TaskStatus::InProgress);
            p_bad_wait.waiting_on_owner = Some(bad_wait);
            assert_eq!(
                h.transition(p_bad_wait).unwrap_err().code,
                ErrorCode::InvalidRequest,
                "waiting_on_owner={bad_wait} must fail closed on non-blocked ({engine:?})"
            );
        }

        let mut p_blank_reason =
            h.tr_params(&t2.key, t2_unblocked.revision, TaskStatus::InProgress);
        p_blank_reason.reason = Some("   \t ".into());
        assert_eq!(
            h.transition(p_blank_reason).unwrap_err().code,
            ErrorCode::InvalidRequest
        );

        // --- Part 3: Explicit block, in-place edit preserving `blocked_since`, coexistence ---
        let t0_ms = h.clock.advance_ms(60_000);
        let t0_iso = format_rfc3339_ms(t0_ms).expect("rfc3339");
        let multiline_1000 = format!("Line 1\n{}", "é".repeat(993));
        assert_eq!(multiline_1000.chars().count(), 1000);
        let mut p_block1 = h.tr_params(&t2.key, t2_unblocked.revision, TaskStatus::Blocked);
        p_block1.reason = Some(format!("  {multiline_1000}  "));
        let b1 = h.transition(p_block1).expect("explicit block succeeds");
        assert_eq!(b1.status, TaskStatus::Blocked);
        assert_eq!(b1.blocked_reason.as_deref(), Some(multiline_1000.as_str()));
        assert_eq!(b1.blocked_since.as_deref(), Some(t0_iso.as_str()));
        assert!(!b1.waiting_on_owner, "waiting_on_owner defaults to false");

        // In-place `blocked -> blocked` at `t1`: updates reason & sets `waiting_on_owner = true`,
        // records `previous_blocked_reason`, WITHOUT resetting `blocked_since`!
        let t1_ms = h.clock.advance_ms(120_000);
        let t1_iso = format_rfc3339_ms(t1_ms).expect("rfc3339");
        let mut p_block2 = h.tr_params(&t2.key, b1.revision, TaskStatus::Blocked);
        p_block2.reason = Some("Waiting on owner DNS sign-off".into());
        p_block2.waiting_on_owner = Some(true);
        let b2 = h.transition(p_block2).expect("in-place blocked edit");
        assert_eq!(b2.status, TaskStatus::Blocked);
        assert_eq!(
            b2.blocked_reason.as_deref(),
            Some("Waiting on owner DNS sign-off")
        );
        assert_eq!(
            b2.blocked_since.as_deref(),
            Some(t0_iso.as_str()),
            "in-place blocked->blocked must preserve continuous blocked_since"
        );
        assert_eq!(b2.updated_at.as_deref(), Some(t1_iso.as_str()));
        assert!(b2.waiting_on_owner);

        // Second in-place `blocked -> blocked` at `t2` omitting `waiting_on_owner` preserves `true` and `t0`.
        let t2_ms = h.clock.advance_ms(120_000);
        let t2_iso = format_rfc3339_ms(t2_ms).expect("rfc3339");
        let mut p_block3 = h.tr_params(&t2.key, b2.revision, TaskStatus::Blocked);
        p_block3.blocked_reason = Some("Waiting on owner DNS sign-off (escalated)".into());
        let b3 = h
            .transition(p_block3)
            .expect("second in-place blocked edit");
        assert_eq!(b3.blocked_since.as_deref(), Some(t0_iso.as_str()));
        assert_eq!(b3.updated_at.as_deref(), Some(t2_iso.as_str()));
        assert!(b3.waiting_on_owner);

        // Coexisting explicit block + derived dependency block on the SAME task!
        let b3_coexist = h.add_dep(&t2.key, &t3.key);
        assert_eq!(b3_coexist.status, TaskStatus::Blocked);
        assert_eq!(
            b3_coexist.blocked_reason.as_deref(),
            Some("Waiting on owner DNS sign-off (escalated)")
        );
        assert!(b3_coexist.waiting_on_owner);
        assert!(b3_coexist.dependency_blocked);
        assert_eq!(b3_coexist.unmet_blocking_dependencies.len(), 1);
        assert_eq!(b3_coexist.unmet_blocking_dependencies[0].key, t3.key);

        // `tasks.list --query` searches `blocked_reason` on BOTH engines.
        let query_hits = h.list(TasksListParams {
            status_scope: Some(TaskStatusScope::Open),
            query: Some("dns sign-off".into()),
            ..TasksListParams::default()
        });
        assert_eq!(query_hits.len(), 1, "{engine:?}");
        assert_eq!(query_hits[0].key, t2.key);

        // FTS5 / shared search indexes `blocked_reason` too!
        let search_req = search_query::validate(SearchQueryParams {
            q: "sign-off".into(),
            types: vec![SearchEntityType::Task],
            ..SearchQueryParams::default()
        })
        .expect("valid search query");
        let creator_id = id(CREATOR);
        let fts_hits = h
            .store()
            .read(TestSearchRead {
                request: search_req,
                owner: creator_id.clone(),
                caller: creator_id,
                now_utc: t2_iso,
            })
            .unwrap()
            .0;
        assert_eq!(
            fts_hits.hits.len(),
            1,
            "FTS5 must index blocked_reason ({engine:?})"
        );
        assert_eq!(fts_hits.hits[0].entity_id, t2.id);

        // --- Part 4: Lossless Delete+Rebuild parity ---
        let before_list = h.list(TasksListParams::default());
        let before_inspect = h.inspect(&t2.key);
        let before_history = h.history(TasksHistoryParams::new(&t2.key));
        h.delete_and_rebuild();
        assert_eq!(
            h.list(TasksListParams::default()),
            before_list,
            "{engine:?}"
        );
        assert_eq!(h.inspect(&t2.key), before_inspect, "{engine:?}");
        assert_eq!(
            h.history(TasksHistoryParams::new(&t2.key)),
            before_history,
            "{engine:?}"
        );

        // --- Part 5: Atomic unblock clears explicit fields while retaining full history ---
        let _t3_ms = h.clock.advance_ms(60_000);
        let mut p_unblock = h.tr_params(&t2.key, b3_coexist.revision, TaskStatus::InProgress);
        p_unblock.reason = Some("Owner signed off on DNS".into());
        let unblocked = h.transition(p_unblock).expect("unblock succeeds");
        assert_eq!(unblocked.status, TaskStatus::InProgress);
        assert_eq!(unblocked.blocked_reason, None);
        assert_eq!(unblocked.blocked_since, None);
        assert!(!unblocked.waiting_on_owner);
        // Derived dependency block on `t3` still coexists cleanly until `t3` is done!
        assert!(unblocked.dependency_blocked);

        // Old `blocked_reason` no longer matches `tasks.list --query`.
        let post_unblock_hits = h.list(TasksListParams {
            query: Some("escalated".into()),
            ..TasksListParams::default()
        });
        assert!(post_unblock_hits.is_empty(), "{engine:?}");

        // Filtered `tasks.history`: `event_type = status_changed` and `blocks_only = true`.
        let block_events = h.history(TasksHistoryParams {
            task: t2.key.clone(),
            event_type: None,
            blocks_only: true,
        });
        // 4 blocking status events: todo->blocked, blocked->blocked (#1), blocked->blocked (#2), blocked->in_progress.
        assert_eq!(block_events.len(), 4, "{engine:?}: {block_events:#?}");
        assert_eq!(block_events[0].change["to"], "blocked");
        assert_eq!(block_events[1].change["from"], "blocked");
        assert_eq!(block_events[1].change["to"], "blocked");
        assert_eq!(
            block_events[1].change["previous_blocked_reason"],
            multiline_1000
        );
        assert_eq!(block_events[1].change["previous_waiting_on_owner"], false);
        assert_eq!(block_events[1].change["waiting_on_owner"], true);
        assert_eq!(block_events[1].change["blocked_since"], t0_iso);
        assert_eq!(
            block_events[2].change["previous_blocked_reason"],
            "Waiting on owner DNS sign-off"
        );
        assert_eq!(block_events[3].change["from"], "blocked");
        assert_eq!(block_events[3].change["to"], "in_progress");
        assert_eq!(block_events[3].change["note"], "Owner signed off on DNS");
        assert_eq!(
            block_events[3].change["previous_blocked_reason"],
            "Waiting on owner DNS sign-off (escalated)"
        );

        let status_only = h.history(TasksHistoryParams {
            task: t2.key.clone(),
            event_type: Some("status_changed".into()),
            blocks_only: false,
        });
        assert_eq!(status_only.len(), 4, "{engine:?}");

        // Re-blocking via `note` alias starts a fresh `blocked_since` at `t4` (`t4 != t0`).
        let t4_ms = h.clock.advance_ms(300_000);
        let t4_iso = format_rfc3339_ms(t4_ms).expect("rfc3339");
        assert_ne!(t4_iso, t0_iso);
        let mut p_reblock = h.tr_params(&t2.key, unblocked.revision, TaskStatus::Blocked);
        p_reblock.note = Some("  New external dependency  ".into());
        let reblocked = h.transition(p_reblock).expect("reblock via note alias");
        assert_eq!(
            reblocked.blocked_reason.as_deref(),
            Some("New external dependency")
        );
        assert_eq!(reblocked.blocked_since.as_deref(), Some(t4_iso.as_str()));
        assert!(!reblocked.waiting_on_owner);
    }
}
