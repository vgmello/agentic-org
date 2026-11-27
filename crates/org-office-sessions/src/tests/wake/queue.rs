//! Pending-wake coalescing, hourly limits, the deadlock breaker, and budget holds (`C8`, `L3`).

use super::*;

#[test]
fn c8_busy_agent_coalesces_three_arriving_wakes_into_one_follow_up_turn() {
    let fx = WakeFixture::new(SLOW_TURN_SCRIPT);
    let mgr = fx.make_manager(12, Duration::from_secs(3600), Duration::from_secs(2));

    mgr.start_session(
        &fx.owner_id,
        &MemberSessionStartParams {
            command_id: next_id(),
            employee_id: fx.agent_a.clone(),
            instruction: Some("Initial work".into()),
            workspace: None,
        },
    )
    .unwrap();
    assert!(wait_until(
        || mgr
            .inspect_session(&fx.owner_id, Some(&fx.agent_a))
            .ok()
            .is_some_and(|r| r.session.state == SessionState::Running),
        Duration::from_secs(2)
    ));
    for i in 1..=3 {
        let res = mgr.enqueue_wake(
            &fx.agent_a,
            PendingWakeItem {
                item_id: format!("m-{i}"),
                kind: WakeKind::Handoff,
                sender_id: fx.agent_b.clone(),
                sender_name: "Grace".into(),
                sender_handle: None,
                sender_relation: None,
                sender_is_person: false,
                conversation_id: Some("c1".into()),
                source_id: format!("msg-{i}"),
                summary: format!("Queued item {i}"),
                attachment_ids: Vec::new(),
                enqueued_at_ms: now_ms(),
            },
        );
        assert_eq!(
            res,
            WakeOutcome::Queued {
                reason: WakeHoldReason::Busy
            }
        );
    }
    assert!(wait_until(
        || fx.recorded.turns.lock().unwrap().len() >= 2,
        Duration::from_secs(3)
    ));
    let coalesced = fx.recorded.turns.lock().unwrap()[1].clone();
    assert!(coalesced.contains("Queued item 1"));
    assert!(coalesced.contains("Queued item 2"));
    assert!(coalesced.contains("Queued item 3"));
    mgr.stop_all("done");
}

#[test]
fn c8_hourly_limit_person_exemption_and_post_restart_pending_wakes() {
    let fx = WakeFixture::new(LOOP_ECHO_SCRIPT);
    let mgr = fx.make_manager(2, Duration::from_millis(2000), Duration::from_secs(2));

    let make_agent_wake = |id: &str| PendingWakeItem {
        item_id: id.into(),
        kind: WakeKind::Ask,
        sender_id: fx.agent_b.clone(),
        sender_name: "Grace".into(),
        sender_handle: None,
        sender_relation: None,
        sender_is_person: false,
        conversation_id: None,
        source_id: id.into(),
        summary: format!("Peer ask {id}"),
        attachment_ids: Vec::new(),
        enqueued_at_ms: now_ms(),
    };

    assert_eq!(
        mgr.enqueue_wake(&fx.agent_a, make_agent_wake("ask-1")),
        WakeOutcome::Started
    );
    mgr.stop_all("step1");
    assert_eq!(
        mgr.enqueue_wake(&fx.agent_a, make_agent_wake("ask-2")),
        WakeOutcome::Started
    );
    mgr.stop_all("step2");
    assert_eq!(
        mgr.enqueue_wake(&fx.agent_a, make_agent_wake("ask-3")),
        WakeOutcome::Queued {
            reason: WakeHoldReason::HourlyLimit
        }
    );

    // Person task assignment is exempt from hourly limit (AVAIL-2, AVAIL-6)!
    let p_wake = mgr.on_task_assigned(
        &fx.agent_a,
        &fx.owner_id,
        "Owner",
        true,
        "T-A99",
        "Urgent prod fix",
    );
    assert_eq!(p_wake, WakeOutcome::Started);
    mgr.stop_all("step3");

    // Simulate window expiry (LIVE-3) + full office restart (LIVE-4) with durable pending wake.
    std::thread::sleep(Duration::from_millis(2100));
    let mgr_after_restart = fx.make_manager(2, Duration::from_millis(2000), Duration::from_secs(2));
    mgr_after_restart
        .store()
        .write_pending_wakes(&crate::wake::PendingWakeQueueRecord {
            version: 1,
            employee_id: fx.agent_a.clone(),
            items: vec![PendingWakeItem {
                item_id: "after-restart-1".into(),
                kind: WakeKind::Mention,
                sender_id: fx.agent_b.clone(),
                sender_name: "Grace".into(),
                sender_handle: None,
                sender_relation: None,
                sender_is_person: false,
                conversation_id: Some("c-gen".into()),
                source_id: "msg-restart".into(),
                summary: "Survives restart".into(),
                attachment_ids: Vec::new(),
                enqueued_at_ms: now_ms(),
            }],
            hold_reason: Some(WakeHoldReason::HourlyLimit),
            held_since_ms: Some(now_ms().saturating_sub(2200)),
            retry_after_ms: Some(now_ms().saturating_sub(10)),
            updated_at: crate::credentials::rfc3339_now(),
            seen_item_ids: Vec::new(),
        })
        .unwrap();
    assert_eq!(
        mgr_after_restart.process_due_wakes_for(&fx.agent_a),
        WakeOutcome::Started
    );
    mgr_after_restart.stop_all("done");
}

