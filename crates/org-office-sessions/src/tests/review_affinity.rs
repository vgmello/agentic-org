//! ADR-0028 review wakes, `ReviewReply` deduplication, and two-stage warm/fresh session affinity
//! tests (`TASK-92.1`).

use super::*;
use crate::wake::{
    compose_coalesced_wake_objective, PendingWakeItem, PendingWakeQueueRecord, WakeHoldReason,
    WakeKind, WakeOutcome, WakeSessionAffinity,
};

struct RecordingClaudeRuntime {
    resumed_ids: Arc<Mutex<Vec<Option<String>>>>,
    objectives: Arc<Mutex<Vec<String>>>,
}

struct RecordingSession {
    ready: bool,
    closed: bool,
    objectives: Arc<Mutex<Vec<String>>>,
}

impl Session for RecordingSession {
    fn start(&mut self, _now: Instant) -> Vec<Output> {
        Vec::new()
    }

    fn on_line(&mut self, line: &[u8], _now: Instant) -> Vec<Output> {
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(line) else {
            return Vec::new();
        };
        match v.get("type").and_then(|t| t.as_str()) {
            Some("ready") => {
                self.ready = true;
                let rsid = v
                    .get("session_id")
                    .and_then(|s| s.as_str())
                    .unwrap_or("sess-rec-1");
                vec![
                    Output::Event(SessionEvent::SessionStarted {
                        runtime_session_id: SessionId::try_from(rsid).unwrap(),
                    }),
                    Output::Event(SessionEvent::Ready {
                        runtime_version: Some("1.0.0".into()),
                        pid: None,
                        agents: vec!["reviewer-bot".into()],
                        permission_mode: Some("default".into()),
                    }),
                ]
            }
            Some("reply") => {
                let text = v
                    .get("text")
                    .and_then(|s| s.as_str())
                    .unwrap_or_default()
                    .to_owned();
                vec![
                    Output::Event(SessionEvent::AssistantText {
                        message_id: Some("m1".into()),
                        text,
                    }),
                    Output::Event(SessionEvent::TurnEnded {
                        outcome: TurnOutcome::Completed,
                        errors: Vec::new(),
                    }),
                ]
            }
            _ => Vec::new(),
        }
    }

    fn on_truncated(&mut self, _: usize, _: Instant) -> Vec<Output> {
        Vec::new()
    }
    fn on_stderr_line(&mut self, _: &[u8], _: Instant) -> Vec<Output> {
        Vec::new()
    }
    fn on_stdout_closed(&mut self, _: Instant) -> Vec<Output> {
        Vec::new()
    }
    fn on_exit(&mut self, exit: ExitInfo, _: Instant) -> Vec<Output> {
        if self.closed {
            return Vec::new();
        }
        self.closed = true;
        vec![Output::Event(SessionEvent::Ended { exit })]
    }
    fn on_tick(&mut self, _: Instant) -> Vec<Output> {
        Vec::new()
    }

    fn control(&mut self, control: Control, _now: Instant) -> Result<Vec<Output>, ControlError> {
        match control {
            Control::Send { text, .. } if !self.closed => {
                self.objectives.lock().unwrap().push(text.clone());
                let line = encode_line(&serde_json::json!({ "turn": text }))
                    .map_err(|_| ControlError::Unsupported)?;
                Ok(vec![
                    Output::Event(SessionEvent::TurnStarted {
                        turn_id: Some("t1".into()),
                        solicited: true,
                    }),
                    Output::Write(line),
                ])
            }
            Control::Close => {
                self.closed = true;
                Ok(vec![Output::CloseStdin])
            }
            _ => Ok(Vec::new()),
        }
    }

    fn next_deadline(&self) -> Option<Instant> {
        None
    }
}

