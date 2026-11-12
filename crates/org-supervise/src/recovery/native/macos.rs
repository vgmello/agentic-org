//! macOS recovery observations with immediate identity checks on individual signals.

use std::collections::BTreeMap;
use std::io;
use std::time::{Duration, Instant};

use super::{
    native_mark, sealed, valid_pid, MarkEvidence, ProcessIdentity, ProcessObservation,
    RecoveryContext, RecoveryProcesses, RecoverySignal, SignalOutcome, VerifiedTarget,
};

// SDK sys/proc_info.h.
const PROC_FLAG_TRACED: u32 = 2;
const PROC_FLAG_LP64: u32 = 0x10;
const PROC_FLAG_PSUGID: u32 = 0x2000;
const MAX_LISTED_PIDS: usize = 131_072;
// SDK sys/sysctl.h. The LP64 kinfo_proc ABI is the same on arm64 and x86_64.
const KINFO_PROC_SIZE: usize = 648;
const KINFO_PROC_PID_OFFSET: usize = 40;
const MAX_ARG_BYTES: usize = 2 * 1024 * 1024;
const EXIT_RECHECK_INTERVAL: Duration = Duration::from_millis(10);

// XNU sys/codesign.h and kern/cs_blobs.h. CS_RESTRICT controls PROCARGS2 redaction.
const CS_OPS_STATUS: libc::c_uint = 0;
const CS_RESTRICT: u32 = 0x0000_0800;

