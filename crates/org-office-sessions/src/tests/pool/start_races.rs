//! Start-path lock scope and stop-versus-start races (`TASK-190`): watch-notice I/O never runs
//! under the `registry` or a handle's state lock, and an offboard or pause that arrives while a
//! cold start, a warm send or a pool failover relaunch holds the start slot leaves no live
//! session, sends no turn and queues no recovery wake, while a paused wake stays durable. A
//! `stop_all`/`shutdown` drain that overtakes a warm send refuses it the same way, with
//! `Unavailable`.

use std::sync::mpsc;

use super::*;

const REPLY_SCRIPT: &str = r#"#!/bin/sh
echo '{"kind":"ready"}'
while IFS= read -r line; do
  echo '{"kind":"reply","text":"ok"}'
done
"#;

/// A watch-notice provider that, for `employee_id`, signals `entered` and then blocks until
/// the test releases it (or a safety timeout passes).
struct NoticeGate {
    entered: mpsc::Receiver<()>,
    release: mpsc::Sender<()>,
}

fn gate_watch_notices(mgr: &SessionManager, employee_id: &str) -> NoticeGate {
    let (entered_tx, entered) = mpsc::channel();
    let (release, release_rx) = mpsc::channel::<()>();
    let entered_tx = Mutex::new(entered_tx);
    let release_rx = Mutex::new(release_rx);
    let gated = employee_id.to_owned();
    mgr.set_watch_notice_provider(Arc::new(move |emp: &str| {
        if emp != gated {
            return Vec::new();
        }
        let _ = entered_tx.lock().unwrap().send(());
        let _ = release_rx
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(20));
        vec!["[watch] channel watch ended".to_owned()]
    }));
    NoticeGate { entered, release }
}

fn pool_agent(office: &Office, owner: &str, handle: &str) -> String {
    let conn = create_connection_and_bind(
        office,
        owner,
        &format!("conn-{handle}"),
        None,
        4,
        &[("claude-sonnet-4", "claude-sonnet-4")],
    );
    let profile_id = next_id();
    office
        .model_profile_create(
            owner,
            ModelProfileCreateParams {
                command_id: next_id(),
                profile_id: profile_id.clone(),
                key: format!("pool-{handle}"),
                name: format!("Pool {handle}"),
                description: "start race pool".into(),
                connection_ids: vec![conn],
            },
        )
        .unwrap()
        .unwrap();
    hire_agent(office, owner, handle, &profile_id, "claude-sonnet-4")
}

fn start_params(agent: &str, instruction: &str) -> MemberSessionStartParams {
    MemberSessionStartParams {
        command_id: next_id(),
        employee_id: agent.to_owned(),
        instruction: Some(instruction.to_owned()),
        workspace: None,
    }
}

/// Runs a first turn to completion so the agent's next start takes the warm path, and returns
/// the number of turns the child has received.
fn warm_up(mgr: &SessionManager, owner: &str, agent: &str, recorded: &PoolRecorded) -> usize {
    mgr.start_session(owner, &start_params(agent, "first turn"))
        .unwrap();
    assert!(
        wait_until(Duration::from_secs(8), || {
            let idle = mgr.inspect_session(owner, Some(agent)).is_ok_and(|r| {
                r.session.state == SessionState::Idle && r.session.last_reply.is_some()
            });
            if !idle {
                return false;
            }
            let reg = mgr.registry.lock().unwrap();
            let permit_released = reg
                .live
                .get(agent)
                .is_some_and(|h| h.active_permit.lock().unwrap().is_none());
            let bg_quiet = reg.bg_spawning == 0
                && reg
                    .bg_workers
                    .iter()
                    .all(std::thread::JoinHandle::is_finished);
            permit_released && bg_quiet
        }),
        "the first turn must finish and release its permit so the next start takes the warm path"
    );
    recorded.turns.lock().unwrap().len()
}

fn assert_no_live_session(mgr: &SessionManager, agent: &str) {
    let reg = mgr.registry.lock().unwrap();
    assert!(!reg.live.contains_key(agent), "zero live sessions");
    assert!(!reg.starting.contains(agent), "the start slot is released");
}

