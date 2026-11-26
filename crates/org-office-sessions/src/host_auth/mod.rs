//! Host-local CLI runtime auth helpers for `subscription_cli` bindings (`TASK-120`).
//!
//! Claude Code sessions keep `LaunchContext::config_dir = None` so host `~/.claude/plugins` are
//! never loaded (preserving S1 isolation) and macOS Keychain hashing is avoided; instead, when no
//! vault credential is enrolled for a `subscription_cli` connection, the host OAuth token is
//! resolved cleanly and passed via `CLAUDE_CODE_OAUTH_TOKEN` (scrubbed from tool subprocesses by
//! `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB=1` and redacted from transcripts).

use std::ffi::OsString;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use org_agent_runtime::{LaunchPlan, SpecPath};
use org_office::organization_family::records::ProviderAuthMode;
use zeroize::Zeroizing;

mod keychain;
#[cfg(test)]
mod tests;
mod token;

use keychain::{invalidate_keychain_cache, read_claude_keychain_token, CLAUDE_KEYCHAIN_CACHE};
use token::{parse_claude_oauth_token, ClaudeOAuthToken, MAX_HOST_CLAUDE_CRED_BYTES};

/// Process-wide single-flight lock serializing host Claude OAuth refresh probes so concurrent
/// expiring sessions never race on single-use rotating refresh tokens.
static CLAUDE_OAUTH_REFRESH_LOCK: Mutex<()> = Mutex::new(());

/// Safe host environment variables forwarded to the bounded host-side Claude OAuth refresh probe.
/// Never includes `ANTHROPIC_API_KEY`, `CLAUDE_CODE_OAUTH_TOKEN`, `ORG_*`, or `AGENT_OS_*`.
const REFRESH_SAFE_HOST_ENV: &[&str] = &[
    "HOME",
    "PATH",
    "USER",
    "LOGNAME",
    "TMPDIR",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
    "XDG_RUNTIME_DIR",
    "LANG",
    "LC_ALL",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
];

/// Maximum byte length for an agent's `TMPDIR` so Linux `bwrap`/`socat` bridge sockets
/// (`$TMPDIR/<name>.sock`, ~35 bytes) stay well under the 108-byte `sun_path` limit.
const MAX_SHORT_TMPDIR_BYTES: usize = 70;
/// Proactive refresh lead time before a host Claude OAuth access token expires (5 minutes).
pub(crate) const OAUTH_REFRESH_LEAD_MS: u64 = 5 * 60 * 1000;
/// Upper bound on a host-side `claude` OAuth token refresh probe before session launch.
const OAUTH_REFRESH_TIMEOUT: Duration = Duration::from_secs(15);

/// Returns `true` when an OAuth token with `expires_at_ms` is already expired or within
/// [`OAUTH_REFRESH_LEAD_MS`] of expiry.
pub(crate) fn is_oauth_expiring(expires_at_ms: Option<u64>, now_ms: u64) -> bool {
    expires_at_ms.is_some_and(|exp| now_ms.saturating_add(OAUTH_REFRESH_LEAD_MS) >= exp)
}

/// The runtime folder that holds the office's privileged entries (`host.sock`, the launch
/// rendezvous `r/`, per-agent temporary folders `t/`) beside the office sockets folder: `<run>`
/// for an office socket at `<run>/o/<handle>.sock` (`org_proto::location`), else the socket's own
/// folder (a socket configured outside the `o/` layout, as tests and older callers do). Every
/// derivation of those entries from the office socket goes through here.
pub(crate) fn office_runtime_dir(office_socket: &Path) -> Option<&Path> {
    org_proto::location::runtime_dir_of_office_socket(office_socket)
        .or_else(|| office_socket.parent())
}

/// Deterministic 8-hex per-agent slug used under `<runtime>/t/<slug>` ([`office_runtime_dir`]).
pub(crate) fn agent_tmpdir_slug(office_id: &str, employee_id: &str) -> String {
    let digest = ring::digest::digest(
        &ring::digest::SHA256,
        format!("{office_id}:{employee_id}").as_bytes(),
    );
    let mut slug = String::with_capacity(8);
    for &b in &digest.as_ref()[..4] {
        use std::fmt::Write as _;
        let _ = write!(slug, "{b:02x}");
    }
    slug
}

