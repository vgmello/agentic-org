//! Constant-time HMAC-SHA256 GitHub webhook verification, bounded delivery replay deduplication,
//! and typed, secret-redacted parsers for PR, review, review comment, and CI check events.

mod normalize;

use std::collections::{HashSet, VecDeque};

use org_proto::github::{
    GitHubCheckRunView, GitHubPrReviewCommentView, GitHubPrReviewView, GitHubPrState,
    GITHUB_WEBHOOK_MAX_BYTES,
};
use ring::hmac;
use serde::Deserialize;

use crate::config::{validate_html_url, ValidatedRepoSlug};
use crate::error::WebhookError;
use crate::sanitize::sanitize_github_meta;

pub use self::normalize::MAX_CHECK_ANNOTATIONS;
pub(crate) use self::normalize::{
    normalize_annotations, normalize_check_run, normalize_pull_request, normalize_review,
    normalize_review_comment, parse_check_conclusion, parse_check_status,
    sanitize_git_sha_optional, RawAnnotation, RawAppRef, RawCheckRun, RawPrRef, RawPullRequest,
    RawRepo, RawReview, RawReviewComment, MAX_CHECK_NAME_BYTES,
};

/// Maximum accepted raw webhook payload size (512 KiB).
pub const MAX_WEBHOOK_PAYLOAD_BYTES: usize = GITHUB_WEBHOOK_MAX_BYTES;

/// Default bounded capacity for `X-GitHub-Delivery` replay deduplication.
pub const MAX_DEDUP_DELIVERIES: usize = 4096;

const MAX_EVENT_NAME_BYTES: usize = 64;

/// Verifies webhook payload size, constant-time HMAC-SHA256 signature (`X-Hub-Signature-256`) via
/// `ring::hmac::verify`, and then required event/delivery headers (`X-GitHub-Event`,
/// `X-GitHub-Delivery`).
///
/// Signature authentication strictly precedes event/delivery syntax validation so unauthenticated
/// callers cannot probe secret configuration or organization existence via a `400` vs `401`
/// oracle. Fails closed on any oversized payload, unconfigured secret, mismatched MAC, or
/// missing/malformed header.
pub fn verify_webhook_headers_and_signature(
    event: &str,
    delivery_id: &str,
    signature_256: &str,
    payload: &[u8],
    webhook_secret: &str,
) -> Result<(String, String), WebhookError> {
    if payload.len() > MAX_WEBHOOK_PAYLOAD_BYTES {
        return Err(WebhookError::PayloadTooLarge);
    }
    if webhook_secret.is_empty() {
        return Err(WebhookError::SecretNotConfigured);
    }

    let sig = signature_256.trim();
    if sig.is_empty() {
        return Err(WebhookError::MissingHeader("X-Hub-Signature-256"));
    }
    let hex_digest = sig
        .strip_prefix("sha256=")
        .ok_or(WebhookError::MalformedSignature)?;
    let sig_bytes = decode_hex_32(hex_digest).ok_or(WebhookError::MalformedSignature)?;

    let key = hmac::Key::new(hmac::HMAC_SHA256, webhook_secret.as_bytes());
    hmac::verify(&key, payload, &sig_bytes).map_err(|_| WebhookError::InvalidSignature)?;

    let event = event.trim();
    if event.is_empty() {
        return Err(WebhookError::MissingHeader("X-GitHub-Event"));
    }
    if event.len() > MAX_EVENT_NAME_BYTES
        || !event
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err(WebhookError::InvalidEventName);
    }

    let delivery_id = delivery_id.trim();
    if delivery_id.is_empty() {
        return Err(WebhookError::MissingHeader("X-GitHub-Delivery"));
    }
    if !is_valid_uuid(delivery_id) {
        return Err(WebhookError::InvalidDeliveryId);
    }

    Ok((event.to_owned(), delivery_id.to_ascii_lowercase()))
}

/// Bounded FIFO deduplication ring of `X-GitHub-Delivery` UUIDs preventing replayed webhooks
/// from duplicating records or notifications.
#[derive(Debug, Clone)]
pub struct DeliveryDeduplicator {
    max_entries: usize,
    order: VecDeque<String>,
    seen: HashSet<String>,
}

