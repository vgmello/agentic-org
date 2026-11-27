//! The Chief of Staff's partitions under contention (`TASK-80.1`, pass 2): the partition check
//! and the start-slot reservation are one critical section, so a second caller never rides a
//! start in flight or a busy turn; a warm send makes the session busy before its slot is free;
//! a wake never joins or drains into a member's live turn and starts only in `tess:wake`; a
//! partition keeps its own conversation, rotates at 60% of its window, and a failover serves
//! the failed run's partition and never lands a member's turn in the shared wake queue.

use std::sync::mpsc;

use org_agent_runtime_claude::ClaudeRuntime;
use org_office::organization_family::records::BuiltinKey;
use org_office::rules::OrgCommand;
use org_proto::org::MemberAddPersonParams;
use org_proto::ErrorCode;

use super::*;
use crate::credentials::now_ms;
use crate::manager::{LiveSessionHandle, StartReservation};
use crate::partition::{
    PartitionHandle, PartitionKey, PartitionTurn, DEFAULT_CONTEXT_WINDOW_TOKENS,
};
use crate::pump::{FailoverRequest, PumpCommand, SessionRuntimeState};
use crate::wake::{PendingWakeItem, PendingWakeQueueRecord, WakeKind};

const SWITCH: &str = "chief_of_staff_partition_switch";

struct Fixture {
    office: Arc<Office>,
    manager: Arc<SessionManager>,
    owner: String,
    member: String,
    tess: String,
    _sock: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let office = Arc::new(Office::for_test(TestOffice::default()));
        let owner = office.owner_member_id().to_owned();
        let root = office.org_snapshot().unwrap().teams[0].id.clone();
        let member = new_uuid_v7();
        office
            .submit_org(
                &owner,
                OrgCommand::MemberAddPerson(MemberAddPersonParams {
                    command_id: new_uuid_v7(),
                    member_id: member.clone(),
                    name: "Bea".into(),
                    handle: Some("bea".into()),
                    home_team_id: Some(root),
                    contact: None,
                }),
            )
            .unwrap()
            .unwrap();
        let tess = office
            .chief_of_staff()
            .unwrap()
            .expect("Tess")
            .id
            .to_string();
        let sock = tempfile::tempdir().unwrap();
        let mut cfg = SessionManagerConfig::production(sock.path().join("office.sock"));
        // No runtime binary resolves: a start that gets past the partition check is refused
        // before anything is spawned.
        let mut runtimes = RuntimeRegistry::production();
        runtimes.register(
            Arc::new(ClaudeRuntime::new()),
            Some(PathBuf::from("/nonexistent/claude")),
        );
        runtimes.register(
            Arc::new(org_agent_runtime_codex::CodexRuntime::new()),
            Some(PathBuf::from("/nonexistent/codex")),
        );
        runtimes.register(
            Arc::new(org_agent_runtime_agy::AgyRuntime::new()),
            Some(PathBuf::from("/nonexistent/agy")),
        );
        cfg.runtimes = runtimes;
        let manager = Arc::new(SessionManager::new(Arc::clone(&office), cfg));
        SessionManager::attach_self(&manager);
        Self {
            office,
            manager,
            owner,
            member,
            tess,
            _sock: sock,
        }
    }

    fn tess(&self) -> org_office::organization_family::records::Employee {
        self.office.chief_of_staff().unwrap().unwrap()
    }

    fn owner_key(&self) -> PartitionKey {
        PartitionKey::tess_ask(&self.owner)
    }

    fn member_key(&self) -> PartitionKey {
        PartitionKey::tess_ask(&self.member)
    }

    /// Installs a stand-in live process of Tess serving `partition`, in `state`, holding
    /// `conversation`; its pump commands are captured.
    fn install(
        &self,
        state: SessionState,
        partition: Option<PartitionKey>,
        conversation: Option<&str>,
    ) -> mpsc::Receiver<PumpCommand> {
        let (handle, rx) = stub_handle(
            &self.office.info().office_id,
            &self.tess,
            state,
            partition,
            conversation,
        );
        self.manager
            .registry
            .lock()
            .unwrap()
            .live
            .insert(self.tess.clone(), handle);
        rx
    }

    fn reserve(
        &self,
        text: &str,
        partition: &PartitionKey,
    ) -> Result<StartReservation, org_proto::ApiError> {
        self.manager.reserve_start_or_send(
            &self.tess(),
            Some(text.to_owned()),
            None,
            None,
            Some(partition),
        )
    }

    fn release_slot(&self) {
        self.manager
            .registry
            .lock()
            .unwrap()
            .starting
            .remove(&self.tess);
    }

    fn live_state(&self) -> Option<SessionState> {
        let reg = self.manager.registry.lock().unwrap();
        reg.live
            .get(&self.tess)
            .map(|h| h.state.lock().unwrap().state)
    }

    fn wake_from_member(&self, id: &str) -> PendingWakeItem {
        PendingWakeItem {
            item_id: id.into(),
            kind: WakeKind::DirectMessage,
            sender_id: self.member.clone(),
            sender_name: "Bea".into(),
            sender_handle: None,
            sender_relation: None,
            sender_is_person: true,
            conversation_id: Some("dm-bea-tess".into()),
            source_id: format!("msg-{id}"),
            summary: "Bea: what is the owner planning?".into(),
            enqueued_at_ms: now_ms(),
        }
    }

    fn queued(&self) -> Vec<String> {
        self.manager
            .store
            .read_pending_wakes(&self.tess)
            .unwrap()
            .map(|q| q.items.into_iter().map(|i| i.item_id).collect())
            .unwrap_or_default()
    }
}

