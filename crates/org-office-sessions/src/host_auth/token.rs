//! The host Claude Code OAuth sign-in record shared by the credentials-file and Keychain readers:
//! one size bound and one parser, so both sources refuse oversized or malformed answers alike.

use zeroize::Zeroizing;

/// Upper bound on a host Claude credentials answer (file or Keychain item) before parsing.
pub(super) const MAX_HOST_CLAUDE_CRED_BYTES: usize = 64 * 1024;

/// Resolved host Claude Code OAuth access token and its optional expiry timestamp (epoch ms).
#[derive(Clone)]
pub(crate) struct ClaudeOAuthToken {
    pub access_token: Zeroizing<String>,
    pub expires_at_ms: Option<u64>,
}

/// Parses `{"claudeAiOauth": {"accessToken": ..., "expiresAt": ...}}`; a blank token is refused.
pub(super) fn parse_claude_oauth_token(raw: &[u8]) -> Option<ClaudeOAuthToken> {
    let val: serde_json::Value = serde_json::from_slice(raw).ok()?;
    let oauth = val.get("claudeAiOauth")?;
    let token = oauth.get("accessToken")?.as_str()?.trim();
    if token.is_empty() {
        return None;
    }
    let expires_at_ms = oauth.get("expiresAt").and_then(serde_json::Value::as_u64);
    Some(ClaudeOAuthToken {
        access_token: Zeroizing::new(token.to_owned()),
        expires_at_ms,
    })
}
