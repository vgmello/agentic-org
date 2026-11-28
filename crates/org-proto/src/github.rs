//! Wire types and typed calls for GitHub App integration (`TASK-272`): owner-only wrapped App
//! configuration and repository bindings, constant-time signed webhook ingestion for pull
//! requests, reviews, and CI check runs/failures, and project-scoped PR/CI read and sync flows.
//!
//! Strictly excludes chat bridges (`doc-7` #11). Secret fields (private key PEM, webhook secret,
//! webhook signature header) use [`SecretText`] so they are wiped on drop and never appear in
//! `Debug` output.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use zeroize::Zeroizing;

use crate::{Call, Empty};

/// Reserved non-member grant principal used exclusively by the admitted web process when
/// forwarding HMAC-signed GitHub webhooks to an organization's HQ office.
///
/// The office's `authority.rs` admits this principal **only** for [`GitHubWebhookIngest::METHOD`]
/// and refuses every other RPC fail-closed.
pub const WEBHOOK_PRINCIPAL_MEMBER: &str = "__github_webhook__";

/// Maximum accepted raw GitHub webhook payload size in bytes (512 KiB).
///
/// Raw bytes are base64-encoded on the RPC wire so every worst-case webhook request (including
/// HMAC grant and JSON-RPC framing) fits strictly inside [`crate::MAX_FRAME_BYTES`].
pub const GITHUB_WEBHOOK_MAX_BYTES: usize = 512 * 1024;

/// Maximum base64-encoded length for a raw webhook payload: `4 * ceil(512 KiB / 3)`.
pub const GITHUB_WEBHOOK_MAX_BASE64_CHARS: usize = GITHUB_WEBHOOK_MAX_BYTES.div_ceil(3) * 4;

/// Conservative upper bound on JSON-RPC envelope, headers, and HMAC grant bytes around
/// [`GitHubWebhookIngestParams::payload_base64`].
const GITHUB_WEBHOOK_RPC_OVERHEAD_BYTES: usize = 16 * 1024;

const _: () = assert!(
    GITHUB_WEBHOOK_MAX_BASE64_CHARS + GITHUB_WEBHOOK_RPC_OVERHEAD_BYTES < crate::MAX_FRAME_BYTES
);

/// Maximum limit for `github.pr.list`.
pub const GITHUB_PR_LIST_MAX_LIMIT: u32 = 200;

/// Maximum UTF-8 bytes retained for `body_excerpt` in `github.pr.list` summary items.
///
/// Full sanitized excerpts remain available via `github.pr.inspect`.
pub const GITHUB_PR_LIST_BODY_EXCERPT_MAX_BYTES: usize = 512;

/// Serialized JSON byte ceiling for `GitHubPrListResult` payloads (768 KiB), keeping every
/// `github.pr.list` RPC response frame strictly below [`crate::MAX_FRAME_BYTES`].
pub const GITHUB_PR_LIST_MAX_RESPONSE_BYTES: usize = 768 * 1024;

const _: () = assert!(GITHUB_PR_LIST_MAX_RESPONSE_BYTES < crate::MAX_FRAME_BYTES);

/// Every GitHub App integration RPC method served by an office.
pub const GITHUB_METHODS: [&str; 10] = [
    GitHubAppConfigure::METHOD,
    GitHubAppStatus::METHOD,
    GitHubAppRevoke::METHOD,
    GitHubRepoBind::METHOD,
    GitHubRepoUnbind::METHOD,
    GitHubWebhookIngest::METHOD,
    GitHubPrList::METHOD,
    GitHubPrInspect::METHOD,
    GitHubPrChecks::METHOD,
    GitHubPrSync::METHOD,
];

/// Secret text carried transiently on wire configuration or webhook verification requests.
///
/// Wiped from memory on drop (`Zeroizing<String>`) and redacted in `Debug`.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretText(Zeroizing<String>);

impl SecretText {
    pub fn new(value: impl Into<String>) -> Self {
        Self(Zeroizing::new(value.into()))
    }

    pub fn expose(&self) -> &str {
        self.0.as_str()
    }
}

impl From<String> for SecretText {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for SecretText {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl fmt::Debug for SecretText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretText(<redacted>)")
    }
}

impl Serialize for SecretText {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0.as_str())
    }
}

impl<'de> Deserialize<'de> for SecretText {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Ok(Self(Zeroizing::new(value)))
    }
}

/// Pull request lifecycle state normalized from GitHub payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitHubPrState {
    Open,
    Closed,
    Merged,
}

/// Aggregated CI status computed across a pull request's latest check runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitHubCiOverallStatus {
    Passing,
    Failing,
    Pending,
    Neutral,
    Unknown,
}

