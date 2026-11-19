//! The browser listener's TLS pair (ADR-0017 decision 10, implementation plan X7): the one
//! place that makes the instance's own certificate, verifies a pair and keeps it.
//!
//! - **Making** ([`make`], `certificate`): an ECDSA P-256 key and a self-signed X.509
//!   certificate whose only subject alternative name is the origin's host (a DNS name or an IP
//!   literal), valid [`VALIDITY_DAYS`] days. rcgen writes it with ring; nothing here uses
//!   aws-lc.
//! - **Verifying** ([`check`], `verify`): `tls.set`'s check order (X-02 §6.2) from its third
//!   step: the chain and the key parse (`tls_invalid`), the key is the leaf's
//!   (`tls_key_mismatch`), the leaf is valid now (`tls_not_valid`) and names every `https`
//!   origin's host by a subject alternative name (`tls_name_mismatch`). The certificate and key
//!   blocks are framed by `org_host_control::pem`, the reader `tls.set`'s fingerprint uses;
//!   x509-parser reads the certificates and rustls (the web process's own TLS stack, ring
//!   provider) loads the key and compares it with the leaf. A pair that passes is returned in
//!   a normal form ([`Checked`]): only its `CERTIFICATE` blocks and its first key block,
//!   re-encoded, so text around them (another key in a certificate file, say) is never kept.
//! - **Keeping** ([`WebTls`], `store`): the key in the host's secrets under `tls.<pair
//!   fingerprint>`, the chain and whether the pair was made here or supplied in the registry
//!   (`hostdb::web_tls`). The instance's data and records never hold the key; nothing logs it.
//!   What is printed is the leaf's SHA-256 fingerprint ([`Fingerprint`]).
//! - **Reading a supplied pair** ([`read_pem_file`], `files`): the setup configuration names
//!   two PEM files; each is opened without following a link in its last component, must be a
//!   regular file and is read up to [`FILE_LIMIT`] bytes.
//!
//! Setup verifies a supplied pair before it saves anything; the host imports it at its first
//! start, verified again, and from then on reads only its own copy. A self-made key that is
//! lost is made again; a supplied one is not, and the start is refused. A supplied pair that
//! no longer verifies is kept but not served (the web start's gate) until `tls.set` replaces it.
mod certificate;
mod files;
mod renewal;
mod store;
mod verify;

#[cfg(test)]
pub(crate) mod tests;

pub(crate) use certificate::make;
pub(crate) use files::read_pem_file;
#[cfg(test)]
use files::FILE_LIMIT;
pub(crate) use renewal::{Renewal, EVERY as RENEW_EVERY};
#[cfg(test)]
use store::StartRefusal;
pub(crate) use store::{read_supplied, Unservable, WebTls};
pub(crate) use verify::check;

use std::fmt;

use zeroize::Zeroizing;

/// How long the instance's own certificate is valid: 397 days, the longest a browser accepts
/// for a server certificate.
pub(crate) const VALIDITY_DAYS: i64 = 397;
/// The instance makes itself a new certificate when fewer than this many days remain.
pub(crate) const RENEW_WITHIN_DAYS: i64 = 30;
const DAY_SECS: i64 = 86_400;

/// Who made the stored pair: the instance itself, or the operator (setup's configuration, or
/// `tls.set`). It decides what a lost key means: one made here is made again, a supplied one
/// refuses the start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    Made,
    Supplied,
}

impl Source {
    /// The registry's spelling.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Source::Made => "made",
            Source::Supplied => "supplied",
        }
    }

    pub(crate) fn parse(text: &str) -> Option<Self> {
        match text {
            "made" => Some(Source::Made),
            "supplied" => Some(Source::Supplied),
            _ => None,
        }
    }

    /// How the host log and setup name it.
    fn described(self) -> &'static str {
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
pub(crate) struct Fingerprint(pub(crate) [u8; 32]);

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
pub(crate) struct Checked {
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
    pub(crate) fn chain(&self) -> &str {
        &self.chain
    }
    pub(crate) fn key(&self) -> &str {
        &self.key
    }
    pub(crate) fn pair(&self) -> [u8; 32] {
        self.pair
    }
    #[cfg(test)]
    pub(crate) fn fingerprint(&self) -> Fingerprint {
        self.leaf
    }
    #[cfg(test)]
    pub(crate) fn not_after(&self) -> i64 {
        self.not_after
    }
    #[cfg(test)]
    pub(crate) fn not_before_for_test(&self) -> i64 {
        self.not_before
    }
    /// Whether fewer than [`RENEW_WITHIN_DAYS`] days of validity remain at `now`.
    pub(crate) fn expiring(&self, now: i64) -> bool {
        self.not_after - now < RENEW_WITHIN_DAYS * DAY_SECS
    }
    /// One line: the fingerprint and the validity. No certificate or key bytes.
    pub(crate) fn summary(&self, source: Source) -> String {
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
pub(crate) fn utc(secs: i64) -> Option<String> {
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
pub(crate) fn now() -> Option<i64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())
}

/// Lowercase hex, for the secret's name.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
