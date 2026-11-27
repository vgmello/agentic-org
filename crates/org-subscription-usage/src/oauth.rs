//! Strict parser for enrolled subscription OAuth envelopes (`TASK-79`, `TASK-132`).
//!
//! Leaf module with zero sibling imports.

use serde::Deserialize;
use zeroize::Zeroizing;

const MAX_VAULT_OAUTH_CRED_BYTES: usize = 64 * 1024;

pub(crate) struct ParsedUsageOAuth {
    pub access_token: Zeroizing<String>,
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

fn is_valid_secret_token(token: &str) -> bool {
    !token.is_empty() && token == token.trim() && !token.bytes().any(|b| b.is_ascii_control())
}

/// Parses an enrolled `subscription_cli` credential payload (raw token or JSON OAuth envelope)
/// into its zeroized bearer `access_token` and optional `expires_at_ms`.
pub(crate) fn parse_usage_oauth_credential(raw: &[u8]) -> Option<ParsedUsageOAuth> {
    if raw.len() > MAX_VAULT_OAUTH_CRED_BYTES {
        return None;
    }
    let text = std::str::from_utf8(raw).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with('{') {
        let env: RawOAuthEnvelope = serde_json::from_str(trimmed).ok()?;
        let fields = env.claude_ai_oauth.unwrap_or(RawOAuthFields {
            access_token: env.access_token,
            refresh_token: env.refresh_token,
            expires_at_ms: env.expires_at_ms,
        });
        let ZeroizingStr(access_token) = fields.access_token?;
        if !is_valid_secret_token(access_token.as_str()) {
            return None;
        }
        if let Some(ZeroizingStr(rt)) = fields.refresh_token {
            if !rt.is_empty() && !is_valid_secret_token(rt.as_str()) {
                return None;
            }
        }
        return Some(ParsedUsageOAuth {
            access_token,
            expires_at_ms: fields.expires_at_ms,
        });
    }
    if !is_valid_secret_token(trimmed) {
        return None;
    }
    Some(ParsedUsageOAuth {
        access_token: Zeroizing::new(trimmed.to_owned()),
        expires_at_ms: None,
    })
}
