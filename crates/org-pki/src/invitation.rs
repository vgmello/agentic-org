//! Satellite office invitation tokens, Crockford check codes, and TLS-exporter possession proofs
//! (`doc-67` §The invitation and connection, `TASK-52` Slice 1).
use std::fmt;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ring::signature::{self, UnparsedPublicKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use zeroize::Zeroizing;

use crate::{Error, SigningKey};

pub const INVITATION_VERSION: u32 = 1;
pub const INVITATION_PREFIX: &str = "agorg-inv1.";
const MAX_TOKEN_LEN: usize = 16 * 1024;
const CROCKFORD_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const CHECK_CODE_DOMAIN: &[u8] = b"agentic-org/satellite-check-code/v1";
const EXPORTER_PROOF_DOMAIN: &[u8] = b"agentic-org/satellite-exporter-proof/v1";

/// Versioned secret invitation token issued by HQ and delivered once to the joining machine.
/// Contains the one-time invitation private key; HQ retains only the public half.
#[derive(Clone, PartialEq, Eq)]
pub struct InvitationToken {
    pub version: u32,
    pub invitation_id: String,
    pub org_id: String,
    pub org_name: String,
    pub office_id: String,
    pub office_name: String,
    pub hq_office_id: String,
    pub hq_addresses: Vec<String>,
    pub ca_fingerprint_hex: String,
    pub ca_cert_der: Vec<u8>,
    pub expires_at_ms: u64,
    invitation_key_der: Zeroizing<Vec<u8>>,
}

impl fmt::Debug for InvitationToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InvitationToken")
            .field("version", &self.version)
            .field("invitation_id", &self.invitation_id)
            .field("org_id", &self.org_id)
            .field("office_id", &self.office_id)
            .field("hq_office_id", &self.hq_office_id)
            .field("expires_at_ms", &self.expires_at_ms)
            .field("invitation_key_der", &"<redacted>")
            .finish()
    }
}

/// Non-secret metadata safe to display or pass in a launch first line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvitationSummary {
    pub invitation_id: String,
    pub org_id: String,
    pub org_name: String,
    pub office_id: String,
    pub office_name: String,
    pub hq_office_id: String,
    pub hq_addresses: Vec<String>,
    pub ca_fingerprint_hex: String,
    pub expires_at_ms: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireInvitation {
    v: u32,
    invitation_id: String,
    org_id: String,
    org_name: String,
    office_id: String,
    office_name: String,
    hq_office_id: String,
    hq_addresses: Vec<String>,
    ca_fingerprint_hex: String,
    ca_cert_der_b64: String,
    expires_at_ms: u64,
    invitation_key_der_b64: String,
}

pub struct NewInvitationParams {
    pub invitation_id: String,
    pub org_id: String,
    pub org_name: String,
    pub office_id: String,
    pub office_name: String,
    pub hq_office_id: String,
    pub hq_addresses: Vec<String>,
    pub ca_cert_der: Vec<u8>,
    pub expires_at_ms: u64,
}

impl InvitationToken {
    pub fn new(params: NewInvitationParams, invitation_key: &SigningKey) -> Result<Self, Error> {
        if params.invitation_id.is_empty()
            || params.org_id.is_empty()
            || params.office_id.is_empty()
            || params.hq_office_id.is_empty()
            || params.hq_addresses.is_empty()
            || params.ca_cert_der.is_empty()
        {
            return Err(Error::InvalidPackage);
        }
        let ca_fingerprint_hex = crate::cert::cert_fingerprint_hex(&params.ca_cert_der);
        Ok(Self {
            version: INVITATION_VERSION,
            invitation_id: params.invitation_id,
            org_id: params.org_id,
            org_name: params.org_name,
            office_id: params.office_id,
            office_name: params.office_name,
            hq_office_id: params.hq_office_id,
            hq_addresses: params.hq_addresses,
            ca_fingerprint_hex,
            ca_cert_der: params.ca_cert_der,
            expires_at_ms: params.expires_at_ms,
            invitation_key_der: Zeroizing::new(invitation_key.local_der().to_vec()),
        })
    }

