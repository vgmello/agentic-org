//! TLS ops: `tls.fetch`, `web.serving` and `tls.set`, their refusals, certificate timestamps and
//! pair fingerprints.

use std::fmt;
use std::fmt::Write;

use org_proto::{Call, Empty};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::listen::{
    Hsts, InvalidListenConfig, ListenFailure, ListenMode, ListenOrigin, Uncovered,
};
use crate::pem::InvalidPem;
use crate::{pem, HostOpResult, SessionHandle, TlsPrivateKey};

/// `tls.fetch` (**control connection only**; refused on `host.sock`): what the gateway serves, at
/// start and after each `tls.renew` event. `mode`, `origin` and `hsts` come from the **per-bind
/// snapshot** ([`crate::listen::ListenConfig`]) of the listener this gateway was given, never a live re-read;
/// `cert` and `key` are the stored pair. Before answering `ok` the host re-runs its pre-spawn
/// check on that snapshot ([`ListenOrigin`]'s two steps, and in `https` coverage of the
/// snapshot's origin by the stored pair), in the pre-spawn check's order, and answers
/// `unavailable` with the reason on a failure. A correct host never does: the form is defence in
/// depth. If the stored pair can't be read (the credential item was lost), the host answers the
/// plain error `storage_error` instead and reports `tls_missing` in status; the gateway treats
/// that error as it treats `unavailable`.
///
/// The gateway, at start: on `ok` it checks `origin` against `mode` and its listener with
/// [`ListenOrigin::check`] and refuses to start on a failure; on `unavailable` it refuses to
/// start. After `tls.renew`: `ok` with the same `mode`, `origin` and `hsts` swaps the pair for new
/// handshakes; `ok` with different ones is a host bug, and it exits without swapping;
/// `unavailable` keeps the current pair.
pub struct TlsFetch;

/// `tls.fetch`'s tagged result:
/// - `{"ok": {"mode": "https", "cert", "key", "origin", "hsts"}}`;
/// - `{"ok": {"mode": "http_loopback", "origin", "hsts"}}`, with no certificate fields;
/// - `{"ok": {"mode": "http_published", "origin", "hsts"}}`, with no certificate fields;
/// - `{"unavailable": <ListenFailure>}`: exactly a [`ListenFailure`].
///
/// Not `Clone`: the key is a secret. The `ok` object decodes as one flat object, not through
/// serde's internally tagged enums, so this decode adds no buffer of its own between the JSON and
/// [`TlsPrivateKey`]. The JSON itself is the envelope's raw result, a `serde_json::Value`: built
/// with `serde_json::to_value` on the host's side and parsed from the response line on the
/// gateway's, so the key also sits there, unzeroed, on both sides (§6.5's raw-JSON exception).
/// `https` needs both `cert` and `key`, non-empty strings; `http_loopback` and
/// `http_published` take neither, so a `cert` or `key` there is refused whatever its value,
/// `null` included; any other `mode` is refused. `origin` must be canonical; its fit to the listener is the gateway's check, not the
/// decoder's.
#[derive(Debug)]
pub enum TlsFetchResult {
    Https {
        /// The certificate chain, PEM.
        cert: String,
        key: TlsPrivateKey,
        origin: ListenOrigin,
        hsts: Hsts,
    },
    /// Plain HTTP on `127.0.0.1` or `::1`: no pair.
    HttpLoopback { origin: ListenOrigin, hsts: Hsts },
    /// Plain HTTP on a published port of a foreground instance
    /// ([`ListenMode::HttpPublished`]): no pair.
    HttpPublished { origin: ListenOrigin, hsts: Hsts },
    /// The host can't serve the snapshot (a host bug).
    Unavailable(ListenFailure),
}