/// Execution state of a GitHub check run or check suite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitHubCheckStatus {
    Queued,
    InProgress,
    Completed,
    #[serde(other)]
    Unknown,
}

/// Terminal conclusion of a completed GitHub check run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitHubCheckConclusion {
    Success,
    Failure,
    TimedOut,
    Cancelled,
    ActionRequired,
    Neutral,
    Skipped,
    Stale,
    #[serde(other)]
    Unknown,
}

impl GitHubCheckConclusion {
    /// Whether this conclusion represents a CI failure requiring developer attention.
    pub fn is_failure(self) -> bool {
        matches!(
            self,
            Self::Failure | Self::TimedOut | Self::Cancelled | Self::ActionRequired
        )
    }
}

/// Normalized pull request review state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitHubReviewState {
    Approved,
    ChangesRequested,
    Commented,
    Dismissed,
    Pending,
    #[serde(other)]
    Unknown,
}

/// Non-secret repository binding between an Agentic Org repository and a GitHub `owner/repo`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubRepoBindingView {
    pub repository_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_key: Option<String>,
    pub owner: String,
    pub repo: String,
    pub full_name: String,
    pub installation_id: u64,
    pub bound_at: String,
}

/// Non-secret status view for an office's wrapped GitHub App integration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubAppStatusView {
    pub office_id: String,
    pub configured: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installation_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_base_url: Option<String>,
    pub webhook_configured: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub bindings: Vec<GitHubRepoBindingView>,
}

/// `github.app.configure`: owner-only wrapped GitHub App private key & webhook secret custody.
pub struct GitHubAppConfigure;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubAppConfigureParams {
    pub app_id: u64,
    pub installation_id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_base_url: Option<String>,
    pub private_key_pem: SecretText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webhook_secret: Option<SecretText>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_hint: Option<String>,
}

impl Call for GitHubAppConfigure {
    const METHOD: &'static str = "github.app.configure";
    type Params = GitHubAppConfigureParams;
    type Result = GitHubAppStatusView;
}

/// `github.app.status`: owner-only non-secret status of the office's GitHub App configuration.
pub struct GitHubAppStatus;

impl Call for GitHubAppStatus {
    const METHOD: &'static str = "github.app.status";
    type Params = Empty;
    type Result = GitHubAppStatusView;
}

/// `github.app.revoke`: owner-only durable removal of wrapped GitHub App credentials and cached tokens.
pub struct GitHubAppRevoke;

impl Call for GitHubAppRevoke {
    const METHOD: &'static str = "github.app.revoke";
    type Params = Empty;
    type Result = GitHubAppStatusView;
}

/// `github.repo.bind`: owner-only binding from a registered org repository to a GitHub `owner/repo`.
pub struct GitHubRepoBind;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubRepoBindParams {
    pub repository: String,
    pub owner: String,
    pub repo: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installation_id: Option<u64>,
}

impl Call for GitHubRepoBind {
    const METHOD: &'static str = "github.repo.bind";
    type Params = GitHubRepoBindParams;
    type Result = GitHubRepoBindingView;
}

/// `github.repo.unbind`: owner-only removal of a repository's GitHub binding.
pub struct GitHubRepoUnbind;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubRepoUnbindParams {
    pub repository: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubRepoUnbindResult {
    pub repository_id: String,
    pub unbound: bool,
}

impl Call for GitHubRepoUnbind {
    const METHOD: &'static str = "github.repo.unbind";
    type Params = GitHubRepoUnbindParams;
    type Result = GitHubRepoUnbindResult;
}

/// `github.webhook.ingest`: verifies `X-Hub-Signature-256` against the office's wrapped webhook
/// secret in constant time, deduplicates by `X-GitHub-Delivery`, and indexes PR/review/CI state.
pub struct GitHubWebhookIngest;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubWebhookIngestParams {
    pub event: String,
    pub delivery_id: String,
    pub signature_256: SecretText,
    /// Standard base64-encoded raw HTTP webhook payload bytes (exact bytes used for HMAC-SHA256).
    pub payload_base64: String,
}

impl fmt::Debug for GitHubWebhookIngestParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GitHubWebhookIngestParams")
            .field("event", &self.event)
            .field("delivery_id", &self.delivery_id)
            .field("signature_256", &self.signature_256)
            .field("payload_base64_len", &self.payload_base64.len())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubWebhookIngestResult {
    pub delivery_id: String,
    pub event: String,
    pub accepted: bool,
    pub duplicate: bool,
    pub ignored: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_number: Option<u64>,
    #[serde(default)]
    pub linked_task_ids: Vec<String>,
    #[serde(default)]
    pub linked_review_ids: Vec<String>,
}

impl Call for GitHubWebhookIngest {
    const METHOD: &'static str = "github.webhook.ingest";
    type Params = GitHubWebhookIngestParams;
    type Result = GitHubWebhookIngestResult;
}

/// Sanitized, secret-redacted pull request summary linked to org tasks and `RV-` reviews.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubPrSummaryView {
    pub repository_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_key: Option<String>,
    pub repo_full_name: String,
    pub pr_number: u64,
    pub state: GitHubPrState,
    pub draft: bool,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_excerpt: Option<String>,
    pub html_url: String,
    pub diff_url: String,
    pub head_ref: String,
    pub head_sha: String,
    pub base_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_sha: Option<String>,
    pub author_login: String,
    #[serde(default)]
    pub linked_task_refs: Vec<String>,
    #[serde(default)]
    pub linked_task_ids: Vec<String>,
    #[serde(default)]
    pub linked_review_ids: Vec<String>,
    #[serde(default)]
    pub linked_review_keys: Vec<String>,
    pub ci_status: GitHubCiOverallStatus,
    pub failing_checks_count: u32,
    pub total_checks_count: u32,
    pub review_count: u32,
    pub review_comment_count: u32,
    pub updated_at: String,
}

