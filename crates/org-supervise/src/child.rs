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

/// Send process owner. Waiting observes exit without reaping; ownership ends on Drop.
/// Full graceful stopping/cleanup is a later slice. This minimal guard kills the leader's
/// group, then reaps, and does not discover detached tools.
pub struct OwnedChild {
    pub(crate) pid: libc::pid_t,
    pub(crate) group: bool,
    owned: bool,
    pub(crate) identity: Option<ChildIdentity>,
    pub(crate) watch: Option<Watch>,
}

impl OwnedChild {
    pub(crate) fn new(pid: libc::pid_t) -> Self {
        Self {
            pid,
            group: false,
            owned: true,
            identity: None,
            watch: None,
        }
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
                return Ok((pid != 0).then(|| {
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
        if self.try_exit()?.is_some() {
            return Ok(());
        }
        // SAFETY: waitid confirmed ownership; only this owner can reap this child.
        if unsafe { libc::kill(self.pid, libc::SIGTERM) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
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
    }
}
