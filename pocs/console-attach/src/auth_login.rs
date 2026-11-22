use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::buffer::strip_ansi;
use crate::pty::{PtySession, PtySize};

/// Supported CLI providers for headless SSO-like browser login.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthProvider {
    Claude,
    /// Runs `claude setup-token` to mint a portable 1-year `CLAUDE_CODE_OAUTH_TOKEN` for remote offices.
    ClaudeToken,
    Codex,
}

/// Interactive flow required on the web UI side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthFlowKind {
    /// User opens URL, authorizes, and pastes the returned code back to our API (Claude).
    PasteCode,
    /// User opens URL, types the displayed one-time device code, and our backend polls completion (Codex).
    DeviceCode,
}

/// Lifecycle state of a single headless login attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthState {
    Starting,
    AwaitingCode,
    AwaitingDeviceAuth,
    Submitting,
    Authenticated,
    Failed,
    Cancelled,
}

/// Options for starting a headless login flow.
#[derive(Debug, Clone)]
pub struct StartLoginOptions {
    pub provider: AuthProvider,
    /// Optional binary override (useful for tests or non-PATH installs).
    pub binary_override: Option<String>,
    /// Optional isolated config directory. When `None`, uses the CLI's default user config dir.
    pub config_dir: Option<PathBuf>,
    /// How long `start_login` waits for the OAuth URL / device code to appear before returning.
    pub ready_timeout: Duration,
}

impl StartLoginOptions {
    pub fn for_provider(provider: AuthProvider) -> Self {
        Self {
            provider,
            binary_override: None,
            config_dir: None,
            ready_timeout: Duration::from_secs(12),
        }
    }
}

/// Serializable status snapshot returned to the UI / API caller.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthSessionSnapshot {
    pub login_id: String,
    pub provider: AuthProvider,
    pub flow: AuthFlowKind,
    pub state: AuthState,
    pub url: Option<String>,
    pub user_code: Option<String>,
    /// Populated when `provider == ClaudeToken` completes (`sk-ant-oat01-...`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_token: Option<String>,
    pub message: Option<String>,
    pub output_preview: String,
    pub config_dir: Option<String>,
}

struct LoginStateInner {
    state: AuthState,
    url: Option<String>,
    user_code: Option<String>,
    oauth_token: Option<String>,
    message: Option<String>,
    raw_output: Vec<u8>,
}

struct AuthLoginSession {
    login_id: String,
    provider: AuthProvider,
    flow: AuthFlowKind,
    config_dir: Option<PathBuf>,
    pid: u32,
    master_write: Mutex<File>,
    inner: Mutex<LoginStateInner>,
    changed: Condvar,
}

impl AuthLoginSession {
    fn build_snapshot(&self, g: &LoginStateInner) -> AuthSessionSnapshot {
        let mut cleaned = strip_ansi(&g.raw_output);
        if let Some(tok) = &g.oauth_token {
            cleaned = cleaned.replace(tok, "sk-ant-oat01-***REDACTED***");
        }
        AuthSessionSnapshot {
            login_id: self.login_id.clone(),
            provider: self.provider,
            flow: self.flow,
            state: g.state,
            url: g.url.clone(),
            user_code: g.user_code.clone(),
            oauth_token: g.oauth_token.clone(),
            message: g.message.clone(),
            output_preview: cleaned,
            config_dir: self
                .config_dir
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
        }
    }

    fn snapshot(&self) -> AuthSessionSnapshot {
        let g = self.inner.lock().expect("login state lock poisoned");
        self.build_snapshot(&g)
    }

    fn wait_until_ready(&self, timeout: Duration) -> AuthSessionSnapshot {
        let deadline = Instant::now() + timeout;
        let mut g = self.inner.lock().expect("login state lock poisoned");
        while g.state == AuthState::Starting {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            let (next_g, _) = self
                .changed
                .wait_timeout(g, deadline - now)
                .expect("login condvar poisoned");
            g = next_g;
        }
        self.build_snapshot(&g)
    }

