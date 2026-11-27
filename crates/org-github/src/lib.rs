//! GitHub App integration library (`TASK-272`): wrapped-key RS256 JWT signing, short-lived
//! per-repository installation token exchange and in-memory TTL caching, SSRF-hardened HTTPS
//! transport behind a consumer-owned [`GitHubHttpClient`] port, constant-time HMAC-SHA256 webhook
//! verification with bounded replay deduplication, and sanitized PR/review/CI parsers.
//!
//! Strictly excludes chat bridges (`doc-7` #11).

#![forbid(unsafe_code)]

pub mod config;
pub mod error;
pub mod http;
pub mod jwt;
pub mod sanitize;
pub mod sync;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
#[cfg(test)]
mod tests;
pub mod token;
pub mod webhook;

pub use config::{
    validate_html_url, GitHubAppConfig, ValidatedApiBaseUrl, ValidatedRepoSlug,
    DEFAULT_GITHUB_API_BASE_URL,
};
pub use error::{GitHubError, WebhookError};
pub use http::{
    GitHubHttpClient, GitHubHttpRequest, GitHubHttpResponse, HttpMethod, SystemGitHubHttpClient,
    DEFAULT_HTTP_TIMEOUT, MAX_GITHUB_HTTP_RESPONSE_BYTES,
};
pub use jwt::{verify_app_jwt_rs256, GitHubAppPrivateKey, MAX_PRIVATE_KEY_PEM_BYTES};
pub use sanitize::{
    compute_ci_overall_status, extract_task_refs, format_fenced_pr_context, sanitize_github_body,
    sanitize_github_meta,
};
pub use sync::{GitHubSyncClient, SyncedPullRequest};
pub use token::InstallationTokenManager;
pub use webhook::{
    parse_webhook_event, verify_webhook_headers_and_signature, DeliveryDeduplicator,
    ParsedCheckRunEvent, ParsedCheckSuiteEvent, ParsedPullRequestEvent,
    ParsedPullRequestReviewCommentEvent, ParsedPullRequestReviewEvent, ParsedWebhookEvent,
    MAX_DEDUP_DELIVERIES, MAX_WEBHOOK_PAYLOAD_BYTES,
};
