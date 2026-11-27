//! Live fake-ACP stdio process integration tests driving real spawned executables through
//! [`org_agent_runtime::LineFramer`], [`org_agent_runtime::Session`], and
//! [`org_agent_runtime::ProbeSession`] for all four ACP adapters (`agy`, `gemini`, `kimi`, `pi`).

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use org_agent_runtime::{
    AgentIdentity, AgentName, AgentRuntime, ApprovalId, Control, Decision, ExitInfo, Frame,
    LaunchContext, LineFramer, Output, PermissionMode, PermissionPolicy, Probe, ProbeContext,
    ProbeResult, ProbeRun, ProbeStep, RuntimeId, SandboxSpec, SessionEvent, SessionKind,
    SessionSpec, SpecPath, Token, TurnOutcome,
};
use org_agent_runtime_acp::{AgyRuntime, GeminiRuntime, KimiRuntime, PiRuntime};

fn write_executable(dir: &Path, name: &str, script: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, script).unwrap();
    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms).unwrap();
    path
}

fn fake_acp_server_script(vendor_name: &str, mode_id: &str) -> String {
    format!(
        r#"#!/bin/sh
# Minimal live JSON-RPC 2.0 ACP stdio responder for {vendor_name}.
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      if [ "${{FAKE_ACP_UNAUTH:-0}}" = "1" ]; then
        printf '{{"jsonrpc":"2.0","id":1,"error":{{"code":-32000,"message":"Authentication required"}}}}\n'
        exit 0
      fi
      printf '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":1,"agentInfo":{{"name":"{vendor_name}","version":"1.5.17"}}}}}}\n'
      ;;
    *'"method":"session/new"'*)
      printf '{{"jsonrpc":"2.0","id":2,"result":{{"sessionId":"live-{vendor_name}-1","models":{{"currentModelId":"default-model"}},"modes":{{"currentModeId":"default"}}}}}}\n'
      ;;
    *'"method":"session/set_mode"'*)
      printf '{{"jsonrpc":"2.0","id":3,"result":{{"modeId":"{mode_id}"}}}}\n'
      ;;
    *'"method":"session/set_model"'*)
      printf '{{"jsonrpc":"2.0","id":4,"result":{{"modelId":"target-model"}}}}\n'
      ;;
    *'"method":"session/prompt"'*)
      case "$line" in
        *'trigger-cancel'*)
          printf '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"live-{vendor_name}-1","update":{{"sessionUpdate":"agent_thought_chunk"}}}}}}\n'
          IFS= read -r cancel_line
          printf '{{"jsonrpc":"2.0","id":6,"result":{{"stopReason":"cancelled"}}}}\n'
          ;;
        *)
          printf '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"live-{vendor_name}-1","update":{{"sessionUpdate":"agent_thought_chunk"}}}}}}\n'
          printf '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"live-{vendor_name}-1","update":{{"sessionUpdate":"tool_call","toolCallId":"tc-live-1","name":"run_command","rawInput":{{"CommandLine":"echo ok"}}}}}}}}\n'
          printf '{{"jsonrpc":"2.0","id":"perm-live-1","method":"session/request_permission","params":{{"sessionId":"live-{vendor_name}-1","toolCall":{{"toolCallId":"tc-live-1","name":"run_command","rawInput":{{"CommandLine":"echo ok"}}}},"options":[{{"optionId":"allow","kind":"allow_once"}},{{"optionId":"deny","kind":"reject_once"}}]}}}}\n'
          IFS= read -r perm_reply
          printf '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"live-{vendor_name}-1","update":{{"sessionUpdate":"tool_call_update","toolCallId":"tc-live-1","status":"completed"}}}}}}\n'
          printf '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"live-{vendor_name}-1","update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"Live ACP reply from {vendor_name}"}}}}}}}}\n'
          printf '{{"jsonrpc":"2.0","id":5,"result":{{"stopReason":"end_turn","usage":{{"inputTokens":55,"outputTokens":14}}}}}}\n'
          ;;
      esac
      ;;
    *'"method":"session/close"'*)
      printf '{{"jsonrpc":"2.0","id":7,"result":{{}}}}\n'
      exit 0
      ;;
  esac
done
"#
    )
}

fn build_spec(runtime_id: &str, ws: &Path) -> SessionSpec {
    let ws_spec = SpecPath::try_from(ws.to_str().unwrap()).unwrap();
    SessionSpec {
        runtime: RuntimeId::try_from(runtime_id).unwrap(),
        kind: SessionKind::Home {},
        agent: AgentIdentity {
            name: AgentName::try_from("live-agent").unwrap(),
            description: "Live ACP agent".to_owned(),
            prompt: "Be helpful.".to_owned(),
        },
        model: Some(Token::try_from("target-model").unwrap()),
        effort: None,
        workspace: ws_spec.clone(),
        read_only_dirs: Vec::new(),
        instructions: "Stay within workspace.".to_owned(),
        repository_instructions: String::new(),
        permission_mode: PermissionMode::AcceptEdits,
        policy: PermissionPolicy {
            version: 1,
            allow: Vec::new(),
            deny: Vec::new(),
        },
        sandbox: SandboxSpec {
            enabled: true,
            writable_roots: vec![ws_spec],
            deny_write: Vec::new(),
            deny_read: Vec::new(),
            allowed_unix_sockets: Vec::new(),
        },
        mcp_servers: Default::default(),
        tools: None,
        operator_shell_prefixes: Vec::new(),
    }
}

