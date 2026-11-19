use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::{json, Value};

use crate::agent::{
    format_event_human, AgentEvent, AgentStatus, EventLog, PendingApproval, PermissionPreset,
};

/// Configuration for a long-lived Claude Code `stream-json` session.
#[derive(Debug, Clone)]
pub struct ClaudeConfig {
    pub binary: String,
    pub cwd: Option<PathBuf>,
    /// Optional isolated CLI config/state directory (`CLAUDE_CONFIG_DIR`).
    /// Relocates sessions, memory, settings, and credentials; does NOT sandbox workspace files.
    pub config_dir: Option<PathBuf>,
    /// Optional 1-year `CLAUDE_CODE_OAUTH_TOKEN` (from `claude setup-token`) for remote headless offices.
    pub oauth_token: Option<String>,
    pub model: Option<String>,
    /// Friendly security preset (maps to `--permission-mode` + OS sandbox settings).
    pub preset: Option<PermissionPreset>,
    /// Explicit override for the OS sandbox (`None` follows `preset`, or off when no preset).
    pub sandbox: Option<bool>,
    /// Raw Claude `--permission-mode` override (takes precedence over `preset` if set).
    pub permission_mode: Option<String>,
    pub tools: Option<String>,
    pub allowed_tools: Vec<String>,
    pub max_budget_usd: Option<String>,
    pub resume: Option<String>,
    pub stdio_approvals: bool,
    pub auto_approve: bool,
    pub forward_subagent_text: bool,
    pub include_partials: bool,
    pub echo_events: bool,
    pub extra_args: Vec<String>,
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self {
            binary: "claude".to_string(),
            cwd: None,
            config_dir: None,
            oauth_token: None,
            model: None,
            preset: None,
            sandbox: None,
            permission_mode: None,
            tools: None,
            allowed_tools: Vec::new(),
            max_budget_usd: None,
            resume: None,
            stdio_approvals: false,
            auto_approve: false,
            forward_subagent_text: true,
            include_partials: false,
            echo_events: false,
            extra_args: Vec::new(),
        }
    }
}

struct ClaudeMutableState {
    session_id: Option<String>,
    state: String,
    turns_completed: u64,
    pending_approvals: Vec<PendingApproval>,
}

struct ClaudeShared {
    pid: u32,
    alive: AtomicBool,
    next_req_id: AtomicU64,
    auto_approve: bool,
    echo_events: bool,
    stdin: Mutex<Option<ChildStdin>>,
    meta: Mutex<ClaudeMutableState>,
    events: EventLog,
}

impl ClaudeShared {
    fn write_json(&self, value: &Value) -> io::Result<()> {
        let mut line = serde_json::to_string(value)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        line.push('\n');
        let mut guard = self
            .stdin
            .lock()
            .map_err(|_| io::Error::other("stdin lock poisoned"))?;
        let Some(stdin) = guard.as_mut() else {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "claude stdin is already closed",
            ));
        };
        stdin.write_all(line.as_bytes())?;
        stdin.flush()
    }

    fn next_request_id(&self) -> String {
        let n = self.next_req_id.fetch_add(1, Ordering::Relaxed);
        format!("host-{n}")
    }

    fn record_event(&self, kind: &str, text: Option<String>, raw: Value) -> AgentEvent {
        let ev = self.events.push(kind, text, raw);
        if self.echo_events {
            println!("{}", format_event_human(&ev));
        }
        ev
    }

    fn set_state(&self, next: &str) {
        if let Ok(mut meta) = self.meta.lock() {
            meta.state = next.to_string();
        }
    }

    fn close_stdin(&self) {
        if let Ok(mut guard) = self.stdin.lock() {
            let _ = guard.take();
        }
    }
}

/// Handle to a running long-lived Claude Code `stream-json` child process.
pub struct ClaudeSession {
    child: Mutex<Child>,
    shared: Arc<ClaudeShared>,
    stdout_thread: Mutex<Option<JoinHandle<()>>>,
    stderr_thread: Mutex<Option<JoinHandle<()>>>,
}

