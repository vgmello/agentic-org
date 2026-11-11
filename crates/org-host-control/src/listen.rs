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
//! reports it as a [`ListenFailure`]; `bootstrap` and `tls.set` refuse with `ops::TlsRefusal`
//! (`ops` depends on this module, not the other way round). Certificate coverage is the host's
//! check, not this module's.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The listen scheme: the instance configuration's `scheme` and `tls.fetch`'s `mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListenMode {
    /// TLS, with the stored pair. The only mode in which the host binds a non-loopback listener.
    Https,
    /// Plain HTTP, bound to exactly `127.0.0.1` or `::1`.
    HttpLoopback,
}

impl ListenMode {
    /// The origin scheme this mode serves: `https` or `http`.
    pub fn scheme(self) -> OriginScheme {
        match self {
            ListenMode::Https => OriginScheme::Https,
            ListenMode::HttpLoopback => OriginScheme::Http,
        }
    }
}

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

/// An origin's scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OriginScheme {
    Http,
    Https,
}

impl OriginScheme {
    pub fn as_str(self) -> &'static str {
        match self {
            OriginScheme::Http => "http",
            OriginScheme::Https => "https",
        }
    }

    /// The port browsers leave out of the origin: 80 for `http`, 443 for `https`.
    pub fn default_port(self) -> u16 {
        match self {
            OriginScheme::Http => 80,
            OriginScheme::Https => 443,
        }
    }
}

/// An origin's host, as [`ListenOrigin::parse`] accepted it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum OriginHost {
    /// Lowercase ASCII labels of letters, digits, `-` and `_`, without a trailing dot. An
    /// internationalised name is its `xn--` A-label.
    Domain(String),
    Ipv4(Ipv4Addr),
    Ipv6(Ipv6Addr),
}

impl fmt::Display for OriginHost {
    /// The host as it appears in the origin: an IPv6 address in brackets, in WHATWG's
    /// serialization ([`ipv6_whatwg`]).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OriginHost::Domain(name) => f.write_str(name),
            OriginHost::Ipv4(addr) => write!(f, "{addr}"),
            OriginHost::Ipv6(addr) => write!(f, "[{}]", ipv6_whatwg(addr)),
        }
    }
}

/// The browser-facing origin of the gateway, as browsers send it in `Host` and `Origin`:
/// always the **WHATWG URL origin serialization** (`new URL(input).origin`), so the Host and
/// Origin checks and the HSTS rule can compare exact strings.
///
/// [`ListenOrigin::parse`] accepts only input already in that form and refuses everything else,
/// never rewriting it; the one allowance is a single trailing `/`, which isn't part of the origin.
/// Its `Display` and serde form is the canonical string, so it round-trips. Serde decodes from a
/// string through `parse`.
///
/// It is stricter than browsers in three places, by decision (X-02 §2): a trailing dot in a
/// domain is refused; a last label that is all digits or starts with `0x`/`0X` must be part of a
/// canonical dotted quad (so `foo.0xg`, a domain to browsers, is refused); and a domain is only
/// lowercase letters, digits, `-` and `_`, in labels of 1 to 63 bytes, at most 253 in total.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ListenOrigin {
    scheme: OriginScheme,
    host: OriginHost,
    /// `None` for the scheme's default port.
    port: Option<u16>,
    serialized: String,
}

/// Why an origin was refused: by [`ListenOrigin::parse`] (its canonical form) or by
/// [`ListenOrigin::check`] (its fit to the listen configuration). Either is `origin_invalid` in
/// a stored configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum OriginError {
    /// A stored listen configuration has no `origin`.
    Missing,
    /// A stored listen configuration's `origin` isn't a JSON string (a number, a boolean, an
    /// array or an object).
    NotAString,
    /// Empty input.
    Empty,
    /// A byte outside ASCII. An internationalised name is configured as its `xn--` A-label.
    NonAscii,
    /// Not `scheme "://" authority`.
    NotAnOrigin,
    /// A scheme other than `http` or `https`.
    Scheme,
    /// An uppercase letter in the scheme or a domain.
    Uppercase,
    /// Userinfo (`user@`).
    Userinfo,
    /// A path other than one `/`, a query or a fragment.
    Path,
    /// A domain ending in a dot.
    TrailingDot,
    /// Not a domain, an IPv4 literal or a bracketed IPv6 literal: an empty host or label, a
    /// label over 63 bytes, a name over 253, a character outside `a-z 0-9 - _`, a zone ID, or a
    /// numeric last label in a name that isn't an address (`foo.123`).
    InvalidHost,
    /// An IPv4 address not written as a canonical dotted quad (`127.1`, `2130706433`,
    /// `0x7f.0.0.1`, `0177.0.0.1`, `127.000.000.001`): browsers send the dotted quad.
    NonCanonicalIpv4,
    /// An IPv6 address not in WHATWG's serialization: uppercase, leading zeros, an uncompressed
    /// or wrongly compressed zero run, or a dotted IPv4 tail.
    NonCanonicalIpv6,
    /// An explicit `:80` on `http` or `:443` on `https`.
    DefaultPort,
    /// An empty port, a leading zero, a non-digit, or a port outside 1 to 65535.
    NonCanonicalPort,
    /// Fit: the scheme isn't the mode's (`https` for `https`, `http` for `http_loopback`).
    SchemeMismatch,
    /// Fit: the port (the default when omitted) isn't the listen port.
    PortMismatch,
    /// Fit, `http_loopback`: the listen address isn't exactly `127.0.0.1` or `::1`.
    LoopbackAddress,
    /// Fit, `http_loopback`: the host isn't the bound IP literal itself (plan A-10).
    NotBoundLiteral,
}

