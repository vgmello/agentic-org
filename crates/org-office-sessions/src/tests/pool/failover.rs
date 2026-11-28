//! Pool failover before and after side effects, and single-slot permit hand-off (`L4`,
//! `ASK-4`, `LIVE-2`).

use super::*;

#[test]
fn endpoint_env_mapping_and_failover_classification_rules() {
    let runtimes = RuntimeRegistry::production();
    assert_eq!(
        endpoint_env_for(&runtimes, "claude", "https://proxy.example.com/v1").unwrap(),
        ("ANTHROPIC_BASE_URL", "https://proxy.example.com/v1".into())
    );
    assert_eq!(
        endpoint_env_for(&runtimes, "codex", "https://openai.example.com/v1").unwrap(),
        ("OPENAI_BASE_URL", "https://openai.example.com/v1".into())
    );
    assert_eq!(
        endpoint_env_for(&runtimes, "agy", "https://gemini.example.com/v1").unwrap(),
        (
            "GOOGLE_GEMINI_BASE_URL",
            "https://gemini.example.com/v1".into()
        )
    );
    assert_eq!(
        endpoint_env_for(&runtimes, "gemini", "https://gemini.example.com/v1").unwrap(),
        (
            "GOOGLE_GEMINI_BASE_URL",
            "https://gemini.example.com/v1".into()
        )
    );
    assert_eq!(
        endpoint_env_for(&runtimes, "kimi", "https://kimi.example.com/v1").unwrap(),
        ("KIMI_BASE_URL", "https://kimi.example.com/v1".into())
    );
    assert_eq!(
        endpoint_env_for(&runtimes, "pi", "https://pi.example.com/v1").unwrap(),
        ("PI_BASE_URL", "https://pi.example.com/v1".into())
    );
    assert!(endpoint_env_for(&runtimes, "claude", "http://169.254.169.254/latest").is_err());
    assert!(endpoint_env_for(&runtimes, "claude", "ftp://bad.example.com").is_err());

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
            resume_runtime_session_id: None,
            fresh: false,
            model: None,
            effort: None,
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
            resume_runtime_session_id: None,
            fresh: false,
            model: None,
            effort: None,
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
            resume_runtime_session_id: None,
            fresh: false,
            model: None,
            effort: None,
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
            session_affinity: None,
            model_override: None,
            effort_override: None,
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

#[test]
fn task132_missing_and_expired_vault_oauth_fail_closed_without_host_borrowing() {
    let script = r#"#!/bin/sh
echo '{"kind":"ready","endpoint":"should-never-run"}'
while IFS= read -r line; do
  echo '{"kind":"reply","text":"should-never-reply"}'
done
"#;
    let (_tmp, office, mgr, recorded, owner) = setup_office_and_manager(script);
    let conn_id = create_subscription_connection_and_bind(
        &office,
        &owner,
        "claude-sub-missing",
        None,
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
                key: "sub-missing-pool".into(),
                name: "Subscription Missing Pool".into(),
                description: "Vault-native OAuth required".into(),
                connection_ids: vec![conn_id.clone()],
            },
        )
        .unwrap()
        .unwrap();
    let agent = hire_agent(
        &office,
        &owner,
        "sub-no-vault",
        &profile_id,
        "claude-sonnet-4",
    );

    // Plant a decoy `.claude/.credentials.json` inside the agent's home to confirm ambient files
    // are never borrowed when the office vault has no enrolled credential.
    let agent_home = office.ensure_private_home(&agent).unwrap();
    fs::create_dir_all(agent_home.join(".claude")).unwrap();
    fs::write(
        agent_home.join(".claude/.credentials.json"),
        r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-ambient-decoy","expiresAt":9999999999999}}"#,
    )
    .unwrap();

    // 1. Missing vault credential MUST fail closed with NotAuthorized + actionable message +
    //    mark connection AuthExpired (`vault_credential_missing`), never spawning the child.
    let err = mgr
        .start_session(
            &owner,
            &MemberSessionStartParams {
                command_id: "run-sub-missing".into(),
                employee_id: agent.clone(),
                instruction: Some("Hello".into()),
                workspace: None,
                resume_runtime_session_id: None,
                fresh: false,
                model: None,
                effort: None,
            },
        )
        .expect_err("missing vault credential must refuse session launch");
    assert_eq!(err.code, org_proto::ErrorCode::NotAuthorized);
    assert!(
        err.message.contains("AUTH_EXPIRED")
            && err.message.contains("vault")
            && err.message.contains("agentic-org"),
        "expected actionable vault enrollment guidance: {}",
        err.message
    );
    let state = office.load_connection_state(&conn_id).unwrap().unwrap();
    assert_eq!(
        state.status,
        org_proto::providers::ConnectionReadinessStatus::AuthExpired
    );
    assert_eq!(
        state.issue_code.as_deref(),
        Some("vault_credential_missing")
    );
    assert!(recorded.observed_endpoints.lock().unwrap().is_empty());

    // 2. Re-ready the connection and store an EXPIRED vault OAuth envelope without refresh_token:
    //    MUST fail closed, mark AuthExpired (`vault_oauth_expired`), and never spawn.
    refresh_subscription_connection_ready(
        &office,
        &owner,
        &conn_id,
        "claude-sub-missing",
        &[("claude-sonnet-4", "claude-sonnet-4")],
    );
    let expired_json = serde_json::json!({
        "access_token": "sk-ant-oat01-expired-only",
        "expires_at_ms": 1_u64,
    })
    .to_string();
    mgr.enrollment()
        .vault()
        .store_wrapped(&conn_id, "anthropic", expired_json.as_bytes(), None, None)
        .unwrap();
    let exp_err = mgr
        .start_session(
            &owner,
            &MemberSessionStartParams {
                command_id: "run-sub-expired".into(),
                employee_id: agent,
                instruction: Some("Hello again".into()),
                workspace: None,
                resume_runtime_session_id: None,
                fresh: false,
                model: None,
                effort: None,
            },
        )
        .expect_err("expired unrefreshable vault OAuth must refuse session launch");
    assert_eq!(exp_err.code, org_proto::ErrorCode::NotAuthorized);
    assert!(
        exp_err.message.contains("AUTH_EXPIRED") && exp_err.message.contains("agentic-org"),
        "expected actionable re-auth guidance: {}",
        exp_err.message
    );
    let exp_state = office.load_connection_state(&conn_id).unwrap().unwrap();
    assert_eq!(
        exp_state.status,
        org_proto::providers::ConnectionReadinessStatus::AuthExpired
    );
    assert_eq!(exp_state.issue_code.as_deref(), Some("vault_oauth_expired"));
    assert!(recorded.observed_endpoints.lock().unwrap().is_empty());

    mgr.stop_all("test-end");
}

