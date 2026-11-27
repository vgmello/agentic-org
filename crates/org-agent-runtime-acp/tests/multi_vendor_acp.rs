//! Multi-vendor conformance, permission-mode enforcement, sandbox-bypass denial, system-prompt
//! delivery, ProbeSession, host credential hygiene, and transcript parser tests covering all
//! four ACP runtimes (`agy`, `gemini`, `kimi`, `pi`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Instant;

use org_agent_runtime::testkit::{self, Script, ScriptApproval};
use org_agent_runtime::transcript::TranscriptDiscoveryLayout;
use org_agent_runtime::{
    AgentIdentity, AgentName, AgentRuntime, ApprovalId, ApprovalKind, Control, Decision, EnvName,
    Escalation, LaunchContext, McpServer, McpServerName, Output, PathPattern, PermissionMode,
    PermissionPolicy, PlainId, PlanError, Probe, ProbeContext, ProbeOutput, ProbeResult, ProbeRun,
    ProbeStep, RuntimeId, SandboxSpec, SessionEvent, SessionId, SessionKind, SessionSpec, SpecPath,
    StartFailure, Token, ToolName, Unavailable,
};
use org_agent_runtime_acp::{AgyRuntime, GeminiRuntime, KimiRuntime, PiRuntime};

fn sp(s: &str) -> SpecPath {
    SpecPath::try_from(s).unwrap()
}

fn pat(s: &str) -> PathPattern {
    PathPattern::try_from(s).unwrap()
}

fn vendor_spec(runtime: &str, mode: PermissionMode) -> SessionSpec {
    let mut mcp_servers = BTreeMap::new();
    mcp_servers.insert(
        McpServerName::try_from("office").unwrap(),
        McpServer {
            command: sp("/office/bin/org-mcp"),
            args: vec!["--stdio".to_owned()],
        },
    );
    SessionSpec {
        runtime: RuntimeId::try_from(runtime).unwrap(),
        kind: SessionKind::Task {
            task_id: PlainId::try_from("tacp1").unwrap(),
        },
        agent: AgentIdentity {
            name: AgentName::try_from("acp-dev").unwrap(),
            description: "ACP engineer".to_owned(),
            prompt: "You are an ACP engineer.".to_owned(),
        },
        model: Some(Token::try_from("vendor-model-1").unwrap()),
        effort: None,
        workspace: sp("/work/t-acp"),
        read_only_dirs: vec![sp("/homes/acp-dev")],
        instructions: "Follow workspace security rules.".to_owned(),
        repository_instructions: String::new(),
        permission_mode: mode,
        policy: PermissionPolicy {
            version: 1,
            allow: Vec::new(),
            deny: Vec::new(),
        },
        sandbox: SandboxSpec {
            enabled: true,
            writable_roots: vec![sp("/work/t-acp")],
            deny_write: vec![pat("/work/t-acp/.git/hooks/**")],
            deny_read: vec![pat("/office/offices/*/**")],
            allowed_unix_sockets: Vec::new(),
        },
        mcp_servers,
        tools: Some(vec![
            ToolName::try_from("cap:read").unwrap(),
            ToolName::try_from("cap:edit").unwrap(),
            ToolName::try_from("cap:shell").unwrap(),
        ]),
        operator_shell_prefixes: Vec::new(),
    }
}

fn vendor_ctx(binary: &str, resume: Option<&str>) -> LaunchContext {
    LaunchContext {
        runtime_path: PathBuf::from(format!("/usr/local/bin/{binary}")),
        runtime_version: None,
        runtime_dir: sp("/office/agents/acp-dev/runtime"),
        tmpdir: sp("/private/tmp/agentic-org/run-acp"),
        office_env_names: vec![EnvName::try_from("AGENT_OS_SESSION_TOKEN").unwrap()],
        new_session_id: None,
        resume: resume.map(|s| SessionId::try_from(s).unwrap()),
        config_dir: None,
    }
}