fn drive_writes(stdin: &mut Option<std::process::ChildStdin>, outputs: &[Output]) {
    for output in outputs {
        match output {
            Output::Write(line) => {
                if let Some(writer) = stdin.as_mut() {
                    writer.write_all(line.as_bytes()).unwrap();
                    writer.flush().unwrap();
                }
            }
            Output::CloseStdin => {
                stdin.take();
            }
            Output::Event(_) => {}
        }
    }
}

fn read_framed_line(
    reader: &mut BufReader<std::process::ChildStdout>,
    framer: &mut LineFramer,
) -> Option<Vec<u8>> {
    let mut raw = Vec::new();
    let n = reader.read_until(b'\n', &mut raw).unwrap();
    if n == 0 {
        return None;
    }
    let frames = framer.push(&raw);
    frames.into_iter().find_map(|f| match f {
        Frame::Line(line) => Some(line),
        Frame::Truncated(_) => None,
    })
}

#[test]
fn live_fake_acp_stdio_lifecycle_passes_for_agy_gemini_kimi_and_pi() {
    let vendors: [(&dyn AgentRuntime, &str, &str, &str); 4] = [
        (&AgyRuntime, "agy", "agy", "accept-edits"),
        (&GeminiRuntime, "gemini", "gemini", "auto_edit"),
        (&KimiRuntime, "kimi", "kimi", "accept-edits"),
        (&PiRuntime, "pi", "pi-acp", "accept-edits"),
    ];

    for (runtime, id, bin_name, expected_mode) in vendors {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let ws = root.join("ws");
        let rt_dir = root.join("rt");
        let tmp = root.join("tmp");
        for d in [&ws, &rt_dir, &tmp] {
            fs::create_dir_all(d).unwrap();
        }
        let bin = write_executable(&root, bin_name, &fake_acp_server_script(id, expected_mode));
        let spec = build_spec(id, &ws);
        let ctx = LaunchContext {
            runtime_path: bin,
            runtime_version: None,
            runtime_dir: SpecPath::try_from(rt_dir.to_str().unwrap()).unwrap(),
            tmpdir: SpecPath::try_from(tmp.to_str().unwrap()).unwrap(),
            office_env_names: Vec::new(),
            new_session_id: None,
            resume: None,
            config_dir: None,
        };
        let (plan, mut session) = runtime.plan_session(&spec, &ctx).unwrap();
        // Stage planned dirs/files just as org-supervise does.
        for d in &plan.dirs {
            fs::create_dir_all(rt_dir.join(d.rel_path.as_str())).unwrap();
        }
        for f in &plan.files {
            fs::write(rt_dir.join(f.rel_path.as_str()), &f.contents).unwrap();
        }

        let mut cmd = Command::new(&plan.program);
        cmd.args(&plan.args)
            .current_dir(plan.cwd.as_str())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (k, v) in &plan.env_add {
            cmd.env(k.as_str(), v);
        }
        let mut child = cmd.spawn().unwrap();
        let mut stdin = child.stdin.take();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut framer = LineFramer::new();
        let now = Instant::now();

        // 1. Handshake: initialize -> session/new -> session/set_mode -> session/set_model -> Ready.
        let start_out = session.start(now);
        drive_writes(&mut stdin, &start_out);

        let mut ready_mode = None;
        for _ in 0..8 {
            let line = read_framed_line(&mut stdout, &mut framer).expect("handshake line");
            let out = session.on_line(&line, now);
            drive_writes(&mut stdin, &out);
            for item in &out {
                if let Output::Event(SessionEvent::Ready {
                    permission_mode, ..
                }) = item
                {
                    ready_mode = permission_mode.clone();
                }
            }
            if ready_mode.is_some() {
                break;
            }
        }
        assert_eq!(ready_mode.as_deref(), Some(expected_mode), "{id}");

        // 2. Turn 1: prompt -> thinking -> tool_call -> approval -> allow_once -> reply -> usage -> TurnEnded.
        let send_out = session
            .control(
                Control::Send {
                    text: "Say hello".into(),
                    images: Vec::new(),
                },
                now,
            )
            .unwrap();
        drive_writes(&mut stdin, &send_out);

        let mut saw_reply = String::new();
        let mut saw_usage = false;
        let mut turn1_outcome = None;
        for _ in 0..16 {
            let line = read_framed_line(&mut stdout, &mut framer).expect("turn 1 line");
            let out = session.on_line(&line, now);
            drive_writes(&mut stdin, &out);
            for item in out {
                match item {
                    Output::Event(SessionEvent::ApprovalRequested(req)) => {
                        assert_eq!(req.id, ApprovalId::new("perm-live-1"));
                        let ans = session
                            .control(
                                Control::Answer {
                                    request_id: req.id,
                                    decision: Decision::AllowOnce,
                                },
                                now,
                            )
                            .unwrap();
                        drive_writes(&mut stdin, &ans);
                    }
                    Output::Event(SessionEvent::AssistantText { text, .. }) => saw_reply = text,
                    Output::Event(SessionEvent::Usage(u)) => {
                        assert_eq!(u.input, 55);
                        assert_eq!(u.output, 14);
                        saw_usage = true;
                    }
                    Output::Event(SessionEvent::TurnEnded { outcome, .. }) => {
                        turn1_outcome = Some(outcome);
                    }
                    _ => {}
                }
            }
            if turn1_outcome.is_some() {
                break;
            }
        }
        assert_eq!(saw_reply, format!("Live ACP reply from {id}"));
        assert!(saw_usage, "{id}: expected usage before TurnEnded");
        assert_eq!(turn1_outcome, Some(TurnOutcome::Completed));

        // 3. Turn 2: Interrupt (`session/cancel`) ends cleanly with TurnOutcome::Interrupted.
        let send2 = session
            .control(
                Control::Send {
                    text: "trigger-cancel".into(),
                    images: Vec::new(),
                },
                now,
            )
            .unwrap();
        drive_writes(&mut stdin, &send2);
        let thought = read_framed_line(&mut stdout, &mut framer).expect("thought chunk");
        let _ = session.on_line(&thought, now);
        let intr = session.control(Control::Interrupt, now).unwrap();
        drive_writes(&mut stdin, &intr);
        let cancel_resp = read_framed_line(&mut stdout, &mut framer).expect("cancel response");
        let cancel_out = session.on_line(&cancel_resp, now);
        assert!(
            cancel_out.iter().any(|o| matches!(
                o,
                Output::Event(SessionEvent::TurnEnded {
                    outcome: TurnOutcome::Interrupted,
                    ..
                })
            )),
            "{id}: expected TurnOutcome::Interrupted, got {cancel_out:?}"
        );

        // 4. Close -> session/close + CloseStdin -> clean exit.
        let close_out = session.control(Control::Close, now).unwrap();
        drive_writes(&mut stdin, &close_out);
        let status = child.wait().unwrap();
        let end_out = session.on_exit(
            ExitInfo {
                code: status.code(),
                signal: None,
            },
            now,
        );
        assert!(end_out
            .iter()
            .any(|o| matches!(o, Output::Event(SessionEvent::Ended { .. }))));
    }
}

