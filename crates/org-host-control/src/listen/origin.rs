//! The browser-facing origin: parsing and checking its scheme, host and port (X-02 §2), and WHATWG
//! IPv6 serialization.

use super::*;

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
    /// Fit, `http_published`: the listen address is a loopback address. A loopback listener
    /// is `http_loopback`'s.
    PublishedAddress,
    /// Fit, `http_published`: the host isn't `127.0.0.1`, the address the port is published
    /// on.
    PublishedHost,
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
            OriginError::PublishedAddress => "published_address",
            OriginError::PublishedHost => "published_host",
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
            OriginError::PublishedAddress => {
                "http_published is for a listen address that is not a loopback address; on \
                 127.0.0.1 or ::1 the scheme is http_loopback"
            }
            OriginError::PublishedHost => {
                "in http_published the origin's host must be 127.0.0.1: the port is published on \
                 the Docker host's 127.0.0.1, and a name such as localhost can resolve to ::1, \
                 where it is not"
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
    /// - in `http_published`, the listen address is not a loopback address (a loopback
    ///   listener is `http_loopback`'s, with its stricter host rule);
    /// - in `http_published`, the host is exactly the IPv4 literal `127.0.0.1`: the browser
    ///   reaches the listener through a port published on its own machine's `127.0.0.1`, so
    ///   no other host is the listener's (plan A-10's rule, as in `http_loopback`: the name
    ///   `localhost` can resolve to `::1`, where the port is not published and another local
    ///   program could listen), and a request that names another host is not for this
    ///   origin.
    ///
    /// In `https` any canonical host fits, IP literals included. Certificate coverage isn't part
    /// of this check. The first failure, in that order, is returned. Whether the instance may
    /// have the mode at all is [`ListenMode::fits_service`].
    pub fn check(&self, mode: ListenMode, address: IpAddr, port: u16) -> Result<(), OriginError> {
        if self.scheme != mode.scheme() {
            return Err(OriginError::SchemeMismatch);
        }
        if self.port() != port {
            return Err(OriginError::PortMismatch);
        }
        if mode == ListenMode::HttpPublished {
            if address.is_loopback() {
                return Err(OriginError::PublishedAddress);
            }
            if self.host != OriginHost::Ipv4(Ipv4Addr::LOCALHOST) {
                return Err(OriginError::PublishedHost);
            }
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

    pub(super) fn serialize(&self) -> String {
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
        let origin = ListenOrigin::parse(&text).map_err(serde::de::Error::custom)?;
        // On the wire the origin is its serialization exactly: no trailing `/`, never rewritten.
        if origin.as_str() != text {
            return Err(serde::de::Error::custom(OriginError::Path));
        }
        Ok(origin)
    }
}

/// Split an authority into its host and, after a `:`, its port text. A bracketed host is an
/// IPv6 literal and keeps its brackets.
pub(super) fn split_host_port(authority: &str) -> Result<(&str, Option<&str>), OriginError> {
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
pub(super) fn parse_port(text: &str) -> Result<u16, OriginError> {
    let digits = !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    if !digits || text.starts_with('0') || text.len() > 5 {
        return Err(OriginError::NonCanonicalPort);
    }
    text.parse::<u16>()
        .map_err(|_| OriginError::NonCanonicalPort)
}

pub(super) fn parse_host(host: &str) -> Result<OriginHost, OriginError> {
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
pub(super) fn is_ipv4_attempt(last: &str) -> bool {
    let digits = !last.is_empty() && last.bytes().all(|b| b.is_ascii_digit());
    digits || last.starts_with("0x") || last.starts_with("0X")
}

/// An IPv4 attempt must be a canonical dotted quad: four decimal parts, 0 to 255, without
/// leading zeros. A host whose every label is a number in some IPv4 notation (decimal, octal
/// with a leading 0, or hex with `0x`) is an address written another way: browsers would send
/// its dotted quad. Anything else (`foo.123`) is no valid host.
pub(super) fn parse_ipv4(host: &str) -> Result<Ipv4Addr, OriginError> {
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