#[test]
fn task132_proactive_and_reactive_vault_refresh_rotate_token_without_subprocess_probe() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use zeroize::Zeroizing;

    struct ScriptedRefresher {
        calls: AtomicUsize,
        next_token: &'static str,
        next_refresh: &'static str,
        next_exp: u64,
    }

    impl crate::host_auth::VaultTokenRefresher for ScriptedRefresher {
        fn refresh(
            &self,
            _connection_id: &str,
            _provider: &str,
            _refresh_token: &str,
        ) -> Result<crate::host_auth::VaultOAuthCredential, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(crate::host_auth::VaultOAuthCredential {
                access_token: Zeroizing::new(self.next_token.to_owned()),
                refresh_token: Some(Zeroizing::new(self.next_refresh.to_owned())),
                expires_at_ms: Some(self.next_exp),
            })
        }
    }

    // Script rejects `revoked-t1` with auth_expired_fail and accepts rotated tokens.
    let script = r#"#!/bin/sh
echo '{"kind":"ready","endpoint":"vault-oauth"}'
while IFS= read -r line; do
  case "$CLAUDE_CODE_OAUTH_TOKEN" in
    sk-ant-oat01-proactive-t2)
      echo '{"kind":"reply","text":"accepted-proactive-t2"}'
      ;;
    sk-ant-oat01-revoked-t1)
      echo '{"kind":"auth_expired_fail"}'
      ;;
    sk-ant-oat01-reactive-t2)
      echo '{"kind":"reply","text":"accepted-reactive-t2"}'
      ;;
    *)
      echo '{"kind":"auth_expired_fail"}'
      ;;
  esac
