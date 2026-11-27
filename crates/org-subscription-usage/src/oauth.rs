//! Strict parser for enrolled subscription OAuth envelopes (`TASK-79`, `TASK-132`).
//!
//! Leaf module with zero sibling imports. Shared by `org-subscription-usage` and
//! `org-office-sessions` (`host_auth::token`) as the single canonical OAuth envelope parser.

use serde::Deserialize;
use zeroize::Zeroizing;

/// Upper bound on a vault-stored OAuth credential payload before parsing.
pub const MAX_VAULT_OAUTH_CRED_BYTES: usize = 64 * 1024;

/// Parsed, validated OAuth credential envelope with zeroized secrets.
pub struct ParsedOAuthEnvelope {
    pub access_token: Zeroizing<String>,
    pub refresh_token: Option<Zeroizing<String>>,
    pub expires_at_ms: Option<u64>,
}

struct ZeroizingStr(Zeroizing<String>);

impl<'de> Deserialize<'de> for ZeroizingStr {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(|s| Self(Zeroizing::new(s)))
    }
}

#[derive(Deserialize)]
struct RawOAuthFields {
    #[serde(default, alias = "accessToken")]
    access_token: Option<ZeroizingStr>,
    #[serde(default, alias = "refreshToken")]
    refresh_token: Option<ZeroizingStr>,
    #[serde(default, alias = "expiresAt")]
    expires_at_ms: Option<u64>,
}

#[derive(Deserialize)]
struct RawOAuthEnvelope {
    #[serde(default, rename = "claudeAiOauth")]
    claude_ai_oauth: Option<RawOAuthFields>,
    #[serde(default, alias = "accessToken")]
    access_token: Option<ZeroizingStr>,
    #[serde(default, alias = "refreshToken")]
    refresh_token: Option<ZeroizingStr>,
    #[serde(default, alias = "expiresAt")]
    expires_at_ms: Option<u64>,
}

/// Validates that a secret token is non-empty, trimmed, and free of ASCII control characters.
pub fn validate_oauth_secret_token(token: &str) -> Result<(), &'static str> {
    if token.is_empty() || token != token.trim() {
        return Err("OAuth token must be non-empty and trimmed");
    }
    if token.bytes().any(|b| b.is_ascii_control()) {
        return Err("OAuth token has disallowed control chars");
    }
    Ok(())
}

fn parse_oauth_json(trimmed: &str) -> Result<ParsedOAuthEnvelope, &'static str> {
    let env: RawOAuthEnvelope =
        serde_json::from_str(trimmed).map_err(|_| "malformed JSON OAuth envelope")?;
    let fields = env.claude_ai_oauth.unwrap_or(RawOAuthFields {
        access_token: env.access_token,
        refresh_token: env.refresh_token,
        expires_at_ms: env.expires_at_ms,
    });
    let ZeroizingStr(access_token) = fields
        .access_token
        .ok_or("OAuth envelope is missing access_token")?;
    validate_oauth_secret_token(access_token.as_str())?;
    let refresh_token = match fields.refresh_token {
        Some(ZeroizingStr(rt)) if !rt.is_empty() => {
            validate_oauth_secret_token(rt.as_str())?;
            Some(rt)
        }
        _ => None,
    };
    Ok(ParsedOAuthEnvelope {
        access_token,
        refresh_token,
        expires_at_ms: fields.expires_at_ms,
    })
}

/// Parses an enrolled `subscription_cli` credential payload (raw token or JSON OAuth envelope)
/// into its zeroized bearer `access_token`, optional `refresh_token`, and optional `expires_at_ms`.
pub fn parse_oauth_envelope(raw: &[u8]) -> Result<ParsedOAuthEnvelope, &'static str> {
    if raw.len() > MAX_VAULT_OAUTH_CRED_BYTES {
        return Err("vault credential exceeds maximum size");
    }
    let text = std::str::from_utf8(raw).map_err(|_| "vault credential is not valid UTF-8")?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("vault credential is empty");
    }
    if trimmed.starts_with('{') {
        return parse_oauth_json(trimmed);
    }
    validate_oauth_secret_token(trimmed)?;
    Ok(ParsedOAuthEnvelope {
        access_token: Zeroizing::new(trimmed.to_owned()),
        refresh_token: None,
        expires_at_ms: None,
    })
}