impl OriginError {
    /// A stable name for the reason, as the vectors spell it.
    pub fn code(self) -> &'static str {
        match self {
            OriginError::Missing => "missing",
            OriginError::NotAString => "not_a_string",
            OriginError::Empty => "empty",
            OriginError::NonAscii => "non_ascii",
            OriginError::NotAnOrigin => "not_an_origin",
            OriginError::Scheme => "scheme",
            OriginError::Uppercase => "uppercase",
            OriginError::Userinfo => "userinfo",
            OriginError::Path => "path",
            OriginError::TrailingDot => "trailing_dot",
            OriginError::InvalidHost => "invalid_host",
            OriginError::NonCanonicalIpv4 => "non_canonical_ipv4",
            OriginError::NonCanonicalIpv6 => "non_canonical_ipv6",
            OriginError::DefaultPort => "default_port",
            OriginError::NonCanonicalPort => "non_canonical_port",
            OriginError::SchemeMismatch => "scheme_mismatch",
            OriginError::PortMismatch => "port_mismatch",
            OriginError::LoopbackAddress => "loopback_address",
            OriginError::NotBoundLiteral => "not_bound_literal",
        }
    }
}

impl fmt::Display for OriginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            OriginError::Missing => "the origin is missing",
            OriginError::NotAString => "the origin isn't a string",
            OriginError::Empty => "the origin is empty",
            OriginError::NonAscii => "the origin isn't ASCII (use the xn-- A-label)",
            OriginError::NotAnOrigin => "the origin isn't scheme://host[:port]",
            OriginError::Scheme => "the origin's scheme isn't http or https",
            OriginError::Uppercase => "the origin has an uppercase letter",
            OriginError::Userinfo => "the origin has userinfo",
            OriginError::Path => "the origin has a path, a query or a fragment",
            OriginError::TrailingDot => "the origin's host ends in a dot",
            OriginError::InvalidHost => "the origin's host isn't a valid host",
            OriginError::NonCanonicalIpv4 => "the origin's IPv4 address isn't a dotted quad",
            OriginError::NonCanonicalIpv6 => {
                "the origin's IPv6 address isn't in its canonical form"
            }
            OriginError::DefaultPort => "the origin names the scheme's default port",
            OriginError::NonCanonicalPort => "the origin's port isn't 1 to 65535 in canonical form",
            OriginError::SchemeMismatch => "the origin's scheme doesn't match the listen scheme",
            OriginError::PortMismatch => "the origin's port isn't the listen port",
            OriginError::LoopbackAddress => "http_loopback must listen on exactly 127.0.0.1 or ::1",
            OriginError::NotBoundLiteral => {
                "in http_loopback the origin's host must be the bound IP literal"
            }
        })
    }
}

impl std::error::Error for OriginError {}

impl ListenOrigin {
    /// Accept `input` only when it is already the canonical WHATWG origin serialization,
    /// optionally followed by a single `/`. Refused, never rewritten, otherwise.
    pub fn parse(input: &str) -> Result<ListenOrigin, OriginError> {
        if input.is_empty() {
            return Err(OriginError::Empty);
        }
        if !input.is_ascii() {
            return Err(OriginError::NonAscii);
        }
        let (scheme, rest) = input.split_once("://").ok_or(OriginError::NotAnOrigin)?;
        let scheme = match scheme {
            "http" => OriginScheme::Http,
            "https" => OriginScheme::Https,
            s if s.eq_ignore_ascii_case("http") || s.eq_ignore_ascii_case("https") => {
                return Err(OriginError::Uppercase)
            }
            _ => return Err(OriginError::Scheme),
        };
        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let (authority, tail) = rest.split_at(end);
        if !tail.is_empty() && tail != "/" {
            return Err(OriginError::Path);
        }
        if authority.contains('@') {
            return Err(OriginError::Userinfo);
        }
        let (host, port) = split_host_port(authority)?;
        let port = port.map(parse_port).transpose()?;
        let host = parse_host(host)?;
        if port == Some(scheme.default_port()) {
            return Err(OriginError::DefaultPort);
        }
        let serialized = &input[..input.len() - tail.len()];
        let origin = ListenOrigin {
            scheme,
            host,
            port,
            serialized: serialized.to_owned(),
        };
        // Every component was checked to be canonical, so serialising gives back the input. Kept
        // as a real check, so a later change to a component rule can't let a non-canonical
        // string through in a release build.
        if origin.serialize() != origin.serialized {
            return Err(OriginError::InvalidHost);
        }
        Ok(origin)
    }