#[test]
fn c8_mutual_suspension_deadlock_breaker_resumes_peer_without_human_intervention() {
    let fx = WakeFixture::new(LOOP_ECHO_SCRIPT);
    let mgr = fx.make_manager(12, Duration::from_secs(3600), Duration::from_secs(2));

    // Agent B is suspended waiting for an answer from Agent A (LIVE-8 / ADR-0030 d7).
    mgr.store()
        .write_suspended_state(&crate::wake::SuspendedStateRecord {
            version: 1,
            employee_id: fx.agent_b.clone(),
            session_id: "sess-b-1".into(),
            runtime_session_id: Some("rsid-b-1".into()),
            suspended_since: crate::credentials::rfc3339_now(),
            asks: vec![SuspendedAskEntry {
                ask_id: "ask-b-to-a".into(),
                conversation_id: "dm-ab".into(),
                question_summary: "What API version do you emit?".into(),
                waiting_for_member_id: Some(fx.agent_a.clone()),
                waiting_for_name: Some("Ada".into()),
                asked_by_subagent: false,
                opened_at_ms: now_ms(),
            }],
            reaped_background_tasks: Vec::new(),
            pending_resume_answers: Vec::new(),
            hold_reason: None,
            held_since_ms: None,
            updated_at: crate::credentials::rfc3339_now(),
        })
        .unwrap();

    // An ordinary message from someone else stays queued as Suspended (WAKE-3).
    let blocked = mgr.enqueue_wake(
        &fx.agent_b,
        PendingWakeItem {
            item_id: "other-dm".into(),
            kind: WakeKind::DirectMessage,
            sender_id: fx.owner_id.clone(),
            sender_name: "Owner".into(),
            sender_handle: None,
            sender_relation: None,
            sender_is_person: true,
            conversation_id: Some("dm-ob".into()),
            source_id: "msg-ob".into(),
            summary: "Just checking in".into(),
            attachment_ids: Vec::new(),
            enqueued_at_ms: now_ms(),
        },
    );
    assert_eq!(
        blocked,
        WakeOutcome::Queued {
            reason: WakeHoldReason::Suspended
        }
    );

    // An ask from Agent A (whom B is waiting on) breaks the mutual deadlock and wakes B!
    let broken = mgr.enqueue_wake(
        &fx.agent_b,
        PendingWakeItem {
            item_id: "ask-a-to-b".into(),
            kind: WakeKind::Ask,
            sender_id: fx.agent_a.clone(),
            sender_name: "Ada".into(),
            sender_handle: None,
            sender_relation: None,
            sender_is_person: false,
            conversation_id: Some("dm-ab".into()),
            source_id: "ask-a-to-b".into(),
            summary: "Which auth header format?".into(),
            attachment_ids: Vec::new(),
            enqueued_at_ms: now_ms(),
        },
    );
    assert_eq!(broken, WakeOutcome::Started);
    assert!(wait_until(
        || !fx.recorded.turns.lock().unwrap().is_empty(),
        Duration::from_secs(3)
    ));
    let turn_text = fx.recorded.turns.lock().unwrap()[0].clone();
    assert!(turn_text.contains("Which auth header format?"));
    assert!(turn_text.contains("What API version do you emit?"));
    assert_eq!(
        fx.recorded.resumes.lock().unwrap()[0].as_deref(),
        Some("rsid-b-1")
    );
    mgr.stop_all("done");
}

