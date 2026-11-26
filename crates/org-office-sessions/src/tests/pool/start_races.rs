//! Start-path lock scope and stop-versus-start races (`TASK-190`): watch-notice I/O never runs
//! under the `registry` or a handle's state lock, and an offboard that arrives while a start or
//! a pool failover relaunch holds the start slot leaves no live session and no queued
//! recovery wake.

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
