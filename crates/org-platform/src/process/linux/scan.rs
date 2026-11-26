//! Linux process observations and individual signals bound to held pidfds.

use std::collections::BTreeMap;
use std::ffi::CString;
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::fs::MetadataExt;
use std::time::{Duration, Instant};

use crate::process::{
    check_time, valid_pid, JobObservation, MarkEvidence, ProcessIdentity, ProcessObservation,
    RecoveryContext, RecoverySignal, SignalOutcome,
};

mod pidfd;
mod procfs;
mod records;

use pidfd::{descriptor_error, exited, pidfd, pidfd_startup_error, send, vanished};
use procfs::{
    boot_identity, check_mount, mount_id, open_at, pidfd_namespace_matches, read_at, ProcDirectory,
};
use records::{
    credentials, number, ordinary_exec, read_mark, read_stat, sample_mark_allowed,
    verify_signal_stat, Stat,
};

const PROC_RECORD_LIMIT: usize = 64 * 1024;
// status can include 65,536 decimal supplementary group IDs plus other fields.
const PROC_STATUS_LIMIT: usize = 2 * 1024 * 1024;
const MOUNTINFO_LIMIT: usize = 2 * 1024 * 1024;
const ENVIRONMENT_LIMIT: usize = 2 * 1024 * 1024;

/// Native Linux recovery operations. A snapshot retains pidfds for its live processes.
/// Each signal is sent through the matching descriptor after revalidating the target.
pub(in crate::process) struct RecoveryScan {
    procfs: File,
    procfs_mount_id: u64,
    context: RecoveryContext,
    pins: BTreeMap<u32, Pin>,
    /// The mark variable's name followed by `=`.
    mark_prefix: Vec<u8>,
}

impl RecoveryScan {
    /// Open the native adapter, checking the procfs namespace and kernel pidfd support.
    pub(in crate::process) fn new(mark_prefix: Vec<u8>) -> io::Result<Self> {
        crate::process::validate()?;
        let procfs = File::open("/proc").map_err(descriptor_error)?;
        let mut filesystem = std::mem::MaybeUninit::<libc::statfs>::uninit();
        // SAFETY: an open file descriptor and correctly sized output slot.
        if unsafe { libc::fstatfs(procfs.as_raw_fd(), filesystem.as_mut_ptr()) } < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful fstatfs filled the slot.
        if unsafe { filesystem.assume_init() }.f_type != libc::PROC_SUPER_MAGIC as libc::c_long {
            return Err(invalid("/proc is not procfs"));
        }
        // SAFETY: these getters have no pointer arguments or side effects.
        let (pid, uid) = unsafe { (libc::getpid() as u32, libc::getuid()) };
        let deadline = Instant::now() + Duration::from_secs(1);
        let fd = pidfd(pid).map_err(pidfd_startup_error)?;
        send(&fd, 0).map_err(pidfd_startup_error)?;
        let info_path = CString::new(format!("self/fdinfo/{}", fd.as_raw_fd())).unwrap();
        let info = read_at(&procfs, &info_path, PROC_RECORD_LIMIT, Some(deadline))?;
        let info = std::str::from_utf8(&info).map_err(|_| invalid("invalid pidfd metadata"))?;
        if !pidfd_namespace_matches(info, pid)? {
            return Err(invalid(
                "procfs does not match the supervisor PID namespace",
            ));
        }
        let procfs_mount_id = mount_id(&procfs)?;
        check_mount(&procfs, procfs_mount_id, deadline)?;
        let boot = boot_identity(&procfs, deadline)?;
        Ok(Self {
            procfs,
            procfs_mount_id,
            context: RecoveryContext { uid, pid, boot },
            pins: BTreeMap::new(),
            mark_prefix,
        })
    }

    pub(in crate::process) fn context(&self) -> &RecoveryContext {
        &self.context
    }

    fn identity(&self, pid: u32, stat: Stat) -> ProcessIdentity {
        ProcessIdentity {
            pid,
            started: stat.started.to_string(),
            boot: self.context.boot.clone(),
        }
    }

    pub(in crate::process) fn job_observation(
        &self,
        identity: &ProcessIdentity,
        deadline: Instant,
    ) -> io::Result<Option<JobObservation>> {
        self.job_observation_with(identity, deadline, |directory| {
            Ok(directory.metadata()?.uid())
        })
    }