impl AgentRuntime for RecordingClaudeRuntime {
    fn id(&self) -> RuntimeId {
        RuntimeId::try_from("claude").unwrap()
    }
    fn aliases(&self) -> &'static [&'static str] {
        &["anthropic", "claude-code"]
    }
    fn binary_name(&self) -> &'static str {
        "recording-claude"
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            resume_ignores_cwd: true,
            resume_applies_instructions: false,
            ..ScriptedRuntime.capabilities()
        }
    }
    fn plan_session(
        &self,
        spec: &SessionSpec,
        ctx: &LaunchContext,
    ) -> Result<(LaunchPlan, Box<dyn Session>), PlanError> {
        self.resumed_ids
            .lock()
            .unwrap()
            .push(ctx.resume.as_ref().map(|s| s.as_str().to_owned()));
        Ok((
            LaunchPlan {
                program: ctx.runtime_path.clone(),
                args: Vec::new(),
                cwd: spec.workspace.clone(),
                stdio: Stdio::Piped,
                files: Vec::new(),
                dirs: Vec::new(),
                env_add: Vec::new(),
                auth_env_names: Vec::new(),
                secret_env_names: Vec::new(),
                stop: StopPlan {
                    steps: vec![
                        StopStep::CloseStdin { grace_ms: 200 },
                        StopStep::Signal {
                            signal: StopSignal::Terminate,
                            target: org_agent_runtime::SignalTarget::Group,
                            grace_ms: 200,
                        },
                    ],
                },
                cleanup_after_exit: Vec::new(),
                runtime_session_id: None,
            },
            Box::new(RecordingSession {
                ready: false,
                closed: false,
                objectives: Arc::clone(&self.objectives),
            }),
        ))
    }
    fn plan_take_over(
        &self,
        _: &SessionSpec,
        _: &LaunchContext,
        _: u16,
        _: u16,
    ) -> Result<LaunchPlan, PlanError> {
        Err(PlanError::Unsupported("no takeover"))
    }
    fn plan_probe(&self, _: Probe, _: &ProbeContext) -> Option<ProbeRun> {
        None
    }
    fn parse_probe(&self, _: Probe, _: &ProbeOutput) -> ProbeResult {
        ProbeResult::Unavailable {
            reason: Unavailable::NotInstalled,
        }
    }
}

fn write_recording_agent_script(dir: &Path, session_id: &str) -> PathBuf {
    let script = dir.join("recording-claude.sh");
    let body = format!(
        "#!/bin/sh\nprintf '{{\"type\":\"ready\",\"session_id\":\"{session_id}\"}}\\n'\nwhile IFS= read -r line; do\n  printf '{{\"type\":\"reply\",\"text\":\"ok\"}}\\n'\ndone\n"
    );
    fs::write(&script, body).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    script
}

struct ReviewManagerFixture {
    _office: Arc<Office>,
    mgr: Arc<SessionManager>,
    owner_id: String,
    emp_id: String,
    resumed_ids: Arc<Mutex<Vec<Option<String>>>>,
    objectives: Arc<Mutex<Vec<String>>>,
}

fn setup_review_manager(
    script_dir: &Path,
    session_id: &str,
    hourly_limit: usize,
) -> ReviewManagerFixture {
    let _ = org_platform::process::harden_self();
    let office: Arc<Office> = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let root_team_id = office.org_snapshot().unwrap().teams[0].id.clone();

    let (emp_id, _conn_id) = hire_agent_with_runtime(
        &office,
        &owner_id,
        &root_team_id,
        "anthropic",
        "claude-code",
        "reviewer-bot",
        "claude-sonnet-4-6",
    );

    let resumed_ids = Arc::new(Mutex::new(Vec::new()));
    let objectives = Arc::new(Mutex::new(Vec::new()));
    let bin_dir = script_dir.join("bin");
    let sock_dir = script_dir.join("sock");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::create_dir_all(&sock_dir).unwrap();
    let script = write_recording_agent_script(&bin_dir, session_id);
    let mut runtimes = RuntimeRegistry::production();
    runtimes.register(
        Arc::new(RecordingClaudeRuntime {
            resumed_ids: Arc::clone(&resumed_ids),
            objectives: Arc::clone(&objectives),
        }),
        Some(script),
    );

    let mut cfg = SessionManagerConfig::production(sock_dir.join("office.sock"));
    cfg.runtimes = runtimes;
    cfg.hourly_wake_limit = hourly_limit;
    let mgr = Arc::new(SessionManager::new(Arc::clone(&office), cfg));
    SessionManager::attach_self(&mgr);
    ReviewManagerFixture {
        _office: office,
        mgr,
        owner_id,
        emp_id,
        resumed_ids,
        objectives,
    }
}

