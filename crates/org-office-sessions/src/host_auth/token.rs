//! Vault-native OAuth credential parsing, serialization, validation, and expiry helpers
//! (`TASK-132`). Supports raw OAuth access tokens and structured JSON envelopes
//! (`access_token`/`accessToken`, optional `refresh_token`/`refreshToken`, optional
//! `expires_at_ms`/`expiresAt`, or a `{"claudeAiOauth": {...}}` wrapper).

use std::fmt;

use serde::Serialize;
use zeroize::Zeroizing;

/// Upper bound on a vault-stored OAuth credential payload before parsing.
pub(crate) const MAX_VAULT_OAUTH_CRED_BYTES: usize = 64 * 1024;
/// Proactive refresh lead time before a vault OAuth access token expires (5 minutes).
pub(crate) const OAUTH_REFRESH_LEAD_MS: u64 = 5 * 60 * 1000;

/// Returns `true` when an OAuth token is expired or within [`OAUTH_REFRESH_LEAD_MS`] of expiry.
pub(crate) fn is_oauth_expiring(expires_at_ms: Option<u64>, now_ms: u64) -> bool {
    expires_at_ms.is_some_and(|exp| now_ms.saturating_add(OAUTH_REFRESH_LEAD_MS) >= exp)
}

/// Non-reversible 16-hex SHA-256 digest of `bytes`.
pub(crate) fn hex_short_sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = ring::digest::digest(&ring::digest::SHA256, bytes);
    let mut out = String::with_capacity(16);
    for &b in &digest.as_ref()[..8] {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// Non-reversible 16-hex SHA-256 fingerprint of an OAuth access token (never plaintext).
pub(crate) fn oauth_token_fingerprint(token: &str) -> String {
    hex_short_sha256(token.as_bytes())
}

/// Structured per-connection OAuth credential stored inside the office credential vault.
/// Secret fields are wrapped in [`Zeroizing`] and never formatted by [`fmt::Debug`].
#[derive(Clone)]
pub(crate) struct VaultOAuthCredential {
    pub access_token: Zeroizing<String>,
    pub refresh_token: Option<Zeroizing<String>>,
    pub expires_at_ms: Option<u64>,
}

impl fmt::Debug for VaultOAuthCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VaultOAuthCredential")
            .field("token_fp", &self.fingerprint())
            .field("has_refresh_token", &self.refresh_token.is_some())
            .field("expires_at_ms", &self.expires_at_ms)
            .finish()
    }
}

#[derive(Serialize)]
struct VaultOAuthEnvelopeWire<'a> {
    access_token: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh_token: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_at_ms: Option<u64>,
}

impl VaultOAuthCredential {
    pub(crate) fn fingerprint(&self) -> String {
        oauth_token_fingerprint(self.access_token.as_str())
    }

    /// Encodes the credential as canonical flat JSON for `CredentialVault::store_wrapped`.
    pub(crate) fn to_vault_json(&self) -> Result<Zeroizing<Vec<u8>>, VaultAuthError> {
        let wire = VaultOAuthEnvelopeWire {
            access_token: self.access_token.as_str(),
            refresh_token: self.refresh_token.as_deref().map(String::as_str),
            expires_at_ms: self.expires_at_ms,
        };
        serde_json::to_vec(&wire)
            .map(Zeroizing::new)
            .map_err(|_| VaultAuthError::VaultPersistFailed)
    }

    pub(crate) fn validate(&self) -> Result<(), VaultAuthError> {
        validate_secret_token(self.access_token.as_str())?;
        if let Some(ref rt) = self.refresh_token {
            validate_secret_token(rt.as_str())?;
        }
        Ok(())
    }
}

/// Fail-closed errors raised when resolving, validating, or refreshing a vault OAuth credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VaultAuthError {
    MissingCredential,
    MalformedCredential(&'static str),
    ReauthRequired {
        reason: &'static str,
        detail: String,
    },
    VaultPersistFailed,
}

impl VaultAuthError {
    pub(crate) fn issue_code(&self) -> &'static str {
        match self {
            Self::MissingCredential => "vault_credential_missing",
            Self::MalformedCredential(_) => "vault_credential_malformed",
            Self::ReauthRequired { reason, .. } => reason,
            Self::VaultPersistFailed => "vault_oauth_persist_failed",
        }
    }

    pub(crate) fn actionable_message(&self, connection_key: &str) -> String {
        let guide = "Enroll or rotate this connection's vault credential via `agentic-org` provider connection enrollment.";
        match self {
            Self::MissingCredential => format!(
                "provider connection `{connection_key}` has no credential enrolled in the office vault. {guide}"
            ),
            Self::MalformedCredential(why) => format!(
                "provider connection `{connection_key}` has an invalid vault credential ({why}). {guide}"
            ),
            Self::ReauthRequired { detail, .. } => format!(
                "provider connection `{connection_key}` OAuth credential requires re-authentication ({detail}). {guide}"
            ),
            Self::VaultPersistFailed => format!(
                "provider connection `{connection_key}` refreshed OAuth credential could not be persisted to the office vault. {guide}"
            ),
        }
    }
}

