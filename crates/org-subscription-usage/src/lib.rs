//! Authenticated provider subscription model usage collection (`TASK-79`, `TASK-255`).
//!
//! Keeps all provider/user usage-API credentials private to office storage behind the
//! consumer-owned [`UsageCredentialLookup`] port (outside shared organization Git), binds
//! credentials to explicit HTTPS destination origins, isolates `curl` (`--disable`, `--noproxy '*'`,
//! `--globoff`, cleared env, pinned `--resolve`), enforces a single shared deadline inside the 5s
//! office bridge limit, fences concurrent writes by monotonic attempt generation, and delegates
//! external collection behind consumer-owned ports ([`SubscriptionUsageCollector`] and
//! [`SubscriptionUsageTransport`]).

#![forbid(unsafe_code)]

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use org_office::accounting::canonical_provider_namespace;
use org_proto::costs::{
    SubscriptionModelUsageView, SubscriptionUsageAuthSource, SubscriptionUsageCredentialView,
};
use org_proto::sessions::ProviderCredentialView;
use org_redact::redact_text;
use zeroize::{Zeroize as _, Zeroizing};

mod endpoint;
mod oauth;
mod parse;
mod rollup;
#[cfg(test)]
mod tests;
mod transport;

use endpoint::{
    classify_http_and_parse, default_provider_origin, is_valid_bearer_token,
    resolve_provider_endpoint, transport_error_msg,
};

pub use endpoint::{validate_usage_endpoint_url, ValidatedEndpoint};
pub use oauth::{
    parse_oauth_envelope, validate_oauth_secret_token, ParsedOAuthEnvelope,
    MAX_VAULT_OAUTH_CRED_BYTES,
};
pub use rollup::{
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

/// Unwrapped credential secret and optional expiry returned by [`UsageCredentialLookup`].
pub struct UnwrappedUsageCredential {
    pub secret: Zeroizing<Vec<u8>>,
    pub expires_at: Option<String>,
}

/// Errors raised by a [`UsageCredentialLookup`] implementation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageCredentialError {
    MissingSecretsKey,
    InvalidInput(String),
    NotAuthorized(String),
    Conflict(String),
    NotFound(String),
    Expired(String),
    Crypto(String),
    Io(String),
}

impl fmt::Display for UsageCredentialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSecretsKey => {
                f.write_str("office secrets key unavailable; cannot unwrap provider credentials")
            }
            Self::InvalidInput(msg)
            | Self::NotAuthorized(msg)
            | Self::Conflict(msg)
            | Self::NotFound(msg)
            | Self::Expired(msg)
            | Self::Crypto(msg)
            | Self::Io(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for UsageCredentialError {}

/// Consumer-owned credential custody port used by subscription usage orchestration
/// (`AGENTS.md` Rule 1, `TASK-255`).
pub trait UsageCredentialLookup: Send + Sync {
    fn usage_status(
        &self,
        connection_id: &str,
    ) -> Result<SubscriptionUsageCredentialView, UsageCredentialError>;

    fn enrolled_status(
        &self,
        connection_id: &str,
    ) -> Result<ProviderCredentialView, UsageCredentialError>;

    fn store_wrapped_usage(
        &self,
        connection_id: &str,
        provider: &str,
        bound_origin: &str,
        secret: &[u8],
        expires_at: Option<String>,
        account_hint: Option<String>,
    ) -> Result<SubscriptionUsageCredentialView, UsageCredentialError>;

    fn revoke_usage(
        &self,
        connection_id: &str,
    ) -> Result<SubscriptionUsageCredentialView, UsageCredentialError>;

    fn load_unwrapped_usage(
        &self,
        connection_id: &str,
        expected_origin: &str,
    ) -> Result<Option<UnwrappedUsageCredential>, UsageCredentialError>;

    fn load_unwrapped_enrolled(
        &self,
        connection_id: &str,
    ) -> Result<Option<UnwrappedUsageCredential>, UsageCredentialError>;
}

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

fn extract_usage_bearer_token(
    auth_source: SubscriptionUsageAuthSource,
    raw: &[u8],
) -> Option<Zeroizing<String>> {
    let candidate = if auth_source == SubscriptionUsageAuthSource::EnrolledSubscriptionCredential
        || raw.iter().copied().find(|b| !b.is_ascii_whitespace()) == Some(b'{')
    {
        parse_oauth_envelope(raw).ok()?.access_token
    } else {
        Zeroizing::new(std::str::from_utf8(raw).ok()?.trim().to_owned())
    };
    is_valid_bearer_token(candidate.as_str()).then_some(candidate)
}

/// RAII header list that scrubs every header name and value from heap memory on drop
/// (including across unwinds) so live bearer tokens never linger unzeroized.
pub(crate) struct ZeroizingHeaders(Vec<(String, String)>);

impl ZeroizingHeaders {
    pub(crate) fn new(bearer_token: &str, extra_headers: Vec<(String, String)>) -> Self {
        let mut auth = String::with_capacity("Bearer ".len() + bearer_token.len());
        auth.push_str("Bearer ");
        auth.push_str(bearer_token);
        let mut headers = Vec::with_capacity(2 + extra_headers.len());
        headers.push(("Authorization".to_owned(), auth));
        headers.push(("Accept".to_owned(), "application/json".to_owned()));
        headers.extend(extra_headers);
        Self(headers)
    }

    pub(crate) fn as_slice(&self) -> &[(String, String)] {
        &self.0
    }
}

impl Drop for ZeroizingHeaders {
    fn drop(&mut self) {
        for (name, value) in &mut self.0 {
            name.zeroize();
            value.zeroize();
        }
    }
}

impl SubscriptionUsageCollector for ProviderSubscriptionUsageCollector {
    fn collect(&self, req: &SubscriptionUsageCollectRequest) -> SubscriptionUsageCollectOutcome {
        let provider = canonical_provider_namespace(&req.provider_type);
        let Some(token_owned) = extract_usage_bearer_token(req.auth_source, &req.credential) else {
            return SubscriptionUsageCollectOutcome::AuthRequired {
                issue_code: "INVALID_CREDENTIAL_FORMAT".into(),
                message:
                    "Stored credential cannot be encoded as an HTTPS bearer token; re-authenticate."
                        .into(),
            };
        };
        let token = token_owned.as_str();
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
        let headers = ZeroizingHeaders::new(token, extra_headers);
        let transport_result =
            self.transport
                .get_json(&url, headers.as_slice(), effective_deadline);
        drop(headers);
        let resp = match transport_result {
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