impl Default for DeliveryDeduplicator {
    fn default() -> Self {
        Self::new(MAX_DEDUP_DELIVERIES)
    }
}

impl DeliveryDeduplicator {
    pub fn new(max_entries: usize) -> Self {
        let cap = max_entries.clamp(1, MAX_DEDUP_DELIVERIES);
        Self {
            max_entries: cap,
            order: VecDeque::with_capacity(cap.min(256)),
            seen: HashSet::with_capacity(cap.min(256)),
        }
    }

    pub fn from_ids(ids: Vec<String>, max_entries: usize) -> Self {
        let mut dedup = Self::new(max_entries);
        for id in ids {
            let _ = dedup.record_if_new(&id);
        }
        dedup
    }

    /// Returns `true` if `delivery_id` has not been seen before and records it; returns `false`
    /// if it is a replay/duplicate.
    pub fn record_if_new(&mut self, delivery_id: &str) -> bool {
        let normalized = delivery_id.trim().to_ascii_lowercase();
        if normalized.is_empty() || self.seen.contains(&normalized) {
            return false;
        }
        while self.order.len() >= self.max_entries {
            if let Some(evicted) = self.order.pop_front() {
                self.seen.remove(&evicted);
            }
        }
        self.seen.insert(normalized.clone());
        self.order.push_back(normalized);
        true
    }

    pub fn contains(&self, delivery_id: &str) -> bool {
        self.seen.contains(&delivery_id.trim().to_ascii_lowercase())
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub fn snapshot(&self) -> Vec<String> {
        self.order.iter().cloned().collect()
    }
}

/// Sanitized pull request snapshot extracted from a `pull_request` webhook or REST API payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedPullRequestEvent {
    pub repo_full_name: String,
    pub pr_number: u64,
    pub state: GitHubPrState,
    pub draft: bool,
    pub title: String,
    pub body_excerpt: Option<String>,
    pub html_url: String,
    pub diff_url: String,
    pub head_ref: String,
    pub head_sha: String,
    pub base_ref: String,
    pub base_sha: Option<String>,
    pub author_login: String,
    pub linked_task_refs: Vec<String>,
    pub updated_at: String,
}

/// Sanitized pull request review extracted from `pull_request_review`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedPullRequestReviewEvent {
    pub repo_full_name: String,
    pub pr_number: u64,
    pub head_sha: Option<String>,
    pub review: GitHubPrReviewView,
}

/// Sanitized inline pull request review comment extracted from `pull_request_review_comment`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedPullRequestReviewCommentEvent {
    pub repo_full_name: String,
    pub pr_number: u64,
    pub head_sha: Option<String>,
    pub comment: GitHubPrReviewCommentView,
}

/// Sanitized CI check run extracted from `check_run`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCheckRunEvent {
    pub repo_full_name: String,
    pub pr_numbers: Vec<u64>,
    pub check_run: GitHubCheckRunView,
}

/// Sanitized CI check suite extracted from `check_suite`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCheckSuiteEvent {
    pub repo_full_name: String,
    pub pr_numbers: Vec<u64>,
    pub check_run: GitHubCheckRunView,
}

/// Result of parsing a verified GitHub webhook JSON body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedWebhookEvent {
    PullRequest(ParsedPullRequestEvent),
    PullRequestReview(ParsedPullRequestReviewEvent),
    PullRequestReviewComment(ParsedPullRequestReviewCommentEvent),
    CheckRun(ParsedCheckRunEvent),
    CheckSuite(ParsedCheckSuiteEvent),
    Ping { repo_full_name: Option<String> },
    Unsupported { event: String },
}

/// Parses an already-authenticated webhook JSON payload into a typed, sanitized domain event.
pub fn parse_webhook_event(
    event: &str,
    payload: &[u8],
) -> Result<ParsedWebhookEvent, WebhookError> {
    if payload.len() > MAX_WEBHOOK_PAYLOAD_BYTES {
        return Err(WebhookError::PayloadTooLarge);
    }
    match event {
        "pull_request" => parse_pull_request_webhook(payload).map(ParsedWebhookEvent::PullRequest),
        "pull_request_review" => {
            parse_pull_request_review_webhook(payload).map(ParsedWebhookEvent::PullRequestReview)
        }
        "pull_request_review_comment" => parse_pull_request_review_comment_webhook(payload)
            .map(ParsedWebhookEvent::PullRequestReviewComment),
        "check_run" => parse_check_run_webhook(payload).map(ParsedWebhookEvent::CheckRun),
        "check_suite" => parse_check_suite_webhook(payload).map(ParsedWebhookEvent::CheckSuite),
        "ping" => parse_ping_webhook(payload),
        other => Ok(ParsedWebhookEvent::Unsupported {
            event: other.to_owned(),
        }),
    }
}