unsafe extern "C" {
    fn csops(
        pid: libc::pid_t,
        ops: libc::c_uint,
        address: *mut libc::c_void,
        size: usize,
    ) -> libc::c_int;
}

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

    fn job_observation(
        &self,
        identity: &ProcessIdentity,
        deadline: Instant,
    ) -> io::Result<Option<super::job::JobObservation>> {
        self.job_observation_with(identity, deadline, bsd_info, |pid| {
            // SAFETY: positive PID validated by the proof helper; checked again after this call.
            unsafe { libc::getsid(pid as libc::pid_t) }
        })
    }

    fn job_observation_with(
        &self,
        identity: &ProcessIdentity,
        deadline: Instant,
        mut observe: impl FnMut(u32) -> io::Result<Option<libc::proc_bsdinfo>>,
        mut session: impl FnMut(u32) -> libc::pid_t,
    ) -> io::Result<Option<super::job::JobObservation>> {
        if self.captured.get(&identity.pid) != Some(&identity.started) {
            return Ok(None);
        }
        let Some(before) = observe(identity.pid)? else {
            return Ok(None);
        };
        let session =
            u32::try_from(session(identity.pid)).map_err(|_| io::Error::last_os_error())?;
        let Some(after) = observe(identity.pid)? else {
            return Ok(None);
        };
        check_time(deadline)?;
        let safe = same_identity(&before, &after)
            && !is_zombie(&before)
            && !is_zombie(&after)
            && safe_credentials(&before, self.context.uid)
            && safe_credentials(&after, self.context.uid);
        Ok(Some(super::job::JobObservation {
            identity: self.identity(&after),
            order: (after.pbi_start_tvsec, after.pbi_start_tvusec),
            group: after.pbi_pgid,
            session,
            safe,
        }))
    }

    fn mark(&self, info: &libc::proc_bsdinfo, deadline: Instant) -> io::Result<MarkEvidence> {
        check_time(deadline)?;
        let pid = info.pbi_pid as libc::pid_t;
        if info.pbi_flags & PROC_FLAG_LP64 == 0 || !unrestricted(pid) {
            return Ok(MarkEvidence::Unreadable);
        }
        let header = std::mem::size_of::<i32>();
        let mut oid = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
        let mut expected_size = 0;
        // With no output buffer, XNU reports the full launch image size.
        // SAFETY: valid OID and size output, with no read or write data buffer.
        let sizing = unsafe {
            libc::sysctl(
                oid.as_mut_ptr(),
                oid.len() as _,
                std::ptr::null_mut(),
                &mut expected_size,
                std::ptr::null_mut(),
                0,
            )
        };
        check_time(deadline)?;
        if sizing != 0 || !(header..self.arg_max + header).contains(&expected_size) {
            return Ok(MarkEvidence::Unreadable);
        }
        // The extra byte distinguishes a complete image from a larger one
        // truncated to the buffer size if exec raced the sizing query.
        let mut bytes = vec![0; expected_size + 1];
        let mut count = bytes.len();
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
        if result != 0 || count > bytes.len() || !unrestricted(pid) {
            return Ok(MarkEvidence::Unreadable);
        }
        Ok(procargs_mark(&bytes[..count], expected_size))
    }

    fn pids(&self, deadline: Instant) -> io::Result<Vec<libc::pid_t>> {
        check_time(deadline)?;
        // KERN_PROC_RUID sizes the kernel's iteration under the process-list
        // lock and reports ENOMEM instead of silently capping the result.
        let mut oid = [
            libc::CTL_KERN,
            libc::KERN_PROC,
            libc::KERN_PROC_RUID,
            self.context.uid as libc::c_int,
        ];
        let mut capacity = 0;
        // SAFETY: valid OID and size output, with no read or write data buffer.
        if unsafe {
            libc::sysctl(
                oid.as_mut_ptr(),
                oid.len() as _,
                std::ptr::null_mut(),
                &mut capacity,
                std::ptr::null_mut(),
                0,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        let maximum = MAX_LISTED_PIDS * KINFO_PROC_SIZE;
        if capacity < KINFO_PROC_SIZE || capacity > maximum {
            return Err(invalid("invalid recovery process list size"));
        }
        loop {
            check_time(deadline)?;
            let mut bytes = vec![0; capacity];
            let mut count = bytes.len();
            // SAFETY: a valid OID and writable buffer with an exact byte size.
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
            if result == 0 {
                check_time(deadline)?;
                let capture = bytes
                    .get(..count)
                    .ok_or_else(|| invalid("invalid recovery process list length"))?;
                return proc_list_pids(capture, self.context.pid as libc::pid_t);
            }
            let err = io::Error::last_os_error();
            if err.raw_os_error() != Some(libc::ENOMEM) || capacity == maximum {
                return Err(err);
            }
            capacity = (capacity * 2).min(maximum);
        }
    }
}

impl sealed::Processes for NativeRecovery {}

impl RecoveryProcesses for NativeRecovery {
    fn context(&mut self) -> io::Result<RecoveryContext> {
        Ok(self.context.clone())
    }

    fn verify_recorded_job(
        &mut self,
        target: &ProcessIdentity,
        leader: &crate::ChildIdentity,
        deadline: Instant,
    ) -> io::Result<bool> {
        super::job::verify(
            target,
            leader,
            &self.context,
            deadline,
            Instant::now,
            |id| self.job_observation(id, deadline),
        )
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
            let Some(before) = enumerated_info(pid as u32, self.context.uid, &mut rows, bsd_info)?
            else {
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
            let Some(after) = enumerated_info(pid as u32, self.context.uid, &mut rows, bsd_info)?
            else {
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
            let Some(initial_child) = observations.get(&row.pid) else {
                continue;
            };
            let parent_pid = initial_child.pbi_ppid;
            let Some(initial_parent) = observations.get(&parent_pid) else {
                continue;
            };
            if row.zombie || is_zombie(initial_parent) {
                continue;
            }
            let (
                SnapshotInfo::Observed(parent_before),
                SnapshotInfo::Observed(child),
                SnapshotInfo::Observed(parent_after),
            ) = (
                snapshot_info(parent_pid)?,
                snapshot_info(row.pid)?,
                snapshot_info(parent_pid)?,
            )
            else {
                continue;
            };
            if same_identity(initial_parent, &parent_before)
                && same_identity(&parent_before, &parent_after)
                && safe_credentials(&parent_before, self.context.uid)
                && safe_credentials(&parent_after, self.context.uid)
                && observed_parent_pid(initial_child, &child) == Some(parent_pid)
                && !is_zombie(&parent_before)
                && !is_zombie(&parent_after)
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
        signal_with(
            target,
            deadline,
            self.context.uid,
            || bsd_info(identity.pid),
            |before| self.mark(before, deadline),
            || {
                let signal = match signal {
                    RecoverySignal::Terminate => libc::SIGTERM,
                    RecoverySignal::Kill => libc::SIGKILL,
                };
                // SAFETY: this positive individual PID was just checked against the authorized
                // identity and credentials. For a non-child, reuse in this remaining interval
                // is a documented limit of macOS's numeric signal interface.
                if unsafe { libc::kill(identity.pid as libc::pid_t, signal) } == 0 {
                    Ok(SignalOutcome::Signalled)
                } else {
                    let err = io::Error::last_os_error();
                    if err.raw_os_error() == Some(libc::ESRCH) {
                        Ok(SignalOutcome::Gone)
                    } else {
                        Err(err)
                    }
                }
            },
        )
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
        if Instant::now() >= deadline {
            return gone();
        }
        if gone()? {
            return Ok(Instant::now() < deadline);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        let watch = crate::platform::Watch::new(identity.pid as libc::pid_t)?;
        wait_after_registration(deadline, Instant::now, gone, |until| watch.wait(until))
    }

    fn now(&mut self) -> Instant {
        Instant::now()
    }

    fn wait_until(&mut self, deadline: Instant) -> io::Result<()> {
        super::native_wait_until(deadline)
    }
}

fn proc_list_pids(bytes: &[u8], supervisor: libc::pid_t) -> io::Result<Vec<libc::pid_t>> {
    let (rows, remainder) = bytes.as_chunks::<KINFO_PROC_SIZE>();
    if !remainder.is_empty() {
        return Err(invalid("short kinfo_proc record"));
    }
    let pids: Vec<_> = rows
        .iter()
        .map(|row| {
            let pid = &row[KINFO_PROC_PID_OFFSET..][..std::mem::size_of::<libc::pid_t>()];
            libc::pid_t::from_ne_bytes(pid.try_into().unwrap())
        })
        .collect();
    if !pids.contains(&supervisor) {
        return Err(invalid("recovery process list omits the supervisor"));
    }
    Ok(pids)
}

fn wait_after_registration(
    deadline: Instant,
    mut now: impl FnMut() -> Instant,
    mut gone: impl FnMut() -> io::Result<bool>,
    mut wait: impl FnMut(Instant) -> io::Result<()>,
) -> io::Result<bool> {
    // XNU can refuse kqueue registration while libproc still reports the exact
    // process as live. Treat the queue as a hint and keep checking that identity.
    while now() < deadline {
        let exited = gone()?;
        let observed = now();
        if observed >= deadline {
            break;
        }
        if exited {
            return Ok(true);
        }
        wait(deadline.min(observed + EXIT_RECHECK_INTERVAL))?;
    }
    Ok(false)
}

enum SnapshotInfo {
    Observed(libc::proc_bsdinfo),
    Absent,
    Inaccessible,
}

fn snapshot_info(pid: u32) -> io::Result<SnapshotInfo> {
    classify_snapshot_info(bsd_info(pid))
}

fn classify_snapshot_info(
    result: io::Result<Option<libc::proc_bsdinfo>>,
) -> io::Result<SnapshotInfo> {
    match result {
        Ok(Some(info)) => Ok(SnapshotInfo::Observed(info)),
        Ok(None) => Ok(SnapshotInfo::Absent),
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            Ok(SnapshotInfo::Inaccessible)
        }
        Err(error) => Err(error),
    }
}

fn enumerated_info(
    pid: u32,
    uid: u32,
    rows: &mut Vec<ProcessObservation>,
    query: impl FnOnce(u32) -> io::Result<Option<libc::proc_bsdinfo>>,
) -> io::Result<Option<libc::proc_bsdinfo>> {
    match classify_snapshot_info(query(pid))? {
        SnapshotInfo::Observed(info) => Ok(Some(info)),
        SnapshotInfo::Absent => Ok(None),
        SnapshotInfo::Inaccessible => {
            // The real-UID list supplies the PID. Without a readable identity
            // or credentials the row can only contribute to S0 reporting.
            rows.push(ProcessObservation {
                pid,
                identity: None,
                parent: None,
                group: 0,
                session: 0,
                uid,
                credentials_safe: false,
                zombie: false,
                mark: MarkEvidence::Unreadable,
            });
            Ok(None)
        }
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
            1, // Include zombies that have not been reaped.
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

// Keep the read-mark-read-check-signal ordering testable without a racing process.
fn signal_with(
    target: &VerifiedTarget,
    deadline: Instant,
    uid: u32,
    mut info: impl FnMut() -> io::Result<Option<libc::proc_bsdinfo>>,
    mark: impl FnOnce(&libc::proc_bsdinfo) -> io::Result<MarkEvidence>,
    send: impl FnOnce() -> io::Result<SignalOutcome>,
) -> io::Result<SignalOutcome> {
    let before = info();
    check_time(deadline)?;
    let Some(before) = before? else {
        return Ok(SignalOutcome::Gone);
    };
    if !live_matches(&before, target.identity()) {
        return Ok(SignalOutcome::Gone);
    }
    if !safe_credentials(&before, uid) {
        return Err(unsafe_target());
    }
    let mark = mark(&before)?;
    let after = info();
    check_time(deadline)?;
    if !recheck_after_mark(target, after?.as_ref(), &mark, uid)? {
        return Ok(SignalOutcome::Gone);
    }
    check_time(deadline)?;
    let result = send();
    check_time(deadline)?;
    result
}

fn recheck_after_mark(
    target: &VerifiedTarget,
    after: Option<&libc::proc_bsdinfo>,
    mark: &MarkEvidence,
    uid: u32,
) -> io::Result<bool> {
    let Some(after) = after else {
        return Ok(false);
    };
    if !live_matches(after, target.identity()) {
        return Ok(false);
    }
    if !safe_credentials(after, uid) {
        return Err(unsafe_target());
    }
    if !super::signal_mark_matches(target, mark) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "recovery mark changed or is unreadable",
        ));
    }
    Ok(true)
}

fn observed_parent_pid(initial: &libc::proc_bsdinfo, current: &libc::proc_bsdinfo) -> Option<u32> {
    // XNU temporarily reports an attaching debugger as the tracee's parent.
    // Both observations must show ordinary process ancestry.
    (same_identity(initial, current)
        && !is_zombie(current)
        && (initial.pbi_flags | current.pbi_flags) & PROC_FLAG_TRACED == 0
        && initial.pbi_ppid == current.pbi_ppid)
        .then_some(current.pbi_ppid)
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
    super::failure::check_deadline(Instant::now(), deadline, "native recovery deadline elapsed")
}

fn unrestricted(pid: libc::pid_t) -> bool {
    let mut flags = 0u32;
    // SAFETY: CS_OPS_STATUS writes one u32 to the correctly sized output slot.
    let status = unsafe {
        csops(
            pid,
            CS_OPS_STATUS,
            (&mut flags as *mut u32).cast(),
            std::mem::size_of_val(&flags),
        )
    };
    code_signing_allows(status, flags)
}

fn code_signing_allows(status: libc::c_int, flags: u32) -> bool {
    status == 0 && flags & CS_RESTRICT == 0
}

fn procargs_mark(bytes: &[u8], expected_size: usize) -> MarkEvidence {
    if bytes.len() != expected_size {
        return MarkEvidence::Unreadable;
    }
    let Some((header, payload)) = bytes.split_at_checked(std::mem::size_of::<i32>()) else {
        return MarkEvidence::Unreadable;
    };
    let argc = i32::from_ne_bytes(header.try_into().unwrap());
    if argc <= 0 || !payload.len().is_multiple_of(8) || payload.last() != Some(&0) {
        return MarkEvidence::Unreadable;
    }
    let Some(path_end) = payload.iter().position(|b| *b == 0).filter(|end| *end > 0) else {
        return MarkEvidence::Unreadable;
    };
    // XNU aligns the argument vector strings after the saved executable path.
    // PROCARGS2 strips a 16-byte executable_path= prefix, preserving alignment.
    // Skip just that padding. XNU may partially redact captures with empty
    // argv[0]. Subsequent empty arguments still count toward argc.
    let args_start = (path_end + 1).next_multiple_of(8);
    let Some(padding) = payload.get(path_end..args_start) else {
        return MarkEvidence::Unreadable;
    };
    if padding.iter().any(|b| *b != 0) {
        return MarkEvidence::Unreadable;
    }
    let mut rest = &payload[args_start..];
    if rest.first().is_none_or(|byte| *byte == 0) {
        return MarkEvidence::Unreadable;
    }
    for _ in 0..argc {
        let Some(end) = rest.iter().position(|b| *b == 0) else {
            return MarkEvidence::Unreadable;
        };
        rest = &rest[end + 1..];
    }
    // A complete launch image includes strings after argv, even with an empty env.
    if rest.iter().all(|b| *b == 0) {
        return MarkEvidence::Unreadable;
    }
    // The kernel supplies the launch argc and reads the current stack strings.
    // Deliberate same-UID argv mutation can forge the boundary if the process
    // already knows the exact mark. Recovery trusts cooperating same-UID code.
    native_mark(rest)
}

#[cfg(test)]
mod tests {
    use super::{
        proc_list_pids, procargs_mark, wait_after_registration, KINFO_PROC_PID_OFFSET,
        KINFO_PROC_SIZE,
    };
    use crate::recovery::MarkEvidence;
    use std::cell::Cell;
    use std::time::{Duration, Instant};

    fn ordinary_info() -> libc::proc_bsdinfo {
        // SAFETY: proc_bsdinfo is a C record of integer fields and arrays.
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        info.pbi_pid = 42;
        info.pbi_start_tvsec = 12;
        info.pbi_start_tvusec = 34;
        info.pbi_uid = 501;
        info.pbi_ruid = 501;
        info.pbi_svuid = 501;
        info.pbi_gid = 20;
        info.pbi_rgid = 20;
        info.pbi_svgid = 20;
        info
    }

    #[test]
    fn native_job_observation_rechecks_membership_identity_and_credentials() {
        use crate::recovery::{ProcessIdentity, RecoveryContext};
        let info = libc::proc_bsdinfo {
            pbi_pgid: 40,
            ..ordinary_info()
        };
        let identity = ProcessIdentity {
            pid: info.pbi_pid,
            started: super::start(&info),
            boot: "boot".into(),
        };
        let mut native = super::NativeRecovery {
            context: RecoveryContext {
                uid: 501,
                pid: 999,
                boot: "boot".into(),
            },
            arg_max: 4096,
            captured: [(identity.pid, identity.started.clone())].into(),
        };
        for variant in 0..20 {
            let mut before = info;
            let mut after = info;
            match variant {
                1 => after.pbi_start_tvsec += 1,
                2 => after.pbi_start_tvusec += 1,
                3 => after.pbi_pid += 1,
                4 => before.pbi_status = libc::SZOMB,
                5 => after.pbi_status = libc::SZOMB,
                6 => before.pbi_uid = 0,
                7 => after.pbi_uid = 0,
                8 => before.pbi_ruid = 502,
                9 => after.pbi_ruid = 502,
                10 => before.pbi_svuid = 0,
                11 => after.pbi_svuid = 0,
                12 => before.pbi_gid = 0,
                13 => after.pbi_gid = 0,
                14 => before.pbi_rgid = 0,
                15 => after.pbi_rgid = 0,
                16 => before.pbi_svgid = 0,
                17 => after.pbi_svgid = 0,
                18 => after.pbi_flags = super::PROC_FLAG_PSUGID,
                19 => before.pbi_flags = super::PROC_FLAG_PSUGID,
                _ => {}
            }
            let calls = std::cell::RefCell::new(vec![]);
            let mut count = 0;
            let result = native
                .job_observation_with(
                    &identity,
                    Instant::now() + Duration::from_secs(1),
                    |_| {
                        count += 1;
                        calls.borrow_mut().push("identity");
                        Ok(Some(if count == 1 { before } else { after }))
                    },
                    |_| {
                        calls.borrow_mut().push("session");
                        41
                    },
                )
                .unwrap()
                .unwrap();
            assert_eq!(*calls.borrow(), ["identity", "session", "identity"]);
            assert_eq!(result.safe, variant == 0, "variant {variant}");
            assert_eq!(result.group, 40);
            assert_eq!(result.session, 41);
            assert_eq!(
                result.order,
                (after.pbi_start_tvsec, after.pbi_start_tvusec)
            );
        }
        for absent in [1, 2] {
            let mut count = 0;
            assert!(native
                .job_observation_with(
                    &identity,
                    Instant::now() + Duration::from_secs(1),
                    |_| {
                        count += 1;
                        Ok((count != absent).then_some(info))
                    },
                    |_| 41
                )
                .unwrap()
                .is_none());
        }
        for fail_at in [1, 2] {
            let mut count = 0;
            let error = native
                .job_observation_with(
                    &identity,
                    Instant::now() + Duration::from_secs(1),
                    |_| {
                        count += 1;
                        if count == fail_at {
                            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
                        } else {
                            Ok(Some(info))
                        }
                    },
                    |_| 41,
                )
                .err()
                .unwrap();
            assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        }
        let error = native
            .job_observation_with(&identity, Instant::now(), |_| Ok(Some(info)), |_| 41)
            .err()
            .unwrap();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        let error = native
            .job_observation_with(
                &identity,
                Instant::now() + Duration::from_secs(1),
                |_| Ok(Some(info)),
                |_| {
                    // SAFETY: __error returns this thread's writable errno slot.
                    unsafe {
                        *libc::__error() = libc::EPERM;
                    }
                    -1
                },
            )
            .err()
            .unwrap();
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        native.captured.clear();
        assert!(native
            .job_observation_with(
                &identity,
                Instant::now() + Duration::from_secs(1),
                |_| panic!("no observation without captured identity"),
                |_| panic!("no session without captured identity")
            )
            .unwrap()
            .is_none());
    }

    #[test]
    fn native_guards_recheck_identity_credentials_and_mark_after_reading_it() {
        use crate::recovery::{ProcessIdentity, TargetEvidence, VerifiedTarget};
        let info = ordinary_info();
        let target = VerifiedTarget {
            identity: ProcessIdentity {
                pid: 42,
                started: super::start(&info),
                boot: "boot".into(),
            },
            mark: "guard-mark".into(),
            evidence: TargetEvidence::Marked,
        };
        let mark = MarkEvidence::Present("guard-mark".into());
        assert!(super::recheck_after_mark(&target, Some(&info), &mark, 501).unwrap());
        assert!(!super::recheck_after_mark(&target, None, &mark, 501).unwrap());
        for changed in [
            libc::proc_bsdinfo {
                pbi_pid: 43,
                ..info
            },
            libc::proc_bsdinfo {
                pbi_start_tvsec: 13,
                ..info
            },
            libc::proc_bsdinfo {
                pbi_start_tvusec: 35,
                ..info
            },
            libc::proc_bsdinfo {
                pbi_status: libc::SZOMB,
                ..info
            },
        ] {
            assert!(!super::recheck_after_mark(&target, Some(&changed), &mark, 501).unwrap());
        }
        for mark in [
            MarkEvidence::Absent,
            MarkEvidence::Unreadable,
            MarkEvidence::Present("other".into()),
        ] {
            assert!(super::recheck_after_mark(&target, Some(&info), &mark, 501).is_err());
        }
        for changed in [
            libc::proc_bsdinfo { pbi_uid: 0, ..info },
            libc::proc_bsdinfo {
                pbi_ruid: 0,
                ..info
            },
            libc::proc_bsdinfo {
                pbi_svuid: 0,
                ..info
            },
            libc::proc_bsdinfo { pbi_gid: 0, ..info },
            libc::proc_bsdinfo {
                pbi_rgid: 0,
                ..info
            },
            libc::proc_bsdinfo {
                pbi_svgid: 0,
                ..info
            },
            libc::proc_bsdinfo {
                pbi_flags: super::PROC_FLAG_PSUGID,
                ..info
            },
        ] {
            assert_eq!(
                super::recheck_after_mark(&target, Some(&changed), &mark, 501)
                    .unwrap_err()
                    .kind(),
                std::io::ErrorKind::PermissionDenied
            );
        }
    }

    #[test]
    fn native_guards_reject_restricted_or_unknown_code_signing_status() {
        assert!(super::code_signing_allows(0, 0));
        assert!(!super::code_signing_allows(-1, 0));
        assert!(!super::code_signing_allows(0, super::CS_RESTRICT));
    }

    #[test]
    fn native_signal_wiring_refuses_changes_during_the_mark_read() {
        use crate::recovery::{ProcessIdentity, SignalOutcome, TargetEvidence, VerifiedTarget};
        let before = ordinary_info();
        let target = VerifiedTarget {
            identity: ProcessIdentity {
                pid: 42,
                started: super::start(&before),
                boot: "boot".into(),
            },
            mark: "guard-mark".into(),
            evidence: TargetEvidence::Marked,
        };
        let exact = MarkEvidence::Present(target.mark.clone());
        let cases = [
            (Some(before), exact.clone(), "signal"),
            (None, exact.clone(), "gone"),
            (
                Some(libc::proc_bsdinfo {
                    pbi_start_tvsec: 13,
                    ..before
                }),
                exact.clone(),
                "gone",
            ),
            (
                Some(libc::proc_bsdinfo {
                    pbi_uid: 0,
                    ..before
                }),
                exact.clone(),
                "refuse",
            ),
            (
                Some(libc::proc_bsdinfo {
                    pbi_flags: super::PROC_FLAG_PSUGID,
                    ..before
                }),
                exact,
                "refuse",
            ),
            (
                Some(before),
                MarkEvidence::Present("changed".into()),
                "refuse",
            ),
            (Some(before), MarkEvidence::Unreadable, "refuse"),
        ];
        for (after, mark, expected) in cases {
            let queries = Cell::new(0);
            let marked = Cell::new(false);
            let sent = Cell::new(false);
            let outcome = super::signal_with(
                &target,
                Instant::now() + Duration::from_secs(1),
                501,
                || {
                    queries.set(queries.get() + 1);
                    if queries.get() == 1 {
                        assert!(!marked.get());
                        Ok(Some(before))
                    } else {
                        assert!(marked.get());
                        Ok(after)
                    }
                },
                |_| {
                    assert_eq!(queries.get(), 1);
                    marked.set(true);
                    Ok(mark)
                },
                || {
                    assert_eq!(queries.get(), 2);
                    sent.set(true);
                    Ok(SignalOutcome::Signalled)
                },
            );
            assert_eq!(queries.get(), 2);
            assert!(marked.get());
            match expected {
                "signal" => assert_eq!(outcome.unwrap(), SignalOutcome::Signalled),
                "gone" => assert_eq!(outcome.unwrap(), SignalOutcome::Gone),
                _ => assert_eq!(
                    outcome.unwrap_err().kind(),
                    std::io::ErrorKind::PermissionDenied
                ),
            }
            assert_eq!(sent.get(), expected == "signal");
        }
    }

    #[test]
    fn denied_libproc_query_preserves_an_unreadable_same_uid_row() {
        let mut rows = Vec::new();
        let observed = super::enumerated_info(54, 501, &mut rows, |pid| {
            assert_eq!(pid, 54);
            Err(std::io::Error::from_raw_os_error(libc::EPERM))
        })
        .unwrap();
        assert!(observed.is_none());
        let [row] = rows.as_slice() else {
            panic!("expected one S0 row");
        };
        assert_eq!((row.pid, row.uid), (54, 501));
        assert_eq!(row.mark, MarkEvidence::Unreadable);
        assert!(row.identity.is_none() && row.parent.is_none());
        assert!(!row.credentials_safe && !row.zombie);

        let absent = super::enumerated_info(56, 501, &mut rows, |_| Ok(None)).unwrap();
        assert!(absent.is_none());
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn debugger_parent_is_not_process_ancestry() {
        // SAFETY: proc_bsdinfo is a plain C record containing integer fields and arrays.
        let mut child: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        child.pbi_pid = 56;
        child.pbi_ppid = 54;
        child.pbi_start_tvsec = 12;
        let mut traced = child;
        traced.pbi_flags = super::PROC_FLAG_TRACED;
        assert_eq!(super::observed_parent_pid(&child, &child), Some(54));
        assert_eq!(super::observed_parent_pid(&child, &traced), None);
        assert_eq!(super::observed_parent_pid(&traced, &child), None);
    }

    #[test]
    fn process_list_rejects_partial_records_and_missing_supervisor() {
        let mut bytes = vec![0; 2 * KINFO_PROC_SIZE];
        for (index, pid) in [54i32, 56].into_iter().enumerate() {
            let field = index * KINFO_PROC_SIZE + KINFO_PROC_PID_OFFSET;
            bytes[field..field + 4].copy_from_slice(&pid.to_ne_bytes());
        }
        assert_eq!(proc_list_pids(&bytes, 54).unwrap(), [54, 56]);
        assert!(proc_list_pids(&bytes[..bytes.len() - 1], 54).is_err());
        assert!(proc_list_pids(&bytes, 55).is_err());
    }

    #[test]
    fn an_empty_exit_queue_does_not_wait_out_the_deadline() {
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut observations = 0;
        let gone = wait_after_registration(
            deadline,
            Instant::now,
            || {
                observations += 1;
                Ok(observations == 2)
            },
            |until| {
                assert!(
                    until < deadline,
                    "an empty kqueue consumed the exit deadline"
                );
                Ok(())
            },
        )
        .unwrap();
        assert!(gone);
        assert_eq!(observations, 2);
    }

    #[test]
    fn exit_wait_never_starts_an_observation_after_its_deadline() {
        let start = Instant::now();
        let deadline = start + Duration::from_millis(30);
        let clock = Cell::new(start);
        let observations = Cell::new(0);
        let gone = wait_after_registration(
            deadline,
            || clock.get(),
            || {
                observations.set(observations.get() + 1);
                Ok(clock.get() >= deadline)
            },
            |_| {
                clock.set(deadline);
                Ok(())
            },
        )
        .unwrap();
        assert!(!gone, "an observation after the deadline was accepted");
        assert_eq!(observations.get(), 1);
    }

    fn capture(argc: i32, entries: &[u8]) -> Vec<u8> {
        let mut bytes = argc.to_ne_bytes().to_vec();
        let path = b"/tmp/tool\0";
        bytes.extend_from_slice(path);
        bytes.resize(4 + path.len().next_multiple_of(8), 0);
        bytes.extend_from_slice(entries);
        bytes.resize(4 + (bytes.len() - 4).next_multiple_of(8), 0);
        bytes
    }

    #[test]
    fn procargs_mark_ignores_argv_and_counts_later_empty_arguments() {
        let bytes = capture(
            3,
            b"tool\0\0ORG_SESSION_MARK=argument\0ORG_SESSION_MARK=environment\0",
        );
        assert_eq!(
            procargs_mark(&bytes, bytes.len()),
            MarkEvidence::Present("environment".into())
        );
        let unrelated = capture(1, b"tool\0PREFIX_ORG_SESSION_MARK=prefix\0");
        assert_eq!(
            procargs_mark(&unrelated, unrelated.len()),
            MarkEvidence::Absent
        );
    }

    #[test]
    fn procargs_mark_rejects_an_aligned_prefix_shorter_than_the_kernel_size() {
        let prefix = capture(1, b"tool\0ORG_SESSION_MARK=mark\0");
        let full = capture(
            1,
            b"tool\0ORG_SESSION_MARK=mark\0ORG_SESSION_MARK=conflict\0",
        );
        assert_eq!(procargs_mark(&prefix, full.len()), MarkEvidence::Unreadable);
    }

    #[test]
    fn procargs_mark_quarantines_incomplete_or_conflicting_captures() {
        let empty_argv_zero = capture(1, b"\0ORG_SESSION_MARK=mark\0");
        let invalid_argc = capture(100, b"tool\0ORG_SESSION_MARK=mark\0");
        let conflicting = capture(1, b"tool\0ORG_SESSION_MARK=a\0ORG_SESSION_MARK=b\0");
        let valid = capture(1, b"tool\0ORG_SESSION_MARK=mark\0");
        for bad in [
            &empty_argv_zero,
            &invalid_argc,
            &conflicting,
            &valid[..valid.len() - 1],
        ] {
            assert_eq!(procargs_mark(bad, bad.len()), MarkEvidence::Unreadable);
        }
    }
}

#[cfg(test)]
mod failure_tests {
    use super::*;
    use crate::recovery::{RecoveryReport, ScanFailure, ScanFailureKind};

    #[test]
    fn native_deadline_failure_classification_preserves_io() {
        check_time(Instant::now() + std::time::Duration::from_secs(3600)).unwrap();
        let error = check_time(Instant::now() - std::time::Duration::from_nanos(1)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(error.to_string(), "native recovery deadline elapsed");
        assert_eq!(
            ScanFailure {
                error,
                report: RecoveryReport::default()
            }
            .kind(),
            ScanFailureKind::DeadlineElapsed
        );
    }
}
