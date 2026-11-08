//! The relay (`org-agent relay run|status`, `org-relay`): one instance per spool, guarded by an
//! exclusive `flock` on `relay.lock`, moves `outbox/new` into the recipients' inboxes.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
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
        let initial = self.deliver_all()?;
        if once {
            return Ok(RelayOutcome::Once(initial));
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

        let poll_interval = Duration::from_millis(500);
        let mut last_sweep = Instant::now();

        while !is_interrupted() {
            let mut nudged = false;
            if let Some(ref l) = listener {
                while let Ok((mut stream, _)) = l.accept() {
                    let mut buf = [0u8; 16];
                    let _ = stream.read(&mut buf);
                    nudged = true;
                }
            }
            if nudged || self.has_pending_outbox() || last_sweep.elapsed() >= poll_interval {
                let _ = self.deliver_all();
                last_sweep = Instant::now();
            }
            if let Some(ref l) = listener {
                let remaining = poll_interval.saturating_sub(last_sweep.elapsed());
                let wait_slice = remaining.min(Duration::from_millis(2));
                let _ = poll_unix_listener(l, wait_slice);
            } else {
                thread::sleep(Duration::from_millis(2));
            }
        }

        let _ = self.deliver_all();
        let _ = fs::remove_file(&sock_path);
        Ok(RelayOutcome::Stopped)
    }
}
