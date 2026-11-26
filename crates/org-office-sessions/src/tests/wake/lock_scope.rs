//! `TASK-170`: lifecycle locks are per employee and are never held across a warm-permit
//! acquisition (an HQ permit RPC on satellites), so one agent blocked in a slow permit RPC never
//! stalls another agent's wakes, and wakes that arrive meanwhile are kept.

use std::sync::mpsc;

use super::*;
use crate::manager::LiveSessionHandle;
use crate::pump::{PumpCommand, SessionRuntimeState};
use crate::router::WarmPermitOutcome;
use crate::wake::PendingWakeQueueRecord;

/// Every operation for employee B (and a new wake for A) must finish within this budget while A
/// sits inside its permit RPC.
const UNBLOCKED_BUDGET: Duration = Duration::from_millis(50);

/// A live handle whose pump commands are captured instead of run.
fn stub_live_handle(
    office_id: &str,
    emp_id: &str,
    state: SessionState,
    connection_id: Option<&str>,
) -> (LiveSessionHandle, mpsc::Receiver<PumpCommand>) {
    let (tx, rx) = mpsc::channel();
    let runtime_state = Arc::new(Mutex::new(SessionRuntimeState {
        employee_id: emp_id.into(),
        office_id: office_id.into(),
        state,
        run_id: Some(format!("run-{emp_id}")),
        session_id: Some(format!("sess-{emp_id}")),
        runtime_id: Some("claude".into()),
        runtime_session_id: None,
        private_home: format!("/tmp/home-{emp_id}"),
        permission_preset: "workspace_edits".into(),
        sandbox_enabled: true,
        spec_hash: "h".into(),
        watchdog: crate::recovery::StallWatchdog::new(Duration::from_secs(60), Instant::now()),
        recovery_required: false,
        last_reply: None,
        last_error: None,
        backgrounds: crate::subagents::BackgroundRegistry::default(),
        pending_approvals: Vec::new(),
        policy: org_agent_runtime::PermissionPolicy {
            version: 1,
            allow: Vec::new(),
            deny: Vec::new(),
        },
        preset: org_office::organization_family::records::PermissionPreset::WorkspaceEdits,
        allowed_tools: None,
        writable_roots: Vec::new(),
        suspended_on_asks: Vec::new(),
        waiting_for_member_id: None,
        waiting_for_name: None,
        suspended_since: None,
        pending_wake_count: 0,
        pending_wake_reason: None,
    }));
    let handle = LiveSessionHandle {
        state: runtime_state,
        tx,
        thread: None,
        connection_id: connection_id.map(str::to_owned),
        route_digest: String::new(),
        oauth_expires_at_ms: None,
        workspace: None,
        active_permit: Arc::new(Mutex::new(None)),
        supports_mid_turn: false,
        bwrap_masked_siblings: None,
    };
    (handle, rx)
}

fn person_wake(id: &str, sender: &str, summary: &str) -> PendingWakeItem {
    PendingWakeItem {
        item_id: id.into(),
        kind: WakeKind::DirectMessage,
        sender_id: sender.into(),
        sender_name: "Owner".into(),
        sender_handle: None,
        sender_relation: None,
        sender_is_person: true,
        conversation_id: Some("dm-1".into()),
        source_id: format!("msg-{id}"),
        summary: summary.into(),
        enqueued_at_ms: now_ms(),
    }
}

/// Runs `op`, asserting it finishes within [`UNBLOCKED_BUDGET`], and returns its result.
fn within_budget<T>(label: &str, op: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let out = op();
    let took = started.elapsed();
    assert!(
        took < UNBLOCKED_BUDGET,
        "{label} took {took:?} while another employee was blocked in its permit RPC"
    );
    out
}