/// Sanitized, secret-redacted pull request review summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubPrReviewView {
    pub review_id: u64,
    pub state: GitHubReviewState,
    pub author_login: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_excerpt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submitted_at: Option<String>,
}

/// Sanitized, secret-redacted inline pull request review comment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubPrReviewCommentView {
    pub comment_id: u64,
    pub author_login: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    pub body_excerpt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Bounded, sanitized CI check annotation (file/line failure diagnostic).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubCheckAnnotationView {
    pub path: String,
    pub start_line: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
    pub level: String,
    pub message: String,
}

/// Sanitized, secret-redacted GitHub CI check run snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubCheckRunView {
    pub check_run_id: u64,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_name: Option<String>,
    pub status: GitHubCheckStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conclusion: Option<GitHubCheckConclusion>,
    pub head_sha: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default)]
    pub annotations: Vec<GitHubCheckAnnotationView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
}

/// Condensed CI failure analysis item surfaced by `github.pr.checks`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubFailureSummaryView {
    pub check_run_id: u64,
    pub name: String,
    pub conclusion: GitHubCheckConclusion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default)]
    pub annotations: Vec<GitHubCheckAnnotationView>,
}

/// `github.pr.list`: lists ingested PRs for repositories attached to an authorized project.
pub struct GitHubPrList;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubPrListParams {
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<GitHubPrState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubPrListResult {
    pub project_id: String,
    pub pull_requests: Vec<GitHubPrSummaryView>,
    #[serde(default)]
    pub has_more: bool,
}

impl Call for GitHubPrList {
    const METHOD: &'static str = "github.pr.list";
    type Params = GitHubPrListParams;
    type Result = GitHubPrListResult;
}

/// `github.pr.inspect`: inspects one PR with reviews, review comments, CI check runs, task/review
/// links, and a cleanly fenced untrusted-context block.
pub struct GitHubPrInspect;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubPrInspectParams {
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    pub pr_number: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubPrDetailView {
    pub project_id: String,
    pub pr: GitHubPrSummaryView,
    pub reviews: Vec<GitHubPrReviewView>,
    pub review_comments: Vec<GitHubPrReviewCommentView>,
    pub check_runs: Vec<GitHubCheckRunView>,
    pub fenced_untrusted_context: String,
}

impl Call for GitHubPrInspect {
    const METHOD: &'static str = "github.pr.inspect";
    type Params = GitHubPrInspectParams;
    type Result = GitHubPrDetailView;
}

/// `github.pr.checks`: inspects CI check runs and structured failure summaries for a PR.
pub struct GitHubPrChecks;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubPrChecksParams {
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    pub pr_number: u64,
    #[serde(default)]
    pub only_failures: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubPrChecksResult {
    pub project_id: String,
    pub repository_id: String,
    pub pr_number: u64,
    pub head_sha: String,
    pub overall_status: GitHubCiOverallStatus,
    #[serde(default)]
    pub linked_task_ids: Vec<String>,
    #[serde(default)]
    pub linked_review_ids: Vec<String>,
    #[serde(default)]
    pub linked_review_keys: Vec<String>,
    pub check_runs: Vec<GitHubCheckRunView>,
    pub failure_summaries: Vec<GitHubFailureSummaryView>,
}

impl Call for GitHubPrChecks {
    const METHOD: &'static str = "github.pr.checks";
    type Params = GitHubPrChecksParams;
    type Result = GitHubPrChecksResult;
}

/// `github.pr.sync`: refreshes PR metadata, reviews, and CI check runs on demand using a
/// short-lived repository-scoped GitHub App installation token.
pub struct GitHubPrSync;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubPrSyncParams {
    pub project: String,
    pub repository: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_number: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubPrSyncResult {
    pub project_id: String,
    pub repository_id: String,
    pub synced_prs: Vec<u64>,
    pub synced_at: String,
}

impl Call for GitHubPrSync {
    const METHOD: &'static str = "github.pr.sync";
    type Params = GitHubPrSyncParams;
    type Result = GitHubPrSyncResult;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_text_is_redacted_in_debug() {
        let secret = SecretText::new("-----BEGIN RSA PRIVATE KEY-----\nSECRET_MATERIAL");
        let dbg = format!("{secret:?}");
        assert_eq!(dbg, "SecretText(<redacted>)");
        assert!(!dbg.contains("SECRET_MATERIAL"));
    }

