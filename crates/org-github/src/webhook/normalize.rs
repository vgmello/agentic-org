//! Raw GitHub JSON payload shapes and bounded, secret-redacted normalization helpers shared by
//! both webhook ingestion and REST API sync.

use org_proto::github::{
    GitHubCheckAnnotationView, GitHubCheckConclusion, GitHubCheckRunView, GitHubCheckStatus,
    GitHubPrReviewCommentView, GitHubPrReviewView, GitHubPrState, GitHubReviewState,
};
use serde::Deserialize;

use crate::config::validate_html_url;
use crate::error::WebhookError;
use crate::sanitize::{extract_task_refs, sanitize_github_body, sanitize_github_meta};

use super::ParsedPullRequestEvent;

pub(crate) const MAX_TITLE_BYTES: usize = 256;
pub(crate) const MAX_LOGIN_BYTES: usize = 64;
pub(crate) const MAX_REF_BYTES: usize = 160;
pub(crate) const MAX_SHA_BYTES: usize = 64;
pub(crate) const MAX_PATH_BYTES: usize = 256;
pub(crate) const MAX_PR_BODY_BYTES: usize = 4096;
pub(crate) const MAX_REVIEW_BODY_BYTES: usize = 2048;
pub(crate) const MAX_CHECK_NAME_BYTES: usize = 160;
pub(crate) const MAX_CHECK_SUMMARY_BYTES: usize = 4096;
pub(crate) const MAX_ANNOTATION_MESSAGE_BYTES: usize = 512;

/// Maximum CI check annotations retained per check run.
pub const MAX_CHECK_ANNOTATIONS: usize = 20;

#[derive(Deserialize)]
pub(crate) struct RawRepo {
    pub full_name: String,
}

#[derive(Deserialize)]
pub(crate) struct RawUser {
    pub login: String,
}

#[derive(Deserialize)]
pub(crate) struct RawBranch {
    #[serde(rename = "ref", default)]
    pub git_ref: String,
    #[serde(default)]
    pub sha: String,
}

#[derive(Deserialize)]
pub(crate) struct RawPrRef {
    pub number: u64,
    #[serde(default)]
    pub head: Option<RawBranch>,
}

