//! The browser listener's configuration (X-02 §2, "Listen configuration"): the listen address,
//! port and scheme, the browser-facing `origin` and the `hsts` setting, and the one
//! implementation that parses, checks and serialises `origin`, [`ListenOrigin`].
//!
//! Setup (B2-02) writes the five values to the instance configuration; the host (B2-03) reads
//! them **once per bind** into a snapshot ([`ListenConfig`]) that it pairs with the listener and
//! serves in `tls.fetch`; the gateway checks the origin it is given against its listener. All
//! three use [`ListenOrigin`], never a copy of their own.
//!
//! **An invalid stored configuration is one rule everywhere** ([`StoredListenConfig::validate`]):
//! an `origin` that fails [`ListenOrigin::parse`] or [`ListenOrigin::check`], or a missing or
//! malformed `address`, `port` or `scheme` it can't be checked against, is `origin_invalid`; an
//! `hsts` value outside `off`/`on`/`clear` is `hsts_invalid`. The host's pre-spawn check
//! reports it as a [`ListenFailure`]; `tls.set` refuses with `ops::TlsRefusal`
//! (`ops` depends on this module, not the other way round). Certificate coverage is the host's
//! check, not this module's.

use std::fmt;
use std::net::IpAddr;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

mod origin;

pub use origin::{ipv6_whatwg, ListenOrigin, OriginError, OriginHost, OriginScheme};

/// The listen scheme: the instance configuration's `scheme` and `tls.fetch`'s `mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListenMode {
    /// TLS, with the stored pair.
    Https,
    /// Plain HTTP, bound to exactly `127.0.0.1` or `::1`.
    HttpLoopback,
    /// Plain HTTP on a listen address that is not a loopback address, for one case only: an
    /// instance that runs in the foreground as a container's main process, whose port the
    /// container runtime publishes on the Docker host's own loopback
    /// (`-p 127.0.0.1:8443:8443`). The browser-facing origin is therefore that address,
    /// `http://127.0.0.1:<port>`, and nothing else is accepted as one: not the name
    /// `localhost`, which can resolve to `::1`, where the port is not published and another
    /// program of the machine could listen, and not `[::1]` either. It is chosen
    /// by name in a setup file, never implied by `http` and an address, and only an instance
    /// whose service mode is `foreground` may have it ([`ListenMode::fits_service`]).
    HttpPublished,
}

impl ListenMode {
    /// The origin scheme this mode serves: `https` or `http`.
    pub fn scheme(self) -> OriginScheme {
        match self {
            ListenMode::Https => OriginScheme::Https,
            ListenMode::HttpLoopback | ListenMode::HttpPublished => OriginScheme::Http,
        }
    }

    /// The wire spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            ListenMode::Https => "https",
            ListenMode::HttpLoopback => "http_loopback",
            ListenMode::HttpPublished => "http_published",
        }
    }

    /// Whether an instance may have a listener of this mode, given whether its service mode
    /// is `foreground` (nothing is registered; a container runtime is its supervisor). The
    /// one rule: `http_published` is for a foreground instance only. A registered service
    /// runs on a machine, where a port bound off loopback is on that machine's networks;
    /// in a container the runtime decides where the port is published. The service mode is
    /// data of the setup record: nothing detects a container.
    pub fn fits_service(self, foreground: bool) -> Result<(), ForegroundOnly> {
        match (self, foreground) {
            (ListenMode::HttpPublished, false) => Err(ForegroundOnly),
            _ => Ok(()),
        }
    }
}

/// `http_published` on an instance whose service mode is not `foreground`
/// ([`ListenMode::fits_service`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ForegroundOnly;

impl fmt::Display for ForegroundOnly {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "the scheme http_published (plain HTTP on a published port) is only for an instance \
             whose service mode is foreground, the main process of a container; a registered \
             service listens with http_loopback or https",
        )
    }
}

impl std::error::Error for ForegroundOnly {}

/// The `hsts` setting: `off`, `on` or `clear`. X-02 carries the value only; what it means for
/// `Strict-Transport-Security` is B1-05's header rules. `off` is what setup writes when the
/// operator chooses none, but a stored configuration has no default: a missing or other value is
/// `hsts_invalid` ([`StoredListenConfig::validate`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hsts {
    Off,
    On,
    Clear,
}

