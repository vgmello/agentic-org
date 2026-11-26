//! The production watcher runtime: a tool-less Claude CLI adapter in a private temporary work
//! directory, with a bounded child wait and JSON-fence stripping.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use org_copilot::{CancelToken, CopilotRuntime, RunOutput, RunRequest, RuntimeError, Usage};
use org_office::organization_family::records::{ModelProfile, ProviderConnection};

use super::WatcherRuntimeProvider;

/// Production CLI adapter implementing strict tool-less runs (`WATCH-4`: no tools, no files, no
/// MCP servers, no slash commands). Drivers without a true tool-less CLI mode fail closed (`None`)
/// per `doc-8` and `ADR-0024` d10.
pub struct CliWatcherRuntimeProvider;

impl WatcherRuntimeProvider for CliWatcherRuntimeProvider {
    fn runtime_for_profile(
        &self,
        profile: &ModelProfile,
        connection: Option<&ProviderConnection>,
    ) -> Option<Arc<dyn CopilotRuntime>> {
        let conn = connection?;
        if !conn.enabled || conn.archived {
            return None;
        }
        match conn.runtime_driver.as_str() {
            "claude-code" => Some(Arc::new(ClaudeToollessAdapter {
                model: profile.key.clone(),
            })),
            // Codex CLI currently has no true zero-tool/zero-file mode (`--sandbox read-only`
            // still exposes file-reading tools), so refuse at composition (`WATCH-4`, `doc-8`).
            _ => None,
        }
    }
}

struct TempWorkDir(PathBuf);

impl TempWorkDir {
    fn create(prefix: &str) -> Result<Self, RuntimeError> {
        static SEQ: AtomicU64 = AtomicU64::new(1);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("{prefix}-{}-{seq}", std::process::id()));
        std::fs::create_dir_all(&path).map_err(|_| RuntimeError::Failed)?;
        Ok(Self(path))
    }
}

impl Drop for TempWorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct ClaudeToollessAdapter {
    model: String,
}

impl CopilotRuntime for ClaudeToollessAdapter {
    fn available(&self) -> bool {
        Command::new("claude")
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    fn run(&self, request: &RunRequest, cancel: &CancelToken) -> Result<RunOutput, RuntimeError> {
        if cancel.is_cancelled() {
            return Err(RuntimeError::TimedOut);
        }
        let work_dir = TempWorkDir::create("org-watcher-claude")?;
        let mut cmd = Command::new("claude");
        cmd.current_dir(&work_dir.0)
            .args([
                "-p",
                "--tools",
                "",
                "--strict-mcp-config",
                "--disable-slash-commands",
                "--output-format",
                "json",
                "--no-session-persistence",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if !self.model.is_empty() && self.model.starts_with("claude-") {
            cmd.args(["--model", &self.model]);
        }
        let mut child = cmd.spawn().map_err(|_| RuntimeError::Failed)?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(request.prompt.as_bytes());
        }
        let max_bytes = request.max_output_bytes.saturating_mul(4).max(256 * 1024);
        let stdout_bytes = wait_child_bounded(&mut child, cancel, request.timeout, max_bytes)?;
        let env: serde_json::Value =
            serde_json::from_slice(&stdout_bytes).map_err(|_| RuntimeError::Failed)?;
        if env
            .get("is_error")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return Err(RuntimeError::Failed);
        }
        let raw = env
            .get("result")
            .and_then(|v| v.as_str())
            .ok_or(RuntimeError::Failed)?;
        let usage = env.get("usage").map(|u| Usage {
            input_tokens: u.get("input_tokens").and_then(|v| v.as_u64()),
            output_tokens: u.get("output_tokens").and_then(|v| v.as_u64()),
        });
        Ok(RunOutput {
            text: strip_markdown_json_fence(raw),
            usage,
        })
    }
}

/// Drains child stdout on a helper reader thread while polling `try_wait()` against `cancel` and
/// `timeout`; kills and reaps the child immediately on cancellation or timeout (`High #3`).
fn wait_child_bounded(
    child: &mut Child,
    cancel: &CancelToken,
    timeout: Duration,
    max_bytes: usize,
) -> Result<Vec<u8>, RuntimeError> {
    let stdout = child.stdout.take().ok_or(RuntimeError::Failed)?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut limited = stdout.take(max_bytes as u64 + 1);
        let _ = limited.read_to_end(&mut buf);
        buf
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        if cancel.is_cancelled() || Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            return Err(RuntimeError::TimedOut);
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(RuntimeError::Failed);
            }
        }
    };
    let bytes = reader.join().map_err(|_| RuntimeError::Failed)?;
    if !status.success() || bytes.len() > max_bytes {
        return Err(RuntimeError::Failed);
    }
    Ok(bytes)
}

fn strip_markdown_json_fence(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix("```json") {
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim().to_owned();
        }
    }
    if let Some(rest) = trimmed.strip_prefix("```") {
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim().to_owned();
        }
    }
    trimmed.to_owned()
}