    /// Check the origin's fit to the listen configuration it is served from (X-02 §2, step 2):
    /// - the scheme is the mode's: `https` for `https`, `http` for `http_loopback`;
    /// - the port (the default when omitted) is the listen port: it is the browser-facing port,
    ///   and v1 has no reverse proxy in front of the gateway;
    /// - in `http_loopback`, the listen address itself is exactly `127.0.0.1` or `::1`;
    /// - in `http_loopback`, the host is exactly that bound IP literal, never a name such as
    ///   `localhost` (plan A-10: a name could resolve to an address another local process
    ///   holds).
    ///
    /// In `https` any canonical host fits, IP literals included. Certificate coverage isn't part
    /// of this check. The first failure, in that order, is returned.
    pub fn check(&self, mode: ListenMode, address: IpAddr, port: u16) -> Result<(), OriginError> {
        if self.scheme != mode.scheme() {
            return Err(OriginError::SchemeMismatch);
        }
        if self.port() != port {
            return Err(OriginError::PortMismatch);
        }
        if mode == ListenMode::HttpLoopback {
            let loopback = address == IpAddr::V4(Ipv4Addr::LOCALHOST)
                || address == IpAddr::V6(Ipv6Addr::LOCALHOST);
            if !loopback {
                return Err(OriginError::LoopbackAddress);
            }
            let literal = match (&self.host, address) {
                (OriginHost::Ipv4(host), IpAddr::V4(bound)) => *host == bound,
                (OriginHost::Ipv6(host), IpAddr::V6(bound)) => *host == bound,
                _ => false,
            };
            if !literal {
                return Err(OriginError::NotBoundLiteral);
            }
        }
        Ok(())
    }

    pub fn scheme(&self) -> OriginScheme {
        self.scheme
    }

    pub fn host(&self) -> &OriginHost {
        &self.host
    }

    /// The port browsers connect to: the explicit one, or the scheme's default.
    pub fn port(&self) -> u16 {
        self.port.unwrap_or(self.scheme.default_port())
    }

    /// The canonical origin string.
    pub fn as_str(&self) -> &str {
        &self.serialized
    }

    fn serialize(&self) -> String {
        match self.port {
            Some(port) => format!("{}://{}:{port}", self.scheme.as_str(), self.host),
            None => format!("{}://{}", self.scheme.as_str(), self.host),
        }
    }
}

impl fmt::Display for ListenOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.serialized)
    }
}

impl FromStr for ListenOrigin {
    type Err = OriginError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ListenOrigin::parse(s)
    }
}

impl Serialize for ListenOrigin {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.serialized)
    }
}

impl<'de> Deserialize<'de> for ListenOrigin {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        ListenOrigin::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// Split an authority into its host and, after a `:`, its port text. A bracketed host is an
/// IPv6 literal and keeps its brackets.
fn split_host_port(authority: &str) -> Result<(&str, Option<&str>), OriginError> {
    if authority.starts_with('[') {
        let close = authority.find(']').ok_or(OriginError::InvalidHost)?;
        let (host, after) = authority.split_at(close + 1);
        match after {
            "" => Ok((host, None)),
            _ => match after.strip_prefix(':') {
                Some(port) => Ok((host, Some(port))),
                None => Err(OriginError::InvalidHost),
            },
        }
    } else {
        Ok(match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        })
    }
}

/// 1 to 65535 in decimal, without leading zeros.
fn parse_port(text: &str) -> Result<u16, OriginError> {
    let digits = !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    if !digits || text.starts_with('0') || text.len() > 5 {
        return Err(OriginError::NonCanonicalPort);
    }
    text.parse::<u16>()
        .map_err(|_| OriginError::NonCanonicalPort)
}