fn parse_ping_webhook(payload: &[u8]) -> Result<ParsedWebhookEvent, WebhookError> {
    #[derive(Deserialize)]
    struct PingPayload {
        #[serde(default)]
        repository: Option<RawRepo>,
    }
    let parsed: PingPayload = serde_json::from_slice(payload)
        .map_err(|_| WebhookError::MalformedPayload("invalid ping JSON".into()))?;
    let repo_full_name = parsed
        .repository
        .and_then(|r| ValidatedRepoSlug::parse_full_name(&r.full_name).ok())
        .map(|slug| slug.full_name());
    Ok(ParsedWebhookEvent::Ping { repo_full_name })
}

fn parse_pull_request_webhook(payload: &[u8]) -> Result<ParsedPullRequestEvent, WebhookError> {
    #[derive(Deserialize)]
    struct PullRequestWebhook {
        repository: RawRepo,
        pull_request: RawPullRequest,
    }
    let parsed: PullRequestWebhook = serde_json::from_slice(payload)
        .map_err(|_| WebhookError::MalformedPayload("invalid pull_request payload".into()))?;
    let slug = ValidatedRepoSlug::parse_full_name(&parsed.repository.full_name)
        .map_err(|_| WebhookError::MalformedPayload("invalid repository.full_name".into()))?;
    normalize_pull_request(slug.full_name(), parsed.pull_request)
}

fn parse_pull_request_review_webhook(
    payload: &[u8],
) -> Result<ParsedPullRequestReviewEvent, WebhookError> {
    #[derive(Deserialize)]
    struct ReviewWebhook {
        repository: RawRepo,
        pull_request: RawPrRef,
        review: RawReview,
    }
    let parsed: ReviewWebhook = serde_json::from_slice(payload).map_err(|_| {
        WebhookError::MalformedPayload("invalid pull_request_review payload".into())
    })?;
    let slug = ValidatedRepoSlug::parse_full_name(&parsed.repository.full_name)
        .map_err(|_| WebhookError::MalformedPayload("invalid repository.full_name".into()))?;
    if parsed.pull_request.number == 0 {
        return Err(WebhookError::MalformedPayload(
            "pull_request.number must be positive".into(),
        ));
    }
    Ok(ParsedPullRequestReviewEvent {
        repo_full_name: slug.full_name(),
        pr_number: parsed.pull_request.number,
        head_sha: parsed
            .pull_request
            .head
            .and_then(|h| sanitize_git_sha_optional(&h.sha)),
        review: normalize_review(parsed.review),
    })
}

fn parse_pull_request_review_comment_webhook(
    payload: &[u8],
) -> Result<ParsedPullRequestReviewCommentEvent, WebhookError> {
    #[derive(Deserialize)]
    struct CommentWebhook {
        repository: RawRepo,
        pull_request: RawPrRef,
        comment: RawReviewComment,
    }
    let parsed: CommentWebhook = serde_json::from_slice(payload).map_err(|_| {
        WebhookError::MalformedPayload("invalid pull_request_review_comment payload".into())
    })?;
    let slug = ValidatedRepoSlug::parse_full_name(&parsed.repository.full_name)
        .map_err(|_| WebhookError::MalformedPayload("invalid repository.full_name".into()))?;
    if parsed.pull_request.number == 0 {
        return Err(WebhookError::MalformedPayload(
            "pull_request.number must be positive".into(),
        ));
    }
    Ok(ParsedPullRequestReviewCommentEvent {
        repo_full_name: slug.full_name(),
        pr_number: parsed.pull_request.number,
        head_sha: parsed
            .pull_request
            .head
            .and_then(|h| sanitize_git_sha_optional(&h.sha)),
        comment: normalize_review_comment(parsed.comment),
    })
}