fn wait_for_idle(mgr: &SessionManager, caller: &str, emp_id: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(insp) = mgr.inspect_session(caller, Some(emp_id)) {
            if insp.session.state == SessionState::Idle
                && insp.session.last_reply.is_some()
                && mgr.hibernation.is_turn_idle(emp_id)
            {
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("session for {emp_id} did not settle to Idle");
}

#[test]
fn review_wake_kinds_hints_and_review_reply_deduplication() {
    assert_eq!(WakeKind::ReviewRequested.as_str(), "review_requested");
    assert_eq!(WakeKind::ReviewVerdict.as_str(), "review_verdict");
    assert_eq!(WakeKind::ReviewReply.as_str(), "review_reply");
    assert!(!WakeKind::ReviewRequested.is_mid_turn_eligible());
    assert!(!WakeKind::ReviewVerdict.is_mid_turn_eligible());
    assert!(!WakeKind::ReviewReply.is_mid_turn_eligible());

    let mut q = PendingWakeQueueRecord::empty("emp-1");
    let mk_reply = |id: &str, body: &str, person: bool, att: &str| PendingWakeItem {
        item_id: id.into(),
        kind: WakeKind::ReviewReply,
        sender_id: "u1".into(),
        sender_name: "Alice".into(),
        sender_handle: Some("alice".into()),
        sender_relation: Some("peer".into()),
        sender_is_person: person,
        conversation_id: Some("finding-1".into()),
        source_id: "RV-A1".into(),
        summary: body.into(),
        enqueued_at_ms: 100,
        attachment_ids: vec![att.into()],
        session_affinity: None,
        model_override: None,
        effort_override: None,
    };
    assert!(q.push_dedup(mk_reply("rep-1", "First reply", false, "att-1")));
    // Duplicate item_id ignored.
    assert!(!q.push_dedup(mk_reply("rep-1", "First reply", false, "att-1")));
    // Second reply on the same (source_id, conversation_id) coalesces into ONE queued item!
    assert!(q.push_dedup(mk_reply("rep-2", "Second reply", true, "att-2")));
    assert_eq!(q.items.len(), 1);
    assert!(q.items[0].sender_is_person);
    assert!(q.items[0].summary.contains("First reply"));
    assert!(q.items[0].summary.contains("Second reply"));
    assert_eq!(q.items[0].attachment_ids, vec!["att-1", "att-2"]);

    let obj = compose_coalesced_wake_objective(&q.items, &[]);
    assert!(obj.contains("agent-os review inspect --review RV-A1"));
    assert!(obj.contains("agent-os review finding --review RV-A1 --action reply"));
}

#[test]
fn review_warm_resume_guards_workspace_and_spec_hash_and_honors_fresh() {
    let dir = tempfile::tempdir().unwrap();
    let ReviewManagerFixture {
        _office,
        mgr,
        owner_id,
        emp_id,
        resumed_ids,
        objectives,
    } = setup_review_manager(dir.path(), "sess-review-round-1", 12);

    // Reject contradictory fresh + resume_runtime_session_id.
    let bad = mgr
        .start_session(
            &owner_id,
            &MemberSessionStartParams {
                command_id: new_uuid_v7(),
                employee_id: emp_id.clone(),
                instruction: Some("bad".into()),
                workspace: None,
                resume_runtime_session_id: Some("sess-review-round-1".into()),
                fresh: true,
                model: None,
                effort: None,
            },
        )
        .unwrap_err();
    assert_eq!(bad.code, org_proto::ErrorCode::InvalidRequest);

    // Round 1: initial review wake records runtime_session_id + affinity ledger.
    let r1 = mgr.enqueue_wake(
        &emp_id,
        PendingWakeItem {
            item_id: "rev:RV-A1:r1".into(),
            kind: WakeKind::ReviewRequested,
            sender_id: owner_id.clone(),
            sender_name: "Owner".into(),
            sender_handle: None,
            sender_relation: None,
            sender_is_person: true,
            conversation_id: Some("RV-A1".into()),
            source_id: "RV-A1".into(),
            summary: "Round 1 code review".into(),
            enqueued_at_ms: 1,
            attachment_ids: Vec::new(),
            session_affinity: None,
            model_override: None,
            effort_override: None,
        },
    );
    assert_eq!(r1, WakeOutcome::Started);
    wait_for_idle(&mgr, &owner_id, &emp_id);
    assert_eq!(
        mgr.current_runtime_session_id(&emp_id).as_deref(),
        Some("sess-review-round-1")
    );
    assert_eq!(*resumed_ids.lock().unwrap(), vec![None]);

    // Round 2 (Warm, same workspace & same spec_hash): resumes `sess-review-round-1`!
    let r2 = mgr.enqueue_wake(
        &emp_id,
        PendingWakeItem {
            item_id: "rev:RV-A1:r2".into(),
            kind: WakeKind::ReviewRequested,
            sender_id: owner_id.clone(),
            sender_name: "Owner".into(),
            sender_handle: None,
            sender_relation: None,
            sender_is_person: true,
            conversation_id: Some("RV-A1".into()),
            source_id: "RV-A1".into(),
            summary: "Round 2 inter-round diff".into(),
            enqueued_at_ms: 2,
            attachment_ids: Vec::new(),
            session_affinity: Some(WakeSessionAffinity {
                resume_runtime_session_id: Some("sess-review-round-1".into()),
                fresh: false,
            }),
            model_override: None,
            effort_override: None,
        },
    );
    assert_eq!(r2, WakeOutcome::Started);
    wait_for_idle(&mgr, &owner_id, &emp_id);
    assert_eq!(
        *resumed_ids.lock().unwrap(),
        vec![None, Some("sess-review-round-1".into())]
    );
    let r1_entry = mgr
        .store()
        .lookup_session_affinity(&emp_id, "sess-review-round-1")
        .unwrap()
        .unwrap();

    // Workspace changed: explicit resume MUST fall back to fresh with notice (Claude `resume_ignores_cwd`).
    let alt_ws = dir.path().join("alt-workspace");
    fs::create_dir_all(&alt_ws).unwrap();
    let _ = mgr
        .start_session(
            &owner_id,
            &MemberSessionStartParams {
                command_id: new_uuid_v7(),
                employee_id: emp_id.clone(),
                instruction: Some("Continue in different workspace".into()),
                workspace: Some(alt_ws.display().to_string()),
                resume_runtime_session_id: Some("sess-review-round-1".into()),
                fresh: false,
                model: None,
                effort: None,
            },
        )
        .unwrap();
    wait_for_idle(&mgr, &owner_id, &emp_id);
    assert_eq!(
        resumed_ids.lock().unwrap().last().cloned().flatten(),
        None,
        "workspace change must fall back to fresh session"
    );
    let last_obj = objectives.lock().unwrap().last().cloned().unwrap();
    assert!(
        last_obj.contains("started a fresh session instead of resuming `sess-review-round-1`")
            && last_obj.contains("workspace changed"),
        "expected explicit fallback notice, got: {last_obj}"
    );

    // Spec/instructions changed (same workspace): warm Round 3 MUST fall back to fresh with notice!
    let mut stale_entry = r1_entry;
    stale_entry.spec_hash = "changed-instructions-hash".into();
    mgr.store()
        .record_session_affinity(&emp_id, stale_entry)
        .unwrap();
    let r3 = mgr.enqueue_wake(
        &emp_id,
        PendingWakeItem {
            item_id: "rev:RV-A1:r3".into(),
            kind: WakeKind::ReviewRequested,
            sender_id: owner_id.clone(),
            sender_name: "Owner".into(),
            sender_handle: None,
            sender_relation: None,
            sender_is_person: true,
            conversation_id: Some("RV-A1".into()),
            source_id: "RV-A1".into(),
            summary: "Round 3 after instruction update".into(),
            enqueued_at_ms: 3,
            attachment_ids: Vec::new(),
            session_affinity: Some(WakeSessionAffinity {
                resume_runtime_session_id: Some("sess-review-round-1".into()),
                fresh: false,
            }),
            model_override: None,
            effort_override: None,
        },
    );
    assert_eq!(r3, WakeOutcome::Started);
    wait_for_idle(&mgr, &owner_id, &emp_id);
    assert_eq!(
        resumed_ids.lock().unwrap().last().cloned().flatten(),
        None,
        "spec_hash change must fall back to fresh session"
    );
    let r3_obj = objectives.lock().unwrap().last().cloned().unwrap();
    assert!(
        r3_obj.contains("instructions or execution spec changed"),
        "expected spec fallback notice, got: {r3_obj}"
    );

    // Final validation pass (`--fresh`): forces clean empty session with NO fallback notice.
    let r4 = mgr.enqueue_wake(
        &emp_id,
        PendingWakeItem {
            item_id: "rev:RV-A1:r4-fresh".into(),
            kind: WakeKind::ReviewRequested,
            sender_id: owner_id.clone(),
            sender_name: "Owner".into(),
            sender_handle: None,
            sender_relation: None,
            sender_is_person: true,
            conversation_id: Some("RV-A1".into()),
            source_id: "RV-A1".into(),
            summary: "Final fresh validation pass".into(),
            enqueued_at_ms: 4,
            attachment_ids: Vec::new(),
            session_affinity: Some(WakeSessionAffinity {
                resume_runtime_session_id: None,
                fresh: true,
            }),
            model_override: None,
            effort_override: None,
        },
    );
    assert_eq!(r4, WakeOutcome::Started);
    wait_for_idle(&mgr, &owner_id, &emp_id);
    assert_eq!(resumed_ids.lock().unwrap().last().cloned().flatten(), None);
    let r4_obj = objectives.lock().unwrap().last().cloned().unwrap();
    assert!(!r4_obj.contains("Session notice:"));

    let history = fs::read_to_string(mgr.store().history_path(&emp_id).unwrap()).unwrap();
    assert!(history.contains("resume_fallback_fresh"));
    mgr.stop_all("test");
}

#[test]
fn review_wakes_honor_hourly_quota_and_person_exemption() {
    let dir = tempfile::tempdir().unwrap();
    let ReviewManagerFixture {
        _office,
        mgr,
        owner_id,
        emp_id,
        ..
    } = setup_review_manager(dir.path(), "sess-quota-1", 1);

    let peer_agent_id = new_uuid_v7();
    let first = mgr.enqueue_wake(
        &emp_id,
        PendingWakeItem {
            item_id: "rev:RV-A2:agent-1".into(),
            kind: WakeKind::ReviewRequested,
            sender_id: peer_agent_id.clone(),
            sender_name: "Peer".into(),
            sender_handle: None,
            sender_relation: None,
            sender_is_person: false,
            conversation_id: Some("RV-A2".into()),
            source_id: "RV-A2".into(),
            summary: "Agent review wake 1".into(),
            enqueued_at_ms: 10,
            attachment_ids: Vec::new(),
            session_affinity: None,
            model_override: None,
            effort_override: None,
        },
    );
    assert_eq!(first, WakeOutcome::Started);
    wait_for_idle(&mgr, &owner_id, &emp_id);

    // Second agent wake hits the hourly limit (1/hr in this test).
    let second = mgr.enqueue_wake(
        &emp_id,
        PendingWakeItem {
            item_id: "rev:RV-A2:agent-2".into(),
            kind: WakeKind::ReviewVerdict,
            sender_id: peer_agent_id,
            sender_name: "Peer".into(),
            sender_handle: None,
            sender_relation: None,
            sender_is_person: false,
            conversation_id: Some("RV-A2".into()),
            source_id: "RV-A2".into(),
            summary: "Agent verdict wake 2".into(),
            enqueued_at_ms: 20,
            attachment_ids: Vec::new(),
            session_affinity: None,
            model_override: None,
            effort_override: None,
        },
    );
    assert_eq!(
        second,
        WakeOutcome::Queued {
            reason: WakeHoldReason::HourlyLimit
        }
    );

    // Person-triggered review wake is exempt from the agent hourly limit!
    let person = mgr.enqueue_wake(
        &emp_id,
        PendingWakeItem {
            item_id: "rev:RV-A2:owner-1".into(),
            kind: WakeKind::ReviewReply,
            sender_id: owner_id.clone(),
            sender_name: "Owner".into(),
            sender_handle: None,
            sender_relation: None,
            sender_is_person: true,
            conversation_id: Some("RV-A2".into()),
            source_id: "RV-A2".into(),
            summary: "Owner review reply".into(),
            enqueued_at_ms: 30,
            attachment_ids: Vec::new(),
            session_affinity: None,
            model_override: None,
            effort_override: None,
        },
    );
    assert_eq!(person, WakeOutcome::Started);
    wait_for_idle(&mgr, &owner_id, &emp_id);
    mgr.stop_all("test");
}
