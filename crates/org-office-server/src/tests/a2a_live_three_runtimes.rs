//! Opt-in live 3-runtime cross-office A2A messaging integration test (`TASK-117`).
//!
//! Exercises the real installed `claude` (Claude Code), `codex` (Codex CLI), and `agy`
//! (Antigravity CLI) binaries against three live `org-office-server` instances (HQ + Satellite 1 +
//! Satellite 2 over real Org-CA mTLS) and the real compiled `agent-os` binary, proving all three
//! runtimes wake on incoming messages, authenticate via PID-lineage `RunBindings`, read their
//! messages via `agent-os`, and relay messages across offices.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use org_office::{Office, TestOffice};
use org_proto::chat::ChatSendParams;
use serde_json::{json, Value};

use super::a2a_messaging::{
    bind_provider_on_office, enroll_satellite, hire_agent_at, setup_provider_on_hq, wait_until,
    LiveOfficeNode,
};

struct RuntimeEnv<'a> {
    node: &'a LiveOfficeNode,
    employee_id: &'a str,
    handle: &'a str,
    agent_os_dir: &'a Path,
    workspace: &'a Path,
}

fn find_on_path(bin: &str) -> PathBuf {
    let path_var = std::env::var_os("PATH").expect("PATH must be set");
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(bin);
        if candidate.is_file() {
            return candidate;
        }
    }
    panic!("required live runtime binary `{bin}` not found on PATH");
}

fn find_agent_os_dir() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let debug_dir = workspace_root.join("target/debug");
    assert!(
        debug_dir.join("agent-os").is_file(),
        "build `agent-os` first via `cargo build -p agent-os` (missing {})",
        debug_dir.join("agent-os").display()
    );
    debug_dir
}

fn apply_agent_os_env(cmd: &mut Command, env: &RuntimeEnv<'_>, token: &str, session_id: &str) {
    let cur_path = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".to_owned());
    let merged_path = format!("{}:{cur_path}", env.agent_os_dir.display());
    let info = env.node.office.info();
    cmd.current_dir(env.workspace)
        .env("PATH", merged_path)
        .env("AGENT_OS_SOCKET", &env.node.socket)
        .env("AGENT_OS_OFFICE", &info.office_id)
        .env("AGENT_OS_ORG", &info.org_id)
        .env("AGENT_OS_AGENT_ID", env.employee_id)
        .env("AGENT_OS_SESSION_ID", session_id)
        .env("AGENT_OS_SESSION_TOKEN", token);
}

fn wait_for_idle_session(node: &LiveOfficeNode, employee_id: &str) {
    wait_until(
        Duration::from_secs(10),
        "session idle before live turn",
        || {
            !matches!(
                node.sessions
                    .inspect_session(employee_id, None)
                    .ok()
                    .map(|s| s.session.state),
                Some(
                    org_proto::sessions::SessionState::Starting
                        | org_proto::sessions::SessionState::Running
                        | org_proto::sessions::SessionState::WaitingApproval
                )
            )
        },
    );
}

fn spawn_line_reader(stdout: std::process::ChildStdout) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if tx.send(line.trim_end().to_owned()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    rx
}