    pub fn summary(&self) -> InvitationSummary {
        InvitationSummary {
            invitation_id: self.invitation_id.clone(),
            org_id: self.org_id.clone(),
            org_name: self.org_name.clone(),
            office_id: self.office_id.clone(),
            office_name: self.office_name.clone(),
            hq_office_id: self.hq_office_id.clone(),
            hq_addresses: self.hq_addresses.clone(),
            ca_fingerprint_hex: self.ca_fingerprint_hex.clone(),
            expires_at_ms: self.expires_at_ms,
        }
    }

    pub fn signing_key(&self) -> Result<SigningKey, Error> {
        SigningKey::from_local_der(&self.invitation_key_der)
    }

    pub fn encode(&self) -> Result<Zeroizing<String>, Error> {
        let wire = WireInvitation {
            v: self.version,
            invitation_id: self.invitation_id.clone(),
            org_id: self.org_id.clone(),
            org_name: self.org_name.clone(),
            office_id: self.office_id.clone(),
            office_name: self.office_name.clone(),
            hq_office_id: self.hq_office_id.clone(),
            hq_addresses: self.hq_addresses.clone(),
            ca_fingerprint_hex: self.ca_fingerprint_hex.clone(),
            ca_cert_der_b64: URL_SAFE_NO_PAD.encode(&self.ca_cert_der),
            expires_at_ms: self.expires_at_ms,
            invitation_key_der_b64: URL_SAFE_NO_PAD.encode(&*self.invitation_key_der),
        };
        let raw = Zeroizing::new(serde_json::to_vec(&wire).map_err(|_| Error::InvalidPackage)?);
        let encoded = format!("{INVITATION_PREFIX}{}", URL_SAFE_NO_PAD.encode(&*raw));
        if encoded.len() > MAX_TOKEN_LEN {
            return Err(Error::InvalidPackage);
        }
        Ok(Zeroizing::new(encoded))
    }

    pub fn decode(token: &str) -> Result<Self, Error> {
        let trimmed = token.trim();
        if trimmed.len() > MAX_TOKEN_LEN {
            return Err(Error::InvalidPackage);
        }
        let body = trimmed
            .strip_prefix(INVITATION_PREFIX)
            .ok_or(Error::InvalidPackage)?;
        let raw = Zeroizing::new(
            URL_SAFE_NO_PAD
                .decode(body)
                .map_err(|_| Error::InvalidPackage)?,
        );
        let wire: WireInvitation =
            serde_json::from_slice(&raw).map_err(|_| Error::InvalidPackage)?;
        if wire.v != INVITATION_VERSION
            || wire.invitation_id.is_empty()
            || wire.org_id.is_empty()
            || wire.office_id.is_empty()
            || wire.hq_office_id.is_empty()
            || wire.hq_addresses.is_empty()
        {
            return Err(Error::InvalidPackage);
        }
        let ca_cert_der = URL_SAFE_NO_PAD
            .decode(&wire.ca_cert_der_b64)
            .map_err(|_| Error::InvalidPackage)?;
        let expected_fp = crate::cert::cert_fingerprint_hex(&ca_cert_der);
        if wire.ca_fingerprint_hex != expected_fp {
            return Err(Error::InvalidPackage);
        }
        let key_der = Zeroizing::new(
            URL_SAFE_NO_PAD
                .decode(&wire.invitation_key_der_b64)
                .map_err(|_| Error::InvalidPackage)?,
        );
        // Validate that the private key actually parses as a P-256 signing key.
        let _ = SigningKey::from_local_der(&key_der)?;
        Ok(Self {
            version: wire.v,
            invitation_id: wire.invitation_id,
            org_id: wire.org_id,
            org_name: wire.org_name,
            office_id: wire.office_id,
            office_name: wire.office_name,
            hq_office_id: wire.hq_office_id,
            hq_addresses: wire.hq_addresses,
            ca_fingerprint_hex: wire.ca_fingerprint_hex,
            ca_cert_der,
            expires_at_ms: wire.expires_at_ms,
            invitation_key_der: key_der,
        })
    }
}