fn queued_recovery_wakes(mgr: &SessionManager, agent: &str) -> usize {
    mgr.store()
        .read_pending_wakes(agent)
        .unwrap()
        .map_or(0, |queue| {
            queue
                .items
                .iter()
                .filter(|item| item.kind == WakeKind::Recovery)
                .count()
        })
}

#[test]
fn watch_notice_io_on_the_warm_path_never_blocks_other_session_operations() {
    let (_tmp, office, mgr, recorded, owner) = setup_office_and_manager(REPLY_SCRIPT);
    let agent = pool_agent(&office, &owner, "warm-ada");
    let sibling = pool_agent(&office, &owner, "warm-bob");
    mgr.start_session(&owner, &start_params(&agent, "first turn"))
        .unwrap();
    assert!(
        wait_until(Duration::from_secs(8), || {
            mgr.inspect_session(&owner, Some(&agent)).is_ok_and(|r| {
                r.session.state == SessionState::Idle && r.session.last_reply.is_some()
            })
        }),
        "the first turn must finish so the next start takes the warm path"
    );

    let warm_run = mgr
        .inspect_session(&owner, Some(&agent))
        .unwrap()
        .session
        .run_id;
    assert!(warm_run.is_some());
    let gate = gate_watch_notices(&mgr, &agent);
    let starter = {
        let mgr = Arc::clone(&mgr);
        let owner = owner.clone();
        let agent = agent.clone();
        std::thread::spawn(move || mgr.start_session(&owner, &start_params(&agent, "second turn")))
    };
    gate.entered
        .recv_timeout(Duration::from_secs(10))
        .expect("the warm send asks for watch notices");

    // While the warm send waits on the watch-notice callback, registry readers and a sibling's
    // operations complete: no lock is held across that callback.
    let (done_tx, done_rx) = mpsc::channel();
    let probe = {
        let mgr = Arc::clone(&mgr);
        let owner = owner.clone();
        let agent = agent.clone();
        let sibling = sibling.clone();
        std::thread::spawn(move || {
            let own = mgr.inspect_session(&owner, Some(&agent)).map(|_| ());
            let other = mgr.inspect_session(&owner, Some(&sibling)).map(|_| ());
            let listed = mgr
                .list_sessions(
                    &owner,
                    &org_proto::sessions::MemberSessionListParams {
                        employee_id: Some(agent.clone()),
                        limit: None,
                    },
                )
                .map(|_| ());
            let stopped = mgr.stop_session(&sibling, "sibling_stop").map(|_| ());
            let _ = done_tx.send((own, other, listed, stopped));
        })
    };
    let (own, other, listed, stopped) = done_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("session operations must not block on the watch-notice callback");
    own.unwrap();
    other.unwrap();
    listed.unwrap();
    stopped.unwrap();
    probe.join().unwrap();

    gate.release.send(()).unwrap();
    let delivered = starter.join().unwrap().expect("the warm turn is delivered");
    assert_eq!(
        delivered.run_id, warm_run,
        "the turn went to the warm handle"
    );
    assert!(
        wait_until(Duration::from_secs(8), || recorded
            .turns
            .lock()
            .unwrap()
            .iter()
            .any(|t| t.starts_with("[watch] channel watch ended"))),
        "the drained notices still lead the delivered turn: {:?}",
        recorded.turns.lock().unwrap()
    );
    mgr.shutdown("test_end");
}

#[test]
fn offboard_during_a_cold_start_leaves_no_live_session_and_no_recovery_wake() {
    let (_tmp, office, mgr, _recorded, owner) = setup_office_and_manager(REPLY_SCRIPT);
    let agent = pool_agent(&office, &owner, "cold-cyd");
    let gate = gate_watch_notices(&mgr, &agent);
    let epoch = mgr.registry.lock().unwrap().stop_epoch(&agent);
    let starter = {
        let mgr = Arc::clone(&mgr);
        let owner = owner.clone();
        let agent = agent.clone();
        std::thread::spawn(move || mgr.start_session(&owner, &start_params(&agent, "go")))
    };
    gate.entered
        .recv_timeout(Duration::from_secs(10))
        .expect("the cold start reaches its watch notices");
    assert!(mgr.registry.lock().unwrap().starting.contains(&agent));

    mgr.on_agent_offboarded(&agent);
    assert_eq!(
        mgr.registry.lock().unwrap().stop_epoch(&agent),
        epoch + 1,
        "the refused stop still moves the epoch"
    );
    gate.release.send(()).unwrap();

    let err = starter
        .join()
        .unwrap()
        .expect_err("the start must abort at registration");
    assert_eq!(err.code, org_proto::ErrorCode::Conflict);
    assert_eq!(
        err.message,
        "agent session was stopped while it was starting"
    );
    let reg = mgr.registry.lock().unwrap();
    assert!(!reg.live.contains_key(&agent), "zero live sessions");
    assert!(!reg.starting.contains(&agent), "the start slot is released");
    drop(reg);
    assert_eq!(queued_recovery_wakes(&mgr, &agent), 0);
    assert!(
        mgr.bindings().active_tokens().is_empty(),
        "the run binding is revoked"
    );
    mgr.shutdown("test_end");
}

