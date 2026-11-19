//! Private OS identity, event notification and spawn support.
use std::ffi::CStr;
#[cfg(target_os = "macos")]
use std::ffi::CString;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::time::Instant;

use crate::{invalid, unsupported};

pub(crate) fn check(rc: libc::c_int) -> io::Result<()> {
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(rc))
    }
}

pub(crate) fn validate() -> io::Result<()> {
    // SAFETY: querying process credentials and signal disposition, without changing either.
    unsafe {
        let real = libc::getuid();
        let effective = libc::geteuid();
        #[cfg(target_os = "linux")]
        let (saved, tainted) = {
            let (mut real_out, mut effective_out, mut saved) = (0, 0, 0);
            if libc::getresuid(&mut real_out, &mut effective_out, &mut saved) != 0 {
                return Err(io::Error::last_os_error());
            }
            (saved, false)
        };
        #[cfg(target_os = "macos")]
        let (saved, tainted) = (effective, libc::issetugid() != 0);
        // Actual root/set-id integration needs a privileged harness; the complete
        // credential-policy combinations are tested below without changing process IDs.
        if privileged_credentials(real, effective, saved, tainted) {
            return Err(unsupported("refusing root process supervision"));
        }
        let mut action = std::mem::zeroed::<libc::sigaction>();
        if libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut action) != 0 {
            return Err(io::Error::last_os_error());
        }
        if action.sa_sigaction == libc::SIG_IGN || action.sa_flags & libc::SA_NOCLDWAIT != 0 {
            return Err(unsupported(
                "SIGCHLD must preserve unreaped child ownership",
            ));
        }
    }
    check_core_limit(core_limit())?;
    check_version()
}