#[derive(Deserialize)]
pub(crate) struct RawPullRequest {
    pub number: u64,
    pub state: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub merged: Option<bool>,
    #[serde(default)]
    pub merged_at: Option<String>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub html_url: String,
    #[serde(default)]
    pub diff_url: Option<String>,
    pub head: RawBranch,
    pub base: RawBranch,
    #[serde(default)]
    pub user: Option<RawUser>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct RawReview {
    pub id: u64,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub commit_id: Option<String>,
    #[serde(default)]
    pub html_url: Option<String>,
    #[serde(default)]
    pub submitted_at: Option<String>,
    #[serde(default)]
    pub user: Option<RawUser>,
}

#[derive(Deserialize)]
pub(crate) struct RawReviewComment {
    pub id: u64,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub line: Option<u32>,
    #[serde(default)]
    pub original_line: Option<u32>,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub html_url: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub user: Option<RawUser>,
}

#[derive(Deserialize)]
pub(crate) struct RawAnnotation {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub start_line: u32,
    #[serde(default)]
    pub end_line: Option<u32>,
    #[serde(default)]
    pub annotation_level: String,
    #[serde(default)]
    pub message: String,
}

#[derive(Deserialize)]
pub(crate) struct RawCheckOutput {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub annotations: Vec<RawAnnotation>,
    #[serde(default)]
    pub annotations_count: u32,
}

#[derive(Deserialize)]
pub(crate) struct RawAppRef {
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct RawCheckRun {
    pub id: u64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub conclusion: Option<String>,
    #[serde(default)]
    pub head_sha: String,
    #[serde(default)]
    pub html_url: Option<String>,
    #[serde(default)]
    pub details_url: Option<String>,
    #[serde(default)]
    pub completed_at: Option<String>,
    #[serde(default)]
    pub output: Option<RawCheckOutput>,
    #[serde(default)]
    pub app: Option<RawAppRef>,
    #[serde(default)]
    pub pull_requests: Vec<RawPrRef>,
}

/// Validates and normalizes an optional Git commit SHA strictly to lowercase ASCII hex
/// (`1..=MAX_SHA_BYTES`). Rejects any non-ASCII-hex or multibyte UTF-8 characters.
pub(crate) fn sanitize_git_sha_optional(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.len() > MAX_SHA_BYTES
        || !trimmed.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return None;
    }
    Some(trimmed.to_ascii_lowercase())
}

pub(crate) fn normalize_pull_request(
    repo_full_name: String,
    raw: RawPullRequest,
) -> Result<ParsedPullRequestEvent, WebhookError> {
    if raw.number == 0 {
        return Err(WebhookError::MalformedPayload(
            "pull_request.number must be positive".into(),
        ));
    }
    let state = if raw.merged == Some(true) || raw.merged_at.is_some() {
        GitHubPrState::Merged
    } else if raw.state.eq_ignore_ascii_case("open") {
        GitHubPrState::Open
    } else {
        GitHubPrState::Closed
    };
    let html_url = validate_html_url(&raw.html_url)
        .unwrap_or_else(|| format!("https://github.com/{repo_full_name}/pull/{}", raw.number));
    let diff_url = raw
        .diff_url
        .as_deref()
        .and_then(validate_html_url)
        .unwrap_or_else(|| format!("{html_url}.diff"));
    let title = sanitize_github_meta(&raw.title, MAX_TITLE_BYTES);
    let body_excerpt = raw
        .body
        .as_deref()
        .map(|b| sanitize_github_body(b, MAX_PR_BODY_BYTES))
        .filter(|s| !s.is_empty());
    let head_ref = sanitize_github_meta(&raw.head.git_ref, MAX_REF_BYTES);
    let head_sha = if raw.head.sha.trim().is_empty() {
        String::new()
    } else {
        sanitize_git_sha_optional(&raw.head.sha).ok_or_else(|| {
            WebhookError::MalformedPayload("pull_request.head.sha must be ASCII hex".into())
        })?
    };
    let base_ref = sanitize_github_meta(&raw.base.git_ref, MAX_REF_BYTES);
    let base_sha = sanitize_git_sha_optional(&raw.base.sha);
    let author_login = raw
        .user
        .map(|u| sanitize_github_meta(&u.login, MAX_LOGIN_BYTES))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_owned());
    let linked_task_refs = extract_task_refs(&head_ref, &title, body_excerpt.as_deref());
    let updated_at = raw
        .updated_at
        .map(|t| sanitize_github_meta(&t, 64))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(now_rfc3339);

    Ok(ParsedPullRequestEvent {
        repo_full_name,
        pr_number: raw.number,
        state,
        draft: raw.draft,
        title,
        body_excerpt,
        html_url,
        diff_url,
        head_ref,
        head_sha,
        base_ref,
        base_sha,
        author_login,
        linked_task_refs,
        updated_at,
    })
}

pub(crate) fn normalize_review(raw: RawReview) -> GitHubPrReviewView {
    GitHubPrReviewView {
        review_id: raw.id,
        state: parse_review_state(&raw.state),
        author_login: raw
            .user
            .map(|u| sanitize_github_meta(&u.login, MAX_LOGIN_BYTES))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "unknown".to_owned()),
        body_excerpt: raw
            .body
            .as_deref()
            .map(|b| sanitize_github_body(b, MAX_REVIEW_BODY_BYTES))
            .filter(|s| !s.is_empty()),
        commit_id: raw.commit_id.as_deref().and_then(sanitize_git_sha_optional),
        html_url: raw.html_url.as_deref().and_then(validate_html_url),
        submitted_at: raw
            .submitted_at
            .as_deref()
            .map(|s| sanitize_github_meta(s, 64))
            .filter(|s| !s.is_empty()),
    }
}

pub(crate) fn normalize_review_comment(raw: RawReviewComment) -> GitHubPrReviewCommentView {
    GitHubPrReviewCommentView {
        comment_id: raw.id,
        author_login: raw
            .user
            .map(|u| sanitize_github_meta(&u.login, MAX_LOGIN_BYTES))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "unknown".to_owned()),
        path: sanitize_github_meta(&raw.path, MAX_PATH_BYTES),
        line: raw.line.or(raw.original_line),
        body_excerpt: sanitize_github_body(&raw.body, MAX_REVIEW_BODY_BYTES),
        html_url: raw.html_url.as_deref().and_then(validate_html_url),
        updated_at: raw
            .updated_at
            .as_deref()
            .map(|s| sanitize_github_meta(s, 64))
            .filter(|s| !s.is_empty()),
    }
}