fn stub_handle(
    office_id: &str,
    emp_id: &str,
    state: SessionState,
    partition: Option<PartitionKey>,
    conversation: Option<&str>,
) -> (LiveSessionHandle, mpsc::Receiver<PumpCommand>) {
    let (tx, rx) = mpsc::channel();
    let runtime_state = Arc::new(Mutex::new(SessionRuntimeState {
        employee_id: emp_id.into(),
        office_id: office_id.into(),
        state,
        run_id: Some("run-live".into()),
        session_id: Some("sess-live".into()),
        runtime_id: Some("claude".into()),
        runtime_session_id: conversation.map(str::to_owned),
        private_home: "/tmp/tess-home".into(),
        permission_preset: "operator_sandboxed".into(),
        sandbox_enabled: true,
        spec_hash: "h".into(),
        watchdog: StallWatchdog::new(Duration::from_secs(60), Instant::now()),
        recovery_required: false,
        last_reply: None,
        last_error: None,
        backgrounds: BackgroundRegistry::default(),
        pending_approvals: Vec::new(),
        policy: org_agent_runtime::PermissionPolicy {
            version: 1,
            allow: Vec::new(),
            deny: Vec::new(),
        },
        preset: PermissionPreset::OperatorSandboxed,
        allowed_tools: None,
        writable_roots: Vec::new(),
        suspended_on_asks: Vec::new(),
        waiting_for_member_id: None,
        waiting_for_name: None,
        suspended_since: None,
        pending_wake_count: 0,
        pending_wake_reason: None,
        queued_turns: 0,
    }));
    let handle = LiveSessionHandle {
        state: runtime_state,
        tx,
        thread: None,
        connection_id: None,
        route_digest: String::new(),
        oauth_expires_at_ms: None,
        workspace: None,
        active_permit: Arc::new(Mutex::new(None)),
        supports_mid_turn: true,
        bwrap_masked_siblings: None,
        partition,
    };
    (handle, rx)
}

fn drained(rx: &mpsc::Receiver<PumpCommand>) -> Vec<PumpCommand> {
    rx.try_iter().collect()
}

fn stop_reasons(cmds: &[PumpCommand]) -> Vec<String> {
    cmds.iter()
        .filter_map(|c| match c {
            PumpCommand::Stop { reason } => Some(reason.clone()),
            _ => None,
        })
        .collect()
}

fn sent_turns(cmds: &[PumpCommand]) -> usize {
    cmds.iter()
        .filter(|c| matches!(c, PumpCommand::SendTurn(_)))
        .count()
}

fn refusal(result: Result<StartReservation, org_proto::ApiError>) -> org_proto::ApiError {
    match result {
        Ok(_) => panic!("expected a refusal"),
        Err(err) => err,
    }
}

#[test]
fn a_start_in_flight_refuses_another_callers_turn_before_anything_changes() {
    let f = Fixture::new();
    // Bea's cold start holds the slot; the owner asks meanwhile.
    f.manager
        .registry
        .lock()
        .unwrap()
        .starting
        .insert(f.tess.clone());
    let refused = refusal(f.reserve("Owner: plan the offsite", &f.owner_key()));
    assert_eq!(refused.code, ErrorCode::Conflict);
    // Nothing changed: no turn counted, the slot is still Bea's, no binding was touched.
    assert_eq!(f.manager.partitions.state(&f.tess, &f.owner_key()), None);
    assert!(f
        .manager
        .registry
        .lock()
        .unwrap()
        .starting
        .contains(&f.tess));
    // Bea's run is bound from its own launch's partition: it acts for Bea, never the owner.
    let run = f
        .manager
        .bindings
        .issue(
            &f.tess,
            "run-bea",
            "sess-bea",
            0,
            f.member_key().caller_id(),
        )
        .unwrap();
    assert_eq!(run.acting_for_caller_id.as_deref(), Some(f.member.as_str()));
    assert_eq!(
        f.manager.bindings.acting_for_caller_id(&f.tess).as_deref(),
        Some(f.member.as_str())
    );
}

