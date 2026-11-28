//! Pull-verified release pointer checking and bounded bundle staging (ADR-0009).
//!
//! Fetches `pointer.v1.json` + raw 64-byte `pointer.v1.sig` through [`ReleaseFetcher`],
//! verifies Ed25519 authenticity against the build-pinned [`TrustedKeys`] and [`TrustedPolicy`],
//! checks sequence freshness and local quarantine tombstones, and streams the referenced bundle
//! archive into an isolated `0700` staging directory with `O_NOFOLLOW` + streaming SHA-256 +
//! full [`ArchiveBundle`] verification BEFORE any active pin or running service is touched.

use std::fmt;
use std::fs;
use std::io::{self, Seek, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use org_release::{
    validate_pointer_url, verify_pointer, PayloadVerifier, TrustedKeys, TrustedPolicy, UrlPolicy,
    VerifiedPointer, MAX_POINTER_BYTES,
};
use serde::Serialize;

use crate::archive::ArchiveBundle;
use crate::fetcher::{FetchError, ReleaseFetcher};
use crate::update_state::UpdateState;
use crate::{Error, ReleasePin};

/// Default GitHub Releases signed pointer URL for Agentic Org.
pub const DEFAULT_POINTER_URL: &str =
    "https://github.com/vgmello/agentic-org/releases/latest/download/release-pointer.v1.json";

/// Default timeout for small metadata fetches (pointer JSON & 64-byte signature).
pub const DEFAULT_METADATA_TIMEOUT: Duration = Duration::from_secs(30);
/// Default timeout for streaming release bundle downloads.
pub const DEFAULT_BUNDLE_TIMEOUT: Duration = Duration::from_secs(300);

/// Configuration for pull-verified pointer and bundle retrieval.
#[derive(Debug, Clone)]
pub struct PullConfig {
    pub pointer_url: String,
    pub signature_url: String,
    pub url_policy: UrlPolicy,
    pub metadata_timeout: Duration,
    pub bundle_timeout: Duration,
}

impl PullConfig {
    /// Build a production HTTPS-only pull config from a pointer URL, deriving the sibling `.sig` URL
    /// unless overridden explicitly.
    pub fn https(pointer_url: impl Into<String>, signature_url: Option<String>) -> Self {
        let pointer_url = pointer_url.into();
        let signature_url = signature_url.unwrap_or_else(|| default_signature_url(&pointer_url));
        Self {
            pointer_url,
            signature_url,
            url_policy: UrlPolicy::HttpsOnly,
            metadata_timeout: DEFAULT_METADATA_TIMEOUT,
            bundle_timeout: DEFAULT_BUNDLE_TIMEOUT,
        }
    }

    /// Build a pull config with an explicit URL policy (e.g. `AllowLocalForTesting` in test harnesses).
    pub fn with_url_policy(
        pointer_url: impl Into<String>,
        signature_url: Option<String>,
        url_policy: UrlPolicy,
    ) -> Self {
        let pointer_url = pointer_url.into();
        let signature_url = signature_url.unwrap_or_else(|| default_signature_url(&pointer_url));
        Self {
            pointer_url,
            signature_url,
            url_policy,
            metadata_timeout: DEFAULT_METADATA_TIMEOUT,
            bundle_timeout: DEFAULT_BUNDLE_TIMEOUT,
        }
    }
}

/// Derives the default sibling signature URL from `pointer_url`:
/// `.../release-pointer.v1.json` -> `.../release-pointer.v1.sig`,
/// otherwise appends `.sig`.
pub fn default_signature_url(pointer_url: &str) -> String {
    let (base, query) = match pointer_url.split_once('?') {
        Some((b, q)) => (b, Some(q)),
        None => (pointer_url, None),
    };
    let sig_base = if let Some(stem) = base.strip_suffix(".json") {
        format!("{stem}.sig")
    } else {
        format!("{base}.sig")
    };
    match query {
        Some(q) => format!("{sig_base}?{q}"),
        None => sig_base,
    }
}

/// Decision produced by evaluating a verified signed pointer against the active release and quarantine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PointerDecision {
    /// Active release already matches the pointer's release ID and sequence.
    UpToDate,
    /// A strictly newer verified release is available to stage and apply.
    UpdateAvailable,
    /// Pointer names a release previously quarantined after a failed health check.
    Quarantined { reason: String },
    /// Pointer sequence is behind or conflicts with the active/highest release sequence.
    DowngradeRejected {
        current_sequence: u64,
        pointer_sequence: u64,
    },
}

