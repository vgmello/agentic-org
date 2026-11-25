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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use org_agent_runtime::{LaunchPlan, SpecPath};
use org_office::organization_family::records::ProviderAuthMode;
use zeroize::Zeroizing;

/// Process-wide single-flight lock serializing host Claude OAuth refresh probes so concurrent
/// expiring sessions never race on single-use rotating refresh tokens.
static CLAUDE_OAUTH_REFRESH_LOCK: Mutex<()> = Mutex::new(());
static REFRESH_TEMP_SEQ: AtomicU64 = AtomicU64::new(1);

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

const MAX_HOST_CLAUDE_CRED_BYTES: usize = 64 * 1024;
/// Maximum byte length for an agent's `TMPDIR` so Linux `bwrap`/`socat` bridge sockets
/// (`$TMPDIR/<name>.sock`, ~35 bytes) stay well under the 108-byte `sun_path` limit.
const MAX_SHORT_TMPDIR_BYTES: usize = 70;
/// Upper bound on macOS Keychain lookup so a locked Keychain never hangs session launch or
/// transcript redaction.
const KEYCHAIN_LOOKUP_TIMEOUT: Duration = Duration::from_secs(3);
/// Proactive refresh lead time before a host Claude OAuth access token expires (5 minutes).
pub(crate) const OAUTH_REFRESH_LEAD_MS: u64 = 5 * 60 * 1000;
/// Upper bound on a host-side `claude` OAuth token refresh probe before session launch.
const OAUTH_REFRESH_TIMEOUT: Duration = Duration::from_secs(15);

/// Resolved host Claude Code OAuth access token and its optional expiry timestamp (epoch ms).
pub(crate) struct ClaudeOAuthToken {
    pub access_token: Zeroizing<String>,
    pub expires_at_ms: Option<u64>,
}

/// Returns `true` when an OAuth token with `expires_at_ms` is already expired or within
/// [`OAUTH_REFRESH_LEAD_MS`] of expiry.
pub(crate) fn is_oauth_expiring(expires_at_ms: Option<u64>, now_ms: u64) -> bool {
    expires_at_ms.is_some_and(|exp| now_ms.saturating_add(OAUTH_REFRESH_LEAD_MS) >= exp)
}