#[test]
fn a_busy_process_of_another_partition_is_never_interrupted() {
    for busy in [
        SessionState::Starting,
        SessionState::Running,
        SessionState::WaitingApproval,
        SessionState::Stopping,
    ] {
        let f = Fixture::new();
        let rx = f.install(busy, Some(f.member_key()), Some("conv-bea"));
        let refused = refusal(f.reserve("Owner: status?", &f.owner_key()));
        assert_eq!(refused.code, ErrorCode::Conflict, "{busy:?}");
        assert!(
            drained(&rx).is_empty(),
            "{busy:?}: nothing reached Bea's run"
        );
        assert_eq!(f.live_state(), Some(busy));
        assert!(!f
            .manager
            .registry
            .lock()
            .unwrap()
            .starting
            .contains(&f.tess));
        assert_eq!(f.manager.partitions.state(&f.tess, &f.owner_key()), None);
    }
}

#[test]
fn a_warm_send_marks_her_busy_before_the_slot_is_released() {
    let f = Fixture::new();
    let rx = f.install(SessionState::Idle, Some(f.owner_key()), Some("conv-owner"));
    let warm = f.reserve("Owner: and the budget?", &f.owner_key()).unwrap();
    assert!(warm.live_view.is_some(), "a warm send");
    assert_eq!(warm.partition_turn.as_ref().map(|t| t.turn), Some(1));
    let cmds = drained(&rx);
    assert_eq!(sent_turns(&cmds), 1);
    // Busy at once, before the pump has even read the turn, with the slot already free.
    assert_eq!(f.live_state(), Some(SessionState::Running));
    assert!(!f
        .manager
        .registry
        .lock()
        .unwrap()
        .starting
        .contains(&f.tess));
    // So Bea's request in that window is refused, and the owner's turn is never stopped.
    let refused = refusal(f.reserve("Bea: hi", &f.member_key()));
    assert_eq!(refused.code, ErrorCode::Conflict);
    assert!(stop_reasons(&drained(&rx)).is_empty());
}

#[test]
fn an_idle_process_of_another_partition_is_switched_and_its_conversation_kept() {
    let f = Fixture::new();
    f.manager.partitions.begin_turn(&f.tess, &f.owner_key());
    let rx = f.install(SessionState::Idle, Some(f.owner_key()), Some("conv-owner"));
    let cold = f.reserve("Bea: hello", &f.member_key()).unwrap();
    assert!(cold.live_view.is_none());
    let replaced = cold.finished_handle.as_ref().expect("the owner's process");
    assert_eq!(replaced.partition, Some(f.owner_key()));
    assert_eq!(stop_reasons(&drained(&rx)), [SWITCH]);
    let turn: &PartitionTurn = cold.partition_turn.as_ref().expect("Bea's turn");
    assert_eq!(turn.key, f.member_key());
    assert_eq!(
        (turn.turn, turn.resume_runtime_session_id.as_deref()),
        (1, None)
    );
    // The owner's conversation was recorded for the owner's partition alone.
    let owner = f.manager.partitions.state(&f.tess, &f.owner_key()).unwrap();
    assert_eq!(owner.runtime_session_id.as_deref(), Some("conv-owner"));
    assert!(!f
        .manager
        .registry
        .lock()
        .unwrap()
        .live
        .contains_key(&f.tess));
    f.release_slot();
    let back = f.manager.partitions.begin_turn(&f.tess, &f.owner_key());
    assert_eq!(
        back.resume_runtime_session_id.as_deref(),
        Some("conv-owner")
    );
}

#[test]
fn her_partition_rotates_on_the_turn_after_it_fills_sixty_percent_of_its_window() {
    let f = Fixture::new();
    let key = f.owner_key();
    f.manager.partitions.begin_turn(&f.tess, &key);
    let run = PartitionHandle::new(f.manager.partitions.clone(), f.tess.clone(), key.clone());
    run.record_runtime_session("conv-1");
    let usage = |input: u64| {
        let mut u = Usage::new(UsageScope::TurnDelta, UsageScope::TurnDelta);
        u.input = input;
        u.cache_read = 1_000;
        u
    };
    // 59%: the conversation carries on.
    run.observe_usage(&usage(DEFAULT_CONTEXT_WINDOW_TOKENS * 59 / 100 - 1_000));
    assert!(!f.manager.partitions.needs_rotation(&f.tess, &key));
    // 60%: the next turn starts a fresh conversation in a fresh process.
    run.observe_usage(&usage(DEFAULT_CONTEXT_WINDOW_TOKENS * 60 / 100 - 1_000));
    assert!(f.manager.partitions.needs_rotation(&f.tess, &key));
    let rx = f.install(SessionState::Idle, Some(key.clone()), Some("conv-1"));
    let fresh = f.reserve("Owner: next", &key).unwrap();
    assert!(
        fresh.live_view.is_none(),
        "not sent into the full conversation"
    );
    assert_eq!(stop_reasons(&drained(&rx)), [SWITCH]);
    let turn = fresh.partition_turn.expect("the turn");
    assert!(turn.rotated);
    assert_eq!((turn.turn, turn.resume_runtime_session_id), (1, None));
    f.release_slot();
}

