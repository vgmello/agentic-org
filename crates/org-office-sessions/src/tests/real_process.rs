//! Real-process session RPCs, native adapters, process injection, and task wakes.

use super::*;

#[test]
fn c4_session_list_and_transcript_rpcs_enforce_auth_and_redaction() {
    let office: Arc<Office> = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let snap = office.org_snapshot().unwrap();
    let hr_agent_id = snap
        .members
        .iter()
        .find(|m| m.kind == org_proto::org::MemberKind::Agent)
        .expect("built-in HR agent exists")
        .id
        .clone();

    let home = office.ensure_private_home(&hr_agent_id).unwrap();
    let proj_dir = home.join(".claude/projects/-workspace");
    fs::create_dir_all(&proj_dir).unwrap();
    let sample = [
        r#"{"type":"user","timestamp":"2026-11-22T04:00:00Z","message":{"role":"user","content":"Check sk-ant-api03-SECRETVALUE12345"}}"#,
        r#"{"type":"assistant","timestamp":"2026-11-22T04:00:01Z","message":{"model":"claude-sonnet-4-6","content":[{"type":"text","text":"Checked."},{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"README.md"}}]}}"#,
    ]
    .join("\n");
    fs::write(proj_dir.join("sess-hr-1.jsonl"), sample).unwrap();

    let sock_dir = tempfile::tempdir().unwrap();
    let cfg = SessionManagerConfig::production(sock_dir.path().join("office.sock"));
    let manager = SessionManager::new(Arc::clone(&office), cfg);

    // Owner lists sessions for HR agent.
    let list_req = Request::new::<org_proto::sessions::MemberSessionList>(
        1,
        &org_proto::sessions::MemberSessionListParams {
            employee_id: Some(hr_agent_id.clone()),
            limit: Some(10),
        },
    )
    .unwrap();
    let list_resp = manager.dispatch_rpc(Some(&owner_id), &list_req).unwrap();
    let listed: org_proto::sessions::SessionListResult = match list_resp.outcome {
        org_proto::Outcome::Result(v) => serde_json::from_value(v).unwrap(),
        org_proto::Outcome::Error(e) => panic!("list failed: {e:?}"),
    };
    assert_eq!(listed.sessions.len(), 1);
    assert_eq!(listed.sessions[0].session_id, "sess-hr-1");
    assert!(!listed.sessions[0]
        .preview
        .as_deref()
        .unwrap_or("")
        .contains("SECRETVALUE12345"));

    // Owner reads transcript with toggles.
    let tx_req = Request::new::<org_proto::sessions::MemberSessionTranscript>(
        2,
        &org_proto::sessions::MemberSessionTranscriptParams {
            employee_id: Some(hr_agent_id.clone()),
            session_id: Some("sess-hr-1".into()),
            include_tool_calls: Some(false),
            include_file_changes: Some(true),
            include_subagents: Some(true),
            cursor: Some(0),
            limit: Some(50),
        },
    )
    .unwrap();
    let tx_resp = manager.dispatch_rpc(Some(&owner_id), &tx_req).unwrap();
    let tx: org_proto::sessions::SessionTranscriptResult = match tx_resp.outcome {
        org_proto::Outcome::Result(v) => serde_json::from_value(v).unwrap(),
        org_proto::Outcome::Error(e) => panic!("transcript failed: {e:?}"),
    };
    assert_eq!(tx.session_id, "sess-hr-1");
    assert_eq!(tx.files.len(), 1);
    assert_eq!(tx.files[0].path, "README.md");
    assert!(tx
        .entries
        .iter()
        .all(|e| e.kind != org_proto::sessions::TranscriptEntryKind::ToolCall));
    assert!(!serde_json::to_string(&tx)
        .unwrap()
        .contains("SECRETVALUE12345"));

    // Unbound caller and path-traversal session_id are refused.
    let unbound = manager.dispatch_rpc(None, &tx_req).unwrap();
    assert!(matches!(
        unbound.outcome,
        org_proto::Outcome::Error(e) if e.code == org_proto::ErrorCode::NotAuthorized
    ));
    let bad_sid = Request::new::<org_proto::sessions::MemberSessionTranscript>(
        3,
        &org_proto::sessions::MemberSessionTranscriptParams {
            employee_id: Some(hr_agent_id.clone()),
            session_id: Some("../etc/passwd".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let bad_resp = manager.dispatch_rpc(Some(&owner_id), &bad_sid).unwrap();
    assert!(matches!(
        bad_resp.outcome,
        org_proto::Outcome::Error(e) if e.code == org_proto::ErrorCode::InvalidRequest
    ));

    // Another agent cannot inspect HR's transcript.
    let fake_other_agent = new_uuid_v7();
    let cross = manager
        .dispatch_rpc(Some(&fake_other_agent), &tx_req)
        .unwrap();
    assert!(matches!(
        cross.outcome,
        org_proto::Outcome::Error(e) if e.code == org_proto::ErrorCode::NotAuthorized
    ));
}

#[test]
fn real_process_fake_cli_runs_native_agy_and_codex_adapters_end_to_end() {
    let _ = org_platform::process::harden_self();
    let office: Arc<Office> = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let root_team_id = office.org_snapshot().unwrap().teams[0].id.clone();

    let (agy_id, agy_conn_id) = hire_agent_with_runtime(
        &office,
        &owner_id,
        &root_team_id,
        "google",
        "agy",
        "neo-agy",
        "gemini-2.5-pro",
    );
    let (codex_id, codex_conn_id) = hire_agent_with_runtime(
        &office,
        &owner_id,
        &root_team_id,
        "openai",
        "codex",
        "gpt-codex",
        "gpt-5.4",
    );

    let bin_dir = tempfile::tempdir().unwrap();
    // Because enforces_deny_rules=false and fixed denies exist, workspace_edits narrows
    // to Ask ("ask" on Agy, "workspace-write" + "on-request" on Codex).
    let fake_agy = write_fake_cli(
        bin_dir.path(),
        "agy",
        r#"#!/bin/sh
read init_line
echo '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"name":"antigravity","version":"1.5.17"}}}'
read open_line
echo '{"jsonrpc":"2.0","id":2,"result":{"sessionId":"agy-live-1","models":{"currentModelId":"gemini-2.5-pro"},"modes":{"currentModeId":"default"}}}'
read prompt_line
mkdir -p "$HOME/.gemini/antigravity-cli/brain/agy-live-1/.system_generated/logs"
printf '{"step_index":1,"type":"PLANNER_RESPONSE","created_at":"2026-11-22T06:00:00Z","content":"Transcript leak key=%s"}\n' "$GEMINI_API_KEY" > "$HOME/.gemini/antigravity-cli/brain/agy-live-1/.system_generated/logs/transcript.jsonl"
printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"agy-live-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Agy reply tok=%s key=%s"}}}}\n' "$AGENT_OS_SESSION_TOKEN" "$GEMINI_API_KEY"
echo '{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn","usage":{"inputTokens":20,"outputTokens":10}}}'
"#,
    );
    let fake_codex = write_fake_cli(
        bin_dir.path(),
        "codex",
        r#"#!/bin/sh
read init_line
echo '{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"name":"codex","version":"0.169.0"}}}'
read initialized_line
read thread_line
echo '{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"codex-live-1"},"activePermissionProfile":{"id":"office","extends":null},"sandbox":{"type":"workspaceWrite","writableRoots":[]},"approvalPolicy":{"granular":{"sandbox_approval":false,"rules":true,"mcp_elicitations":false,"request_permissions":false,"skill_approval":false}}}}'
read turn_line
mkdir -p "$HOME/.codex/sessions/2026/11/22"
printf '{"type":"item.completed","item":{"id":"i1","type":"agent_message","text":"Transcript leak key=%s"}}\n' "$OPENAI_API_KEY" > "$HOME/.codex/sessions/2026/11/22/codex-live-1.jsonl"
echo '{"jsonrpc":"2.0","method":"turn/started","params":{"turn":{"id":"t1"}}}'
printf '{"jsonrpc":"2.0","method":"item/completed","params":{"item":{"id":"i1","type":"agentMessage","text":"Codex reply tok=%s key=%s"}}}\n' "$AGENT_OS_SESSION_TOKEN" "$OPENAI_API_KEY"
echo '{"jsonrpc":"2.0","method":"thread/tokenUsage/updated","params":{"tokenUsage":{"last":{"inputTokens":30,"outputTokens":15}}}}'
echo '{"jsonrpc":"2.0","method":"turn/completed","params":{"turn":{"id":"t1","status":"completed"}}}'
"#,
    );

    let mut registry = RuntimeRegistry::production();
    registry.register(
        Arc::new(org_agent_runtime_agy::AgyRuntime::new()),
        Some(fake_agy),
    );
    registry.register(
        Arc::new(org_agent_runtime_codex::CodexRuntime::new()),
        Some(fake_codex),
    );
    let sock_dir = tempfile::tempdir().unwrap();
    let mut cfg = SessionManagerConfig::production(sock_dir.path().join("office.sock"));
    cfg.runtimes = registry;
    cfg.helper_dir = Some(bin_dir.path().to_path_buf());
    let manager = SessionManager::new(Arc::clone(&office), cfg);

    // Verify CLI enrollment aliases ("agy" and "gpt") and credential vault injection.
    let agy_secret = "AIzaSyAgyTestKeySecretValue987654";
    let codex_secret = "sk-proj-CodexTestKeySecretValue987654";
    let agy_enroll = manager
        .enrollment()
        .start(&owner_id, &agy_conn_id, "agy")
        .expect("agy alias enrollment starts");
    assert_eq!(agy_enroll.state, EnrollmentState::AwaitingCode);
    let agy_done = manager
        .enrollment()
        .submit_code(&owner_id, &agy_conn_id, &agy_enroll.login_id, agy_secret)
        .expect("agy credential stored");
    assert_eq!(agy_done.state, EnrollmentState::Completed);

    let codex_enroll = manager
        .enrollment()
        .start(&owner_id, &codex_conn_id, "gpt")
        .expect("gpt alias enrollment starts");
    assert_eq!(codex_enroll.state, EnrollmentState::PendingBrowser);
    let codex_done = manager
        .enrollment()
        .submit_code(
            &owner_id,
            &codex_conn_id,
            &codex_enroll.login_id,
            codex_secret,
        )
        .expect("codex credential stored");
    assert_eq!(codex_done.state, EnrollmentState::Completed);

    for (agent_id, expected_prefix, expected_sid, secret) in [
        (
            &agy_id,
            "Agy reply tok=[REDACTED] key=[REDACTED]",
            "agy-live-1",
            agy_secret,
        ),
        (
            &codex_id,
            "Codex reply tok=[REDACTED] key=[REDACTED]",
            "codex-live-1",
            codex_secret,
        ),
    ] {
        manager
            .start_session(
                &owner_id,
                &MemberSessionStartParams {
                    command_id: new_uuid_v7(),
                    employee_id: agent_id.clone(),
                    instruction: Some("Say hello".into()),
                    workspace: None,
                },
            )
            .expect("session started");
        let reply = wait_for_reply(&manager, &owner_id, agent_id);
        assert!(reply.contains(expected_prefix), "unexpected reply: {reply}");
        assert!(!reply.contains(secret), "secret leaked in reply: {reply}");
        let stored = manager.store().read_active_turn(agent_id).unwrap().unwrap();
        assert_eq!(stored.runtime_session_id.as_deref(), Some(expected_sid));

        // Verify C4 transcript list + read with secret redaction for both Agy and Codex.
        let list = manager
            .list_sessions(
                &owner_id,
                &org_proto::sessions::MemberSessionListParams {
                    employee_id: Some(agent_id.clone()),
                    limit: Some(10),
                },
            )
            .expect("list_sessions");
        assert!(
            list.sessions.iter().any(|s| s.session_id == expected_sid),
            "expected {expected_sid} in {:?}",
            list.sessions
        );
        let tx = manager
            .read_session_transcript(
                &owner_id,
                &org_proto::sessions::MemberSessionTranscriptParams {
                    employee_id: Some(agent_id.clone()),
                    session_id: Some(expected_sid.into()),
                    include_tool_calls: Some(true),
                    include_file_changes: Some(true),
                    include_subagents: Some(true),
                    cursor: None,
                    limit: Some(50),
                },
            )
            .expect("read_session_transcript");
        let combined: String = tx
            .entries
            .iter()
            .filter_map(|e| e.text.as_deref())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            combined.contains("Transcript leak key=[REDACTED]"),
            "unexpected transcript text: {combined}"
        );
        assert!(
            !combined.contains(secret),
            "secret leaked in transcript: {combined}"
        );

        let _ = manager.stop_session(agent_id, "done");
    }
}

