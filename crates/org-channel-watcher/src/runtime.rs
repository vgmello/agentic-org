//! The production watcher runtime: a vendor-neutral CLI runner delegating tool-less argv
//! construction and stdout parsing to the registered runtime adapter's tool-less prompt
//! capability (`WATCH-4`, `TASK-259`).
//!
//! Runs each prompt inside a private owner-only (`0700`) temporary work directory with bounded
//! stdout draining and immediate child kill/reap on timeout or cancellation. Drivers without a
//! true tool-less mode return `None` from `toolless_prompt()` and fail closed.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use org_copilot::{CancelToken, CopilotRuntime, RunOutput, RunRequest, RuntimeError, Usage};
use org_office::organization_family::records::{ModelProfile, ProviderConnection};
use org_office_sessions::RuntimeRegistry;

use super::WatcherRuntimeProvider;

/// Maximum wall time allowed for a CLI availability probe (`--version` / `--help`) before the
/// child process is killed, reaped, and treated as unavailable.
const AVAILABILITY_PROBE_TIMEOUT: Duration = Duration::from_secs(2);

type OutputParser = Box<dyn Fn(&[u8]) -> Option<(String, Option<Usage>)> + Send + Sync>;

/// Production CLI adapter resolving runtime drivers through [`RuntimeRegistry`] and enforcing
/// strict tool-less runs (`WATCH-4`: no tools, no files, no MCP servers, no slash commands).
/// Drivers without a true tool-less CLI capability fail closed (`None`) per `doc-8` and
/// `ADR-0024` d10.
pub struct CliWatcherRuntimeProvider {
    runtimes: RuntimeRegistry,
}

impl CliWatcherRuntimeProvider {
    pub fn new(runtimes: RuntimeRegistry) -> Self {
        Self { runtimes }
    }
}

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
        let adapter = self.runtimes.resolve(&conn.runtime_driver).ok()?;
        // Fail closed unless the adapter explicitly declares a strict tool-less CLI mode.
        let toolless = adapter.toolless_prompt()?;
        let availability_args = toolless.availability_args();
        let run_args = toolless.run_args(&profile.key);
        let binary = self
            .runtimes
            .resolve_binary(&*adapter, None)
            .unwrap_or_else(|_| PathBuf::from(adapter.binary_name()));
        let parse_output: OutputParser = Box::new(move |bytes: &[u8]| {
            let parsed = adapter.toolless_prompt()?.parse_output(bytes)?;
            let usage = parsed.usage.map(|u| Usage {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
            });
            Some((parsed.text, usage))
        });
        Some(Arc::new(ToollessCliRuntime {
            binary,
            availability_args,
            run_args,
            parse_output,
        }))
    }
}

struct TempWorkDir(PathBuf);

impl TempWorkDir {
    fn create(prefix: &str) -> Result<Self, RuntimeError> {
        let mut nonce = [0u8; 8];
        org_platform::confinement::fill_entropy(&mut nonce).map_err(|_| RuntimeError::Failed)?;
        let suffix = u64::from_be_bytes(nonce);
        let path =
            std::env::temp_dir().join(format!("{prefix}-{}-{suffix:016x}", std::process::id()));
        org_platform::fs::create_owner_only_dir(&path).map_err(|_| RuntimeError::Failed)?;
        Ok(Self(path))
    }
}

impl Drop for TempWorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct ToollessCliRuntime {
    binary: PathBuf,
    availability_args: &'static [&'static str],
    run_args: Vec<String>,
    parse_output: OutputParser,
}

impl CopilotRuntime for ToollessCliRuntime {
    fn available(&self) -> bool {
        probe_binary_available(
            &self.binary,
            self.availability_args,
            AVAILABILITY_PROBE_TIMEOUT,
        )
    }

    fn run(&self, request: &RunRequest, cancel: &CancelToken) -> Result<RunOutput, RuntimeError> {
        if cancel.is_cancelled() {
            return Err(RuntimeError::TimedOut);
        }
        let work_dir = TempWorkDir::create("org-watcher")?;
        let mut cmd = Command::new(&self.binary);
        cmd.current_dir(&work_dir.0)
            .args(&self.run_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd.spawn().map_err(|_| RuntimeError::Failed)?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(request.prompt.as_bytes());
        }
        let max_bytes = request.max_output_bytes.saturating_mul(4).max(256 * 1024);
        let stdout_bytes = wait_child_bounded(&mut child, cancel, request.timeout, max_bytes)?;
        let (text, usage) = (self.parse_output)(&stdout_bytes).ok_or(RuntimeError::Failed)?;
        Ok(RunOutput { text, usage })
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

/// Probes whether `binary` exits cleanly within `timeout`; kills and reaps the child immediately
/// on timeout or I/O error so a hanging binary can never stall a multiplexed channel loop.
pub(crate) fn probe_binary_available(binary: &Path, args: &[&str], timeout: Duration) -> bool {
    let mut child = match Command::new(binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return false;
                }
                std::thread::sleep(Duration::from_millis(15));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}