    fn job_observation_with(
        &self,
        identity: &ProcessIdentity,
        deadline: Instant,
        owner: impl FnOnce(&File) -> io::Result<u32>,
    ) -> io::Result<Option<JobObservation>> {
        let Some(pin) = self
            .pins
            .get(&identity.pid)
            .filter(|pin| pin.stat.started.to_string() == identity.started)
        else {
            return Ok(None);
        };
        let Some((directory, stat)) = self.reopen(identity.pid, pin, deadline)? else {
            return Ok(None);
        };
        let credentials = credentials(&directory, deadline)?;
        let safe = !stat.zombie
            && credentials.safe(self.context.uid)
            && owner(&directory)? == self.context.uid
            && ordinary_exec(&directory, deadline)?;
        Ok(Some(JobObservation {
            identity: self.identity(identity.pid, stat),
            order: (stat.started, 0),
            group: stat.group,
            session: stat.session,
            safe,
        }))
    }

    fn sample(&self, pid: u32, deadline: Instant) -> io::Result<Option<(ProcessObservation, Pin)>> {
        let Some((pin, directory)) = self.pin(pid, Some(deadline))? else {
            return Ok(None);
        };
        let credentials = match credentials(&directory, deadline) {
            Ok(credentials) => credentials,
            Err(err) if vanished(&err) && exited(&pin.fd, None)? => return Ok(None),
            Err(err) => return Err(err),
        };
        if credentials.uid[0] != self.context.uid {
            return Ok(None);
        }
        let credentials_safe = credentials.safe(self.context.uid)
            && directory.metadata()?.uid() == self.context.uid
            && ordinary_exec(&directory, deadline)?;
        let mark = if sample_mark_allowed(pin.stat, credentials_safe, pid, self.context.pid) {
            read_mark(&directory, pid, pin.stat, &self.mark_prefix, deadline)?
        } else {
            MarkEvidence::Unreadable
        };
        // A zombie thread-group leader can still have live workers. Only the
        // process pidfd certifies exit of the whole group.
        let exited = exited(&pin.fd, None)?;
        if exited && !pin.stat.zombie {
            return Ok(None);
        }
        Ok(Some((
            ProcessObservation {
                pid,
                identity: Some(self.identity(pid, pin.stat)),
                parent: None,
                group: pin.stat.group,
                session: pin.stat.session,
                uid: credentials.uid[0],
                credentials_safe,
                zombie: exited,
                mark,
            },
            pin,
        )))
    }

    fn pin(&self, pid: u32, deadline: Option<Instant>) -> io::Result<Option<(Pin, File)>> {
        let name = CString::new(pid.to_string()).unwrap();
        for _ in 0..2 {
            if let Some(deadline) = deadline {
                check_time(deadline)?;
            }
            // Open the identity-bound procfs directory before the numeric pidfd
            // lookup. Reading stat through that directory afterward proves the
            // old PID could not have been reaped and reassigned between opens.
            let directory = open_at(
                &self.procfs,
                &name,
                libc::O_DIRECTORY,
                libc::RESOLVE_NO_XDEV,
            );
            let fd = match pidfd(pid) {
                Ok(fd) => fd,
                Err(err) if vanished(&err) => return Ok(None),
                Err(err) => return Err(err),
            };
            let capture = directory.and_then(|directory| {
                let stat = read_stat(&directory, pid, deadline)?;
                Ok((directory, stat))
            });
            match capture {
                Ok((directory, stat)) => {
                    return Ok(Some((Pin { fd, stat }, directory)));
                }
                // Retry a vanished lookup. A hidden procfs row alone is never
                // proof of exit while a pidfd lookup succeeds.
                Err(err) if vanished(&err) => continue,
                Err(err) => return Err(err),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "process identity could not be pinned to its procfs directory",
        ))
    }
    fn reopen(&self, pid: u32, pin: &Pin, deadline: Instant) -> io::Result<Option<(File, Stat)>> {
        check_time(deadline)?;
        let name = CString::new(pid.to_string()).unwrap();
        let capture = open_at(
            &self.procfs,
            &name,
            libc::O_DIRECTORY,
            libc::RESOLVE_NO_XDEV,
        )
        .and_then(|directory| {
            let stat = read_stat(&directory, pid, Some(deadline))?;
            Ok((directory, stat))
        });
        // A live held pidfd excludes PID recycling while this directory was read.
        // An exited pin authorizes no evidence from a numeric replacement.
        if exited(&pin.fd, None)? {
            return Ok(None);
        }
        let (directory, stat) = capture?;
        if stat.started != pin.stat.started {
            return Err(invalid("reopened procfs identity differs from its pidfd"));
        }
        Ok(Some((directory, stat)))
    }
}

