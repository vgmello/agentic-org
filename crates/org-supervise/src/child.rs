use std::fs::File;
use std::io::{self, Write};
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::platform::Watch;

/// Identity to persist before the first protocol write. Start and boot IDs are opaque
/// platform-native values; a numeric pid alone is never a restart/cleanup identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildIdentity {
    /// Kernel process ID.
    pub pid: u32,
    /// The child's new session/process group ID (equal to pid).
    pub pgid: u32,
    /// Platform-native process start identity.
    pub started: String,
    /// Boot session identity.
    pub boot: String,
    /// Persisted non-secret session mark.
    pub mark: String,
}

/// Idempotently closable, shared stdin. Closing it makes future writes fail BrokenPipe.
/// Writes are nonblocking: the pump must track partial-write offsets and retry WouldBlock
/// while draining output. Do not retry an entire line after a partial write_all failure.
#[derive(Clone)]
pub struct ChildStdin(Arc<Mutex<Option<File>>>);

impl ChildStdin {
    pub(crate) fn new(file: File) -> Self {
        Self(Arc::new(Mutex::new(Some(file))))
    }

    /// Close the pipe once. All clones observe the close.
    pub fn close(&self) -> io::Result<()> {
        self.0
            .lock()
            .map_err(|_| io::Error::other("stdin lock poisoned"))?
            .take();
        Ok(())
    }
}

impl Write for ChildStdin {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut guard = self
            .0
            .lock()
            .map_err(|_| io::Error::other("stdin lock poisoned"))?;
        guard
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "stdin closed"))?
            .write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        let mut guard = self
            .0
            .lock()
            .map_err(|_| io::Error::other("stdin lock poisoned"))?;
        guard
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "stdin closed"))?
            .flush()
    }
}

/// The single child owner plus the pump's streams. No clone of either reader is retained.
pub struct Spawned {
    /// Owning process handle. Drop kills its group and reaps this child.
    pub child: OwnedChild,
    /// Protocol writer, released only after identity persistence.
    pub stdin: ChildStdin,
    /// Exactly one stdout reader, owned by the pump even during/after stopping.
    pub stdout: File,
    /// Exactly one stderr reader, owned by the pump even during/after stopping.
    pub stderr: File,
}

/// Send process owner. Waiting observes exit without reaping. Stop, cleanup and reaping
/// are explicit, retaining ownership on failure. Drop is an emergency group-kill/reap
/// guard. Detached tools still require the later orphan-discovery slice.
pub struct OwnedChild {
    pub(crate) pid: libc::pid_t,
    pub(crate) group: bool,
    owned: bool,
    pub(crate) identity: Option<ChildIdentity>,
    pub(crate) watch: Option<Watch>,
    pub(crate) cleanup_dir: Option<crate::files::PrivateDirectory>,
}

// Keep the whole exceptional permission decision together, including the ownership
// recheck after the snapshot. Its refusal paths cannot be provoked by a same-UID live
// fixture, whose group signal succeeds; exercise those paths through these capabilities.
#[cfg(any(target_os = "macos", test))]
fn tolerate_zombie_group_eperm(
    mut exited: impl FnMut() -> io::Result<bool>,
    singleton: impl FnOnce() -> io::Result<bool>,
) -> io::Result<bool> {
    Ok(exited()? && singleton()? && exited()?)
}

impl OwnedChild {
    pub(crate) fn new(pid: libc::pid_t) -> Self {
        Self {
            pid,
            group: false,
            owned: true,
            identity: None,
            watch: None,
            cleanup_dir: None,
        }
    }