#[test]
fn live_fake_acp_auth_probe_session_verifies_signed_in_and_unauthenticated_binaries() {
    for (runtime, id, bin_name, expected_mode) in [
        (
            &GeminiRuntime as &dyn AgentRuntime,
            "gemini",
            "gemini",
            "auto_edit",
        ),
        (
            &KimiRuntime as &dyn AgentRuntime,
            "kimi",
            "kimi",
            "accept-edits",
        ),
        (
            &PiRuntime as &dyn AgentRuntime,
            "pi",
            "pi-acp",
            "accept-edits",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let bin = write_executable(
            dir.path(),
            bin_name,
            &fake_acp_server_script(id, expected_mode),
        );
        for (unauth, expected_signed_in) in [("0", true), ("1", false)] {
            let Some(ProbeRun::Conversation { plan, mut session }) =
                runtime.plan_probe(Probe::Auth, &ProbeContext::new(bin.clone()))
            else {
                panic!("{id}: expected conversation Probe::Auth");
            };
            let mut child = Command::new(&plan.program)
                .args(&plan.args)
                .env("FAKE_ACP_UNAUTH", unauth)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let mut stdin = child.stdin.take().unwrap();
            let mut stdout = BufReader::new(child.stdout.take().unwrap());
            let mut framer = LineFramer::new();
            let now = Instant::now();

            for step in session.start(now) {
                if let ProbeStep::Write(line) = step {
                    stdin.write_all(line.as_bytes()).unwrap();
                    stdin.flush().unwrap();
                }
            }
            let line = read_framed_line(&mut stdout, &mut framer).expect("probe response");
            let steps = session.on_line(&line, now);
            drop(stdin);
            let _ = child.wait();
            assert!(
                steps.iter().any(|s| matches!(
                    s,
                    ProbeStep::Done(ProbeResult::Auth { signed_in, .. })
                        if *signed_in == expected_signed_in
                )),
                "{id} (unauth={unauth}): unexpected probe steps: {steps:?}"
            );
        }
    }
}
