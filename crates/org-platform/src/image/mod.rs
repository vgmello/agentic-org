//! Which image is loaded, and which release this build belongs to.
//!
//! Read-only facts. Nothing here authenticates a peer or authorizes a publisher, a
//! credential, a descriptor transfer or a release: the consumer compares what these
//! functions report with the signed release manifest it has already verified.
//!
//! The host (through `org-platform-host`'s admission) and the web process (when it
//! authenticates its own image before it loads its assets) are the consumers. This
//! module needs only the C library, so the web process stays free of every framework.
//!
//! # Systems
//!
//! - macOS: the kernel's CodeDirectory hash of a running image (`csops`,
//!   `csops_audittoken`); releases for `Macos`.
//! - GNU/Linux: the kernel has no CodeDirectory hash and no audit token, so both
//!   image functions return the `image identity` refusal. The Linux rule, the SHA-256
//!   of the executed bytes, needs a hash this crate does not link: the host applies
//!   it in `org_platform_host::admission`. Releases for `Linux`.
//! - Every other Unix target: each function returns its refusal.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as native;

#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod linux;
#[cfg(all(target_os = "linux", target_env = "gnu"))]
use linux as native;

#[cfg_attr(
    any(target_os = "macos", all(target_os = "linux", target_env = "gnu")),
    allow(
        dead_code,
        reason = "refusing fallback module is used on unsupported targets and in unit tests"
    )
)]
mod refused;
#[cfg(not(any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))))]
use refused as native;

use crate::Refused;
use std::io;

/// How this process's own executable image is exposed to it on systems that identify
/// executables by content digest rather than CodeDirectory hash (GNU/Linux).
#[derive(Debug)]
pub enum OwnExecutable {
    /// Readable file descriptor for `/proc/self/exe` (opened once with `O_CLOEXEC`).
    File(std::fs::File),
    /// Executed by the admitted host from a sealed execute-only (`0111`) `memfd` with
    /// `PR_SET_DUMPABLE == 0`, so `/proc/self/exe` is unreadable by design (`doc-64` §3.1).
    SealedMemfd,
}

/// Opens this process's own executable image for SHA-256 verification on GNU/Linux, or
/// reports [`OwnExecutable::SealedMemfd`] when launched from an execute-only sealed memfd
/// while non-dumpable. Systems that identify loaded images by CodeDirectory hash (macOS)
/// or unsupported targets return the `image identity` refusal.
pub fn open_own_executable() -> io::Result<OwnExecutable> {
    native::open_own_executable()
}

/// The kernel's CodeDirectory hash of this running image (macOS). A replaced on-disk
/// executable cannot change this result. A system without a loaded-image query
/// returns the `image identity` refusal: it never answers for another image.
pub fn own_loaded_cdhash() -> io::Result<[u8; 20]> {
    native::own_loaded_cdhash()
}

/// The kernel's CodeDirectory hash of the exact process incarnation an audit token
/// names (macOS). Callers must obtain the token from the kernel (their admitted child
/// or protected peer), never from browser or command bytes. There is no unchecked
/// PID, static-signature, filesystem or environment fallback: a process that has
/// exited or `exec`ed since is refused. A system without audit tokens returns the
/// `image identity` refusal.
pub fn loaded_cdhash_for_audit_token(token: &[u8; 32]) -> io::Result<[u8; 20]> {
    native::loaded_cdhash_for_audit_token(token)
}

/// The operating system a release is built for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleasePlatform {
    /// macOS.
    Macos,
    /// GNU/Linux.
    Linux,
}

/// The processor architecture a release is built for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseArchitecture {
    /// 64-bit ARM.
    Aarch64,
    /// 64-bit x86.
    X86_64,
}

/// Which release this build belongs to. Plain data: a consumer maps it to its own
/// release-manifest types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReleaseTarget {
    /// The operating system.
    pub platform: ReleasePlatform,
    /// The processor architecture.
    pub architecture: ReleaseArchitecture,
}

/// The release target of this build, or the `release target` refusal on a system or
/// an architecture no release is built for.
pub fn release_target() -> Result<ReleaseTarget, Refused> {
    let platform = native::release_platform()?;
    let architecture = release_architecture().ok_or_else(refused::release_target)?;
    Ok(ReleaseTarget {
        platform,
        architecture,
    })
}

/// The architecture of this build, when a release is built for it.
fn release_architecture() -> Option<ReleaseArchitecture> {
    if cfg!(target_arch = "aarch64") {
        Some(ReleaseArchitecture::Aarch64)
    } else if cfg!(target_arch = "x86_64") {
        Some(ReleaseArchitecture::X86_64)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The release target names this build's architecture whenever the adapter names a
    /// platform; a refusing adapter refuses the whole target.
    #[test]
    fn the_release_target_is_the_adapters_platform_with_this_builds_architecture() {
        match native::release_platform() {
            Ok(platform) => {
                let architecture = release_architecture();
                assert_eq!(
                    release_target().ok(),
                    architecture.map(|architecture| ReleaseTarget {
                        platform,
                        architecture
                    })
                );
            }
            Err(refusal) => assert_eq!(release_target(), Err(refusal)),
        }
    }

    /// A refusal of the image query is an `Unsupported` error that keeps the refusal;
    /// an adapter that answers gives this process a hash that is not all zero.
    #[test]
    fn the_own_image_query_answers_or_keeps_its_refusal() {
        match own_loaded_cdhash() {
            Ok(hash) => assert_ne!(hash, [0; 20]),
            Err(error) => {
                assert_eq!(error.kind(), io::ErrorKind::Unsupported);
                assert_eq!(
                    error
                        .get_ref()
                        .and_then(|source| source.downcast_ref::<Refused>()),
                    Some(&refused::image_identity())
                );
            }
        }
    }
}
