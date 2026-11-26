//! Pure `agentic-org-release-v1` verification shared by installer and gateway.
//!
//! Trust is explicit: composition code must obtain [`TrustedKeys`] and
//! [`TrustedPolicy`] from its trusted build/package, never the input release.
//! This crate performs no I/O, platform admission, extraction or activation.
//! A verified manifest authenticates expectations, not a loaded image or files.

#![forbid(unsafe_code)]

mod decode;
mod payload;

use caseless::Caseless;
use ring::signature::{UnparsedPublicKey, ED25519};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use unicode_normalization::UnicodeNormalization;

pub use payload::PayloadVerifier;

/// Exact signed profile name.
pub const FORMAT: &str = "agentic-org-release-v1";
/// Manifest limit in UTF-8 bytes, inclusive.
pub const MAX_MANIFEST_BYTES: usize = 2 * 1024 * 1024;
/// Public registry limit in UTF-8 bytes, inclusive.
pub const MAX_REGISTRY_BYTES: usize = 32 * 1024;
/// Maximum publisher keys in one trusted registry.
pub const MAX_TRUSTED_KEYS: usize = 16;
/// Maximum signed regular files.
pub const MAX_FILES: usize = 16_384;
/// Maximum relative path length in UTF-8 bytes.
pub const MAX_PATH_BYTES: usize = 1024;
/// Maximum sum of signed payload sizes.
pub const MAX_PAYLOAD_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Maximum individual executable size.
pub const MAX_EXECUTABLE_BYTES: u64 = 512 * 1024 * 1024;
/// Maximum individual UI asset size.
pub const MAX_UI_ASSET_BYTES: u64 = 64 * 1024 * 1024;
/// Maximum aggregate signed UI payload bytes, inclusive.
pub const MAX_UI_BYTES: u64 = 64 * 1024 * 1024;
/// Maximum regular UI files plus unique inferred directories below `ui/`.
/// The `ui/` root is excluded; this inclusive cap matches the gateway loader.
pub const MAX_UI_ENTRIES: usize = 4096;
/// Closed executable inventory, at the release root.
pub const EXECUTABLE_PATHS: [&str; 4] = [
    "agentic-org",
    "agentic-org-office",
    "agentic-org-web",
    "agent-os",
];

/// Errors deliberately carry no untrusted strings or parser excerpts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Error {
    ManifestTooLarge,
    RegistryTooLarge,
    InvalidManifest,
    InvalidRegistry,
    InvalidPolicy,
    InvalidSignatureLength,
    UnknownSigningKey,
    InvalidSignature,
    InvalidReleaseId,
    InvalidKeyId,
    InvalidDigest,
    InvalidSequence,
    TargetMismatch,
    InstallerContractMismatch,
    ReleaseBelowMinimum,
    KeySequenceOutOfRange,
    InvalidPath,
    PathCollision,
    InvalidInventory,
    InvalidRole,
    InvalidCdHash,
    FileSizeLimit,
    PayloadSizeLimit,
    UnexpectedPayload,
    DuplicatePayload,
    MissingPayload,
    PayloadOverrun,
    PayloadUnderrun,
    PayloadDigestMismatch,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "release verification failed: {self:?}")
    }
}

impl std::error::Error for Error {}

/// Supported manifest operating systems. Host admission remains independent.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize)]
#[serde(try_from = "String")]
pub enum Platform {
    Macos,
    Linux,
}

/// Supported manifest CPUs.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize)]
#[serde(try_from = "String")]
pub enum Architecture {
    Aarch64,
    X86_64,
}

/// Signed payload role.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize)]
#[serde(try_from = "String")]
pub enum Role {
    Executable,
    Data,
}

impl TryFrom<String> for Platform {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "macos" => Ok(Self::Macos),
            "linux" => Ok(Self::Linux),
            _ => Err("unsupported release platform"),
        }
    }
}

impl TryFrom<String> for Architecture {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "aarch64" => Ok(Self::Aarch64),
            "x86_64" => Ok(Self::X86_64),
            _ => Err("unsupported release architecture"),
        }
    }
}

