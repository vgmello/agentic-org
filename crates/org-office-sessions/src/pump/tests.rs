//! Pump unit tests: failover classification, permits, and mid-turn delivery.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use org_agent_runtime::{ApprovalId, ApprovalRequest, Control, Decision, PermissionPolicy};
use org_office::organization_family::records::PermissionPreset;
use org_proto::sessions::SessionState;

use crate::binding::RunBindingRegistry;
use crate::recovery::{SessionStore, StallWatchdog};
use crate::subagents::BackgroundRegistry;

use super::approval::handle_approval_request;
use super::commands::{finalize_drained_stop, merge_stop_preserve};
use super::events::{evaluate_turn_failover, redact_event_into_history};
use super::turn::{ensure_permit_for_turn, PermitCheck};
use super::{PumpContext, PumpHooks, PumpLoopState, SessionRuntimeState};

fn test_pump_ctx(
    dir: &std::path::Path,
    connection_id: Option<String>,
    oauth_expires_at_ms: Option<u64>,
) -> PumpContext {
    let state = Arc::new(Mutex::new(SessionRuntimeState {
        employee_id: "emp-1".into(),
        office_id: "office-1".into(),
        state: SessionState::Running,
        run_id: Some("run-1".into()),
        session_id: Some("sess-1".into()),
        runtime_id: Some("claude".into()),
        runtime_session_id: None,
        private_home: dir.display().to_string(),
        permission_preset: "autonomous_sandboxed".into(),
        sandbox_enabled: true,
        spec_hash: "h".into(),
        watchdog: StallWatchdog::new(Duration::from_secs(60), Instant::now()),
        recovery_required: false,
        last_reply: None,
        last_error: None,
        backgrounds: BackgroundRegistry::default(),
        pending_approvals: Vec::new(),
        policy: PermissionPolicy {
            version: 1,
            allow: Vec::new(),
            deny: Vec::new(),
        },
        preset: PermissionPreset::AutonomousSandboxed,
        allowed_tools: None,
        writable_roots: Vec::new(),
        suspended_on_asks: Vec::new(),
        waiting_for_member_id: None,
        waiting_for_name: None,
        suspended_since: None,
        pending_wake_count: 0,
        pending_wake_reason: None,
    }));
    PumpContext {
        employee_id: "emp-1".into(),
        run_id: "run-1".into(),
        runtime: Arc::new(org_agent_runtime_claude::ClaudeRuntime::new()),
        supports_mid_turn: false,
        connection_id,
        active_permit: Arc::new(Mutex::new(None)),
        workspace: None,
        task_ref: None,
        tried_connections: Vec::new(),
        oauth_refresh_attempted: false,
        oauth_expires_at_ms,
        oauth_token_fp: Some("deadbeefcafebabe".into()),
        store: SessionStore::new(dir, "office-1"),
        bindings: RunBindingRegistry::new(),
        state,
        stop_plan: org_agent_runtime::StopPlan { steps: Vec::new() },
        cleanup_paths: Vec::new(),
        binding_file: dir.join("binding"),
        hooks: PumpHooks {
            has_open_required_asks: None,
            on_turn_ended: None,
            on_suspended: None,
            on_usage: None,
            on_permit_acquire: None,
            on_permit_renew: None,
            on_permit_release: None,
            on_permit_release_pending: None,
            on_rate_limit: None,
            on_failover: None,
            stage_mid_turn_wakes: None,
            on_mid_turn_delivered: None,
            is_bwrap_topology_stale: None,
        },
        stop_epoch: 0,
        partition: None,
    }
}