/// Parses a vault-unwrapped `subscription_cli` secret (raw token or JSON envelope).
pub(crate) fn parse_vault_oauth_credential(
    raw: &[u8],
) -> Result<VaultOAuthCredential, VaultAuthError> {
    if raw.len() > MAX_VAULT_OAUTH_CRED_BYTES {
        return Err(VaultAuthError::MalformedCredential(
            "vault credential exceeds maximum size",
        ));
    }
    let text = std::str::from_utf8(raw)
        .map_err(|_| VaultAuthError::MalformedCredential("vault credential is not valid UTF-8"))?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(VaultAuthError::MalformedCredential(
            "vault credential is empty",
        ));
    }
    if trimmed.starts_with('{') {
        return parse_vault_oauth_json(trimmed);
    }
    validate_secret_token(trimmed)?;
    Ok(VaultOAuthCredential {
        access_token: Zeroizing::new(trimmed.to_owned()),
        refresh_token: None,
        expires_at_ms: None,
    })
}

/// Extracts vault secret values (raw payload plus parsed OAuth tokens) for transcript redaction.
pub(crate) fn collect_vault_redaction_secrets(raw: &[u8], out: &mut Vec<Zeroizing<String>>) {
    let Ok(trimmed) = std::str::from_utf8(raw).map(str::trim) else {
        return;
    };
    if trimmed.is_empty() {
        return;
    }
    out.push(Zeroizing::new(trimmed.to_owned()));
    if let Ok(cred) = parse_vault_oauth_credential(raw) {
        if cred.access_token.as_str() != trimmed {
            out.push(cred.access_token);
        }
        if let Some(rt) = cred.refresh_token {
            out.push(rt);
        }
    }
}

fn parse_vault_oauth_json(trimmed: &str) -> Result<VaultOAuthCredential, VaultAuthError> {
    let val: serde_json::Value = serde_json::from_str(trimmed)
        .map_err(|_| VaultAuthError::MalformedCredential("malformed JSON OAuth envelope"))?;
    let root = val.as_object().ok_or(VaultAuthError::MalformedCredential(
        "OAuth envelope must be a JSON object",
    ))?;
    let obj = match root.get("claudeAiOauth") {
        Some(inner) => inner
            .as_object()
            .ok_or(VaultAuthError::MalformedCredential(
                "claudeAiOauth must be a JSON object",
            ))?,
        None => root,
    };
    let access = extract_optional_str(obj, "access_token", "accessToken")?.ok_or(
        VaultAuthError::MalformedCredential("OAuth envelope is missing access_token"),
    )?;
    validate_secret_token(access)?;
    let refresh_token = match extract_optional_str(obj, "refresh_token", "refreshToken")? {
        Some(rt) => {
            validate_secret_token(rt)?;
            Some(Zeroizing::new(rt.to_owned()))
        }
        None => None,
    };
    let expires_at_ms = extract_optional_u64(obj, "expires_at_ms", "expiresAt")?;
    Ok(VaultOAuthCredential {
        access_token: Zeroizing::new(access.to_owned()),
        refresh_token,
        expires_at_ms,
    })
}

fn extract_optional_str<'a>(
    obj: &'a serde_json::Map<String, serde_json::Value>,
    primary: &str,
    alias: &str,
) -> Result<Option<&'a str>, VaultAuthError> {
    let Some(val) = obj.get(primary).or_else(|| obj.get(alias)) else {
        return Ok(None);
    };
    if val.is_null() {
        return Ok(None);
    }
    let s = val.as_str().ok_or(VaultAuthError::MalformedCredential(
        "OAuth token field must be a string",
    ))?;
    let trimmed = s.trim();
    Ok((!trimmed.is_empty()).then_some(trimmed))
}

fn extract_optional_u64(
    obj: &serde_json::Map<String, serde_json::Value>,
    primary: &str,
    alias: &str,
) -> Result<Option<u64>, VaultAuthError> {
    let Some(val) = obj.get(primary).or_else(|| obj.get(alias)) else {
        return Ok(None);
    };
    if val.is_null() {
        return Ok(None);
    }
    val.as_u64()
        .map(Some)
        .ok_or(VaultAuthError::MalformedCredential(
            "OAuth expires_at_ms must be an unsigned integer",
        ))
}

fn validate_secret_token(token: &str) -> Result<(), VaultAuthError> {
    if token.trim().is_empty() {
        return Err(VaultAuthError::MalformedCredential(
            "OAuth token must not be empty",
        ));
    }
    if token.bytes().any(|b| b == 0 || b == b'\r' || b == b'\n') {
        return Err(VaultAuthError::MalformedCredential(
            "OAuth token has disallowed control chars",
        ));
    }
    Ok(())
}