impl TryFrom<String> for Role {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "executable" => Ok(Self::Executable),
            "data" => Ok(Self::Data),
            _ => Err("unsupported release role"),
        }
    }
}

/// Immutable expectations supplied by trusted composition code.
#[derive(Clone, Debug)]
pub struct TrustedPolicy {
    platform: Platform,
    architecture: Architecture,
    minimum_sequence: u64,
}

impl TrustedPolicy {
    /// Construct build-owned expectations. Only installer contract 1 is supported;
    /// the minimum sequence may be zero (the manifest sequence must be positive).
    pub fn new(
        platform: Platform,
        architecture: Architecture,
        installer_contract: u64,
        minimum_sequence: u64,
    ) -> Result<Self, Error> {
        if installer_contract != 1 {
            return Err(Error::InvalidPolicy);
        }
        Ok(Self {
            platform,
            architecture,
            minimum_sequence,
        })
    }
}

#[derive(Clone, Debug)]
struct TrustedKey {
    public_key: [u8; 32],
    min_sequence: u64,
    max_sequence: Option<u64>,
}

/// Immutable trusted publisher registry. It has no empty/default constructor.
///
/// Parsing validates a public registry; it does not establish its provenance.
/// Release consumers must parse their build-pinned registry, never bundle, CLI
/// or runtime environment bytes. A manifest cannot introduce another signer.
#[derive(Clone, Debug)]
pub struct TrustedKeys {
    keys: BTreeMap<String, TrustedKey>,
}

impl TrustedKeys {
    /// Parse strict version 1 registry bytes, including required nullable
    /// `max_sequence`. Inclusive intervals may begin at zero.
    pub fn from_registry_bytes(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_REGISTRY_BYTES {
            return Err(Error::RegistryTooLarge);
        }
        let raw: decode::Registry =
            decode::object_from_slice(bytes).map_err(|_| Error::InvalidRegistry)?;
        if raw.version != 1 || raw.keys.is_empty() || raw.keys.len() > MAX_TRUSTED_KEYS {
            return Err(Error::InvalidRegistry);
        }
        let mut keys = BTreeMap::new();
        let mut public_keys = BTreeSet::new();
        for raw_key in raw.keys {
            let public_key =
                decode_hex::<32>(&raw_key.public_key_hex).map_err(|_| Error::InvalidRegistry)?;
            let key_id = decode_hex::<32>(&raw_key.key_id).map_err(|_| Error::InvalidRegistry)?;
            if <[u8; 32]>::from(Sha256::digest(public_key)) != key_id
                || raw_key
                    .max_sequence
                    .0
                    .is_some_and(|maximum| maximum < raw_key.min_sequence)
                || !public_keys.insert(public_key)
            {
                return Err(Error::InvalidRegistry);
            }
            if keys
                .insert(
                    raw_key.key_id,
                    TrustedKey {
                        public_key,
                        min_sequence: raw_key.min_sequence,
                        max_sequence: raw_key.max_sequence.0,
                    },
                )
                .is_some()
            {
                return Err(Error::InvalidRegistry);
            }
        }
        Ok(Self { keys })
    }

    /// Number of validated publisher keys, always 1..=16.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// A valid registry is never empty; provided for collection ergonomics.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

/// Immutable authenticated file expectation. No public constructors or setters.
#[derive(Clone, Debug)]
pub struct FileEntry {
    path: String,
    size: u64,
    sha256: [u8; 32],
    role: Role,
    macos_cdhash: Option<[u8; 20]>,
}

impl FileEntry {
    /// Original decoded UTF-8 path, without case folding or normalization.
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn sha256(&self) -> &[u8; 32] {
        &self.sha256
    }

    pub fn role(&self) -> Role {
        self.role
    }

    /// Signed expected CodeDirectory unique hash. This is not evidence that a
    /// platform inspected any static or loaded code; consumers must do that.
    pub fn macos_cdhash(&self) -> Option<&[u8; 20]> {
        self.macos_cdhash.as_ref()
    }

