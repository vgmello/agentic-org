//! The single Git error mapping and log-redaction choke point (`TASK-168`, `TASK-218`): every Git
//! error that reaches a log or a caller passes through here.
//!
//! Secret/token/PEM/URL-userinfo/header/env-assignment redaction delegates to the workspace
//! canonical Layer-0 choke-point [`org_redact::redact_text`], followed by Git-specific
//! `GIT_SSH_COMMAND=` / `GIT_ASKPASS=` line scrubbing, control-character normalization, and
//! UTF-8-safe truncation. Callers receive fixed reasons only.

use org_git::{AllowedProtocol, GitCredential, GitError, RemoteSpec};
use org_proto::{ApiError, ErrorCode};
use org_redact::{redact_text, REDACTED};

const MAX_GIT_LOG_BYTES: usize = 512;
const TRUNCATED_SUFFIX: &str = "...[truncated]";

pub(super) fn map_office_error(err: org_office::OfficeError) -> ApiError {
    let scrubbed = scrub_git_log_text(&err.to_string());
    eprintln!("agentic-org-office: office error in git service: {scrubbed}");
    ApiError::new(ErrorCode::StorageError, "the office could not answer")
}

pub(super) fn sanitized_git_reason(err: &GitError) -> String {
    match err {
        GitError::InvalidInput(_) => "invalid Git input".into(),
        GitError::SecurityViolation(_) => "Git repository security check failed".into(),
        GitError::Conflict(_) => "Git conflict (non-fast-forward or workspace conflict)".into(),
        GitError::UnsupportedGit(_) => "host Git binary unavailable or unsupported version".into(),
        GitError::RemoteUnavailable(_) => "Git remote is unreachable or refused connection".into(),
        GitError::CommandFailed { .. } => "Git command failed".into(),
        GitError::Io { .. } => "office Git storage error".into(),
    }
}

/// Scrubs Git-specific environment variable dumps (`GIT_SSH_COMMAND=...`, `GIT_ASKPASS=...`,
/// `GIT_CONFIG_VALUE_<N>=...`) that are not classified as generic secret keys by `org_redact`.
fn scrub_git_env_lines(input: &str) -> String {
    const KEYS: &[&str] = &["git_ssh_command=", "git_askpass=", "git_config_value_"];
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut pos = 0;
    let mut i = 0;
    while i < bytes.len() {
        let escaped_space =
            i >= 2 && bytes[i - 2] == b'\\' && matches!(bytes[i - 1], b'n' | b'r' | b't');
        let word_start = i == 0 || !bytes[i - 1].is_ascii_alphanumeric() || escaped_space;
        let matched = word_start
            .then(|| {
                KEYS.iter().find(|k| {
                    bytes
                        .get(i..i + k.len())
                        .is_some_and(|slice| slice.eq_ignore_ascii_case(k.as_bytes()))
                })
            })
            .flatten();
        if let Some(key) = matched {
            let mut val_start = i + key.len();
            if *key == "git_config_value_" {
                while val_start < bytes.len() && bytes[val_start].is_ascii_digit() {
                    val_start += 1;
                }
                if bytes.get(val_start) != Some(&b'=') {
                    i += 1;
                    continue;
                }
                val_start += 1;
            }
            let val_end = input[val_start..]
                .find(['\n', '\r'])
                .map_or(input.len(), |off| val_start + off);
            out.push_str(&input[pos..val_start]);
            out.push_str(REDACTED);
            pos = val_end;
            i = val_end;
        } else {
            i += 1;
        }
    }
    out.push_str(&input[pos..]);
    out
}

pub(super) fn scrub_git_log_text(raw: &str) -> String {
    let redacted = redact_text(raw, &[]);
    let git_scrubbed = scrub_git_env_lines(&redacted);
    let mut cleaned: String = git_scrubbed
        .chars()
        .map(|ch| {
            if ch.is_control() && ch != '\n' {
                ' '
            } else {
                ch
            }
        })
        .collect();
    if cleaned.len() > MAX_GIT_LOG_BYTES {
        let cut = cleaned.floor_char_boundary(MAX_GIT_LOG_BYTES - TRUNCATED_SUFFIX.len());
        cleaned.truncate(cut);
        cleaned.push_str(TRUNCATED_SUFFIX);
    }
    cleaned
}

