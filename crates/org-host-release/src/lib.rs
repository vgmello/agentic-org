//! Publisher-authenticated input, separate from installation/activation. All
//! payloads are rehashed while copying through held no-follow source handles.
//! No credentials, service or instance are opened by these operations.

#![forbid(unsafe_code)]

mod archive;
pub mod fetcher;
mod files;
mod ids;
mod platform;
pub mod pull;
mod runtime;
pub mod signing;
mod store;
pub mod update_state;

pub use archive::ArchiveBundle;
pub use fetcher::{FetchError, HttpsReleaseFetcher, LocalDirectoryFetcher, ReleaseFetcher};
pub use pull::{
    check_pointer, check_pointer_with_policy, default_signature_url, stage_verified_pointer,
    stage_verified_pointer_with_policy, PointerCheck, PointerCheckState, PointerDecision,
    PullConfig, PullError, StagedRelease, DEFAULT_BUNDLE_TIMEOUT, DEFAULT_METADATA_TIMEOUT,
    DEFAULT_POINTER_URL,
};
pub use runtime::{Pin, RuntimeRelease};
pub use store::{ReleasePin, ReleaseStore};
pub use update_state::{
    default_update_state_dir, PendingHealthCheck, QuarantinedRelease, UpdateState, UpdateStateStore,
};

use org_platform::image::{ReleaseArchitecture, ReleasePlatform};
use org_release::{Architecture, Platform, TrustedKeys, TrustedPolicy, VerifiedManifest};
use std::{
    io::{self, Read, Write},
    path::Path,
};

// Public policy data only: the reviewed build script validates the registry
// and always supplies this compile-time floor. No generated Rust is included.
const RELEASE_REGISTRY: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/release-trust-registry.json"));
const RELEASE_MIN_SEQUENCE: u64 = build_floor(env!("ORG_EMBEDDED_RELEASE_MIN_SEQUENCE"));

const fn build_floor(raw: &str) -> u64 {
    let bytes = raw.as_bytes();
    assert!(!bytes.is_empty(), "missing compiled release floor");
    assert!(
        bytes.len() == 1 || bytes[0] != b'0',
        "noncanonical compiled release floor"
    );
    let mut value = 0u64;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        assert!(
            byte >= b'0' && byte <= b'9',
            "invalid compiled release floor"
        );
        value = match value.checked_mul(10) {
            Some(n) => n,
            None => panic!("compiled release floor overflow"),
        };
        value = match value.checked_add((byte - b'0') as u64) {
            Some(n) => n,
            None => panic!("compiled release floor overflow"),
        };
        index += 1;
    }
    value
}

/// Sanitized release input error. Untrusted filenames/parser excerpts are absent.
#[derive(Debug)]
pub enum Error {
    /// No publisher registry/floor was provided by the trusted build.
    MissingBuildTrust,
    /// This build target is outside the selected release profile.
    UnsupportedTarget,
    /// Manifest, signature, inventory or payload did not authenticate.
    InvalidRelease,
    /// Unsafe entry, changed input, resource bound or unavailable filesystem.
    UnsafeInput,
    /// Another local installer owns the store; no transaction was started.
    Busy,
    /// Required OS/CPU/signing/hardening policy was not established.
    PlatformAdmission,
    /// The store cannot allocate another commit; original pins remain readable.
    Exhausted,
    /// Bounded filesystem/copy operation failed; no activation is implied.
    Io(io::ErrorKind),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "release verification: {self:?}")
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e.kind())
    }
}

fn invalid(_: org_release::Error) -> Error {
    Error::InvalidRelease
}

