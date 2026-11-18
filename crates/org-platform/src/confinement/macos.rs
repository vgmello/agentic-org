//! The macOS web confinement adapter (ADR-0008): the entry preconditions, the fixed
//! Seatbelt profile and the host's `sandbox_check` probes.

use super::refused;
use std::{ffi::CString, io, mem, path::Path, ptr};

pub(super) use super::shared::fill_entropy;
use super::Enforcement;
use crate::Refused;
use std::os::unix::net::UnixStream;

const PROFILE: &std::ffi::CStr = c"(version 1)(deny default)(allow network-inbound)(deny network-bind)(allow network-outbound (remote unix-socket (path-literal (param \"RENDEZVOUS\"))))";
#[link(name = "sandbox")]
unsafe extern "C" {
    fn sandbox_init_with_parameters(
        profile: *const libc::c_char,
        flags: u64,
        parameters: *const *const libc::c_char,
        error: *mut *mut libc::c_char,
    ) -> libc::c_int;
    fn sandbox_free_error(error: *mut libc::c_char);
    fn sandbox_check(
        pid: libc::pid_t,
        operation: *const libc::c_char,
        filter: libc::c_int,
        ...
    ) -> libc::c_int;
}
unsafe extern "C" {
    fn pthread_is_threaded_np() -> libc::c_int;
}

fn path(path: &Path) -> io::Result<CString> {
    if !path.is_absolute() || path.as_os_str().len() > 4096 {
        return Err(refused());
    }
    CString::new(path.to_str().ok_or_else(refused)?).map_err(|_| refused())
}
fn entry() -> io::Result<()> {
    // SAFETY: these pthread queries have no arguments or mutable process effects.
    if unsafe { libc::pthread_main_np() } != 1 || unsafe { pthread_is_threaded_np() } != 0 {
        return Err(refused());
    }
    // SAFETY: initialized output struct and exact size from the pinned libc SDK binding.
    let mut task: libc::proc_taskinfo = unsafe { mem::zeroed() };
    let size = mem::size_of_val(&task) as libc::c_int;
    // SAFETY: live output buffer of precisely size bytes, querying only this process.
    if unsafe {
        libc::proc_pidinfo(
            libc::getpid(),
            libc::PROC_PIDTASKINFO,
            0,
            (&mut task as *mut libc::proc_taskinfo).cast(),
            size,
        )
    } != size
        || task.pti_threadnum != 1
    {
        return Err(refused());
    }
    // The parent uses POSIX_SPAWN_CLOEXEC_DEFAULT and explicit dup2 of 0/1/2.
    // Four slots suffice to detect any additional descriptor without unbounded
    // allocation or accidentally closing an OwnedFd held by the caller.
    let mut fds: [libc::proc_fdinfo; 4] = unsafe { mem::zeroed() };
    // SAFETY: initialized fixed array, exact byte size, current process only.
    let count = unsafe {
        libc::proc_pidinfo(
            libc::getpid(),
            libc::PROC_PIDLISTFDS,
            0,
            fds.as_mut_ptr().cast(),
            mem::size_of_val(&fds) as libc::c_int,
        )
    };
    let unit = mem::size_of::<libc::proc_fdinfo>() as libc::c_int;
    if count != 3 * unit {
        return Err(refused());
    }
    let mut seen = [false; 3];
    for fd in &fds[..3] {
        let index = usize::try_from(fd.proc_fd)
            .ok()
            .filter(|n| *n < 3)
            .ok_or_else(refused)?;
        if std::mem::replace(&mut seen[index], true) {
            return Err(refused());
        }
    }
    Ok(())
}