#[test]
fn an_autonomous_turn_resumes_only_her_wake_partitions_conversation() {
    let wake = PartitionTurn {
        key: PartitionKey::tess_wake(),
        resume_runtime_session_id: Some("conv-wake".into()),
        rotated: false,
        turn: 2,
    };
    let resume = crate::manager::autonomous_resume;
    // The last run's (or a suspended run's) conversation may be a member's: never resumed.
    assert_eq!(
        resume(Some(&wake), true, Some("conv-owner")),
        Some("conv-wake")
    );
    assert_eq!(resume(None, true, Some("conv-owner")), None);
    // Everyone else resumes what they asked for.
    assert_eq!(resume(None, false, Some("conv-x")), Some("conv-x"));
}

#[test]
fn a_wake_never_drains_into_or_joins_a_members_live_turn() {
    let f = Fixture::new();
    // Her runs get no mid-turn delivery at all; other agents' do.
    let hers = f
        .manager
        .build_pump_hooks(&f.tess, String::new(), None, None, true);
    assert!(hers.stage_mid_turn_wakes.is_none() && hers.on_mid_turn_delivered.is_none());
    let others = f
        .manager
        .build_pump_hooks(&f.tess, String::new(), None, None, false);
    assert!(others.stage_mid_turn_wakes.is_some());

    // The owner's turn is running: Bea's DM waits, and nothing reaches the owner's process.
    let rx = f.install(
        SessionState::Running,
        Some(f.owner_key()),
        Some("conv-owner"),
    );
    let outcome = f.manager.enqueue_wake(&f.tess, f.wake_from_member("w-1"));
    assert!(
        matches!(outcome, crate::wake::WakeOutcome::Queued { .. }),
        "{outcome:?}"
    );
    let cmds = drained(&rx);
    assert_eq!(sent_turns(&cmds), 0);
    assert!(stop_reasons(&cmds).is_empty());

    // The owner's turn ended: a turn-end drain never sends the wake into the idle process.
    f.manager
        .registry
        .lock()
        .unwrap()
        .live
        .get(&f.tess)
        .unwrap()
        .state
        .lock()
        .unwrap()
        .state = SessionState::Idle;
    let acquirer: crate::lifecycle::WarmPermitAcquirer =
        Arc::new(|_, _, _, _, _, _| panic!("a drain into her process never gets this far"));
    crate::lifecycle::drain_into_live_pump_with_acquirer(&f.manager, &f.tess, acquirer);
    assert_eq!(sent_turns(&drained(&rx)), 0);
    assert_eq!(f.queued(), ["w-1"]);
    assert_eq!(
        f.manager.bindings.acting_for_caller_id(&f.tess),
        None,
        "no run acts for the owner on Bea's wake"
    );
}