    fn submit_code(&self, code: &str) -> io::Result<AuthSessionSnapshot> {
        if self.flow != AuthFlowKind::PasteCode {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "this provider uses device-code browser authorization and does not take a pasted code",
            ));
        }
        let clean = code.trim();
        if clean.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "authorization code must not be empty",
            ));
        }
        {
            let mut g = self.inner.lock().expect("login state lock poisoned");
            if g.state != AuthState::AwaitingCode && g.state != AuthState::Submitting {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("cannot submit code while login is in state {:?}", g.state),
                ));
            }
            g.state = AuthState::Submitting;
            g.message = Some("Submitted code to CLI; verifying with provider...".to_string());
        }

        let mut master = self
            .master_write
            .lock()
            .map_err(|_| io::Error::other("PTY write lock poisoned"))?;
        master.write_all(clean.as_bytes())?;
        master.write_all(b"\r")?;
        master.flush()?;

        Ok(self.snapshot())
    }

    fn cancel(&self) -> AuthSessionSnapshot {
        {
            let mut g = self.inner.lock().expect("login state lock poisoned");
            if matches!(
                g.state,
                AuthState::Starting
                    | AuthState::AwaitingCode
                    | AuthState::AwaitingDeviceAuth
                    | AuthState::Submitting
            ) {
                g.state = AuthState::Cancelled;
                g.message = Some("Login cancelled by user.".to_string());
            }
        }
        self.changed.notify_all();
        // SAFETY: sending SIGTERM to our own child PID is safe.
        unsafe {
            let _ = libc::kill(self.pid as libc::pid_t, libc::SIGTERM);
        }
        self.snapshot()
    }
}

/// Manages concurrent headless OAuth login sessions backed by Unix PTYs.
pub struct AuthManager {
    counter: AtomicU64,
    sessions: Mutex<HashMap<String, Arc<AuthLoginSession>>>,
}

