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
use std::time::{Duration, Instant};

use org_agent_runtime::{LaunchPlan, SpecPath};
use org_office::organization_family::records::ProviderAuthMode;
use zeroize::Zeroizing;

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

/// Triggers a bounded host-side Claude CLI refresh under the host account's own home so expired
/// `subscription_cli` OAuth access tokens rotate cleanly before spawning an isolated session.
pub(crate) fn refresh_host_claude_oauth(runtime_binary: &Path) -> bool {
    if !runtime_binary.is_file() {
        return false;
    }
    let Ok(mut child) = Command::new(runtime_binary)
        .args([
            "-p",
            "ok",
            "--model",
            "haiku",
            "--no-session-persistence",
            "--output-format",
            "text",
        ])
        .env_remove("CLAUDE_CODE_OAUTH_TOKEN")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let deadline = Instant::now() + OAUTH_REFRESH_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return false;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
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
/// `$HOME/.claude/.credentials.json` or macOS Keychain (`Claude Code-credentials`).
pub(crate) fn resolve_host_claude_oauth_entry() -> Option<ClaudeOAuthToken> {
    read_claude_credentials_file().or_else(read_claude_keychain_token)
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
}