fn write_claude_live_config(
    env: &RuntimeEnv<'_>,
) -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let cfg_dir = tempfile::tempdir().unwrap();
    let agents_path = cfg_dir.path().join("agents.json");
    let mcp_path = cfg_dir.path().join("mcp.json");
    let settings_path = cfg_dir.path().join("settings.json");
    fs::write(
        &agents_path,
        serde_json::to_vec(&json!({
            env.handle: {
                "description": "Live A2A messaging test agent",
                "prompt": "You are an Agentic Org employee. Always execute the exact `agent-os` CLI commands requested and confirm concisely."
            }
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(&mcp_path, br#"{"mcpServers":{}}"#).unwrap();
    fs::write(
        &settings_path,
        serde_json::to_vec(&json!({
            "permissions": {
                "allow": ["Bash(agent-os *)"],
                "deny": []
            },
            "sandbox": {
                "enabled": true,
                "autoAllowBashIfSandboxed": false,
                "allowUnsandboxedCommands": false,
                "filesystem": { "allowWrite": [env.workspace.display().to_string()] },
                "network": { "allowUnixSockets": [env.node.socket.display().to_string()] }
            }
        }))
        .unwrap(),
    )
    .unwrap();
    (cfg_dir, agents_path, mcp_path, settings_path)
}

fn drive_claude_live_stream(
    stdin: &mut std::process::ChildStdin,
    rx: &mpsc::Receiver<String>,
    prompt: &str,
) -> String {
    writeln!(
        stdin,
        "{}",
        json!({"type":"control_request","request_id":"init-1","request":{"subtype":"initialize"}})
    )
    .unwrap();
    stdin.flush().unwrap();

    let deadline = Instant::now() + Duration::from_secs(90);
    let mut sent_prompt = false;
    while Instant::now() < deadline {
        let rem = deadline.saturating_duration_since(Instant::now());
        let Ok(line) = rx.recv_timeout(rem.min(Duration::from_secs(2))) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match v.get("type").and_then(Value::as_str) {
            Some("control_response") if !sent_prompt => {
                sent_prompt = true;
                let msg = json!({"type":"user","message":{"role":"user","content":prompt}});
                writeln!(stdin, "{msg}").unwrap();
                stdin.flush().unwrap();
            }
            Some("control_request") => allow_claude_control_request(stdin, &v),
            Some("result") => {
                return v
                    .get("result")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
            }
            _ => {}
        }
    }
    String::new()
}

fn allow_claude_control_request(stdin: &mut std::process::ChildStdin, v: &Value) {
    let Some(req_id) = v.get("request_id").and_then(Value::as_str) else {
        return;
    };
    let input = v
        .pointer("/request/input")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let resp = json!({
        "type": "control_response",
        "response": {
            "subtype": "success",
            "request_id": req_id,
            "response": { "behavior": "allow", "updatedInput": input }
        }
    });
    writeln!(stdin, "{resp}").unwrap();
    stdin.flush().unwrap();
}

fn run_claude_live_turn(env: &RuntimeEnv<'_>, prompt: &str) -> String {
    wait_for_idle_session(env.node, env.employee_id);
    let claude_bin = find_on_path("claude");
    let run_id = format!("live-claude-{}", env.node.office.next_uuid_v7());
    let sess_id = format!("sess-claude-{}", env.handle);
    let binding = env
        .node
        .sessions
        .bindings()
        .issue(env.employee_id, &run_id, &sess_id, 1, None)
        .unwrap();
    let token = binding.expose_token().to_owned();
    let (_cfg_dir, agents_path, mcp_path, settings_path) = write_claude_live_config(env);

    let mut cmd = Command::new(&claude_bin);
    cmd.args([
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--disable-slash-commands",
        "--setting-sources",
        "",
        "--strict-mcp-config",
        "--mcp-config",
        mcp_path.to_str().unwrap(),
        "--settings",
        settings_path.to_str().unwrap(),
        "--permission-mode",
        "acceptEdits",
        "--permission-prompt-tool",
        "stdio",
        "--agents",
        agents_path.to_str().unwrap(),
        "--agent",
        env.handle,
        "--model",
        "claude-haiku-4-5-20251001",
    ])
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    apply_agent_os_env(&mut cmd, env, &token, &sess_id);

    let mut child = cmd.spawn().expect("spawn live claude");
    env.node
        .sessions
        .bindings()
        .set_root_pid(env.employee_id, &run_id, child.id() as libc::pid_t);
    let mut stdin = child.stdin.take().unwrap();
    let rx = spawn_line_reader(child.stdout.take().unwrap());
    let final_text = drive_claude_live_stream(&mut stdin, &rx, prompt);
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    env.node
        .sessions
        .bindings()
        .revoke(env.employee_id, Some(&run_id));
    final_text
}

fn build_codex_live_cmd(env: &RuntimeEnv<'_>, ws: &Path, token: &str, sess_id: &str) -> Command {
    let codex_bin = find_on_path("codex");
    let real_home = std::env::var("HOME").expect("HOME set");
    let codex_home = PathBuf::from(&real_home).join(".codex");
    let sock = env.node.socket.canonicalize().unwrap();
    let sock_dir = sock.parent().unwrap();
    let fs_cfg = format!(
        "permissions.office.filesystem={{\"/\"=\"read\",{}=\"write\",{}=\"write\"}}",
        serde_json::to_string(&ws.display().to_string()).unwrap(),
        serde_json::to_string(&sock_dir.display().to_string()).unwrap(),
    );
    let net_cfg = format!(
        "permissions.office.network={{enabled=true,unix_sockets={{{}=\"allow\"}}}}",
        serde_json::to_string(&sock.display().to_string()).unwrap(),
    );
    let mut cmd = Command::new(&codex_bin);
    cmd.args([
        "app-server",
        "--stdio",
        "-c",
        "analytics.enabled=false",
        "-c",
        "approval_policy={granular={sandbox_approval=false,rules=true,mcp_elicitations=false,request_permissions=false,skill_approval=false}}",
        "-c",
        "features.network_proxy=true",
        "-c",
        "default_permissions=\"office\"",
        "-c",
        &fs_cfg,
        "-c",
        &net_cfg,
        "-c",
        "shell_environment_policy.ignore_default_excludes=true",
        "-c",
        "shell_environment_policy.include_only=[\"PATH\",\"HOME\",\"TMPDIR\",\"LANG\",\"TERM\",\"AGENT_OS_SOCKET\",\"AGENT_OS_OFFICE\",\"AGENT_OS_ORG\",\"AGENT_OS_AGENT_ID\",\"AGENT_OS_SESSION_ID\",\"AGENT_OS_SESSION_TOKEN\"]",
    ])
    .env("CODEX_HOME", &codex_home)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    apply_agent_os_env(&mut cmd, env, token, sess_id);
    cmd
}

fn log_codex_live_cmd_item(v: &Value) {
    let Some(item) = v.pointer("/params/item") else {
        return;
    };
    if item.get("type").and_then(Value::as_str) != Some("commandExecution") {
        return;
    }
    let out: String = item
        .get("aggregatedOutput")
        .and_then(Value::as_str)
        .unwrap_or("")
        .chars()
        .take(240)
        .collect();
    eprintln!(
        "[LIVE CODEX CMD] exit={:?} cmd={:?} out={:?}",
        item.get("exitCode"),
        item.get("command"),
        out
    );
}

fn handle_codex_live_frame(
    stdin: &mut std::process::ChildStdin,
    v: &Value,
    ws: &Path,
    prompt: &str,
    text: &mut String,
) -> bool {
    match v.get("id").and_then(Value::as_u64) {
        Some(1) => {
            writeln!(
                stdin,
                "{}",
                json!({"jsonrpc":"2.0","method":"initialized","params":{}})
            )
            .unwrap();
            let start = json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "thread/start",
                "params": {
                    "cwd": ws.display().to_string(),
                    "developerInstructions": "You are an Agentic Org employee. Always run the exact `agent-os` CLI commands requested and confirm concisely."
                }
            });
            writeln!(stdin, "{start}").unwrap();
            stdin.flush().unwrap();
            return false;
        }
        Some(2) => {
            if let Some(err) = v.get("error") {
                panic!("codex thread/start failed: {err}");
            }
            let tid = v
                .pointer("/result/thread/id")
                .and_then(Value::as_str)
                .unwrap();
            let turn = json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "turn/start",
                "params": { "threadId": tid, "input": [{ "type": "text", "text": prompt }] }
            });
            writeln!(stdin, "{turn}").unwrap();
            stdin.flush().unwrap();
            return false;
        }
        _ => {}
    }
    match v.get("method").and_then(Value::as_str) {
        Some("item/agentMessage/delta") => {
            if let Some(delta) = v.pointer("/params/delta").and_then(Value::as_str) {
                text.push_str(delta);
            }
            false
        }
        Some("item/completed") => {
            log_codex_live_cmd_item(v);
            false
        }
        Some("turn/completed") => true,
        _ => false,
    }
}