done
"#;
    let (_tmp, office, mgr, _recorded, owner) = setup_office_and_manager(script);
    let conn_pro = create_subscription_connection_and_bind(
        &office,
        &owner,
        "claude-proactive",
        None,
        2,
        &[("claude-sonnet-4", "claude-sonnet-4")],
    );
    let prof_pro = next_id();
    office
        .model_profile_create(
            &owner,
            ModelProfileCreateParams {
                command_id: next_id(),
                profile_id: prof_pro.clone(),
                key: "pool-proactive".into(),
                name: "Proactive Refresh Pool".into(),
                description: "Proactive vault refresh".into(),
                connection_ids: vec![conn_pro.clone()],
            },
        )
        .unwrap()
        .unwrap();
    let agent_pro = hire_agent(&office, &owner, "pro-ada", &prof_pro, "claude-sonnet-4");

    // 1. Proactive launch refresh: expiring vault envelope rotates once and persists before spawn.
    let future_exp = now_ms() + 3_600_000;
    let pro_refresher = Arc::new(ScriptedRefresher {
        calls: AtomicUsize::new(0),
        next_token: "sk-ant-oat01-proactive-t2",
        next_refresh: "rt-pro-2",
        next_exp: future_exp,
    });
    mgr.vault_oauth_refresh
        .set_refresher(Arc::clone(&pro_refresher) as Arc<dyn crate::host_auth::VaultTokenRefresher>);
    let expiring_env = serde_json::json!({
        "access_token": "sk-ant-oat01-expiring-t1",
        "refresh_token": "rt-pro-1",
        "expires_at_ms": 1_u64,
    })
    .to_string();
    mgr.enrollment()
        .vault()
        .store_wrapped(&conn_pro, "anthropic", expiring_env.as_bytes(), None, None)
        .unwrap();

    mgr.start_session(
        &owner,
        &MemberSessionStartParams {
            command_id: "run-proactive-1".into(),
            employee_id: agent_pro.clone(),
            instruction: Some("Proactive turn".into()),
            workspace: None,
            resume_runtime_session_id: None,
            fresh: false,
            model: None,
            effort: None,
        },
    )
    .unwrap();
    assert!(
        wait_until(Duration::from_secs(6), || {
            mgr.inspect_session(&owner, Some(&agent_pro))
                .ok()
                .and_then(|r| r.session.last_reply)
                .as_deref()
                == Some("accepted-proactive-t2")
        }),
        "proactive vault refresh must inject rotated token: {:?}",
        mgr.inspect_session(&owner, Some(&agent_pro)).ok()
    );
    assert_eq!(pro_refresher.calls.load(Ordering::SeqCst), 1);
    {
        let reg = mgr.registry.lock().unwrap();
        let live = reg.live.get(&agent_pro).expect("live session handle");
        assert_eq!(live.oauth_expires_at_ms, Some(future_exp));
    }
    let redaction = mgr.active_redaction_secrets(&agent_pro);
    let redaction_strs: Vec<&str> = redaction.iter().map(|s| s.as_str()).collect();
    assert!(redaction_strs.contains(&"sk-ant-oat01-proactive-t2"));
    assert!(redaction_strs.contains(&"rt-pro-2"));

    // 2. Reactive `auth_expired` failover: revoked future-dated token rotates in vault & retries once.
    let conn_react = create_subscription_connection_and_bind(
        &office,
        &owner,
        "claude-reactive",
        None,
        2,
        &[("claude-sonnet-4", "claude-sonnet-4")],
    );
    let prof_react = next_id();
    office
        .model_profile_create(
            &owner,
            ModelProfileCreateParams {
                command_id: next_id(),
                profile_id: prof_react.clone(),
                key: "pool-reactive".into(),
                name: "Reactive Refresh Pool".into(),
                description: "Reactive vault refresh".into(),
                connection_ids: vec![conn_react.clone()],
            },
        )
        .unwrap()
        .unwrap();
    let agent_react = hire_agent(&office, &owner, "react-ada", &prof_react, "claude-sonnet-4");
    let react_exp = now_ms() + 7_200_000;
    let react_refresher = Arc::new(ScriptedRefresher {
        calls: AtomicUsize::new(0),
        next_token: "sk-ant-oat01-reactive-t2",
        next_refresh: "rt-react-2",
        next_exp: react_exp,
    });
    mgr.vault_oauth_refresh.set_refresher(
        Arc::clone(&react_refresher) as Arc<dyn crate::host_auth::VaultTokenRefresher>
    );
    let revoked_env = serde_json::json!({
        "access_token": "sk-ant-oat01-revoked-t1",
        "refresh_token": "rt-react-1",
        "expires_at_ms": future_exp,
    })
    .to_string();
    mgr.enrollment()
        .vault()
        .store_wrapped(&conn_react, "anthropic", revoked_env.as_bytes(), None, None)
        .unwrap();

    mgr.start_session(
        &owner,
        &MemberSessionStartParams {
            command_id: "run-reactive-1".into(),
            employee_id: agent_react.clone(),
            instruction: Some("Reactive turn".into()),
            workspace: None,
            resume_runtime_session_id: None,
            fresh: false,
            model: None,
            effort: None,
        },
    )
    .unwrap();
    assert!(
        wait_until(Duration::from_secs(6), || {
            mgr.inspect_session(&owner, Some(&agent_react))
                .ok()
                .and_then(|r| r.session.last_reply)
                .as_deref()
                == Some("accepted-reactive-t2")
        }),
        "reactive vault refresh must rotate token and relaunch cleanly: {:?}",
        mgr.inspect_session(&owner, Some(&agent_react)).ok()
    );
    assert_eq!(react_refresher.calls.load(Ordering::SeqCst), 1);
    let stored_raw = mgr
        .enrollment()
        .vault()
        .load_unwrapped(&conn_react)
        .unwrap()
        .unwrap();
    let stored = crate::host_auth::parse_vault_oauth_credential(&stored_raw).unwrap();
    assert_eq!(stored.access_token.as_str(), "sk-ant-oat01-reactive-t2");
    assert_eq!(stored.expires_at_ms, Some(react_exp));
    let conn_state = office.load_connection_state(&conn_react).unwrap().unwrap();
    assert_eq!(
        conn_state.status,
        org_proto::providers::ConnectionReadinessStatus::Ready
    );

    mgr.stop_all("test-end");
}