    /// Start a bounded-memory check of this entry's exact signed bytes.
    pub fn payload_verifier(&self) -> PayloadVerifier {
        PayloadVerifier::new(self.size, self.sha256)
    }
}

/// Immutable manifest semantics, accessible only after authentication succeeds.
#[derive(Clone, Debug)]
pub struct Manifest {
    release_id: String,
    sequence: u64,
    platform: Platform,
    architecture: Architecture,
    key_id: String,
    files: Vec<FileEntry>,
    total_size: u64,
}

impl Manifest {
    pub fn release_id(&self) -> &str {
        &self.release_id
    }

    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    pub fn platform(&self) -> Platform {
        self.platform
    }

    pub fn architecture(&self) -> Architecture {
        self.architecture
    }

    pub fn installer_contract(&self) -> u64 {
        1
    }

    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    /// Original signed file order; entries cannot be mutated through this API.
    pub fn files(&self) -> &[FileEntry] {
        &self.files
    }

    pub fn total_size(&self) -> u64 {
        self.total_size
    }
}

/// Exact signed bytes, validated semantics and efficient exact-path lookup.
/// It cannot be deserialized or changed into a different trusted inventory.
#[derive(Clone, Debug)]
pub struct VerifiedManifest {
    manifest: Manifest,
    bytes: Box<[u8]>,
    sha256: [u8; 32],
    by_path: BTreeMap<String, usize>,
}

impl VerifiedManifest {
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn manifest_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn manifest_sha256(&self) -> &[u8; 32] {
        &self.sha256
    }

    /// Exact byte-sensitive lookup, never a normalized alias.
    pub fn file(&self, path: &str) -> Option<&FileEntry> {
        self.by_path
            .get(path)
            .map(|&index| &self.manifest.files[index])
    }

    /// Compare a regular-payload inventory with the complete signed inventory.
    /// Exclude the exact manifest/signature control files; include every other
    /// regular payload once. This does not inspect filesystem types or content.
    /// It consumes the iterator incrementally and refuses extra/duplicate names.
    pub fn verify_inventory<'a>(
        &self,
        paths: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), Error> {
        let mut seen = BTreeSet::new();
        for path in paths {
            if !self.by_path.contains_key(path) {
                return Err(Error::UnexpectedPayload);
            }
            if !seen.insert(path) {
                return Err(Error::DuplicatePayload);
            }
        }
        if seen.len() != self.by_path.len() {
            return Err(Error::MissingPayload);
        }
        Ok(())
    }
}

/// Authenticate the exact UTF-8 bytes and validate all profile constraints.
/// The signature is exactly 64 raw Ed25519 bytes, without a text/base64 envelope.
/// No reserialization or untrusted signer fallback is performed.
pub fn verify_manifest(
    manifest_bytes: &[u8],
    signature: &[u8],
    policy: &TrustedPolicy,
    trusted_keys: &TrustedKeys,
) -> Result<VerifiedManifest, Error> {
    let (raw, key) = authenticate_manifest(manifest_bytes, signature, trusted_keys)?;
    validate_release_policy(&raw, policy, key)?;
    let inventory = InventoryVerifier::verify(raw.files, raw.platform)?;
    Ok(VerifiedManifest {
        manifest: Manifest {
            release_id: raw.release_id,
            sequence: raw.sequence,
            platform: raw.platform,
            architecture: raw.architecture,
            key_id: raw.key_id,
            files: inventory.files,
            total_size: inventory.total_size,
        },
        bytes: manifest_bytes.into(),
        sha256: Sha256::digest(manifest_bytes).into(),
        by_path: inventory.by_path,
    })
}

