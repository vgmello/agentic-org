//! Confinement of the web process.
//!
//! Linux host design section 3.3 (the `confinement` contract of ADR-0017); the macOS
//! requirements are ADR-0008. Narrow OS process-entry confinement: no protocol,
//! storage or domain policy. The web process and the host are the consumers; they
//! keep the wire protocol and decide what a failure means.
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
//! function.

#![deny(unsafe_op_in_unsafe_fn)]

use std::io;

use crate::Refused;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as native;

#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod linux;
#[cfg(all(target_os = "linux", target_env = "gnu"))]
use linux as native;

// Unused in a build for macOS, where only its tests call it.
#[cfg_attr(target_os = "macos", allow(dead_code))]
mod refused;
#[cfg(not(any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))))]
use refused as native;

#[cfg(test)]
mod tests;

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

/// Whether this platform has a web process confinement adapter. The host asks before
/// any bind, child or secret handoff. macOS: supported. Every other target refuses
/// with the reason until its adapter exists (ADR-0017 LNX-3/LNX-5).
pub fn web_confinement_supported() -> Result<(), Refused> {
    native::supported()
}