pub(crate) fn normalize_annotations(raw: Vec<RawAnnotation>) -> Vec<GitHubCheckAnnotationView> {
    raw.into_iter()
        .take(MAX_CHECK_ANNOTATIONS)
        .map(|a| GitHubCheckAnnotationView {
            path: sanitize_github_meta(&a.path, MAX_PATH_BYTES),
            start_line: a.start_line,
            end_line: a.end_line,
            level: sanitize_github_meta(&a.annotation_level, 32),
            message: sanitize_github_body(&a.message, MAX_ANNOTATION_MESSAGE_BYTES),
        })
        .collect()
}

pub(crate) fn normalize_check_run(raw: RawCheckRun) -> GitHubCheckRunView {
    let details_url = raw
        .details_url
        .as_deref()
        .and_then(validate_html_url)
        .or_else(|| raw.html_url.as_deref().and_then(validate_html_url));
    let (title, summary, annotations) = match raw.output {
        Some(out) => {
            let title = out
                .title
                .as_deref()
                .map(|t| sanitize_github_meta(t, MAX_TITLE_BYTES))
                .filter(|s| !s.is_empty());
            let combined_summary = out
                .summary
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .or(out.text.as_deref())
                .map(|s| sanitize_github_body(s, MAX_CHECK_SUMMARY_BYTES))
                .filter(|s| !s.is_empty());
            let anns = normalize_annotations(out.annotations);
            (title, combined_summary, anns)
        }
        None => (None, None, Vec::new()),
    };
    GitHubCheckRunView {
        check_run_id: raw.id,
        name: sanitize_github_meta(&raw.name, MAX_CHECK_NAME_BYTES),
        workflow_name: raw
            .app
            .and_then(|a| a.name)
            .map(|n| sanitize_github_meta(&n, MAX_CHECK_NAME_BYTES))
            .filter(|s| !s.is_empty()),
        status: parse_check_status(&raw.status),
        conclusion: raw.conclusion.as_deref().map(parse_check_conclusion),
        head_sha: sanitize_git_sha_optional(&raw.head_sha).unwrap_or_default(),
        details_url,
        title,
        summary,
        annotations,
        completed_at: raw
            .completed_at
            .as_deref()
            .map(|t| sanitize_github_meta(t, 64))
            .filter(|s| !s.is_empty()),
    }
}

fn parse_review_state(raw: &str) -> GitHubReviewState {
    match raw.trim().to_ascii_lowercase().as_str() {
        "approved" => GitHubReviewState::Approved,
        "changes_requested" => GitHubReviewState::ChangesRequested,
        "commented" => GitHubReviewState::Commented,
        "dismissed" => GitHubReviewState::Dismissed,
        "pending" => GitHubReviewState::Pending,
        _ => GitHubReviewState::Unknown,
    }
}

pub(crate) fn parse_check_status(raw: &str) -> GitHubCheckStatus {
    match raw.trim().to_ascii_lowercase().as_str() {
        "queued" => GitHubCheckStatus::Queued,
        "in_progress" => GitHubCheckStatus::InProgress,
        "completed" => GitHubCheckStatus::Completed,
        _ => GitHubCheckStatus::Unknown,
    }
}

pub(crate) fn parse_check_conclusion(raw: &str) -> GitHubCheckConclusion {
    match raw.trim().to_ascii_lowercase().as_str() {
        "success" => GitHubCheckConclusion::Success,
        "failure" => GitHubCheckConclusion::Failure,
        "timed_out" => GitHubCheckConclusion::TimedOut,
        "cancelled" => GitHubCheckConclusion::Cancelled,
        "action_required" => GitHubCheckConclusion::ActionRequired,
        "neutral" => GitHubCheckConclusion::Neutral,
        "skipped" => GitHubCheckConclusion::Skipped,
        "stale" => GitHubCheckConclusion::Stale,
        _ => GitHubCheckConclusion::Unknown,
    }
}

fn now_rfc3339() -> String {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    org_proto::format_rfc3339_ms(ms).unwrap_or_else(|| "1970-01-01T00:00:00.000Z".into())
}
