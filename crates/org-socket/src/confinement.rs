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

/// Why this platform has no web process confinement adapter. Fixed text for the
/// host log; every refusal below names it.
#[cfg(not(target_os = "macos"))]
const NO_WEB_ADAPTER: &str =
    "the web process requires process confinement, which has no adapter on this platform";

/// Whether this platform has a web process confinement adapter. The host asks before
/// any bind, child or secret handoff. macOS: supported. Every other target refuses
/// with the reason until its adapter exists (ADR-0017 LNX-3/LNX-5).
pub fn web_confinement_supported() -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(io::Error::other(NO_WEB_ADAPTER))
    }
}

/// Checks the confinement report of the web process's ready line: `landlock_reported`
/// is whether it carried a Landlock status. macOS has no Landlock, so a report refuses.
/// Every other target refuses until its adapter checks its own report.
pub fn verify_ready_report(landlock_reported: bool) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        if landlock_reported {
            return Err(io::Error::other("the ready line reports landlock on macOS"));
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = landlock_reported;
        Err(io::Error::other(NO_WEB_ADAPTER))
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

#[cfg(test)]
mod web_adapter_tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_supports_the_web_process_and_refuses_a_landlock_report() {
        web_confinement_supported().unwrap();
        verify_ready_report(false).unwrap();
        let error = verify_ready_report(true).unwrap_err();
        assert_eq!(
            error.to_string(),
            "the ready line reports landlock on macOS"
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_target_without_an_adapter_refuses_with_the_reason() {
        for error in [
            web_confinement_supported().unwrap_err(),
            verify_ready_report(false).unwrap_err(),
            verify_ready_report(true).unwrap_err(),
        ] {
            assert_eq!(error.to_string(), NO_WEB_ADAPTER);
        }
    }
}