fn run_codex_live_turn(env: &RuntimeEnv<'_>, prompt: &str) -> String {
    wait_for_idle_session(env.node, env.employee_id);
    let run_id = format!("live-codex-{}", env.node.office.next_uuid_v7());
    let sess_id = format!("sess-codex-{}", env.handle);
    let binding = env
        .node
        .sessions
        .bindings()
        .issue(env.employee_id, &run_id, &sess_id, 1, None)
        .unwrap();
    let token = binding.expose_token().to_owned();
    let ws = env.workspace.canonicalize().unwrap();
    let mut cmd = build_codex_live_cmd(env, &ws, &token, &sess_id);
    let mut child = cmd.spawn().expect("spawn live codex");
    env.node
        .sessions
        .bindings()
        .set_root_pid(env.employee_id, &run_id, child.id() as libc::pid_t);
    let mut stdin = child.stdin.take().unwrap();
    let rx = spawn_line_reader(child.stdout.take().unwrap());
    let init = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"org-office-live","version":"1.0"},"capabilities":{"experimentalApi":true}}});
    writeln!(stdin, "{init}").unwrap();
    stdin.flush().unwrap();

    let deadline = Instant::now() + Duration::from_secs(90);
    let mut text = String::new();
    while Instant::now() < deadline {
        let rem = deadline.saturating_duration_since(Instant::now());
        let Ok(line) = rx.recv_timeout(rem.min(Duration::from_secs(2))) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if handle_codex_live_frame(&mut stdin, &v, &ws, prompt, &mut text) {
            break;
        }
    }
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    env.node
        .sessions
        .bindings()
        .revoke(env.employee_id, Some(&run_id));
    text
}