impl Default for AuthManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AuthManager {
    pub fn new() -> Self {
        Self {
            counter: AtomicU64::new(1),
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Starts a headless login PTY process and waits up to `ready_timeout` for its URL/code.
    pub fn start_login(&self, opts: StartLoginOptions) -> io::Result<AuthSessionSnapshot> {
        let seq = self.counter.fetch_add(1, Ordering::Relaxed);
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let login_id = format!("login-{seq}-{}", stamp % 100_000);

        let (program, args, flow, mut extra_env) = match opts.provider {
            AuthProvider::Claude => (
                opts.binary_override
                    .clone()
                    .unwrap_or_else(|| "claude".to_string()),
                vec![
                    "auth".to_string(),
                    "login".to_string(),
                    "--claudeai".to_string(),
                ],
                AuthFlowKind::PasteCode,
                vec![
                    // Suppress opening a local browser and force the manual paste-code OAuth callback.
                    ("BROWSER".to_string(), "/usr/bin/true".to_string()),
                    ("NO_BROWSER".to_string(), "1".to_string()),
                    ("SSH_TTY".to_string(), "/dev/pts/0".to_string()),
                    (
                        "SSH_CONNECTION".to_string(),
                        "127.0.0.1 12345 127.0.0.1 22".to_string(),
                    ),
                ],
            ),
            AuthProvider::ClaudeToken => (
                opts.binary_override
                    .clone()
                    .unwrap_or_else(|| "claude".to_string()),
                vec!["setup-token".to_string()],
                AuthFlowKind::PasteCode,
                vec![
                    ("BROWSER".to_string(), "/usr/bin/true".to_string()),
                    ("NO_BROWSER".to_string(), "1".to_string()),
                    ("SSH_TTY".to_string(), "/dev/pts/0".to_string()),
                    (
                        "SSH_CONNECTION".to_string(),
                        "127.0.0.1 12345 127.0.0.1 22".to_string(),
                    ),
                ],
            ),
            AuthProvider::Codex => (
                opts.binary_override
                    .clone()
                    .unwrap_or_else(|| "codex".to_string()),
                vec!["login".to_string(), "--device-auth".to_string()],
                AuthFlowKind::DeviceCode,
                vec![
                    ("BROWSER".to_string(), "/usr/bin/true".to_string()),
                    ("NO_BROWSER".to_string(), "1".to_string()),
                ],
            ),
        };

        if let Some(cfg_dir) = &opts.config_dir {
            fs::create_dir_all(cfg_dir)?;
            match opts.provider {
                AuthProvider::Claude | AuthProvider::ClaudeToken => {
                    let sec_dir = cfg_dir.join("securestorage");
                    fs::create_dir_all(&sec_dir)?;
                    extra_env.push((
                        "CLAUDE_CONFIG_DIR".to_string(),
                        cfg_dir.to_string_lossy().into_owned(),
                    ));
                    extra_env.push((
                        "CLAUDE_SECURESTORAGE_CONFIG_DIR".to_string(),
                        sec_dir.to_string_lossy().into_owned(),
                    ));
                }
                AuthProvider::Codex => {
                    extra_env.push((
                        "CODEX_HOME".to_string(),
                        cfg_dir.to_string_lossy().into_owned(),
                    ));
                }
            }
        }

        // Wide 512-column PTY prevents long OAuth URLs from line-wrapping across rows.
        let mut pty = PtySession::spawn_with_env(
            &program,
            &args,
            PtySize {
                rows: 24,
                cols: 512,
            },
            Some("xterm-256color"),
            &extra_env,
        )?;
        let pid = pty.pid();
        let master_read = pty.clone_master()?;
        let master_write = pty.clone_master()?;

        let session = Arc::new(AuthLoginSession {
            login_id: login_id.clone(),
            provider: opts.provider,
            flow,
            config_dir: opts.config_dir,
            pid,
            master_write: Mutex::new(master_write),
            inner: Mutex::new(LoginStateInner {
                state: AuthState::Starting,
                url: None,
                user_code: None,
                oauth_token: None,
                message: Some("Starting CLI login flow...".to_string()),
                raw_output: Vec::new(),
            }),
            changed: Condvar::new(),
        });

        self.sessions
            .lock()
            .expect("sessions lock poisoned")
            .insert(login_id, Arc::clone(&session));

        // Thread 1: read PTY output and extract URL / device code as soon as they appear.
        let reader_session = Arc::clone(&session);
        thread::Builder::new()
            .name("auth-pty-reader".to_string())
            .spawn(move || auth_pty_reader_loop(master_read, reader_session))?;

        // Thread 2: wait for CLI process exit to detect authenticated vs failed.
        let waiter_session = Arc::clone(&session);
        thread::Builder::new()
            .name("auth-pty-waiter".to_string())
            .spawn(move || {
                let res = pty.wait();
                let mut g = waiter_session.inner.lock().expect("login lock poisoned");
                if g.state != AuthState::Cancelled {
                    match res {
                        Ok(status) if status.success() => {
                            g.state = AuthState::Authenticated;
                            g.message = Some("Signed in successfully.".to_string());
                        }
                        Ok(status) => {
                            g.state = AuthState::Failed;
                            g.message = Some(format!("CLI login exited with {status}"));
                        }
                        Err(err) => {
                            g.state = AuthState::Failed;
                            g.message = Some(format!("Failed waiting for CLI login: {err}"));
                        }
                    }
                }
                waiter_session.changed.notify_all();
            })?;

        Ok(session.wait_until_ready(opts.ready_timeout))
    }

    pub fn status(&self, login_id: &str) -> io::Result<AuthSessionSnapshot> {
        let session = self.get(login_id)?;
        Ok(session.snapshot())
    }

    pub fn submit_code(&self, login_id: &str, code: &str) -> io::Result<AuthSessionSnapshot> {
        let session = self.get(login_id)?;
        session.submit_code(code)
    }

    pub fn cancel(&self, login_id: &str) -> io::Result<AuthSessionSnapshot> {
        let session = self.get(login_id)?;
        Ok(session.cancel())
    }

    fn get(&self, login_id: &str) -> io::Result<Arc<AuthLoginSession>> {
        self.sessions
            .lock()
            .expect("sessions lock poisoned")
            .get(login_id)
            .cloned()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("unknown login_id: '{login_id}'"),
                )
            })
    }
}