fn core_limit() -> io::Result<libc::rlimit> {
    let mut limit = std::mem::MaybeUninit::<libc::rlimit>::uninit();
    // SAFETY: querying a process limit with a correctly sized output slot.
    if unsafe { libc::getrlimit(libc::RLIMIT_CORE, limit.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful getrlimit initialized the complete record.
    Ok(unsafe { limit.assume_init() })
}

// This policy seam covers nonzero values even when the test harness already inherited
// hard0, which an unprivileged fixture cannot raise again. Query errors fail closed too.
fn check_core_limit(limit: io::Result<libc::rlimit>) -> io::Result<()> {
    let limit = limit?;
    if limit.rlim_cur != 0 || limit.rlim_max != 0 {
        return Err(unsupported("RLIMIT_CORE soft and hard limits must be 0; disable core dumps at host/server startup before supervising children"));
    }
    Ok(())
}

fn privileged_credentials(
    real: libc::uid_t,
    effective: libc::uid_t,
    saved: libc::uid_t,
    tainted: bool,
) -> bool {
    real == 0 || effective == 0 || saved == 0 || tainted
}

#[cfg(target_os = "macos")]
fn sysctl(name: &str) -> io::Result<String> {
    let name = CString::new(name).unwrap();
    let mut data = [0u8; 256];
    let mut size = data.len();
    // SAFETY: valid output buffer and its byte length; no new value is supplied.
    if unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            data.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    CStr::from_bytes_until_nul(&data[..size])
        .map_err(|_| invalid("invalid system identity"))?
        .to_str()
        .map(str::to_owned)
        .map_err(|_| invalid("invalid system identity"))
}

#[cfg(target_os = "macos")]
fn check_version() -> io::Result<()> {
    if sysctl("kern.osrelease")?
        .split('.')
        .next()
        .and_then(|x| x.parse::<u32>().ok())
        .is_none_or(|x| x < 22)
    {
        return Err(unsupported("org-supervise requires macOS 13 or later"));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn check_version() -> io::Result<()> {
    let release = std::fs::read_to_string("/proc/sys/kernel/osrelease")?;
    let version = |s: &str| -> Option<(u32, u32)> {
        let mut fields = s.split('.');
        Some((fields.next()?.parse().ok()?, fields.next()?.parse().ok()?))
    };
    // SAFETY: glibc returns a static, NUL-terminated version string.
    let glibc = unsafe { CStr::from_ptr(libc::gnu_get_libc_version()) }
        .to_str()
        .map_err(|_| invalid("invalid libc version"))?;
    if version(&release).is_none_or(|v| v < (5, 10)) || version(glibc).is_none_or(|v| v < (2, 34)) {
        return Err(unsupported(
            "org-supervise requires Linux 5.10 and glibc 2.34 or later",
        ));
    }
    Ok(())
}

pub(crate) struct Watch(OwnedFd);

impl Watch {
    pub(crate) fn new(pid: libc::pid_t) -> io::Result<Self> {
        #[cfg(target_os = "macos")]
        {
            // SAFETY: kqueue returns a new owned descriptor; event pointers are valid.
            let fd = unsafe { libc::kqueue() };
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: ownership of the newly created descriptor.
            let fd = unsafe { OwnedFd::from_raw_fd(fd) };
            // SAFETY: valid descriptor, flags do not transfer ownership.
            if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
                return Err(io::Error::last_os_error());
            }
            let event = libc::kevent {
                ident: pid as usize,
                filter: libc::EVFILT_PROC,
                flags: libc::EV_ADD | libc::EV_ENABLE,
                fflags: libc::NOTE_EXIT,
                data: 0,
                udata: std::ptr::null_mut(),
            };
            // SAFETY: valid initialized registration and owned queue. No events requested yet.
            if unsafe {
                libc::kevent(
                    fd.as_raw_fd(),
                    &event,
                    1,
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null(),
                )
            } < 0
            {
                let e = io::Error::last_os_error();
                // A fast child may already be a zombie: waitid is authoritative before waiting.
                if e.raw_os_error() != Some(libc::ESRCH) {
                    return Err(e);
                }
            }
            Ok(Self(fd))
        }
        #[cfg(target_os = "linux")]
        {
            // SAFETY: syscall arguments are an owned unreaped child pid and zero flags.
            let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: pidfd_open returns a new owned close-on-exec descriptor.
            Ok(Self(unsafe { OwnedFd::from_raw_fd(fd as i32) }))
        }
    }

    pub(crate) fn wait(&self, deadline: Instant) -> io::Result<()> {
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            #[cfg(target_os = "macos")]
            let rc = {
                let time = libc::timespec {
                    tv_sec: remaining.as_secs().min(i64::MAX as u64) as _,
                    tv_nsec: remaining.subsec_nanos() as _,
                };
                let mut event = std::mem::MaybeUninit::<libc::kevent>::uninit();
                // SAFETY: valid queue, output slot and finite timeout.
                unsafe {
                    libc::kevent(
                        self.0.as_raw_fd(),
                        std::ptr::null(),
                        0,
                        event.as_mut_ptr(),
                        1,
                        &time,
                    )
                }
            };
            #[cfg(target_os = "linux")]
            let rc = {
                let mut pfd = libc::pollfd {
                    fd: self.0.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                let millis = remaining
                    .as_millis()
                    .saturating_add(u128::from(
                        !remaining.subsec_nanos().is_multiple_of(1_000_000),
                    ))
                    .min(i32::MAX as u128) as i32;
                // SAFETY: valid poll slot and finite timeout; pidfd pins the child identity.
                unsafe { libc::poll(&mut pfd, 1, millis) }
            };
            if rc >= 0 {
                return Ok(());
            }
            let e = io::Error::last_os_error();
            if e.kind() != io::ErrorKind::Interrupted {
                return Err(e);
            }
        }
    }
}

pub(crate) fn identity(pid: libc::pid_t) -> io::Result<(u32, String, String)> {
    #[cfg(target_os = "macos")]
    {
        let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
        let size = std::mem::size_of::<libc::proc_bsdinfo>();
        // SAFETY: proc_pidinfo writes at most size bytes into the correctly sized slot.
        let rc = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                info.as_mut_ptr().cast(),
                size as i32,
            )
        };
        if rc != size as i32 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: a complete proc_bsdinfo was returned.
        let info = unsafe { info.assume_init() };
        Ok((
            info.pbi_pgid,
            format!("{}:{}", info.pbi_start_tvsec, info.pbi_start_tvusec),
            sysctl("kern.bootsessionuuid")?,
        ))
    }
    #[cfg(target_os = "linux")]
    {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
        let tail = stat
            .rsplit_once(')')
            .ok_or_else(|| invalid("invalid process stat"))?
            .1;
        let fields: Vec<_> = tail.split_whitespace().collect();
        let pgid = fields
            .get(2)
            .and_then(|x| x.parse().ok())
            .ok_or_else(|| invalid("invalid process group"))?;
        let started = fields
            .get(19)
            .ok_or_else(|| invalid("invalid process start identity"))?
            .to_string();
        let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
        Ok((pgid, started, boot.trim().to_owned()))
    }
}

/// Called only for an exited WNOWAIT-owned leader after group signalling returned EPERM.
/// XNU kern_sig.c:1612-1621 filters SZOMB during group iteration, then returns EPERM when
/// no live member was signalled. Never infer an empty group from that error alone.
/// https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_sig.c#L1612-L1621
#[cfg(target_os = "macos")]
pub(crate) fn group_contains_only_leader(pid: libc::pid_t) -> io::Result<bool> {
    group_contains_only_leader_with(pid, |members| {
        let (buffer, bytes) = match members {
            None => (std::ptr::null_mut(), 0),
            Some(members) => (
                members.as_mut_ptr().cast(),
                std::mem::size_of_val(members) as i32,
            ),
        };
        // proc_listpgrppids returns a COUNT, whereas proc_listpids returns BYTES. The
        // null-buffer estimate may cover all system processes; it is only a size hint.
        // SAFETY: null sizing query or initialized pid buffer; query confined to held pgid.
        let count = unsafe { libc::proc_listpgrppids(pid, buffer, bytes) };
        if count <= 0 {
            return Err(io::Error::other(
                "cannot enumerate the retained leader's group",
            ));
        }
        Ok(count as usize)
    })
}

#[cfg(target_os = "macos")]
fn group_contains_only_leader_with(
    pid: libc::pid_t,
    mut list: impl FnMut(Option<&mut [libc::pid_t]>) -> io::Result<usize>,
) -> io::Result<bool> {
    let estimate = list(None)?;
    if estimate == 0 {
        return Err(io::Error::other("empty group size estimate"));
    }
    let capacity = estimate
        .checked_add(16)
        .filter(|size| *size <= i32::MAX as usize / std::mem::size_of::<libc::pid_t>())
        .ok_or_else(|| invalid("process group snapshot exceeds native buffer size"))?;
    let mut members = Vec::new();
    members
        .try_reserve_exact(capacity)
        .map_err(io::Error::other)?;
    members.resize(capacity, 0);
    let count = list(Some(&mut members))?;
    if count == 0 || count >= capacity {
        return Err(io::Error::other(
            "empty or truncated process group snapshot",
        ));
    }
    Ok(count == 1 && members[0] == pid)
}

#[cfg(test)]
mod tests {
    #[test]
    fn core_dump_limits_and_query_errors_fail_closed() {
        for (soft, hard) in [
            (1, 1),
            (0, 1),
            (1, 0),
            (libc::RLIM_INFINITY, libc::RLIM_INFINITY),
        ] {
            let error = super::check_core_limit(Ok(libc::rlimit {
                rlim_cur: soft,
                rlim_max: hard,
            }))
            .unwrap_err();
            assert!(error.to_string().contains("RLIMIT_CORE"));
        }
        super::check_core_limit(Ok(libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        }))
        .unwrap();
        let error = super::check_core_limit(Err(std::io::Error::from_raw_os_error(libc::EACCES)))
            .unwrap_err();
        assert_eq!(error.raw_os_error(), Some(libc::EACCES));
    }

    #[test]
    fn refuses_every_root_credential_and_setid_provenance() {
        assert!(!super::privileged_credentials(501, 501, 501, false));
        assert!(super::privileged_credentials(0, 501, 501, false));
        assert!(super::privileged_credentials(501, 0, 501, false));
        assert!(super::privileged_credentials(501, 501, 0, false));
        assert!(super::privileged_credentials(501, 501, 501, true));
    }
}