impl ClaudeSession {
    /// Spawns Claude Code with bidirectional `stream-json` and performs the `initialize` handshake.
    pub fn spawn(config: ClaudeConfig) -> io::Result<Arc<Self>> {
        let mut cmd = Command::new(&config.binary);
        cmd.args([
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
        ]);

        if config.forward_subagent_text {
            cmd.arg("--forward-subagent-text");
        }
        if config.include_partials {
            cmd.arg("--include-partial-messages");
        }
        if let Some(model) = &config.model {
            cmd.args(["--model", model]);
        }

        let effective_mode: Option<&str> = config
            .permission_mode
            .as_deref()
            .or_else(|| config.preset.map(PermissionPreset::claude_permission_mode));
        if let Some(mode) = effective_mode {
            cmd.args(["--permission-mode", mode]);
        }

        let sandbox_enabled = config
            .sandbox
            .or_else(|| config.preset.map(PermissionPreset::default_sandbox_enabled))
            .unwrap_or(false);
        if let Some(settings_json) = config
            .preset
            .unwrap_or(PermissionPreset::SafeAsk)
            .claude_sandbox_settings(sandbox_enabled)
        {
            cmd.args(["--settings", &settings_json]);
        }

        let needs_stdio_approvals = config.stdio_approvals
            || matches!(
                config.preset,
                Some(PermissionPreset::SafeAsk | PermissionPreset::WorkspaceEdits)
            );
        if needs_stdio_approvals {
            cmd.args(["--permission-prompt-tool", "stdio"]);
        }

        if let Some(cfg_dir) = &config.config_dir {
            let sec_dir = cfg_dir.join("securestorage");
            cmd.env("CLAUDE_CONFIG_DIR", cfg_dir);
            cmd.env("CLAUDE_SECURESTORAGE_CONFIG_DIR", sec_dir);
        }
        if let Some(token) = &config.oauth_token {
            cmd.env("CLAUDE_CODE_OAUTH_TOKEN", token);
        }
        if let Some(tools) = &config.tools {
            cmd.args(["--tools", tools]);
        }
        if !config.allowed_tools.is_empty() {
            cmd.arg("--allowedTools");
            for tool in &config.allowed_tools {
                cmd.arg(tool);
            }
        }
        if let Some(budget) = &config.max_budget_usd {
            cmd.args(["--max-budget-usd", budget]);
        }
        if let Some(resume) = &config.resume {
            cmd.args(["--resume", resume]);
        }
        for arg in &config.extra_args {
            cmd.arg(arg);
        }
        if let Some(cwd) = &config.cwd {
            cmd.current_dir(cwd);
        }

        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        // SAFETY: setsid is async-signal-safe and isolates the child in its own session/group.
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }

        let mut child = cmd.spawn()?;
        let pid = child.id();
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("failed to capture child stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("failed to capture child stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("failed to capture child stderr"))?;

        let shared = Arc::new(ClaudeShared {
            pid,
            alive: AtomicBool::new(true),
            next_req_id: AtomicU64::new(1),
            auto_approve: config.auto_approve,
            echo_events: config.echo_events,
            stdin: Mutex::new(Some(stdin)),
            meta: Mutex::new(ClaudeMutableState {
                session_id: None,
                state: "starting".to_string(),
                turns_completed: 0,
                pending_approvals: Vec::new(),
            }),
            events: EventLog::default(),
        });

        let out_shared = Arc::clone(&shared);
        let stdout_thread = thread::Builder::new()
            .name("claude-stdout".to_string())
            .spawn(move || read_stdout_loop(stdout, out_shared))?;

        let err_shared = Arc::clone(&shared);
        let stderr_thread = thread::Builder::new()
            .name("claude-stderr".to_string())
            .spawn(move || read_stderr_loop(stderr, err_shared))?;

