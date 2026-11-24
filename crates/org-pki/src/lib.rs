//! Organization CA escrow, X.509 certificates, pinned mTLS, satellite invitations, and
//! per-office private-key wrapping (`ADR-0016`, `doc-67`, `TASK-52`).
#![forbid(unsafe_code)]
pub mod cert;
mod code;
mod envelope;
pub mod invitation;
pub mod wrap;
pub use code::Code;
pub use envelope::{Envelope, MAX_PACKAGE_BYTES};
use ring::{
    rand::SystemRandom,
    signature::{self, EcdsaKeyPair, KeyPair},
};
pub use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidCode,
    InvalidPackage,
    RecoveryFailed,
    Unavailable,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidCode => "invalid recovery code",
            Self::InvalidPackage => "invalid escrow package",
            Self::RecoveryFailed => "escrow recovery failed",
            Self::Unavailable => "escrow service unavailable",
        })
    }
}
impl std::error::Error for Error {}

/// Application-owned DER is zeroed on drop. ring also holds an opaque key pair;
/// its internal allocations are outside this crate's zeroization guarantee.
pub struct SigningKey {
    der: Zeroizing<Vec<u8>>,
    pair: EcdsaKeyPair,
}
impl std::fmt::Debug for SigningKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SigningKey(<redacted>)")
    }
}
impl SigningKey {
    pub fn generate() -> Result<Self, Error> {
        let der = EcdsaKeyPair::generate_pkcs8(
            &signature::ECDSA_P256_SHA256_ASN1_SIGNING,
            &SystemRandom::new(),
        )
        .map_err(|_| Error::Unavailable)?;
        Self::from_local_der(der.as_ref())
    }
    pub fn from_local_der(der: &[u8]) -> Result<Self, Error> {
        if der.is_empty() || der.len() > 4096 {
            return Err(Error::RecoveryFailed);
        }
        let pair = EcdsaKeyPair::from_pkcs8(
            &signature::ECDSA_P256_SHA256_ASN1_SIGNING,
            der,
            &SystemRandom::new(),
        )
        .map_err(|_| Error::RecoveryFailed)?;
        Ok(Self {
            der: Zeroizing::new(der.to_vec()),
            pair,
        })
    }
    pub fn public_key(&self) -> [u8; 65] {
        self.pair
            .public_key()
            .as_ref()
            .try_into()
            .expect("P-256 uncompressed key")
    }
    /// For wrapping into the worker's local store ([`wrap`]); never copy into
    /// public events, snapshots, status, logs or a host-control response.
    pub fn local_der(&self) -> &[u8] {
        &self.der
    }
    pub fn sign_message_der(&self, message: &[u8]) -> Result<Vec<u8>, Error> {
        self.pair
            .sign(&SystemRandom::new(), message)
            .map(|s| s.as_ref().to_vec())
            .map_err(|_| Error::Unavailable)
    }
    pub fn seal(&self, organization: &str, code: &Code) -> Result<Vec<u8>, Error> {
        envelope::seal(self, organization, code)
    }
}

/// Parse every framing/identity/profile bound before allocating Argon2 memory.
pub fn recover(package: &[u8], organization: &str, code: &Code) -> Result<SigningKey, Error> {
    Envelope::parse(package, organization)?.recover(code)
}
#[cfg(test)]
mod tests;