#[test]
fn offboard_during_a_failover_relaunch_leaves_no_live_session_and_no_recovery_wake() {
    let (_tmp, office, mgr, _recorded, owner) = setup_office_and_manager(REPLY_SCRIPT);
    let agent = pool_agent(&office, &owner, "fail-dee");
    let gate = gate_watch_notices(&mgr, &agent);
    let epoch = mgr.registry.lock().unwrap().stop_epoch(&agent);
    let relaunch = {
        let mgr = Arc::clone(&mgr);
        let agent = agent.clone();
        std::thread::spawn(move || {
            crate::manager::run_failover_relaunch(
                &mgr,
                crate::pump::FailoverRequest {
                    employee_id: agent,
                    failed_run_id: "run-failed".into(),
                    failed_connection_id: "conn-failed".into(),
                    objective: "keep-this-turn".into(),
                    workspace: None,
                    task_ref: None,
                    tried_connections: vec!["conn-failed".into()],
                    oauth_refresh_attempted: false,
                    oauth_token_fp: None,
                    reason: "rate_limited",
                    rate_limit_resets_at: None,
                    stop_epoch: epoch,
                    partition: None,
                },
            );
        })
    };
    gate.entered
        .recv_timeout(Duration::from_secs(10))
        .expect("the relaunch reaches its watch notices holding the start slot");
    assert!(mgr.registry.lock().unwrap().starting.contains(&agent));

    mgr.on_agent_offboarded(&agent);
    gate.release.send(()).unwrap();
    relaunch.join().unwrap();

    let reg = mgr.registry.lock().unwrap();
    assert!(!reg.live.contains_key(&agent), "zero live sessions");
    assert!(!reg.starting.contains(&agent), "the start slot is released");
    drop(reg);
    assert_eq!(
        queued_recovery_wakes(&mgr, &agent),
        0,
        "an abandoned failover never requeues its turn"
    );
    assert!(mgr.bindings().active_tokens().is_empty());
    mgr.shutdown("test_end");
}

#[test]
fn offboard_during_a_warm_send_sends_no_turn_and_leaves_no_live_session() {
    let (_tmp, office, mgr, recorded, owner) = setup_office_and_manager(REPLY_SCRIPT);
    let agent = pool_agent(&office, &owner, "warm-eve");
    let turns_before = warm_up(&mgr, &owner, &agent, &recorded);
    let gate = gate_watch_notices(&mgr, &agent);
    let epoch = mgr.registry.lock().unwrap().stop_epoch(&agent);
    let starter = {
        let mgr = Arc::clone(&mgr);
        let owner = owner.clone();
        let agent = agent.clone();
        std::thread::spawn(move || mgr.start_session(&owner, &start_params(&agent, "second turn")))
    };
    gate.entered
        .recv_timeout(Duration::from_secs(10))
        .expect("the warm send reads its watch notices holding the start slot");
    assert!(mgr.registry.lock().unwrap().starting.contains(&agent));
    assert!(mgr.registry.lock().unwrap().live.contains_key(&agent));

    mgr.on_agent_offboarded(&agent);
    assert_eq!(mgr.registry.lock().unwrap().stop_epoch(&agent), epoch + 1);
    gate.release.send(()).unwrap();

    let err = starter
        .join()
        .unwrap()
        .expect_err("a warm send overtaken by an offboard is refused");
    assert_eq!(err.code, org_proto::ErrorCode::Conflict);
    assert_eq!(
        err.message,
        "agent session was stopped while it was starting"
    );
    // The reap joined the pump thread, so any turn sent to the child is already recorded.
    assert_eq!(
        recorded.turns.lock().unwrap().len(),
        turns_before,
        "no turn reaches the child: {:?}",
        recorded.turns.lock().unwrap()
    );
    assert_no_live_session(&mgr, &agent);
    assert!(mgr.bindings().active_tokens().is_empty());
    mgr.shutdown("test_end");
}

