use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use console_attach::agent_server::{AgentClient, RunningAgentDaemon};
use console_attach::claude::ClaudeConfig;

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempDirGuard {
    path: PathBuf,
}

impl TempDirGuard {
    fn new(prefix: &str) -> Self {
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(format!(
            "/tmp/ca-claude-{prefix}-{}-{seq}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Writes an executable Python mock speaking Claude Code's exact `stream-json` wire format,
/// including `initialize`, `can_use_tool`, background task/subagent events, and `interrupt`.
fn write_mock_claude(dir: &std::path::Path) -> PathBuf {
    let script_path = dir.join("mock-claude.py");
    let script = r#"#!/usr/bin/env python3
import json
import sys
import threading
import time

out_lock = threading.Lock()
interrupted = threading.Event()
pending_approval = {}

def emit(obj):
    with out_lock:
        sys.stdout.write(json.dumps(obj) + "\n")
        sys.stdout.flush()

def run_background_task():
    time.sleep(0.15)
    emit({
        "type": "assistant",
        "parent_tool_use_id": "toolu_bg_1",
        "message": {
            "role": "assistant",
            "content": [{"type": "text", "text": "subagent progress: background job still alive"}]
        }
    })

for raw in sys.stdin:
    line = raw.strip()
    if not line:
        continue
    msg = json.loads(line)
    mtype = msg.get("type")

    if mtype == "control_request":
        rid = msg["request_id"]
        subtype = msg["request"]["subtype"]
        if subtype == "initialize":
            emit({
                "type": "control_response",
                "response": {"subtype": "success", "request_id": rid, "response": {"ok": True}}
            })
        elif subtype == "interrupt":
            interrupted.set()
            emit({
                "type": "control_response",
                "response": {"subtype": "success", "request_id": rid, "response": {"still_queued": []}}
            })

    elif mtype == "control_response":
        resp = msg.get("response", {})
        rid = resp.get("request_id")
        if rid in pending_approval:
            behavior = resp.get("response", {}).get("behavior")
            pending_approval.pop(rid)
            if behavior == "allow":
                threading.Thread(target=run_background_task, daemon=True).start()
                emit({
                    "type": "assistant",
                    "message": {
                        "role": "assistant",
                        "content": [{"type": "text", "text": "Started background job."}]
                    }
                })
                emit({
                    "type": "result",
                    "subtype": "success",
                    "is_error": False,
                    "session_id": "sess-mock-1",
                    "result": "Started background job."
                })
            else:
                emit({
                    "type": "result",
                    "subtype": "success",
                    "is_error": False,
                    "session_id": "sess-mock-1",
                    "result": "Tool denied."
                })

    elif mtype == "user":
        text = msg["message"]["content"]
        emit({"type": "system", "subtype": "init", "session_id": "sess-mock-1"})
        if text == "start-bg":
            req_id = "perm-1"
            pending_approval[req_id] = True
            emit({
                "type": "control_request",
                "request_id": req_id,
                "request": {
                    "subtype": "can_use_tool",
                    "tool_name": "Bash",
                    "input": {"command": "sleep 30", "run_in_background": True}
                }
            })
        elif text == "slow":
            interrupted.clear()
            def slow_turn():
                for _ in range(40):
                    if interrupted.is_set():
                        emit({
                            "type": "result",
                            "subtype": "error_during_execution",
                            "is_error": False,
                            "terminal_reason": "aborted_tools",
                            "session_id": "sess-mock-1",
                            "result": "interrupted"
                        })
                        return
                    time.sleep(0.05)
            threading.Thread(target=slow_turn, daemon=True).start()
        else:
            emit({
                "type": "assistant",
                "message": {
                    "role": "assistant",
                    "content": [{"type": "text", "text": f"echo:{text}"}]
                }
            })
            emit({
                "type": "result",
                "subtype": "success",
                "is_error": False,
                "session_id": "sess-mock-1",
                "result": f"echo:{text}"
            })
"#;
    fs::write(&script_path, script).expect("write mock-claude.py");
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755))
        .expect("chmod +x mock-claude.py");
    script_path
}

#[test]
fn persistent_stream_json_supports_background_work_approvals_followup_and_interrupt() {
    let dir = TempDirGuard::new("mock");
    let mock_bin = write_mock_claude(&dir.path);
    let socket_path = dir.path.join("claude.sock");

    let daemon = RunningAgentDaemon::start_claude(
        socket_path.clone(),
        ClaudeConfig {
            binary: mock_bin.to_string_lossy().into_owned(),
            stdio_approvals: true,
            ..ClaudeConfig::default()
        },
    )
    .expect("start mock claude daemon");

    let client = AgentClient::new(&socket_path);

    // Wait briefly for `initialize` control_response -> state transitions to idle.
    let mut initialized = false;
    for _ in 0..40 {
        let st = client.status().expect("status");
        if st.state == "idle" {
            initialized = true;
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }
    assert!(initialized, "expected initialize handshake to reach idle");

    // Turn 1: send message that requests Bash approval to start a background job.
    let sent1 = client.send("start-bg").expect("send start-bg");
    assert_eq!(sent1.kind, "user_sent");

    // Wait for approval_requested.
    let mut pending_id = None;
    for _ in 0..40 {
        let st = client.status().expect("status");
        if let Some(p) = st.pending_approvals.first() {
            pending_id = Some(p.request_id.clone());
            assert_eq!(st.state, "awaiting_approval");
            assert_eq!(p.tool_name, "Bash");
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }
    let req_id = pending_id.expect("expected pending approval perm-1");
    assert_eq!(req_id, "perm-1");

    // Approve tool call over the Unix socket.
    client.approve(&req_id).expect("approve perm-1");

    // Immediately send Turn 2 while background subagent/job is still running!
    let turn2 = client
        .send_wait("ping while bg running", Duration::from_secs(5))
        .expect("send_wait turn 2");
    assert!(
        turn2.text.as_deref() == Some("Started background job.")
            || turn2.text.as_deref() == Some("echo:ping while bg running"),
        "unexpected turn result: {turn2:?}"
    );

    // Wait until both the background subagent text and the Turn 2 echo arrive in the event log.
    let mut saw_subagent = false;
    let mut saw_turn2_echo = false;
    for _ in 0..40 {
        let events = client.events(0).expect("events");
        saw_subagent = events.iter().any(|e| e.kind == "subagent_text");
        saw_turn2_echo = events
            .iter()
            .any(|e| e.kind == "result" && e.text.as_deref() == Some("echo:ping while bg running"));
        if saw_subagent && saw_turn2_echo {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(
        saw_subagent,
        "expected subagent_text event from background job"
    );
    assert!(
        saw_turn2_echo,
        "expected follow-up turn reply while bg job ran"
    );

    // Turn 3: start a slow turn and interrupt it mid-flight, then verify Turn 4 still works.
    let slow_sent = client.send("slow").expect("send slow");
    thread::sleep(Duration::from_millis(80));
    client.interrupt().expect("interrupt");

    let mut saw_interrupt_result = false;
    for _ in 0..40 {
        let events = client.events(slow_sent.seq).expect("events after slow");
        if events
            .iter()
            .any(|e| e.kind == "result" && e.text.as_deref() == Some("interrupted"))
        {
            saw_interrupt_result = true;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(saw_interrupt_result, "expected interrupted result");

    // Turn 4: session is still alive after interrupt.
    let after = client
        .send_wait("after-interrupt", Duration::from_secs(5))
        .expect("send_wait after interrupt");
    assert_eq!(after.text.as_deref(), Some("echo:after-interrupt"));

    // Clean stop.
    client.stop().expect("stop daemon");
    let _ = daemon.wait().expect("wait daemon exit");
    assert!(!socket_path.exists(), "socket should be cleaned up on exit");
}

#[test]
fn real_claude_binary_completes_stream_json_initialize_without_model_call() {
    // Only run if `claude` is installed on PATH.
    let has_claude = std::process::Command::new("which")
        .arg("claude")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !has_claude {
        return;
    }

    let dir = TempDirGuard::new("real-init");
    let socket_path = dir.path.join("real-claude.sock");

    let daemon = RunningAgentDaemon::start_claude(
        socket_path.clone(),
        ClaudeConfig {
            cwd: Some(dir.path.clone()),
            extra_args: vec![
                "--setting-sources".to_string(),
                "".to_string(),
                "--strict-mcp-config".to_string(),
                "--mcp-config".to_string(),
                r#"{"mcpServers":{}}"#.to_string(),
            ],
            ..ClaudeConfig::default()
        },
    )
    .expect("spawn real claude in stream-json mode");

    let client = AgentClient::new(&socket_path);

    let mut got_control_response = false;
    for _ in 0..80 {
        let events = client.events(0).expect("events");
        if events.iter().any(|e| e.kind == "control_response") {
            got_control_response = true;
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }

    let st = client.status().expect("status");
    assert!(
        got_control_response,
        "expected real claude initialize control_response; status={st:?}, events={:?}",
        client.events(0)
    );
    assert!(
        st.alive,
        "real claude should stay alive waiting for user turns"
    );

    client.stop().expect("stop real claude");
    let _ = daemon.wait().expect("wait real claude exit");
    assert!(!socket_path.exists());
}