    #[test]
    fn configure_and_webhook_params_debug_never_leak_secrets() {
        let cfg = GitHubAppConfigureParams {
            app_id: 12345,
            installation_id: 67890,
            api_base_url: None,
            private_key_pem: SecretText::new("PRIVATE_KEY_SENTINEL"),
            webhook_secret: Some(SecretText::new("WEBHOOK_SECRET_SENTINEL")),
            account_hint: Some("acme".into()),
        };
        let dbg = format!("{cfg:?}");
        assert!(!dbg.contains("PRIVATE_KEY_SENTINEL"));
        assert!(!dbg.contains("WEBHOOK_SECRET_SENTINEL"));

        let hook = GitHubWebhookIngestParams {
            event: "pull_request".into(),
            delivery_id: "11111111-2222-3333-4444-555555555555".into(),
            signature_256: SecretText::new("sha256=SIG_SENTINEL"),
            payload_base64: "UEFZTE9BRF9TRU5USU5FTA==".into(),
        };
        let hdbg = format!("{hook:?}");
        assert!(!hdbg.contains("SIG_SENTINEL"));
        assert!(!hdbg.contains("UEFZTE9BRF9TRU5USU5FTA=="));
    }

    #[test]
    fn max_size_webhook_rpc_frame_with_grant_fits_inside_max_frame_bytes() {
        assert_eq!(GITHUB_WEBHOOK_MAX_BYTES, 512 * 1024);
        assert_eq!(
            GITHUB_WEBHOOK_MAX_BYTES.div_ceil(3) * 4,
            GITHUB_WEBHOOK_MAX_BASE64_CHARS
        );
        const {
            assert!(
                GITHUB_WEBHOOK_MAX_BASE64_CHARS + GITHUB_WEBHOOK_RPC_OVERHEAD_BYTES
                    < crate::MAX_FRAME_BYTES
            );
        }

        let params = GitHubWebhookIngestParams {
            event: "pull_request_review_comment".into(),
            delivery_id: "11111111-2222-4333-8444-555555555555".into(),
            signature_256: SecretText::new(format!("sha256={}", "a".repeat(64))),
            payload_base64: "A".repeat(GITHUB_WEBHOOK_MAX_BASE64_CHARS),
        };
        let grant = crate::Grant {
            claims: crate::GrantClaims {
                account: "github-webhook".into(),
                member: WEBHOOK_PRINCIPAL_MEMBER.into(),
                org_id: "0193a5c0-0000-7000-8000-000000000001".into(),
                office_id: "0193a5c0-0000-7000-8000-000000000002".into(),
                channel_id: "0193a5c0-0000-7000-8000-000000000003".into(),
                session_id: "01000000000000000000000001".into(),
                exp: 1_794_312_060_000,
            },
            mac: crate::GrantMac::from_bytes([0xab; crate::GRANT_MAC_BYTES]),
        };
        let req = crate::Request::new::<GitHubWebhookIngest>(u64::MAX / 2, &params)
            .unwrap()
            .with_principal(&crate::Principal { grant })
            .unwrap();
        let encoded = serde_json::to_vec(&req).unwrap();
        assert!(
            encoded.len() < crate::MAX_FRAME_BYTES,
            "encoded webhook RPC frame {} >= MAX_FRAME_BYTES {}",
            encoded.len(),
            crate::MAX_FRAME_BYTES
        );
    }
}