impl TlsFetchResult {
    /// The snapshot's `mode`, `origin` and `hsts`, for `ok`: what the gateway compares after
    /// `tls.renew`. `None` for `unavailable`.
    pub fn listen(&self) -> Option<(ListenMode, &ListenOrigin, Hsts)> {
        match self {
            TlsFetchResult::Https { origin, hsts, .. } => Some((ListenMode::Https, origin, *hsts)),
            TlsFetchResult::HttpLoopback { origin, hsts } => {
                Some((ListenMode::HttpLoopback, origin, *hsts))
            }
            TlsFetchResult::HttpPublished { origin, hsts } => {
                Some((ListenMode::HttpPublished, origin, *hsts))
            }
            TlsFetchResult::Unavailable(_) => None,
        }
    }
}

#[derive(Serialize)]
pub(super) struct TlsFetchRef<'a> {
    mode: ListenMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    cert: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    key: Option<&'a TlsPrivateKey>,
    origin: &'a ListenOrigin,
    hsts: Hsts,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum TlsFetchOut<'a> {
    Ok(TlsFetchRef<'a>),
    Unavailable(&'a ListenFailure),
}

/// `cert` and `key` are present-means-`Some`, like `Request.principal`: the outer `Option` says
/// whether the key is there at all, so `http_loopback` refuses `"cert": null` as it refuses any
/// other value, and the inner one carries a `null`, which `https` refuses too.
#[derive(Deserialize)]
pub(super) struct RawTlsFetch {
    mode: ListenMode,
    #[serde(default, deserialize_with = "present")]
    cert: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    key: Option<Option<TlsPrivateKey>>,
    origin: ListenOrigin,
    hsts: Hsts,
}

/// Decodes a present field as `Some`, even when its value is `null`. An absent one is `None`
/// through `default`.
pub(super) fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum TlsFetchIn {
    Ok(RawTlsFetch),
    Unavailable(ListenFailure),
}

impl Serialize for TlsFetchResult {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            TlsFetchResult::Https {
                cert,
                key,
                origin,
                hsts,
            } => TlsFetchOut::Ok(TlsFetchRef {
                mode: ListenMode::Https,
                cert: Some(cert),
                key: Some(key),
                origin,
                hsts: *hsts,
            }),
            TlsFetchResult::HttpLoopback { origin, hsts } => TlsFetchOut::Ok(TlsFetchRef {
                mode: ListenMode::HttpLoopback,
                cert: None,
                key: None,
                origin,
                hsts: *hsts,
            }),
            TlsFetchResult::HttpPublished { origin, hsts } => TlsFetchOut::Ok(TlsFetchRef {
                mode: ListenMode::HttpPublished,
                cert: None,
                key: None,
                origin,
                hsts: *hsts,
            }),
            TlsFetchResult::Unavailable(failed) => TlsFetchOut::Unavailable(failed),
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for TlsFetchResult {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        let raw = match TlsFetchIn::deserialize(d)? {
            TlsFetchIn::Ok(raw) => raw,
            TlsFetchIn::Unavailable(failed) => return Ok(TlsFetchResult::Unavailable(failed)),
        };
        let RawTlsFetch {
            mode,
            cert,
            key,
            origin,
            hsts,
        } = raw;
        match (mode, cert, key) {
            // `TlsPrivateKey` already refuses an empty key; the key test keeps the rule local.
            (ListenMode::Https, Some(Some(cert)), Some(Some(key)))
                if !cert.is_empty() && !key.expose().is_empty() =>
            {
                Ok(TlsFetchResult::Https {
                    cert,
                    key,
                    origin,
                    hsts,
                })
            }
            (ListenMode::HttpLoopback, None, None) => {
                Ok(TlsFetchResult::HttpLoopback { origin, hsts })
            }
            (ListenMode::HttpPublished, None, None) => {
                Ok(TlsFetchResult::HttpPublished { origin, hsts })
            }
            (ListenMode::Https, _, _) => {
                Err(D::Error::custom("https needs a non-empty cert and key"))
            }
            (ListenMode::HttpLoopback, _, _) => {
                Err(D::Error::custom("http_loopback takes no cert or key"))
            }
            (ListenMode::HttpPublished, _, _) => {
                Err(D::Error::custom("http_published takes no cert or key"))
            }
        }
    }
}

impl Call for TlsFetch {
    const METHOD: &'static str = "tls.fetch";
    type Params = Empty;
    type Result = TlsFetchResult;
}

/// `web.serving` (**web process control connection only**; `not_authorized` on `host.sock` and
/// setup connections): the web process's one notification that its HTTP server is initialized and
/// accepting on the listener it was given. Sent exactly once, after a successful `tls.fetch`; the
/// host answers `{}`. The host reports the web process `running` only after it, and fails the start if
/// it does not arrive within the host's serving deadline. A repeated or out-of-order notification
/// is a protocol error: the host answers `invalid_request` and closes the control connection.
pub struct WebServing;

/// `web.serving`'s params: closed and empty (`{}`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebServingParams {}

impl Call for WebServing {
    const METHOD: &'static str = "web.serving";
    type Params = WebServingParams;
    type Result = Empty;
}

/// `tls.set` (`host.sock` only; admin, the signed `agentic-org` peer on macOS; journaled): install
/// a TLS pair. First, as for every journaled op, the journal lookup, which needs the op's
/// fingerprint, the pair's [`TlsPairFingerprint`]: a pair the PEM reader ([`crate::pem`]) refuses
/// has none and gets [`TlsRefusal::TlsInvalid`], unstored, right there, whether or not the
/// `operation_id` is known (never a replay or `operation_reused`). A known `operation_id` with
/// a stored result replays it before any check (a stored success replays even if setup has since
/// rewritten the origin; [`crate::journal`]). Otherwise, before it stores the pair and emits
/// `tls.renew`, the host checks, in this order, and answers the first failure (X-02 §6.2):
/// 1. the stored listener fields (`address`, `port`, `scheme`) and `origin`
///    ([`TlsRefusal::OriginInvalid`]);
/// 2. the stored `hsts` ([`TlsRefusal::HstsInvalid`]);
/// 3. that the certificate and the key parse ([`TlsRefusal::TlsInvalid`]), framed by the same
///    PEM reader;
/// 4. that the key matches the certificate ([`TlsRefusal::TlsKeyMismatch`]);
/// 5. that the certificate is valid now ([`TlsRefusal::TlsNotValid`]);
/// 6. in `https`, that it covers every `https` origin among the stored one and the running one
///    ([`TlsRefusal::TlsNameMismatch`]; Certificate coverage).
///
/// It never changes the scheme. Its journal fingerprint fields are the pair's
/// [`TlsPairFingerprint`] (SHA-256 of the whole certificate chain, every `CERTIFICATE` block's DER
/// with a u32 big-endian length prefix, in order, and SHA-256 of the key DER), computed
/// from the secret type (`TlsSetParams`'s [`crate::journal::Journaled`] implementation); the
/// stored result echoes neither.
///
/// A failed check answers `{"status": "rejected", "refusal": <TlsRefusal>}` and stores no pair;
/// only [`TlsRefusal::TlsKeyMismatch`] is journaled ([`TlsRefusal::is_stored`]).
pub struct TlsSet;

#[derive(Debug, Serialize, Deserialize)]
pub struct TlsSetParams {
    pub session: SessionHandle,
    pub operation_id: String,
    pub cert: String,
    pub key: TlsPrivateKey,
}

impl Call for TlsSet {
    const METHOD: &'static str = "tls.set";
    type Params = TlsSetParams;
    type Result = HostOpResult<Empty, TlsRefusal>;
}

/// Why `tls.set` refused a TLS pair (X-02 §6.2, §6.3), with its wire form. These
/// aren't §9 error codes, and none carries a message.
///
/// A code this build doesn't know decodes as [`TlsRefusal::Unknown`]. It is never sent:
/// serializing it fails.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
#[non_exhaustive]
pub enum TlsRefusal {
    /// `{"code": "tls_name_mismatch", "uncovered": [...]}`: in `https`, the certificate doesn't
    /// cover these origins (the stored one first, then the running one).
    TlsNameMismatch { uncovered: Uncovered },
    /// `{"code": "tls_invalid"}`: the certificate or the key doesn't parse. Never stored, so a
    /// retry with the same `operation_id` and a fixed pair is checked as new. A pair the PEM reader
    /// refuses has no fingerprint at all: it
    /// ([`TlsPairFingerprint`] fails, `TlsRefusal::from` an [`InvalidPem`]) gets it on every
    /// path, before anything else that needs the pair's fingerprint: `tls.set`'s journal lookup.
    TlsInvalid,
    /// `{"code": "tls_key_mismatch"}`: the key doesn't match the certificate.
    TlsKeyMismatch,
    /// `{"code": "tls_not_valid", "not_before", "not_after"}`: the certificate isn't valid now.
    /// Both are the certificate's validity bounds, as [`UtcTimestamp`]s.
    TlsNotValid {
        not_before: UtcTimestamp,
        not_after: UtcTimestamp,
    },
    /// `{"code": "origin_invalid"}`: the stored listen configuration's `origin` is invalid, or an
    /// `address`, `port` or `scheme` it is checked against is missing or malformed.
    OriginInvalid,
    /// `{"code": "hsts_invalid"}`: the stored listen configuration's `hsts` is invalid.
    HstsInvalid,
    #[serde(other, skip_serializing)]
    Unknown,
}

/// A certificate validity bound, as `tls_not_valid` carries it: RFC 3339 in one profile,
/// `YYYY-MM-DDTHH:MM:SSZ`. UTC with an uppercase `T` and `Z`, no fractional seconds and no offset
/// (`+00:00` is refused), and a real date and time: month 1-12, a day that exists in that month
/// (leap years by the Gregorian rule), hour 0-23, minute and second 0-59 (no leap second).
/// Decoding refuses any other form; it is never rewritten.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct UtcTimestamp(String);

impl UtcTimestamp {
    /// Accept `text` only in the profile above.
    pub fn parse(text: &str) -> Result<UtcTimestamp, InvalidTimestamp> {
        let b = text.as_bytes();
        if b.len() != 20 {
            return Err(InvalidTimestamp);
        }
        for (i, want) in [
            (4, b'-'),
            (7, b'-'),
            (10, b'T'),
            (13, b':'),
            (16, b':'),
            (19, b'Z'),
        ] {
            if b[i] != want {
                return Err(InvalidTimestamp);
            }
        }
        let number = |from: usize, to: usize| -> Result<u32, InvalidTimestamp> {
            b[from..to].iter().try_fold(0u32, |n, &c| {
                if c.is_ascii_digit() {
                    Ok(n * 10 + u32::from(c - b'0'))
                } else {
                    Err(InvalidTimestamp)
                }
            })
        };
        let year = number(0, 4)?;
        let month = number(5, 7)?;
        let day = number(8, 10)?;
        let (hour, minute, second) = (number(11, 13)?, number(14, 16)?, number(17, 19)?);
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => return Err(InvalidTimestamp),
        };
        if !(1..=days).contains(&day) || hour > 23 || minute > 59 || second > 59 {
            return Err(InvalidTimestamp);
        }
        Ok(UtcTimestamp(text.to_owned()))
    }

    /// The timestamp, as it is on the wire.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for UtcTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for UtcTimestamp {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        UtcTimestamp::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// A timestamp outside [`UtcTimestamp`]'s profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidTimestamp;

impl fmt::Display for InvalidTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a timestamp must be a real UTC time as YYYY-MM-DDTHH:MM:SSZ")
    }
}