fn run_agy_live_turn(env: &RuntimeEnv<'_>, prompt: &str) -> String {
    wait_for_idle_session(env.node, env.employee_id);
    let agy_bin = find_on_path("agy");
    let run_id = format!("live-agy-{}", env.node.office.next_uuid_v7());
    let sess_id = format!("sess-agy-{}", env.handle);
    let binding = env
        .node
        .sessions
        .bindings()
        .issue(env.employee_id, &run_id, &sess_id, 1, None)
        .unwrap();
    let token = binding.expose_token().to_owned();

    let sock_dir = env.node.socket.parent().unwrap();
    let mut cmd = Command::new(&agy_bin);
    cmd.args([
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--disable-slash-commands",
        "--dangerously-skip-permissions",
        "--mode",
        "auto",
        "--add-dir",
        env.agent_os_dir.to_str().unwrap(),
        "--add-dir",
        sock_dir.to_str().unwrap(),
        "--model",
        "gemini-3.8-flash-medium",
    ])
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    apply_agent_os_env(&mut cmd, env, &token, &sess_id);

    let mut child = cmd.spawn().expect("spawn live agy");
    env.node
        .sessions
        .bindings()
        .set_root_pid(env.employee_id, &run_id, child.id() as libc::pid_t);
    let mut stdin = child.stdin.take().unwrap();
    let rx = spawn_line_reader(child.stdout.take().unwrap());

    writeln!(
        stdin,
        "{}",
        json!({"event":"user","message":{"role":"user","content":prompt}})
    )
    .unwrap();
    stdin.flush().unwrap();

    let deadline = Instant::now() + Duration::from_secs(90);
    let mut final_text = String::new();
    while Instant::now() < deadline {
        let rem = deadline.saturating_duration_since(Instant::now());
        let Ok(line) = rx.recv_timeout(rem.min(Duration::from_secs(2))) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match v.get("event").and_then(Value::as_str) {
            Some("step_update") => {
                if v.pointer("/step_update/step_type").and_then(Value::as_str) == Some("tool")
                    && v.pointer("/step_update/state").and_then(Value::as_str) == Some("DONE")
                {
                    eprintln!(
                        "[LIVE AGY TOOL] {}",
                        v.pointer("/step_update/tool_info")
                            .map(|t| t.to_string().chars().take(240).collect::<String>())
                            .unwrap_or_default()
                    );
                }
            }
            Some("result") => {
                eprintln!("[LIVE AGY RESULT] {}", v);
                final_text = v
                    .pointer("/result/response")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                break;
            }
            _ => {}
        }
    }
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    env.node
        .sessions
        .bindings()
        .revoke(env.employee_id, Some(&run_id));
    final_text
}