fn parse_check_run_webhook(payload: &[u8]) -> Result<ParsedCheckRunEvent, WebhookError> {
    #[derive(Deserialize)]
    struct CheckRunWebhook {
        repository: RawRepo,
        check_run: RawCheckRun,
    }
    let parsed: CheckRunWebhook = serde_json::from_slice(payload)
        .map_err(|_| WebhookError::MalformedPayload("invalid check_run payload".into()))?;
    let slug = ValidatedRepoSlug::parse_full_name(&parsed.repository.full_name)
        .map_err(|_| WebhookError::MalformedPayload("invalid repository.full_name".into()))?;
    let pr_numbers: Vec<u64> = parsed
        .check_run
        .pull_requests
        .iter()
        .map(|p| p.number)
        .filter(|&n| n > 0)
        .take(20)
        .collect();
    let check_run = normalize_check_run(parsed.check_run);
    Ok(ParsedCheckRunEvent {
        repo_full_name: slug.full_name(),
        pr_numbers,
        check_run,
    })
}

fn parse_check_suite_webhook(payload: &[u8]) -> Result<ParsedCheckSuiteEvent, WebhookError> {
    #[derive(Deserialize)]
    struct RawCheckSuite {
        id: u64,
        #[serde(default)]
        status: String,
        #[serde(default)]
        conclusion: Option<String>,
        #[serde(default)]
        head_sha: String,
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        updated_at: Option<String>,
        #[serde(default)]
        app: Option<RawAppRef>,
        #[serde(default)]
        pull_requests: Vec<RawPrRef>,
    }
    #[derive(Deserialize)]
    struct CheckSuiteWebhook {
        repository: RawRepo,
        check_suite: RawCheckSuite,
    }
    let parsed: CheckSuiteWebhook = serde_json::from_slice(payload)
        .map_err(|_| WebhookError::MalformedPayload("invalid check_suite payload".into()))?;
    let slug = ValidatedRepoSlug::parse_full_name(&parsed.repository.full_name)
        .map_err(|_| WebhookError::MalformedPayload("invalid repository.full_name".into()))?;
    let pr_numbers: Vec<u64> = parsed
        .check_suite
        .pull_requests
        .iter()
        .map(|p| p.number)
        .filter(|&n| n > 0)
        .take(20)
        .collect();
    let name = parsed
        .check_suite
        .app
        .and_then(|a| a.name)
        .map(|n| sanitize_github_meta(&format!("Check suite ({n})"), MAX_CHECK_NAME_BYTES))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Check suite".to_owned());
    let check_run = GitHubCheckRunView {
        check_run_id: parsed.check_suite.id,
        name,
        workflow_name: None,
        status: parse_check_status(&parsed.check_suite.status),
        conclusion: parsed
            .check_suite
            .conclusion
            .as_deref()
            .map(parse_check_conclusion),
        head_sha: sanitize_git_sha_optional(&parsed.check_suite.head_sha).unwrap_or_default(),
        details_url: parsed
            .check_suite
            .url
            .as_deref()
            .and_then(validate_html_url),
        title: None,
        summary: None,
        annotations: Vec::new(),
        completed_at: parsed
            .check_suite
            .updated_at
            .map(|t| sanitize_github_meta(&t, 64)),
    };
    Ok(ParsedCheckSuiteEvent {
        repo_full_name: slug.full_name(),
        pr_numbers,
        check_run,
    })
}

/// Validates a canonical hyphenated UUID (`8-4-4-4-12` hex characters).
fn is_valid_uuid(raw: &str) -> bool {
    let b = raw.as_bytes();
    if b.len() != 36 {
        return false;
    }
    for (idx, &ch) in b.iter().enumerate() {
        if matches!(idx, 8 | 13 | 18 | 23) {
            if ch != b'-' {
                return false;
            }
        } else if !ch.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

fn decode_hex_32(hex: &str) -> Option<[u8; 32]> {
    let bytes = hex.as_bytes();
    if bytes.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        let hi = hex_nibble(bytes[2 * i])?;
        let lo = hex_nibble(bytes[2 * i + 1])?;
        out[i] = (hi << 4) | lo;
    }
    Some(out)
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