pub(super) fn restrict(rendezvous: &Path) -> io::Result<()> {
    let value = path(rendezvous)?;
    if value.as_bytes().len() >= 104 {
        return Err(refused());
    }
    entry()?;
    let parameters = [c"RENDEZVOUS".as_ptr(), value.as_ptr(), ptr::null()];
    let mut error = ptr::null_mut();
    // SAFETY: fixed reviewed C profile and terminated name/value vector; every
    // string lives through this call. No path is interpolated as SBPL source.
    let status = unsafe {
        sandbox_init_with_parameters(PROFILE.as_ptr(), 0, parameters.as_ptr(), &mut error)
    };
    if !error.is_null() {
        // SAFETY: libSandbox owns this error allocation and specifies this free.
        unsafe {
            sandbox_free_error(error);
        }
    }
    if status != 0 {
        return Err(refused());
    }
    Ok(())
}

pub(super) fn verify(pid: u32, paths: &[&Path]) -> io::Result<()> {
    let pid = libc::pid_t::try_from(pid)
        .ok()
        .filter(|p| *p > 0)
        .ok_or_else(refused)?;
    if paths.is_empty() || paths.len() > 32 {
        return Err(refused());
    }
    // SAFETY: NULL operation is libSandbox's process sandbox-state query.
    if unsafe { sandbox_check(pid, ptr::null(), 0) } != 1 {
        return Err(refused());
    }
    for item in paths {
        let item = path(item)?;
        for operation in [c"file-read-data", c"file-write-data"] {
            // SAFETY: the path-filter value 1 is the private libSandbox ABI
            // exercised by the recorded SDK/OS probes, not a public sandbox.h
            // declaration. Its vararg is a live terminated pathname. Query only,
            // never open/read the protected file; ABI failure refuses startup.
            if unsafe { sandbox_check(pid, operation.as_ptr(), 1, item.as_ptr()) } != 1 {
                return Err(refused());
            }
        }
    }
    Ok(())
}

/// What a confined macOS web process reports: Seatbelt has no Landlock ABI and no
/// seccomp filter.
const REPORT: Enforcement = Enforcement {
    landlock_abi: None,
    seccomp: false,
};

pub(super) fn supported() -> Result<(), Refused> {
    Ok(())
}

/// ADR-0008 order: the entry preconditions and the profile, then the one connection
/// the profile allows. Nothing is read from or written to the stream here.
pub(super) fn confine_web(rendezvous: &Path) -> io::Result<(UnixStream, Enforcement)> {
    restrict(rendezvous)?;
    let stream = UnixStream::connect(rendezvous)?;
    Ok((stream, REPORT))
}

pub(super) fn verify_ready_report(enforcement: &Enforcement) -> io::Result<()> {
    if enforcement.landlock_abi.is_some() {
        return Err(io::Error::other("the ready line reports landlock on macOS"));
    }
    if enforcement.seccomp {
        return Err(io::Error::other("the ready line reports seccomp on macOS"));
    }
    Ok(())
}

pub(super) fn verify_web(
    pid: u32,
    enforcement: &Enforcement,
    critical: &[&Path],
) -> io::Result<()> {
    verify_ready_report(enforcement)?;
    verify(pid, critical)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_supports_the_web_process_and_refuses_a_landlock_report() {
        super::super::web_confinement_supported().unwrap();
        super::super::verify_ready_report(&REPORT).unwrap();
        let error = super::super::verify_ready_report(&Enforcement {
            landlock_abi: Some(4),
            seccomp: false,
        })
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "the ready line reports landlock on macOS"
        );
    }

    #[test]
    fn macos_refuses_a_seccomp_report() {
        let error = verify_ready_report(&Enforcement {
            landlock_abi: None,
            seccomp: true,
        })
        .unwrap_err();
        assert_eq!(error.to_string(), "the ready line reports seccomp on macOS");
    }

    #[test]
    fn a_foreign_report_refuses_before_any_probe_of_the_child() {
        // PID 0 and an empty path list would each refuse with the adapter's fixed
        // text; the report is checked first and names its own cause.
        let error = verify_web(
            0,
            &Enforcement {
                landlock_abi: Some(3),
                seccomp: true,
            },
            &[],
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "the ready line reports landlock on macOS"
        );
        let error = verify_web(0, &REPORT, &[]).unwrap_err();
        assert_eq!(error.to_string(), "gateway confinement unavailable");
    }
}
