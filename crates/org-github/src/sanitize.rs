//! Untrusted GitHub text sanitization, secret redaction, task-reference extraction, CI status
//! aggregation, and prompt-injection-safe context fencing.
//!
//! Every GitHub-originated title, author login, branch name, review comment, and CI failure
//! snippet passes through `org_redact::redact_text` and `org_redact::sanitize` BEFORE being
//! persisted or returned to humans/agents.

use org_proto::github::{
    GitHubCheckConclusion, GitHubCheckRunView, GitHubCheckStatus, GitHubCiOverallStatus,
    GitHubPrReviewCommentView, GitHubPrReviewView, GitHubPrSummaryView,
};
use org_redact::redact_text;
use org_redact::sanitize::{sanitize_single_line_display, sanitize_untrusted_body};

/// Maximum task references extracted from a single PR (branch + title + body).
pub const MAX_EXTRACTED_TASK_REFS: usize = 16;

/// Maximum UTF-8 bytes for the fenced untrusted GitHub context rendered by `github.pr.inspect`.
pub const MAX_FENCED_CONTEXT_BYTES: usize = 16 * 1024;

/// Opening fence prefix for untrusted GitHub PR/review/CI content.
pub const UNTRUSTED_GITHUB_OPEN: &str = "<<<UNTRUSTED_GITHUB_CONTEXT";
/// Closing fence delimiter for untrusted GitHub PR/review/CI content.
pub const UNTRUSTED_GITHUB_CLOSE: &str = "<<<END_UNTRUSTED_GITHUB_CONTEXT>>>";

/// Redacts secrets, strips ANSI/control/BiDi characters, neutralizes fence delimiters, collapses
/// whitespace onto one line, and truncates safely on a UTF-8 char boundary.
pub fn sanitize_github_meta(raw: &str, max_bytes: usize) -> String {
    let redacted = redact_text(raw, &[]);
    let single_line = sanitize_single_line_display(&redacted);
    sanitize_untrusted_body(&single_line, max_bytes)
}

/// Redacts secrets, strips control/invisible characters (preserving newlines and tabs),
/// neutralizes fence breakout delimiters (`<<<`, `>>>`, `===`), and truncates safely on a UTF-8
/// char boundary.
pub fn sanitize_github_body(raw: &str, max_bytes: usize) -> String {
    let redacted = redact_text(raw, &[]);
    sanitize_untrusted_body(&redacted, max_bytes)
}

/// Extracts normalized Agentic Org task keys (e.g. `T-A42`, `T-HQ12`, `T-CORE9`) from a PR's
/// head branch (`task/T-A42-*`), title, and optional body, preserving first-seen order without
/// duplicates.
pub fn extract_task_refs(branch: &str, title: &str, body: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    for source in [Some(branch), Some(title), body].into_iter().flatten() {
        scan_task_refs(source, &mut out);
        if out.len() >= MAX_EXTRACTED_TASK_REFS {
            break;
        }
    }
    out
}

fn scan_task_refs(input: &str, out: &mut Vec<String>) {
    let len = input.len();
    let mut i = 0;
    while i + 3 < len && out.len() < MAX_EXTRACTED_TASK_REFS {
        if let Some((normalized, next_idx)) = parse_task_ref_at(input, i) {
            if !out.contains(&normalized) {
                out.push(normalized);
            }
            i = next_idx;
        } else {
            i += 1;
        }
    }
}

fn parse_task_ref_at(input: &str, start: usize) -> Option<(String, usize)> {
    let bytes = input.as_bytes();
    if start > 0 && bytes[start - 1].is_ascii_alphanumeric() {
        return None;
    }
    if !matches!(bytes[start], b'T' | b't') || bytes[start + 1] != b'-' {
        return None;
    }
    let letters_start = start + 2;
    let letters_end = take_ascii_run(bytes, letters_start, 8, u8::is_ascii_alphabetic);
    if letters_end == letters_start {
        return None;
    }
    let digits_end = take_ascii_run(bytes, letters_end, 7, u8::is_ascii_digit);
    if digits_end == letters_end {
        return None;
    }
    if digits_end < bytes.len() && bytes[digits_end].is_ascii_alphanumeric() {
        return None;
    }
    let digits = &input[letters_end..digits_end];
    if !digits.parse::<u64>().is_ok_and(|n| n > 0) {
        return None;
    }
    let normalized = format!(
        "T-{}{}",
        input[letters_start..letters_end].to_ascii_uppercase(),
        digits
    );
    Some((normalized, digits_end))
}

fn take_ascii_run(bytes: &[u8], start: usize, max_len: usize, pred: fn(&u8) -> bool) -> usize {
    let limit = (start + max_len).min(bytes.len());
    let mut cursor = start;
    while cursor < limit && pred(&bytes[cursor]) {
        cursor += 1;
    }
    cursor
}