fn parse_host(host: &str) -> Result<OriginHost, OriginError> {
    if let Some(inner) = host.strip_prefix('[') {
        let inner = inner.strip_suffix(']').ok_or(OriginError::InvalidHost)?;
        let addr = Ipv6Addr::from_str(inner).map_err(|_| OriginError::InvalidHost)?;
        if ipv6_whatwg(&addr) != inner {
            return Err(OriginError::NonCanonicalIpv6);
        }
        return Ok(OriginHost::Ipv6(addr));
    }
    if host.is_empty() {
        return Err(OriginError::InvalidHost);
    }
    if host.ends_with('.') {
        return Err(OriginError::TrailingDot);
    }
    let last = host.rsplit('.').next().unwrap_or(host);
    if is_ipv4_attempt(last) {
        return parse_ipv4(host).map(OriginHost::Ipv4);
    }
    if host.bytes().any(|b| b.is_ascii_uppercase()) {
        return Err(OriginError::Uppercase);
    }
    let label_ok = |label: &str| {
        (1..=63).contains(&label.len())
            && label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
    };
    if host.len() > 253 || !host.split('.').all(label_ok) {
        return Err(OriginError::InvalidHost);
    }
    Ok(OriginHost::Domain(host.to_owned()))
}

/// X-02 §2: a last label that is all ASCII digits or starts with `0x`/`0X` makes the host an
/// IPv4 attempt. This is broader than WHATWG's "ends in a number" (which needs valid hex after
/// `0x`), so `foo.0xg` is an attempt here and a domain to browsers.
fn is_ipv4_attempt(last: &str) -> bool {
    let digits = !last.is_empty() && last.bytes().all(|b| b.is_ascii_digit());
    digits || last.starts_with("0x") || last.starts_with("0X")
}

/// An IPv4 attempt must be a canonical dotted quad: four decimal parts, 0 to 255, without
/// leading zeros. A host whose every label is a number in some IPv4 notation (decimal, octal
/// with a leading 0, or hex with `0x`) is an address written another way: browsers would send
/// its dotted quad. Anything else (`foo.123`) is no valid host.
fn parse_ipv4(host: &str) -> Result<Ipv4Addr, OriginError> {
    let labels: Vec<&str> = host.split('.').collect();
    let canonical_part = |part: &&str| {
        let digits = (1..=3).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_digit());
        digits
            && (*part == "0" || !part.starts_with('0'))
            && part.parse::<u16>().is_ok_and(|n| n <= 255)
    };
    if labels.len() == 4 && labels.iter().all(canonical_part) {
        let mut octets = [0u8; 4];
        for (octet, part) in octets.iter_mut().zip(&labels) {
            *octet = part.parse().map_err(|_| OriginError::NonCanonicalIpv4)?;
        }
        return Ok(Ipv4Addr::from(octets));
    }
    let numeric = |label: &&str| {
        let hex = label
            .strip_prefix("0x")
            .or_else(|| label.strip_prefix("0X"))
            .map(|h| h.bytes().all(|b| b.is_ascii_hexdigit()));
        match hex {
            Some(valid) => valid,
            None => !label.is_empty() && label.bytes().all(|b| b.is_ascii_digit()),
        }
    };
    if labels.iter().all(numeric) {
        Err(OriginError::NonCanonicalIpv4)
    } else {
        Err(OriginError::InvalidHost)
    }
}

/// WHATWG's IPv6 serializer (URL Standard, "IPv6 serializer"): eight 16-bit pieces in lowercase
/// hex without leading zeros, the **first longest** run of two or more zero pieces written as
/// `::`, and never a dotted IPv4 tail. Not Rust's `Ipv6Addr` `Display`, which prints an
/// IPv4-mapped address as `::ffff:127.0.0.1`.
pub fn ipv6_whatwg(addr: &Ipv6Addr) -> String {
    let pieces = addr.segments();
    // The first longest run of zero pieces, if it is at least two long.
    let mut compress: Option<(usize, usize)> = None;
    let mut i = 0;
    while i < pieces.len() {
        if pieces[i] == 0 {
            let start = i;
            while i < pieces.len() && pieces[i] == 0 {
                i += 1;
            }
            let len = i - start;
            if len >= 2 && compress.is_none_or(|(_, best)| len > best) {
                compress = Some((start, len));
            }
        } else {
            i += 1;
        }
    }
    let mut out = String::with_capacity(39);
    let mut i = 0;
    while i < pieces.len() {
        if let Some((start, len)) = compress.filter(|&(start, _)| start == i) {
            out.push_str(if start == 0 { "::" } else { ":" });
            i += len;
            continue;
        }
        out.push_str(&format!("{:x}", pieces[i]));
        if i != pieces.len() - 1 {
            out.push(':');
        }
        i += 1;
    }
    out
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
        let origin = ListenOrigin::parse(origin).map_err(InvalidListenConfig::Origin)?;
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
/// [`InvalidListenConfig::listen_failure`]; `bootstrap` and `tls.set` refuse with the
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
/// (`hsts_invalid`), in
/// `https` a stored pair (`tls_missing`, a status reason that isn't a `ListenFailure`), then
/// coverage (`tls_name_mismatch`). On a failure the host neither binds nor spawns.
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
