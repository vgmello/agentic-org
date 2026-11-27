//! Pool failover before and after side effects, and single-slot permit hand-off (`L4`,
//! `ASK-4`, `LIVE-2`).

use super::*;

#[test]
fn endpoint_env_mapping_and_failover_classification_rules() {
    assert_eq!(
        endpoint_env_for("claude", "https://proxy.example.com/v1").unwrap(),
        ("ANTHROPIC_BASE_URL", "https://proxy.example.com/v1".into())
    );
    assert_eq!(
        endpoint_env_for("codex", "https://openai.example.com/v1").unwrap(),
        ("OPENAI_BASE_URL", "https://openai.example.com/v1".into())
    );
    assert_eq!(
        endpoint_env_for("agy", "https://gemini.example.com/v1").unwrap(),
        (
            "GOOGLE_GEMINI_BASE_URL",
            "https://gemini.example.com/v1".into()
        )
    );
    assert!(endpoint_env_for("claude", "http://169.254.169.254/latest").is_err());
    assert!(endpoint_env_for("claude", "ftp://bad.example.com").is_err());

    assert_eq!(classify_failover_error(&[], true), Some("rate_limited"));
    assert_eq!(
        classify_failover_error(&["HTTP 429 too many requests".into()], false),
        Some("rate_limited")
    );
    assert_eq!(
        classify_failover_error(&["quota_exhausted".into()], false),
        Some("quota_exhausted")
    );
    assert_eq!(
        classify_failover_error(&["401 invalid_api_key".into()], false),
        Some("auth_expired")
    );
    assert_eq!(
        classify_failover_error(&["model_unavailable".into()], false),
        Some("model_unavailable")
    );
    assert_eq!(
        classify_failover_error(&["syntax error in prompt".into()], false),
        None
    );
}

#[test]
fn l4_pool_failover_before_side_effects_switches_endpoint_and_records_cooldown() {
    // Conn A rejects with rate_limit; Conn B replies with its isolated ANTHROPIC_BASE_URL!
    let script = r#"#!/bin/sh
echo "{\"kind\":\"ready\",\"endpoint\":\"$ANTHROPIC_BASE_URL\"}"
while IFS= read -r line; do
  if [ "$ANTHROPIC_BASE_URL" = "https://conn-a.example.com/v1" ]; then
    echo '{"kind":"rate_limit_fail"}'
  else
    echo "{\"kind\":\"reply\",\"text\":\"ok-from:$ANTHROPIC_BASE_URL\"}"
  fi
done
"#;
    let (_tmp, office, mgr, recorded, owner) = setup_office_and_manager(script);
    let conn_a = create_connection_and_bind(
        &office,
        &owner,
        "conn-a",
        Some("https://conn-a.example.com/v1"),
        2,
        &[("claude-sonnet-4", "claude-sonnet-4-a")],
    );
    let conn_b = create_connection_and_bind(
        &office,
        &owner,
        "conn-b",
        Some("https://conn-b.example.com/v1"),
        2,
        &[("claude-sonnet-4", "claude-sonnet-4-b")],
    );
    let profile_id = next_id();
    office
        .model_profile_create(
            &owner,
            ModelProfileCreateParams {
                command_id: next_id(),
                profile_id: profile_id.clone(),
                key: "pool-claude".into(),
                name: "Claude Pool".into(),
                description: "Ordered failover pool".into(),
                connection_ids: vec![conn_a.clone(), conn_b.clone()],
            },
        )
        .unwrap()
        .unwrap();
    let agent = hire_agent(
        &office,
        &owner,
        "failover-ada",
        &profile_id,
        "claude-sonnet-4",
    );

    mgr.start_session(
        &owner,
        &MemberSessionStartParams {
            command_id: "run-failover-1".into(),
            employee_id: agent.clone(),
            instruction: Some("Deliver status report".into()),
            workspace: None,
        },
    )
    .unwrap();

    assert!(
        wait_until(Duration::from_secs(6), || {
            mgr.inspect_session(&owner, Some(&agent))
                .ok()
                .and_then(|r| r.session.last_reply)
                .as_deref()
                == Some("ok-from:https://conn-b.example.com/v1")
        }),
        "agent should automatically fail over from conn-a to conn-b; endpoints={:?}, inspect={:?}",
        recorded.observed_endpoints.lock().unwrap(),
        mgr.inspect_session(&owner, Some(&agent)).ok()
    );

    let endpoints = recorded.observed_endpoints.lock().unwrap().clone();
    assert_eq!(
        endpoints,
        vec![
            "https://conn-a.example.com/v1".to_owned(),
            "https://conn-b.example.com/v1".to_owned(),
        ]
    );
    let inspect = mgr.inspect_session(&owner, Some(&agent)).unwrap();
    assert!(
        inspect.spec_hash.contains(":route="),
        "spec_hash must bind connection route digest: {}",
        inspect.spec_hash
    );

    let quota = office
        .provider_quota_status(
            &owner,
            ProviderQuotaStatusParams {
                connection_id: Some(conn_a.clone()),
            },
        )
        .unwrap()
        .unwrap();
    assert!(
        quota
            .cooldowns
            .iter()
            .any(|c| c.connection_id == conn_a && c.status == ConnectionQuotaStatus::Cooldown),
        "conn-a must enter cooldown after rate_limited failover: {quota:?}"
    );
    assert!(
        wait_until(Duration::from_secs(2), || {
            office
                .provider_quota_status(
                    &owner,
                    ProviderQuotaStatusParams {
                        connection_id: None,
                    },
                )
                .unwrap()
                .unwrap()
                .active_permits
                .is_empty()
        }),
        "idle agent must hold 0 active permits after failover turn finishes"
    );

    mgr.stop_all("test-end");
}