#[test]
fn her_turn_end_starts_a_queued_dm_in_its_senders_partition_never_in_the_owners_process() {
    let f = Fixture::new();
    let rx = f.install(SessionState::Idle, Some(f.owner_key()), Some("conv-owner"));
    let mut queue = PendingWakeQueueRecord::empty(&f.tess);
    queue.push_dedup(f.wake_from_member("w-2"));
    f.manager.store.write_pending_wakes(&queue).unwrap();
    f.manager.partitions.begin_turn(&f.tess, &f.owner_key());

    let hooks = f
        .manager
        .build_pump_hooks(&f.tess, String::new(), None, None, true);
    (hooks.on_turn_ended.expect("the hook"))(&f.tess);

    // The dispatch switches partitions: the owner's idle process is stopped, never sent to.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut cmds = Vec::new();
    while Instant::now() < deadline && stop_reasons(&cmds).is_empty() {
        cmds.extend(drained(&rx));
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(stop_reasons(&cmds), [SWITCH]);
    assert_eq!(sent_turns(&cmds), 0);
    // Bea's DM ran (or was refused) in her own partition, never in `tess:wake`: no runtime is
    // installed here, so it stays queued for a retry, and the owner's conversation is kept.
    let wait = Instant::now() + Duration::from_secs(10);
    while Instant::now() < wait
        && f.manager
            .registry
            .lock()
            .unwrap()
            .starting
            .contains(&f.tess)
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(f
        .manager
        .partitions
        .state(&f.tess, &f.member_key())
        .is_some());
    assert!(f
        .manager
        .partitions
        .state(&f.tess, &PartitionKey::tess_wake())
        .is_none());
    assert_eq!(f.queued(), ["w-2"]);
    assert_eq!(f.manager.bindings.acting_for_caller_id(&f.tess), None);
    let owner = f.manager.partitions.state(&f.tess, &f.owner_key()).unwrap();
    assert_eq!(owner.runtime_session_id.as_deref(), Some("conv-owner"));
}

fn failover(f: &Fixture, partition: Option<PartitionKey>) -> FailoverRequest {
    FailoverRequest {
        employee_id: f.tess.clone(),
        failed_run_id: "run-live".into(),
        failed_connection_id: "conn-1".into(),
        objective: "Owner: draft the board update".into(),
        workspace: None,
        task_ref: None,
        tried_connections: vec!["conn-1".into()],
        oauth_refresh_attempted: false,
        oauth_token_fp: None,
        reason: "rate_limited",
        rate_limit_resets_at: None,
        stop_epoch: 0,
        partition,
    }
}

#[test]
fn a_failover_serves_the_failed_runs_partition_and_never_queues_a_members_turn() {
    let f = Fixture::new();
    let req = failover(&f, Some(f.owner_key()));
    assert_eq!(req.acting_for_caller_id(), Some(f.owner.as_str()));
    // The relaunch is planned for exactly the failed run's partition, so its binding acts for
    // exactly the same caller.
    let opts = crate::manager::failover_route_opts(&req, false);
    assert_eq!(opts.partition, Some(f.owner_key()));
    let relaunch = f
        .manager
        .bindings
        .issue(
            &f.tess,
            "run-re",
            "sess-re",
            0,
            opts.partition.as_ref().and_then(PartitionKey::caller_id),
        )
        .unwrap();
    assert_eq!(
        relaunch.acting_for_caller_id.as_deref(),
        Some(f.owner.as_str())
    );
    f.manager.bindings.revoke(&f.tess, None);

    // Contended: the owner's request is never put in the shared queue, where `tess:wake`
    // would run it for nobody.
    f.manager
        .registry
        .lock()
        .unwrap()
        .starting
        .insert(f.tess.clone());
    crate::manager::run_failover_relaunch(&f.manager, req);
    assert!(f.queued().is_empty(), "{:?}", f.queued());
    // Nor is what an agent addressed to her: `tess:wake` never receives a sender's message.
    crate::manager::run_failover_relaunch(
        &f.manager,
        failover(&f, Some(PartitionKey::tess_dm("agent-ada"))),
    );
    assert!(f.queued().is_empty(), "{:?}", f.queued());
    // Neither is logged as requeued: only as deliberately not requeued.
    let history =
        std::fs::read_to_string(f.manager.store.history_path(&f.tess).unwrap()).unwrap_or_default();
    assert_eq!(history.matches("pool_failover_not_requeued").count(), 2);
    assert!(!history.contains("\"pool_failover_requeued\""), "{history}");
    // A `tess:wake` turn came from the shared queue and goes back to it.
    crate::manager::run_failover_relaunch(
        &f.manager,
        failover(&f, Some(PartitionKey::tess_wake())),
    );
    assert_eq!(f.queued().len(), 1);
    f.release_slot();
}

#[test]
fn the_operator_rules_folder_keeps_only_the_offices_execpolicy() {
    use std::os::unix::fs::symlink;
    let home = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let rules = home.path().join(".codex/rules");
    fs::create_dir_all(rules.join("nested")).unwrap();
    fs::write(rules.join("office-operator.rules"), "# office").unwrap();
    fs::write(
        rules.join("allow-all.rules"),
        "prefix_rule(decision=\"allow\")",
    )
    .unwrap();
    fs::write(rules.join("nested/x.rules"), "x").unwrap();
    fs::write(outside.path().join("keep.txt"), "outside").unwrap();
    symlink(outside.path().join("keep.txt"), rules.join("linked.rules")).unwrap();
    crate::manager::clean_operator_rules(home.path()).unwrap();
    let left: Vec<String> = fs::read_dir(&rules)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, ["office-operator.rules"]);
    assert!(
        outside.path().join("keep.txt").exists(),
        "a link is never followed"
    );

    // Her own file as a link, the rules folder as a link, `.codex` as a link: removed as links.
    fs::remove_file(rules.join("office-operator.rules")).unwrap();
    symlink(
        outside.path().join("keep.txt"),
        rules.join("office-operator.rules"),
    )
    .unwrap();
    crate::manager::clean_operator_rules(home.path()).unwrap();
    assert_eq!(fs::read_dir(&rules).unwrap().count(), 0);
    fs::remove_dir_all(&rules).unwrap();
    symlink(outside.path(), &rules).unwrap();
    crate::manager::clean_operator_rules(home.path()).unwrap();
    assert!(!rules.exists() && fs::symlink_metadata(&rules).is_err());
    fs::remove_dir_all(home.path().join(".codex")).unwrap();
    symlink(outside.path(), home.path().join(".codex")).unwrap();
    crate::manager::clean_operator_rules(home.path()).unwrap();
    assert!(fs::symlink_metadata(home.path().join(".codex")).is_err());
    assert!(outside.path().join("keep.txt").exists());
    // Nothing there: nothing to do.
    crate::manager::clean_operator_rules(home.path()).unwrap();
}

#[test]
fn only_the_chief_of_staff_is_partitioned() {
    let f = Fixture::new();
    let hr = f
        .office
        .list_employees()
        .unwrap()
        .into_iter()
        .find(|e| e.builtin_key == Some(BuiltinKey::Hr))
        .unwrap();
    assert_eq!(SessionManager::turn_partition(&hr, None), None);
    assert_eq!(
        SessionManager::turn_partition(&f.tess(), None),
        Some(PartitionKey::tess_wake())
    );
}

