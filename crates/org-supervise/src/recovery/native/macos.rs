//! macOS recovery observations with immediate identity checks on individual signals.

use std::collections::BTreeMap;
use std::io;
use std::time::Instant;

use super::{
    native_mark, sealed, valid_pid, MarkEvidence, ProcessIdentity, ProcessObservation,
    RecoveryContext, RecoveryProcesses, RecoverySignal, SignalOutcome, TargetEvidence,
    VerifiedTarget,
};

// SDK sys/proc_info.h. libproc reports enumeration lengths in bytes.
const PROC_RUID_ONLY: libc::c_uint = 5;
const PROC_FLAG_LP64: u32 = 0x10;
const PROC_FLAG_PSUGID: u32 = 0x2000;
const MAX_LISTED_PIDS: usize = 131_072;
const MAX_ARG_BYTES: usize = 2 * 1024 * 1024;

/// Native macOS recovery operations. Signals recheck start identity and credentials,
/// and directly marked targets also require the exact current mark. For non-child
/// processes, macOS can still recycle a PID between the final check and `kill`.
pub struct NativeRecovery {
    context: RecoveryContext,
    arg_max: usize,
    captured: BTreeMap<u32, String>,
}

impl NativeRecovery {
    /// Validate the host and initialize bounded native process queries.
    pub fn new() -> io::Result<Self> {
        crate::platform::validate()?;
        // SAFETY: simple identity getters without pointer arguments.
        let (pid, uid) = unsafe { (libc::getpid(), libc::getuid()) };
        let (_, _, boot) = crate::platform::identity(pid)?;
        let mut arg_max = 0i32;
        let mut length = std::mem::size_of_val(&arg_max);
        let mut oid = [libc::CTL_KERN, libc::KERN_ARGMAX];
        // SAFETY: valid OID and correctly sized read-only output slot.
        if unsafe {
            libc::sysctl(
                oid.as_mut_ptr(),
                oid.len() as _,
                (&mut arg_max as *mut i32).cast(),
                &mut length,
                std::ptr::null_mut(),
                0,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        if length != std::mem::size_of_val(&arg_max)
            || !(1..=MAX_ARG_BYTES as i32).contains(&arg_max)
        {
            return Err(invalid("invalid kern.argmax"));
        }
        Ok(Self {
            context: RecoveryContext {
                uid,
                pid: pid as u32,
                boot,
            },
            arg_max: arg_max as usize,
            captured: BTreeMap::new(),
        })
    }

    fn identity(&self, info: &libc::proc_bsdinfo) -> ProcessIdentity {
        ProcessIdentity {
            pid: info.pbi_pid,
            started: start(info),
            boot: self.context.boot.clone(),
        }
    }

    fn mark(&self, info: &libc::proc_bsdinfo, deadline: Instant) -> io::Result<MarkEvidence> {
        check_time(deadline)?;
        if info.pbi_flags & PROC_FLAG_LP64 == 0 {
            return Ok(MarkEvidence::Unreadable);
        }
        let mut bytes = vec![0; self.arg_max + std::mem::size_of::<i32>()];
        let mut count = bytes.len();
        let mut oid = [
            libc::CTL_KERN,
            libc::KERN_PROCARGS2,
            info.pbi_pid as libc::pid_t,
        ];
        // SAFETY: valid OID and allocated output buffer, with no write input.
        let result = unsafe {
            libc::sysctl(
                oid.as_mut_ptr(),
                oid.len() as _,
                bytes.as_mut_ptr().cast(),
                &mut count,
                std::ptr::null_mut(),
                0,
            )
        };
        check_time(deadline)?;
        if result != 0 || count >= bytes.len() {
            return Ok(MarkEvidence::Unreadable);
        }
        Ok(procargs_mark(&bytes[..count]))
    }

    fn pids(&self, deadline: Instant) -> io::Result<Vec<libc::pid_t>> {
        check_time(deadline)?;
        // SAFETY: null sizing query for same-real-UID process IDs.
        let estimate = unsafe {
            libc::proc_listpids(PROC_RUID_ONLY, self.context.uid, std::ptr::null_mut(), 0)
        };
        if estimate <= 0 {
            return Err(io::Error::other("cannot size the recovery process list"));
        }
        let width = std::mem::size_of::<libc::pid_t>();
        let mut capacity = (estimate as usize)
            .div_ceil(width)
            .saturating_add(16)
            .min(MAX_LISTED_PIDS);
        loop {
            check_time(deadline)?;
            let mut pids = vec![0; capacity];
            let size = capacity * width;
            // SAFETY: writable PID buffer with its byte size, bounded to c_int.
            let count = unsafe {
                libc::proc_listpids(
                    PROC_RUID_ONLY,
                    self.context.uid,
                    pids.as_mut_ptr().cast(),
                    size as _,
                )
            };
            if count <= 0 || !(count as usize).is_multiple_of(width) {
                return Err(io::Error::other("invalid recovery process list"));
            }
            if (count as usize) < size {
                pids.truncate(count as usize / width);
                if !pids.contains(&(self.context.pid as libc::pid_t)) {
                    return Err(io::Error::other(
                        "recovery process list omits the supervisor",
                    ));
                }
                return Ok(pids);
            }
            if capacity == MAX_LISTED_PIDS {
                return Err(io::Error::other("recovery process list may be truncated"));
            }
            capacity = (capacity * 2).min(MAX_LISTED_PIDS);
        }
    }
}

impl sealed::Processes for NativeRecovery {}

impl RecoveryProcesses for NativeRecovery {
    fn context(&mut self) -> io::Result<RecoveryContext> {
        Ok(self.context.clone())
    }

    fn snapshot(&mut self, deadline: Instant) -> io::Result<Vec<ProcessObservation>> {
        self.captured.clear();
        let mut rows = Vec::new();
        let mut observations = BTreeMap::new();
        for pid in self.pids(deadline)? {
            check_time(deadline)?;
            if pid <= 1 || pid as u32 == self.context.pid {
                continue;
            }
            let Some(before) = bsd_info(pid as u32)? else {
                continue;
            };
            if before.pbi_ruid != self.context.uid {
                continue;
            }
            let zombie = is_zombie(&before);
            let mark = if zombie || !safe_credentials(&before, self.context.uid) {
                MarkEvidence::Unreadable
            } else {
                self.mark(&before, deadline)?
            };
            // SAFETY: getsid reads a session hint for a positive PID. Errors are not authority.
            let session = u32::try_from(unsafe { libc::getsid(pid) }).unwrap_or(0);
            let Some(after) = bsd_info(pid as u32)? else {
                continue;
            };
            if !same_identity(&before, &after) || after.pbi_ruid != self.context.uid {
                continue;
            }
            let credentials_safe = safe_credentials(&before, self.context.uid)
                && safe_credentials(&after, self.context.uid);
            let zombie = zombie || is_zombie(&after);
            rows.push(ProcessObservation {
                pid: after.pbi_pid,
                identity: Some(self.identity(&after)),
                parent: None,
                group: after.pbi_pgid,
                session,
                uid: after.pbi_ruid,
                credentials_safe,
                zombie,
                mark: if zombie || !credentials_safe {
                    MarkEvidence::Unreadable
                } else {
                    mark
                },
            });
            if observations.insert(after.pbi_pid, after).is_some() {
                return Err(invalid("duplicate PID in native process list"));
            }
        }
        for row in &mut rows {
            check_time(deadline)?;
            let initial_child = &observations[&row.pid];
            let parent_pid = initial_child.pbi_ppid;
            let Some(initial_parent) = observations.get(&parent_pid) else {
                continue;
            };
            if row.zombie || is_zombie(initial_parent) {
                continue;
            }
            let (Some(parent_before), Some(child), Some(parent_after)) = (
                bsd_info(parent_pid)?,
                bsd_info(row.pid)?,
                bsd_info(parent_pid)?,
            ) else {
                continue;
            };
            if same_identity(initial_parent, &parent_before)
                && same_identity(&parent_before, &parent_after)
                && safe_credentials(&parent_before, self.context.uid)
                && safe_credentials(&parent_after, self.context.uid)
                && same_identity(initial_child, &child)
                && child.pbi_ppid == parent_pid
                && !is_zombie(&parent_before)
                && !is_zombie(&parent_after)
                && !is_zombie(&child)
            {
                row.parent = Some(self.identity(&parent_after));
            }
        }
        check_time(deadline)?;
        self.captured = rows
            .iter()
            .filter_map(|row| {
                row.identity
                    .as_ref()
                    .map(|id| (row.pid, id.started.clone()))
            })
            .collect();
        Ok(rows)
    }

    fn signal_checked(
        &mut self,
        target: &VerifiedTarget,
        signal: RecoverySignal,
        deadline: Instant,
    ) -> io::Result<SignalOutcome> {
        check_time(deadline)?;
        let identity = target.identity();
        if !valid_pid(identity.pid)
            || identity.pid == self.context.pid
            || identity.boot != self.context.boot
            || self.captured.get(&identity.pid) != Some(&identity.started)
        {
            return Err(invalid(
                "signal target is not in the current native capture",
            ));
        }
        let Some(before) = bsd_info(identity.pid)? else {
            return Ok(SignalOutcome::Gone);
        };
        if !live_matches(&before, identity) {
            return Ok(SignalOutcome::Gone);
        }
        if !safe_credentials(&before, self.context.uid) {
            return Err(unsafe_target());
        }
        let mark = self.mark(&before, deadline)?;
        let direct = matches!(
            target.evidence(),
            TargetEvidence::Marked | TargetEvidence::RecordedLeader
        );
        if mark == MarkEvidence::Unreadable
            || (direct && mark != MarkEvidence::Present(target.mark().into()))
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "recovery mark changed or is unreadable",
            ));
        }
        let Some(after) = bsd_info(identity.pid)? else {
            return Ok(SignalOutcome::Gone);
        };
        if !live_matches(&after, identity) {
            return Ok(SignalOutcome::Gone);
        }
        if !safe_credentials(&after, self.context.uid) {
            return Err(unsafe_target());
        }
        check_time(deadline)?;
        let signal = match signal {
            RecoverySignal::Terminate => libc::SIGTERM,
            RecoverySignal::Kill => libc::SIGKILL,
        };
        // SAFETY: this positive individual PID was just checked against the authorized
        // identity and credentials. For a non-child, reuse in this remaining interval
        // is a documented limit of macOS's numeric signal interface.
        if unsafe { libc::kill(identity.pid as libc::pid_t, signal) } == 0 {
            return Ok(SignalOutcome::Signalled);
        }
        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ESRCH) {
            Ok(SignalOutcome::Gone)
        } else {
            Err(err)
        }
    }

    fn wait_exited(&mut self, identity: &ProcessIdentity, deadline: Instant) -> io::Result<bool> {
        if identity.boot != self.context.boot {
            return Ok(true);
        }
        if !valid_pid(identity.pid) {
            return Err(invalid("invalid process wait identity"));
        }
        let gone = || -> io::Result<bool> {
            Ok(bsd_info(identity.pid)?.is_none_or(|info| !live_matches(&info, identity)))
        };
        if gone()? {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        let watch = crate::platform::Watch::new(identity.pid as libc::pid_t)?;
        // The registration is only a wake-up mechanism. Confirm the same identity
        // after registering and again after waiting without consuming exit status.
        if gone()? {
            return Ok(true);
        }
        watch.wait(deadline)?;
        gone()
    }

    fn now(&mut self) -> Instant {
        Instant::now()
    }

    fn wait_until(&mut self, deadline: Instant) -> io::Result<()> {
        std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
        Ok(())
    }
}