#[test]
fn session_launch_injects_effective_processes_and_refuses_mandatory_budget_overflow() {
    use org_proto::processes::{
        DocumentKindWire, DocumentStatusWire, ProcessScopeWire, ProcessWriteParams,
    };

    let _ = org_platform::process::harden_self();
    let office: Arc<Office> = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let root_team_id = office.org_snapshot().unwrap().teams[0].id.clone();

    let (agent_id, conn_id) = hire_agent_with_runtime(
        &office,
        &owner_id,
        &root_team_id,
        "google",
        "agy",
        "proc-bot",
        "gemini-2.5-pro",
    );

    // Seed a non-overridable org standard and an overridable guidance process.
    office
        .process_write(
            &owner_id,
            ProcessWriteParams {
                command_id: new_uuid_v7(),
                document_id: None,
                kind: DocumentKindWire::Standard,
                scope: ProcessScopeWire::Organization,
                key: Some("no-secrets".into()),
                title: "Never Log Secrets".into(),
                status: DocumentStatusWire::Current,
                owner_employee_id: Some(owner_id.clone()),
                overridable: false,
                body_markdown: "MANDATORY_SENTINEL: Never print API keys in stdout.".into(),
                expected_revision: None,
            },
        )
        .unwrap()
        .unwrap();

    office
        .process_write(
            &owner_id,
            ProcessWriteParams {
                command_id: new_uuid_v7(),
                document_id: None,
                kind: DocumentKindWire::Process,
                scope: ProcessScopeWire::Organization,
                key: Some("pr-style".into()),
                title: "PR Style Guide".into(),
                status: DocumentStatusWire::Current,
                owner_employee_id: Some(owner_id.clone()),
                overridable: true,
                body_markdown: "GUIDANCE_SENTINEL: Keep PRs concise.".into(),
                expected_revision: None,
            },
        )
        .unwrap()
        .unwrap();

    // Verify rendered instructions include mandatory standard in full and overridable guidance.
    let rendered = office
        .render_agent_process_instructions(
            &agent_id,
            None,
            None,
            std::collections::BTreeMap::new(),
            128 * 1024,
        )
        .unwrap()
        .unwrap();
    assert!(
        rendered
            .text
            .contains("MANDATORY_SENTINEL: Never print API keys in stdout."),
        "mandatory non-overridable standard missing: {}",
        rendered.text
    );
    assert!(
        rendered.text.contains("PR Style Guide"),
        "overridable guidance missing: {}",
        rendered.text
    );

    // Prepare a real SessionManager with a stub agy binary + enrolled credential.
    let tmp = tempfile::tempdir().unwrap();
    let capture_path = tmp.path().join("captured-init.txt");
    let fake_agy = write_fake_cli(
        tmp.path(),
        "agy",
        &format!(
            r#"#!/bin/sh
read init_line
echo '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":1,"agentInfo":{{"name":"antigravity","version":"1.5.17"}}}}}}'
read open_line
printf '%s\n' "$open_line" > "{capture}"
echo '{{"jsonrpc":"2.0","id":2,"result":{{"sessionId":"agy-proc-1","models":{{"currentModelId":"gemini-2.5-pro"}},"modes":{{"currentModeId":"default"}}}}}}'
read prompt_line
echo '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"agy-proc-1","update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"Ack"}}}}}}}}'
echo '{{"jsonrpc":"2.0","id":3,"result":{{"stopReason":"end_turn","usage":{{"inputTokens":5,"outputTokens":2}}}}}}'
"#,
            capture = capture_path.display()
        ),
    );
    let mut registry = RuntimeRegistry::production();
    registry.register(
        Arc::new(org_agent_runtime_agy::AgyRuntime::new()),
        Some(fake_agy),
    );
    let mut cfg = SessionManagerConfig::production(tmp.path().join("office.sock"));
    cfg.runtimes = registry;
    cfg.helper_dir = Some(tmp.path().to_path_buf());
    let manager = SessionManager::new(Arc::clone(&office), cfg);

    // Enroll API key credential for the agent's connection so start_session reaches build_instructions.
    let enrolled = manager
        .enrollment()
        .start(&owner_id, &conn_id, "agy")
        .unwrap();
    manager
        .enrollment()
        .submit_code(
            &owner_id,
            &conn_id,
            &enrolled.login_id,
            "test-gemini-key-for-c5",
        )
        .unwrap();

    // 1. Live launch succeeds and injects the mandatory standard into the runtime's instructions!
    manager
        .start_session(
            &owner_id,
            &MemberSessionStartParams {
                command_id: new_uuid_v7(),
                employee_id: agent_id.clone(),
                instruction: Some("Hello".into()),
                workspace: None,
            },
        )
        .expect("session starts with effective processes");
    let _ = wait_for_reply(&manager, &owner_id, &agent_id);
    let _ = manager.stop_session(&agent_id, "check capture");
    let captured = fs::read_to_string(&capture_path).expect("captured init");
    assert!(
        captured.contains("MANDATORY_SENTINEL: Never print API keys in stdout."),
        "launched runtime did not receive mandatory org standard in instructions: {captured}"
    );
    assert!(
        captured.contains("PR Style Guide"),
        "launched runtime did not receive overridable guidance in instructions: {captured}"
    );

    // 2. Seed a huge non-overridable standard that overflows the 128 KiB launch budget.
    office
        .process_write(
            &owner_id,
            ProcessWriteParams {
                command_id: new_uuid_v7(),
                document_id: None,
                kind: DocumentKindWire::Standard,
                scope: ProcessScopeWire::Organization,
                key: Some("huge-mandatory-policy".into()),
                title: "Oversized Mandatory Policy".into(),
                status: DocumentStatusWire::Current,
                owner_employee_id: Some(owner_id.clone()),
                overridable: false,
                body_markdown: "X".repeat(135 * 1024),
                expected_revision: None,
            },
        )
        .unwrap()
        .unwrap();

    // Launch MUST fail closed when mandatory non-overridable standards exceed budget (doc-34 Decision 4).
    let err = manager
        .start_session(
            &owner_id,
            &MemberSessionStartParams {
                command_id: new_uuid_v7(),
                employee_id: agent_id,
                instruction: Some("Start".into()),
                workspace: None,
            },
        )
        .expect_err("launch must refuse when mandatory standards overflow budget");
    assert_eq!(err.code, org_proto::ErrorCode::InvalidRequest);
    assert!(
        err.message.contains("Oversized Mandatory Policy")
            || err.message.contains("huge-mandatory-policy"),
        "refusal should name offending mandatory doc: {}",
        err.message
    );
}