/// Computes an aggregate CI status from a PR's latest check runs.
pub fn compute_ci_overall_status(checks: &[GitHubCheckRunView]) -> GitHubCiOverallStatus {
    if checks.is_empty() {
        return GitHubCiOverallStatus::Unknown;
    }
    if checks.iter().any(|c| {
        c.status == GitHubCheckStatus::Completed
            && c.conclusion.is_some_and(GitHubCheckConclusion::is_failure)
    }) {
        return GitHubCiOverallStatus::Failing;
    }
    if checks.iter().any(|c| {
        matches!(
            c.status,
            GitHubCheckStatus::Queued | GitHubCheckStatus::InProgress
        )
    }) {
        return GitHubCiOverallStatus::Pending;
    }
    if checks
        .iter()
        .any(|c| c.conclusion == Some(GitHubCheckConclusion::Success))
    {
        return GitHubCiOverallStatus::Passing;
    }
    if checks.iter().all(|c| {
        matches!(
            c.conclusion,
            Some(
                GitHubCheckConclusion::Neutral
                    | GitHubCheckConclusion::Skipped
                    | GitHubCheckConclusion::Stale
            )
        )
    }) {
        return GitHubCiOverallStatus::Neutral;
    }
    GitHubCiOverallStatus::Unknown
}

/// Renders PR metadata, reviews, review comments, and failing CI summaries inside a strict
/// untrusted-data fence (`<<<UNTRUSTED_GITHUB_CONTEXT ...>>>`).
///
/// All interpolated strings are re-sanitized through [`sanitize_github_meta`] and
/// [`sanitize_github_body`] so untrusted GitHub payloads cannot break out of the fence or leak
/// secrets into an agent's context window.
pub fn format_fenced_pr_context(
    pr: &GitHubPrSummaryView,
    reviews: &[GitHubPrReviewView],
    comments: &[GitHubPrReviewCommentView],
    checks: &[GitHubCheckRunView],
) -> String {
    let repo = sanitize_github_meta(&pr.repo_full_name, 140);
    let title = sanitize_github_meta(&pr.title, 256);
    let author = sanitize_github_meta(&pr.author_login, 64);
    let head = sanitize_github_meta(&pr.head_ref, 128);
    let base = sanitize_github_meta(&pr.base_ref, 128);

    let mut body = String::with_capacity(1024);
    body.push_str(&format!(
        "title: {title}\nauthor: {author}\nstate: {:?} (draft={})\nbranches: {head} -> {base}\ndiff_url: {}\nci_status: {:?} (failing={}, total={})\n",
        pr.state,
        pr.draft,
        sanitize_github_meta(&pr.diff_url, 512),
        pr.ci_status,
        pr.failing_checks_count,
        pr.total_checks_count,
    ));

    if let Some(excerpt) = pr.body_excerpt.as_deref().filter(|s| !s.is_empty()) {
        body.push_str("\npr_description:\n");
        body.push_str(&sanitize_github_body(excerpt, 2048));
        body.push('\n');
    }

    if !reviews.is_empty() {
        body.push_str("\nreviews:\n");
        for rev in reviews.iter().take(15) {
            let who = sanitize_github_meta(&rev.author_login, 64);
            body.push_str(&format!("- {who}: {:?}", rev.state));
            if let Some(text) = rev.body_excerpt.as_deref().filter(|s| !s.is_empty()) {
                body.push_str(" — ");
                body.push_str(&sanitize_github_meta(text, 256));
            }
            body.push('\n');
        }
    }

    if !comments.is_empty() {
        body.push_str("\nreview_comments:\n");
        for c in comments.iter().take(20) {
            let who = sanitize_github_meta(&c.author_login, 64);
            let path = sanitize_github_meta(&c.path, 160);
            let loc = c
                .line
                .map_or_else(|| path.clone(), |l| format!("{path}:{l}"));
            let msg = sanitize_github_meta(&c.body_excerpt, 320);
            body.push_str(&format!("- {who} @ {loc}: {msg}\n"));
        }
    }

    if !checks.is_empty() {
        body.push_str("\nci_checks:\n");
        for chk in checks.iter().take(25) {
            let name = sanitize_github_meta(&chk.name, 128);
            body.push_str(&format!(
                "- {name}: status={:?} conclusion={:?}",
                chk.status, chk.conclusion
            ));
            if let Some(summary) = chk.summary.as_deref().filter(|s| !s.is_empty()) {
                body.push_str(" | ");
                body.push_str(&sanitize_github_meta(summary, 320));
            }
            body.push('\n');
            for ann in chk.annotations.iter().take(5) {
                let path = sanitize_github_meta(&ann.path, 160);
                let msg = sanitize_github_meta(&ann.message, 200);
                body.push_str(&format!("  * {path}:{}: {msg}\n", ann.start_line));
            }
        }
    }

    let open = format!(
        "{UNTRUSTED_GITHUB_OPEN} source=\"github\" repo=\"{repo}\" pr=\"#{}\">>>\n",
        pr.pr_number
    );
    let close = format!("\n{UNTRUSTED_GITHUB_CLOSE}");
    let budget = MAX_FENCED_CONTEXT_BYTES.saturating_sub(open.len() + close.len());
    let bounded_body = sanitize_untrusted_body(&body, budget);
    format!("{open}{bounded_body}{close}")
}