impl RecoveryScan {
    pub(in crate::process) fn snapshot(
        &mut self,
        deadline: Instant,
    ) -> io::Result<Vec<ProcessObservation>> {
        self.pins.clear();
        check_mount(&self.procfs, self.procfs_mount_id, deadline)?;
        let mut rows = Vec::new();
        let mut pins = BTreeMap::new();
        let mut directory = ProcDirectory::open(&self.procfs)?;
        while let Some(name) = directory.next(deadline)? {
            let Ok(pid) = number::<u32>(name.to_bytes()) else {
                continue;
            };
            if !valid_pid(pid) || pid == self.context.pid {
                continue;
            }
            if let Some((row, pin)) = self.sample(pid, deadline)? {
                rows.push(row);
                pins.insert(pid, pin);
            }
        }
        for row in &mut rows {
            check_time(deadline)?;
            let child = &pins[&row.pid];
            let Some(parent) = pins.get(&child.stat.parent) else {
                continue;
            };
            if child.stat.zombie || parent.stat.zombie {
                continue;
            }
            // Read PPID after both descriptors were acquired. A live parent fd at
            // the end excludes reuse during the observation of the child's PPID.
            let Some((_directory, current)) = self.reopen(row.pid, child, deadline)? else {
                continue;
            };
            if current.started == child.stat.started
                && current.parent == child.stat.parent
                && !current.zombie
                && !exited(&child.fd, None)?
                && !exited(&parent.fd, None)?
            {
                row.parent = Some(self.identity(current.parent, parent.stat));
            }
        }
        check_mount(&self.procfs, self.procfs_mount_id, deadline)?;
        check_time(deadline)?;
        self.pins = pins;
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
        {
            return Err(invalid("invalid pidfd signal target"));
        }
        let pin = self
            .pins
            .get(&identity.pid)
            .filter(|p| p.stat.started.to_string() == identity.started)
            .ok_or_else(|| invalid("signal target is not pinned by the current snapshot"))?;
        let current = self.reopen(identity.pid, pin, deadline);
        check_time(deadline)?;
        let Some((directory, stat)) = current? else {
            return Ok(SignalOutcome::Gone);
        };
        let verified = (|| {
            verify_signal_stat(stat, target)?;
            let mark = read_mark(&directory, identity.pid, stat, &self.mark_prefix, deadline)?;
            if !mark_accepted(&mark) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "recovery mark changed or is unreadable",
                ));
            }
            if !credentials(&directory, deadline)?.safe(self.context.uid)
                || directory.metadata()?.uid() != self.context.uid
                || !ordinary_exec(&directory, deadline)?
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "recovery target credentials are unsafe",
                ));
            }
            Ok(())
        })();
        check_time(deadline)?;
        let gone = exited(&pin.fd, None);
        check_time(deadline)?;
        if gone? {
            return Ok(SignalOutcome::Gone);
        }
        verified?;
        check_time(deadline)?;
        let result = send(
            &pin.fd,
            match signal {
                RecoverySignal::Terminate => libc::SIGTERM,
                RecoverySignal::Kill => libc::SIGKILL,
            },
        );
        check_time(deadline)?;
        match result {
            Ok(()) => Ok(SignalOutcome::Signalled),
            Err(err) if err.raw_os_error() == Some(libc::ESRCH) => Ok(SignalOutcome::Gone),
            Err(err) => Err(err),
        }
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
        // An already-expired deadline requests one nonblocking observation.
        let deadline = (Instant::now() < deadline).then_some(deadline);
        if let Some(pin) = self
            .pins
            .get(&identity.pid)
            .filter(|p| p.stat.started.to_string() == identity.started)
        {
            return exited(&pin.fd, deadline);
        }
        let pin = self.pin(identity.pid, deadline)?;
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Ok(false);
        }
        let Some((pin, directory)) = pin else {
            return Ok(true);
        };
        drop(directory);
        if pin.stat.started.to_string() != identity.started {
            return Ok(true);
        }
        exited(&pin.fd, deadline)
    }
}

struct Pin {
    fd: OwnedFd,
    stat: Stat,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests;
