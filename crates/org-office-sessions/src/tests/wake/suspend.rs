//! Required-ask suspension, grace periods, and same-session resume (`C6`, `ASK-3..12`).

use super::*;

#[test]
fn c6_required_ask_suspends_on_turn_end_survives_restart_and_resumes_same_session() {
    let fx = WakeFixture::new(LOOP_ECHO_SCRIPT);
    let mgr1 = fx.make_manager(12, Duration::from_secs(3600), Duration::from_secs(2));

    // Register open required ask (from a sub-agent, ASK-12) before turn ends (ASK-11).
    mgr1.on_required_ask_opened(
        &fx.agent_a,
        SuspendedAskEntry {
            ask_id: "ask-req-1".into(),
            conversation_id: "conv-1".into(),
            question_summary: "Which target region should we deploy to?".into(),
            waiting_for_member_id: Some(fx.owner_id.clone()),
            waiting_for_name: Some("Owner".into()),
            asked_by_subagent: true,
            opened_at_ms: now_ms(),
        },
    );
    mgr1.start_session(
        &fx.owner_id,
        &MemberSessionStartParams {
            command_id: next_id(),
            employee_id: fx.agent_a.clone(),
            instruction: Some("Deploy service".into()),
            workspace: None,
            resume_runtime_session_id: None,
            fresh: false,
        },
    )
    .unwrap();

    // Turn ends -> immediately suspends without waiting for timer (ASK-11) & releases binding (ASK-4).
    assert!(
        wait_until(
            || mgr1
                .store()
                .read_suspended_state(&fx.agent_a)
                .ok()
                .flatten()
                .is_some(),
            Duration::from_secs(3)
        ),
        "agent did not suspend on turn end with open required ask"
    );
    assert!(!mgr1.bindings().has_active(&fx.agent_a));
    let rt = mgr1.member_runtime_presence(&fx.agent_a).unwrap();
    assert_eq!(rt.presence, MemberPresence::WaitingForAnswer);
    assert!(rt
        .status
        .unwrap()
        .contains("Waiting for an answer from Owner"));

    // Simulate full office restart (ASK-6): fresh SessionManager on same office durable root!
    mgr1.stop_all("restart");
    let mgr2 = fx.make_manager(12, Duration::from_secs(3600), Duration::from_secs(2));
    let inspected = mgr2
        .inspect_session(&fx.owner_id, Some(&fx.agent_a))
        .unwrap();
    assert_eq!(inspected.session.state, SessionState::Suspended);
    assert_eq!(inspected.session.suspended_on_asks, vec!["ask-req-1"]);
    let orig_rsid = inspected.session.runtime_session_id.clone();
    assert!(orig_rsid.is_some());

    // Budget hold blocks resume without losing the answer (ASK-8).
    mgr2.set_budget_hold(Some("monthly cap".into()));
    let held = mgr2.on_ask_answered(
        &fx.agent_a,
        "ask-req-1",
        "Which target region should we deploy to?",
        "eu-west-1",
        &fx.owner_id,
        "Owner",
        true,
        true,
        false,
    );
    assert_eq!(
        held,
        WakeOutcome::Queued {
            reason: WakeHoldReason::BudgetHold
        }
    );

    // Clearing budget hold resumes the EXACT same runtime_session_id with question + answer (ASK-5/8/12).
    mgr2.set_budget_hold(None);
    assert!(wait_until(
        || fx.recorded.turns.lock().unwrap().len() >= 2,
        Duration::from_secs(3)
    ));
    let resumes = fx.recorded.resumes.lock().unwrap().clone();
    assert_eq!(resumes.len(), 2);
    assert_eq!(resumes[0], None);
    assert_eq!(resumes[1], orig_rsid);
    let resumed_input = fx.recorded.turns.lock().unwrap()[1].clone();
    assert!(resumed_input.contains("Which target region should we deploy to?"));
    assert!(resumed_input.contains("eu-west-1"));
    assert!(resumed_input.contains("sub-agent"));
    mgr2.stop_all("done");
}