#[test]
fn pass1_idle_live_session_accepts_subsequent_wake_and_person_run_clears_stall_lock() {
    let fx = WakeFixture::new(LOOP_ECHO_SCRIPT);
    let mgr = fx.make_manager(12, Duration::from_secs(3600), Duration::from_secs(2));

    // 1. High #1 regression: start a session, wait until Idle (binding stays active!), then
    // enqueue a peer wake -> MUST dispatch immediately instead of sticking in Busy forever.
    mgr.start_session(
        &fx.owner_id,
        &MemberSessionStartParams {
            command_id: next_id(),
            employee_id: fx.agent_a.clone(),
            instruction: Some("First turn".into()),
            workspace: None,
        },
    )
    .unwrap();
    assert!(wait_until(
        || mgr
            .inspect_session(&fx.owner_id, Some(&fx.agent_a))
            .ok()
            .is_some_and(|r| r.session.state == SessionState::Idle),
        Duration::from_secs(3)
    ));
    let wake = mgr.enqueue_wake(
        &fx.agent_a,
        PendingWakeItem {
            item_id: "idle-wake-1".into(),
            kind: WakeKind::DirectMessage,
            sender_id: fx.owner_id.clone(),
            sender_name: "Owner".into(),
            sender_handle: None,
            sender_relation: None,
            sender_is_person: true,
            conversation_id: Some("dm-oa".into()),
            source_id: "msg-idle-1".into(),
            summary: "Follow up while live idle".into(),
            attachment_ids: Vec::new(),
            enqueued_at_ms: now_ms(),
        },
    );
    assert_eq!(wake, WakeOutcome::Started);
    assert!(wait_until(
        || fx.recorded.turns.lock().unwrap().len() >= 2,
        Duration::from_secs(3)
    ));
    mgr.stop_all("step1");

    // 2. Medium #9 regression: person Run clears stall_locked (LIVE-13), while autonomous wake stays held.
    let mut rec = mgr.store().read_active_turn(&fx.agent_a).unwrap().unwrap();
    rec.stall_locked = true;
    rec.consecutive_stalls = 3;
    mgr.store().write_active_turn(&rec).unwrap();
    let held = mgr.enqueue_wake(
        &fx.agent_a,
        PendingWakeItem {
            item_id: "stall-held-1".into(),
            kind: WakeKind::Ask,
            sender_id: fx.agent_b.clone(),
            sender_name: "Grace".into(),
            sender_handle: None,
            sender_relation: None,
            sender_is_person: false,
            conversation_id: None,
            source_id: "ask-s1".into(),
            summary: "Should stay held".into(),
            attachment_ids: Vec::new(),
            enqueued_at_ms: now_ms(),
        },
    );
    assert_eq!(
        held,
        WakeOutcome::Queued {
            reason: WakeHoldReason::StallLocked
        }
    );
    mgr.start_session(
        &fx.owner_id,
        &MemberSessionStartParams {
            command_id: next_id(),
            employee_id: fx.agent_a.clone(),
            instruction: Some("Owner manually resumes stalled agent".into()),
            workspace: None,
        },
    )
    .expect("person Run must clear stall_locked and start");
    mgr.stop_all("done");
}

#[test]
fn pass1_concurrent_live8_deadlock_process_due_wakes_and_redaction() {
    let fx = WakeFixture::new(LOOP_ECHO_SCRIPT);
    let mgr = fx.make_manager(12, Duration::from_secs(3600), Duration::from_millis(80));

    // 1. High #2 regression: agent B is suspended on A AND already has A's ask queued in
    // pending-wakes.v1.json (real concurrent LIVE-8); process_due_wakes breaks deadlock!
    mgr.store()
        .write_suspended_state(&crate::wake::SuspendedStateRecord {
            version: 1,
            employee_id: fx.agent_b.clone(),
            session_id: "sess-b-live8".into(),
            runtime_session_id: Some("rsid-b-live8".into()),
            suspended_since: crate::credentials::rfc3339_now(),
            asks: vec![SuspendedAskEntry {
                ask_id: "ask-b-live8".into(),
                conversation_id: "dm-ab".into(),
                question_summary: "Need secret sk-ant-oat01-SECRETTOKEN1234567890".into(),
                waiting_for_member_id: Some(fx.agent_a.clone()),
                waiting_for_name: Some("Ada".into()),
                asked_by_subagent: false,
                opened_at_ms: now_ms(),
            }],
            reaped_background_tasks: Vec::new(),
            pending_resume_answers: Vec::new(),
            hold_reason: None,
            held_since_ms: None,
            updated_at: crate::credentials::rfc3339_now(),
        })
        .unwrap();
    // Medium #12: secret prefix is redacted on disk!
    let raw_susp =
        fs::read_to_string(mgr.store().suspended_state_path(&fx.agent_b).unwrap()).unwrap();
    assert!(!raw_susp.contains("SECRETTOKEN1234567890"), "{raw_susp}");

    mgr.store()
        .write_pending_wakes(&crate::wake::PendingWakeQueueRecord {
            version: 1,
            employee_id: fx.agent_b.clone(),
            items: vec![PendingWakeItem {
                item_id: "live8-queued-ask".into(),
                kind: WakeKind::Ask,
                sender_id: fx.agent_a.clone(),
                sender_name: "Ada".into(),
                sender_handle: None,
                sender_relation: None,
                sender_is_person: false,
                conversation_id: Some("dm-ab".into()),
                source_id: "ask-a-live8".into(),
                summary: "Concurrent peer ask".into(),
                attachment_ids: Vec::new(),
                enqueued_at_ms: now_ms(),
            }],
            hold_reason: Some(WakeHoldReason::Busy),
            held_since_ms: Some(now_ms().saturating_sub(50)),
            retry_after_ms: None,
            updated_at: crate::credentials::rfc3339_now(),
            seen_item_ids: Vec::new(),
        })
        .unwrap();
    assert_eq!(mgr.process_due_wakes_for(&fx.agent_b), WakeOutcome::Started);
    assert!(wait_until(
        || !fx.recorded.turns.lock().unwrap().is_empty(),
        Duration::from_secs(3)
    ));
    mgr.stop_all("done");
}

