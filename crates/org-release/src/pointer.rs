//! Pure `agentic-org-release-pointer-v1` verification (ADR-0009).
//!
//! Authenticates a signed channel release pointer (`pointer.v1.json` + raw 64-byte
//! `pointer.v1.sig`) against the exact same build-pinned [`TrustedKeys`] and
//! [`TrustedPolicy`] used for release manifests. A pointer never introduces or rotates
//! trust keys, never lowers `minimum_sequence`, and never grants execution authority on
//! its own: the referenced bundle must still pass full `verify_manifest` + payload checks.

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use ring::signature::{UnparsedPublicKey, ED25519};
use sha2::{Digest, Sha256};

use crate::{
    decode, decode_hex, valid_release_id, Architecture, Error, PayloadVerifier, Platform,
    TrustedKey, TrustedKeys, TrustedPolicy, MAX_PAYLOAD_BYTES,
};

/// Exact signed release pointer profile name.
pub const POINTER_FORMAT: &str = "agentic-org-release-pointer-v1";
/// Signed pointer JSON limit in UTF-8 bytes, inclusive.
pub const MAX_POINTER_BYTES: usize = 64 * 1024;
/// Maximum target download URL length in UTF-8 bytes, inclusive.
pub const MAX_URL_BYTES: usize = 2048;
/// Maximum channel name length in UTF-8 bytes, inclusive.
pub const MAX_CHANNEL_BYTES: usize = 64;

/// URL scheme constraint for release pointer bundle URLs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UrlPolicy {
    /// Production default: strict `https://` URLs only (no HTTP, no `file://`, no userinfo).
    HttpsOnly,
    /// Explicit test/offline harness opt-in: permits `https://`, loopback `http://`, and `file:///`.
    AllowLocalForTesting,
}

/// Authenticated download target for one `(platform, architecture)` pair.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PointerTarget {
    platform: Platform,
    architecture: Architecture,
    url: String,
    sha256: [u8; 32],
    size: u64,
}

impl PointerTarget {
    pub fn platform(&self) -> Platform {
        self.platform
    }

    pub fn architecture(&self) -> Architecture {
        self.architecture
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn sha256(&self) -> &[u8; 32] {
        &self.sha256
    }

    pub fn sha256_hex(&self) -> String {
        self.sha256.iter().map(|b| format!("{b:02x}")).collect()
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    /// Start a bounded streaming exact-size + SHA-256 check for the downloaded bundle archive.
    pub fn bundle_verifier(&self) -> PayloadVerifier {
        PayloadVerifier::new(self.size, self.sha256)
    }
}

/// Immutable verified pointer semantics, accessible only after authentication succeeds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pointer {
    channel: String,
    release_id: String,
    sequence: u64,
    key_id: String,
    published_at: String,
    target: PointerTarget,
}

impl Pointer {
    pub fn channel(&self) -> &str {
        &self.channel
    }

    pub fn release_id(&self) -> &str {
        &self.release_id
    }

    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    pub fn installer_contract(&self) -> u64 {
        1
    }

    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    pub fn published_at(&self) -> &str {
        &self.published_at
    }

    /// The exact target selected for the build policy's `(platform, architecture)`.
    pub fn target(&self) -> &PointerTarget {
        &self.target
    }
}

/// Exact signed pointer bytes, validated semantics, and selected host target.
#[derive(Clone, Debug)]
pub struct VerifiedPointer {
    pointer: Pointer,
    bytes: Box<[u8]>,
    sha256: [u8; 32],
}

impl VerifiedPointer {
    pub fn pointer(&self) -> &Pointer {
        &self.pointer
    }

    pub fn pointer_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn pointer_sha256(&self) -> &[u8; 32] {
        &self.sha256
    }

