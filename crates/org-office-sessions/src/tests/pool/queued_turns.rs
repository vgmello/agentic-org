//! A turn queued on a live pump keeps its session busy until the turn ends, and never wedges it
//! busy (`TASK-80.1`): a warm send is counted and marked busy BEFORE it reaches the pump, so a
//! pump that finishes the turn at once, or refuses it for want of a turn permit, always leaves
//! the session idle, and a turn queued behind a running one keeps the session busy across the
//! first turn's end. Every test here drives the real pump over a real child process.

use std::sync::atomic::{AtomicBool, Ordering};

use super::start_races::{pool_agent, start_params, warm_up};
use super::*;

const REPLY_SCRIPT: &str = r#"#!/bin/sh
echo '{"kind":"ready"}'
while IFS= read -r line; do
  echo '{"kind":"reply","text":"ok"}'
done
"#;

/// Replies at once, except to a turn naming `HOLD_UNTIL_GATE`, which waits for `gate`.
fn gated_script(gate: &std::path::Path) -> String {
    format!(
        r#"#!/bin/sh
echo '{{"kind":"ready"}}'
while IFS= read -r line; do
  case "$line" in
    *HOLD_UNTIL_GATE*)
      while [ ! -f "{gate}" ]; do sleep 0.05; done
      echo '{{"kind":"reply","text":"held"}}'
      ;;
    *)
      echo '{{"kind":"reply","text":"ok"}}'
      ;;
  esac
done
"#,
        gate = gate.display()
    )
}

/// The live session's state and queued-turn count.
fn live_state(mgr: &SessionManager, agent: &str) -> Option<(SessionState, usize)> {
    let reg = mgr.registry.lock().unwrap();
    reg.live.get(agent).map(|handle| {
        let st = handle.state.lock().unwrap();
        (st.state, st.queued_turns)
    })
}

fn settles_idle(mgr: &SessionManager, agent: &str) -> bool {
    wait_until(Duration::from_secs(8), || {
        live_state(mgr, agent) == Some((SessionState::Idle, 0))
    })
}

#[test]
fn warm_sends_that_finish_at_once_never_leave_the_session_busy() {
    let (_tmp, office, mgr, recorded, owner) = setup_office_and_manager(REPLY_SCRIPT);
    let agent = pool_agent(&office, &owner, "fast-ada");
    let warm_turns = warm_up(&mgr, &owner, &agent, &recorded);
    for i in 0..20 {
        mgr.start_session(&owner, &start_params(&agent, &format!("fast turn {i}")))
            .unwrap();
        let sent = warm_turns + i + 1;
        assert!(
            wait_until(Duration::from_secs(8), || {
                recorded.turns.lock().unwrap().len() >= sent
                    && live_state(&mgr, &agent) == Some((SessionState::Idle, 0))
            }),
            "fast warm turn {i} must leave the session idle: {:?}",
            live_state(&mgr, &agent)
        );
    }
    // Nothing marks the session busy after the pump finished its last turn.
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(live_state(&mgr, &agent), Some((SessionState::Idle, 0)));
}

/// Two agents share a single-slot pool; returns `(holder, waiter)`.
fn single_slot_pair(office: &Office, owner: &str) -> (String, String) {
    let conn = create_connection_and_bind(
        office,
        owner,
        "conn-single",
        None,
        1,
        &[("claude-sonnet-4", "claude-sonnet-4")],
    );
    let profile_id = next_id();
    office
        .model_profile_create(
            owner,
            ModelProfileCreateParams {
                command_id: next_id(),
                profile_id: profile_id.clone(),
                key: "single-slot".into(),
                name: "Single Slot".into(),
                description: "one concurrent turn".into(),
                connection_ids: vec![conn],
            },
        )
        .unwrap()
        .unwrap();
    // Both are hired before either starts, so neither sandbox's sibling set goes stale.
    let holder = hire_agent(office, owner, "holder", &profile_id, "claude-sonnet-4");
    let waiter = hire_agent(office, owner, "waiter", &profile_id, "claude-sonnet-4");
    (holder, waiter)
}