impl Hsts {
    /// The wire spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Hsts::Off => "off",
            Hsts::On => "on",
            Hsts::Clear => "clear",
        }
    }
}

impl FromStr for Hsts {
    type Err = InvalidListenConfig;

    /// Exactly `off`, `on` or `clear`; anything else, other cases included, is `hsts_invalid`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "off" => Ok(Hsts::Off),
            "on" => Ok(Hsts::On),
            "clear" => Ok(Hsts::Clear),
            _ => Err(InvalidListenConfig::Hsts),
        }
    }
}

impl fmt::Display for Hsts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The listen configuration, validated: the **per-bind snapshot** the host reads once at each
/// bind, pairs with that listener, and serves in every `tls.fetch` from a gateway holding that
/// listener (X-02 §2). A re-bind takes a fresh snapshot.
///
/// Every value of this type has passed [`StoredListenConfig::validate`]: the origin is
/// canonical and fits the scheme, address and port. Changing any of the five values is a
/// reconfigure followed by a host restart.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ListenConfig {
    address: IpAddr,
    port: u16,
    scheme: ListenMode,
    origin: ListenOrigin,
    hsts: Hsts,
}

impl ListenConfig {
    /// The address the host binds.
    pub fn address(&self) -> IpAddr {
        self.address
    }

    /// The port the host binds, which is also the origin's port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The listen scheme: `tls.fetch`'s `mode`.
    pub fn scheme(&self) -> ListenMode {
        self.scheme
    }

    pub fn origin(&self) -> &ListenOrigin {
        &self.origin
    }

    pub fn hsts(&self) -> Hsts {
        self.hsts
    }

    /// The configuration as it is stored.
    pub fn to_stored(&self) -> StoredListenConfig {
        StoredListenConfig {
            address: Some(Stored::Value(self.address)),
            port: Some(Stored::Value(self.port)),
            scheme: Some(Stored::Value(self.scheme)),
            origin: Some(Stored::Value(self.origin.as_str().to_owned())),
            hsts: Some(Stored::Value(self.hsts.as_str().to_owned())),
        }
    }
}

impl Serialize for ListenConfig {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.to_stored().serialize(s)
    }
}

impl<'de> Deserialize<'de> for ListenConfig {
    /// Decodes the stored form and validates it; an invalid one is a decode error. To report
    /// `origin_invalid` or `hsts_invalid`, decode a [`StoredListenConfig`] and validate it.
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        StoredListenConfig::deserialize(d)?
            .validate()
            .map_err(serde::de::Error::custom)
    }
}

/// The listen configuration as setup stores it in the instance configuration, before
/// validation: every field is read without judging it, so a missing or invalid value is reported
/// as the one rule says rather than as an unreadable file. A missing field and `null` read as
/// `None`; a value of the wrong type or out of range reads as [`Stored::Malformed`]. Both are
/// invalid ([`StoredListenConfig::validate`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredListenConfig {
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub address: Option<Stored<IpAddr>>,
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub port: Option<Stored<u16>>,
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub scheme: Option<Stored<ListenMode>>,
    /// Always written by setup: in `http_loopback` it derives one when the operator gives none.
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub origin: Option<Stored<String>>,
    /// Always written by setup.
    #[serde(
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub hsts: Option<Stored<String>>,
}

/// A stored field as read, before validation: a value of the field's type, or one that isn't
/// (the wrong JSON type, an address that isn't an IP literal, a port outside `u16`, a scheme
/// other than `https` and `http_loopback`). A missing field and `null` are `None` around it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stored<T> {
    Value(T),
    /// Never written: serializing it fails.
    Malformed,
}

impl<T> Stored<T> {
    /// The value, if it is one.
    pub fn value(&self) -> Option<&T> {
        match self {
            Stored::Value(value) => Some(value),
            Stored::Malformed => None,
        }
    }
}

impl<T: Serialize> Serialize for Stored<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Stored::Value(value) => value.serialize(s),
            Stored::Malformed => Err(serde::ser::Error::custom(
                "a malformed stored field is never written",
            )),
        }
    }
}

