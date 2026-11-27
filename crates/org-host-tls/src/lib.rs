//! Browser-listener TLS pair generation, X.509/rustls verification, bounded PEM reading, and
//! background certificate renewal (ADR-0017 decision 10).
//!
//! Pure leaf below `org-host`: holds no registry or secrets store and never logs key material.

#![forbid(unsafe_code)]

mod certificate;
mod files;
mod renewal;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
#[cfg(test)]
mod tests;
mod verify;

pub use certificate::make;
pub use files::{read_pem_file, FILE_LIMIT};
pub use renewal::{Renewal, RENEW_EVERY};
pub use verify::{check, CHAIN_LIMIT, MAX_KEY_BYTES};

use std::fmt;
use std::path::Path;

use org_host_control::listen::ListenOrigin;
use org_host_control::ops::TlsRefusal;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

/// How long the instance's own certificate is valid: 397 days, the longest a browser accepts
/// for a server certificate.
pub const VALIDITY_DAYS: i64 = 397;
/// The instance makes itself a new certificate when fewer than this many days remain.
pub const RENEW_WITHIN_DAYS: i64 = 30;
/// Seconds in one UTC day.
pub const DAY_SECS: i64 = 86_400;

/// Who made the stored pair: decides what a lost key means (one made here is made again;
/// a supplied one refuses the start).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// Made by this instance itself.
    Made,
    /// Supplied by the operator (setup or `tls.set`).
    Supplied,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Made => "made",
            Source::Supplied => "supplied",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "made" => Some(Source::Made),
            "supplied" => Some(Source::Supplied),
            _ => None,
        }
    }

    /// How the host log and setup name it.
    pub fn described(self) -> &'static str {
        match self {
            Source::Made => "made by this instance",
            Source::Supplied => "supplied by the operator",
        }
    }
}

/// The SHA-256 of a certificate's DER: what a browser shows as the certificate's fingerprint,
/// and the one thing about a pair the host prints. Displayed as uppercase hex pairs joined by
/// colons, as browsers and `openssl x509 -fingerprint -sha256` show it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fingerprint(pub [u8; 32]);

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, byte) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(":")?;
            }
            write!(f, "{byte:02X}")?;
        }
        Ok(())
    }
}

/// A pair that passed [`check`], in its normal form.
pub struct Checked {
    /// The chain's `CERTIFICATE` blocks, leaf first, re-encoded: nothing else of the text.
    chain: String,
    /// The key's first block, re-encoded with the label of its form. A secret.
    key: Zeroizing<String>,
    /// SHA-256 of the length-prefixed chain DER and of the key DER, hashed together: the
    /// name of the key in the host's secrets (`tls.<hex>`) and the registry's `pair_fp`.
    pair: [u8; 32],
    leaf: Fingerprint,
    /// The leaf's validity bounds, Unix seconds.
    not_before: i64,
    not_after: i64,
}

impl Checked {
    pub fn chain(&self) -> &str {
        &self.chain
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    /// Consumes the verified pair, returning `(normalized_chain_pem, zeroizing_key_pem)`.
    pub fn into_parts(self) -> (String, Zeroizing<String>) {
        (self.chain, self.key)
    }

    pub fn pair(&self) -> [u8; 32] {
        self.pair
    }

    /// The leaf's SHA-256 fingerprint, the one thing about the pair a command prints.
    pub fn fingerprint(&self) -> Fingerprint {
        self.leaf
    }

    pub fn not_before(&self) -> i64 {
        self.not_before
    }

    pub fn not_after(&self) -> i64 {
        self.not_after
    }

    /// Whether fewer than [`RENEW_WITHIN_DAYS`] days of validity remain at `now`.
    pub fn expiring(&self, now: i64) -> bool {
        self.not_after - now < RENEW_WITHIN_DAYS * DAY_SECS
    }

    /// One line: the fingerprint and the validity. No certificate or key bytes.
    pub fn summary(&self, source: Source) -> String {
        let at = |secs| utc(secs).unwrap_or_else(|| "an unreadable date".into());
        format!(
            "{}, SHA-256 fingerprint {}, valid from {} until {}",
            source.described(),
            self.leaf,
            at(self.not_before),
            at(self.not_after),
        )
    }
}

/// Unix seconds as `YYYY-MM-DDTHH:MM:SSZ` (the profile of `tls_not_valid`'s timestamps), or
/// `None` outside the years 0 to 9999.
pub fn utc(secs: i64) -> Option<String> {
    let at = time::OffsetDateTime::from_unix_timestamp(secs).ok()?;
    if !(0..=9999).contains(&at.year()) {
        return None;
    }
    Some(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        at.year(),
        u8::from(at.month()),
        at.day(),
        at.hour(),
        at.minute(),
        at.second()
    ))
}

/// The wall clock, Unix seconds.
pub fn now() -> Option<i64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())
}

/// Lowercase hex, for the secret's name.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The fingerprint of `chain`'s leaf, read without its key, or `unreadable`.
pub fn leaf_fingerprint(chain: &str) -> String {
    org_host_control::pem::first_block(chain, org_host_control::pem::CERTIFICATE)
        .map(|der| Fingerprint(Sha256::digest(der.bytes()).into()).to_string())
        .unwrap_or_else(|_| "unreadable".into())
}

/// The supplied pair at `cert` and `key`, read and checked for `origin` at `now`.
pub fn read_supplied(
    cert: &Path,
    key: &Path,
    origin: &ListenOrigin,
    now: i64,
) -> Result<Checked, String> {
    let cert_text = read_pem_file(cert).map_err(|why| format!("the TLS certificate {why}"))?;
    let key_text = read_pem_file(key).map_err(|why| format!("the TLS key {why}"))?;
    check(&cert_text, &key_text, &[origin], Some(now)).map_err(|why| {
        format!(
            "the TLS pair ({}, {}) is refused: {}",
            cert.display(),
            key.display(),
            refusal_text(&why)
        )
    })
}

/// A refusal in words: its code and what it means.
pub fn refusal_text(refusal: &TlsRefusal) -> String {
    match refusal {
        TlsRefusal::TlsInvalid => "tls_invalid: the certificate chain or the key does not parse \
             (PEM CERTIFICATE blocks, leaf first, and an unencrypted PRIVATE KEY, RSA PRIVATE KEY \
             or EC PRIVATE KEY block)"
            .into(),
        TlsRefusal::TlsKeyMismatch => "tls_key_mismatch: the key is not the certificate's".into(),
        TlsRefusal::TlsNotValid {
            not_before,
            not_after,
        } => format!(
            "tls_not_valid: the certificate is valid from {} to {}, not now",
            serde_json::to_string(not_before).unwrap_or_default(),
            serde_json::to_string(not_after).unwrap_or_default()
        ),
        TlsRefusal::TlsNameMismatch { uncovered } => format!(
            "tls_name_mismatch: the certificate names no subject alternative name for {}",
            uncovered
                .origins()
                .iter()
                .map(|o| o.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        other => format!("{other:?}"),
    }
}