#[test]
fn task132_operator_vault_rotation_and_unrefreshable_auth_expired_failover() {
    let tmp_gate = tempfile::tempdir().unwrap();
    let gate_file = tmp_gate.path().join("emit_401");
    let script = format!(
        r#"#!/bin/sh
echo '{{"kind":"ready","endpoint":"op-rotate"}}'
while IFS= read -r line; do
  case "$CLAUDE_CODE_OAUTH_TOKEN" in
    sk-ant-oat01-op-t1)
      while [ ! -f "{gate}" ]; do sleep 0.05; done
      echo '{{"kind":"auth_expired_fail"}}'
      ;;
    sk-ant-oat01-op-t2)
      echo '{{"kind":"reply","text":"accepted-operator-t2"}}'
      ;;
    sk-ant-oat01-bad-conn)
      echo '{{"kind":"auth_expired_fail"}}'
      ;;
    sk-ant-oat01-backup-ok)
      echo '{{"kind":"reply","text":"accepted-backup-conn"}}'
      ;;
    *)
      echo '{{"kind":"auth_expired_fail"}}'
      ;;
  esac
done
"#,
        gate = gate_file.display()
    );
    let (_tmp, office, mgr, _recorded, owner) = setup_office_and_manager(&script);

    // 1. Operator rotates vault T1 -> T2 while Turn 1 is in flight; when T1 hits 401 with default
    //    NoopVaultTokenRefresher, failover re-reads the vault, adopts T2, and completes cleanly.
    let conn_op = create_subscription_connection_and_bind(
        &office,
        &owner,
        "claude-op-rot",
        None,
        2,
        &[("claude-sonnet-4", "claude-sonnet-4")],
    );
    let prof_op = next_id();
    office
        .model_profile_create(
            &owner,
            ModelProfileCreateParams {
                command_id: next_id(),
                profile_id: prof_op.clone(),
                key: "pool-op-rot".into(),
                name: "Operator Rotation Pool".into(),
                description: "Adopts operator vault re-enrollment".into(),
                connection_ids: vec![conn_op.clone()],
            },
        )
        .unwrap()
        .unwrap();
    let agent_op = hire_agent(&office, &owner, "op-ada", &prof_op, "claude-sonnet-4");
    mgr.enrollment()
        .vault()
        .store_wrapped(&conn_op, "anthropic", b"sk-ant-oat01-op-t1", None, None)
        .unwrap();
    mgr.start_session(
        &owner,
        &MemberSessionStartParams {
            command_id: "run-op-1".into(),
            employee_id: agent_op.clone(),
            instruction: Some("Wait then fail 401".into()),
            workspace: None,
            resume_runtime_session_id: None,
            fresh: false,
            model: None,
            effort: None,
        },
    )
    .unwrap();
    assert!(wait_until(Duration::from_secs(3), || {
        mgr.inspect_session(&owner, Some(&agent_op))
            .ok()
            .is_some_and(|r| r.session.state == SessionState::Running)
    }));
    // Operator re-enrolls fresh token T2 into the vault before T1's 401 arrives.
    mgr.enrollment()
        .vault()
        .store_wrapped(&conn_op, "anthropic", b"sk-ant-oat01-op-t2", None, None)
        .unwrap();
    fs::write(&gate_file, "go").unwrap();
    assert!(
        wait_until(Duration::from_secs(6), || {
            mgr.inspect_session(&owner, Some(&agent_op))
                .ok()
                .and_then(|r| r.session.last_reply)
                .as_deref()
                == Some("accepted-operator-t2")
        }),
        "operator-rotated vault token must be adopted on auth_expired failover: {:?}",
        mgr.inspect_session(&owner, Some(&agent_op)).ok()
    );

    // 2. Unrefreshable 401 on conn_a marks conn_a AuthExpired and fails over to healthy conn_b.
    let conn_a = create_subscription_connection_and_bind(
        &office,
        &owner,
        "claude-bad-a",
        None,
        2,
        &[("claude-sonnet-4", "claude-sonnet-4")],
    );
    let conn_b = create_subscription_connection_and_bind(
        &office,
        &owner,
        "claude-good-b",
        None,
        2,
        &[("claude-sonnet-4", "claude-sonnet-4")],
    );
    mgr.enrollment()
        .vault()
        .store_wrapped(&conn_a, "anthropic", b"sk-ant-oat01-bad-conn", None, None)
        .unwrap();
    mgr.enrollment()
        .vault()
        .store_wrapped(&conn_b, "anthropic", b"sk-ant-oat01-backup-ok", None, None)
        .unwrap();
    let prof_ab = next_id();
    office
        .model_profile_create(
            &owner,
            ModelProfileCreateParams {
                command_id: next_id(),
                profile_id: prof_ab.clone(),
                key: "pool-ab-auth".into(),
                name: "Auth Failover Pool".into(),
                description: "Failover on unrefreshable auth_expired".into(),
                connection_ids: vec![conn_a.clone(), conn_b],
            },
        )
        .unwrap()
        .unwrap();
    let agent_ab = hire_agent(&office, &owner, "ab-ada", &prof_ab, "claude-sonnet-4");
    mgr.start_session(
        &owner,
        &MemberSessionStartParams {
            command_id: "run-ab-auth".into(),
            employee_id: agent_ab.clone(),
            instruction: Some("Fail over to conn-b".into()),
            workspace: None,
            resume_runtime_session_id: None,
            fresh: false,
            model: None,
            effort: None,
        },
    )
    .unwrap();
    assert!(
        wait_until(Duration::from_secs(6), || {
            mgr.inspect_session(&owner, Some(&agent_ab))
                .ok()
                .and_then(|r| r.session.last_reply)
                .as_deref()
                == Some("accepted-backup-conn")
        }),
        "unrefreshable auth_expired must fail over to conn_b: {:?}",
        mgr.inspect_session(&owner, Some(&agent_ab)).ok()
    );
    let state_a = office.load_connection_state(&conn_a).unwrap().unwrap();
    assert_eq!(
        state_a.status,
        org_proto::providers::ConnectionReadinessStatus::AuthExpired
    );
    assert_eq!(state_a.issue_code.as_deref(), Some("auth_expired"));

    // 3. Single-connection unrefreshable 401 exhausts pool with actionable AUTH_EXPIRED last_error.
    let conn_solo = create_subscription_connection_and_bind(
        &office,
        &owner,
        "claude-bad-solo",
        None,
        2,
        &[("claude-sonnet-4", "claude-sonnet-4")],
    );
    mgr.enrollment()
        .vault()
        .store_wrapped(
            &conn_solo,
            "anthropic",
            b"sk-ant-oat01-bad-conn",
            None,
            None,
        )
        .unwrap();
    let prof_solo = next_id();
    office
        .model_profile_create(
            &owner,
            ModelProfileCreateParams {
                command_id: next_id(),
                profile_id: prof_solo.clone(),
                key: "pool-solo-auth".into(),
                name: "Solo Auth Pool".into(),
                description: "Exhausts into actionable re-auth error".into(),
                connection_ids: vec![conn_solo.clone()],
            },
        )
        .unwrap()
        .unwrap();
    let agent_solo = hire_agent(&office, &owner, "solo-auth", &prof_solo, "claude-sonnet-4");
    mgr.start_session(
        &owner,
        &MemberSessionStartParams {
            command_id: "run-solo-auth".into(),
            employee_id: agent_solo.clone(),
            instruction: Some("Exhaust auth".into()),
            workspace: None,
            resume_runtime_session_id: None,
            fresh: false,
            model: None,
            effort: None,
        },
    )
    .unwrap();
    assert!(
        wait_until(Duration::from_secs(6), || {
            mgr.inspect_session(&owner, Some(&agent_solo))
                .ok()
                .and_then(|r| r.session.last_error)
                .is_some_and(|e| e.contains("AUTH_EXPIRED") && e.contains("agentic-org"))
        }),
        "exhausted auth_expired failover must surface actionable re-auth error: {:?}",
        mgr.inspect_session(&owner, Some(&agent_solo)).ok()
    );
    let state_solo = office.load_connection_state(&conn_solo).unwrap().unwrap();
    assert_eq!(
        state_solo.status,
        org_proto::providers::ConnectionReadinessStatus::AuthExpired
    );

    mgr.stop_all("test-end");
}

