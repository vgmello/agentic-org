//! Office private-key wrapping (ADR-0016 decision 3, ENC-5/ENC-6).
//!
//! An office keeps each private key in its own store as a record encrypted and
//! authenticated with its 32-byte office secrets key. The record is bound to the
//! organization, the office and the key's purpose, and its format is versioned.
//! This module takes raw bytes only: it knows no host, transport or storage types.
//!
//! Record: `version(1) || nonce(12) || AES-256-GCM(ciphertext || tag(16))`.
//! Authenticated data: `DOMAIN || version || purpose || organization || office`,
//! each of the last three prefixed with its length as one byte.
use ring::{
    aead::{self, Aad, LessSafeKey, Nonce, UnboundKey},
    rand::{SecureRandom, SystemRandom},
};
use zeroize::Zeroizing;

const DOMAIN: &[u8] = b"agentic-org/office-private-key/v1";
const VERSION: u8 = 1;
const NONCE: usize = 12;
const TAG: usize = 16;
/// Largest private-key encoding accepted for wrapping.
pub const MAX_KEY_BYTES: usize = 4096;

/// The role of the wrapped private key. TLS identity and invitation keys are
/// added with satellite enrollment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WrapPurpose {
    OrganizationCa,
}
impl WrapPurpose {
    fn label(self) -> &'static [u8] {
        match self {
            Self::OrganizationCa => b"organization-ca",
        }
    }
}

/// What a wrapped record is bound to. A record opens only for the same binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WrapBinding<'a> {
    pub organization_id: &'a str,
    pub office_id: &'a str,
    pub purpose: WrapPurpose,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WrapError {
    /// The binding or key encoding is outside the accepted bounds.
    Invalid,
    /// The record is malformed, of another version, or fails authentication
    /// (wrong key, organization, office or purpose).
    Rejected,
    /// The random source or cipher is unavailable.
    Unavailable,
}
impl std::fmt::Display for WrapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Invalid => "invalid key wrapping input",
            Self::Rejected => "wrapped key record rejected",
            Self::Unavailable => "key wrapping unavailable",
        })
    }
}
impl std::error::Error for WrapError {}

/// Encrypts `secret` (a private-key encoding) under `key` for `binding`.
pub fn wrap(
    key: &[u8; 32],
    binding: &WrapBinding<'_>,
    secret: &[u8],
) -> Result<Vec<u8>, WrapError> {
    if secret.is_empty() || secret.len() > MAX_KEY_BYTES {
        return Err(WrapError::Invalid);
    }
    let aad = aad(binding)?;
    let mut nonce = [0; NONCE];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| WrapError::Unavailable)?;
    let mut sealed = Zeroizing::new(Vec::with_capacity(secret.len() + TAG));
    sealed.extend_from_slice(secret);
    cipher(key)?
        .seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(aad),
            &mut *sealed,
        )
        .map_err(|_| WrapError::Unavailable)?;
    let mut record = Vec::with_capacity(1 + NONCE + sealed.len());
    record.push(VERSION);
    record.extend_from_slice(&nonce);
    record.extend_from_slice(&sealed);
    Ok(record)
}

/// Opens a record made by [`wrap`] with the same key and binding.
pub fn unwrap(
    key: &[u8; 32],
    binding: &WrapBinding<'_>,
    record: &[u8],
) -> Result<Zeroizing<Vec<u8>>, WrapError> {
    let aad = aad(binding)?;
    if record.len() <= 1 + NONCE + TAG || record.len() > 1 + NONCE + MAX_KEY_BYTES + TAG {
        return Err(WrapError::Rejected);
    }
    if record[0] != VERSION {
        return Err(WrapError::Rejected);
    }
    let nonce: [u8; NONCE] = record[1..1 + NONCE]
        .try_into()
        .map_err(|_| WrapError::Rejected)?;
    let mut buffer = Zeroizing::new(record[1 + NONCE..].to_vec());
    let len = cipher(key)?
        .open_in_place(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(aad),
            &mut buffer,
        )
        .map_err(|_| WrapError::Rejected)?
        .len();
    buffer.truncate(len);
    Ok(buffer)
}

fn cipher(key: &[u8; 32]) -> Result<LessSafeKey, WrapError> {
    Ok(LessSafeKey::new(
        UnboundKey::new(&aead::AES_256_GCM, key).map_err(|_| WrapError::Unavailable)?,
    ))
}

fn aad(binding: &WrapBinding<'_>) -> Result<Vec<u8>, WrapError> {
    let fields = [
        binding.purpose.label(),
        binding.organization_id.as_bytes(),
        binding.office_id.as_bytes(),
    ];
    let mut aad =
        Vec::with_capacity(DOMAIN.len() + 1 + fields.iter().map(|f| 1 + f.len()).sum::<usize>());
    aad.extend_from_slice(DOMAIN);
    aad.push(VERSION);
    for field in fields {
        let len = u8::try_from(field.len()).map_err(|_| WrapError::Invalid)?;
        if len == 0 {
            return Err(WrapError::Invalid);
        }
        aad.push(len);
        aad.extend_from_slice(field);
    }
    Ok(aad)
}

#[cfg(test)]
mod tests;