pub(super) fn redact_git_error_for_log(err: &GitError) -> String {
    match err {
        GitError::CommandFailed { status, stderr } => {
            format!(
                "git command failed ({status}): {}",
                scrub_git_log_text(stderr)
            )
        }
        GitError::RemoteUnavailable(msg) => {
            format!(
                "git remote unreachable or failed: {}",
                scrub_git_log_text(msg)
            )
        }
        other => scrub_git_log_text(&other.to_string()),
    }
}

/// Whether a raw Git HTTPS stderr message indicates missing or rejected authentication rather
/// than a transient network outage (`TASK-135`).
pub(super) fn is_https_auth_failure(stderr: &str) -> bool {
    let lower = stderr.to_ascii_lowercase();
    [
        "could not read username",
        "could not read password",
        "terminal prompts disabled",
        "authentication failed",
        "invalid username or token",
        "invalid username or password",
        "returned error: 401",
        "returned error: 403",
        "http basic: access denied",
        "repository not found",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

pub(super) fn sanitized_remote_git_reason(err: &GitError, remote: Option<&RemoteSpec>) -> String {
    if let (GitError::RemoteUnavailable(detail), Some(spec)) = (err, remote) {
        if spec.protocol == AllowedProtocol::Https && is_https_auth_failure(detail) {
            return if matches!(spec.credential, GitCredential::None) {
                "Git HTTPS authentication required: configure a scoped repository credential in this office's vault"
                    .into()
            } else {
                "Git HTTPS authentication rejected: rotate the configured office repository credential"
                    .into()
            };
        }
    }
    sanitized_git_reason(err)
}

/// Maps a remote Git error with an actionable, secret-free operator hint when an HTTPS remote
/// fails authentication on an office/satellite lacking a scoped vault credential (`TASK-135`).
pub(super) fn map_remote_git_error(err: GitError, remote: &RemoteSpec, repo_ref: &str) -> ApiError {
    if let GitError::RemoteUnavailable(ref detail) = err {
        if remote.protocol == AllowedProtocol::Https && is_https_auth_failure(detail) {
            eprintln!(
                "agentic-org-office: git auth error: {}",
                redact_git_error_for_log(&err)
            );
            let message = if matches!(remote.credential, GitCredential::None) {
                format!(
                    "Git HTTPS authentication failed for `{repo_ref}`: no scoped repository credential is configured in this office's vault (personal host `gh` credentials are never used). Configure one with `agentic-org repo set-credential --repo {repo_ref}`."
                )
            } else {
                format!(
                    "Git HTTPS authentication failed for `{repo_ref}` with the configured office repository credential (token may be expired, revoked, or lack repository scope). Rotate it with `agentic-org repo set-credential --repo {repo_ref}`."
                )
            };
            return ApiError::new(ErrorCode::NotAuthorized, message);
        }
    }
    map_git_error(err)
}

pub(super) fn map_git_error(err: GitError) -> ApiError {
    eprintln!(
        "agentic-org-office: git error: {}",
        redact_git_error_for_log(&err)
    );
    match &err {
        GitError::InvalidInput(msg) => ApiError::new(ErrorCode::InvalidRequest, msg.clone()),
        GitError::SecurityViolation(_) => ApiError::new(
            ErrorCode::InvalidRequest,
            "Git repository integrity or security check failed",
        ),
        GitError::Conflict(_) => ApiError::new(
            ErrorCode::Conflict,
            "Git push or workspace operation conflicted with repository state",
        ),
        GitError::UnsupportedGit(_) => ApiError::new(
            ErrorCode::Unavailable,
            "host Git binary unavailable or older than required version",
        ),
        GitError::RemoteUnavailable(_) => ApiError::new(
            ErrorCode::Unavailable,
            "Git remote is unreachable or missing requested branch",
        ),
        GitError::CommandFailed { .. } => {
            ApiError::new(ErrorCode::Unavailable, "Git operation failed")
        }
        GitError::Io { .. } => ApiError::new(ErrorCode::StorageError, "office Git storage error"),
    }
}
