//! Error types for GitHub App authentication, HTTP transport, and webhook verification.
//!
//! All user-facing diagnostic strings are redacted through `org_redact::redact_text` so private
//! key bytes, installation tokens, App JWTs, and webhook secrets never leak into logs or replies.

use thiserror::Error;

/// Errors raised during GitHub App configuration, JWT signing, token exchange, or API sync.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum GitHubError {
    #[error("invalid GitHub App configuration: {0}")]
    InvalidConfig(String),
    #[error("invalid GitHub App private key: {0}")]
    InvalidPrivateKey(String),
    #[error("GitHub App JWT signing failed: {0}")]
    SigningFailed(String),
    #[error("GitHub API endpoint rejected: {0}")]
    InvalidEndpoint(String),
    #[error("GitHub HTTP request timed out")]
    Timeout,
    #[error("GitHub API rejected authentication (HTTP {0})")]
    AuthRejected(u16),
    #[error("GitHub API rate limit exceeded")]
    RateLimited,
    #[error("GitHub API resource not found")]
    NotFound,
    #[error("GitHub API transport error: {0}")]
    Transport(String),
    #[error("GitHub API returned malformed response: {0}")]
    MalformedResponse(String),
}

/// Errors raised while verifying or parsing an incoming signed GitHub webhook delivery.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum WebhookError {
    #[error("webhook payload exceeds maximum allowed size")]
    PayloadTooLarge,
    #[error("missing required webhook header: {0}")]
    MissingHeader(&'static str),
    #[error("invalid X-GitHub-Delivery UUID")]
    InvalidDeliveryId,
    #[error("invalid X-GitHub-Event header")]
    InvalidEventName,
    #[error("malformed X-Hub-Signature-256 header")]
    MalformedSignature,
    #[error("webhook HMAC-SHA256 signature verification failed")]
    InvalidSignature,
    #[error("webhook secret is not configured")]
    SecretNotConfigured,
    #[error("malformed webhook JSON payload: {0}")]
    MalformedPayload(String),
}
