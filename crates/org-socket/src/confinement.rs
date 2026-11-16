//! Narrow OS process-entry confinement. No protocol, storage or domain policy.
//!
//! Call before runtime/thread creation and before any secret or network input.
//! The host spawn contract closes all inherited descriptors except stdio. We
//! verify that closure rather than closing descriptors owned by safe Rust code.
use std::{io, path::Path};

#[cfg(target_os = "macos")]
mod macos;

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

/// Apply the reviewed macOS policy, only with verified main/never-threaded entry
/// and exactly the host's three stdio descriptors. Failure must terminate the
/// caller before readiness. Unsupported platforms never return success.
pub fn restrict_gateway(rendezvous: &Path) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        macos::restrict(rendezvous)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = rendezvous;
        Err(refused())
    }
}

/// Parent-only kernel observation after child image/peer verification. Queries
/// permissions for critical paths without opening or reading those paths.
pub fn verify_gateway(pid: u32, critical_paths: &[&Path]) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        macos::verify(pid, critical_paths)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (pid, critical_paths);
        Err(refused())
    }
}

/// OS CSPRNG without opening an ambient random-device path after confinement.
/// Each call uses the OS's bounded getentropy operation; there is no file fallback.
pub fn fill_entropy(bytes: &mut [u8]) -> io::Result<()> {
    for chunk in bytes.chunks_mut(256) {
        loop {
            // SAFETY: chunk is exclusive, initialized, at most getentropy's 256-byte limit.
            let status = unsafe { libc::getentropy(chunk.as_mut_ptr().cast(), chunk.len()) };
            if status == 0 {
                break;
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(refused());
            }
        }
    }
    Ok(())
}