/// Verified pointer check outcome (zero mutations to store or services).
#[derive(Debug, Clone)]
pub struct PointerCheck {
    pub verified: VerifiedPointer,
    pub decision: PointerDecision,
    pub current_pin: Option<ReleasePin>,
    pub highest_sequence: u64,
}

/// Errors returned while pulling, verifying, or staging a remote release.
#[derive(Debug)]
pub enum PullError {
    Fetch(FetchError),
    Verify(org_release::Error),
    Release(Error),
    Downgrade {
        current_sequence: u64,
        pointer_sequence: u64,
    },
    Quarantined {
        release_id: String,
        reason: String,
    },
    CrossBindingMismatch,
}

impl fmt::Display for PullError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fetch(e) => write!(f, "{e}"),
            Self::Verify(e) => write!(f, "{e}"),
            Self::Release(e) => write!(f, "{e}"),
            Self::Downgrade {
                current_sequence,
                pointer_sequence,
            } => write!(
                f,
                "refusing release downgrade/replay: pointer sequence {pointer_sequence} is not newer than current sequence {current_sequence}"
            ),
            Self::Quarantined { release_id, reason } => write!(
                f,
                "release `{release_id}` is quarantined after a prior failed health check: {reason}"
            ),
            Self::CrossBindingMismatch => write!(
                f,
                "downloaded release bundle manifest does not match the signed release pointer"
            ),
        }
    }
}

impl std::error::Error for PullError {}

impl From<FetchError> for PullError {
    fn from(e: FetchError) -> Self {
        Self::Fetch(e)
    }
}

impl From<org_release::Error> for PullError {
    fn from(e: org_release::Error) -> Self {
        Self::Verify(e)
    }
}

impl From<Error> for PullError {
    fn from(e: Error) -> Self {
        Self::Release(e)
    }
}

/// RAII private `0700` staging directory deleted on drop (including early errors or panics).
#[derive(Debug)]
struct StagingDirGuard(PathBuf);

impl Drop for StagingDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Fully verified, privately frozen release archive ready to install into [`crate::ReleaseStore`].
///
/// The backing `0700` staging folder is cleaned up automatically when this value drops.
pub struct StagedRelease {
    _guard: StagingDirGuard,
    pointer: VerifiedPointer,
    bundle: ArchiveBundle,
}

impl StagedRelease {
    pub fn pointer(&self) -> &VerifiedPointer {
        &self.pointer
    }

    pub fn bundle(&self) -> &ArchiveBundle {
        &self.bundle
    }

    pub fn bundle_mut(&mut self) -> &mut ArchiveBundle {
        &mut self.bundle
    }
}

/// Host release state compared against an authenticated release pointer.
#[derive(Debug, Clone, Copy)]
pub struct PointerCheckState<'a> {
    pub current_pin: Option<&'a ReleasePin>,
    pub highest_sequence: u64,
    pub update_state: &'a UpdateState,
    pub allow_quarantined: bool,
}

/// Fetch and verify a signed release pointer using the build-embedded trust registry and floor.
pub fn check_pointer(
    fetcher: &dyn ReleaseFetcher,
    config: &PullConfig,
    state: &PointerCheckState<'_>,
) -> Result<PointerCheck, PullError> {
    let (policy, keys) = crate::policy()?;
    check_pointer_with_policy(fetcher, config, &policy, &keys, state)
}

/// Fetch and verify a signed release pointer against explicit policy and keys.
pub fn check_pointer_with_policy(
    fetcher: &dyn ReleaseFetcher,
    config: &PullConfig,
    policy: &TrustedPolicy,
    keys: &TrustedKeys,
    state: &PointerCheckState<'_>,
) -> Result<PointerCheck, PullError> {
    validate_pointer_url(&config.pointer_url, config.url_policy)?;
    validate_pointer_url(&config.signature_url, config.url_policy)?;

    let pointer_bytes = fetcher.fetch_bytes(
        &config.pointer_url,
        MAX_POINTER_BYTES,
        config.metadata_timeout,
    )?;
    let sig_bytes = fetcher.fetch_bytes(&config.signature_url, 64, config.metadata_timeout)?;
    let verified = verify_pointer(&pointer_bytes, &sig_bytes, policy, keys, config.url_policy)?;
    let decision = evaluate_pointer_decision(&verified, state);
    Ok(PointerCheck {
        verified,
        decision,
        current_pin: state.current_pin.cloned(),
        highest_sequence: state.highest_sequence,
    })
}