#[test]
fn task132_mixed_pool_exhaustion_preserves_pool_exhausted_and_earliest_cooldown() {
    let script = "#!/bin/sh\necho '{\"kind\":\"ready\"}'\n";
    let (_tmp, office, mgr, _recorded, owner) = setup_office_and_manager(script);
    let conn_missing = create_subscription_connection_and_bind(
        &office,
        &owner,
        "claude-mixed-missing",
        None,
        2,
        &[("claude-sonnet-4", "claude-sonnet-4")],
    );
    let conn_cd = create_subscription_connection_and_bind(
        &office,
        &owner,
        "claude-mixed-cd",
        None,
        2,
        &[("claude-sonnet-4", "claude-sonnet-4")],
    );
    mgr.enrollment()
        .vault()
        .store_wrapped(&conn_cd, "anthropic", b"sk-ant-oat01-cd-ok", None, None)
        .unwrap();
    office
        .record_connection_cooldown_internal(&conn_cd, None, Some("2027-01-01T00:00:00Z"), None)
        .unwrap();

    let prof = next_id();
    office
        .model_profile_create(
            &owner,
            ModelProfileCreateParams {
                command_id: next_id(),
                profile_id: prof.clone(),
                key: "pool-mixed-auth-cd".into(),
                name: "Mixed Pool".into(),
                description: "Missing vault + cooldown".into(),
                connection_ids: vec![conn_missing.clone(), conn_cd],
            },
        )
        .unwrap()
        .unwrap();
    let agent = hire_agent(&office, &owner, "mixed-ada", &prof, "claude-sonnet-4");

    // Both Pass 1 (conn_missing fails vault lookup) and Pass 2 (conn_missing already AuthExpired)
    // must classify as Conflict / POOL_EXHAUSTED and preserve earliest_cooldown=.
    for cmd in ["run-mixed-pass1", "run-mixed-pass2"] {
        let err = mgr
            .start_session(
                &owner,
                &MemberSessionStartParams {
                    command_id: cmd.into(),
                    employee_id: agent.clone(),
                    instruction: Some("Check mixed pool".into()),
                    workspace: None,
                    resume_runtime_session_id: None,
                    fresh: false,
                    model: None,
                    effort: None,
                },
            )
            .expect_err("mixed pool must refuse launch");
        assert_eq!(err.code, org_proto::ErrorCode::Conflict, "{cmd}: {err:?}");
        assert!(
            err.message.contains("POOL_EXHAUSTED") && err.message.contains("earliest_cooldown="),
            "{cmd}: expected POOL_EXHAUSTED with earliest_cooldown, got: {}",
            err.message
        );
    }
    let state_missing = office
        .load_connection_state(&conn_missing)
        .unwrap()
        .unwrap();
    assert_eq!(
        state_missing.status,
        org_proto::providers::ConnectionReadinessStatus::AuthExpired
    );
    mgr.stop_all("test-end");
}

