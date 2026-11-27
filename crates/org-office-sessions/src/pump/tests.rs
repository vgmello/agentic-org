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
use super::{PumpContext, PumpHooks, PumpLoopState, QueuedTurn, SessionRuntimeState};

struct NoopAttachmentResolver;

impl crate::turn_attachments::TurnAttachmentResolver for NoopAttachmentResolver {
    fn resolve_for_turn(
        &self,
        _employee_id: &str,
        _delivery_scope: &crate::turn_attachments::TurnDeliveryScope,
        _attachment_id: &str,
        _remaining_staging_budget: u64,
    ) -> Result<
        Result<crate::turn_attachments::TurnDeliveryAttachment, org_proto::ApiError>,
        crate::turn_attachments::TurnAttachmentResolveError,
    > {
        Ok(Err(org_proto::ApiError::new(
            org_proto::ErrorCode::NotFound,
            "not found in pump unit test",
        )))
    }
}

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
        allow_host_auth: false,
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
        queued_turns: 0,
        activity: Default::default(),
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
            is_sandbox_topology_stale: None,
            on_approval_changed: None,
        },
        stop_epoch: 0,
        partition: None,
        stream: None,
        attachments_root: dir.join("attachments"),
        model_supports_vision: true,
        attachment_resolver: Arc::new(NoopAttachmentResolver),
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
    loop_state
        .pending_turns
        .push(QueuedTurn::new("queued-wake".into(), vec!["att-1".into()]));
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
    assert_eq!(req.attachment_ids, vec!["att-1".to_owned()]);
    assert_eq!(req.oauth_token_fp.as_deref(), Some("deadbeefcafebabe"));

    // Non-recycle Stop (e.g. user stop / shutdown) clears failover_pending.
    loop_state
        .pending_turns
        .push(QueuedTurn::text_only("discard-on-user-stop".into()));
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
    loop_state.current_attachment_ids = vec!["att-a".into()];
    loop_state
        .pending_turns
        .push(QueuedTurn::new("turn-2".into(), vec!["att-b".into()]));
    loop_state
        .pending_turns
        .push(QueuedTurn::new("turn-3".into(), vec!["att-a".into()]));
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
    assert_eq!(
        req.attachment_ids,
        vec!["att-a".to_owned(), "att-b".to_owned()]
    );
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
    let stale = super::commands::build_stale_failover_request(&ctx, "queued".into(), Vec::new());
    assert_eq!(stale.acting_for_caller_id(), Some("owner-1"));
}

#[test]
fn a_turn_queued_behind_the_ending_one_keeps_the_session_busy() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = test_pump_ctx(dir.path(), None, None);
    let mut loop_state = PumpLoopState::new(false);
    let mut st = ctx.state.lock().unwrap();
    st.queued_turns = 1;
    super::events::handle_turn_ended(
        &mut st,
        &mut loop_state,
        &ctx,
        &org_agent_runtime::TurnOutcome::Completed,
        &[],
    );
    assert_eq!(st.state, SessionState::Running, "turn two is still queued");
    st.dequeue_turns(1);
    super::events::handle_turn_ended(
        &mut st,
        &mut loop_state,
        &ctx,
        &org_agent_runtime::TurnOutcome::Completed,
        &[],
    );
    assert_eq!(st.state, SessionState::Idle);
}

#[test]
fn queue_turn_counts_and_marks_busy_before_sending_and_undoes_both_for_a_dead_pump() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = test_pump_ctx(dir.path(), None, None);
    ctx.state.lock().unwrap().state = SessionState::Idle;

    let (tx, rx) = std::sync::mpsc::channel();
    assert!(super::queue_turn(&ctx.state, &tx, "one".into()));
    {
        let st = ctx.state.lock().unwrap();
        assert_eq!((st.state, st.queued_turns), (SessionState::Running, 1));
    }
    drop(rx);
    assert!(!super::queue_turn(&ctx.state, &tx, "two".into()));
    {
        // The live turn keeps the session busy; only the undelivered one is forgotten.
        let st = ctx.state.lock().unwrap();
        assert_eq!((st.state, st.queued_turns), (SessionState::Running, 1));
    }

    let idle_ctx = test_pump_ctx(dir.path(), None, None);
    idle_ctx.state.lock().unwrap().state = SessionState::Idle;
    assert!(!super::queue_turn(&idle_ctx.state, &tx, "lost".into()));
    let st = idle_ctx.state.lock().unwrap();
    assert_eq!((st.state, st.queued_turns), (SessionState::Idle, 0));
}