/// Deterministic 8-hex per-agent slug used under `<socket_parent>/t/<slug>`.
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
/// (`<socket_parent>/t/<8-hex>`) whenever that stays within [`MAX_SHORT_TMPDIR_BYTES`] and shorter
/// than `durable_fallback`, avoiding Linux `ENAMETOOLONG` (`sun_path` > 108 bytes) in Claude's
/// `bubblewrap`/`socat` sandbox while keeping `TMPDIR` out of shared `/tmp`.
pub(crate) fn resolve_agent_tmpdir(
    office_socket: &Path,
    durable_fallback: &Path,
    office_id: &str,
    employee_id: &str,
) -> PathBuf {
    let Some(sock_parent) = office_socket.parent() else {
        return durable_fallback.to_path_buf();
    };
    let candidate = sock_parent
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

/// Appends `CLAUDE_CODE_OAUTH_TOKEN` to `launch_env` when a Claude `subscription_cli` route has
/// no vault credential and the runtime plan both accepts and protects `CLAUDE_CODE_OAUTH_TOKEN`.
/// Proactively refreshes an expiring/expired host OAuth token via `runtime_binary` before launch
/// and returns the token's `expiresAt` timestamp (epoch ms) when known.
pub(crate) fn append_host_claude_oauth(
    runtime_id: &str,
    allowed: bool,
    runtime_binary: &Path,
    plan: &LaunchPlan,
    launch_env: &mut Vec<(OsString, OsString)>,
) -> Option<u64> {
    if !allowed || runtime_id != "claude" || !plan_protects_claude_oauth(plan) {
        return None;
    }
    let mut resolved = resolve_host_claude_oauth_entry()?;
    if is_oauth_expiring(resolved.expires_at_ms, crate::credentials::now_ms()) {
        refresh_host_claude_oauth(runtime_binary);
        if let Some(refreshed) = resolve_host_claude_oauth_entry() {
            resolved = refreshed;
        }
    }
    launch_env.push((
        OsString::from("CLAUDE_CODE_OAUTH_TOKEN"),
        OsString::from(resolved.access_token.as_str()),
    ));
    resolved.expires_at_ms
}

/// Triggers a bounded, single-flight, env-scrubbed host-side Claude CLI refresh so expired
/// `subscription_cli` OAuth access tokens rotate cleanly before spawning an isolated session.
/// Returns `true` only when the post-refresh host token is verifiably non-expiring or rotated.
pub(crate) fn refresh_host_claude_oauth(runtime_binary: &Path) -> bool {
    refresh_host_claude_oauth_with(runtime_binary, false, resolve_host_claude_oauth_entry)
}

/// Reactive variant used after a 401 auth-expired failure: still runs one bounded probe even when
/// the stored token's local `expiresAt` has not elapsed (early server-side revocation), and
/// succeeds only when the token actually rotates.
pub(crate) fn refresh_host_claude_oauth_reactive(runtime_binary: &Path) -> bool {
    refresh_host_claude_oauth_with(runtime_binary, true, resolve_host_claude_oauth_entry)
}

fn refresh_host_claude_oauth_with(
    runtime_binary: &Path,
    force_probe: bool,
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
    if !force_probe && before_fresh {
        return true;
    }
    let Some(scratch) = PrivateRefreshScratch::create() else {
        return false;
    };
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
    let exited_ok = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break false;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    if !exited_ok {
        return false;
    }
    let Some(after) = read_entry() else {
        return false;
    };
    oauth_token_rotated_or_fresh(
        before.as_ref(),
        &after,
        crate::credentials::now_ms(),
        force_probe && before_fresh,
    )
}

fn oauth_token_rotated_or_fresh(
    before: Option<&ClaudeOAuthToken>,
    after: &ClaudeOAuthToken,
    now_ms: u64,
    require_rotation: bool,
) -> bool {
    let rotated = before.is_some_and(|b| b.access_token.as_str() != after.access_token.as_str());
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
        use std::os::unix::fs::DirBuilderExt as _;
        let base = std::env::temp_dir();
        let pid = std::process::id();
        for _ in 0..8 {
            let seq = REFRESH_TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
            let dir = base.join(format!("org-oauth-refresh-{pid}-{seq}"));
            let mut builder = std::fs::DirBuilder::new();
            builder.mode(0o700);
            if builder.create(&dir).is_ok() {
                let empty_mcp = dir.join("empty-mcp.json");
                if std::fs::write(&empty_mcp, b"{\"mcpServers\":{}}\n").is_ok() {
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
    use std::os::unix::fs::OpenOptionsExt as _;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(
            org_platform::fs::O_NOFOLLOW
                | org_platform::fs::O_NONBLOCK
                | org_platform::fs::O_CLOEXEC,
        )
        .open(cred_path)
        .ok()?;
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

fn read_claude_keychain_token() -> Option<ClaudeOAuthToken> {
    let security_bin = Path::new("/usr/bin/security");
    if !security_bin.is_file() {
        return None;
    }
    let mut child = Command::new(security_bin)
        .args([
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-w",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + KEYCHAIN_LOOKUP_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let mut stdout = child.stdout.take()?;
                let mut buf = Zeroizing::new(Vec::new());
                stdout
                    .by_ref()
                    .take(MAX_HOST_CLAUDE_CRED_BYTES as u64 + 1)
                    .read_to_end(&mut buf)
                    .ok()?;
                if buf.len() > MAX_HOST_CLAUDE_CRED_BYTES {
                    return None;
                }
                return parse_claude_oauth_token(&buf);
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

fn parse_claude_oauth_token(raw: &[u8]) -> Option<ClaudeOAuthToken> {
    let val: serde_json::Value = serde_json::from_slice(raw).ok()?;
    let oauth = val.get("claudeAiOauth")?;
    let token = oauth.get("accessToken")?.as_str()?.trim();
    if token.is_empty() {
        return None;
    }
    let expires_at_ms = oauth.get("expiresAt").and_then(serde_json::Value::as_u64);
    Some(ClaudeOAuthToken {
        access_token: Zeroizing::new(token.to_owned()),
        expires_at_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_claude_oauth_access_token_json() {
        let valid = br#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-example","expiresAt":1795620000000}}"#;
        let parsed = parse_claude_oauth_token(valid).unwrap();
        assert_eq!(parsed.access_token.as_str(), "sk-ant-oat01-example");
        assert_eq!(parsed.expires_at_ms, Some(1_795_620_000_000));
        assert!(is_oauth_expiring(
            parsed.expires_at_ms,
            1_795_620_000_000 - OAUTH_REFRESH_LEAD_MS
        ));
        assert!(!is_oauth_expiring(
            parsed.expires_at_ms,
            1_795_620_000_000 - OAUTH_REFRESH_LEAD_MS - 1
        ));
        assert!(!is_oauth_expiring(None, 1_795_620_000_000));
        assert!(parse_claude_oauth_token(br#"{"claudeAiOauth":{"accessToken":"  "}}"#).is_none());
        assert!(parse_claude_oauth_token(br#"not-json"#).is_none());
    }

    #[test]
    fn resolve_agent_tmpdir_prefers_short_socket_parent_for_linux_sun_path() {
        let sock = Path::new("/home/agents/.cache/agentic-org/tmp/ao/2oe/f8fc7ab6ed98.sock");
        let deep = Path::new("/home/agents/.local/share/agentic-org/instances/default/org/offices/off/durable/local/offices/off/agents/emp/tmp");
        let picked = resolve_agent_tmpdir(sock, deep, "off-1", "emp-1");
        assert!(picked.starts_with("/home/agents/.cache/agentic-org/tmp/ao/2oe/t/"));
        assert!(picked.as_os_str().len() <= MAX_SHORT_TMPDIR_BYTES);
        assert_ne!(picked, resolve_agent_tmpdir(sock, deep, "off-1", "emp-2"));
    }

    #[test]
    fn allows_host_subscription_auth_enforces_mode_and_empty_vault() {
        assert!(allows_host_subscription_auth(&[], None));
        assert!(allows_host_subscription_auth(
            &[],
            Some(ProviderAuthMode::SubscriptionCli)
        ));
        assert!(!allows_host_subscription_auth(
            &[],
            Some(ProviderAuthMode::ApiKey)
        ));
        assert!(!allows_host_subscription_auth(
            &[],
            Some(ProviderAuthMode::CloudIam)
        ));
        let non_empty = vec![(OsString::from("ANTHROPIC_API_KEY"), OsString::from("sk-1"))];
        assert!(!allows_host_subscription_auth(
            &non_empty,
            Some(ProviderAuthMode::SubscriptionCli)
        ));
    }

    #[test]
    fn host_config_dir_for_claude_is_always_none_and_cred_read_is_bounded() {
        assert!(host_config_dir_for("claude", true).is_none());
        assert!(host_config_dir_for("codex", false).is_none());
        assert!(host_config_dir_for("agy", false).is_none());

        let dir = tempfile::tempdir().unwrap();
        let valid = dir.path().join("valid.json");
        std::fs::write(
            &valid,
            r#"{"claudeAiOauth":{"accessToken":"sk-ant-bounded-tok"}}"#,
        )
        .unwrap();
        assert_eq!(
            read_claude_credentials_from_path(&valid)
                .as_ref()
                .map(|e| e.access_token.as_str()),
            Some("sk-ant-bounded-tok")
        );

        let oversized = dir.path().join("huge.json");
        let pad = " ".repeat(MAX_HOST_CLAUDE_CRED_BYTES + 16);
        std::fs::write(
            &oversized,
            format!(r#"{{"claudeAiOauth":{{"accessToken":"sk-too-big"}}}}{pad}"#),
        )
        .unwrap();
        assert!(read_claude_credentials_from_path(&oversized).is_none());

        let link = dir.path().join("link.json");
        std::os::unix::fs::symlink(&valid, &link).unwrap();
        assert!(read_claude_credentials_from_path(&link).is_none());
    }

    #[test]
    fn pick_fresher_oauth_entry_prefers_newer_keychain_over_stale_file() {
        let stale = ClaudeOAuthToken {
            access_token: Zeroizing::new("stale-file-tok".into()),
            expires_at_ms: Some(1_000),
        };
        let fresh = ClaudeOAuthToken {
            access_token: Zeroizing::new("fresh-keychain-tok".into()),
            expires_at_ms: Some(9_000_000),
        };
        let picked = pick_fresher_oauth_entry(Some(stale), Some(fresh)).unwrap();
        assert_eq!(picked.access_token.as_str(), "fresh-keychain-tok");
        assert_eq!(picked.expires_at_ms, Some(9_000_000));

        let no_exp = ClaudeOAuthToken {
            access_token: Zeroizing::new("no-exp".into()),
            expires_at_ms: None,
        };
        let with_exp = ClaudeOAuthToken {
            access_token: Zeroizing::new("with-exp".into()),
            expires_at_ms: Some(5_000),
        };
        let picked2 = pick_fresher_oauth_entry(Some(no_exp), Some(with_exp)).unwrap();
        assert_eq!(picked2.access_token.as_str(), "with-exp");
    }

    #[test]
    fn refresh_host_claude_oauth_scrubs_env_isolates_flags_and_verifies_rotation() {
        use std::os::unix::fs::PermissionsExt as _;
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let capture = dir.path().join("capture.txt");
        let fake_claude = dir.path().join("fake-claude");
        let script = format!(
            "#!/bin/sh\nprintf 'ARGS=%s\\nPWD=%s\\nAPI=%s\\nOAUTH=%s\\nAUTOUPD=%s\\n' \"$*\" \"$(pwd)\" \"${{ANTHROPIC_API_KEY:-}}\" \"${{CLAUDE_CODE_OAUTH_TOKEN:-}}\" \"${{DISABLE_AUTOUPDATER:-}}\" > {}\nexit 0\n",
            capture.display()
        );
        std::fs::write(&fake_claude, script).unwrap();
        std::fs::set_permissions(&fake_claude, std::fs::Permissions::from_mode(0o755)).unwrap();

        // Case 1: script exits 0 but token does NOT rotate -> must return false.
        let unrotated = refresh_host_claude_oauth_with(&fake_claude, false, || {
            Some(ClaudeOAuthToken {
                access_token: Zeroizing::new("still-expired".into()),
                expires_at_ms: Some(1),
            })
        });
        assert!(
            !unrotated,
            "refresh must return false when token stays expired and unchanged"
        );
        let log = std::fs::read_to_string(&capture).unwrap();
        assert!(
            log.contains("--setting-sources  --tools  --disable-slash-commands --strict-mcp-config --mcp-config "),
            "refresh probe must pass isolation flags: {log}"
        );
        assert!(
            log.contains("API=\n") && log.contains("OAUTH=\n") && log.contains("AUTOUPD=1\n"),
            "refresh probe must scrub API/OAuth tokens and disable autoupdater: {log}"
        );
        assert!(
            log.contains("org-oauth-refresh-"),
            "refresh probe must run in isolated temp dir: {log}"
        );

        // Case 2: second read returns rotated token -> must return true.
        let called_once = Arc::new(AtomicBool::new(false));
        let called_clone = Arc::clone(&called_once);
        let rotated = refresh_host_claude_oauth_with(&fake_claude, false, move || {
            if !called_clone.swap(true, Ordering::SeqCst) {
                Some(ClaudeOAuthToken {
                    access_token: Zeroizing::new("old-tok".into()),
                    expires_at_ms: Some(1),
                })
            } else {
                Some(ClaudeOAuthToken {
                    access_token: Zeroizing::new("new-rotated-tok".into()),
                    expires_at_ms: Some(crate::credentials::now_ms() + 3_600_000),
                })
            }
        });
        assert!(
            rotated,
            "refresh must return true after verified token rotation"
        );

        // Case 3: reactive 401 refresh on early-revoked token whose local expiresAt is still
        // in the future MUST probe and return false if unchanged, true only if rotated.
        let future_exp = crate::credentials::now_ms() + 3_600_000;
        let unchanged_reactive = refresh_host_claude_oauth_with(&fake_claude, true, move || {
            Some(ClaudeOAuthToken {
                access_token: Zeroizing::new("revoked-early".into()),
                expires_at_ms: Some(future_exp),
            })
        });
        assert!(
            !unchanged_reactive,
            "reactive refresh must require token rotation when local expiresAt is still future"
        );
        let reactive_flag = Arc::new(AtomicBool::new(false));
        let reactive_clone = Arc::clone(&reactive_flag);
        let rotated_reactive = refresh_host_claude_oauth_with(&fake_claude, true, move || {
            if !reactive_clone.swap(true, Ordering::SeqCst) {
                Some(ClaudeOAuthToken {
                    access_token: Zeroizing::new("revoked-early".into()),
                    expires_at_ms: Some(future_exp),
                })
            } else {
                Some(ClaudeOAuthToken {
                    access_token: Zeroizing::new("fresh-rotated-early".into()),
                    expires_at_ms: Some(future_exp + 1_000),
                })
            }
        });
        assert!(
            rotated_reactive,
            "reactive refresh must return true when early-revoked token rotates"
        );
    }
}