fn auth_pty_reader_loop(mut master_read: File, session: Arc<AuthLoginSession>) {
    let mut buf = [0u8; 4096];
    loop {
        match master_read.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let mut g = session.inner.lock().expect("login lock poisoned");
                g.raw_output.extend_from_slice(&buf[..n]);
                if g.raw_output.len() > 64 * 1024 {
                    let drop_n = g.raw_output.len() - 64 * 1024;
                    g.raw_output.drain(..drop_n);
                }
                let cleaned = strip_ansi(&g.raw_output);
                let mut became_ready = false;

                match session.provider {
                    AuthProvider::Claude | AuthProvider::ClaudeToken => {
                        if let Some(url) = extract_claude_oauth_url(&cleaned) {
                            let is_longer =
                                g.url.as_ref().map(|u| url.len() > u.len()).unwrap_or(true);
                            if is_longer {
                                g.url = Some(url);
                            }
                        }
                        if session.provider == AuthProvider::ClaudeToken {
                            if let Some(tok) = extract_claude_oauth_token(&cleaned) {
                                g.oauth_token = Some(tok);
                            }
                        }
                        let prompt_ready = cleaned.to_ascii_lowercase().contains("paste");
                        if g.state == AuthState::Starting
                            && g.url.is_some()
                            && (session.provider == AuthProvider::Claude || prompt_ready)
                        {
                            g.state = AuthState::AwaitingCode;
                            g.message = Some(if session.provider == AuthProvider::ClaudeToken {
                                "Open the Claude URL, sign in, and paste the returned code to mint a 1-year CLAUDE_CODE_OAUTH_TOKEN."
                                    .to_string()
                            } else {
                                "Open the Claude URL, sign in, and paste the returned code."
                                    .to_string()
                            });
                            became_ready = true;
                        }
                    }
                    AuthProvider::Codex => {
                        if g.url.is_none() {
                            if let Some(url) = extract_first_https_url(&cleaned) {
                                g.url = Some(url);
                            }
                        }
                        if g.user_code.is_none() {
                            if let Some(code) = extract_device_code(&cleaned) {
                                g.user_code = Some(code);
                            }
                        }
                        if g.state == AuthState::Starting
                            && g.url.is_some()
                            && g.user_code.is_some()
                        {
                            g.state = AuthState::AwaitingDeviceAuth;
                            g.message = Some(
                                "Open the OpenAI device link and enter the one-time code."
                                    .to_string(),
                            );
                            became_ready = true;
                        }
                    }
                }
                drop(g);
                if became_ready {
                    session.changed.notify_all();
                }
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) if err.raw_os_error() == Some(libc::EIO) => break,
            Err(_) => break,
        }
    }
}

/// Extracts Claude's OAuth authorize URL from ANSI-stripped PTY output, reassembling
/// hard-wrapped lines (such as Ink's 80-column wrap in `claude setup-token`).
pub fn extract_claude_oauth_url(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    for (idx, line) in lines.iter().enumerate() {
        if let Some(pos) = line.find("https://") {
            let mut candidate = line[pos..].trim().to_string();
            // If the URL has no spaces and wraps across consecutive continuation lines
            // (no spaces, valid URL characters), stitch them back together.
            if !candidate.contains(' ') {
                for next in &lines[idx + 1..] {
                    let trimmed = next.trim();
                    if trimmed.is_empty()
                        || trimmed.contains(' ')
                        || !trimmed.bytes().all(is_url_byte)
                    {
                        break;
                    }
                    // Stop if the next line looks like a prose prompt (e.g. "Pastecodehereifprompted>").
                    if trimmed.to_ascii_lowercase().starts_with("paste") {
                        break;
                    }
                    candidate.push_str(trimmed);
                }
            } else if let Some(first_word) = candidate.split_whitespace().next() {
                candidate = first_word.to_string();
            }
            if let Some(url) = trim_url_token(&candidate) {
                if url.contains("oauth/authorize") {
                    return Some(url);
                }
            }
        }
    }
    extract_first_https_url(text)
}

