//! The relay (`org-agent relay run|status`, `org-relay`): one instance per spool, guarded by an
//! exclusive `flock` on `relay.lock`, moves `outbox/new` into the recipients' inboxes.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixListener;
use std::process;
use std::thread;
use std::time::{Duration, Instant};

use crate::meta::{flock, LOCK_EX, LOCK_NB, LOCK_UN};
use crate::safe_read::open_regular_nofollow;
use crate::spool::{is_interrupted, poll_unix_listener, DeliverSummary, Spool, SpoolError};

/// How a [`Spool::run_relay`] call ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayOutcome {
    /// Another relay holds `relay.lock`; nothing was done.
    AlreadyRunning,
    /// `--once`: one delivery pass.
    Once(DeliverSummary),
    /// The loop ran until interrupted (SIGINT/SIGTERM via [`crate::request_interrupt`]).
    Stopped,
}

/// The relay delivers at least this often.
const RELAY_POLL_INTERVAL: Duration = Duration::from_millis(500);
/// Between passes it checks `outbox/new` this often (for writers that didn't nudge it).
const RELAY_PENDING_CHECK: Duration = Duration::from_millis(2);

/// While stuck, the relay waits for a nudge in slices this long.
const RELAY_STUCK_WAIT: Duration = Duration::from_millis(50);

/// Whether the relay loop runs a delivery pass now: on a nudge, once the poll interval has
/// passed, or when something is pending, unless the last pass left a file it couldn't
/// deliver (a transient error), which would otherwise be retried every few milliseconds.
fn should_sweep(
    nudged: bool,
    stuck: bool,
    since_last: Duration,
    pending: impl FnOnce() -> bool,
) -> bool {
    nudged || since_last >= RELAY_POLL_INTERVAL || (!stuck && pending())
}

/// `relay.lock` holds the relay's pid and a newline; never read more than this.
const MAX_PID_BYTES: u64 = 64;

struct RelayLockGuard {
    file: File,
}

impl RelayLockGuard {
    fn try_acquire(spool: &Spool) -> Result<Option<Self>, io::Error> {
        let lock_path = spool.relay_lock_path();
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent)?;
        }
        // Never follows a symlink: `set_len(0)` below would truncate its target.
        let mut file = open_regular_nofollow(
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600),
            &lock_path,
        )?;
        // Nor a hard link to some other file (O_NOFOLLOW can't see one).
        if file.metadata()?.nlink() > 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{} has other hard links; remove it", lock_path.display()),
            ));
        }
        let rc = unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) };
        if rc != 0 {
            return Ok(None);
        }
        file.set_len(0)?;
        let _ = writeln!(file, "{}", process::id());
        let _ = file.sync_all();
        Ok(Some(Self { file }))
    }
}

impl Drop for RelayLockGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = flock(self.file.as_raw_fd(), LOCK_UN);
        }
    }
}

impl Spool {
    /// Whether a relay holds `relay.lock`, and its pid if it wrote one.
    pub fn is_relay_running(&self) -> (bool, Option<u32>) {
        let lock_path = self.relay_lock_path();
        let Ok(file) = open_regular_nofollow(OpenOptions::new().read(true), &lock_path) else {
            return (false, None);
        };
        let rc = unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) };
        if rc == 0 {
            unsafe {
                let _ = flock(file.as_raw_fd(), LOCK_UN);
            }
            (false, None)
        } else {
            let mut s = String::new();
            let _ = file.take(MAX_PID_BYTES).read_to_string(&mut s);
            let pid = s.trim().parse::<u32>().ok();
            (true, pid)
        }
    }

    /// Run the relay: prune `cur/` older than 30 days, deliver what's pending, then (unless
    /// `once`) deliver on every nudge to `relay.sock` and at least every 500 ms until
    /// interrupted.
    pub fn run_relay(&self, once: bool) -> Result<RelayOutcome, SpoolError> {
        let Some(_guard) = RelayLockGuard::try_acquire(self)? else {
            return Ok(RelayOutcome::AlreadyRunning);
        };

        let _ = self.prune_old_cur();
        let initial = self.deliver_all();
        if once {
            return Ok(RelayOutcome::Once(initial?));
        }

        let sock_path = self.relay_socket_path();
        let _ = fs::remove_file(&sock_path);
        let listener = match UnixListener::bind(&sock_path) {
            Ok(l) => {
                let _ = fs::set_permissions(&sock_path, fs::Permissions::from_mode(0o600));
                let _ = l.set_nonblocking(true);
                Some(l)
            }
            Err(_) => None,
        };

        let mut last_sweep = Instant::now();
        // Whether the last pass left a file it couldn't deliver (a transient error): then
        // only a nudge or the poll interval triggers the next pass, not the pending file. The
        // first pass counts too: a stuck file must not stop the relay from starting.
        let mut stuck = initial.is_err();
        // A nudge the wait below already accepted (it drains the socket).
        let mut woke = false;

        while !is_interrupted() {
            let mut nudged = std::mem::take(&mut woke);
            if let Some(ref l) = listener {
                while let Ok((mut stream, _)) = l.accept() {
                    let mut buf = [0u8; 16];
                    let _ = stream.read(&mut buf);
                    nudged = true;
                }
            }
            if should_sweep(nudged, stuck, last_sweep.elapsed(), || {
                self.has_pending_outbox()
            }) {
                stuck = self.deliver_all().is_err();
                last_sweep = Instant::now();
            }
            let remaining = RELAY_POLL_INTERVAL.saturating_sub(last_sweep.elapsed());
            let wait_slice = if stuck {
                // Long enough not to spin, short enough that a stop signal landing just
                // before the wait is seen quickly.
                remaining.min(RELAY_STUCK_WAIT)
            } else {
                remaining.min(RELAY_PENDING_CHECK)
            };
            if let Some(ref l) = listener {
                woke = poll_unix_listener(l, wait_slice);
            } else {
                // Short slices, so a stop signal is seen at once (sleep resumes after EINTR);
                // while stuck, these turns are cheap (no pending check).
                thread::sleep(RELAY_PENDING_CHECK);
            }
        }

        let _ = self.deliver_all();
        let _ = fs::remove_file(&sock_path);
        Ok(RelayOutcome::Stopped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_that_keeps_failing_waits_for_the_poll_interval() {
        let soon = Duration::from_millis(3);
        assert!(should_sweep(false, false, soon, || true), "pending work");
        assert!(!should_sweep(false, false, soon, || false), "nothing to do");
        assert!(
            !should_sweep(false, true, soon, || panic!("not even checked")),
            "stuck: no 2 ms re-sweep"
        );
        assert!(
            should_sweep(true, true, soon, || false),
            "a nudge still delivers"
        );
        assert!(
            should_sweep(false, true, RELAY_POLL_INTERVAL, || false),
            "retried at the poll interval"
        );
    }
}