/// Computes the deterministic 8-character Crockford Base32 check code (`XXXX-XXXX`) covering
/// `invitation_id` and the joining satellite's public key (`doc-67` §The invitation and connection).
pub fn compute_check_code(invitation_id: &str, joining_pub_key: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(CHECK_CODE_DOMAIN);
    h.update((invitation_id.len() as u32).to_be_bytes());
    h.update(invitation_id.as_bytes());
    h.update((joining_pub_key.len() as u32).to_be_bytes());
    h.update(joining_pub_key);
    let digest = h.finalize();
    // Extract 40 bits (8 x 5-bit symbols) from the first 5 digest bytes.
    let bits: u64 = ((digest[0] as u64) << 32)
        | ((digest[1] as u64) << 24)
        | ((digest[2] as u64) << 16)
        | ((digest[3] as u64) << 8)
        | (digest[4] as u64);
    let mut out = String::with_capacity(9);
    for i in 0..8 {
        if i == 4 {
            out.push('-');
        }
        let idx = ((bits >> (35 - i * 5)) & 0x1f) as usize;
        out.push(CROCKFORD_ALPHABET[idx] as char);
    }
    out
}

/// Normalizes human-entered Crockford Base32 check code input (`XXXX-XXXX`), rejecting invalid
/// lengths or symbols while mapping Crockford confusables (`O -> 0`, `I/L -> 1`).
pub fn normalize_check_code(input: &str) -> Option<String> {
    let mut syms = Vec::with_capacity(8);
    for b in input.bytes() {
        if b == b'-' || b.is_ascii_whitespace() {
            continue;
        }
        let upper = b.to_ascii_uppercase();
        let canonical = match upper {
            b'O' => b'0',
            b'I' | b'L' => b'1',
            c if CROCKFORD_ALPHABET.contains(&c) => c,
            _ => return None,
        };
        if syms.len() >= 8 {
            return None;
        }
        syms.push(canonical as char);
    }
    if syms.len() != 8 {
        return None;
    }
    let mut out = String::with_capacity(9);
    for (idx, ch) in syms.into_iter().enumerate() {
        if idx == 4 {
            out.push('-');
        }
        out.push(ch);
    }
    Some(out)
}

fn exporter_proof_preimage(
    exporter: &[u8; 32],
    invitation_id: &str,
    office_id: &str,
    joining_pub_key: &[u8],
) -> Vec<u8> {
    let mut msg = Vec::with_capacity(
        EXPORTER_PROOF_DOMAIN.len()
            + 32
            + 12
            + invitation_id.len()
            + office_id.len()
            + joining_pub_key.len(),
    );
    msg.extend_from_slice(EXPORTER_PROOF_DOMAIN);
    msg.extend_from_slice(exporter);
    msg.extend_from_slice(&(invitation_id.len() as u32).to_be_bytes());
    msg.extend_from_slice(invitation_id.as_bytes());
    msg.extend_from_slice(&(office_id.len() as u32).to_be_bytes());
    msg.extend_from_slice(office_id.as_bytes());
    msg.extend_from_slice(&(joining_pub_key.len() as u32).to_be_bytes());
    msg.extend_from_slice(joining_pub_key);
    msg
}

/// Signs the TLS exporter binding with the one-time invitation key so the key never leaves the
/// joining satellite (`doc-67` §The invitation and connection).
pub fn sign_exporter_proof(
    invitation_key: &SigningKey,
    exporter: &[u8; 32],
    invitation_id: &str,
    office_id: &str,
    joining_pub_key: &[u8],
) -> Result<Vec<u8>, Error> {
    let preimage = exporter_proof_preimage(exporter, invitation_id, office_id, joining_pub_key);
    invitation_key.sign_message_der(&preimage)
}

/// Verifies a satellite's TLS-exporter proof using HQ's stored public half of the invitation key.
pub fn verify_exporter_proof(
    invitation_pub_key: &[u8; 65],
    exporter: &[u8; 32],
    invitation_id: &str,
    office_id: &str,
    joining_pub_key: &[u8],
    signature_der: &[u8],
) -> Result<(), Error> {
    let preimage = exporter_proof_preimage(exporter, invitation_id, office_id, joining_pub_key);
    UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_ASN1, invitation_pub_key)
        .verify(&preimage, signature_der)
        .map_err(|_| Error::RecoveryFailed)
}
