//! Host-local CLI runtime auth helpers for `subscription_cli` bindings (`TASK-120`).
//!
//! Claude Code sessions keep `LaunchContext::config_dir = None` so host `~/.claude/plugins` are
//! never loaded (preserving S1 isolation) and macOS Keychain hashing is avoided; instead, when no
//! vault credential is enrolled for a `subscription_cli` connection, the host OAuth token is
//! resolved cleanly and passed via `CLAUDE_CODE_OAUTH_TOKEN` (scrubbed from tool subprocesses by
//! `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB=1` and redacted from transcripts).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use org_agent_runtime::{LaunchPlan, SpecPath};
use org_office::organization_family::records::ProviderAuthMode;
use zeroize::Zeroizing;

const MAX_HOST_CLAUDE_CRED_BYTES: usize = 64 * 1024;
/// Maximum byte length for an agent's `TMPDIR` so Linux `bwrap`/`socat` bridge sockets
/// (`$TMPDIR/<name>.sock`, ~35 bytes) stay well under the 108-byte `sun_path` limit.
const MAX_SHORT_TMPDIR_BYTES: usize = 70;

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
    let digest = ring::digest::digest(
        &ring::digest::SHA256,
        format!("{office_id}:{employee_id}").as_bytes(),
    );
    let mut slug = String::with_capacity(8);
    for &b in &digest.as_ref()[..4] {
        use std::fmt::Write as _;
        let _ = write!(slug, "{b:02x}");
    }
    let candidate = sock_parent.join("t").join(slug);
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
pub(crate) fn append_host_claude_oauth(
    runtime_id: &str,
    allowed: bool,
    plan: &LaunchPlan,
    launch_env: &mut Vec<(OsString, OsString)>,
) {
    if !allowed || runtime_id != "claude" || !plan_protects_claude_oauth(plan) {
        return;
    }
    if let Some(token) = resolve_host_claude_oauth_token() {
        launch_env.push((
            OsString::from("CLAUDE_CODE_OAUTH_TOKEN"),
            OsString::from(token.as_str()),
        ));
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

/// Resolves the host user's local Claude Code OAuth access token from
/// `$HOME/.claude/.credentials.json` or macOS Keychain (`Claude Code-credentials`).
pub(crate) fn resolve_host_claude_oauth_token() -> Option<Zeroizing<String>> {
    read_claude_credentials_file().or_else(read_claude_keychain_token)
}

fn read_claude_credentials_file() -> Option<Zeroizing<String>> {
    let cred_path = std::env::var_os("HOME")
        .map(PathBuf::from)?
        .join(".claude")
        .join(".credentials.json");
    let bytes = std::fs::read(&cred_path).ok()?;
    if bytes.len() > MAX_HOST_CLAUDE_CRED_BYTES {
        return None;
    }
    parse_claude_oauth_access_token(&bytes)
}

fn read_claude_keychain_token() -> Option<Zeroizing<String>> {
    let security_bin = Path::new("/usr/bin/security");
    if !security_bin.is_file() {
        return None;
    }
    let out = Command::new(security_bin)
        .args([
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-w",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() || out.stdout.len() > MAX_HOST_CLAUDE_CRED_BYTES {
        return None;
    }
    parse_claude_oauth_access_token(&out.stdout)
}

fn parse_claude_oauth_access_token(raw: &[u8]) -> Option<Zeroizing<String>> {
    let val: serde_json::Value = serde_json::from_slice(raw).ok()?;
    let token = val
        .get("claudeAiOauth")?
        .get("accessToken")?
        .as_str()?
        .trim();
    (!token.is_empty()).then(|| Zeroizing::new(token.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_claude_oauth_access_token_json() {
        let valid = br#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-example"}}"#;
        assert_eq!(
            parse_claude_oauth_access_token(valid)
                .as_deref()
                .map(String::as_str),
            Some("sk-ant-oat01-example")
        );
        assert!(
            parse_claude_oauth_access_token(br#"{"claudeAiOauth":{"accessToken":"  "}}"#).is_none()
        );
        assert!(parse_claude_oauth_access_token(br#"not-json"#).is_none());
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
}