    /// Cross-bind an authenticated inner release manifest against this authenticated pointer.
    /// Prevents mix-and-match attacks pairing a newer pointer with an older/other-target bundle.
    pub fn verify_matches_manifest(&self, manifest: &crate::VerifiedManifest) -> Result<(), Error> {
        let inner = manifest.manifest();
        if inner.release_id() != self.pointer.release_id
            || inner.sequence() != self.pointer.sequence
            || inner.platform() != self.pointer.target.platform
            || inner.architecture() != self.pointer.target.architecture
        {
            return Err(Error::InvalidPointer);
        }
        Ok(())
    }
}

/// Authenticate exact UTF-8 `pointer.v1.json` bytes with raw 64-byte Ed25519 `pointer.v1.sig`,
/// enforcing the build-pinned [`TrustedKeys`], [`TrustedPolicy`], and [`UrlPolicy`].
pub fn verify_pointer(
    pointer_bytes: &[u8],
    signature: &[u8],
    policy: &TrustedPolicy,
    trusted_keys: &TrustedKeys,
    url_policy: UrlPolicy,
) -> Result<VerifiedPointer, Error> {
    let (raw, key) = authenticate_pointer(pointer_bytes, signature, trusted_keys)?;
    validate_pointer_policy(&raw, policy, key)?;
    let target = select_and_validate_targets(raw.targets, policy, url_policy)?;
    Ok(VerifiedPointer {
        pointer: Pointer {
            channel: raw.channel,
            release_id: raw.release_id,
            sequence: raw.sequence,
            key_id: raw.key_id,
            published_at: raw.published_at,
            target,
        },
        bytes: pointer_bytes.into(),
        sha256: Sha256::digest(pointer_bytes).into(),
    })
}

fn authenticate_pointer<'a>(
    pointer_bytes: &[u8],
    signature: &[u8],
    trusted_keys: &'a TrustedKeys,
) -> Result<(decode::Pointer, &'a TrustedKey), Error> {
    if pointer_bytes.len() > MAX_POINTER_BYTES {
        return Err(Error::PointerTooLarge);
    }
    if signature.len() != 64 {
        return Err(Error::InvalidSignatureLength);
    }
    let raw: decode::Pointer =
        decode::object_from_slice(pointer_bytes).map_err(|_| Error::InvalidPointer)?;
    decode_hex::<32>(&raw.key_id).map_err(|_| Error::InvalidKeyId)?;
    let key = trusted_keys
        .keys
        .get(&raw.key_id)
        .ok_or(Error::UnknownSigningKey)?;
    UnparsedPublicKey::new(&ED25519, key.public_key)
        .verify(pointer_bytes, signature)
        .map_err(|_| Error::InvalidSignature)?;
    Ok((raw, key))
}

fn validate_pointer_policy(
    raw: &decode::Pointer,
    policy: &TrustedPolicy,
    key: &TrustedKey,
) -> Result<(), Error> {
    validate_pointer_identity(raw)?;
    validate_pointer_sequence(raw.sequence, policy, key)
}

fn validate_pointer_identity(raw: &decode::Pointer) -> Result<(), Error> {
    if raw.format != POINTER_FORMAT || !valid_published_at(&raw.published_at) {
        return Err(Error::InvalidPointer);
    }
    if !valid_channel(&raw.channel) {
        return Err(Error::InvalidChannel);
    }
    if !valid_release_id(&raw.release_id) {
        return Err(Error::InvalidReleaseId);
    }
    if raw.installer_contract != 1 {
        return Err(Error::InstallerContractMismatch);
    }
    Ok(())
}

fn validate_pointer_sequence(
    sequence: u64,
    policy: &TrustedPolicy,
    key: &TrustedKey,
) -> Result<(), Error> {
    if sequence == 0 {
        return Err(Error::InvalidSequence);
    }
    if sequence < policy.minimum_sequence {
        return Err(Error::ReleaseBelowMinimum);
    }
    if sequence < key.min_sequence || key.max_sequence.is_some_and(|maximum| sequence > maximum) {
        return Err(Error::KeySequenceOutOfRange);
    }
    Ok(())
}