impl std::error::Error for InvalidTimestamp {}

impl TlsRefusal {
    /// Whether `tls.set` journals this refusal, so a retry with the same `operation_id` replays
    /// it (X-02 §6.3). Only `tls_key_mismatch` is stored: it depends on the pair alone, which
    /// the fingerprint covers. `tls_invalid` isn't, so a retry with a fixed pair is checked as
    /// new; a pair the PEM reader refuses has no fingerprint at all and gets it at the journal
    /// lookup ([`TlsRefusal::TlsInvalid`]). The others depend on state outside the op that is
    /// expected to change (the stored and running origins, the clock, the stored
    /// configuration), so a retry is checked again. [`TlsRefusal::Unknown`] is never stored.
    pub fn is_stored(&self) -> bool {
        matches!(self, TlsRefusal::TlsKeyMismatch)
    }
}

impl From<InvalidPem> for TlsRefusal {
    /// A pair the PEM reader refuses has no fingerprint: `tls_invalid`, unstored, on every path.
    fn from(_: InvalidPem) -> Self {
        TlsRefusal::TlsInvalid
    }
}

impl From<InvalidListenConfig> for TlsRefusal {
    /// The invalid-configuration rule's refusal, as `tls.set` answers it.
    fn from(invalid: InvalidListenConfig) -> Self {
        match invalid {
            InvalidListenConfig::Listen(_) | InvalidListenConfig::Origin(_) => {
                TlsRefusal::OriginInvalid
            }
            InvalidListenConfig::Hsts => TlsRefusal::HstsInvalid,
        }
    }
}