fn standard_acp_script(vendor_name: &str, version: &str) -> Script {
    let init_ok = format!(
        r#"{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":1,"agentInfo":{{"name":"{vendor_name}","version":"{version}"}}}}}}"#
    )
    .into_bytes();
    let session_ok = br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"sess-acp-1","models":{"currentModelId":"vendor-model-1"},"modes":{"currentModeId":"default"}}}"#.to_vec();
    let chunk = br#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sess-acp-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Done."}}}}"#.to_vec();
    let approval_req = br#"{"jsonrpc":"2.0","id":"perm-1","method":"session/request_permission","params":{"sessionId":"sess-acp-1","toolCall":{"toolCallId":"tc-1","name":"run_shell_command","title":"Run ls","rawInput":{"command":"ls"}},"options":[{"optionId":"allow","name":"Allow","kind":"allow_once"},{"optionId":"deny","name":"Deny","kind":"reject_once"}]}}"#.to_vec();
    let approval_cancel =
        br#"{"jsonrpc":"2.0","method":"$/cancel_request","params":{"id":"perm-1"}}"#.to_vec();
    let prompt3_done = br#"{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn","usage":{"inputTokens":40,"outputTokens":10}}}"#.to_vec();
    let prompt4_done = br#"{"jsonrpc":"2.0","id":4,"result":{"stopReason":"end_turn","usage":{"inputTokens":20,"outputTokens":5}}}"#.to_vec();

    let mut approval = ScriptApproval::new(approval_req, ApprovalId::new("perm-1"));
    approval.cancel = Some(approval_cancel);
    Script {
        opening: vec![init_ok, session_ok],
        turn: vec![chunk],
        turn_end: vec![prompt3_done.clone(), prompt4_done],
        stderr: vec![format!("{vendor_name} diagnostic").into_bytes()],
        approval: Some(approval),
        mid_turn_step: Some(prompt3_done),
    }
}

fn all_vendors() -> [(
    &'static dyn AgentRuntime,
    &'static str,
    &'static str,
    &'static str,
); 4] {
    [
        (&AgyRuntime, "agy", "agy", "1.5.17"),
        (&GeminiRuntime, "gemini", "gemini", "0.24.0"),
        (&KimiRuntime, "kimi", "kimi", "0.9.0"),
        (&PiRuntime, "pi", "pi-acp", "0.5.0"),
    ]
}

#[test]
fn all_four_acp_runtimes_pass_org_agent_runtime_testkit_conformance() {
    for (rt, id, bin, ver) in all_vendors() {
        assert_eq!(rt.id().as_str(), id);
        assert_eq!(rt.binary_name(), bin);
        let spec = vendor_spec(id, PermissionMode::Ask);
        let ctx = vendor_ctx(bin, None);
        let caps = rt.capabilities();
        let (plan, _) = rt
            .plan_session(&spec, &ctx)
            .unwrap_or_else(|e| panic!("{id}: plan_session failed: {e}"));
        testkit::check_plan(&plan, &spec, &ctx, &caps);
        let script = standard_acp_script(id, ver);
        testkit::check_all(
            &mut || rt.plan_session(&spec, &ctx).expect("plan_session").1,
            &script,
            &caps,
            Instant::now(),
        );
    }
}

#[test]
fn all_four_acp_runtimes_fail_closed_on_banned_missing_and_drifting_permission_modes() {
    let now = Instant::now();
    for (rt, id, bin, ver) in all_vendors() {
        let spec = vendor_spec(id, PermissionMode::Ask);
        let ctx = vendor_ctx(bin, None);

        // 1. Missing `modes.currentModeId` fails closed with ModeMismatch.
        let (_, mut missing_session) = rt.plan_session(&spec, &ctx).unwrap();
        let _ = missing_session.start(now);
        let _ = missing_session.on_line(
            format!(r#"{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":1,"agentInfo":{{"version":"{ver}"}}}}}}"#).as_bytes(),
            now,
        );
        let res = missing_session.on_line(
            br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"s-1","models":{"currentModelId":"vendor-model-1"}}}"#,
            now,
        );
        assert!(
            res.iter().any(|o| matches!(
                o,
                Output::Event(SessionEvent::StartFailed {
                    reason: StartFailure::ModeMismatch { reported, .. }
                }) if reported == "<missing>"
            )),
            "{id}: expected ModeMismatch on missing mode, got {res:?}"
        );

        // 2. Banned bypass modes fail closed with ModeMismatch + CloseStdin.
        for banned in [
            "yolo",
            "auto",
            "full-auto",
            "dontAsk",
            "bypassPermissions",
            "danger-full-access",
            "allow-all",
        ] {
            let (_, mut s) = rt.plan_session(&spec, &ctx).unwrap();
            let _ = s.start(now);
            let _ = s.on_line(
                format!(r#"{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":1,"agentInfo":{{"version":"{ver}"}}}}}}"#).as_bytes(),
                now,
            );
            let out = s.on_line(
                format!(r#"{{"jsonrpc":"2.0","id":2,"result":{{"sessionId":"s-1","modes":{{"currentModeId":"{banned}"}}}}}}"#).as_bytes(),
                now,
            );
            assert!(
                out.iter().any(|o| matches!(
                    o,
                    Output::Event(SessionEvent::StartFailed {
                        reason: StartFailure::ModeMismatch { reported, .. }
                    }) if reported == banned
                )) && out.contains(&Output::CloseStdin),
                "{id}: banned mode `{banned}` did not fail closed with CloseStdin: {out:?}"
            );
        }

        // 3. Post-opening mode drift stops session immediately (`session/cancel` + `CloseStdin`).
        let (_, mut drift) = rt.plan_session(&spec, &ctx).unwrap();
        let _ = drift.start(now);
        let _ = drift.on_line(
            format!(r#"{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":1,"agentInfo":{{"version":"{ver}"}}}}}}"#).as_bytes(),
            now,
        );
        let _ = drift.on_line(
            br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"s-1","models":{"currentModelId":"vendor-model-1"},"modes":{"currentModeId":"default"}}}"#,
            now,
        );
        let out = drift.on_line(
            br#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"sessionUpdate":"current_mode_update","currentModeId":"yolo"}}}"#,
            now,
        );
        assert!(
            out.contains(&Output::CloseStdin),
            "{id}: drift to yolo must close stdin: {out:?}"
        );
        assert!(
            out.iter().any(|o| matches!(o, Output::Write(line) if String::from_utf8_lossy(line.as_bytes()).contains("session/cancel"))),
            "{id}: drift to yolo must emit session/cancel: {out:?}"
        );
    }
}