/// The `_meta.antigravity.systemPrompt.append` of a captured Agy `session/new|resume` line.
fn captured_system_prompt(open_line: &str) -> String {
    let open: serde_json::Value = serde_json::from_str(open_line.trim()).expect("open line");
    open["params"]["_meta"]["antigravity"]["systemPrompt"]["append"]
        .as_str()
        .expect("system prompt")
        .to_owned()
}

/// The text of a captured Agy `session/prompt` line.
fn captured_turn_text(prompt_line: &str) -> String {
    let prompt: serde_json::Value = serde_json::from_str(prompt_line.trim()).expect("prompt line");
    prompt["params"]["prompt"]
        .as_array()
        .expect("prompt blocks")
        .iter()
        .filter_map(|b| b["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn task_assignment_wake_injects_project_owning_chain_for_contributing_agent() {
    let _ = org_platform::process::harden_self();
    let office: Arc<Office> = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let root_team_id = office.org_snapshot().unwrap().teams[0].id.clone();
    let (contrib_agent_id, conn_id, task_id, task_title) =
        seed_project_task_wake_fixture(&office, &owner_id, &root_team_id);

    let tmp = tempfile::tempdir().unwrap();
    let open_capture = tmp.path().join("captured-open.txt");
    let turn_capture = tmp.path().join("captured-turn.txt");
    let fake_agy = write_fake_cli(
        tmp.path(),
        "agy",
        &format!(
            r#"#!/bin/sh
read init_line
echo '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":1,"agentInfo":{{"name":"antigravity","version":"1.5.17"}}}}}}'
read -r open_line
printf '%s\n' "$open_line" > "{open}"
echo '{{"jsonrpc":"2.0","id":2,"result":{{"sessionId":"agy-task-1","models":{{"currentModelId":"gemini-2.5-pro"}},"modes":{{"currentModeId":"default"}}}}}}'
read -r prompt_line
printf '%s\n' "$prompt_line" > "{turn}"
echo '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"agy-task-1","update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"Audited"}}}}}}}}'
echo '{{"jsonrpc":"2.0","id":3,"result":{{"stopReason":"end_turn","usage":{{"inputTokens":5,"outputTokens":2}}}}}}'
"#,
            open = open_capture.display(),
            turn = turn_capture.display()
        ),
    );
    let mut registry = RuntimeRegistry::production();
    registry.register(
        Arc::new(org_agent_runtime_agy::AgyRuntime::new()),
        Some(fake_agy),
    );
    let mut cfg = SessionManagerConfig::production(tmp.path().join("office.sock"));
    cfg.runtimes = registry;
    cfg.helper_dir = Some(tmp.path().to_path_buf());
    cfg.command_catalogue = Some(org_agent_cli::table);
    let manager = SessionManager::new(Arc::clone(&office), cfg);

    let enrolled = manager
        .enrollment()
        .start(&owner_id, &conn_id, "agy")
        .unwrap();
    manager
        .enrollment()
        .submit_code(
            &owner_id,
            &conn_id,
            &enrolled.login_id,
            "test-key-task-wake",
        )
        .unwrap();

    // 1. A plain start: the standing prompt carries the home team chain only.
    manager
        .start_session(
            &owner_id,
            &MemberSessionStartParams {
                command_id: new_uuid_v7(),
                employee_id: contrib_agent_id.clone(),
                instruction: Some("Say hello.".into()),
                workspace: None,
            },
        )
        .expect("plain start");
    let _ = wait_for_reply(&manager, &owner_id, &contrib_agent_id);
    let _ = manager.stop_session(&contrib_agent_id, "done");
    let plain_prompt =
        captured_system_prompt(&fs::read_to_string(&open_capture).expect("plain open"));
    let plain_turn = captured_turn_text(&fs::read_to_string(&turn_capture).expect("plain turn"));
    assert!(
        plain_turn.contains("kind=\"direct_instruction\""),
        "{plain_turn}"
    );
    assert!(plain_turn.contains("[verified: owner]"), "{plain_turn}");

    // 2. A task wake for the project task the agent's team only contributes to.
    let outcome = manager.on_task_assigned(
        &contrib_agent_id,
        &owner_id,
        "Owner",
        true,
        &task_id,
        &task_title,
    );
    assert_eq!(outcome, crate::WakeOutcome::Started);
    // The first run's reply is still the last one: wait for the task turn itself.
    let deadline = Instant::now() + Duration::from_secs(10);
    while !fs::read_to_string(&turn_capture)
        .is_ok_and(|turn| turn.contains("kind=\\\"assignment\\\""))
    {
        assert!(
            Instant::now() < deadline,
            "the task wake turn never reached the runtime"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    let _ = manager.stop_session(&contrib_agent_id, "done");
    let task_prompt =
        captured_system_prompt(&fs::read_to_string(&open_capture).expect("task open"));
    let task_turn = captured_turn_text(&fs::read_to_string(&turn_capture).expect("task turn"));

    // The task never changes the standing prompt: it is byte-identical to the plain start's,
    // starts with the release-static built-in text, and names the home team, not its ID.
    assert_eq!(
        task_prompt, plain_prompt,
        "per-wake task_ref must not change the prefix"
    );
    assert!(task_prompt.starts_with(&crate::instructions::builtin_instructions()));
    assert!(
        task_prompt.contains("Home team: QA Guild."),
        "{task_prompt}"
    );
    assert!(
        task_prompt.contains("CONTRIB_TEAM_MUST_NOT_APPEAR"),
        "standing home team chain"
    );
    assert!(
        !task_prompt.contains("OWNING_TEAM_MANDATORY_SENTINEL"),
        "{task_prompt}"
    );
    assert!(
        !task_prompt.contains("PROJECT_MANDATORY_SENTINEL"),
        "{task_prompt}"
    );

    // The turn carries the task's chain after its objective, and names the standing document
    // that does not apply to this task without repeating its body.
    let objective_at = task_turn
        .find("UNTRUSTED_MESSAGE")
        .expect("fenced objective");
    let delta_at = task_turn.find("## Processes for task").expect("task delta");
    assert!(
        objective_at < delta_at,
        "the delta follows the objective: {task_turn}"
    );
    assert!(
        task_turn.contains("OWNING_TEAM_MANDATORY_SENTINEL"),
        "project owning team standard missing on task wake: {task_turn}"
    );
    assert!(
        task_turn.contains("PROJECT_MANDATORY_SENTINEL"),
        "project mandatory standard missing on task wake: {task_turn}"
    );
    assert!(
        task_turn.contains("do not apply to this task:\n- QA Guild Rule"),
        "{task_turn}"
    );
    assert!(
        !task_turn.contains("CONTRIB_TEAM_MUST_NOT_APPEAR"),
        "contributing team standard leaked into project task wake: {task_turn}"
    );
}