fn wake_item(
    id: &str,
    kind: WakeKind,
    sender: &str,
    sender_is_person: bool,
    summary: &str,
) -> PendingWakeItem {
    PendingWakeItem {
        item_id: id.into(),
        kind,
        sender_id: sender.into(),
        sender_name: sender.into(),
        sender_handle: None,
        sender_relation: None,
        sender_is_person,
        conversation_id: Some(format!("conv-of-{id}")),
        source_id: format!("msg-{id}"),
        summary: summary.into(),
        enqueued_at_ms: now_ms(),
    }
}

/// What one sender addressed to her runs in that sender's partition: a person's acting for
/// them, an agent's in its own `tess:dm:` partition acting for nobody; everything else in
/// `tess:wake`. Anyone else's wakes are not partitioned.
#[test]
fn each_wake_runs_in_the_partition_of_who_addressed_her() {
    let f = Fixture::new();
    let tess = f.tess();
    let partition = |item: &PendingWakeItem| f.manager.wake_partition(&tess, item);
    for kind in [WakeKind::DirectMessage, WakeKind::Handoff, WakeKind::Ask] {
        let from_owner = wake_item("w", kind, &f.owner, true, "hi");
        assert_eq!(partition(&from_owner), Some(f.owner_key()), "{kind:?}");
        let from_agent = wake_item("w", kind, "agent-ada", false, "hi");
        let agent_key = partition(&from_agent).unwrap();
        assert_eq!(agent_key, PartitionKey::tess_dm("agent-ada"), "{kind:?}");
        assert_eq!(agent_key.caller_id(), None, "an agent's DM acts for nobody");
    }
    for kind in [
        WakeKind::Mention,
        WakeKind::TaskAssignment,
        WakeKind::Recovery,
    ] {
        let item = wake_item("w", kind, &f.owner, true, "hi");
        assert_eq!(
            partition(&item),
            Some(PartitionKey::tess_wake()),
            "{kind:?}"
        );
    }
    // An answer from a conversation the office cannot name counts as private (fail closed).
    for kind in [WakeKind::AskAnswer, WakeKind::WatcherAnswer] {
        let item = wake_item("w", kind, &f.owner, true, "hi");
        assert_eq!(partition(&item), Some(f.owner_key()), "{kind:?}");
    }
    let hr = f
        .office
        .list_employees()
        .unwrap()
        .into_iter()
        .find(|e| e.builtin_key == Some(BuiltinKey::Hr))
        .expect("HR");
    let dm = wake_item("w", WakeKind::DirectMessage, &f.owner, true, "hi");
    assert_eq!(f.manager.wake_partition(&hr, &dm), None);
    let (claimed, none) = f.manager.claim_one_partition(&hr, vec![dm.clone(), dm]);
    assert_eq!((claimed.len(), none), (2, None));
}

/// The owner's DM, Bea's DM and a `#general` mention queued together are three turns, each in
/// its own partition and conversation: the owner's DM never reaches `tess:wake`, and no turn
/// carries another caller's message.
#[test]
fn queued_dms_of_different_senders_never_share_a_turn_or_a_conversation() {
    const OWNER_SECRET: &str = "OWNER-SECRET: the acquisition closes on Friday";
    const BEA_ASK: &str = "BEA: can you help with my review?";
    const MENTION: &str = "@tess in #general: where are the standup notes?";
    let f = Fixture::new();
    let mut queue = PendingWakeQueueRecord::empty(&f.tess);
    queue.push_dedup(wake_item(
        "w-owner",
        WakeKind::DirectMessage,
        &f.owner,
        true,
        OWNER_SECRET,
    ));
    queue.push_dedup(wake_item(
        "w-bea",
        WakeKind::DirectMessage,
        &f.member,
        true,
        BEA_ASK,
    ));
    queue.push_dedup(wake_item(
        "w-general",
        WakeKind::Mention,
        &f.member,
        true,
        MENTION,
    ));
    f.manager.store.write_pending_wakes(&queue).unwrap();

    // One turn per partition, each sent to the idle process serving that partition alone.
    let turn_in = |partition: PartitionKey, conversation: &str| -> String {
        let rx = f.install(SessionState::Idle, Some(partition), Some(conversation));
        let outcome = f.manager.process_due_wakes_for(&f.tess);
        assert_eq!(outcome, crate::wake::WakeOutcome::Started, "{conversation}");
        let cmds = drained(&rx);
        assert!(
            stop_reasons(&cmds).is_empty(),
            "{conversation}: never switched"
        );
        let turns: Vec<String> = cmds
            .into_iter()
            .filter_map(|c| match c {
                PumpCommand::SendTurn(text) => Some(text),
                _ => None,
            })
            .collect();
        assert_eq!(turns.len(), 1, "{conversation}: one turn");
        turns.into_iter().next().unwrap()
    };

    let owner_turn = turn_in(f.owner_key(), "conv-owner");
    assert!(owner_turn.contains(OWNER_SECRET));
    assert!(!owner_turn.contains(BEA_ASK) && !owner_turn.contains(MENTION));
    assert_eq!(f.queued(), ["w-bea", "w-general"]);

    let bea_turn = turn_in(f.member_key(), "conv-bea");
    assert!(bea_turn.contains(BEA_ASK));
    assert!(!bea_turn.contains(OWNER_SECRET) && !bea_turn.contains(MENTION));
    assert_eq!(f.queued(), ["w-general"]);

    let wake_turn = turn_in(PartitionKey::tess_wake(), "conv-wake");
    assert!(wake_turn.contains(MENTION));
    assert!(!wake_turn.contains(OWNER_SECRET) && !wake_turn.contains(BEA_ASK));
    assert!(f.queued().is_empty());

    // Each partition ran exactly one turn; `tess:wake` never saw the owner's DM.
    for key in [f.owner_key(), f.member_key(), PartitionKey::tess_wake()] {
        let state = f.manager.partitions.state(&f.tess, &key).unwrap();
        assert_eq!(state.turns, 1, "{key}");
    }
}

