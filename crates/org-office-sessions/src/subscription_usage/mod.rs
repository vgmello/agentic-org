//! Authenticated provider subscription model usage collection (`TASK-79`).
//!
//! Keeps all provider/user usage-API credentials private to office storage via [`CredentialVault`]
//! (outside shared organization Git), binds credentials to explicit HTTPS destination origins,
//! isolates `curl` (`--disable`, `--noproxy '*'`, `--globoff`, cleared env, pinned `--resolve`),
//! enforces a single shared deadline inside the 5s office bridge limit, fences concurrent writes
//! by monotonic attempt generation, and delegates external collection behind consumer-owned ports
//! ([`SubscriptionUsageCollector`] and [`SubscriptionUsageTransport`]).

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use org_office::accounting::canonical_provider_namespace;
use org_proto::costs::{SubscriptionModelUsageView, SubscriptionUsageAuthSource};
use zeroize::Zeroizing;

use crate::redact::redact_text;
use endpoint::{
    classify_http_and_parse, default_provider_origin, is_valid_bearer_token,
    resolve_provider_endpoint, transport_error_msg,
};

mod endpoint;
mod parse;
mod rollup;
mod transport;

pub use endpoint::{validate_usage_endpoint_url, ValidatedEndpoint};
pub(crate) use rollup::{
    collect_subscription_usage, list_subscription_usage, revoke_subscription_usage_credential,
    set_subscription_usage_credential,
};
pub use transport::SystemHttpsUsageTransport;

const MAX_USAGE_SECRET_BYTES: usize = 4096;
const MAX_USAGE_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_COLLECT_RESPONSE_BYTES: usize = 512 * 1024;
const MAX_ACCOUNT_HINT_BYTES: usize = 256;
const MAX_CONCURRENT_COLLECTORS: usize = 8;
const MAX_CONCURRENT_DNS_LOOKUPS: usize = 16;
const TOTAL_COLLECTION_BUDGET: Duration = Duration::from_millis(3_000);
const MAX_DNS_TIMEOUT: Duration = Duration::from_millis(1_000);
const DEFAULT_ANTHROPIC_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const DEFAULT_OPENAI_USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";

/// Raw HTTP JSON response returned by a [`SubscriptionUsageTransport`].
#[derive(Clone, PartialEq, Eq)]
pub struct HttpUsageResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

impl fmt::Debug for HttpUsageResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpUsageResponse")
            .field("status", &self.status)
            .field("body_len", &self.body.len())
            .finish()
    }
}

/// Low-level transport failure before receiving an HTTP response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageTransportError {
    InvalidEndpoint(String),
    InvalidHeader(String),
    DeadlineExceeded,
    Unavailable(String),
}

/// Consumer-owned HTTPS GET transport port for provider subscription usage APIs (`AGENTS.md` Rule 1).
pub trait SubscriptionUsageTransport: Send + Sync {
    fn get_json(
        &self,
        url: &str,
        headers: &[(String, String)],
        deadline: Instant,
    ) -> Result<HttpUsageResponse, UsageTransportError>;
}

/// Authenticated collection input passed to [`SubscriptionUsageCollector`].
pub struct SubscriptionUsageCollectRequest {
    pub connection_id: String,
    pub provider_type: String,
    pub subscription_identity: Option<String>,
    pub endpoint_url: Option<String>,
    pub auth_source: SubscriptionUsageAuthSource,
    pub credential: Zeroizing<Vec<u8>>,
    pub credential_deadline: Option<Instant>,
    pub time_zone: String,
    pub collected_at: String,
    pub now_ms: u64,
    pub deadline: Instant,
}

/// Structured outcome of collecting subscription model usage for one provider connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubscriptionUsageCollectOutcome {
    Collected {
        models: Vec<SubscriptionModelUsageView>,
        message: Option<String>,
    },
    AuthRequired {
        issue_code: String,
        message: String,
    },
    Unavailable {
        issue_code: String,
        message: String,
    },
    Unsupported {
        issue_code: String,
        message: String,
    },
    /// Batch deadline expired before provider response completed; do not overwrite stored snapshot.
    Deferred,
}

/// Consumer-owned collector port for provider subscription model usage (`TASK-79`).
pub trait SubscriptionUsageCollector: Send + Sync {
    fn collect(&self, req: &SubscriptionUsageCollectRequest) -> SubscriptionUsageCollectOutcome;
}