fn policy() -> Result<(TrustedPolicy, TrustedKeys), Error> {
    if RELEASE_REGISTRY.is_empty() || RELEASE_MIN_SEQUENCE == 0 {
        return Err(Error::MissingBuildTrust);
    }
    // The platform names the release this build belongs to; a target without one refuses.
    let target =
        org_platform::image::release_target().map_err(|_refused| Error::UnsupportedTarget)?;
    let platform = match target.platform {
        ReleasePlatform::Macos => Platform::Macos,
        ReleasePlatform::Linux => Platform::Linux,
    };
    let architecture = match target.architecture {
        ReleaseArchitecture::Aarch64 => Architecture::Aarch64,
        ReleaseArchitecture::X86_64 => Architecture::X86_64,
    };
    Ok((
        TrustedPolicy::new(platform, architecture, 1, RELEASE_MIN_SEQUENCE).map_err(invalid)?,
        TrustedKeys::from_registry_bytes(RELEASE_REGISTRY).map_err(invalid)?,
    ))
}

/// Authenticated directory input. This is not a durable release pin, installed
/// executable or platform image-admission result. Source mutation after the
/// initial pass is detected/refused during each exact-byte payload copy.
pub struct DirectoryBundle {
    root: files::Directory,
    manifest: VerifiedManifest,
    signature: [u8; 64],
}

impl DirectoryBundle {
    /// Verify using only the embedded public registry and target/floor policy.
    /// Runtime environment, bundle keys and configuration cannot supply trust.
    pub fn open(source: &Path) -> Result<Self, Error> {
        let (policy, keys) = policy()?;
        Self::with_policy(source, &policy, &keys)
    }

    fn with_policy(
        source: &Path,
        policy: &TrustedPolicy,
        keys: &TrustedKeys,
    ) -> Result<Self, Error> {
        let root = files::Directory::open(source)?;
        let bytes = root.read("release.v1.json", org_release::MAX_MANIFEST_BYTES)?;
        let signature: [u8; 64] = root
            .read("release.v1.sig", 64)?
            .try_into()
            .map_err(|_| Error::InvalidRelease)?;
        let manifest =
            org_release::verify_manifest(&bytes, &signature, policy, keys).map_err(invalid)?;
        root.verify_inventory(&manifest)?;
        let bundle = Self {
            root,
            manifest,
            signature,
        };
        for file in bundle.manifest.manifest().files() {
            bundle.copy_payload(file.path(), &mut io::sink())?;
        }
        // Controls are not payloads but must remain the exact authenticated bytes.
        if bundle
            .root
            .read("release.v1.json", org_release::MAX_MANIFEST_BYTES)?
            != bytes
            || bundle.root.read("release.v1.sig", 64)? != signature
        {
            return Err(Error::UnsafeInput);
        }
        Ok(bundle)
    }

    /// Encapsulated authenticated expectations; no mutable parsed tree is exposed.
    pub fn manifest(&self) -> &VerifiedManifest {
        &self.manifest
    }

    /// Exact raw Ed25519 sidecar for immutable staging, not a signing key.
    pub fn signature(&self) -> &[u8; 64] {
        &self.signature
    }

    fn verify_controls(&self) -> Result<(), Error> {
        self.root.verify_inventory(&self.manifest)?;
        if self
            .root
            .read("release.v1.json", org_release::MAX_MANIFEST_BYTES)?
            != self.manifest.manifest_bytes()
            || self.root.read("release.v1.sig", 64)? != self.signature
        {
            return Err(Error::UnsafeInput);
        }
        Ok(())
    }

    /// Copy only a signed payload. Every read checks ownership/type/link/mode,
    /// checked exact byte count/hash and opened descriptor identity afterward.
    /// A failed copy leaves its destination uncommitted; caller owns staging.
    pub fn copy_payload(&self, path: &str, output: &mut impl Write) -> Result<(), Error> {
        let expected = self.manifest.file(path).ok_or(Error::InvalidRelease)?;
        let mut source = self.root.payload(path)?;
        let before = files::identity(&source)?;
        let mut verifier = expected.payload_verifier();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let count = source.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            verifier.update(&buffer[..count]).map_err(invalid)?;
            output.write_all(&buffer[..count])?;
        }
        verifier.finish().map_err(invalid)?;
        if before != files::identity(&source)? {
            return Err(Error::UnsafeInput);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