fn select_and_validate_targets(
    raw_targets: Vec<decode::PointerTarget>,
    policy: &TrustedPolicy,
    url_policy: UrlPolicy,
) -> Result<PointerTarget, Error> {
    if raw_targets.is_empty() {
        return Err(Error::MissingTarget);
    }
    let mut seen = BTreeSet::new();
    let mut selected = None;
    for raw in raw_targets {
        let key = (platform_ord(raw.platform), arch_ord(raw.architecture));
        if !seen.insert(key) {
            return Err(Error::DuplicateTarget);
        }
        validate_pointer_url(&raw.url, url_policy)?;
        let sha256 = decode_hex::<32>(&raw.sha256).map_err(|_| Error::InvalidDigest)?;
        if raw.size == 0 || raw.size > MAX_PAYLOAD_BYTES {
            return Err(Error::BundleSizeLimit);
        }
        if raw.platform == policy.platform && raw.architecture == policy.architecture {
            selected = Some(PointerTarget {
                platform: raw.platform,
                architecture: raw.architecture,
                url: raw.url,
                sha256,
                size: raw.size,
            });
        }
    }
    selected.ok_or(Error::TargetMismatch)
}

fn platform_ord(platform: Platform) -> u8 {
    match platform {
        Platform::Macos => 0,
        Platform::Linux => 1,
    }
}

fn arch_ord(arch: Architecture) -> u8 {
    match arch {
        Architecture::Aarch64 => 0,
        Architecture::X86_64 => 1,
    }
}

/// Validates a channel identifier: 1..=64 ASCII lowercase letters, digits, `-`, or `_`,
/// starting with an ASCII lowercase letter or digit.
pub fn valid_channel(channel: &str) -> bool {
    (1..=MAX_CHANNEL_BYTES).contains(&channel.len())
        && (channel.as_bytes()[0].is_ascii_lowercase() || channel.as_bytes()[0].is_ascii_digit())
        && channel
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_'))
}

/// Validates an RFC 3339 UTC timestamp (`YYYY-MM-DDTHH:MM:SSZ` or with fractional seconds).
fn valid_published_at(value: &str) -> bool {
    let bytes = value.as_bytes();
    (20..=32).contains(&bytes.len())
        && bytes.is_ascii()
        && has_rfc3339_utc_layout(bytes)
        && valid_fractional_seconds(&bytes[19..bytes.len() - 1])
        && valid_utc_calendar_fields(bytes)
}

fn has_rfc3339_utc_layout(bytes: &[u8]) -> bool {
    let separators = [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':')];
    let digit_ranges = [0..4, 5..7, 8..10, 11..13, 14..16, 17..19];
    bytes.last() == Some(&b'Z')
        && separators.iter().all(|&(idx, ch)| bytes[idx] == ch)
        && digit_ranges
            .into_iter()
            .all(|range| bytes[range].iter().all(u8::is_ascii_digit))
}

fn valid_fractional_seconds(tail: &[u8]) -> bool {
    tail.is_empty()
        || (tail.starts_with(b".") && tail.len() >= 2 && tail[1..].iter().all(u8::is_ascii_digit))
}

fn valid_utc_calendar_fields(bytes: &[u8]) -> bool {
    let pair = |idx: usize| (bytes[idx] - b'0') * 10 + (bytes[idx + 1] - b'0');
    (1..=12).contains(&pair(5))
        && (1..=31).contains(&pair(8))
        && pair(11) < 24
        && pair(14) < 60
        && pair(17) <= 60
}