#[test]
fn l4_failover_after_tool_side_effect_enters_recovery_required_and_never_replays() {
    let script = r#"#!/bin/sh
echo "{\"kind\":\"ready\",\"endpoint\":\"$ANTHROPIC_BASE_URL\"}"
while IFS= read -r line; do
  echo '{"kind":"tool_start"}'
  echo '{"kind":"rate_limit_fail"}'
done
"#;
    let (_tmp, office, mgr, recorded, owner) = setup_office_and_manager(script);
    let conn_a = create_connection_and_bind(
        &office,
        &owner,
        "conn-se-a",
        Some("https://se-a.example.com/v1"),
        2,
        &[("claude-sonnet-4", "claude-sonnet-4")],
    );
    let conn_b = create_connection_and_bind(
        &office,
        &owner,
        "conn-se-b",
        Some("https://se-b.example.com/v1"),
        2,
        &[("claude-sonnet-4", "claude-sonnet-4")],
    );
    let profile_id = next_id();
    office
        .model_profile_create(
            &owner,
            ModelProfileCreateParams {
                command_id: next_id(),
                profile_id: profile_id.clone(),
                key: "pool-se".into(),
                name: "Side-Effect Pool".into(),
                description: "Must not replay after side effects".into(),
                connection_ids: vec![conn_a, conn_b],
            },
        )
        .unwrap()
        .unwrap();
    let agent = hire_agent(&office, &owner, "se-ada", &profile_id, "claude-sonnet-4");

    mgr.start_session(
        &owner,
        &MemberSessionStartParams {
            command_id: "run-se-1".into(),
            employee_id: agent.clone(),
            instruction: Some("Mutate state".into()),
            workspace: None,
        },
    )
    .unwrap();

    assert!(
        wait_until(Duration::from_secs(5), || {
            mgr.inspect_session(&owner, Some(&agent))
                .ok()
                .is_some_and(|r| {
                    r.session.recovery_required && r.session.state == SessionState::RecoveryRequired
                })
        }),
        "rate_limit after tool side-effect must enter RecoveryRequired: {:?}",
        mgr.inspect_session(&owner, Some(&agent)).ok()
    );
    // Give any rogue failover attempt a moment, then prove conn_b was NEVER launched!
    std::thread::sleep(Duration::from_millis(200));
    let endpoints = recorded.observed_endpoints.lock().unwrap().clone();
    assert_eq!(
        endpoints,
        vec!["https://se-a.example.com/v1".to_owned()],
        "must never replay on conn-b after external tool side effects"
    );
    let quota = office
        .provider_quota_status(
            &owner,
            ProviderQuotaStatusParams {
                connection_id: None,
            },
        )
        .unwrap()
        .unwrap();
    assert!(quota.active_permits.is_empty());

    mgr.stop_all("test-end");
}

#[test]
fn ask4_and_live2_single_slot_connection_hands_permit_from_suspended_a_to_b_and_resumes_a() {
    // Gate file lets the test control exactly when Agent A finishes its first turn!
    let tmp_gate = tempfile::tempdir().unwrap();
    let gate_file = tmp_gate.path().join("release_a");
    let script = format!(
        r#"#!/bin/sh
echo '{{"kind":"ready","endpoint":"solo"}}'
while IFS= read -r line; do
  case "$line" in
    *HOLD_UNTIL_GATE*)
      while [ ! -f "{gate}" ]; do sleep 0.05; done
      echo '{{"kind":"reply","text":"a-asked-b"}}'
      ;;
    *)
      echo '{{"kind":"reply","text":"done-turn"}}'
      ;;
  esac
