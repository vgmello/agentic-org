//! The single Git error mapping and log-redaction choke point: every Git error that reaches a log
//! or a caller passes through here. Log text is scrubbed of PEM private keys, URL userinfo and
//! secret-bearing `key=value`/header spans (quote-aware, line-scoped, UTF-8 safe) and truncated;
//! callers get fixed reasons only.

use org_git::GitError;
use org_proto::{ApiError, ErrorCode};

pub(super) fn map_office_error(err: org_office::OfficeError) -> ApiError {
    eprintln!("agentic-org-office: office error in git service: {err}");
    ApiError::new(ErrorCode::StorageError, err.to_string())
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

fn starts_with_ascii_ci(haystack: &str, needle: &str) -> bool {
    haystack
        .get(..needle.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(needle))
}

fn match_pem_block_len(rest: &str) -> Option<usize> {
    if !starts_with_ascii_ci(rest, "-----begin ") {
        return None;
    }
    let header_end = rest.find("-----").and_then(|_| rest[11..].find("-----"))?;
    let label = &rest[11..11 + header_end];
    if !label.to_ascii_lowercase().contains("private key") {
        return None;
    }
    let lower = rest.to_ascii_lowercase();
    if let Some(end_idx) = lower.find("-----end ") {
        if let Some(close) = rest[end_idx + 9..].find("-----") {
            return Some(end_idx + 9 + close + 5);
        }
    }
    Some(rest.len())
}

fn match_url_userinfo_prefix(rest: &str) -> Option<(usize, usize)> {
    let scheme_len = ["https://", "http://", "ssh://"]
        .iter()
        .find(|s| starts_with_ascii_ci(rest, s))
        .map(|s| s.len())?;
    let after_scheme = &rest[scheme_len..];
    let token_end = after_scheme
        .find(|c: char| c.is_whitespace() || matches!(c, '\'' | '"' | ')' | '>'))
        .unwrap_or(after_scheme.len());
    let authority = after_scheme[..token_end].split('/').next().unwrap_or("");
    let at_pos = authority.rfind('@')?;
    Some((scheme_len, scheme_len + at_pos + 1))
}

fn match_secret_span_len(rest: &str, prev_is_alnum: bool) -> Option<usize> {
    const CI_PREFIXES: &[&str] = &[
        "authorization:",
        "proxy-authorization:",
        "bearer ",
        "basic ",
        "x-access-token:",
        "github_token=",
        "gh_token=",
        "git_ssh_command=",
        "access_token=",
        "refresh_token=",
        "private_key=",
        "api_key=",
        "apikey=",
        "password=",
        "passwd=",
        "token=",
        "secret=",
        "ghp_",
        "gho_",
        "ghu_",
        "ghs_",
        "ghr_",
        "github_pat_",
        "glpat-",
        "sk-ant-",
        "sk-proj-",
    ];
    let prefix_len = CI_PREFIXES
        .iter()
        .find(|p| starts_with_ascii_ci(rest, p))
        .map(|p| p.len())
        .or_else(|| (!prev_is_alnum && starts_with_ascii_ci(rest, "sk-")).then_some(3))?;
    let tail = &rest[prefix_len..];
    let is_line_scoped = starts_with_ascii_ci(rest, "authorization:")
        || starts_with_ascii_ci(rest, "proxy-authorization:")
        || starts_with_ascii_ci(rest, "x-access-token:")
        || starts_with_ascii_ci(rest, "git_ssh_command=");
    let end = if is_line_scoped {
        tail.find(['\n', '\r'])
            .map_or(rest.len(), |i| prefix_len + i)
    } else {
        prefix_len + scan_secret_value_len(tail)
    };
    Some(end)
}

fn scan_secret_value_len(tail: &str) -> usize {
    let ws_skip = tail
        .find(|c: char| !matches!(c, ' ' | '\t'))
        .unwrap_or(tail.len());
    let val = &tail[ws_skip..];
    if let Some(quote) = val.chars().next().filter(|c| matches!(c, '\'' | '"')) {
        let after_open = &val[quote.len_utf8()..];
        let stop = after_open
            .find([quote, '\n', '\r'])
            .map_or(after_open.len(), |i| {
                if after_open[i..].starts_with(quote) {
                    i + quote.len_utf8()
                } else {
                    i
                }
            });
        ws_skip + quote.len_utf8() + stop
    } else {
        val.find(|c: char| {
            c.is_whitespace() || matches!(c, '\'' | '"' | '&' | ';' | ',' | '\n' | '\r' | ')')
        })
        .map_or(tail.len(), |i| ws_skip + i)
    }
}

pub(super) fn scrub_git_log_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len().min(512));
    let mut rest = raw;
    let mut prev_is_alnum = false;
    while !rest.is_empty() && out.len() < 512 {
        if let Some(pem_len) = match_pem_block_len(rest) {
            out.push_str("<redacted-private-key>");
            rest = &rest[pem_len..];
            prev_is_alnum = false;
            continue;
        }
        if let Some((scheme_len, after_at)) = match_url_userinfo_prefix(rest) {
            // Scrub `userinfo@` only, leaving the host/path/query to continue through the loop
            // so query secrets like `?token=SECRET` are scrubbed too.
            out.push_str(&rest[..scheme_len]);
            out.push_str("<redacted>@");
            rest = &rest[after_at..];
            prev_is_alnum = false;
            continue;
        }
        if let Some(span_len) = match_secret_span_len(rest, prev_is_alnum) {
            out.push_str("<redacted>");
            rest = &rest[span_len..];
            prev_is_alnum = false;
            continue;
        }
        let ch = rest.chars().next().unwrap_or(' ');
        out.push(if ch.is_control() && ch != '\n' {
            ' '
        } else {
            ch
        });
        prev_is_alnum = ch.is_ascii_alphanumeric();
        rest = &rest[ch.len_utf8()..];
    }
    if out.len() > 512 || !rest.is_empty() {
        let mut cut = out.len().min(512);
        while cut > 0 && !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
        out.push_str("...[truncated]");
    }
    out
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