#[test]
fn finalize_drained_stop_preserves_recycle_pending_turns_and_clears_on_user_stop() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = test_pump_ctx(dir.path(), Some("conn-1".into()), Some(1));
    let mut loop_state = PumpLoopState::new(false);

    // Verify stop_preserve precedence across mixed Stop reason orderings:
    // non-recycle Stop always wins over recycle Stop in either order.
    assert_eq!(merge_stop_preserve(None, "warm_route_stale"), Some(true));
    assert_eq!(
        merge_stop_preserve(None, "oauth_token_expiring"),
        Some(true)
    );
    assert_eq!(merge_stop_preserve(None, "pool_reroute"), Some(true));
    assert_eq!(
        merge_stop_preserve(Some(true), "user_stop"),
        Some(false),
        "non-recycle Stop after recycle Stop must win"
    );
    assert_eq!(
        merge_stop_preserve(Some(false), "warm_route_stale"),
        Some(false),
        "recycle Stop after non-recycle Stop must not override"
    );

    // Simulate recycle Stop arriving before SendTurn in the same drain pass.
    loop_state.pending_turns.push("queued-wake".into());
    finalize_drained_stop(
        merge_stop_preserve(None, "warm_route_stale"),
        &ctx,
        &mut loop_state,
    );
    assert!(loop_state.pending_turns.is_empty());
    let req = loop_state
        .failover_pending
        .as_ref()
        .expect("recycle stop must preserve pending_turns into failover_pending");
    assert_eq!(req.reason, "warm_route_stale");
    assert_eq!(req.objective, "queued-wake");
    assert_eq!(req.oauth_token_fp.as_deref(), Some("deadbeefcafebabe"));

    // Non-recycle Stop (e.g. user stop / shutdown) clears failover_pending.
    loop_state.pending_turns.push("discard-on-user-stop".into());
    finalize_drained_stop(
        merge_stop_preserve(Some(true), "user_stop"),
        &ctx,
        &mut loop_state,
    );
    assert!(loop_state.failover_pending.is_none());

    // Turn 1 (`turns_dispatched == 0`) with expiring OAuth MUST remain Ready (prevents
    // unbounded cold-start loop when refresh fails or token doesn't rotate in 5m lead window).
    let legacy_ctx = test_pump_ctx(dir.path(), None, Some(1));
    assert!(matches!(
        ensure_permit_for_turn(&legacy_ctx, &mut loop_state),
        PermitCheck::Ready
    ));
    assert!(matches!(
        ensure_permit_for_turn(&ctx, &mut loop_state),
        PermitCheck::Ready
    ));

    // Warm reuse (Turn 2+, `turns_dispatched > 0`) triggers StaleWarm on expiring OAuth for
    // BOTH connected and legacy (`connection_id: None`) sessions.
    loop_state.turns_dispatched = 1;
    assert!(matches!(
        ensure_permit_for_turn(&legacy_ctx, &mut loop_state),
        PermitCheck::StaleWarm
    ));
    assert!(matches!(
        ensure_permit_for_turn(&ctx, &mut loop_state),
        PermitCheck::StaleWarm
    ));
}

#[test]
fn evaluate_turn_failover_coalesces_queued_pending_turns() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = test_pump_ctx(dir.path(), Some("conn-1".into()), None);
    let mut loop_state = PumpLoopState::new(false);
    loop_state.current_objective = "turn-1".into();
    loop_state.pending_turns.push("turn-2".into());
    loop_state.pending_turns.push("turn-3".into());
    loop_state.turn_reply = Some("API Error: 401 OAuth token has expired".into());

    let mut st = ctx.state.lock().unwrap();
    let blocked = evaluate_turn_failover(
        &mut st,
        &mut loop_state,
        &ctx,
        &org_agent_runtime::TurnOutcome::Completed,
        &[],
    );
    assert!(!blocked);
    assert!(loop_state.stop_requested);
    assert!(loop_state.pending_turns.is_empty());
    let req = loop_state
        .failover_pending
        .as_ref()
        .expect("401 failover must produce FailoverRequest");
    assert_eq!(req.reason, "auth_expired");
    assert_eq!(req.objective, "turn-1\n\nturn-2\n\nturn-3");
    assert_eq!(req.oauth_token_fp.as_deref(), Some("deadbeefcafebabe"));
}