#[test]
fn a_turn_the_pump_refuses_for_want_of_a_permit_leaves_the_session_idle() {
    let tmp_gate = tempfile::tempdir().unwrap();
    let gate = tmp_gate.path().join("gate");
    let (_tmp, office, mgr, recorded, owner) = setup_office_and_manager(&gated_script(&gate));
    let (holder, waiter) = single_slot_pair(&office, &owner);
    let warm_turns = warm_up(&mgr, &owner, &waiter, &recorded);

    // The holder takes the pool's only turn permit and keeps it.
    mgr.start_session(&owner, &start_params(&holder, "HOLD_UNTIL_GATE"))
        .unwrap();
    assert!(wait_until(Duration::from_secs(8), || {
        recorded.turns.lock().unwrap().len() > warm_turns
            && live_state(&mgr, &holder).is_some_and(|(state, _)| state == SessionState::Running)
    }));

    // A turn queued with no permit to hand over (as a wake or a follow-up is): the waiter's
    // pump must acquire its own, is refused, and the session goes back to idle every time.
    for i in 0..10 {
        let text = format!("refused turn {i}");
        {
            let reg = mgr.registry.lock().unwrap();
            let handle = reg.live.get(&waiter).expect("the waiter is live");
            assert!(crate::pump::queue_turn(
                &handle.state,
                &handle.tx,
                text.clone()
            ));
        }
        assert!(
            settles_idle(&mgr, &waiter),
            "refused turn {i} must leave the session idle: {:?}",
            live_state(&mgr, &waiter)
        );
        let last_error = mgr
            .inspect_session(&owner, Some(&waiter))
            .unwrap()
            .session
            .last_error
            .unwrap_or_default();
        assert!(last_error.contains("POOL_EXHAUSTED"), "{last_error}");
        assert!(
            !recorded.turns.lock().unwrap().contains(&text),
            "a refused turn never reaches the child"
        );
    }

    // Once the holder lets go, the waiter's next warm turn runs and ends idle.
    fs::write(&gate, "go").unwrap();
    assert!(settles_idle(&mgr, &holder));
    let before = recorded.turns.lock().unwrap().len();
    mgr.start_session(&owner, &start_params(&waiter, "after the holder"))
        .unwrap();
    assert!(wait_until(Duration::from_secs(8), || {
        recorded.turns.lock().unwrap().len() > before
            && live_state(&mgr, &waiter) == Some((SessionState::Idle, 0))
    }));
}

#[test]
fn a_turn_queued_behind_a_running_one_keeps_the_session_busy_until_it_ends() {
    let tmp_gate = tempfile::tempdir().unwrap();
    let gate = tmp_gate.path().join("gate");
    let (_tmp, office, mgr, recorded, owner) = setup_office_and_manager(&gated_script(&gate));
    let agent = pool_agent(&office, &owner, "queue-ada");
    let warm_turns = warm_up(&mgr, &owner, &agent, &recorded);

    mgr.start_session(&owner, &start_params(&agent, "HOLD_UNTIL_GATE turn one"))
        .unwrap();
    assert!(wait_until(Duration::from_secs(8), || {
        recorded.turns.lock().unwrap().len() > warm_turns
    }));
    mgr.start_session(&owner, &start_params(&agent, "turn two"))
        .unwrap();
    assert_eq!(
        live_state(&mgr, &agent),
        Some((SessionState::Running, 1)),
        "turn two waits, counted, behind turn one"
    );

    // Sample the session while turn one ends and turn two runs: it is never idle with a turn
    // still queued, nor idle before turn two reached the child.
    let done = Arc::new(AtomicBool::new(false));
    let violations = Arc::new(Mutex::new(Vec::new()));
    let watcher = {
        let (mgr, recorded, agent) = (Arc::clone(&mgr), Arc::clone(&recorded), agent.clone());
        let (done, violations) = (Arc::clone(&done), Arc::clone(&violations));
        std::thread::spawn(move || {
            while !done.load(Ordering::SeqCst) {
                if let Some((state, queued)) = live_state(&mgr, &agent) {
                    let turn_two_sent = recorded.turns.lock().unwrap().len() >= warm_turns + 2;
                    if state == SessionState::Idle && (queued > 0 || !turn_two_sent) {
                        violations
                            .lock()
                            .unwrap()
                            .push((state, queued, turn_two_sent));
                    }
                }
                std::thread::yield_now();
            }
        })
    };
    fs::write(&gate, "go").unwrap();
    let settled = wait_until(Duration::from_secs(8), || {
        recorded.turns.lock().unwrap().len() >= warm_turns + 2
            && live_state(&mgr, &agent) == Some((SessionState::Idle, 0))
    });
    done.store(true, Ordering::SeqCst);
    watcher.join().unwrap();
    assert!(
        settled,
        "both turns end idle: {:?}",
        live_state(&mgr, &agent)
    );
    assert!(
        violations.lock().unwrap().is_empty(),
        "the session showed idle with a turn still queued: {:?}",
        violations.lock().unwrap()
    );
}
