//! macOS recovery observations with immediate identity checks on individual signals.

use std::collections::BTreeMap;
use std::io;
use std::time::{Duration, Instant};

use crate::process::{
    check_time, valid_pid, JobObservation, MarkEvidence, ProcessIdentity, ProcessObservation,
    RecoveryContext, RecoverySignal, SignalOutcome,
};

mod libproc;
mod procargs;
mod signal;

use libproc::{
    bsd_info, enumerated_info, is_zombie, live_matches, observed_parent_pid, proc_list_pids,
    safe_credentials, same_identity, snapshot_info, start, SnapshotInfo,
};
use procargs::{procargs_mark, unrestricted};
use signal::{signal_with, wait_after_registration};

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

/// Native macOS recovery operations. Signals recheck start identity and credentials,
/// and directly marked targets also require the exact current mark. For non-child
/// processes, macOS can still recycle a PID between the final check and `kill`.
pub(in crate::process) struct RecoveryScan {
    context: RecoveryContext,
    arg_max: usize,
    captured: BTreeMap<u32, String>,
    /// The mark variable's name followed by `=`.
    mark_prefix: Vec<u8>,
}

impl RecoveryScan {
    /// Validate the host and initialize bounded native process queries.
    pub(in crate::process) fn new(mark_prefix: Vec<u8>) -> io::Result<Self> {
        crate::process::validate()?;
        // SAFETY: simple identity getters without pointer arguments.
        let (pid, uid) = unsafe { (libc::getpid(), libc::getuid()) };
        let (_, _, boot) = super::identity(pid)?;
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
            mark_prefix,
        })
    }

    pub(in crate::process) fn context(&self) -> &RecoveryContext {
        &self.context
    }

    fn identity(&self, info: &libc::proc_bsdinfo) -> ProcessIdentity {
        ProcessIdentity {
            pid: info.pbi_pid,
            started: start(info),
            boot: self.context.boot.clone(),
        }
    }

    pub(in crate::process) fn job_observation(
        &self,
        identity: &ProcessIdentity,
        deadline: Instant,
    ) -> io::Result<Option<JobObservation>> {
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
    ) -> io::Result<Option<JobObservation>> {
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
        Ok(Some(JobObservation {
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
        Ok(procargs_mark(
            &bytes[..count],
            expected_size,
            &self.mark_prefix,
        ))
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

impl RecoveryScan {
    pub(in crate::process) fn snapshot(
        &mut self,
        deadline: Instant,
    ) -> io::Result<Vec<ProcessObservation>> {
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

    pub(in crate::process) fn signal_checked(
        &mut self,
        target: &ProcessIdentity,
        mark_accepted: &dyn Fn(&MarkEvidence) -> bool,
        signal: RecoverySignal,
        deadline: Instant,
    ) -> io::Result<SignalOutcome> {
        check_time(deadline)?;
        let identity = target;
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
            mark_accepted,
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

    pub(in crate::process) fn wait_exited(
        &mut self,
        identity: &ProcessIdentity,
        deadline: Instant,
    ) -> io::Result<bool> {
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
        let watch = crate::process::Watch::new(identity.pid as libc::pid_t)?;
        wait_after_registration(deadline, Instant::now, gone, |until| watch.wait(until))
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests;