        let session = Arc::new(Self {
            child: Mutex::new(child),
            shared,
            stdout_thread: Mutex::new(Some(stdout_thread)),
            stderr_thread: Mutex::new(Some(stderr_thread)),
        });

        // Kick off the stream-json control handshake immediately.
        session.send_initialize()?;
        Ok(session)
    }

    pub fn pid(&self) -> u32 {
        self.shared.pid
    }

    pub fn status(&self) -> AgentStatus {
        let meta = self.shared.meta.lock().expect("meta lock poisoned");
        AgentStatus {
            backend: "claude".to_string(),
            pid: self.shared.pid,
            alive: self.shared.alive.load(Ordering::Relaxed),
            session_id: meta.session_id.clone(),
            state: meta.state.clone(),
            turns_completed: meta.turns_completed,
            pending_approvals: meta.pending_approvals.clone(),
            last_seq: self.shared.events.last_seq(),
        }
    }

    fn send_initialize(&self) -> io::Result<String> {
        let rid = self.shared.next_request_id();
        let msg = json!({
            "type": "control_request",
            "request_id": rid,
            "request": { "subtype": "initialize" }
        });
        self.shared.write_json(&msg)?;
        Ok(rid)
    }

    /// Sends a user message immediately without blocking on prior turns or background tasks.
    pub fn send_user_message(&self, content: &str) -> io::Result<AgentEvent> {
        let msg = json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": content,
            }
        });
        self.shared.write_json(&msg)?;
        self.shared.set_state("busy");
        Ok(self
            .shared
            .record_event("user_sent", Some(content.to_string()), msg))
    }

    /// Sends a user message and waits for the next `result` event emitted after it.
    pub fn send_and_wait(&self, content: &str, timeout: Duration) -> io::Result<AgentEvent> {
        let sent = self.send_user_message(content)?;
        self.shared
            .events
            .wait_for_result(sent.seq, timeout)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "timed out after {}ms waiting for result",
                        timeout.as_millis()
                    ),
                )
            })
    }

    /// Sends a mid-turn `interrupt` control request without killing the session.
    pub fn send_interrupt(&self) -> io::Result<String> {
        let rid = self.shared.next_request_id();
        let msg = json!({
            "type": "control_request",
            "request_id": rid,
            "request": { "subtype": "interrupt" }
        });
        self.shared.write_json(&msg)?;
        self.shared
            .record_event("interrupt_sent", Some(rid.clone()), msg);
        Ok(rid)
    }

    /// Approves or denies a pending `can_use_tool` permission request.
    pub fn respond_approval(
        &self,
        request_id: &str,
        allow: bool,
        reason: Option<&str>,
    ) -> io::Result<()> {
        let response_body = if allow {
            json!({ "behavior": "allow" })
        } else {
            json!({
                "behavior": "deny",
                "message": reason.unwrap_or("denied by controller"),
            })
        };
        let msg = json!({
            "type": "control_response",
            "response": {
                "subtype": "success",
                "request_id": request_id,
                "response": response_body,
            }
        });
        self.shared.write_json(&msg)?;

        if let Ok(mut meta) = self.shared.meta.lock() {
            meta.pending_approvals
                .retain(|p| p.request_id != request_id);
            if meta.pending_approvals.is_empty() && meta.state == "awaiting_approval" {
                meta.state = "busy".to_string();
            }
        }

        let summary = if allow {
            format!("allowed {request_id}")
        } else {
            format!("denied {request_id}")
        };
        self.shared
            .record_event("approval_replied", Some(summary), msg);
        Ok(())
    }

    pub fn events_since(&self, after_seq: u64) -> Vec<AgentEvent> {
        self.shared.events.since(after_seq)
    }

    pub fn wait_events_since(&self, after_seq: u64, timeout: Duration) -> Vec<AgentEvent> {
        self.shared.events.wait_since(after_seq, timeout)
    }

    /// Gracefully stops the session by closing stdin (and sending SIGTERM if needed).
    pub fn request_stop(&self) {
        self.shared.close_stdin();
        // SAFETY: sending SIGTERM to our own child PID is safe and lets Claude tear down tool trees.
        unsafe {
            let _ = libc::kill(self.shared.pid as libc::pid_t, libc::SIGTERM);
        }
    }

    /// Waits for the Claude child process to exit and joins reader threads.
    pub fn wait(&self) -> io::Result<ExitStatus> {
        let status = {
            let mut child = self
                .child
                .lock()
                .map_err(|_| io::Error::other("child lock poisoned"))?;
            child.wait()?
        };
        self.shared.alive.store(false, Ordering::SeqCst);
        self.shared.set_state("exited");
        self.shared.events.mark_closed();
        self.join_readers();
        Ok(status)
    }

    fn join_readers(&self) {
        if let Ok(mut guard) = self.stdout_thread.lock() {
            if let Some(h) = guard.take() {
                let _ = h.join();
            }
        }
        if let Ok(mut guard) = self.stderr_thread.lock() {
            if let Some(h) = guard.take() {
                let _ = h.join();
            }
        }
    }
}