#[test]
fn handle_approval_request_enforces_writable_roots_only_when_sandboxed() {
    let dir = tempfile::tempdir().unwrap();
    let canon = dir.path().canonicalize().unwrap();
    let ws = canon.join("ws");
    let outside = canon.join("outside");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    let ws_spec = org_agent_runtime::SpecPath::try_from(ws.to_str().unwrap()).unwrap();
    let preset_cfg = crate::preset::build_session_policy(
        PermissionPreset::AutonomousSandboxed,
        true,
        &ws_spec,
        None,
        &[],
        1,
    )
    .unwrap();

    let ctx = test_pump_ctx(&canon, None, None);
    let mut st = ctx.state.lock().unwrap();
    st.policy = preset_cfg.policy;
    st.preset = preset_cfg.preset;
    st.allowed_tools = preset_cfg.allowed_tools;
    st.writable_roots = vec![ws_spec];
    st.sandbox_enabled = true;

    let make_req = |p: std::path::PathBuf| {
        let mut req = ApprovalRequest::new(
            ApprovalId::new("a-pump"),
            org_agent_runtime::ApprovalKind::FileChange,
            org_agent_runtime::Escalation::None,
            "Write",
            serde_json::json!({ "file_path": p.to_str().unwrap(), "content": "ok" }),
        );
        req.capability = Some(org_agent_runtime::ToolCapability::Edit);
        req
    };

    // Sandboxed: write inside writable_roots is allowed; write outside is auto-denied.
    let inside_ctrl = handle_approval_request(&mut st, &make_req(ws.join("a.rs")), &[]);
    assert!(matches!(
        inside_ctrl,
        Some(Control::Answer {
            decision: Decision::AllowOnce,
            ..
        })
    ));
    let outside_ctrl = handle_approval_request(&mut st, &make_req(outside.join("b.rs")), &[]);
    assert!(matches!(
        outside_ctrl,
        Some(Control::Answer {
            decision: Decision::Deny { .. },
            ..
        })
    ));

    // Unsandboxed: writable_roots check is skipped so preset allow rule governs.
    st.sandbox_enabled = false;
    let unsandboxed_ctrl = handle_approval_request(&mut st, &make_req(outside.join("b.rs")), &[]);
    assert!(matches!(
        unsandboxed_ctrl,
        Some(Control::Answer {
            decision: Decision::AllowOnce,
            ..
        })
    ));
}

#[test]
fn pump_event_history_and_approval_views_go_through_the_redaction_choke_point() {
    use crate::redact::testing::{assert_no_corpus_secret, secret_corpus};
    use org_agent_runtime::SessionEvent;

    let dir = tempfile::tempdir().unwrap();
    let ctx = test_pump_ctx(dir.path(), None, None);
    let corpus = secret_corpus();

    // `pump/events`: the event copy kept for display and the persisted history line.
    let mut text_event = SessionEvent::AssistantText {
        message_id: Some("m-1".into()),
        text: corpus.text.clone(),
    };
    redact_event_into_history(&ctx, &mut text_event, &[]);
    let SessionEvent::AssistantText { text, .. } = &text_event else {
        unreachable!("the event kind never changes");
    };
    assert_no_corpus_secret(&corpus, text, "pump/events display copy");
    let raw_key = ["corpus", "raw", "key", "0042"].join("-");
    let mut tool_event = SessionEvent::ToolStarted {
        tool_use_id: "toolu-1".into(),
        name: "Bash".into(),
        capability: None,
        input: serde_json::json!({ "command": corpus.text, "api_key": raw_key }),
    };
    redact_event_into_history(&ctx, &mut tool_event, &[]);
    let history = std::fs::read_to_string(ctx.store.history_path("emp-1").unwrap()).unwrap();
    assert_no_corpus_secret(&corpus, &history, "pump/events history.jsonl");
    assert!(!history.contains("corpus-raw-key-0042"), "{history}");
    for line in history.lines() {
        serde_json::from_str::<serde_json::Value>(line)
            .expect("every redacted history line stays valid JSON");
    }

    // `pump/approval`: an owner escalation keeps no secret in its pending-approval view, and an
    // auto-deny keeps none in the message sent back to the runtime.
    let mut st = ctx.state.lock().unwrap();
    let secret_tool = format!("mcp__corpus__{}", corpus.text.replace('\n', " "));
    let mut escalated = ApprovalRequest::new(
        ApprovalId::new("a-redact-1"),
        org_agent_runtime::ApprovalKind::Unknown,
        org_agent_runtime::Escalation::None,
        secret_tool.clone(),
        serde_json::json!({}),
    );
    escalated.capability = Some(org_agent_runtime::ToolCapability::Unknown);
    assert!(handle_approval_request(&mut st, &escalated, &[]).is_none());
    let pending = st.pending_approvals.last().expect("escalated to the owner");
    assert_no_corpus_secret(&corpus, &pending.tool, "pump/approval tool");
    assert_no_corpus_secret(
        &corpus,
        pending.reason.as_deref().unwrap_or_default(),
        "pump/approval reason",
    );

    st.allowed_tools = Some(vec![org_agent_runtime::ToolName::try_from("Read").unwrap()]);
    let denied = ApprovalRequest::new(
        ApprovalId::new("a-redact-2"),
        org_agent_runtime::ApprovalKind::Command,
        org_agent_runtime::Escalation::None,
        secret_tool,
        serde_json::json!({ "command": "ls" }),
    );
    match handle_approval_request(&mut st, &denied, &[]) {
        Some(Control::Answer {
            decision: Decision::Deny { message, .. },
            ..
        }) => assert_no_corpus_secret(
            &corpus,
            message.as_deref().unwrap_or_default(),
            "pump/approval deny message",
        ),
        other => panic!("a tool outside the allowlist must be auto-denied: {other:?}"),
    }
}