#[cfg(all(test, target_os = "macos"))]
mod zombie_group_tests {
    use super::*;
    #[test]
    fn only_the_complete_single_owned_leader_snapshot_is_accepted() {
        for (snapshot, expected) in [(vec![19], true), (vec![19, 20], false), (vec![20], false)] {
            assert_eq!(
                group_contains_only_leader_with(19, |members| {
                    let Some(members) = members else {
                        return Ok(2);
                    };
                    members[..snapshot.len()].copy_from_slice(&snapshot);
                    Ok(snapshot.len())
                })
                .unwrap(),
                expected
            );
        }
        assert!(
            group_contains_only_leader_with(19, |_| Err(io::Error::other("sizing denied")))
                .is_err()
        );
        assert!(
            group_contains_only_leader_with(19, |members| if members.is_none() {
                Ok(2)
            } else {
                Err(io::Error::other("enumeration denied"))
            })
            .is_err()
        );
        assert!(
            group_contains_only_leader_with(19, |members| Ok(members.map_or(2, |p| p.len())))
                .is_err()
        );
        assert!(
            group_contains_only_leader_with(19, |members| Ok(if members.is_none() {
                2
            } else {
                0
            }))
            .is_err()
        );
        assert!(group_contains_only_leader_with(19, |_| Ok(0)).is_err());
        assert!(group_contains_only_leader_with(19, |_| Ok(usize::MAX)).is_err());
    }
}
