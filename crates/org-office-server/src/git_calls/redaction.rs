//! The single Git error mapping and log-redaction choke point (`TASK-168`, `TASK-218`): every Git
//! error that reaches a log or a caller passes through here.
//!
//! Secret/token/PEM/URL-userinfo/header/env-assignment redaction delegates to the workspace
//! canonical Layer-0 choke-point [`org_redact::redact_text`], followed by Git-specific
//! `GIT_SSH_COMMAND=` / `GIT_ASKPASS=` line scrubbing, control-character normalization, and
//! UTF-8-safe truncation. Callers receive fixed reasons only.

use org_git::GitError;
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

/// Scrubs Git-specific environment variable dumps (`GIT_SSH_COMMAND=...`, `GIT_ASKPASS=...`)
/// that are not classified as generic secret keys by `org_redact`.
fn scrub_git_env_lines(input: &str) -> String {
    const KEYS: &[&str] = &["git_ssh_command=", "git_askpass="];
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
            let val_start = i + key.len();
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