#[test]
fn a_refused_permit_forgets_every_queued_turn_and_settles_idle() {
    let dir = tempfile::tempdir().unwrap();
    let mut ctx = test_pump_ctx(dir.path(), Some("conn-1".into()), None);
    ctx.hooks.on_permit_acquire = Some(Arc::new(|_, _| Err("POOL_EXHAUSTED: [busy]".into())));
    let mut loop_state = PumpLoopState::new(false);
    loop_state.pending_turns = vec![
        QueuedTurn::text_only("a".into()),
        QueuedTurn::text_only("b".into()),
    ];
    ctx.state.lock().unwrap().queued_turns = 2;
    assert!(matches!(
        ensure_permit_for_turn(&ctx, &mut loop_state),
        PermitCheck::Refused
    ));
    assert!(loop_state.pending_turns.is_empty());
    let st = ctx.state.lock().unwrap();
    assert_eq!((st.state, st.queued_turns), (SessionState::Idle, 0));

    // A turn queued since (still on the channel) keeps the session busy for the next dispatch.
    drop(st);
    {
        let mut st = ctx.state.lock().unwrap();
        st.state = SessionState::Running;
        st.queued_turns = 2;
    }
    loop_state.pending_turns = vec![QueuedTurn::text_only("c".into())];
    assert!(matches!(
        ensure_permit_for_turn(&ctx, &mut loop_state),
        PermitCheck::Refused
    ));
    let st = ctx.state.lock().unwrap();
    assert_eq!((st.state, st.queued_turns), (SessionState::Running, 1));
}

fn tool_started(name: &str) -> org_agent_runtime::SessionEvent {
    org_agent_runtime::SessionEvent::ToolStarted {
        tool_use_id: "tu".into(),
        name: name.into(),
        capability: None,
        input: serde_json::json!({ "command": "echo TOPSECRET" }),
    }
}

/// Coarse activity (`TASK-103`): one change per phase, a tool's name only, and a person's
/// interrupt sticks until the turn ends.
#[test]
fn activity_follows_redacted_events_coarsely_and_interrupting_sticks_until_the_turn_ends() {
    use org_agent_runtime::{SessionEvent, TurnOutcome};
    use org_proto::sessions::ActivityPhase;

    use super::activity::next_activity;
    use crate::stream_hub::SessionActivity;

    let idle = SessionActivity {
        supports_steer: true,
        ..SessionActivity::default()
    };
    let started = SessionEvent::TurnStarted {
        turn_id: None,
        solicited: true,
    };
    let thinking = next_activity(&idle, &started, "t0").unwrap();
    assert_eq!(thinking.phase, Some(ActivityPhase::Thinking));
    assert!(thinking.supports_steer);
    assert_eq!(
        next_activity(&thinking, &SessionEvent::Thinking, "t1"),
        None
    );
    let tool = next_activity(&thinking, &tool_started("Bash\n\u{1b}x"), "t2").unwrap();
    assert_eq!(tool.phase, Some(ActivityPhase::RunningTool));
    assert_eq!(tool.active_tool_name.as_deref(), Some("Bash x"));
    assert_eq!(tool.active_tool_started_at.as_deref(), Some("t2"));
    assert!(!format!("{tool:?}").contains("TOPSECRET"));
    let finished = SessionEvent::ToolFinished {
        tool_use_id: "tu".into(),
        is_error: false,
    };
    let back = next_activity(&tool, &finished, "t3").unwrap();
    assert_eq!(back, thinking);
    assert_eq!(next_activity(&back, &finished, "t3"), None);
    let delta = SessionEvent::TextDelta { text: "a".into() };
    let responding = next_activity(&back, &delta, "t4").unwrap();
    assert_eq!(responding.phase, Some(ActivityPhase::Responding));
    assert_eq!(next_activity(&responding, &delta, "t5"), None, "coalesced");

    let interrupting = tool.in_phase(ActivityPhase::Interrupting);
    for event in [
        &delta,
        &SessionEvent::Thinking,
        &tool_started("Read"),
        &finished,
    ] {
        assert_eq!(next_activity(&interrupting, event, "t6"), None);
    }
    let ended = SessionEvent::TurnEnded {
        outcome: TurnOutcome::Interrupted,
        errors: Vec::new(),
    };
    assert_eq!(
        next_activity(&interrupting, &ended, "t7"),
        Some(idle.clone())
    );
    assert_eq!(next_activity(&idle, &ended, "t8"), None);
}

