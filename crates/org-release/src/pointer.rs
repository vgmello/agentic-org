//! Pure `agentic-org-release-pointer-v1` verification (ADR-0009).
//!
//! Authenticates a signed channel release pointer (`pointer.v1.json` + raw 64-byte
//! `pointer.v1.sig`) against the exact same build-pinned [`TrustedKeys`] and
//! [`TrustedPolicy`] used for release manifests. A pointer never introduces or rotates
//! trust keys, never lowers `minimum_sequence`, and never grants execution authority on
//! its own: the referenced bundle must still pass full `verify_manifest` + payload checks.

use std::collections::BTreeSet;

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
    if raw.format != POINTER_FORMAT {
        return Err(Error::InvalidPointer);
    }
    if !valid_channel(&raw.channel) {
        return Err(Error::InvalidChannel);
    }
    if !valid_release_id(&raw.release_id) {
        return Err(Error::InvalidReleaseId);
    }
    if raw.sequence == 0 {
        return Err(Error::InvalidSequence);
    }
    if raw.installer_contract != 1 {
        return Err(Error::InstallerContractMismatch);
    }
    if !valid_published_at(&raw.published_at) {
        return Err(Error::InvalidPointer);
    }
    if raw.sequence < policy.minimum_sequence {
        return Err(Error::ReleaseBelowMinimum);
    }
    if raw.sequence < key.min_sequence
        || key
            .max_sequence
            .is_some_and(|maximum| raw.sequence > maximum)
    {
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
/// `%2e`). Under [`UrlPolicy::HttpsOnly`], ONLY `https://` with a valid host is accepted.
pub fn validate_pointer_url(url: &str, policy: UrlPolicy) -> Result<(), Error> {
    if has_forbidden_url_syntax(url) || has_forbidden_percent_escape(url) {
        return Err(Error::InvalidUrl);
    }
    if let Some(rest) = url.strip_prefix("https://") {
        return validate_http_authority_and_path(rest, false);
    }
    if policy == UrlPolicy::AllowLocalForTesting {
        return validate_local_testing_url(url);
    }
    Err(Error::InvalidUrl)
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
        return validate_http_authority_and_path(rest, true);
    }
    if let Some(path) = url.strip_prefix("file:///") {
        return validate_file_url_path(path);
    }
    Err(Error::InvalidUrl)
}

fn validate_http_authority_and_path(rest: &str, loopback_only: bool) -> Result<(), Error> {
    let (authority, path_and_query) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => return Err(Error::InvalidUrl),
    };
    if authority.is_empty() || authority.contains('@') {
        return Err(Error::InvalidUrl);
    }
    let host = extract_host(authority)?;
    if loopback_only && !is_loopback_host(host) {
        return Err(Error::InvalidUrl);
    }
    let path = path_and_query.split('?').next().unwrap_or("");
    validate_url_path_segments(path)
}

fn extract_host(authority: &str) -> Result<&str, Error> {
    if let Some(bracketed) = authority.strip_prefix('[') {
        let end = bracketed.find(']').ok_or(Error::InvalidUrl)?;
        let host = &bracketed[..end];
        let after = &bracketed[end + 1..];
        if host.is_empty() || !host.bytes().all(|b| b.is_ascii_hexdigit() || b == b':') {
            return Err(Error::InvalidUrl);
        }
        validate_optional_port(after)?;
        return Ok(host);
    }
    let (host, after) = match authority.find(':') {
        Some(colon) => (&authority[..colon], &authority[colon..]),
        None => (authority, ""),
    };
    validate_optional_port(after)?;
    if host.is_empty()
        || host.starts_with('.')
        || host.ends_with('.')
        || host.contains("..")
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
    {
        return Err(Error::InvalidUrl);
    }
    Ok(host)
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
