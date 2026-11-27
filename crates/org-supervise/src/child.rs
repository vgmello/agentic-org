use std::fs::File;
use std::io::{self, Write};
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use org_platform::posix;
use org_platform::process::{self, Watch};

/// An owned leader or its process group, whose ID remains reserved until reaping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalTarget {
    /// Only the runtime leader, allowing it to stop detached tools itself.
    Leader,
    /// Its process group.
    Group,
}

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
        // The leader is our unreaped child; no identity can be reused here.
        process::release_spawned(self.pid, || self.try_exit().map(|status| status.is_some()))
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
            // Only our own child's pid. The platform observation never reaps (WNOWAIT).
            let e = match process::try_exit(self.pid) {
                Ok(status) => return Ok(status),
                Err(e) => e,
            };
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
        self.signal_owned(libc::SIGTERM, SignalTarget::Leader)
    }

    pub(crate) fn is_owned(&self) -> bool {
        self.owned
    }

    pub(crate) fn signal_owned(
        &mut self,
        signal: libc::c_int,
        target: SignalTarget,
    ) -> io::Result<()> {
        let exited = self.try_exit()?.is_some();
        if target == SignalTarget::Leader && exited {
            return Ok(());
        }
        if target == SignalTarget::Group && !self.group {
            return Err(io::Error::other("owned process group was not established"));
        }
        loop {
            // waitid verified our unreaped child; its pid/group cannot be reused.
            let result = match target {
                SignalTarget::Leader => posix::kill(self.pid, signal),
                SignalTarget::Group => posix::kill_group(self.pid, signal),
            };
            let Err(error) = result else {
                return Ok(());
            };
            if error.raw_os_error() == Some(libc::ESRCH) {
                // The child exited between observation and signal, or its group is empty.
                return Ok(());
            }
            let pid = self.pid;
            if target == SignalTarget::Group
                && process::is_exited_group_denial(&error)
                && tolerate_zombie_group_eperm(
                    || self.try_exit().map(|status| status.is_some()),
                    || process::group_contains_only_leader(pid),
                )?
            {
                // D-B34: any ownership/query error or additional member is unresolved.
                return Ok(());
            }
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }

    pub(crate) fn kill_owned_group(&mut self) -> io::Result<()> {
        self.signal_owned(libc::SIGKILL, SignalTarget::Group)
    }

    pub(crate) fn reap_after_cleanup(&mut self) -> io::Result<ExitStatus> {
        if self.try_exit()?.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "owned child is still running",
            ));
        }
        loop {
            // This owner alone reaps this previously observed terminal child.
            let error = match posix::wait_pid(self.pid, 0) {
                Ok(Some(status)) => {
                    self.owned = false;
                    return Ok(ExitStatus::from_raw(status));
                }
                Ok(None) => continue,
                Err(error) => error,
            };
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
        // This pid is still an unreaped child and cannot have been reused.
        let _ = posix::kill(self.pid, libc::SIGTERM);
        if self.group {
            let _ = posix::kill_group(self.pid, libc::SIGKILL);
        }
        let _ = posix::kill(self.pid, libc::SIGKILL);
        if posix::reap(self.pid).is_ok() {
            self.owned = false;
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