#[test]
fn all_four_acp_runtimes_classify_bypass_sandbox_and_never_select_allow_always() {
    let now = Instant::now();
    for (rt, id, bin, ver) in all_vendors() {
        let spec = vendor_spec(id, PermissionMode::Ask);
        let ctx = vendor_ctx(bin, None);
        let (_, mut s) = rt.plan_session(&spec, &ctx).unwrap();
        let _ = s.start(now);
        let _ = s.on_line(
            format!(r#"{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":1,"agentInfo":{{"version":"{ver}"}}}}}}"#).as_bytes(),
            now,
        );
        let _ = s.on_line(
            br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"s-1","models":{"currentModelId":"vendor-model-1"},"modes":{"currentModeId":"default"}}}"#,
            now,
        );
        let _ = s
            .control(
                Control::Send {
                    text: "Run check".into(),
                    images: Vec::new(),
                },
                now,
            )
            .unwrap();

        // Deeply nested BypassSandbox / dangerouslyDisableSandbox must trigger SandboxBypass.
        for (req_id, payload) in [
            ("b1", r#"{"command":"id","BypassSandbox":true}"#),
            ("b2", r#"{"nested":{"dangerously_disable_sandbox":"yes"}}"#),
            ("b3", r#"{"opts":[{"unsandboxed":1}]}"#),
        ] {
            let line = format!(
                r#"{{"jsonrpc":"2.0","id":"{req_id}","method":"session/request_permission","params":{{"sessionId":"s-1","toolCall":{{"toolCallId":"tc-{req_id}","name":"bash","rawInput":{payload}}},"options":[{{"optionId":"allow","kind":"allow_once"}},{{"optionId":"deny","kind":"reject_once"}}]}}}}"#
            );
            let out = s.on_line(line.as_bytes(), now);
            assert!(
                out.iter().any(|o| matches!(
                    o,
                    Output::Event(SessionEvent::ApprovalRequested(req))
                        if req.escalation == Escalation::SandboxBypass
                            && req.kind == ApprovalKind::Command
                )),
                "{id}/{req_id}: expected Escalation::SandboxBypass, got {out:?}"
            );
            let deny_out = s
                .control(
                    Control::Answer {
                        request_id: ApprovalId::new(req_id),
                        decision: Decision::Deny {
                            message: Some("sandbox bypass forbidden".into()),
                            stop_turn: false,
                        },
                    },
                    now,
                )
                .unwrap();
            assert!(deny_out.iter().any(|o| matches!(
                o,
                Output::Write(l) if String::from_utf8_lossy(l.as_bytes()).contains(r#""optionId":"deny""#)
            )));
        }

        // Only `allow_always` offered -> `Decision::AllowOnce` fails closed with `cancelled`.
        let always_only = br#"{"jsonrpc":"2.0","id":"always-1","method":"session/request_permission","params":{"sessionId":"s-1","toolCall":{"toolCallId":"tc-a","name":"read_file","rawInput":{"path":"/work/a"}},"options":[{"optionId":"perm-always","kind":"allow_always"},{"optionId":"perm-deny","kind":"reject_once"}]}}"#;
        let _ = s.on_line(always_only, now);
        let ans = s
            .control(
                Control::Answer {
                    request_id: ApprovalId::new("always-1"),
                    decision: Decision::AllowOnce,
                },
                now,
            )
            .unwrap();
        assert!(
            ans.iter().any(|o| matches!(
                o,
                Output::Write(l) if String::from_utf8_lossy(l.as_bytes()).contains(r#""outcome":"cancelled""#)
            )),
            "{id}: allow_always-only request must fail closed with cancelled: {ans:?}"
        );
    }
}

#[test]
fn vendor_specific_system_prompt_delivery_resume_methods_and_mode_switching() {
    let now = Instant::now();

    // Gemini stages `gemini-system.md` + `GEMINI_SYSTEM_MD`, resumes via `session/load`, and
    // maps AcceptEdits -> `auto_edit` (when no undropped deny narrows effective mode back to Ask).
    let mut g_spec = vendor_spec("gemini", PermissionMode::AcceptEdits);
    g_spec.sandbox.deny_write.clear();
    g_spec.sandbox.deny_read.clear();
    g_spec.tools = None;
    let g_ctx = vendor_ctx("gemini", Some("g-prev-1"));
    let (g_plan, mut g_sess) = GeminiRuntime.plan_session(&g_spec, &g_ctx).unwrap();
    assert_eq!(g_plan.files.len(), 1);
    assert_eq!(g_plan.files[0].rel_path.as_str(), "gemini-system.md");
    let _ = g_sess.start(now);
    let g_open = g_sess.on_line(
        br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"version":"0.24.0"}}}"#,
        now,
    );
    let g_open_str = match &g_open[0] {
        Output::Write(l) => String::from_utf8_lossy(l.as_bytes()).into_owned(),
        other => panic!("expected Write, got {other:?}"),
    };
    let g_open_val: serde_json::Value = serde_json::from_str(&g_open_str).unwrap();
    assert_eq!(g_open_val["method"], "session/load");
    assert_eq!(g_open_val["params"]["sessionId"], "g-prev-1");
    assert!(g_open_val["params"].get("_meta").is_none());
    // Opens in default -> must send `session/set_mode` to `auto_edit`.
    let g_mode = g_sess.on_line(
        br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"g-prev-1","models":{"currentModelId":"vendor-model-1"},"modes":{"currentModeId":"default"}}}"#,
        now,
    );
    let g_mode_str = match &g_mode[0] {
        Output::Write(l) => String::from_utf8_lossy(l.as_bytes()).into_owned(),
        other => panic!("expected set_mode Write, got {other:?}"),
    };
    let g_mode_val: serde_json::Value = serde_json::from_str(&g_mode_str).unwrap();
    assert_eq!(g_mode_val["method"], "session/set_mode");
    assert_eq!(g_mode_val["params"]["modeId"], "auto_edit");

    // Kimi & Pi inject standing instructions as a leading prompt block on turn 1 only, and resume
    // via `session/load`.
    for (rt, id, bin) in [
        (&KimiRuntime as &dyn AgentRuntime, "kimi", "kimi"),
        (&PiRuntime as &dyn AgentRuntime, "pi", "pi-acp"),
    ] {
        let spec = vendor_spec(id, PermissionMode::Ask);
        let ctx = vendor_ctx(bin, Some("prev-99"));
        let (_, mut sess) = rt.plan_session(&spec, &ctx).unwrap();
        let _ = sess.start(now);
        let open_out = sess.on_line(
            br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"version":"0.9.0"}}}"#,
            now,
        );
        let open_val: serde_json::Value = match &open_out[0] {
            Output::Write(l) => serde_json::from_slice(l.as_bytes()).unwrap(),
            other => panic!("{id}: expected Write, got {other:?}"),
        };
        assert_eq!(open_val["method"], "session/load", "{id}");
        assert!(open_val["params"].get("_meta").is_none(), "{id}");
        let _ = sess.on_line(
            br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"prev-99","models":{"currentModelId":"vendor-model-1"},"modes":{"currentModeId":"default"}}}"#,
            now,
        );
        let turn1 = sess
            .control(
                Control::Send {
                    text: "First message".into(),
                    images: Vec::new(),
                },
                now,
            )
            .unwrap();
        let prompt1: serde_json::Value = match &turn1[0] {
            Output::Write(l) => serde_json::from_slice(l.as_bytes()).unwrap(),
            other => panic!("{id}: expected prompt Write, got {other:?}"),
        };
        let blocks1 = prompt1["params"]["prompt"].as_array().unwrap();
        assert_eq!(
            blocks1.len(),
            2,
            "{id}: turn 1 should carry system + user block"
        );
        assert!(blocks1[0]["text"]
            .as_str()
            .unwrap()
            .contains("Follow workspace security rules."));
        assert_eq!(blocks1[1]["text"], "First message");

        // End turn 1, send turn 2 -> only 1 block (system prompt not re-sent).
        let _ = sess.on_line(
            br#"{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn"}}"#,
            now,
        );
        let turn2 = sess
            .control(
                Control::Send {
                    text: "Second message".into(),
                    images: Vec::new(),
                },
                now,
            )
            .unwrap();
        let prompt2: serde_json::Value = match &turn2[0] {
            Output::Write(l) => serde_json::from_slice(l.as_bytes()).unwrap(),
            other => panic!("{id}: expected turn 2 prompt Write, got {other:?}"),
        };
        let blocks2 = prompt2["params"]["prompt"].as_array().unwrap();
        assert_eq!(blocks2.len(), 1, "{id}: turn 2 should send only user text");
        assert_eq!(blocks2[0]["text"], "Second message");
    }
}