#[test]
fn pause_during_a_warm_wake_send_keeps_the_durable_wakes_and_charges_no_quota() {
    let (_tmp, office, mgr, recorded, owner) = setup_office_and_manager(REPLY_SCRIPT);
    let agent = pool_agent(&office, &owner, "warm-fay");
    let peer = pool_agent(&office, &owner, "warm-gus");
    let turns_before = warm_up(&mgr, &owner, &agent, &recorded);
    let gate = gate_watch_notices(&mgr, &agent);
    let waker = {
        let mgr = Arc::clone(&mgr);
        let agent = agent.clone();
        std::thread::spawn(move || {
            mgr.enqueue_wake(
                &agent,
                PendingWakeItem {
                    item_id: "wake-paused-1".into(),
                    kind: WakeKind::Handoff,
                    sender_id: peer,
                    sender_name: "Gus".into(),
                    sender_handle: None,
                    sender_relation: None,
                    sender_is_person: false,
                    conversation_id: Some("c-pause".into()),
                    source_id: "msg-pause-1".into(),
                    summary: "Peer handoff during pause".into(),
                    enqueued_at_ms: now_ms(),
                },
            )
        })
    };
    gate.entered
        .recv_timeout(Duration::from_secs(10))
        .expect("the autonomous wake takes the warm send path");
    assert!(mgr.registry.lock().unwrap().starting.contains(&agent));

    let refused = mgr
        .stop_session(&agent, "paused")
        .expect_err("a pause during a warm send is refused but still moves the epoch");
    assert_eq!(refused.code, org_proto::ErrorCode::Conflict);
    gate.release.send(()).unwrap();

    assert_eq!(
        waker.join().unwrap(),
        WakeOutcome::Queued {
            reason: WakeHoldReason::AdmissionRefused
        },
        "the paused wake is not reported as started"
    );
    assert_eq!(
        recorded.turns.lock().unwrap().len(),
        turns_before,
        "no turn reaches the child: {:?}",
        recorded.turns.lock().unwrap()
    );
    assert_no_live_session(&mgr, &agent);
    let queue = mgr
        .store()
        .read_pending_wakes(&agent)
        .unwrap()
        .expect("the claimed wakes stay durable");
    assert!(
        queue
            .items
            .iter()
            .any(|item| item.item_id == "wake-paused-1"),
        "the claimed wake is preserved for resume: {:?}",
        queue.items
    );
    let quota = mgr.store().read_wake_quota(&agent).unwrap();
    assert!(
        quota.is_none_or(|q| q.agent_wake_timestamps_ms.is_empty()),
        "a refused wake is never charged to the wake quota"
    );
    mgr.shutdown("test_end");
}

/// Which drain overtakes a warm send.
#[derive(Clone, Copy, Debug)]
enum Drain {
    StopAll,
    Shutdown,
}

/// Starts `drain` while `send` waits in its warm path's watch notices holding the start slot,
/// lets `send` finish once the drain has closed admissions, and joins both.
fn drain_during_warm_send<T: Send + 'static>(
    mgr: &Arc<SessionManager>,
    agent: &str,
    drain: Drain,
    send: impl FnOnce(&SessionManager) -> T + Send + 'static,
) -> T {
    let gate = gate_watch_notices(mgr, agent);
    let sender = {
        let mgr = Arc::clone(mgr);
        std::thread::spawn(move || send(&mgr))
    };
    gate.entered
        .recv_timeout(Duration::from_secs(10))
        .expect("the warm send reads its watch notices holding the start slot");
    assert!(mgr.registry.lock().unwrap().starting.contains(agent));
    let drainer = {
        let mgr = Arc::clone(mgr);
        std::thread::spawn(move || match drain {
            Drain::StopAll => mgr.stop_all("drain_race"),
            Drain::Shutdown => mgr.shutdown("drain_race"),
        })
    };
    assert!(
        wait_until(Duration::from_secs(5), || mgr
            .registry
            .lock()
            .unwrap()
            .draining),
        "{drain:?} closes admissions while the warm send holds the start slot"
    );
    // The drain cannot reap a handle whose start slot is held: the warm send decides first.
    assert!(mgr.registry.lock().unwrap().live.contains_key(agent));
    gate.release.send(()).unwrap();
    let out = sender.join().unwrap();
    drainer.join().unwrap();
    out
}