/// Chooses an owner-only per-agent `TMPDIR` inside the office's short runtime socket directory
/// (`<runtime>/t/<8-hex>`, [`office_runtime_dir`]; never inside the office sockets folder `o/`)
/// whenever that stays within [`MAX_SHORT_TMPDIR_BYTES`] and shorter than `durable_fallback`,
/// avoiding Linux `ENAMETOOLONG` (`sun_path` > 108 bytes) in Claude's `bubblewrap`/`socat`
/// sandbox while keeping `TMPDIR` out of shared `/tmp`.
pub(crate) fn resolve_agent_tmpdir(
    office_socket: &Path,
    durable_fallback: &Path,
    office_id: &str,
    employee_id: &str,
) -> PathBuf {
    let Some(runtime) = office_runtime_dir(office_socket) else {
        return durable_fallback.to_path_buf();
    };
    let candidate = runtime
        .join("t")
        .join(agent_tmpdir_slug(office_id, employee_id));
    let cand_len = candidate.as_os_str().len();
    let fallback_len = durable_fallback.as_os_str().len();
    if cand_len <= MAX_SHORT_TMPDIR_BYTES && cand_len < fallback_len {
        candidate
    } else {
        durable_fallback.to_path_buf()
    }
}

/// Returns whether an admitted route may fall back to host-local CLI subscription auth.
pub(crate) fn allows_host_subscription_auth(
    auth_env: &[(OsString, OsString)],
    auth_mode: Option<ProviderAuthMode>,
) -> bool {
    auth_env.is_empty() && matches!(auth_mode, None | Some(ProviderAuthMode::SubscriptionCli))
}

/// Locates the host user's runtime config dir for Codex (`~/.codex`) or Agy (`~/.gemini`).
/// Returns `None` for Claude so host user plugins are never loaded into isolated sessions.
pub(crate) fn host_config_dir_for(runtime_id: &str, allowed: bool) -> Option<SpecPath> {
    if !allowed {
        return None;
    }
    let subdir = match runtime_id {
        "codex" => ".codex",
        "agy" => ".gemini",
        _ => return None,
    };
    let candidate = std::env::var_os("HOME").map(PathBuf::from)?.join(subdir);
    if !candidate.is_dir() {
        return None;
    }
    let resolved = candidate.canonicalize().unwrap_or(candidate);
    let text = resolved.display().to_string();
    SpecPath::try_from(text.as_str()).ok()
}

/// Non-secret metadata for a host Claude OAuth token injected into a session launch.
pub(crate) struct HostOAuthMeta {
    pub expires_at_ms: Option<u64>,
    pub token_fp: String,
}