    pub(crate) fn release_after_record(&mut self) -> io::Result<()> {
        #[cfg(target_os = "macos")]
        if self.try_exit()?.is_none() {
            // SAFETY: the leader is our unreaped child; no identity can be reused here.
            if unsafe { libc::kill(self.pid, libc::SIGCONT) } != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    /// The recorded identity. Available on every successfully spawned public handle.
    pub fn identity(&self) -> &ChildIdentity {
        self.identity
            .as_ref()
            .expect("identity captured before exposing child")
    }

    /// Observe an exit, leaving a zombie and its PID reserved. Never reaps.
    pub fn try_exit(&mut self) -> io::Result<Option<ExitStatus>> {
        if !self.owned {
            return Err(io::Error::from_raw_os_error(libc::ECHILD));
        }
        loop {
            let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
            // SAFETY: only our own child's pid, valid siginfo slot. WNOWAIT is mandatory.
            let rc = unsafe {
                libc::waitid(
                    libc::P_PID,
                    self.pid as _,
                    info.as_mut_ptr(),
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if rc == 0 {
                // SAFETY: waitid initialized the zeroed siginfo slot.
                let info = unsafe { info.assume_init() };
                #[cfg(target_os = "macos")]
                let (pid, status) = (info.si_pid, info.si_status);
                #[cfg(target_os = "linux")]
                // SAFETY: waitid returns the SIGCHLD variant of this union.
                let (pid, status) = unsafe { (info.si_pid(), info.si_status()) };
                // Darwin reports START_SUSPENDED as CLD_STOPPED even with WEXITED.
                // Only terminal codes are exits; a stop must still be resumed/reaped.
                let exited = matches!(
                    info.si_code,
                    libc::CLD_EXITED | libc::CLD_KILLED | libc::CLD_DUMPED
                );
                return Ok((pid != 0 && exited).then(|| {
                    ExitStatus::from_raw(if info.si_code == libc::CLD_EXITED {
                        status << 8
                    } else {
                        (status & 0x7f)
                            | if info.si_code == libc::CLD_DUMPED {
                                0x80
                            } else {
                                0
                            }
                    })
                }));
            }
            let e = io::Error::last_os_error();
            if e.raw_os_error() == Some(libc::ECHILD) {
                self.owned = false;
            }
            if e.kind() != io::ErrorKind::Interrupted {
                return Err(e);
            }
        }
    }

    /// Wait to a monotonic deadline without reading pipes or reaping. The pump must keep
    /// draining its readers. A held-open pipe does not affect this wait.
    pub fn wait_until(&mut self, deadline: Instant) -> io::Result<Option<ExitStatus>> {
        loop {
            if let Some(status) = self.try_exit()? {
                return Ok(Some(status));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            self.watch
                .as_ref()
                .expect("watch installed before exposing child")
                .wait(deadline)?;
        }
    }

    /// Send TERM only to our unreaped leader. Does not wait, reap, or consume pipes.
    pub fn terminate(&mut self) -> io::Result<()> {
        self.signal_owned(libc::SIGTERM, crate::SignalTarget::Leader)
    }

    pub(crate) fn is_owned(&self) -> bool {
        self.owned
    }

    pub(crate) fn signal_owned(
        &mut self,
        signal: libc::c_int,
        target: crate::SignalTarget,
    ) -> io::Result<()> {
        let exited = self.try_exit()?.is_some();
        if target == crate::SignalTarget::Leader && exited {
            return Ok(());
        }
        if target == crate::SignalTarget::Group && !self.group {
            return Err(io::Error::other("owned process group was not established"));
        }
        loop {
            // SAFETY: waitid verified our unreaped child; its pid/group cannot be reused.
            let result = unsafe {
                match target {
                    crate::SignalTarget::Leader => libc::kill(self.pid, signal),
                    crate::SignalTarget::Group => libc::killpg(self.pid, signal),
                }
            };
            if result == 0 {
                return Ok(());
            }
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                // The child exited between observation and signal, or its group is empty.
                return Ok(());
            }
            #[cfg(target_os = "macos")]
            {
                let pid = self.pid;
                if target == crate::SignalTarget::Group
                    && error.raw_os_error() == Some(libc::EPERM)
                    && tolerate_zombie_group_eperm(
                        || self.try_exit().map(|status| status.is_some()),
                        || crate::platform::group_contains_only_leader(pid),
                    )?
                {
                    // D-B34: any ownership/query error or additional member is unresolved.
                    return Ok(());
                }
            }
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }

    pub(crate) fn kill_owned_group(&mut self) -> io::Result<()> {
        self.signal_owned(libc::SIGKILL, crate::SignalTarget::Group)
    }

    pub(crate) fn reap_after_cleanup(&mut self) -> io::Result<ExitStatus> {
        if self.try_exit()?.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "owned child is still running",
            ));
        }
        loop {
            let mut status = 0;
            // SAFETY: this owner alone reaps this previously observed terminal child.
            let result = unsafe { libc::waitpid(self.pid, &mut status, 0) };
            if result == self.pid {
                self.owned = false;
                return Ok(ExitStatus::from_raw(status));
            }
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ECHILD) {
                self.owned = false;
            }
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        // No signal after ECHILD: some unrelated reaper violated the embedding contract.
        if self.try_exit().is_err() {
            return;
        }
        // SAFETY: this pid is still an unreaped child and cannot have been reused.
        unsafe {
            libc::kill(self.pid, libc::SIGTERM);
            if self.group {
                libc::killpg(self.pid, libc::SIGKILL);
            }
            libc::kill(self.pid, libc::SIGKILL);
            loop {
                let rc = libc::waitpid(self.pid, std::ptr::null_mut(), 0);
                if rc == self.pid {
                    self.owned = false;
                    break;
                }
                let e = io::Error::last_os_error();
                if e.kind() != io::ErrorKind::Interrupted {
                    break;
                }
            }
        }
        if !self.owned {
            if let Some(directory) = self.cleanup_dir.as_mut() {
                let _ = directory.remove();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn zombie_group_eperm_requires_complete_verification_in_order() {
        fn case(
            exits: &[Result<bool, i32>],
            snapshot: Result<bool, i32>,
            expected: Result<bool, i32>,
            expected_trace: &[&str],
        ) {
            let trace = RefCell::new(Vec::new());
            let mut exits = exits.iter();
            let result = tolerate_zombie_group_eperm(
                || {
                    trace.borrow_mut().push("ownership");
                    (*exits.next().expect("unexpected ownership check"))
                        .map_err(io::Error::from_raw_os_error)
                },
                || {
                    trace.borrow_mut().push("snapshot");
                    snapshot.map_err(io::Error::from_raw_os_error)
                },
            )
            .map_err(|error| error.raw_os_error().unwrap());
            assert_eq!(result, expected);
            assert_eq!(trace.into_inner(), expected_trace);
            assert!(exits.next().is_none(), "ownership verification was skipped");
        }
        case(&[Ok(false)], Ok(true), Ok(false), &["ownership"]);
        case(
            &[Err(libc::ECHILD)],
            Ok(true),
            Err(libc::ECHILD),
            &["ownership"],
        );
        case(
            &[Ok(true)],
            Ok(false),
            Ok(false),
            &["ownership", "snapshot"],
        );
        case(
            &[Ok(true)],
            Err(libc::EIO),
            Err(libc::EIO),
            &["ownership", "snapshot"],
        );
        case(
            &[Ok(true), Ok(false)],
            Ok(true),
            Ok(false),
            &["ownership", "snapshot", "ownership"],
        );
        case(
            &[Ok(true), Err(libc::ECHILD)],
            Ok(true),
            Err(libc::ECHILD),
            &["ownership", "snapshot", "ownership"],
        );
        case(
            &[Ok(true), Ok(true)],
            Ok(true),
            Ok(true),
            &["ownership", "snapshot", "ownership"],
        );

        let truncated = tolerate_zombie_group_eperm(
            || Ok(true),
            || Err(io::Error::other("truncated snapshot")),
        )
        .unwrap_err();
        assert_eq!(truncated.kind(), io::ErrorKind::Other);
    }
}