#[test]
fn probes_take_over_guards_host_credentials_and_non_overlapping_transcript_layouts() {
    let now = Instant::now();
    let mut seen_roots = BTreeSet::new();

    for (rt, id, bin, ver) in all_vendors() {
        // Version probe works on all 4 runtimes.
        let pctx = ProbeContext::new(PathBuf::from(format!("/usr/local/bin/{bin}")));
        assert!(matches!(
            rt.plan_probe(Probe::Version, &pctx),
            Some(ProbeRun::OneShot(_))
        ));
        let parsed_ver = rt.parse_probe(
            Probe::Version,
            &ProbeOutput::new(format!("{id} {ver}\n").into_bytes(), Vec::new(), Some(0)),
        );
        assert!(
            matches!(
                parsed_ver,
                ProbeResult::Version {
                    in_tested_range: Some(true),
                    ..
                }
            ),
            "{id}: unexpected version parse: {parsed_ver:?}"
        );

        // Auth probe is non-None on all 4 runtimes (fail-closed office contract).
        let auth_run = rt
            .plan_probe(Probe::Auth, &pctx)
            .expect("Probe::Auth planned");
        match auth_run {
            ProbeRun::OneShot(_) => assert_eq!(id, "agy"),
            ProbeRun::Conversation { mut session, .. } => {
                assert_ne!(id, "agy");
                assert_eq!(session.start(now).len(), 1);
                let signed_out = session.on_line(
                    br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"authenticated":false}}"#,
                    now,
                );
                assert_eq!(
                    signed_out,
                    vec![ProbeStep::Done(ProbeResult::Auth {
                        signed_in: false,
                        method: None,
                    })]
                );
                let eof = session.on_stdout_closed(now);
                assert!(matches!(
                    eof,
                    ProbeResult::Unavailable {
                        reason: Unavailable::Other { .. }
                    }
                ));
            }
        }

        // Take-over is supported only by Agy; Gemini, Kimi, and Pi fail closed.
        let spec = vendor_spec(id, PermissionMode::Ask);
        let ctx = vendor_ctx(bin, Some("conv-1"));
        let take_over = rt.plan_take_over(&spec, &ctx, 120, 40);
        if id == "agy" {
            assert!(take_over.is_ok());
        } else {
            assert_eq!(take_over, Err(PlanError::Unsupported("take-over")));
        }

        // Host credential deny lists are non-empty and never use Claude's OAuth token injection.
        let hp = rt.host_profile();
        assert!(!hp.uses_host_oauth_token_injection, "{id}");
        assert!(hp.host_config_subdir.is_some(), "{id}");
        assert!(!hp.config_credential_files.is_empty(), "{id}");

        // Transcript layouts have unique, non-overlapping discovery roots.
        let parser = rt.transcript_parser().expect("transcript parser");
        assert_eq!(parser.format_id(), id);
        match parser.discovery_layout() {
            TranscriptDiscoveryLayout::ConversationDir { brain_root, .. } => {
                assert!(seen_roots.insert(brain_root), "duplicate root {brain_root}");
            }
            TranscriptDiscoveryLayout::RecursiveJsonl { search_roots, .. } => {
                for root in search_roots {
                    assert!(seen_roots.insert(root), "duplicate root {root}");
                }
            }
            other => panic!("unexpected layout {other:?}"),
        }
    }
}