done
"#,
        gate = gate_file.display()
    );
    let (_tmp, office, mgr, _recorded, owner) = setup_office_and_manager(&script);
    let conn_solo = create_connection_and_bind(
        &office,
        &owner,
        "conn-solo",
        Some("https://solo.example.com/v1"),
        1, // max_concurrent_turns = 1 (`LIVE-2`)
        &[("claude-sonnet-4", "claude-sonnet-4")],
    );
    let profile_id = next_id();
    office
        .model_profile_create(
            &owner,
            ModelProfileCreateParams {
                command_id: next_id(),
                profile_id: profile_id.clone(),
                key: "solo-pool".into(),
                name: "Single-Slot Pool".into(),
                description: "One concurrent turn".into(),
                connection_ids: vec![conn_solo.clone()],
            },
        )
        .unwrap()
        .unwrap();
    let agent_a = hire_agent(&office, &owner, "agent-a", &profile_id, "claude-sonnet-4");
    let agent_b = hire_agent(&office, &owner, "agent-b", &profile_id, "claude-sonnet-4");

    // 1. Agent A starts and holds the sole turn permit on `conn_solo`.
    mgr.start_session(
        &owner,
        &MemberSessionStartParams {
            command_id: "run-a-1".into(),
            employee_id: agent_a.clone(),
            instruction: Some("HOLD_UNTIL_GATE ask B for approval".into()),
            workspace: None,
        },
    )
    .unwrap();
    assert!(wait_until(Duration::from_secs(3), || {
        mgr.inspect_session(&owner, Some(&agent_a))
            .ok()
            .is_some_and(|r| r.session.state == SessionState::Running)
    }));
    let q_while_a = office
        .provider_quota_status(
            &owner,
            ProviderQuotaStatusParams {
                connection_id: Some(conn_solo.clone()),
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(q_while_a.active_permits.len(), 1);
    assert_eq!(q_while_a.active_permits[0].employee_id, agent_a);

    // 2. Agent A opens a required ask directed to Agent B, and Agent B receives the wake while A holds the sole slot.
    mgr.on_required_ask_opened(
        &agent_a,
        SuspendedAskEntry {
            ask_id: "ask-live2-1".into(),
            conversation_id: "conv-1".into(),
            waiting_for_member_id: Some(agent_b.clone()),
            waiting_for_name: Some("Agent agent-b".into()),
            question_summary: "Can we deploy?".into(),
            opened_at_ms: now_ms(),
            asked_by_subagent: false,
        },
    );
    let wake_b = mgr.enqueue_wake(
        &agent_b,
        PendingWakeItem {
            item_id: "ask:ask-live2-1".into(),
            kind: WakeKind::Ask,
            sender_id: agent_a.clone(),
            sender_name: "Agent agent-a".into(),
            sender_handle: None,
            sender_relation: None,
            sender_is_person: false,
            conversation_id: Some("conv-1".into()),
            source_id: "ask-live2-1".into(),
            summary: "Can we deploy?".into(),
            attachment_ids: Vec::new(),
            enqueued_at_ms: now_ms(),
        },
    );
    assert_eq!(
        wake_b,
        WakeOutcome::Queued {
            reason: WakeHoldReason::AdmissionRefused
        },
        "B must queue while A holds the sole connection slot (`LIVE-2`)"
    );

    // 3. Release gate so A finishes its turn and suspends (`ASK-4`):
    //    - A releases its permit (0 permits held by A) and shows WaitingForAnswer naming B
    //    - B automatically wakes, acquires the freed slot, and completes its turn (`LIVE-2`)!
    fs::write(&gate_file, "go").unwrap();
    assert!(
        wait_until(Duration::from_secs(5), || {
            let a_susp = mgr
                .member_runtime_presence(&agent_a)
                .is_some_and(|rt| {
                    rt.presence == MemberPresence::WaitingForAnswer
                        && rt.status.as_deref().unwrap_or("").contains("Agent agent-b")
                });
            let b_done = mgr
                .inspect_session(&owner, Some(&agent_b))
                .ok()
                .and_then(|r| r.session.last_reply)
                .as_deref()
                == Some("done-turn");
            a_susp && b_done
        }),
        "A must suspend (ASK-4) and B must automatically run on the freed slot (LIVE-2): A={:?} B={:?}",
        mgr.member_runtime_presence(&agent_a),
        mgr.inspect_session(&owner, Some(&agent_b)).ok()
    );
    assert!(
        wait_until(Duration::from_secs(3), || {
            office
                .provider_quota_status(
                    &owner,
                    ProviderQuotaStatusParams {
                        connection_id: Some(conn_solo.clone()),
                    },
                )
                .unwrap()
                .unwrap()
                .active_permits
                .is_empty()
        }),
        "after A suspends and B finishes, 0 permits must remain active (`ASK-4`)"
    );

    // 4. B answers A's required ask -> A automatically resumes on `conn_solo` (`LIVE-2`)!
    let resumed = mgr.on_ask_answered(
        &agent_a,
        "ask-live2-1",
        "Can we deploy?",
        "Yes, ship it!",
        &agent_b,
        "Agent agent-b",
        false,
        false,
        false,
    );
    assert_eq!(resumed, WakeOutcome::Started);
    assert!(
        wait_until(Duration::from_secs(5), || {
            mgr.inspect_session(&owner, Some(&agent_a))
                .ok()
                .and_then(|r| r.session.last_reply)
                .as_deref()
                == Some("done-turn")
        }),
        "A must resume and complete after B answers (`LIVE-2`): {:?}",
        mgr.inspect_session(&owner, Some(&agent_a)).ok()
    );

    mgr.stop_all("test-end");
}