/// Opens `from`'s DM with `to` and sends `body` into it: `(conversation, message)`.
fn dm_message(f: &Fixture, from: &str, to: &str, body: &str) -> (String, String) {
    let opened = f
        .office
        .dm_open(
            from,
            org_proto::chat::DmOpenParams {
                participants: vec![to.to_owned()],
                name: None,
                conversation_id: None,
            },
        )
        .unwrap()
        .unwrap();
    let sent = f
        .office
        .chat_send(
            from,
            org_proto::chat::ChatSendParams {
                id: new_uuid_v7(),
                to: to.to_owned(),
                kind: "msg".into(),
                priority: None,
                subject: None,
                body: body.to_owned(),
                meta: Default::default(),
                reply_to: None,
                also_in_channel: false,
                reason: None,
            },
        )
        .unwrap()
        .unwrap();
    (opened.conversation.id, sent.message.id)
}

/// Sends one wake to an idle process serving `partition`: the turns it was sent, with no
/// partition switch.
fn turns_sent_to(f: &Fixture, partition: PartitionKey) -> Vec<String> {
    let rx = f.install(SessionState::Idle, Some(partition), Some("conv-live"));
    let outcome = f.manager.process_due_wakes_for(&f.tess);
    assert_eq!(outcome, crate::wake::WakeOutcome::Started);
    let cmds = drained(&rx);
    assert!(stop_reasons(&cmds).is_empty(), "never switched");
    cmds.into_iter()
        .filter_map(|c| match c {
            PumpCommand::SendTurn(text) => Some(text),
            _ => None,
        })
        .collect()
}

/// `@tess` inside the owner's private DM, or an answer given there, runs in the owner's own
/// partition (acting for the owner), never in the shared `tess:wake`, whatever its wake kind
/// says (`TASK-80.1`, pass 4); an agent's is its own `tess:dm:` partition.
#[test]
fn a_mention_or_answer_inside_a_dm_runs_in_the_senders_partition_never_tess_wake() {
    const OWNER_SECRET: &str = "@tess OWNER-SECRET: the acquisition closes on Friday";
    const MENTION: &str = "@tess in #general: where are the standup notes?";
    let f = Fixture::new();
    let tess = f.tess();
    let (owner_dm, _) = dm_message(&f, &f.owner, &f.tess, "hello");
    let in_owner_dm = |kind: WakeKind, sender: &str, person: bool| PendingWakeItem {
        conversation_id: Some(owner_dm.clone()),
        ..wake_item("w-owner", kind, sender, person, OWNER_SECRET)
    };
    for kind in [
        WakeKind::Mention,
        WakeKind::AskAnswer,
        WakeKind::WatcherAnswer,
        WakeKind::TaskAssignment,
    ] {
        assert_eq!(
            f.manager
                .wake_partition(&tess, &in_owner_dm(kind, &f.owner, true)),
            Some(f.owner_key()),
            "{kind:?}"
        );
        assert_eq!(
            f.manager
                .wake_partition(&tess, &in_owner_dm(kind, "agent-ada", false)),
            Some(PartitionKey::tess_dm("agent-ada")),
            "{kind:?}"
        );
    }

    // Queued behind a channel mention's wake, the owner's DM mention still never shares
    // `tess:wake`: it runs alone in the owner's partition.
    let mut queue = PendingWakeQueueRecord::empty(&f.tess);
    queue.push_dedup(in_owner_dm(WakeKind::Mention, &f.owner, true));
    queue.push_dedup(wake_item(
        "w-general",
        WakeKind::Mention,
        &f.member,
        true,
        MENTION,
    ));
    f.manager.store.write_pending_wakes(&queue).unwrap();
    let turns = turns_sent_to(&f, f.owner_key());
    assert_eq!(turns.len(), 1);
    assert!(turns[0].contains("OWNER-SECRET") && !turns[0].contains(MENTION));
    assert_eq!(f.queued(), ["w-general"]);
    assert!(f
        .manager
        .partitions
        .state(&f.tess, &PartitionKey::tess_wake())
        .is_none_or(|s| s.turns == 0));
}