fn is_url_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric()
        || matches!(
            b,
            b'-' | b'_'
                | b'.'
                | b'~'
                | b':'
                | b'/'
                | b'?'
                | b'#'
                | b'['
                | b']'
                | b'@'
                | b'!'
                | b'$'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
                | b'%'
        )
}

/// Extracts a generated Claude 1-year OAuth token (`sk-ant-oat01-...`) from PTY output,
/// also handling line wrapping if the terminal wrapped the token across lines.
pub fn extract_claude_oauth_token(text: &str) -> Option<String> {
    const PREFIX: &str = "sk-ant-oat01-";
    let lines: Vec<&str> = text.lines().collect();
    for (idx, line) in lines.iter().enumerate() {
        if let Some(pos) = line.find(PREFIX) {
            let mut candidate = line[pos..]
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_matches(['"', '\'', '`', ',', '.'])
                .to_string();
            // Reassemble wrapped token continuation lines if no spaces appear on the next line.
            if line[pos..].trim() == candidate {
                for next in &lines[idx + 1..] {
                    let trimmed = next.trim();
                    if trimmed.is_empty()
                        || trimmed.contains(' ')
                        || !trimmed
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                    {
                        break;
                    }
                    candidate.push_str(trimmed);
                }
            }
            if candidate.len() > PREFIX.len() + 10 {
                return Some(candidate);
            }
        }
    }
    None
}

/// Extracts the first `https://` URL token from plain text.
pub fn extract_first_https_url(text: &str) -> Option<String> {
    for token in text.split_whitespace() {
        if let Some(url) = trim_url_token(token) {
            return Some(url);
        }
    }
    None
}

fn trim_url_token(token: &str) -> Option<String> {
    let idx = token.find("https://")?;
    let slice = &token[idx..];
    let trimmed = slice.trim_end_matches([')', ']', '.', ',', ';', '"', '\'']);
    if trimmed.len() > "https://".len() {
        Some(trimmed.to_string())
    } else {
        None
    }
}

/// Extracts an OAuth device code such as `QGM9-KSR16` from Codex device-auth output.
pub fn extract_device_code(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.contains("://") {
            continue;
        }
        for token in trimmed.split_whitespace() {
            if is_device_code(token) {
                return Some(token.to_string());
            }
        }
    }
    None
}

fn is_device_code(token: &str) -> bool {
    let Some((left, right)) = token.split_once('-') else {
        return false;
    };
    if right.contains('-') {
        return false;
    }
    if !(3..=6).contains(&left.len()) || !(3..=6).contains(&right.len()) {
        return false;
    }
    let all_upper_alnum = |s: &str| {
        s.bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
    };
    let has_digit = token.bytes().any(|b| b.is_ascii_digit());
    let has_upper = token.bytes().any(|b| b.is_ascii_uppercase());
    all_upper_alnum(left) && all_upper_alnum(right) && (has_digit || has_upper)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_claude_oauth_prompt() {
        let sample = "Opening browser to sign in…\nIf the browser didn't open, visit: https://claude.com/cai/oauth/authorize?code=true&client_id=abc&state=xyz\nPaste code here if prompted > ";
        assert_eq!(
            extract_claude_oauth_url(sample).as_deref(),
            Some("https://claude.com/cai/oauth/authorize?code=true&client_id=abc&state=xyz")
        );
    }

    #[test]
    fn parses_real_codex_device_auth_prompt() {
        let sample = "\nWelcome to Codex [v0.169.0]\n1. Open this link in your browser\n   https://auth.openai.com/codex/device\n\n2. Enter this one-time code (expires in 15 minutes)\n   QGM9-KSR16\n";
        assert_eq!(
            extract_first_https_url(sample).as_deref(),
            Some("https://auth.openai.com/codex/device")
        );
        assert_eq!(extract_device_code(sample).as_deref(), Some("QGM9-KSR16"));
    }
}