fn read_stdout_loop(stdout: std::process::ChildStdout, shared: Arc<ClaudeShared>) {
    let reader = BufReader::new(stdout);
    for line_res in reader.lines() {
        let Ok(line) = line_res else {
            break;
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(trimmed) {
            Ok(val) => handle_claude_frame(&shared, val),
            Err(_) => {
                shared.record_event("raw", Some(trimmed.to_string()), json!({ "line": trimmed }));
            }
        }
    }
    shared.alive.store(false, Ordering::SeqCst);
    shared.set_state("exited");
    shared.events.mark_closed();
}

fn read_stderr_loop(stderr: std::process::ChildStderr, shared: Arc<ClaudeShared>) {
    let reader = BufReader::new(stderr);
    for line_res in reader.lines() {
        let Ok(line) = line_res else {
            break;
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        shared.record_event(
            "stderr",
            Some(trimmed.to_string()),
            json!({ "stderr": trimmed }),
        );
    }
}

fn handle_claude_frame(shared: &ClaudeShared, val: Value) {
    let msg_type = val
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();

    if let Some(sid) = val.get("session_id").and_then(Value::as_str) {
        if let Ok(mut meta) = shared.meta.lock() {
            if meta.session_id.is_none() {
                meta.session_id = Some(sid.to_string());
            }
        }
    }

    match msg_type.as_str() {
        "system" => {
            let subtype = val
                .get("subtype")
                .and_then(Value::as_str)
                .unwrap_or("system");
            if subtype == "init" {
                let sid = val
                    .get("session_id")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                if let Ok(mut meta) = shared.meta.lock() {
                    meta.session_id = Some(sid.to_string());
                    meta.state = "busy".to_string();
                }
                shared.record_event("init", Some(format!("session_id={sid}")), val);
            } else {
                let summary = val
                    .get("summary")
                    .or_else(|| val.get("message"))
                    .and_then(Value::as_str)
                    .map(|s| format!("{subtype}: {s}"))
                    .unwrap_or_else(|| subtype.to_string());
                shared.record_event("system", Some(summary), val);
            }
        }
        "assistant" => {
            let is_subagent = val
                .get("parent_tool_use_id")
                .map(|v| !v.is_null())
                .unwrap_or(false);
            let mut texts = Vec::new();
            let mut tools = Vec::new();
            if let Some(blocks) = val
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(Value::as_array)
            {
                for block in blocks {
                    match block.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            if let Some(t) = block.get("text").and_then(Value::as_str) {
                                if !t.trim().is_empty() {
                                    texts.push(t.to_string());
                                }
                            }
                        }
                        Some("tool_use") => {
                            let name = block
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or("unknown");
                            let input_preview = block
                                .get("input")
                                .map(|v| truncate(&v.to_string(), 120))
                                .unwrap_or_default();
                            tools.push(format!("{name}({input_preview})"));
                        }
                        _ => {}
                    }
                }
            }
            if !texts.is_empty() {
                let kind = if is_subagent {
                    "subagent_text"
                } else {
                    "assistant_text"
                };
                shared.record_event(kind, Some(texts.join("\n")), val.clone());
            }
            if !tools.is_empty() {
                let kind = if is_subagent {
                    "subagent_tool_use"
                } else {
                    "tool_use"
                };
                shared.record_event(kind, Some(tools.join(", ")), val);
            } else if texts.is_empty() {
                shared.record_event("assistant", None, val);
            }
        }
        "user" => {
            let is_subagent = val
                .get("parent_tool_use_id")
                .map(|v| !v.is_null())
                .unwrap_or(false);
            let summary = extract_tool_result_summary(&val);
            let kind = if is_subagent {
                "subagent_result"
            } else {
                "tool_result"
            };
            shared.record_event(kind, summary, val);
        }
        "result" => {
            let result_text = val
                .get("result")
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .or_else(|| {
                    val.get("subtype")
                        .and_then(Value::as_str)
                        .map(|s| format!("({s})"))
                });
            if let Ok(mut meta) = shared.meta.lock() {
                meta.turns_completed += 1;
                meta.state = "idle".to_string();
            }
            shared.record_event("result", result_text, val);
        }
        "control_request" => {
            let request_id = val
                .get("request_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let req = val.get("request").cloned().unwrap_or(Value::Null);
            let subtype = req.get("subtype").and_then(Value::as_str).unwrap_or("");
            if subtype == "can_use_tool" {
                let tool_name = req
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_string();
                let input = req.get("input").cloned().unwrap_or(Value::Null);
                let summary = format!(
                    "id={request_id} tool={tool_name} input={}",
                    truncate(&input.to_string(), 140)
                );

                if shared.auto_approve {
                    let reply = json!({
                        "type": "control_response",
                        "response": {
                            "subtype": "success",
                            "request_id": request_id,
                            "response": { "behavior": "allow" }
                        }
                    });
                    let _ = shared.write_json(&reply);
                    shared.record_event("approval_auto_allowed", Some(summary), val);
                } else {
                    if let Ok(mut meta) = shared.meta.lock() {
                        meta.pending_approvals.push(PendingApproval {
                            request_id: request_id.clone(),
                            tool_name,
                            input,
                        });
                        meta.state = "awaiting_approval".to_string();
                    }
                    shared.record_event("approval_requested", Some(summary), val);
                }
            } else {
                shared.record_event("control_request", Some(subtype.to_string()), val);
            }
        }
        "control_response" => {
            if let Ok(mut meta) = shared.meta.lock() {
                if meta.state == "starting" {
                    meta.state = "idle".to_string();
                }
            }
            let summary = val
                .get("response")
                .and_then(|r| r.get("subtype"))
                .and_then(Value::as_str)
                .map(ToString::to_string);
            shared.record_event("control_response", summary, val);
        }
        "stream_event" => {
            if let Some(delta) = val
                .get("event")
                .and_then(|e| e.get("delta"))
                .and_then(|d| d.get("text"))
                .and_then(Value::as_str)
            {
                shared.record_event("stream_delta", Some(delta.to_string()), val);
            }
        }
        other => {
            shared.record_event(other, None, val);
        }
    }
}

fn extract_tool_result_summary(val: &Value) -> Option<String> {
    if let Some(blocks) = val
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
    {
        for block in blocks {
            if block.get("type").and_then(Value::as_str) == Some("tool_result") {
                if let Some(content) = block.get("content") {
                    let s = match content {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    return Some(truncate(&s, 160));
                }
            }
        }
    }
    None
}

fn truncate(s: &str, max_chars: usize) -> String {
    let single_line = s.replace(['\r', '\n'], " ");
    let mut chars = single_line.chars();
    let collected: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{collected}...")
    } else {
        collected
    }
}