/// A TLS pair's fingerprint (X-02 §6.2, §6.3): the SHA-256, as lowercase hex, of the certificate
/// chain (every `CERTIFICATE` block's DER, each with a u32 big-endian length prefix, in order) and
/// of the DER of the key's first `PRIVATE KEY`,
/// `RSA PRIVATE KEY` or `EC PRIVATE KEY` block, each read by [`crate::pem`]. It holds no secret,
/// so the journal can keep it: it is `tls.set`'s journal fingerprint fields, serialized as
/// `{"cert_chain_sha256", "key_der_sha256"}` in that order. The same DER fingerprints the same however its text is
/// wrapped, whatever its line ends (LF, CRLF or a lone CR) and whatever text surrounds the blocks; every
/// `CERTIFICATE` block in the chain must frame, and the whole chain is hashed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct TlsPairFingerprint {
    cert_chain_sha256: String,
    key_der_sha256: String,
}

impl TlsPairFingerprint {
    /// The one implementation, for `TlsSetParams`' [`crate::journal::Journaled`]
    /// implementation. The decoded key bytes are zeroed once hashed
    /// (best effort: `sha2`'s own block buffer isn't). Its error never quotes the pair.
    pub(crate) fn of(cert: &str, key: &TlsPrivateKey) -> Result<Self, InvalidPem> {
        // The whole chain, as the host reads it with `all_blocks`: each block's DER with a u32
        // big-endian length prefix, in order, so a changed intermediate changes the fingerprint.
        let chain = pem::all_blocks(cert, pem::CERTIFICATE)?;
        let mut hasher = Sha256::new();
        for der in &chain {
            let len = u32::try_from(der.bytes().len()).map_err(|_| InvalidPem)?;
            hasher.update(len.to_be_bytes());
            hasher.update(der.bytes());
        }
        let cert_chain_sha256 = hex(&hasher.finalize());
        let key_der_sha256 = {
            // Dropped, and so zeroed, at the end of this block.
            let key_der = pem::first_block(key.expose(), pem::PRIVATE_KEY)?;
            sha256_hex(key_der.bytes())
        };
        Ok(TlsPairFingerprint {
            cert_chain_sha256,
            key_der_sha256,
        })
    }

    /// The SHA-256 of the length-prefixed certificate chain, lowercase hex.
    pub fn cert_chain_sha256(&self) -> &str {
        &self.cert_chain_sha256
    }

    /// The SHA-256 of the key's DER, lowercase hex.
    pub fn key_der_sha256(&self) -> &str {
        &self.key_der_sha256
    }
}

/// SHA-256 as lowercase hex.
pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// Lowercase hex.
pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::with_capacity(64), |mut hex, b| {
        let _ = write!(hex, "{b:02x}");
        hex
    })
}
