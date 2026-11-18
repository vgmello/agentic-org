//! Confinement of the web process.
//!
//! Linux host design section 3.3 (the `confinement` contract of ADR-0017); the macOS
//! requirements are ADR-0008. Narrow OS process-entry confinement: no protocol,
//! storage or domain policy. The web process and the host are the consumers; they
//! keep the wire protocol and decide what a failure means.
//!
//! In the web process: [`disable_core_dumps`], then [`confine_web`]. Call both before
//! runtime/thread creation and before any secret or network input. The host spawn
//! contract closes all inherited descriptors except stdio. We verify that closure
//! rather than closing descriptors owned by safe Rust code.
//!
//! In the host: [`web_confinement_supported`] before any bind, child or secret;
//! [`verify_ready_report`] on the report of the child's ready line; [`verify_web`]
//! after the child's image and peer checks and before any descriptor or secret is
//! sent.
//!
//! # Systems
//!
//! - macOS: the Seatbelt profile of ADR-0008 and the host's `sandbox_check` probes.
//! - GNU/Linux: no adapter yet. Increment X4 writes it (Landlock and seccomp); until
//!   then every adapter function refuses, as on every other target.
//! - Every other Unix target: each adapter function returns the refusal
//!   ([`crate::Refused`], as an [`io::ErrorKind::Unsupported`] error where the
//!   function returns `io::Result`).
//!
//! [`disable_core_dumps`] is the same call on every system and is not an adapter
//! function. [`fill_entropy`] is the same `getentropy` loop on macOS and GNU/Linux and
//! refuses elsewhere.

#![deny(unsafe_op_in_unsafe_fn)]

use std::io;
use std::os::unix::net::UnixStream;
use std::path::Path;

use crate::Refused;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as native;

#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod linux;
#[cfg(all(target_os = "linux", target_env = "gnu"))]
use linux as native;

// Unused in a build for macOS, where only its tests call it. GNU/Linux uses all of it
// but the entropy refusal, until increment X4 writes the Linux adapter.
#[cfg_attr(
    any(target_os = "macos", all(target_os = "linux", target_env = "gnu")),
    allow(dead_code)
)]
mod refused;
#[cfg(not(any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))))]
use refused as native;

#[cfg(any(target_os = "macos", all(target_os = "linux", target_env = "gnu")))]
mod shared;

#[cfg(all(
    test,
    any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))
))]
mod adapter_tests;

#[cfg(test)]
mod tests;

/// What the web process reports about the confinement it applied to itself: the
/// evidence of its ready line. The host trusts it as far as it trusts the image it
/// admitted.
///
/// macOS: no Landlock and no seccomp filter (Seatbelt is observed by the host
/// itself, in [`verify_web`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Enforcement {
    /// The Landlock ABI the process restricted itself under, in full, where it used
    /// Landlock. An adapter that cannot enforce its whole ruleset fails; it reports
    /// nothing.
    pub landlock_abi: Option<u32>,
    /// Whether the process installed its seccomp filter.
    pub seccomp: bool,
}

fn refused() -> io::Error {
    io::Error::other("gateway confinement unavailable")
}

/// Disable core dumps before loading any potentially sensitive process state.
pub fn disable_core_dumps() -> io::Result<()> {
    let limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: a valid initialized input, no descriptors or memory retained by libc.
    if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &limit) } != 0 {
        return Err(refused());
    }
    Ok(())
}

/// Confine this process, the web process, and return its connection to the launch
/// rendezvous together with what it may report in its ready line. The stream is
/// returned only after confinement is in force, so it cannot be used unconfined.
///
/// macOS: apply the reviewed policy, only with verified main/never-threaded entry
/// and exactly the host's three stdio descriptors, then connect to `rendezvous`,
/// the one connection the policy allows. Failure must terminate the caller before
/// readiness. Unsupported platforms never return success.
pub fn confine_web(rendezvous: &Path) -> io::Result<(UnixStream, Enforcement)> {
    native::confine_web(rendezvous)
}

/// Whether this platform has a web process confinement adapter. The host asks before
/// any bind, child or secret handoff. macOS: supported. Every other target refuses
/// with the reason until its adapter exists (ADR-0017 LNX-3/LNX-5).
pub fn web_confinement_supported() -> Result<(), Refused> {
    native::supported()
}

/// Checks the confinement report of the web process's ready line. macOS has no
/// Landlock and no seccomp, so a report of either refuses. Every other target
/// refuses until its adapter checks its own report.
pub fn verify_ready_report(enforcement: &Enforcement) -> io::Result<()> {
    native::verify_ready_report(enforcement)
}

/// Parent-only kernel observation after child image/peer verification: the report
/// is checked as in [`verify_ready_report`], then the child's confinement itself.
/// macOS queries permissions for the critical paths without opening or reading
/// those paths.
pub fn verify_web(pid: u32, enforcement: &Enforcement, critical: &[&Path]) -> io::Result<()> {
    native::verify_web(pid, enforcement, critical)
}

/// OS CSPRNG without opening an ambient random-device path after confinement.
/// Each call uses the OS's bounded getentropy operation; there is no file fallback.
/// macOS and GNU/Linux: `getentropy`. Every other target refuses.
pub fn fill_entropy(bytes: &mut [u8]) -> io::Result<()> {
    native::fill_entropy(bytes)
}