#[test]
fn c6_grace_period_suspends_and_names_reaped_background_work_on_resume() {
    let fx = WakeFixture::new(SLOW_TURN_SCRIPT);
    let mgr = fx.make_manager(12, Duration::from_secs(3600), Duration::from_millis(80));

    mgr.start_session(
        &fx.owner_id,
        &MemberSessionStartParams {
            command_id: next_id(),
            employee_id: fx.agent_a.clone(),
            instruction: Some("Start long background task".into()),
            workspace: None,
            resume_runtime_session_id: None,
            fresh: false,
        },
    )
    .unwrap();
    assert!(wait_until(
        || mgr
            .inspect_session(&fx.owner_id, Some(&fx.agent_a))
            .ok()
            .is_some_and(|r| !r.session.background_tasks.is_empty()),
        Duration::from_secs(3)
    ));

    // Open required ask + wait expired arms grace timer (ASK-3), reaping background work (ASK-7).
    mgr.on_required_ask_opened(
        &fx.agent_a,
        SuspendedAskEntry {
            ask_id: "ask-bg-1".into(),
            conversation_id: "conv-bg".into(),
            question_summary: "Approve schema migration?".into(),
            waiting_for_member_id: Some(fx.agent_b.clone()),
            waiting_for_name: Some("Grace".into()),
            asked_by_subagent: false,
            opened_at_ms: now_ms(),
        },
    );
    mgr.on_required_ask_wait_expired(&fx.agent_a, "ask-bg-1");
    assert!(wait_until(
        || mgr
            .store()
            .read_suspended_state(&fx.agent_a)
            .ok()
            .flatten()
            .is_some(),
        Duration::from_secs(3)
    ));
    let susp = mgr
        .store()
        .read_suspended_state(&fx.agent_a)
        .unwrap()
        .unwrap();
    assert!(
        !susp.reaped_background_tasks.is_empty(),
        "expected reaped background tasks recorded: {susp:?}"
    );

    // Peer agent answers (ASK-9); resumed turn names reaped background work (ASK-7).
    let outcome = mgr.on_ask_answered(
        &fx.agent_a,
        "ask-bg-1",
        "Approve schema migration?",
        "Approved",
        &fx.agent_b,
        "Grace",
        false,
        false,
        false,
    );
    assert_eq!(outcome, WakeOutcome::Started);
    assert!(wait_until(
        || fx.recorded.turns.lock().unwrap().len() >= 2,
        Duration::from_secs(3)
    ));
    let resumed_input = fx.recorded.turns.lock().unwrap()[1].clone();
    assert!(resumed_input.contains("Background work stopped during suspension"));
    assert!(resumed_input.contains("bg-helper-1"));
    mgr.stop_all("done");
}

#[test]
fn pass2_grace_period_disarm_prevents_stale_deadline_on_subsequent_ask() {
    let fx = WakeFixture::new(GRACE_HOLD_SCRIPT);
    let mgr = fx.make_manager(12, Duration::from_secs(3600), Duration::from_millis(250));

    mgr.start_session(
        &fx.owner_id,
        &MemberSessionStartParams {
            command_id: next_id(),
            employee_id: fx.agent_a.clone(),
            instruction: Some("Long running turn".into()),
            workspace: None,
            resume_runtime_session_id: None,
            fresh: false,
        },
    )
    .unwrap();
    assert!(wait_until(
        || mgr
            .inspect_session(&fx.owner_id, Some(&fx.agent_a))
            .unwrap()
            .session
            .state
            == SessionState::Running,
        Duration::from_secs(3)
    ));

    // Arm suspend grace deadline for ask-1.
    mgr.on_required_ask_opened(
        &fx.agent_a,
        SuspendedAskEntry {
            ask_id: "ask-grace-1".into(),
            conversation_id: "conv-1".into(),
            question_summary: "Need quick approval".into(),
            waiting_for_member_id: Some(fx.owner_id.clone()),
            waiting_for_name: Some("Owner".into()),
            asked_by_subagent: false,
            opened_at_ms: now_ms(),
        },
    );
    mgr.on_required_ask_wait_expired(&fx.agent_a, "ask-grace-1");

    // Answer ask-1 well before the 250ms grace deadline, then allow pump ticks to disarm it.
    let _ = mgr.on_ask_answered(
        &fx.agent_a,
        "ask-grace-1",
        "Need quick approval",
        "Approved",
        &fx.owner_id,
        "Owner",
        true,
        false,
        false,
    );
    std::thread::sleep(Duration::from_millis(90));

    // Open ask-2 BEFORE ask-1's original 250ms deadline would have expired.
    // Because ask-1's deadline was disarmed immediately when open asks hit zero, ask-2 must NOT
    // trigger an immediate suspend when ask-1's original 250ms elapses.
    mgr.on_required_ask_opened(
        &fx.agent_a,
        SuspendedAskEntry {
            ask_id: "ask-grace-2".into(),
            conversation_id: "conv-1".into(),
            question_summary: "Second check".into(),
            waiting_for_member_id: Some(fx.owner_id.clone()),
            waiting_for_name: Some("Owner".into()),
            asked_by_subagent: false,
            opened_at_ms: now_ms(),
        },
    );
    std::thread::sleep(Duration::from_millis(240));
    assert_eq!(
        mgr.inspect_session(&fx.owner_id, Some(&fx.agent_a))
            .unwrap()
            .session
            .state,
        SessionState::Running,
        "stale ask-1 grace deadline must not suspend session for ask-2"
    );
    assert!(mgr
        .store()
        .read_suspended_state(&fx.agent_a)
        .unwrap()
        .is_none());

    // Withdraw ask-2 before turn finishes so the session returns to Idle, not Suspended.
    mgr.on_ask_withdrawn(&fx.agent_a, "ask-grace-2");
    assert!(wait_until(
        || mgr
            .inspect_session(&fx.owner_id, Some(&fx.agent_a))
            .unwrap()
            .session
            .state
            == SessionState::Idle,
        Duration::from_secs(3)
    ));
    mgr.stop_all("done");
}