#[test]
fn task132_single_slot_permit_never_leaks_on_auth_rejection_panic_or_keyless_vault() {
    struct PanickingRefresher;
    impl crate::host_auth::VaultTokenRefresher for PanickingRefresher {
        fn refresh(
            &self,
            _connection_id: &str,
            _provider: &str,
            _refresh_token: &str,
        ) -> Result<crate::host_auth::VaultOAuthCredential, String> {
            panic!("simulated refresher panic");
        }
    }

    let script = r#"#!/bin/sh
echo '{"kind":"ready","endpoint":"single-slot"}'
while IFS= read -r line; do
  echo '{"kind":"reply","text":"single-slot-ok"}'
done
"#;
    let (_tmp, office, mgr, _recorded, owner) = setup_office_and_manager(script);
    let conn_id = create_subscription_connection_and_bind(
        &office,
        &owner,
        "claude-solo-1",
        None,
        1, // max_concurrent_turns = 1
        &[("claude-sonnet-4", "claude-sonnet-4")],
    );
    let prof = next_id();
    office
        .model_profile_create(
            &owner,
            ModelProfileCreateParams {
                command_id: next_id(),
                profile_id: prof.clone(),
                key: "pool-solo-slot".into(),
                name: "Single Slot Pool".into(),
                description: "Zero permit leak guard".into(),
                connection_ids: vec![conn_id.clone()],
            },
        )
        .unwrap()
        .unwrap();
    let agent = hire_agent(&office, &owner, "solo-slot-ada", &prof, "claude-sonnet-4");
    let active_permits = || {
        office
            .provider_quota_status(
                &owner,
                ProviderQuotaStatusParams {
                    connection_id: Some(conn_id.clone()),
                },
            )
            .unwrap()
            .unwrap()
            .active_permits
            .len()
    };

    // 1. Missing vault rejection must release the sole turn permit.
    assert!(mgr
        .start_session(
            &owner,
            &MemberSessionStartParams {
                command_id: "solo-missing".into(),
                employee_id: agent.clone(),
                instruction: Some("missing".into()),
                workspace: None,
                resume_runtime_session_id: None,
                fresh: false,
                model: None,
                effort: None,
            },
        )
        .is_err());
    assert_eq!(active_permits(), 0);

    // 2. Expired vault rejection must release the sole turn permit.
    refresh_subscription_connection_ready(
        &office,
        &owner,
        &conn_id,
        "claude-solo-1",
        &[("claude-sonnet-4", "claude-sonnet-4")],
    );
    let exp_json = serde_json::json!({
        "access_token": "sk-ant-oat01-expired",
        "refresh_token": "rt-solo",
        "expires_at_ms": 1_u64,
    })
    .to_string();
    mgr.enrollment()
        .vault()
        .store_wrapped(&conn_id, "anthropic", exp_json.as_bytes(), None, None)
        .unwrap();
    assert!(mgr
        .start_session(
            &owner,
            &MemberSessionStartParams {
                command_id: "solo-expired".into(),
                employee_id: agent.clone(),
                instruction: Some("expired".into()),
                workspace: None,
                resume_runtime_session_id: None,
                fresh: false,
                model: None,
                effort: None,
            },
        )
        .is_err());
    assert_eq!(active_permits(), 0);

    // 3. Panic inside VaultTokenRefresher::refresh must unwind RAII guard and release permit.
    refresh_subscription_connection_ready(
        &office,
        &owner,
        &conn_id,
        "claude-solo-1",
        &[("claude-sonnet-4", "claude-sonnet-4")],
    );
    let panic_coord = crate::host_auth::VaultRefreshCoordinator::new(Arc::new(PanickingRefresher));
    let excluded = std::collections::HashSet::new();
    let runtimes = RuntimeRegistry::production();
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = crate::router::select_and_admit_route(crate::router::AdmitRouteRequest {
            office: &office,
            vault: mgr.enrollment().vault(),
            runtimes: &runtimes,
            hq_permits: None,
            oauth_refresh: Some(&panic_coord),
            employee_id: &agent,
            task_id: None,
            is_failover_switch: false,
            excluded_connections: &excluded,
            turn_override: None,
        });
    }));
    assert!(panicked.is_err(), "refresher panic should unwind");
    assert_eq!(
        active_permits(),
        0,
        "RAII guard must release candidate permit when refresher panics"
    );

    // 4. Keyless vault (MissingSecretsKey) returns Unavailable WITHOUT poisoning AuthExpired or leaking permit.
    let info = office.info();
    let keyless_vault = crate::credentials::CredentialVault::new(
        office.durable_root().to_path_buf(),
        info.org_id.clone(),
        info.office_id.clone(),
        None,
    );
    let unavail = crate::router::select_and_admit_route(crate::router::AdmitRouteRequest {
        office: &office,
        vault: &keyless_vault,
        runtimes: &runtimes,
        hq_permits: None,
        oauth_refresh: None,
        employee_id: &agent,
        task_id: None,
        is_failover_switch: false,
        excluded_connections: &excluded,
        turn_override: None,
    })
    .err()
    .expect("MissingSecretsKey must return Unavailable");
    assert_eq!(unavail.code, org_proto::ErrorCode::Unavailable);
    assert_eq!(
        office
            .load_connection_state(&conn_id)
            .unwrap()
            .unwrap()
            .status,
        org_proto::providers::ConnectionReadinessStatus::Ready,
        "MissingSecretsKey must NOT poison connection readiness as AuthExpired"
    );
    assert_eq!(active_permits(), 0);

    // 5. After enrolling a valid token, launching on max_concurrent_turns = 1 succeeds cleanly.
    mgr.enrollment()
        .vault()
        .store_wrapped(
            &conn_id,
            "anthropic",
            b"sk-ant-oat01-solo-valid",
            None,
            None,
        )
        .unwrap();
    mgr.start_session(
        &owner,
        &MemberSessionStartParams {
            command_id: "solo-valid".into(),
            employee_id: agent.clone(),
            instruction: Some("run ok".into()),
            workspace: None,
            resume_runtime_session_id: None,
            fresh: false,
            model: None,
            effort: None,
        },
    )
    .unwrap();
    assert!(
        wait_until(Duration::from_secs(5), || {
            mgr.inspect_session(&owner, Some(&agent))
                .ok()
                .and_then(|r| r.session.last_reply)
                .as_deref()
                == Some("single-slot-ok")
        }),
        "valid launch must succeed after prior rejections without permit leaks"
    );
    mgr.stop_all("test-end");
}