/// Validates a release pointer or bundle URL according to [`UrlPolicy`].
///
/// Always rejects empty/oversized URLs, whitespace, control characters, backslashes, `#`
/// fragments, userinfo (`user:pass@`), and path traversal segments (`..` or percent-encoded
/// `%2e`). Under [`UrlPolicy::HttpsOnly`], ONLY `https://` with a public host (refusing
/// loopback, link-local, RFC 1918, ULA, CGNAT, and metadata IP literals) is accepted.
pub fn validate_pointer_url(url: &str, policy: UrlPolicy) -> Result<(), Error> {
    if has_forbidden_url_syntax(url) || has_forbidden_percent_escape(url) {
        return Err(Error::InvalidUrl);
    }
    if let Some(rest) = url.strip_prefix("https://") {
        let constraint = match policy {
            UrlPolicy::HttpsOnly => HostConstraint::PublicOnly,
            UrlPolicy::AllowLocalForTesting => HostConstraint::AnyValid,
        };
        return validate_http_authority_and_path(rest, constraint);
    }
    if policy == UrlPolicy::AllowLocalForTesting {
        return validate_local_testing_url(url);
    }
    Err(Error::InvalidUrl)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HostConstraint {
    PublicOnly,
    LoopbackOnly,
    AnyValid,
}

fn has_forbidden_url_syntax(url: &str) -> bool {
    url.is_empty()
        || url.len() > MAX_URL_BYTES
        || url.starts_with('-')
        || url.contains('#')
        || url.bytes().any(|b| b <= 0x20 || b >= 0x7f || b == b'\\')
}

fn has_forbidden_percent_escape(url: &str) -> bool {
    const FORBIDDEN: [&str; 4] = ["%2e", "%2f", "%5c", "%00"];
    let lower = url.to_ascii_lowercase();
    FORBIDDEN.iter().any(|needle| lower.contains(needle))
}

fn validate_local_testing_url(url: &str) -> Result<(), Error> {
    if let Some(rest) = url.strip_prefix("http://") {
        return validate_http_authority_and_path(rest, HostConstraint::LoopbackOnly);
    }
    if let Some(path) = url.strip_prefix("file:///") {
        return validate_file_url_path(path);
    }
    Err(Error::InvalidUrl)
}

fn validate_http_authority_and_path(rest: &str, constraint: HostConstraint) -> Result<(), Error> {
    let (authority, path_and_query) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => return Err(Error::InvalidUrl),
    };
    if authority.is_empty() || authority.contains('@') {
        return Err(Error::InvalidUrl);
    }
    let host = extract_host(authority)?;
    enforce_host_constraint(host, constraint)?;
    let path = path_and_query.split('?').next().unwrap_or("");
    validate_url_path_segments(path)
}

fn extract_host(authority: &str) -> Result<&str, Error> {
    if let Some(bracketed) = authority.strip_prefix('[') {
        return extract_bracketed_host(bracketed);
    }
    extract_unbracketed_host(authority)
}

fn extract_bracketed_host(bracketed: &str) -> Result<&str, Error> {
    let end = bracketed.find(']').ok_or(Error::InvalidUrl)?;
    let host = &bracketed[..end];
    let after = &bracketed[end + 1..];
    validate_optional_port(after)?;
    host.parse::<Ipv6Addr>().map_err(|_| Error::InvalidUrl)?;
    Ok(host)
}

fn extract_unbracketed_host(authority: &str) -> Result<&str, Error> {
    let (host, after) = match authority.find(':') {
        Some(colon) => (&authority[..colon], &authority[colon..]),
        None => (authority, ""),
    };
    validate_optional_port(after)?;
    if !is_valid_unbracketed_host(host) {
        return Err(Error::InvalidUrl);
    }
    Ok(host)
}

fn is_valid_unbracketed_host(host: &str) -> bool {
    !host.is_empty()
        && !host.starts_with('.')
        && !host.ends_with('.')
        && !host.contains("..")
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
}

fn enforce_host_constraint(host: &str, constraint: HostConstraint) -> Result<(), Error> {
    match constraint {
        HostConstraint::AnyValid => Ok(()),
        HostConstraint::LoopbackOnly => {
            if is_loopback_host(host) {
                Ok(())
            } else {
                Err(Error::InvalidUrl)
            }
        }
        HostConstraint::PublicOnly => {
            if is_disallowed_public_host(host) {
                Err(Error::InvalidUrl)
            } else {
                Ok(())
            }
        }
    }
}

fn is_disallowed_public_host(host: &str) -> bool {
    let lower = host.to_ascii_lowercase();
    if lower == "localhost" || lower.ends_with(".localhost") || lower == "metadata.google.internal"
    {
        return true;
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => is_disallowed_public_ipv4(v4),
            IpAddr::V6(v6) => is_disallowed_public_ipv6(v6),
        };
    }
    // Reject non-canonical numeric/hex/octal/short-form IPv4 literals (e.g. `2130706433`,
    // `0x7f000001`, `0177.0.0.1`, `127.1`), since valid DNS TLDs cannot be numeric.
    looks_like_numeric_ipv4_literal(&lower)
}