/// A value of `T` as itself, `null` as `None`, and anything else as [`Stored::Malformed`].
fn lenient<'de, D, T>(d: D) -> Result<Option<Stored<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Lenient<T> {
        Value(T),
        Null(()),
        Other(serde::de::IgnoredAny),
    }
    Ok(match Lenient::<T>::deserialize(d)? {
        Lenient::Value(value) => Some(Stored::Value(value)),
        Lenient::Null(()) => None,
        Lenient::Other(_) => Some(Stored::Malformed),
    })
}

impl StoredListenConfig {
    /// The invalid-configuration rule (X-02 §2), checked in this order:
    /// 1. an `address`, `port` or `scheme` that is missing or malformed (port `0` included) is
    ///    [`InvalidListenConfig::Listen`], reported as `origin_invalid`, because `origin` can't
    ///    be checked against it;
    /// 2. an `origin` that is missing, isn't a string ([`OriginError::NotAString`]) or fails
    ///    [`ListenOrigin::parse`] or [`ListenOrigin::check`] (including the loopback address
    ///    narrowing) is [`InvalidListenConfig::Origin`];
    /// 3. an `hsts` that is missing, isn't a string or is outside `off`/`on`/`clear` is
    ///    [`InvalidListenConfig::Hsts`].
    ///
    /// There is no default for any of them. The first failure is reported.
    pub fn validate(&self) -> Result<ListenConfig, InvalidListenConfig> {
        fn listen<T: Copy>(
            stored: &Option<Stored<T>>,
            field: ListenField,
        ) -> Result<T, InvalidListenConfig> {
            stored
                .as_ref()
                .and_then(Stored::value)
                .copied()
                .ok_or(InvalidListenConfig::Listen(field))
        }
        let address = listen(&self.address, ListenField::Address)?;
        let port = listen(&self.port, ListenField::Port)?;
        if port == 0 {
            return Err(InvalidListenConfig::Listen(ListenField::Port));
        }
        let scheme = listen(&self.scheme, ListenField::Scheme)?;
        let origin = match &self.origin {
            None => return Err(InvalidListenConfig::Origin(OriginError::Missing)),
            Some(Stored::Malformed) => {
                return Err(InvalidListenConfig::Origin(OriginError::NotAString))
            }
            Some(Stored::Value(text)) => text,
        };
        let raw = origin;
        let origin = ListenOrigin::parse(raw).map_err(InvalidListenConfig::Origin)?;
        // The stored value is the serialization itself, never a form `parse` would rewrite.
        if origin.as_str() != raw.as_str() {
            return Err(InvalidListenConfig::Origin(OriginError::Path));
        }
        origin
            .check(scheme, address, port)
            .map_err(InvalidListenConfig::Origin)?;
        let hsts = self
            .hsts
            .as_ref()
            .and_then(Stored::value)
            .ok_or(InvalidListenConfig::Hsts)?
            .parse::<Hsts>()?;
        Ok(ListenConfig {
            address,
            port,
            scheme,
            origin,
            hsts,
        })
    }
}

/// The listener fields of a stored listen configuration, which `origin` is checked against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ListenField {
    Address,
    Port,
    Scheme,
}

impl ListenField {
    /// The field's name in the stored configuration.
    pub fn as_str(self) -> &'static str {
        match self {
            ListenField::Address => "address",
            ListenField::Port => "port",
            ListenField::Scheme => "scheme",
        }
    }
}

/// Why a stored listen configuration is invalid. The host's pre-spawn check reports it as
/// [`InvalidListenConfig::listen_failure`]; `tls.set` refuses with the
/// `TlsRefusal` it converts into (`From`) and store nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidListenConfig {
    /// `origin_invalid`: the listener field `origin` is checked against is missing or malformed
    /// (X-02 §2). Setup's message names that field.
    Listen(ListenField),
    /// `origin_invalid`, with the check that failed ([`OriginError::Missing`] when there is none).
    Origin(OriginError),
    /// `hsts_invalid`: missing, or not `off`, `on` or `clear`.
    Hsts,
}