#[test]
fn a_partitioned_run_records_its_conversation_usage_and_partition_on_failover() {
    use crate::partition::{PartitionHandle, PartitionKey, PartitionLedger};
    let dir = tempfile::tempdir().unwrap();
    let ledger = PartitionLedger::new();
    let key = PartitionKey::tess_ask("owner-1");
    ledger.begin_turn("emp-1", &key);
    let mut ctx = test_pump_ctx(dir.path(), Some("conn-1".into()), None);
    ctx.partition = Some(PartitionHandle::new(ledger.clone(), "emp-1", key.clone()));
    let mut loop_state = PumpLoopState::new(false);

    // The conversation is the partition's the moment the runtime names it.
    let started = org_agent_runtime::SessionEvent::SessionStarted {
        runtime_session_id: org_agent_runtime::SessionId::try_from("conv-owner").unwrap(),
    };
    super::events::handle_event(&started, &ctx, &mut loop_state);
    let state = ledger.state("emp-1", &key).unwrap();
    assert_eq!(state.runtime_session_id.as_deref(), Some("conv-owner"));

    // Each turn's input is how full the conversation is.
    let mut usage = org_agent_runtime::Usage::new(
        org_agent_runtime::UsageScope::TurnDelta,
        org_agent_runtime::UsageScope::TurnDelta,
    );
    usage.input = 90_000;
    usage.cache_read = 30_000;
    super::events::handle_event(
        &org_agent_runtime::SessionEvent::Usage(usage),
        &ctx,
        &mut loop_state,
    );
    let state = ledger.state("emp-1", &key).unwrap();
    assert_eq!(state.context_tokens, 120_000);
    assert!(state.needs_rotation(), "60% of the assumed window");

    // A failover of the run serves the same partition, so acts for the same caller.
    loop_state.current_objective = "owner turn".into();
    loop_state.turn_reply = Some("API Error: 401 OAuth token has expired".into());
    let mut st = ctx.state.lock().unwrap();
    evaluate_turn_failover(
        &mut st,
        &mut loop_state,
        &ctx,
        &org_agent_runtime::TurnOutcome::Completed,
        &[],
    );
    drop(st);
    let req = loop_state.failover_pending.as_ref().expect("a failover");
    assert_eq!(req.partition, Some(key));
    assert_eq!(req.acting_for_caller_id(), Some("owner-1"));
    let stale = super::commands::build_stale_failover_request(&ctx, "queued".into());
    assert_eq!(stale.acting_for_caller_id(), Some("owner-1"));
}