/// A required ask of hers never suspends her (one process serves every caller, so it would
/// hold them all), and its answer in the owner's private DM wakes her in the owner's
/// partition, never `tess:wake` (`TASK-80.1`, pass 4).
#[test]
fn her_required_ask_never_suspends_her_and_a_dm_answer_runs_in_the_answerers_partition() {
    const ANSWER: &str = "OWNER-ANSWER: Friday at nine";
    let f = Fixture::new();
    // Her question to the owner, in their private DM.
    let (owner_dm, ask) = dm_message(&f, &f.tess, &f.owner, "Which day works?");
    let entry = |waiting: &str| crate::wake::SuspendedAskEntry {
        ask_id: ask.clone(),
        conversation_id: owner_dm.clone(),
        question_summary: "Which day works?".into(),
        waiting_for_member_id: Some(waiting.to_owned()),
        waiting_for_name: None,
        asked_by_subagent: false,
        opened_at_ms: now_ms(),
    };
    f.manager.on_required_ask_opened(&f.tess, entry(&f.owner));
    f.manager.on_required_ask_wait_expired(&f.tess, &ask);
    assert!(!f
        .manager
        .open_required_asks
        .lock()
        .unwrap()
        .contains_key(&f.tess));
    assert!(f
        .manager
        .store
        .read_suspended_state(&f.tess)
        .unwrap()
        .is_none());
    // Any other agent's required ask is still recorded.
    let hr = f
        .office
        .list_employees()
        .unwrap()
        .into_iter()
        .find(|e| e.builtin_key == Some(BuiltinKey::Hr))
        .expect("HR");
    f.manager
        .on_required_ask_opened(hr.id.as_str(), entry(&f.owner));
    assert!(f
        .manager
        .open_required_asks
        .lock()
        .unwrap()
        .contains_key(hr.id.as_str()));

    // Her own process idles in the owner's partition; the answer is sent there, warm.
    let rx = f.install(SessionState::Idle, Some(f.owner_key()), Some("conv-owner"));
    let outcome = f.manager.on_ask_answered(
        &f.tess,
        &ask,
        "Which day works?",
        ANSWER,
        &f.owner,
        "Owner",
        true,
        false,
        false,
    );
    assert_eq!(outcome, crate::wake::WakeOutcome::Started);
    let cmds = drained(&rx);
    assert!(
        stop_reasons(&cmds).is_empty(),
        "never switched to tess:wake"
    );
    assert!(cmds
        .iter()
        .any(|c| matches!(c, PumpCommand::SendTurn(text) if text.contains(ANSWER))));
    assert!(f.queued().is_empty());
    assert!(f
        .manager
        .partitions
        .state(&f.tess, &PartitionKey::tess_wake())
        .is_none_or(|s| s.turns == 0));
}

/// Each partition's hold is weighed on its own wakes (`TASK-80.1`, pass 4): with her hourly
/// quota spent, an agent's DM partition never borrows the owner's person-triggered bypass, and
/// the held agent partition never holds the owner's DM behind it.
#[test]
fn a_held_partition_never_borrows_or_blocks_another_partitions_hold() {
    let f = Fixture::new();
    let window = f.manager.config.wake_window.as_millis() as u64;
    let mut quota = crate::wake::WakeQuotaRecord::empty(&f.tess);
    for _ in 0..f.manager.config.hourly_wake_limit {
        quota.record_start(now_ms(), window);
    }
    f.manager.store.write_wake_quota(&quota).unwrap();
    let mut queue = PendingWakeQueueRecord::empty(&f.tess);
    queue.push_dedup(wake_item(
        "w-agent",
        WakeKind::DirectMessage,
        "agent-ada",
        false,
        "ADA: status?",
    ));
    queue.push_dedup(wake_item(
        "w-owner",
        WakeKind::DirectMessage,
        &f.owner,
        true,
        "OWNER: plan?",
    ));
    f.manager.store.write_pending_wakes(&queue).unwrap();

    let turns = turns_sent_to(&f, f.owner_key());
    assert_eq!(turns.len(), 1);
    assert!(turns[0].contains("OWNER: plan?") && !turns[0].contains("ADA: status?"));
    assert_eq!(f.queued(), ["w-agent"]);

    f.install(SessionState::Idle, Some(f.owner_key()), Some("conv-live"));
    assert_eq!(
        f.manager.process_due_wakes_for(&f.tess),
        crate::wake::WakeOutcome::Queued {
            reason: crate::wake::WakeHoldReason::HourlyLimit
        }
    );
    assert_eq!(f.queued(), ["w-agent"]);
}
