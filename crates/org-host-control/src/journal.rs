//! The host's op journal (X-02 §6.3): what makes two requests "the same op".
//!
//! Every journaled op names an `operation_id` (a canonical lowercase UUIDv7, with the org-command age and skew rules)
//! and keeps its result for 30 days. Its fingerprint is SHA-256 over the method and the params
//! **without `session` and `operation_id`**, so a retry after a new sign-in is the same op. The
//! principal is compared by account:
//! - the same `operation_id`, fingerprint and account: a replay of the stored result;
//! - the same `operation_id` with another fingerprint or another account: `operation_reused`.
//!
//! **The journal lookup comes first** for every journaled op, `tls.set` included: a known
//! `operation_id` is looked up before any of the op's checks, a stored result replays (a stored
//! `tls.set` success replays even if setup has since rewritten the origin), and only an unknown
//! or unstored one runs the checks. **A `tls.set` whose fingerprint can't be computed** (its
//! [`Journaled::fingerprint_fields`] fails: the PEM reader refuses the pair) is answered at the
//! lookup with `tls_invalid`, unstored, whether or not its `operation_id` is known.
//!
//! This crate defines and hashes the bytes ([`fingerprint_input`], [`fingerprint`]).
//! `tls.set`'s fields are themselves hashes, which this crate computes
//! ([`crate::ops::TlsPairFingerprint`]).
//!
//! **A secret is never part of a fingerprint.** The journal keeps fingerprints for 30 days, and
//! `tls.set`'s fields are SHA-256 hashes of the certificate's and the key's DER, computed from
//! the secret type by [`crate::ops::TlsPairFingerprint`] (X-02 §6.2).

/// A journaled op's params. Each implementation destructures its params exhaustively, so adding
/// a param fails to compile until it is fingerprinted or explicitly excluded.
pub trait Journaled {
    /// The op's method.
    const METHOD: &'static str;
    /// The op's id.
    fn operation_id(&self) -> &str;
    /// The params that say what the op is, as compact JSON: everything except `session` and
    /// `operation_id`. An error means the op has no fingerprint, so the journal lookup can't run:
    /// for `tls.set`, a pair the PEM reader refuses, answered `tls_invalid`, unstored.
    fn fingerprint_fields(&self) -> Result<String, serde_json::Error>;
}

/// What the host hashes for an op's fingerprint: `method || 0x00 || fingerprint_fields()`, the
/// same form as an org command's fingerprint.
pub fn fingerprint_input<J: Journaled>(params: &J) -> Result<Vec<u8>, serde_json::Error> {
    let fields = params.fingerprint_fields()?;
    let mut out = Vec::with_capacity(J::METHOD.len() + 1 + fields.len());
    out.extend_from_slice(J::METHOD.as_bytes());
    out.push(0);
    out.extend_from_slice(fields.as_bytes());
    Ok(out)
}

/// SHA-256 of the canonical journal input; login normalization remains the host's job.
pub fn fingerprint<J: Journaled>(params: &J) -> Result<[u8; 32], serde_json::Error> {
    use sha2::{Digest, Sha256};
    Ok(Sha256::digest(fingerprint_input(params)?).into())
}
