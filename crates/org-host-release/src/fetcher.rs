//! Consumer-owned [`ReleaseFetcher`] port plus production HTTPS and offline local adapters.
//!
//! All network and subprocess boundaries are strictly bounded in bytes and wall-clock time,
//! refuse protocol downgrades (strict HTTPS-only in production), and route every external
//! diagnostic through `org_redact::redact_text`.

use std::fmt;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use org_release::{validate_pointer_url, UrlPolicy, MAX_PAYLOAD_BYTES};

const READ_CHUNK_BYTES: usize = 64 * 1024;
const MAX_STDERR_BYTES: u64 = 4 * 1024;
const MAX_ERROR_MESSAGE_BYTES: usize = 512;
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const DEFAULT_MAX_REDIRECTS: u32 = 5;

/// Sanitized fetch error; external stderr is redacted before entering this type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// URL failed strict policy validation (e.g. non-HTTPS in production or traversal).
    InvalidUrl,
    /// Fetch tool or local root is unavailable or unsafe.
    Unavailable(String),
    /// Remote or local source exceeded the configured byte limit.
    TooLarge { limit: u64 },
    /// Fetch exceeded the configured wall-clock timeout.
    TimedOut { timeout_secs: u64 },
    /// Remote HTTP or subprocess transport failure (redacted).
    Transport(String),
    /// Local filesystem or sink I/O failure.
    Io(String),
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl => write!(f, "release fetch refused: invalid or disallowed URL"),
            Self::Unavailable(msg) => write!(f, "release fetcher unavailable: {msg}"),
            Self::TooLarge { limit } => {
                write!(f, "release download exceeded {limit}-byte safety limit")
            }
            Self::TimedOut { timeout_secs } => {
                write!(f, "release fetch timed out after {timeout_secs}s")
            }
            Self::Transport(msg) => write!(f, "release fetch failed: {msg}"),
            Self::Io(msg) => write!(f, "release fetch I/O error: {msg}"),
        }
    }
}

impl std::error::Error for FetchError {}

/// Consumer-owned port for fetching release pointers, signatures, and signed archives.
pub trait ReleaseFetcher: Send + Sync {
    /// Fetch small bounded metadata (such as `pointer.v1.json` or raw 64-byte `pointer.v1.sig`).
    fn fetch_bytes(
        &self,
        url: &str,
        max_bytes: usize,
        timeout: Duration,
    ) -> Result<Vec<u8>, FetchError>;

    /// Stream a release archive into `sink`, enforcing `max_bytes` (capped at 2 GiB) and `timeout`.
    fn fetch_to_writer(
        &self,
        url: &str,
        max_bytes: u64,
        timeout: Duration,
        sink: &mut dyn Write,
    ) -> Result<u64, FetchError>;
}

/// Production HTTPS-only release fetcher backed by the system `curl` binary.
///
/// Enforces:
/// - `UrlPolicy::HttpsOnly` before spawning anything;
/// - `--proto =https --proto-redir =https --tlsv1.2` so redirects cannot downgrade to HTTP or `file://`;
/// - cleared subprocess environment (preserving only proxy variables);
/// - independent Rust-side streaming byte limit (never exceeding 2 GiB) and wall-clock deadline;
/// - full secret redaction on captured stderr.
#[derive(Debug, Clone)]
pub struct HttpsReleaseFetcher {
    curl_program: Option<PathBuf>,
    connect_timeout: Duration,
    max_redirects: u32,
}

impl Default for HttpsReleaseFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpsReleaseFetcher {
    pub fn new() -> Self {
        Self {
            curl_program: None,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            max_redirects: DEFAULT_MAX_REDIRECTS,
        }
    }

    /// Construct a fetcher bound to an explicit executable path (used in deterministic tests).
    pub fn with_program(program: PathBuf) -> Self {
        Self {
            curl_program: Some(program),
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            max_redirects: DEFAULT_MAX_REDIRECTS,
        }
    }

    fn resolve_curl(&self) -> Result<PathBuf, FetchError> {
        if let Some(explicit) = &self.curl_program {
            return org_platform::fs::resolve_safe_executable_file(explicit).ok_or_else(|| {
                FetchError::Unavailable(
                    "configured HTTPS fetcher binary is not a safe executable".into(),
                )
            });
        }
        let mut search_dirs: Vec<PathBuf> = std::env::var_os("PATH")
            .map(|paths| std::env::split_paths(&paths).collect())
            .unwrap_or_default();
        for fallback in ["/usr/bin", "/bin", "/usr/local/bin", "/opt/homebrew/bin"] {
            let path = PathBuf::from(fallback);
            if !search_dirs.contains(&path) {
                search_dirs.push(path);
            }
        }
        org_platform::fs::find_safe_executable_in_dirs(search_dirs, "curl").ok_or_else(|| {
            FetchError::Unavailable(
                "required HTTPS fetcher `curl` was not found in PATH or standard system directories"
                    .into(),
            )
        })
    }