fn evaluate_pointer_decision(
    verified: &VerifiedPointer,
    state: &PointerCheckState<'_>,
) -> PointerDecision {
    let ptr = verified.pointer();
    if let Some(active) = state.current_pin {
        if ptr.sequence() < active.sequence {
            return PointerDecision::DowngradeRejected {
                current_sequence: active.sequence,
                pointer_sequence: ptr.sequence(),
            };
        }
        if ptr.sequence() == active.sequence {
            return if ptr.release_id() == active.release_id {
                PointerDecision::UpToDate
            } else {
                PointerDecision::DowngradeRejected {
                    current_sequence: active.sequence,
                    pointer_sequence: ptr.sequence(),
                }
            };
        }
    }
    if !state.allow_quarantined {
        if let Some(q) = state.update_state.quarantined_entry(ptr.release_id()) {
            return PointerDecision::Quarantined {
                reason: q.reason.clone(),
            };
        }
    }
    if ptr.sequence() < state.highest_sequence {
        return PointerDecision::DowngradeRejected {
            current_sequence: state.highest_sequence,
            pointer_sequence: ptr.sequence(),
        };
    }
    PointerDecision::UpdateAvailable
}

/// Download, stream-verify SHA-256 + byte bound, privately freeze (`0700` dir, `O_NOFOLLOW`),
/// and fully authenticate the inner ADR-0009 [`ArchiveBundle`] using embedded build trust.
pub fn stage_verified_pointer(
    fetcher: &dyn ReleaseFetcher,
    config: &PullConfig,
    verified: VerifiedPointer,
    staging_parent: &Path,
) -> Result<StagedRelease, PullError> {
    let (policy, keys) = crate::policy()?;
    stage_verified_pointer_with_policy(fetcher, config, verified, staging_parent, &policy, &keys)
}

/// Download, stream-verify, and authenticate a release archive against explicit policy and keys.
pub fn stage_verified_pointer_with_policy(
    fetcher: &dyn ReleaseFetcher,
    config: &PullConfig,
    verified: VerifiedPointer,
    staging_parent: &Path,
    policy: &TrustedPolicy,
    keys: &TrustedKeys,
) -> Result<StagedRelease, PullError> {
    let target = verified.pointer().target();
    validate_pointer_url(target.url(), config.url_policy)?;

    let guard = create_private_staging_dir(staging_parent)?;
    let dir = crate::files::Directory::open(&guard.0)?;
    let file_name = format!(
        "bundle-{}.tar.gz",
        crate::ids::generate().map_err(Error::from)?
    );
    let mut file = dir.new_file(&file_name)?;

    let mut sink = VerifyingSink {
        file: &mut file,
        verifier: target.bundle_verifier(),
        failed_verify: None,
    };
    let fetched = fetcher.fetch_to_writer(
        target.url(),
        target.size(),
        config.bundle_timeout,
        &mut sink,
    );
    if let Some(verify_err) = sink.failed_verify {
        return Err(PullError::Verify(verify_err));
    }
    let received = fetched?;
    if received != target.size() {
        return Err(PullError::Verify(org_release::Error::PayloadUnderrun));
    }
    sink.verifier.finish()?;

    // Seal staged bundle to 0400, flush durably, rewind the exact held fd, and verify ADR-0009 archive.
    dir.seal_file(&file, false)?;
    file.rewind().map_err(Error::from)?;
    let bundle = ArchiveBundle::with_policy(file, policy, keys)?;
    verified
        .verify_matches_manifest(bundle.manifest())
        .map_err(|_| PullError::CrossBindingMismatch)?;
    Ok(StagedRelease {
        _guard: guard,
        pointer: verified,
        bundle,
    })
}

struct VerifyingSink<'a> {
    file: &'a mut fs::File,
    verifier: PayloadVerifier,
    failed_verify: Option<org_release::Error>,
}

impl Write for VerifyingSink<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Err(err) = self.verifier.update(buf) {
            self.failed_verify = Some(err);
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "bundle payload exceeded signed size",
            ));
        }
        self.file.write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

fn create_private_staging_dir(parent: &Path) -> Result<StagingDirGuard, Error> {
    let canonical_parent = parent.canonicalize().map_err(|_| Error::UnsafeInput)?;
    let run_dir = canonical_parent.join(format!("ao-pull-{}", crate::ids::generate()?));
    org_platform::fs::create_owner_only_dir(&run_dir)
        .and_then(|()| org_platform::fs::set_owner_only_dir(&run_dir))
        .map_err(|_| Error::UnsafeInput)?;
    Ok(StagingDirGuard(run_dir))
}

#[cfg(test)]
#[path = "pull_tests.rs"]
mod tests;
