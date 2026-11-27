//! Short-lived GitHub App installation token exchange and RAM-only TTL cache.
//!
//! Tokens are requested scoped strictly to one repository (`{"repositories": [repo_name]}`),
//! cached only in memory inside `Zeroizing<String>`, refreshed 60 seconds before expiry, and
//! NEVER written to disk or logged.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use org_redact::redact_text;
use serde::Deserialize;
use zeroize::Zeroizing;

use crate::config::ValidatedApiBaseUrl;
use crate::error::GitHubError;
use crate::http::{
    GitHubHttpClient, GitHubHttpRequest, HttpMethod, DEFAULT_HTTP_TIMEOUT,
    MAX_GITHUB_HTTP_RESPONSE_BYTES,
};
use crate::jwt::GitHubAppPrivateKey;

/// Refresh cached installation tokens this many seconds before their `expires_at`.
pub const TOKEN_REFRESH_LEAD_SECS: u64 = 60;

/// Maximum number of per-repository installation tokens kept in RAM.
const MAX_CACHED_TOKENS: usize = 128;

struct CachedToken {
    token: Zeroizing<String>,
    expires_at_epoch_secs: u64,
}

impl fmt::Debug for CachedToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CachedToken")
            .field("token", &"<redacted>")
            .field("expires_at_epoch_secs", &self.expires_at_epoch_secs)
            .finish()
    }
}

/// Manages short-lived repository-scoped GitHub App installation tokens in RAM only.
pub struct InstallationTokenManager {
    http: Arc<dyn GitHubHttpClient>,
    cache: Mutex<HashMap<(u64, String), CachedToken>>,
}

impl fmt::Debug for InstallationTokenManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let count = self.cache.lock().map_or(0, |g| g.len());
        f.debug_struct("InstallationTokenManager")
            .field("cached_tokens", &count)
            .finish_non_exhaustive()
    }
}

impl InstallationTokenManager {
    pub fn new(http: Arc<dyn GitHubHttpClient>) -> Self {
        Self {
            http,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Returns the underlying HTTP client port.
    pub fn http(&self) -> &Arc<dyn GitHubHttpClient> {
        &self.http
    }

    /// Purges every cached installation token from memory (called on configure/revoke).
    pub fn clear(&self) {
        if let Ok(mut guard) = self.cache.lock() {
            guard.clear();
        }
    }

    /// Returns a valid installation token scoped exclusively to `repo_name`, refreshing from
    /// GitHub when absent or within [`TOKEN_REFRESH_LEAD_SECS`] of expiry.
    ///
    /// Never holds the cache mutex across network I/O.
    pub fn get_token(
        &self,
        app_id: u64,
        installation_id: u64,
        api_base: &ValidatedApiBaseUrl,
        private_key: &GitHubAppPrivateKey,
        repo_name: &str,
        now_epoch_secs: u64,
    ) -> Result<Zeroizing<String>, GitHubError> {
        if installation_id == 0 {
            return Err(GitHubError::InvalidConfig(
                "installation_id must be positive".into(),
            ));
        }
        let repo_key = repo_name.trim().to_ascii_lowercase();
        if repo_key.is_empty() {
            return Err(GitHubError::InvalidConfig(
                "repository name for installation token scoping cannot be empty".into(),
            ));
        }

        // Fast path: check RAM cache without holding lock across network call.
        {
            let guard = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(entry) = guard.get(&(installation_id, repo_key.clone())) {
                if now_epoch_secs.saturating_add(TOKEN_REFRESH_LEAD_SECS)
                    < entry.expires_at_epoch_secs
                {
                    return Ok(entry.token.clone());
                }
            }
        }

        let app_jwt = private_key.sign_app_jwt(app_id, now_epoch_secs)?;
        let url = api_base.endpoint(&format!(
            "/app/installations/{installation_id}/access_tokens"
        ))?;
        let payload = serde_json::json!({ "repositories": [repo_name.trim()] }).to_string();
        let req = GitHubHttpRequest {
            method: HttpMethod::Post,
            url,
            bearer_token: app_jwt.clone(),
            json_body: Some(payload),
            timeout: DEFAULT_HTTP_TIMEOUT,
        };
        let resp = self.http.send(&req)?;
        if resp.body.len() > MAX_GITHUB_HTTP_RESPONSE_BYTES {
            return Err(GitHubError::MalformedResponse(
                "installation token response exceeded maximum size".into(),
            ));
        }
        match resp.status {
            200..=299 => {}
            401 | 403 => return Err(GitHubError::AuthRejected(resp.status)),
            404 => return Err(GitHubError::NotFound),
            429 => return Err(GitHubError::RateLimited),
            code => {
                return Err(GitHubError::Transport(redact_text(
                    &format!("GitHub installation token exchange returned HTTP {code}"),
                    &[app_jwt.as_str()],
                )));
            }
        }

        #[derive(Deserialize)]
        struct TokenResponse {
            token: String,
            expires_at: String,
        }
        let parsed: TokenResponse = serde_json::from_slice(&resp.body).map_err(|_| {
            GitHubError::MalformedResponse(
                "installation token response missing required fields".into(),
            )
        })?;
        let token = Zeroizing::new(parsed.token.trim().to_owned());
        if token.is_empty() || token.chars().any(char::is_control) {
            return Err(GitHubError::MalformedResponse(
                "installation token is empty or contains control characters".into(),
            ));
        }
        let expires_at_ms = org_proto::parse_rfc3339_ms(&parsed.expires_at).ok_or_else(|| {
            GitHubError::MalformedResponse(
                "installation token expires_at is not a valid RFC3339 timestamp".into(),
            )
        })?;
        let expires_at_epoch_secs = expires_at_ms / 1000;

        let mut guard = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if guard.len() >= MAX_CACHED_TOKENS {
            guard.retain(|_, v| {
                now_epoch_secs.saturating_add(TOKEN_REFRESH_LEAD_SECS) < v.expires_at_epoch_secs
            });
            if guard.len() >= MAX_CACHED_TOKENS {
                guard.clear();
            }
        }
        guard.insert(
            (installation_id, repo_key),
            CachedToken {
                token: token.clone(),
                expires_at_epoch_secs,
            },
        );
        Ok(token)
    }
}