#[test]
fn a_drain_during_a_warm_wake_send_sends_no_turn_and_keeps_the_durable_wake() {
    for drain in [Drain::StopAll, Drain::Shutdown] {
        let (_tmp, office, mgr, recorded, owner) = setup_office_and_manager(REPLY_SCRIPT);
        let agent = pool_agent(&office, &owner, "drain-hal");
        let peer = pool_agent(&office, &owner, "drain-ivy");
        let turns_before = warm_up(&mgr, &owner, &agent, &recorded);
        let wake_agent = agent.clone();
        let outcome = drain_during_warm_send(&mgr, &agent, drain, move |mgr| {
            mgr.enqueue_wake(
                &wake_agent,
                PendingWakeItem {
                    item_id: "wake-drain-1".into(),
                    kind: WakeKind::Handoff,
                    sender_id: peer,
                    sender_name: "Ivy".into(),
                    sender_handle: None,
                    sender_relation: None,
                    sender_is_person: false,
                    conversation_id: Some("c-drain".into()),
                    source_id: "msg-drain-1".into(),
                    summary: "Peer handoff during a drain".into(),
                    enqueued_at_ms: now_ms(),
                },
            )
        });
        assert_eq!(
            outcome,
            WakeOutcome::Queued {
                reason: WakeHoldReason::AdmissionRefused
            },
            "{drain:?}: the drained wake is not reported as started"
        );
        // The drain reaped the handle, joining its pump, so any turn sent is already recorded.
        assert_eq!(
            recorded.turns.lock().unwrap().len(),
            turns_before,
            "{drain:?}: no turn reaches the child: {:?}",
            recorded.turns.lock().unwrap()
        );
        assert_no_live_session(&mgr, &agent);
        let queue = mgr
            .store()
            .read_pending_wakes(&agent)
            .unwrap()
            .expect("the claimed wake stays durable");
        assert!(
            queue
                .items
                .iter()
                .any(|item| item.item_id == "wake-drain-1"),
            "{drain:?}: the claimed wake is preserved for the next start: {:?}",
            queue.items
        );
        let quota = mgr.store().read_wake_quota(&agent).unwrap();
        assert!(
            quota.is_none_or(|q| q.agent_wake_timestamps_ms.is_empty()),
            "{drain:?}: a refused wake is never charged to the wake quota"
        );
        mgr.shutdown("test_end");
    }
}

#[test]
fn a_drain_during_an_autonomous_warm_send_refuses_it_with_unavailable() {
    for drain in [Drain::StopAll, Drain::Shutdown] {
        let (_tmp, office, mgr, recorded, owner) = setup_office_and_manager(REPLY_SCRIPT);
        let agent = pool_agent(&office, &owner, "drain-jon");
        let turns_before = warm_up(&mgr, &owner, &agent, &recorded);
        let employee = office
            .list_employees()
            .unwrap()
            .into_iter()
            .find(|e| e.id.as_str() == agent)
            .expect("the hired agent");
        let err = drain_during_warm_send(&mgr, &agent, drain, move |mgr| {
            mgr.spawn_or_send_autonomous(
                &employee,
                "autonomous turn".into(),
                None,
                None,
                serde_json::json!({ "type": "turn_trigger" }),
            )
        })
        .expect_err("a warm send overtaken by a drain is refused");
        assert_eq!(err.code, org_proto::ErrorCode::Unavailable, "{drain:?}");
        assert_eq!(err.message, "office session manager is shutting down");
        assert_eq!(
            recorded.turns.lock().unwrap().len(),
            turns_before,
            "{drain:?}: no turn reaches the child: {:?}",
            recorded.turns.lock().unwrap()
        );
        assert_no_live_session(&mgr, &agent);
        mgr.shutdown("test_end");
    }
}