#[test]
#[ignore = "live 3-runtime A2A test: requires signed-in claude, codex, and agy CLIs plus built agent-os binary"]
fn live_three_runtimes_claude_codex_and_agy_cross_office_a2a_ring() {
    let agent_os_dir = find_agent_os_dir();
    let ws_dir = tempfile::tempdir().unwrap();
    let ws = ws_dir.path().canonicalize().unwrap();

    let hq = Arc::new(Office::for_test_with_real_ca(TestOffice::sqlite(
        org_office_sqlite::sqlite_backend(),
    )));
    let owner = hq.owner_member_id().to_owned();
    let team_id = hq.org_snapshot().unwrap().teams[0].id.clone();
    let hq_id = hq.info().office_id.clone();
    let (conn_id, profile_id) = setup_provider_on_hq(&hq, &owner);
    let hq_node = LiveOfficeNode::start(Arc::clone(&hq), true, false);
    let hq_addr = hq_node.hq_fed_addr();

    let (_sat1_dir, sat1) = enroll_satellite(&hq, &owner, &hq_addr, "Codex Satellite", 1);
    let (_sat2_dir, sat2) = enroll_satellite(&hq, &owner, &hq_addr, "Agy Satellite", 2);
    let sat1_id = sat1.info().office_id.clone();
    let sat2_id = sat2.info().office_id.clone();

    let claude_ada = hire_agent_at(
        &hq,
        &owner,
        &team_id,
        &hq_id,
        &profile_id,
        "Claude Ada",
        "claude-ada",
    );
    let codex_ben = hire_agent_at(
        &hq,
        &owner,
        &team_id,
        &sat1_id,
        &profile_id,
        "Codex Ben",
        "codex-ben",
    );
    let agy_cy = hire_agent_at(
        &hq,
        &owner,
        &team_id,
        &sat2_id,
        &profile_id,
        "Agy Cy",
        "agy-cy",
    );

    bind_provider_on_office(&sat1, sat1.owner_member_id(), &conn_id);
    bind_provider_on_office(&sat2, sat2.owner_member_id(), &conn_id);
    let sat1_node = LiveOfficeNode::start(Arc::clone(&sat1), false, true);
    let sat2_node = LiveOfficeNode::start(Arc::clone(&sat2), false, true);

    wait_until(Duration::from_secs(10), "satellites synced hires", || {
        sat1.org_snapshot()
            .is_ok_and(|s| s.members.iter().any(|m| m.id == agy_cy))
            && sat2
                .org_snapshot()
                .is_ok_and(|s| s.members.iter().any(|m| m.id == agy_cy))
    });

    // Hop 1: Owner sends DM to Claude Ada on HQ -> HQ wakes Claude -> real Claude Code reads inbox
    // via `agent-os` and sends cross-office DM to Codex Ben on Satellite 1.
    let hop1 = hq
        .chat_send(
            &owner,
            ChatSendParams {
                id: hq.next_uuid_v7(),
                to: "claude-ada".into(),
                kind: "msg".into(),
                priority: None,
                subject: Some("Live 3-runtime ring".into()),
                body: "LIVE-RING-7741: Run `agent-os msg inbox` and `agent-os msg send --to codex-ben --body \"CLAUDE_TO_CODEX_7741\"` right now.".into(),
                meta: std::collections::BTreeMap::new(),
                reply_to: None,
                also_in_channel: false,
                reason: None,
                attachment_ids: Vec::new(),
            },
        )
        .unwrap()
        .unwrap();
    let _ = crate::wake_bridge::on_chat_send_committed(&hq, &hq_node.sessions, &owner, &hop1);
    let claude_wake_1 = hq_node
        .recorded
        .wait_for_agent("claude-ada", 1, Duration::from_secs(10));
    assert!(claude_wake_1.prompt.contains("LIVE-RING-7741"));

    let claude_env = RuntimeEnv {
        node: &hq_node,
        employee_id: &claude_ada,
        handle: "claude-ada",
        agent_os_dir: &agent_os_dir,
        workspace: &ws,
    };
    let claude_reply_1 = run_claude_live_turn(
        &claude_env,
        &format!(
            "You were woken by Agentic Org:\n{}\n\nExecute `agent-os msg inbox`, then execute `agent-os msg send --to codex-ben --body \"CLAUDE_TO_CODEX_7741\"`, and confirm with RECEIVED_CLAUDE_7741.",
            claude_wake_1.prompt
        ),
    );
    eprintln!("[LIVE CLAUDE TURN 1] {claude_reply_1}");

    // Hop 2: Satellite 1 wakes Codex Ben on Claude's cross-office DM -> real Codex CLI reads inbox
    // via `agent-os` and sends Sat1 -> Sat2 DM to Agy Cy on Satellite 2.
    let codex_wake = sat1_node
        .recorded
        .wait_for_agent("codex-ben", 1, Duration::from_secs(15));
    assert!(
        codex_wake.prompt.contains("CLAUDE_TO_CODEX_7741"),
        "Codex wake missing Claude nonce: {}",
        codex_wake.prompt
    );
    let codex_env = RuntimeEnv {
        node: &sat1_node,
        employee_id: &codex_ben,
        handle: "codex-ben",
        agent_os_dir: &agent_os_dir,
        workspace: &ws,
    };
    let codex_reply = run_codex_live_turn(
        &codex_env,
        &format!(
            "You were woken by Agentic Org:\n{}\n\nExecute `agent-os msg inbox`, then execute `agent-os msg send --to agy-cy --body \"CODEX_TO_AGY_7741\"`, and confirm with RECEIVED_CODEX_7741.",
            codex_wake.prompt
        ),
    );
    eprintln!("[LIVE CODEX TURN 1] {codex_reply}");

    // Hop 3: Satellite 2 wakes Agy Cy on Codex's Sat1 -> HQ -> Sat2 DM -> real Agy CLI reads inbox
    // via `agent-os` and sends Sat2 -> HQ DM back to Claude Ada on HQ.
    let agy_wake = sat2_node
        .recorded
        .wait_for_agent("agy-cy", 1, Duration::from_secs(15));
    assert!(
        agy_wake.prompt.contains("CODEX_TO_AGY_7741"),
        "Agy wake missing Codex nonce: {}",
        agy_wake.prompt
    );
    let agy_env = RuntimeEnv {
        node: &sat2_node,
        employee_id: &agy_cy,
        handle: "agy-cy",
        agent_os_dir: &agent_os_dir,
        workspace: &ws,
    };
    let agy_reply = run_agy_live_turn(
        &agy_env,
        &format!(
            "You were woken by Agentic Org:\n{}\n\nIn ONE shell command (`run_command`), run `agent-os msg inbox && agent-os msg send --to claude-ada --body \"AGY_TO_CLAUDE_7741\"` (if sandbox blocks the socket, retry without sandbox), and confirm with RECEIVED_AGY_7741.",
            agy_wake.prompt
        ),
    );
    eprintln!("[LIVE AGY TURN 1] {agy_reply}");

    // Hop 4: HQ wakes Claude Ada on Agy's Sat2 -> HQ DM -> real Claude Code confirms receipt!
    let claude_wake_2 = hq_node
        .recorded
        .wait_for_agent("claude-ada", 2, Duration::from_secs(15));
    assert!(
        claude_wake_2.prompt.contains("AGY_TO_CLAUDE_7741"),
        "Claude turn 2 wake missing Agy nonce: {}",
        claude_wake_2.prompt
    );
    let claude_reply_2 = run_claude_live_turn(
        &claude_env,
        &format!(
            "You were woken by Agentic Org:\n{}\n\nRun `agent-os msg inbox` and reply with exactly: RING_COMPLETE_7741",
            claude_wake_2.prompt
        ),
    );
    eprintln!("[LIVE CLAUDE TURN 2] {claude_reply_2}");
    assert!(
        claude_reply_2.contains("RING_COMPLETE_7741"),
        "Claude did not confirm final receipt: {claude_reply_2}"
    );
}