fn bsd_info(pid: u32) -> io::Result<Option<libc::proc_bsdinfo>> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: the buffer is sized for this libproc flavor and pid fits in pid_t.
    let written = unsafe {
        libc::proc_pidinfo(
            pid as libc::pid_t,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if written <= 0 {
        let error = io::Error::last_os_error();
        return match error.raw_os_error() {
            Some(libc::ESRCH | libc::ENOENT) => Ok(None),
            _ => Err(error),
        };
    }
    if written != size {
        return Err(invalid("short libproc identity record"));
    }
    // SAFETY: libproc reported a complete record.
    let info = unsafe { info.assume_init() };
    if info.pbi_pid != pid {
        return Err(invalid("libproc PID mismatch"));
    }
    Ok(Some(info))
}

fn start(info: &libc::proc_bsdinfo) -> String {
    format!("{}:{}", info.pbi_start_tvsec, info.pbi_start_tvusec)
}

fn same_identity(first: &libc::proc_bsdinfo, second: &libc::proc_bsdinfo) -> bool {
    first.pbi_pid == second.pbi_pid
        && first.pbi_start_tvsec == second.pbi_start_tvsec
        && first.pbi_start_tvusec == second.pbi_start_tvusec
}

fn is_zombie(info: &libc::proc_bsdinfo) -> bool {
    info.pbi_status == libc::SZOMB
}

fn live_matches(info: &libc::proc_bsdinfo, identity: &ProcessIdentity) -> bool {
    info.pbi_pid == identity.pid && start(info) == identity.started && !is_zombie(info)
}

fn safe_credentials(info: &libc::proc_bsdinfo, uid: u32) -> bool {
    uid != 0
        && info.pbi_uid == uid
        && info.pbi_ruid == uid
        && info.pbi_svuid == uid
        && info.pbi_gid == info.pbi_rgid
        && info.pbi_gid == info.pbi_svgid
        && info.pbi_flags & PROC_FLAG_PSUGID == 0
}

fn unsafe_target() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "recovery target credentials are unsafe",
    )
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn check_time(deadline: Instant) -> io::Result<()> {
    if Instant::now() >= deadline {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "native recovery deadline elapsed",
        ))
    } else {
        Ok(())
    }
}