#[test]
fn l3_office_agent_budget_hold_blocks_wake_until_extended_or_cleared() {
    use org_proto::budgets::{
        BudgetExtendOnceParams, BudgetPeriodKind, BudgetScopeKind, BudgetSetParams,
        BudgetTimeZoneSource, BudgetUnit,
    };
    use org_proto::costs::{
        AttributionBasis, CostUsageRecordParams, UsageActorKind, UsageScopeKind,
    };

    let fx = WakeFixture::new(SLOW_TURN_SCRIPT);
    let mgr = fx.make_manager(12, Duration::from_secs(3600), Duration::from_secs(2));

    // Set a 1,000-token daily budget for agent_a and record 1,000 tokens -> triggers L3 hold!
    let budget = fx
        .office
        .budget_set(
            &fx.owner_id,
            BudgetSetParams {
                operation_id: next_id(),
                scope_kind: BudgetScopeKind::Agent,
                task_id: None,
                agent_id: Some(fx.agent_a.clone()),
                release_id: None,
                period_kind: BudgetPeriodKind::Day,
                unit: BudgetUnit::Tokens,
                limit_amount: 1_000,
                time_zone: Some("UTC".into()),
                time_zone_source: Some(BudgetTimeZoneSource::OwnerPerson),
                reason: None,
            },
        )
        .unwrap()
        .unwrap()
        .budget;
    let occurred_at =
        org_office::format_rfc3339_ms(org_office::organization_family::testkit::CREATION_AT.0);
    fx.office
        .record_office_usage(CostUsageRecordParams {
            id: None,
            producer_id: "test-session".into(),
            source_event_id: "ev-budget-hit".into(),
            run_id: None,
            actor_kind: UsageActorKind::Agent,
            employee_id: Some(fx.agent_a.clone()),
            runtime: "fake".into(),
            provider: None,
            model: None,
            occurred_at,
            source_metadata_json: None,
            input_tokens: Some(600),
            output_tokens: Some(400),
            cache_read_tokens: Some(0),
            cache_write_tokens: Some(0),
            reasoning_tokens: Some(0),
            token_issue: None,
            runtime_cost_micro_usd: None,
            is_subscription: false,
            scope_kind: UsageScopeKind::Organization,
            task_id: None,
            project_id: None,
            team_id: None,
            feature_id: None,
            milestone_id: None,
            release_id: None,
            task_type_snapshot: None,
            work_type: None,
            attribution_basis: AttributionBasis::Unattributed,
        })
        .unwrap()
        .unwrap();

    // Wake for agent_a is held on BudgetHold, while agent_b is unaffected!
    let held = mgr.on_task_assigned(
        &fx.agent_a,
        &fx.owner_id,
        "Owner",
        true,
        "task-1",
        "Check status",
    );
    assert_eq!(
        held,
        WakeOutcome::Queued {
            reason: WakeHoldReason::BudgetHold
        }
    );

    // One-time +20% extension (1_000 -> 1_200 > 1_000) releases hold and retry_budget_held wakes agent_a!
    fx.office
        .budget_extend_once(
            &fx.owner_id,
            BudgetExtendOnceParams {
                operation_id: next_id(),
                budget_id: budget.id,
                reason: Some("finish turn".into()),
            },
        )
        .unwrap()
        .unwrap();
    mgr.retry_budget_held();
    assert!(wait_until(
        || !fx.recorded.turns.lock().unwrap().is_empty(),
        Duration::from_secs(3)
    ));
    mgr.stop_all("done");
}