fn authenticate_manifest<'a>(
    manifest_bytes: &[u8],
    signature: &[u8],
    trusted_keys: &'a TrustedKeys,
) -> Result<(decode::Manifest, &'a TrustedKey), Error> {
    if manifest_bytes.len() > MAX_MANIFEST_BYTES {
        return Err(Error::ManifestTooLarge);
    }
    if signature.len() != 64 {
        return Err(Error::InvalidSignatureLength);
    }
    // Direct typed decoding rejects unknown/duplicate (including escaped)
    // fields, missing required nullable fields and noninteger numeric tokens.
    let raw: decode::Manifest =
        decode::object_from_slice(manifest_bytes).map_err(|_| Error::InvalidManifest)?;
    decode_hex::<32>(&raw.key_id).map_err(|_| Error::InvalidKeyId)?;
    let key = trusted_keys
        .keys
        .get(&raw.key_id)
        .ok_or(Error::UnknownSigningKey)?;
    UnparsedPublicKey::new(&ED25519, key.public_key)
        .verify(manifest_bytes, signature)
        .map_err(|_| Error::InvalidSignature)?;
    Ok((raw, key))
}

fn validate_release_policy(
    raw: &decode::Manifest,
    policy: &TrustedPolicy,
    key: &TrustedKey,
) -> Result<(), Error> {
    if raw.format != FORMAT {
        return Err(Error::InvalidManifest);
    }
    if !valid_release_id(&raw.release_id) {
        return Err(Error::InvalidReleaseId);
    }
    if raw.sequence == 0 {
        return Err(Error::InvalidSequence);
    }
    if raw.platform != policy.platform || raw.architecture != policy.architecture {
        return Err(Error::TargetMismatch);
    }
    if raw.installer_contract != 1 {
        return Err(Error::InstallerContractMismatch);
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

/// Accumulates the closed inventory in signed order, retaining separate
/// whole-release and nested UI budgets until final completeness checks.
#[derive(Default)]
struct InventoryVerifier {
    files: Vec<FileEntry>,
    by_path: BTreeMap<String, usize>,
    normalized_paths: BTreeSet<String>,
    total_size: u64,
    ui: UiBudget,
    executable_count: usize,
    has_index: bool,
}

impl InventoryVerifier {
    fn verify(entries: Vec<decode::File>, platform: Platform) -> Result<Self, Error> {
        let mut inventory = Self {
            files: Vec::with_capacity(entries.len()),
            ..Self::default()
        };
        for entry in entries {
            inventory.add_entry(entry, platform)?;
        }
        inventory.validate_complete()?;
        Ok(inventory)
    }

    fn add_entry(&mut self, entry: decode::File, platform: Platform) -> Result<(), Error> {
        validate_path(&entry.path)?;
        let normalized: String = entry.path.nfc().default_case_fold().nfc().collect();
        if !self.normalized_paths.insert(normalized) {
            return Err(Error::PathCollision);
        }
        let executable = validate_file_role_and_size(&entry)? == Role::Executable;
        self.total_size = self
            .total_size
            .checked_add(entry.size)
            .filter(|&size| size <= MAX_PAYLOAD_BYTES)
            .ok_or(Error::PayloadSizeLimit)?;
        if !executable {
            self.ui.add_entry(&entry.path, entry.size)?;
        }
        let sha256 = decode_hex::<32>(&entry.sha256).map_err(|_| Error::InvalidDigest)?;
        let macos_cdhash = validate_cdhash(platform, executable, entry.macos_cdhash.0)?;
        self.executable_count += usize::from(executable);
        self.has_index |= entry.path == "ui/index.html";
        self.by_path.insert(entry.path.clone(), self.files.len());
        self.files.push(FileEntry {
            path: entry.path,
            size: entry.size,
            sha256,
            role: entry.role,
            macos_cdhash,
        });
        Ok(())
    }

    fn validate_complete(&self) -> Result<(), Error> {
        // Check every normalized ancestor, not only adjacent sorted entries: a
        // lexicographically intervening name must not conceal a file/dir clash.
        for path in &self.normalized_paths {
            for (index, _) in path.match_indices('/') {
                if self.normalized_paths.contains(&path[..index]) {
                    return Err(Error::PathCollision);
                }
            }
        }
        if self.executable_count != EXECUTABLE_PATHS.len() || !self.has_index {
            return Err(Error::InvalidInventory);
        }
        Ok(())
    }
}

fn validate_file_role_and_size(entry: &decode::File) -> Result<Role, Error> {
    let expected_role = if EXECUTABLE_PATHS.contains(&entry.path.as_str()) {
        Role::Executable
    } else if entry.path.starts_with("ui/") {
        Role::Data
    } else {
        return Err(Error::InvalidInventory);
    };
    if entry.role != expected_role {
        return Err(Error::InvalidRole);
    }
    let limit = if expected_role == Role::Executable {
        MAX_EXECUTABLE_BYTES
    } else {
        MAX_UI_ASSET_BYTES
    };
    if entry.size > limit {
        return Err(Error::FileSizeLimit);
    }
    Ok(expected_role)
}

fn validate_cdhash(
    platform: Platform,
    executable: bool,
    hash: Option<String>,
) -> Result<Option<[u8; 20]>, Error> {
    match (platform, executable, hash) {
        (Platform::Macos, true, Some(hash)) => Ok(Some(
            decode_hex::<20>(&hash).map_err(|_| Error::InvalidCdHash)?,
        )),
        (Platform::Macos, false, None) | (Platform::Linux, _, None) => Ok(None),
        _ => Err(Error::InvalidCdHash),
    }
}

/// Accounts for signed UI file bytes and the directory loader's complete
/// entry count; directories consume an entry but no payload bytes.
#[derive(Default)]
struct UiBudget {
    size: u64,
    entry_count: usize,
    directories: BTreeSet<String>,
}

impl UiBudget {
    fn add_entry(&mut self, path: &str, size: u64) -> Result<(), Error> {
        self.size = self
            .size
            .checked_add(size)
            .filter(|&size| size <= MAX_UI_BYTES)
            .ok_or(Error::PayloadSizeLimit)?;
        self.entry_count = self
            .entry_count
            .checked_add(1)
            .filter(|&count| count <= MAX_UI_ENTRIES)
            .ok_or(Error::InvalidInventory)?;
        // Only proper directory prefixes contribute: skip the first slash
        // (the ui root) and count shared exact prefixes just once.
        for (index, _) in path.match_indices('/').skip(1) {
            if self.directories.insert(path[..index].to_owned()) {
                self.entry_count = self
                    .entry_count
                    .checked_add(1)
                    .filter(|&count| count <= MAX_UI_ENTRIES)
                    .ok_or(Error::InvalidInventory)?;
            }
        }
        Ok(())
    }
}

/// Whether `id` is a release ID a manifest may carry, and so the name of a release's folder
/// in a release store: 1 to 96 ASCII letters, digits, `.`, `_` or `-`, starting with a letter
/// or digit (never `.` or `..`).
pub fn valid_release_id(id: &str) -> bool {
    (1..=96).contains(&id.len())
        && id.as_bytes()[0].is_ascii_alphanumeric()
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}

fn validate_path(path: &str) -> Result<(), Error> {
    if path.is_empty()
        || path.len() > MAX_PATH_BYTES
        || path.contains('\\')
        || path.chars().any(char::is_control)
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || (path.as_bytes().get(1) == Some(&b':') && path.as_bytes()[0].is_ascii_alphabetic())
    {
        return Err(Error::InvalidPath);
    }
    Ok(())
}

fn decode_hex<const N: usize>(text: &str) -> Result<[u8; N], ()> {
    if text.len() != N * 2 {
        return Err(());
    }
    let mut output = [0; N];
    for (target, pair) in output.iter_mut().zip(text.as_bytes().as_chunks::<2>().0) {
        let nibble = |byte| match byte {
            b'0'..=b'9' => Ok(byte - b'0'),
            b'a'..=b'f' => Ok(byte - b'a' + 10),
            _ => Err(()),
        };
        *target = nibble(pair[0])? * 16 + nibble(pair[1])?;
    }
    Ok(output)
}

#[cfg(test)]
mod tests;