/// Non-reversible 16-hex SHA-256 fingerprint of an OAuth access token (never plaintext).
pub(crate) fn oauth_token_fingerprint(token: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = ring::digest::digest(&ring::digest::SHA256, token.as_bytes());
    let mut out = String::with_capacity(16);
    for &b in &digest.as_ref()[..8] {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// Appends `CLAUDE_CODE_OAUTH_TOKEN` to `launch_env` when a Claude `subscription_cli` route has
/// no vault credential and the runtime plan both accepts and protects `CLAUDE_CODE_OAUTH_TOKEN`.
/// Proactively refreshes an expiring/expired host OAuth token via `runtime_binary` before launch
/// and returns its non-secret expiry and token fingerprint metadata.
pub(crate) fn append_host_claude_oauth(
    runtime_id: &str,
    allowed: bool,
    runtime_binary: &Path,
    plan: &LaunchPlan,
    launch_env: &mut Vec<(OsString, OsString)>,
) -> Option<HostOAuthMeta> {
    if !allowed || runtime_id != "claude" || !plan_protects_claude_oauth(plan) {
        return None;
    }
    let _keychain_scope = CLAUDE_KEYCHAIN_CACHE.scope();
    let mut resolved = resolve_host_claude_oauth_entry()?;
    if is_oauth_expiring(resolved.expires_at_ms, crate::credentials::now_ms()) {
        refresh_host_claude_oauth(runtime_binary);
        if let Some(refreshed) = resolve_host_claude_oauth_entry() {
            resolved = refreshed;
        }
    }
    let token_fp = oauth_token_fingerprint(resolved.access_token.as_str());
    launch_env.push((
        OsString::from("CLAUDE_CODE_OAUTH_TOKEN"),
        OsString::from(resolved.access_token.as_str()),
    ));
    Some(HostOAuthMeta {
        expires_at_ms: resolved.expires_at_ms,
        token_fp,
    })
}

/// Triggers a bounded, single-flight, env-scrubbed host-side Claude CLI refresh so expired
/// `subscription_cli` OAuth access tokens rotate cleanly before spawning an isolated session.
/// Returns `true` only when the post-refresh host token is verifiably non-expiring or rotated.
pub(crate) fn refresh_host_claude_oauth(runtime_binary: &Path) -> bool {
    refresh_host_claude_oauth_with(runtime_binary, false, None, resolve_host_claude_oauth_entry)
}

/// Reactive variant used after a 401 auth-expired failure: if another session or host CLI has
/// already rotated the host credentials away from `failed_token_fp` to a fresh token, returns
/// `true` immediately without probing or poisoning the shared connection. Otherwise runs one
/// bounded probe and succeeds when the token rotates (or becomes non-expiring if previously stale).
pub(crate) fn refresh_host_claude_oauth_reactive(
    runtime_binary: &Path,
    failed_token_fp: Option<&str>,
) -> bool {
    refresh_host_claude_oauth_with(
        runtime_binary,
        true,
        failed_token_fp,
        resolve_host_claude_oauth_entry,
    )
}

fn refresh_host_claude_oauth_with(
    runtime_binary: &Path,
    force_probe: bool,
    failed_token_fp: Option<&str>,
    read_entry: impl Fn() -> Option<ClaudeOAuthToken>,
) -> bool {
    if !runtime_binary.is_file() {
        return false;
    }
    let _single_flight = CLAUDE_OAUTH_REFRESH_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let before = read_entry();
    let now = crate::credentials::now_ms();
    let before_fresh = before.as_ref().is_some_and(|tok| {
        tok.expires_at_ms.is_some() && !is_oauth_expiring(tok.expires_at_ms, now)
    });
    if before_fresh && (!force_probe || already_rotated_from(before.as_ref(), failed_token_fp)) {
        return true;
    }
    let Some(scratch) = PrivateRefreshScratch::create() else {
        return false;
    };
    let probed = run_claude_refresh_probe(runtime_binary, &scratch);
    // The probe may have rotated the Keychain token: never answer from the pre-probe cache.
    invalidate_keychain_cache();
    if !probed {
        return false;
    }
    let Some(after) = read_entry() else {
        return false;
    };
    oauth_token_rotated_or_fresh(
        before.as_ref(),
        failed_token_fp,
        &after,
        crate::credentials::now_ms(),
        force_probe && before_fresh,
    )
}

fn already_rotated_from(before: Option<&ClaudeOAuthToken>, failed_token_fp: Option<&str>) -> bool {
    match (before, failed_token_fp) {
        (Some(tok), Some(fp)) => oauth_token_fingerprint(tok.access_token.as_str()) != fp,
        _ => false,
    }
}

fn run_claude_refresh_probe(runtime_binary: &Path, scratch: &PrivateRefreshScratch) -> bool {
    let mut cmd = Command::new(runtime_binary);
    cmd.env_clear()
        .current_dir(&scratch.dir)
        .args([
            "-p",
            "ok",
            "--model",
            "haiku",
            "--no-session-persistence",
            "--output-format",
            "text",
            "--setting-sources",
            "",
            "--tools",
            "",
            "--disable-slash-commands",
            "--strict-mcp-config",
            "--mcp-config",
        ])
        .arg(&scratch.empty_mcp)
        .env("DISABLE_AUTOUPDATER", "1")
        .env("CLAUDE_CODE_SUBPROCESS_ENV_SCRUB", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for var in REFRESH_SAFE_HOST_ENV {
        if let Some(val) = std::env::var_os(var) {
            cmd.env(var, val);
        }
    }
    let Ok(mut child) = cmd.spawn() else {
        return false;
    };
    let deadline = Instant::now() + OAUTH_REFRESH_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

fn oauth_token_rotated_or_fresh(
    before: Option<&ClaudeOAuthToken>,
    failed_token_fp: Option<&str>,
    after: &ClaudeOAuthToken,
    now_ms: u64,
    require_rotation: bool,
) -> bool {
    let rotated = match failed_token_fp {
        Some(fp) => oauth_token_fingerprint(after.access_token.as_str()) != fp,
        None => before.is_some_and(|b| b.access_token.as_str() != after.access_token.as_str()),
    };
    if require_rotation {
        return rotated;
    }
    let not_expiring =
        after.expires_at_ms.is_some() && !is_oauth_expiring(after.expires_at_ms, now_ms);
    not_expiring || rotated
}

struct PrivateRefreshScratch {
    dir: PathBuf,
    empty_mcp: PathBuf,
}

impl PrivateRefreshScratch {
    fn create() -> Option<Self> {
        let base = std::env::temp_dir();
        for _ in 0..8 {
            let suffix = crate::credentials::random_hex(8).ok()?;
            let dir = base.join(format!("org-oauth-refresh-{suffix}"));
            if org_platform::fs::create_owner_only_dir(&dir).is_ok() {
                let empty_mcp = dir.join("empty-mcp.json");
                if org_platform::fs::write_owner_only_file(&empty_mcp, b"{\"mcpServers\":{}}\n")
                    .is_ok()
                {
                    return Some(Self { dir, empty_mcp });
                }
                let _ = std::fs::remove_dir_all(&dir);
                return None;
            }
        }
        None
    }
}

impl Drop for PrivateRefreshScratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn plan_protects_claude_oauth(plan: &LaunchPlan) -> bool {
    let accepts = plan
        .auth_env_names
        .iter()
        .any(|n| n.as_str() == "CLAUDE_CODE_OAUTH_TOKEN");
    let protects = plan
        .secret_env_names
        .iter()
        .any(|n| n.as_str() == "CLAUDE_CODE_OAUTH_TOKEN");
    accepts && protects
}

/// Resolves the host user's local Claude Code OAuth access token (for transcript redaction).
pub(crate) fn resolve_host_claude_oauth_token() -> Option<Zeroizing<String>> {
    resolve_host_claude_oauth_entry().map(|e| e.access_token)
}

/// Resolves the host user's local Claude Code OAuth access token + expiry from
/// `$HOME/.claude/.credentials.json` and/or macOS Keychain (`Claude Code-credentials`),
/// preferring whichever source holds the fresher `expiresAt` timestamp.
pub(crate) fn resolve_host_claude_oauth_entry() -> Option<ClaudeOAuthToken> {
    let file_tok = read_claude_credentials_file();
    let keychain_tok = read_claude_keychain_token();
    pick_fresher_oauth_entry(file_tok, keychain_tok)
}

fn pick_fresher_oauth_entry(
    file_tok: Option<ClaudeOAuthToken>,
    keychain_tok: Option<ClaudeOAuthToken>,
) -> Option<ClaudeOAuthToken> {
    match (file_tok, keychain_tok) {
        (Some(f), Some(k)) => match (f.expires_at_ms, k.expires_at_ms) {
            (Some(fe), Some(ke)) if ke > fe => Some(k),
            (None, Some(_)) => Some(k),
            _ => Some(f),
        },
        (Some(f), None) => Some(f),
        (None, Some(k)) => Some(k),
        (None, None) => None,
    }
}

fn read_claude_credentials_file() -> Option<ClaudeOAuthToken> {
    let cred_path = std::env::var_os("HOME")
        .map(PathBuf::from)?
        .join(".claude")
        .join(".credentials.json");
    read_claude_credentials_from_path(&cred_path)
}

fn read_claude_credentials_from_path(cred_path: &Path) -> Option<ClaudeOAuthToken> {
    let file = org_platform::fs::open_nofollow_ro(cred_path).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() > MAX_HOST_CLAUDE_CRED_BYTES as u64 {
        return None;
    }
    let mut bytes = Zeroizing::new(Vec::new());
    file.take(MAX_HOST_CLAUDE_CRED_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > MAX_HOST_CLAUDE_CRED_BYTES {
        return None;
    }
    parse_claude_oauth_token(&bytes)
}