fn is_disallowed_public_ipv4(ip: Ipv4Addr) -> bool {
    is_std_non_public_ipv4(ip) || is_reserved_ipv4_block(ip.octets())
}

fn is_std_non_public_ipv4(ip: Ipv4Addr) -> bool {
    [
        ip.is_loopback(),
        ip.is_private(),
        ip.is_link_local(),
        ip.is_broadcast(),
        ip.is_multicast(),
        ip.is_documentation(),
    ]
    .into_iter()
    .any(|flag| flag)
}

fn is_reserved_ipv4_block(octets: [u8; 4]) -> bool {
    let checks = [
        octets[0] == 0,
        octets[0] == 100 && (octets[1] & 0xc0) == 64,
        octets[0] == 192 && octets[1] == 0 && octets[2] == 0,
        octets[0] == 198 && (octets[1] & 0xfe) == 18,
        octets[0] >= 240,
    ];
    checks.into_iter().any(|flag| flag)
}

fn is_disallowed_public_ipv6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = embedded_ipv4(ip) {
        return is_disallowed_public_ipv4(v4);
    }
    let segs = ip.segments();
    let checks = [
        ip.is_loopback(),
        ip.is_unspecified(),
        ip.is_multicast(),
        (segs[0] & 0xfe00) == 0xfc00,
        (segs[0] & 0xffc0) == 0xfe80,
        (segs[0] & 0xffc0) == 0xfec0,
        segs[0] == 0x2001 && segs[1] == 0x0db8,
        // RFC 6052 well-known NAT64 prefix (`64:ff9b::/96`).
        segs[..6] == [0x0064, 0xff9b, 0, 0, 0, 0],
        // RFC 8215 local-use IPv4/IPv6 translation prefix (`64:ff9b:1::/48`).
        segs[..3] == [0x0064, 0xff9b, 0x0001],
        // RFC 2765 IPv4-translated prefix (`::ffff:0:0:0/96`).
        segs[..6] == [0, 0, 0, 0, 0xffff, 0],
    ];
    checks.into_iter().any(|flag| flag)
}

fn embedded_ipv4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return Some(v4);
    }
    let segs = ip.segments();
    let is_ipv4_compatible = segs[..6] == [0; 6] && (segs[6] != 0 || segs[7] > 1);
    is_ipv4_compatible.then(|| Ipv4Addr::from(((u32::from(segs[6])) << 16) | u32::from(segs[7])))
}

fn looks_like_numeric_ipv4_literal(lower_host: &str) -> bool {
    let last = lower_host.rsplit('.').next().unwrap_or("");
    if last.is_empty() {
        return true;
    }
    let is_decimal = last.bytes().all(|b| b.is_ascii_digit());
    let is_hex = last
        .strip_prefix("0x")
        .is_some_and(|hex| !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit()));
    is_decimal || is_hex
}

fn validate_optional_port(after: &str) -> Result<(), Error> {
    if after.is_empty() {
        return Ok(());
    }
    let port_str = after.strip_prefix(':').ok_or(Error::InvalidUrl)?;
    if port_str.is_empty()
        || port_str.len() > 5
        || !port_str.bytes().all(|b| b.is_ascii_digit())
        || port_str.parse::<u16>().ok().filter(|&p| p > 0).is_none()
    {
        return Err(Error::InvalidUrl);
    }
    Ok(())
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1" || host == "::1"
}

fn validate_file_url_path(path: &str) -> Result<(), Error> {
    if path.is_empty() || path.contains('?') {
        return Err(Error::InvalidUrl);
    }
    validate_url_path_segments(path)
}

fn validate_url_path_segments(path: &str) -> Result<(), Error> {
    if path.is_empty() {
        return Err(Error::InvalidUrl);
    }
    let trimmed = path.strip_prefix('/').unwrap_or(path);
    if trimmed.is_empty() {
        return Err(Error::InvalidUrl);
    }
    for segment in trimmed.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(Error::InvalidUrl);
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "pointer_tests.rs"]
mod tests;