fn procargs_mark(bytes: &[u8]) -> MarkEvidence {
    let Some((header, payload)) = bytes.split_at_checked(std::mem::size_of::<i32>()) else {
        return MarkEvidence::Unreadable;
    };
    let argc = i32::from_ne_bytes(header.try_into().unwrap());
    if argc <= 0 {
        return MarkEvidence::Unreadable;
    }
    let Some(path_end) = payload.iter().position(|b| *b == 0).filter(|end| *end > 0) else {
        return MarkEvidence::Unreadable;
    };
    // XNU aligns the argument vector strings after the saved executable path.
    // PROCARGS2 strips a 16-byte executable_path= prefix, preserving alignment.
    // Skip just that padding, so empty arguments still count as argv entries.
    let args_start = (path_end + 1).next_multiple_of(8);
    let Some(padding) = payload.get(path_end..args_start) else {
        return MarkEvidence::Unreadable;
    };
    if padding.iter().any(|b| *b != 0) {
        return MarkEvidence::Unreadable;
    }
    let mut rest = &payload[args_start..];
    for _ in 0..argc {
        let Some(end) = rest.iter().position(|b| *b == 0) else {
            return MarkEvidence::Unreadable;
        };
        rest = &rest[end + 1..];
    }
    // Restricted processes can have their environment omitted entirely by XNU.
    if rest.iter().all(|b| *b == 0) {
        return MarkEvidence::Unreadable;
    }
    native_mark(rest)
}

#[cfg(test)]
mod tests {
    use super::procargs_mark;
    use crate::recovery::MarkEvidence;

    #[test]
    fn procargs_mark_ignores_argv_and_counts_an_empty_argv_zero() {
        let mut bytes = 2i32.to_ne_bytes().to_vec();
        let path = b"/tmp/tool\0";
        bytes.extend_from_slice(path);
        bytes.resize(4 + path.len().next_multiple_of(8), 0);
        bytes.extend_from_slice(b"\0ORG_SESSION_MARK=argument\0");
        let argv_end = bytes.len();
        bytes.extend_from_slice(b"ORG_SESSION_MARK=environment\0");
        assert_eq!(
            procargs_mark(&bytes),
            MarkEvidence::Present("environment".into())
        );
        assert_eq!(procargs_mark(&bytes[..argv_end]), MarkEvidence::Unreadable);
    }
}