impl InvalidListenConfig {
    /// The field setup names in "the listen configuration is invalid: `<field>`".
    pub fn field(self) -> &'static str {
        match self {
            InvalidListenConfig::Listen(field) => field.as_str(),
            InvalidListenConfig::Origin(_) => "origin",
            InvalidListenConfig::Hsts => "hsts",
        }
    }

    /// As status, `doctor` and `tls.fetch`'s `unavailable` report it.
    pub fn listen_failure(self) -> ListenFailure {
        match self {
            InvalidListenConfig::Listen(_) | InvalidListenConfig::Origin(_) => {
                ListenFailure::OriginInvalid
            }
            InvalidListenConfig::Hsts => ListenFailure::HstsInvalid,
        }
    }
}

impl fmt::Display for InvalidListenConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InvalidListenConfig::Listen(field) => write!(
                f,
                "origin_invalid: the listen {} is missing or malformed",
                field.as_str()
            ),
            InvalidListenConfig::Origin(why) => write!(f, "origin_invalid: {why}"),
            InvalidListenConfig::Hsts => f.write_str("hsts_invalid: hsts must be off, on or clear"),
        }
    }
}

impl std::error::Error for InvalidListenConfig {}

/// `ListenFailure` (X-02 §2, rev 14): the one type for a listen failure in status, `doctor`
/// and `tls.fetch`'s `unavailable`: `{"reason": "origin_invalid"}`, `{"reason": "hsts_invalid"}`
/// or `{"reason": "tls_name_mismatch", "uncovered": [...]}`, where `uncovered` holds the
/// snapshot's origin. These are status reasons, not §9 error codes. The host reports them at
/// once from its pre-spawn check (or the same failure from `tls.fetch`'s re-check), without
/// repeated start failures.
///
/// **The pre-spawn check** (move 2b) runs on the snapshot **before binding**, so a hand-edited
/// `http_loopback` configuration with a non-loopback address is never bound, even briefly. Its
/// order, first failure reported: the listener fields and `origin` (`origin_invalid`), `hsts`
/// (`hsts_invalid`), in `https` a stored pair (`tls_missing`, a status reason that isn't a
/// `ListenFailure`), then coverage (`tls_name_mismatch`). On a failure the host neither binds
/// nor spawns.
///
/// `tls.fetch`'s `unavailable` carries exactly one of these three, so any other reason is
/// refused at decode. Status and `doctor` report `gateway_failed` as a `ListenFailure` or as rev
/// 13's `{"reason": "start_failed"}`; that status type is move 2b's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum ListenFailure {
    /// The stored `origin` is missing, isn't a string or fails [`ListenOrigin`]'s checks, or a
    /// stored `address`, `port` or `scheme` it is checked against is missing or malformed.
    OriginInvalid,
    /// The stored `hsts` is missing, isn't a string or isn't `off`, `on` or `clear`.
    HstsInvalid,
    /// In `https`, the stored pair doesn't cover the snapshot's origin.
    TlsNameMismatch { uncovered: Uncovered },
}

/// The origins a TLS pair doesn't cover: never empty, canonical, without duplicates. In a
/// `tls.set` refusal, the stored origin first, then the running one; in [`ListenFailure`], the
/// snapshot's origin. Decoding refuses an empty list or a duplicate.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct Uncovered(Vec<ListenOrigin>);

impl Uncovered {
    /// The list, in order. Refused when it is empty or has a duplicate.
    pub fn new(origins: Vec<ListenOrigin>) -> Result<Uncovered, &'static str> {
        if origins.is_empty() {
            return Err("uncovered is never empty");
        }
        for (i, origin) in origins.iter().enumerate() {
            if origins[..i].contains(origin) {
                return Err("uncovered lists each origin once");
            }
        }
        Ok(Uncovered(origins))
    }

    /// One uncovered origin.
    pub fn one(origin: ListenOrigin) -> Uncovered {
        Uncovered(vec![origin])
    }

    pub fn origins(&self) -> &[ListenOrigin] {
        &self.0
    }
}

impl<'de> Deserialize<'de> for Uncovered {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Uncovered::new(Vec::<ListenOrigin>::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}