    fn build_curl_command(
        &self,
        program: &Path,
        url: &str,
        effective_limit: u64,
        total_secs: u64,
    ) -> Command {
        let connect_secs = self.connect_timeout.as_secs().max(1).min(total_secs);
        let mut cmd = Command::new(program);
        cmd.env_clear();
        for var in ["HTTPS_PROXY", "https_proxy", "NO_PROXY", "no_proxy"] {
            if let Some(val) = std::env::var_os(var) {
                cmd.env(var, val);
            }
        }
        // `-q` MUST be the very first argument so curl disables `~/.curlrc` even when it falls
        // back to `getpwuid` after `env_clear()`; `--globoff` prevents `[]`/`{}` URL globbing.
        cmd.args([
            "-q",
            "--globoff",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--tlsv1.2",
            "--silent",
            "--show-error",
            "--fail",
            "--location",
            "--max-redirs",
            &self.max_redirects.to_string(),
            "--connect-timeout",
            &connect_secs.to_string(),
            "--max-time",
            &total_secs.to_string(),
            "--max-filesize",
            &effective_limit.to_string(),
            "--",
            url,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
        cmd
    }

    fn run_curl(
        &self,
        url: &str,
        max_bytes: u64,
        timeout: Duration,
        sink: &mut dyn Write,
    ) -> Result<u64, FetchError> {
        validate_pointer_url(url, UrlPolicy::HttpsOnly).map_err(|_| FetchError::InvalidUrl)?;
        let effective_limit = max_bytes.min(MAX_PAYLOAD_BYTES);
        let program = self.resolve_curl()?;
        let total_secs = timeout.as_secs().max(1);
        let mut cmd = self.build_curl_command(&program, url, effective_limit, total_secs);

        let child = cmd.spawn().map_err(|e| {
            FetchError::Unavailable(format!("spawning HTTPS fetcher: {}", e.kind()))
        })?;
        let child_guard = ChildReapGuard::new(child);
        let (mut stdout, stderr) = child_guard.take_pipes()?;
        let stderr_thread = spawn_stderr_reader(stderr)?;

        let timed_out = Arc::new(AtomicBool::new(false));
        let (done_tx, watchdog) =
            spawn_timeout_watchdog(child_guard.slot(), Arc::clone(&timed_out), timeout)?;

        let deadline = Instant::now() + timeout;
        let stream_result = copy_bounded_streaming(
            &mut stdout,
            sink,
            effective_limit,
            deadline,
            &timed_out,
            child_guard.slot(),
        );
        drop(stdout);
        let _ = done_tx.send(());

        let status = child_guard.reap(stream_result.is_err());
        let _ = watchdog.join();
        let raw_stderr = stderr_thread.join().unwrap_or_default();

        interpret_curl_result(
            stream_result,
            status,
            &raw_stderr,
            timed_out.load(Ordering::Acquire),
            total_secs,
        )
    }
}

/// RAII drop guard ensuring a spawned `curl` child is ALWAYS killed and reaped on early errors
/// (such as watchdog thread spawn failure) or panics, preventing leaked processes or zombies.
struct ChildReapGuard {
    slot: Arc<Mutex<Option<Child>>>,
}

impl ChildReapGuard {
    fn new(child: Child) -> Self {
        Self {
            slot: Arc::new(Mutex::new(Some(child))),
        }
    }

    fn slot(&self) -> &Arc<Mutex<Option<Child>>> {
        &self.slot
    }

    fn take_pipes(&self) -> Result<(ChildStdout, ChildStderr), FetchError> {
        let mut guard = self
            .slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let child = guard
            .as_mut()
            .ok_or_else(|| FetchError::Io("HTTPS fetcher handle unavailable".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| FetchError::Io("missing stdout pipe from HTTPS fetcher".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| FetchError::Io("missing stderr pipe from HTTPS fetcher".into()))?;
        Ok((stdout, stderr))
    }

    fn reap(&self, kill_first: bool) -> Option<ExitStatus> {
        let mut guard = self
            .slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut proc_handle = guard.take()?;
        if kill_first {
            let _ = proc_handle.kill();
        }
        proc_handle.wait().ok()
    }
}

impl Drop for ChildReapGuard {
    fn drop(&mut self) {
        let _ = self.reap(true);
    }
}

fn spawn_stderr_reader(stderr: ChildStderr) -> Result<JoinHandle<Vec<u8>>, FetchError> {
    std::thread::Builder::new()
        .name("release-fetch-stderr".into())
        .spawn(move || {
            let mut buf = Vec::new();
            let _ = stderr.take(MAX_STDERR_BYTES).read_to_end(&mut buf);
            buf
        })
        .map_err(|e| FetchError::Io(format!("spawning stderr reader: {}", e.kind())))
}

fn spawn_timeout_watchdog(
    child_slot: &Arc<Mutex<Option<Child>>>,
    timed_out: Arc<AtomicBool>,
    timeout: Duration,
) -> Result<(mpsc::Sender<()>, JoinHandle<()>), FetchError> {
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let watchdog_child = Arc::clone(child_slot);
    let handle = std::thread::Builder::new()
        .name("release-fetch-timeout".into())
        .spawn(move || {
            if matches!(
                done_rx.recv_timeout(timeout),
                Err(mpsc::RecvTimeoutError::Timeout)
            ) {
                timed_out.store(true, Ordering::Release);
                kill_child(&watchdog_child);
            }
        })
        .map_err(|e| FetchError::Io(format!("spawning timeout watchdog: {}", e.kind())))?;
    Ok((done_tx, handle))
}

fn interpret_curl_result(
    stream_result: Result<u64, FetchError>,
    status: Option<ExitStatus>,
    raw_stderr: &[u8],
    did_timeout: bool,
    total_secs: u64,
) -> Result<u64, FetchError> {
    if did_timeout {
        return Err(FetchError::TimedOut {
            timeout_secs: total_secs,
        });
    }
    let copied = stream_result?;
    match status {
        Some(exit) if exit.success() => Ok(copied),
        Some(exit) => Err(FetchError::Transport(sanitize_stderr(
            raw_stderr,
            exit.code(),
        ))),
        None => Err(FetchError::Transport(
            "HTTPS fetcher exited unexpectedly".into(),
        )),
    }
}

impl ReleaseFetcher for HttpsReleaseFetcher {
    fn fetch_bytes(
        &self,
        url: &str,
        max_bytes: usize,
        timeout: Duration,
    ) -> Result<Vec<u8>, FetchError> {
        let mut out = Vec::new();
        self.run_curl(url, max_bytes as u64, timeout, &mut out)?;
        Ok(out)
    }

    fn fetch_to_writer(
        &self,
        url: &str,
        max_bytes: u64,
        timeout: Duration,
        sink: &mut dyn Write,
    ) -> Result<u64, FetchError> {
        self.run_curl(url, max_bytes, timeout, sink)
    }
}

fn copy_bounded_streaming(
    reader: &mut dyn Read,
    sink: &mut dyn Write,
    limit: u64,
    deadline: Instant,
    timed_out: &AtomicBool,
    child_slot: &Arc<Mutex<Option<Child>>>,
) -> Result<u64, FetchError> {
    let mut total = 0_u64;
    let mut buffer = [0_u8; READ_CHUNK_BYTES];
    loop {
        if timed_out.load(Ordering::Acquire) || Instant::now() >= deadline {
            timed_out.store(true, Ordering::Release);
            kill_child(child_slot);
            return Err(FetchError::TimedOut { timeout_secs: 0 });
        }
        let n = reader
            .read(&mut buffer)
            .map_err(|e| FetchError::Io(format!("reading response stream: {}", e.kind())))?;
        if n == 0 {
            return Ok(total);
        }
        let next = total.checked_add(n as u64);
        if next.is_none_or(|size| size > limit) {
            kill_child(child_slot);
            return Err(FetchError::TooLarge { limit });
        }
        total = next.unwrap_or(limit);
        sink.write_all(&buffer[..n])
            .map_err(|e| FetchError::Io(format!("writing staged bytes: {}", e.kind())))?;
    }
}

fn kill_child(child_slot: &Arc<Mutex<Option<Child>>>) {
    if let Ok(mut guard) = child_slot.lock() {
        if let Some(ref mut proc_handle) = *guard {
            let _ = proc_handle.kill();
        }
    }
}

fn sanitize_stderr(raw: &[u8], code: Option<i32>) -> String {
    let lossy = String::from_utf8_lossy(raw);
    let redacted = org_redact::redact_text(lossy.trim(), &[]);
    let single_line = redacted
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>();
    let compact = single_line.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        return match code {
            Some(c) => format!("HTTP transfer failed (exit {c})"),
            None => "HTTP transfer terminated by signal".into(),
        };
    }
    let suffix = " [truncated]";
    if compact.len() <= MAX_ERROR_MESSAGE_BYTES {
        compact
    } else {
        let cut = compact.floor_char_boundary(MAX_ERROR_MESSAGE_BYTES - suffix.len());
        format!("{}{suffix}", &compact[..cut])
    }
}

/// Deterministic offline / test-harness fetcher confined strictly to one canonical local directory.
///
/// Only accepts `file:///` URLs whose path lies beneath `allowed_root` without any symlink or `..`
/// traversal (`org_platform::fs::open_regular_beneath`), enforcing `is_file()` and `max_bytes`.
#[derive(Debug, Clone)]
pub struct LocalDirectoryFetcher {
    allowed_root: PathBuf,
}

impl LocalDirectoryFetcher {
    pub fn new(root: &Path) -> Result<Self, FetchError> {
        let canonical = root
            .canonicalize()
            .map_err(|e| FetchError::Unavailable(format!("local mirror root: {}", e.kind())))?;
        if !canonical.is_dir() {
            return Err(FetchError::Unavailable(
                "local mirror root is not a directory".into(),
            ));
        }
        Ok(Self {
            allowed_root: canonical,
        })
    }

    pub fn root(&self) -> &Path {
        &self.allowed_root
    }

    fn resolve_relative(&self, url: &str) -> Result<PathBuf, FetchError> {
        validate_pointer_url(url, UrlPolicy::AllowLocalForTesting)
            .map_err(|_| FetchError::InvalidUrl)?;
        let Some(raw_path) = url.strip_prefix("file://") else {
            return Err(FetchError::InvalidUrl);
        };
        let target = Path::new(raw_path);
        if !target.is_absolute()
            || target
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err(FetchError::InvalidUrl);
        }
        let rel = target
            .strip_prefix(&self.allowed_root)
            .map_err(|_| FetchError::InvalidUrl)?;
        if rel.as_os_str().is_empty() {
            return Err(FetchError::InvalidUrl);
        }
        Ok(rel.to_path_buf())
    }

    fn open_confined(&self, url: &str) -> Result<std::fs::File, FetchError> {
        let rel = self.resolve_relative(url)?;
        org_platform::fs::open_regular_beneath(&self.allowed_root, &rel).map_err(|e| {
            match e.kind() {
                io::ErrorKind::NotFound => {
                    FetchError::Transport("local mirror file not found".into())
                }
                _ => FetchError::Io(format!(
                    "refusing unsafe local mirror path ({:?})",
                    e.kind()
                )),
            }
        })
    }
}

impl ReleaseFetcher for LocalDirectoryFetcher {
    fn fetch_bytes(
        &self,
        url: &str,
        max_bytes: usize,
        _timeout: Duration,
    ) -> Result<Vec<u8>, FetchError> {
        let file = self.open_confined(url)?;
        let meta = file
            .metadata()
            .map_err(|e| FetchError::Io(format!("reading file metadata: {}", e.kind())))?;
        if !meta.is_file() {
            return Err(FetchError::Io(
                "local mirror entry is not a regular file".into(),
            ));
        }
        if meta.len() > max_bytes as u64 {
            return Err(FetchError::TooLarge {
                limit: max_bytes as u64,
            });
        }
        let mut buf = Vec::new();
        file.take(max_bytes as u64 + 1)
            .read_to_end(&mut buf)
            .map_err(|e| FetchError::Io(format!("reading local mirror file: {}", e.kind())))?;
        if buf.len() > max_bytes {
            return Err(FetchError::TooLarge {
                limit: max_bytes as u64,
            });
        }
        Ok(buf)
    }

    fn fetch_to_writer(
        &self,
        url: &str,
        max_bytes: u64,
        _timeout: Duration,
        sink: &mut dyn Write,
    ) -> Result<u64, FetchError> {
        let effective_limit = max_bytes.min(MAX_PAYLOAD_BYTES);
        let mut file = self.open_confined(url)?;
        let meta = file
            .metadata()
            .map_err(|e| FetchError::Io(format!("reading file metadata: {}", e.kind())))?;
        if !meta.is_file() {
            return Err(FetchError::Io(
                "local mirror entry is not a regular file".into(),
            ));
        }
        if meta.len() > effective_limit {
            return Err(FetchError::TooLarge {
                limit: effective_limit,
            });
        }
        let mut total = 0_u64;
        let mut buffer = [0_u8; READ_CHUNK_BYTES];
        loop {
            let n = file
                .read(&mut buffer)
                .map_err(|e| FetchError::Io(format!("reading local mirror file: {}", e.kind())))?;
            if n == 0 {
                return Ok(total);
            }
            let next = total.checked_add(n as u64);
            if next.is_none_or(|size| size > effective_limit) {
                return Err(FetchError::TooLarge {
                    limit: effective_limit,
                });
            }
            total = next.unwrap_or(effective_limit);
            sink.write_all(&buffer[..n])
                .map_err(|e| FetchError::Io(format!("writing staged bytes: {}", e.kind())))?;
        }
    }
}

#[cfg(test)]
#[path = "fetcher_tests.rs"]
mod tests;