/// Production provider subscription usage collector backed by a pluggable [`SubscriptionUsageTransport`].
pub struct ProviderSubscriptionUsageCollector {
    transport: Arc<dyn SubscriptionUsageTransport>,
}

impl ProviderSubscriptionUsageCollector {
    pub fn production() -> Self {
        Self::with_transport(Arc::new(SystemHttpsUsageTransport))
    }

    pub fn with_transport(transport: Arc<dyn SubscriptionUsageTransport>) -> Self {
        Self { transport }
    }
}

impl SubscriptionUsageCollector for ProviderSubscriptionUsageCollector {
    fn collect(&self, req: &SubscriptionUsageCollectRequest) -> SubscriptionUsageCollectOutcome {
        let provider = canonical_provider_namespace(&req.provider_type);
        let token = match std::str::from_utf8(&req.credential) {
            Ok(s) if is_valid_bearer_token(s.trim()) => s.trim(),
            _ => {
                return SubscriptionUsageCollectOutcome::AuthRequired {
                    issue_code: "INVALID_CREDENTIAL_FORMAT".into(),
                    message: "Stored credential cannot be encoded as an HTTPS bearer token; re-authenticate.".into(),
                };
            }
        };
        let (url, extra_headers) =
            match resolve_provider_endpoint(&provider, req.endpoint_url.as_deref()) {
                Ok(pair) => pair,
                Err(outcome) => return outcome,
            };
        let ep = match validate_usage_endpoint_url(&url) {
            Ok(ep) => ep,
            Err(err) => {
                return SubscriptionUsageCollectOutcome::Unavailable {
                    issue_code: "INVALID_ENDPOINT".into(),
                    message: redact_text(&transport_error_msg(err), &[token]),
                };
            }
        };
        if req.auth_source == SubscriptionUsageAuthSource::EnrolledSubscriptionCredential
            && default_provider_origin(&provider) != Some(ep.origin().as_str())
        {
            return SubscriptionUsageCollectOutcome::AuthRequired {
                issue_code: "ENDPOINT_CREDENTIAL_MISMATCH".into(),
                message: "Enrolled subscription credentials are only sent to the official provider origin; configure a dedicated usage credential for custom endpoints.".into(),
            };
        }
        let cred_deadline = req.credential_deadline;
        if cred_deadline.is_some_and(|cd| Instant::now() >= cd) {
            return SubscriptionUsageCollectOutcome::AuthRequired {
                issue_code: "CREDENTIAL_EXPIRED".into(),
                message: "Subscription usage credential expired before dispatch; re-authenticate."
                    .into(),
            };
        }
        let effective_deadline = cred_deadline.map_or(req.deadline, |cd| req.deadline.min(cd));
        let mut headers = vec![
            ("Authorization".to_owned(), format!("Bearer {token}")),
            ("Accept".to_owned(), "application/json".to_owned()),
        ];
        headers.extend(extra_headers);
        let resp = match self.transport.get_json(&url, &headers, effective_deadline) {
            Ok(r) => r,
            Err(UsageTransportError::InvalidHeader(_)) => {
                return SubscriptionUsageCollectOutcome::AuthRequired {
                    issue_code: "INVALID_CREDENTIAL_FORMAT".into(),
                    message: "Stored credential contains disallowed header characters.".into(),
                };
            }
            Err(UsageTransportError::InvalidEndpoint(msg)) => {
                return SubscriptionUsageCollectOutcome::Unavailable {
                    issue_code: "INVALID_ENDPOINT".into(),
                    message: redact_text(&msg, &[token]),
                };
            }
            Err(UsageTransportError::DeadlineExceeded) => {
                if cred_deadline.is_some_and(|cd| Instant::now() >= cd) {
                    return SubscriptionUsageCollectOutcome::AuthRequired {
                        issue_code: "CREDENTIAL_EXPIRED".into(),
                        message: "Subscription usage credential expired before dispatch; re-authenticate.".into(),
                    };
                }
                return SubscriptionUsageCollectOutcome::Deferred;
            }
            Err(UsageTransportError::Unavailable(msg)) => {
                return SubscriptionUsageCollectOutcome::Unavailable {
                    issue_code: "PROVIDER_UNAVAILABLE".into(),
                    message: redact_text(&msg, &[token]),
                };
            }
        };
        classify_http_and_parse(&provider, resp, token, req.subscription_identity.as_deref())
    }
}
