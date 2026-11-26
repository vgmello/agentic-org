//! Single-use challenge nonces and Ed25519 verification for external leases (`TASK-137`).
//!
//! A nonce is bound to one agent, one lease session, that lease's public key and one purpose,
//! lives [`CHALLENGE_TTL_SECS`] and is removed the moment it is presented, whatever the outcome,
//! so a captured answer can never be replayed.

use std::collections::HashMap;

use org_proto::external_auth::{ChallengePurpose, CHALLENGE_TTL_SECS};
use org_proto::{ApiError, ErrorCode};
use ring::signature::{UnparsedPublicKey, ED25519};

/// Most outstanding nonces on one office, and per lease session.
pub(crate) const MAX_NONCES_TOTAL: usize = 256;
pub(crate) const MAX_NONCES_PER_SESSION: usize = 4;
/// Longest base64 signature accepted (64 bytes encode to 88 characters).
const MAX_SIGNATURE_B64: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NonceEntry {
    pub(crate) employee_id: String,
    pub(crate) session_id: String,
    pub(crate) pubkey_ed25519_b64: String,
    pub(crate) purpose: ChallengePurpose,
    pub(crate) issued_ms: u64,
    pub(crate) expires_ms: u64,
}

/// The outstanding nonces.
#[derive(Default)]
pub(crate) struct NonceBook {
    map: HashMap<String, NonceEntry>,
}

impl NonceBook {
    fn prune(&mut self, now_ms: u64) {
        self.map.retain(|_, e| now_ms < e.expires_ms);
    }

    /// Issues a fresh nonce for `(employee_id, session_id)`. A session holding the most nonces
    /// loses its oldest, so a retrying client is never stuck and the book stays bounded.
    pub(crate) fn issue(
        &mut self,
        entry_for: impl FnOnce(u64, u64) -> NonceEntry,
        now_ms: u64,
    ) -> Result<(String, NonceEntry), ApiError> {
        self.prune(now_ms);
        let entry = entry_for(now_ms, now_ms.saturating_add(CHALLENGE_TTL_SECS * 1000));
        let same: Vec<(String, u64)> = self
            .map
            .iter()
            .filter(|(_, e)| e.employee_id == entry.employee_id && e.session_id == entry.session_id)
            .map(|(k, e)| (k.clone(), e.issued_ms))
            .collect();
        if same.len() >= MAX_NONCES_PER_SESSION {
            if let Some((oldest, _)) = same.iter().min_by_key(|(_, issued)| *issued) {
                self.map.remove(oldest);
            }
        }
        if self.map.len() >= MAX_NONCES_TOTAL {
            return Err(ApiError::new(
                ErrorCode::Unavailable,
                "too many sign-in challenges are outstanding; retry shortly",
            ));
        }
        let nonce = crate::credentials::random_hex(32)
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "system entropy unavailable"))?;
        self.map.insert(nonce.clone(), entry.clone());
        Ok((nonce, entry))
    }

    /// Removes and answers `nonce` (single use), if it was issued and has not expired.
    pub(crate) fn take(&mut self, nonce: &str, now_ms: u64) -> Option<NonceEntry> {
        let entry = self.map.remove(nonce)?;
        (now_ms < entry.expires_ms).then_some(entry)
    }

    /// Drops every nonce of `employee_id` (its lease changed or ended).
    pub(crate) fn purge_member(&mut self, employee_id: &str) {
        self.map.retain(|_, e| e.employee_id != employee_id);
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.map.len()
    }
}

/// Verifies an Ed25519 `signature_b64` by `pubkey` over `message`. Fails closed on any
/// malformed input.
pub(crate) fn verify_signature(pubkey: &[u8], message: &[u8], signature_b64: &str) -> bool {
    use base64::Engine as _;
    if signature_b64.len() > MAX_SIGNATURE_B64 {
        return false;
    }
    let Ok(signature) = base64::engine::general_purpose::STANDARD.decode(signature_b64.trim())
    else {
        return false;
    };
    signature.len() == 64
        && UnparsedPublicKey::new(&ED25519, pubkey)
            .verify(message, &signature)
            .is_ok()
}