/// Steers run ahead of queued turns, fold into a steer already waiting, and are counted as
/// exactly one queued turn each until they run (`TASK-103`).
#[test]
fn steers_queue_at_the_front_fold_together_and_keep_the_queued_count_exact() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = test_pump_ctx(dir.path(), None, None);
    let mut loop_state = PumpLoopState::new(false);
    let mut st = ctx.state.lock().unwrap();
    st.state = SessionState::Idle;
    // Two ordinary turns and two steers queued, each counted once by its sender.
    st.queued_turns = 4;
    loop_state.pending_turns = vec![
        QueuedTurn::text_only("ordinary-1".into()),
        QueuedTurn::text_only("ordinary-2".into()),
    ];
    loop_state.queue_steer_turn(&mut st, "steer-a".into());
    loop_state.queue_steer_turn(&mut st, "steer-b".into());
    let texts: Vec<&str> = loop_state
        .pending_turns
        .iter()
        .map(|t| t.text.as_str())
        .collect();
    assert_eq!(texts, ["steer-a\n\nsteer-b", "ordinary-1", "ordinary-2"]);
    assert_eq!(st.queued_turns, 3, "the folded steer runs as one turn");

    // A steer the runtime never confirmed is counted again and keeps the session busy.
    loop_state.requeue_steer(&mut st, "steer-c".into());
    assert_eq!(st.queued_turns, 3);
    assert_eq!(st.state, SessionState::Running);
    assert_eq!(
        loop_state.pending_turns[0].text,
        "steer-a\n\nsteer-b\n\nsteer-c"
    );
    drop(st);

    let taken = loop_state.take_queued_turns();
    assert_eq!(taken.len(), 3);
    assert!(!loop_state.front_is_steer);
    // After the queue is taken, a new steer starts a fresh front slot.
    let mut st = ctx.state.lock().unwrap();
    // One ordinary turn and one steer, each counted once.
    st.queued_turns = 2;
    loop_state
        .pending_turns
        .push(QueuedTurn::text_only("ordinary-3".into()));
    loop_state.queue_steer_turn(&mut st, "steer-d".into());
    let texts: Vec<&str> = loop_state
        .pending_turns
        .iter()
        .map(|t| t.text.as_str())
        .collect();
    assert_eq!(texts, ["steer-d", "ordinary-3"]);
    assert_eq!(st.queued_turns, 2);
}

#[test]
fn approval_hook_fires_on_cancel_and_interrupted_turn_clearance() {
    use org_agent_runtime::SessionEvent;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let dir = tempfile::tempdir().unwrap();
    let mut ctx = test_pump_ctx(dir.path(), Some("conn-1".into()), None);
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    ctx.hooks.on_approval_changed = Some(Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    }));
    let mut loop_state = PumpLoopState::new(false);

    let req1 = ApprovalRequest::new(
        ApprovalId::new("a-cancel-1"),
        org_agent_runtime::ApprovalKind::Unknown,
        org_agent_runtime::Escalation::None,
        "mystery_tool",
        serde_json::json!({}),
    );
    let req2 = ApprovalRequest::new(
        ApprovalId::new("a-interrupt-2"),
        org_agent_runtime::ApprovalKind::Unknown,
        org_agent_runtime::Escalation::None,
        "mystery_tool",
        serde_json::json!({}),
    );
    super::events::handle_event(
        &SessionEvent::ApprovalRequested(req1),
        &ctx,
        &mut loop_state,
    );
    super::events::handle_event(
        &SessionEvent::ApprovalRequested(req2),
        &ctx,
        &mut loop_state,
    );
    assert_eq!(hits.load(Ordering::SeqCst), 2);

    // Runtime cancellation clears only the cancelled ID and fires the hook.
    super::events::handle_event(
        &SessionEvent::ApprovalCancelled {
            id: ApprovalId::new("a-cancel-1"),
        },
        &ctx,
        &mut loop_state,
    );
    assert_eq!(hits.load(Ordering::SeqCst), 3);
    assert_eq!(ctx.state.lock().unwrap().pending_approvals.len(), 1);

    // Interrupting the turn clears the remaining approval and fires the hook.
    loop_state.interrupt_sent = true;
    super::events::handle_event(
        &SessionEvent::TurnEnded {
            outcome: org_agent_runtime::TurnOutcome::Interrupted,
            errors: Vec::new(),
        },
        &ctx,
        &mut loop_state,
    );
    assert_eq!(hits.load(Ordering::SeqCst), 4);
    assert!(ctx.state.lock().unwrap().pending_approvals.is_empty());
}