#[test]
fn task170_wake_for_b_completes_while_a_is_blocked_in_hq_permit_rpc_and_a_keeps_new_wakes() {
    let fx = WakeFixture::new(LOOP_ECHO_SCRIPT);
    let mgr = Arc::new(fx.make_manager(50, Duration::from_secs(3600), Duration::from_secs(30)));
    SessionManager::attach_self(&mgr);
    let office_id = fx.office.info().office_id.clone();
    let (a, b) = (fx.agent_a.clone(), fx.agent_b.clone());

    // A: idle warm session on a pooled connection with one queued wake to drain at turn end.
    // B: busy live session, so its wakes queue without spawning anything.
    let (a_handle, a_commands) = stub_live_handle(&office_id, &a, SessionState::Idle, Some("c-a"));
    let a_permit = Arc::clone(&a_handle.active_permit);
    let (b_handle, _b_commands) = stub_live_handle(&office_id, &b, SessionState::Running, None);
    {
        let mut reg = mgr.registry.lock().unwrap();
        reg.live.insert(a.clone(), a_handle);
        reg.live.insert(b.clone(), b_handle);
    }
    let mut queue = PendingWakeQueueRecord::empty(&a);
    queue.push_dedup(person_wake("a-1", &fx.owner_id, "First queued for A"));
    mgr.store.write_pending_wakes(&queue).unwrap();

    // Warm-up outside the blocked window so the timed calls measure lock waits, not cold I/O.
    assert_eq!(
        mgr.enqueue_wake(&b, person_wake("b-0", &fx.owner_id, "warm-up for B")),
        WakeOutcome::Queued {
            reason: WakeHoldReason::Busy
        }
    );

    // The stand-in HQ permit RPC reports how many employee locks are held when it is entered,
    // then blocks until the test releases it.
    let (entered_tx, entered_rx) = mpsc::channel::<(usize, bool)>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let release_rx = Mutex::new(release_rx);
    let locks = Arc::clone(&mgr.employee_locks);
    let registry = Arc::clone(&mgr.registry);
    let acquirer: crate::lifecycle::WarmPermitAcquirer =
        Arc::new(move |_office, _hq, conn, emp, _task, _digest| {
            assert_eq!(conn, "c-a");
            let registry_free = registry.try_lock().is_ok();
            entered_tx
                .send((locks.held_count(), registry_free))
                .unwrap();
            let _ = emp;
            release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(30))
                .expect("test releases the permit RPC");
            WarmPermitOutcome::Granted(("permit-a".into(), 7))
        });
    let drain_mgr = Arc::clone(&mgr);
    let drain_emp = a.clone();
    let drain = std::thread::spawn(move || {
        crate::lifecycle::drain_into_live_pump_with_acquirer(&drain_mgr, &drain_emp, acquirer);
    });

    let (held_in_rpc, registry_free_in_rpc) = entered_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("A's drain reaches its permit RPC");
    assert_eq!(
        held_in_rpc, 0,
        "no employee lock is held across the permit RPC"
    );
    assert!(
        registry_free_in_rpc,
        "the session registry lock is free too"
    );

    // A is now parked inside its permit RPC. B's wake path and lock-taking hooks are unaffected.
    let b_outcome = within_budget("enqueue_wake(B)", || {
        mgr.enqueue_wake(&b, person_wake("b-1", &fx.owner_id, "Hello B"))
    });
    assert_eq!(
        b_outcome,
        WakeOutcome::Queued {
            reason: WakeHoldReason::Busy
        }
    );
    within_budget("on_required_ask_opened(B)", || {
        mgr.on_required_ask_opened(
            &b,
            SuspendedAskEntry {
                ask_id: "ask-b".into(),
                conversation_id: "dm-b".into(),
                question_summary: "Q?".into(),
                waiting_for_member_id: None,
                waiting_for_name: None,
                opened_at_ms: now_ms(),
                asked_by_subagent: false,
            },
        );
    });
    // Withdrawal sweeps every employee's queue, A's included: each lock is taken in turn.
    within_budget("on_ask_withdrawn_durable(B)", || {
        mgr.on_ask_withdrawn_durable(&b, "ask-b").unwrap();
    });
    // A new wake for A itself is persisted at once; the in-flight drain keeps the dispatch slot.
    let a_outcome = within_budget("enqueue_wake(A)", || {
        mgr.enqueue_wake(&a, person_wake("a-2", &fx.owner_id, "Second for A"))
    });
    assert_eq!(
        a_outcome,
        WakeOutcome::Queued {
            reason: WakeHoldReason::Busy
        }
    );
    assert!(
        a_commands.try_recv().is_err(),
        "A's turn is not sent before its permit is granted"
    );

    release_tx.send(()).unwrap();
    drain.join().unwrap();

    // A's turn carries only the wake it claimed; the wake that arrived during the RPC is kept.
    let sent = a_commands
        .recv_timeout(Duration::from_secs(5))
        .expect("A's drained turn is sent");
    let PumpCommand::SendTurn(objective) = sent else {
        panic!("expected SendTurn for A");
    };
    assert!(objective.contains("First queued for A"), "{objective}");
    assert!(!objective.contains("Second for A"), "{objective}");
    let remaining = mgr.store.read_pending_wakes(&a).unwrap().unwrap();
    let ids: Vec<&str> = remaining.items.iter().map(|i| i.item_id.as_str()).collect();
    assert_eq!(
        ids,
        ["a-2"],
        "claimed items removed, concurrent arrivals kept"
    );
    assert_eq!(
        a_permit.lock().unwrap().as_ref().map(|(id, _)| id.as_str()),
        Some("permit-a")
    );
    let b_queue = mgr.store.read_pending_wakes(&b).unwrap().unwrap();
    assert!(b_queue.items.iter().any(|i| i.item_id == "b-1"));
    assert_eq!(mgr.employee_locks.held_count(), 0);
}
