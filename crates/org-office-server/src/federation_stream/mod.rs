//! Event-driven persistent mTLS federation transport primitives (`TASK-140`) and
//! HQ-to-satellite session-control RPC proxy (`TASK-143` AC #3).
//!
//! Keeps `#![forbid(unsafe_code)]` intact by pairing safe non-blocking `UnixStream::pair()`
//! self-pipes with [`org_socket::poll_readable_either`] for sub-millisecond bidirectional wakeups
//! and zero idle polling CPU.

mod peer_registry;
mod persistent;
mod satellite_rpc;

use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

pub use self::peer_registry::HqFederationHandle;

pub(crate) use self::peer_registry::{HqPeerHandle, HqPeerRegistry};
pub(crate) use self::persistent::{run_persistent_satellite_sync, PersistentSyncHooks};
pub(crate) use self::satellite_rpc::{spawn_satellite_session_rpc, SatRpcContext};

const MAX_INFLIGHT_PROXIED_RPCS: usize = 64;
const MAX_SATELLITE_RPC_WORKERS: usize = 8;
const KEEPALIVE_IDLE: Duration = Duration::from_secs(15);
const KEEPALIVE_TIMEOUT: Duration = Duration::from_secs(10);
const RECONNECT_MIN: Duration = Duration::from_millis(100);
const RECONNECT_MAX: Duration = Duration::from_secs(5);
const IDLE_POLL_SLICE: Duration = Duration::from_millis(500);

/// Returns `true` when `method` is allowed to be proxied from HQ to a satellite office where
/// the target agent is placed (`TASK-143` AC #3).
pub(crate) fn is_proxied_session_method(method: &str) -> bool {
    matches!(
        method,
        "member.session_inspect"
            | "member.session_list"
            | "member.session_approval"
            | "member.session_stop"
            | "member.session_interrupt"
            | "member.session_steer"
            | "member.session_start"
            | "member.session_transcript"
            | "member.session_clear_stall"
            | "member.session_grants"
    )
}

/// Safe, non-blocking self-pipe used to interrupt `poll_readable_either` immediately when local
/// outbox items, org deltas, relays, or proxied RPC requests arrive.
pub(crate) struct WakePipe {
    read: UnixStream,
    write: Mutex<UnixStream>,
}

impl WakePipe {
    pub(crate) fn new() -> io::Result<Self> {
        let (read, write) = UnixStream::pair()?;
        read.set_nonblocking(true)?;
        write.set_nonblocking(true)?;
        Ok(Self {
            read,
            write: Mutex::new(write),
        })
    }

    pub(crate) fn notify(&self) {
        if let Ok(mut w) = self.write.lock() {
            let _ = w.write(&[1u8]);
        }
    }

    pub(crate) fn drain(&self) {
        let mut buf = [0u8; 64];
        loop {
            match (&self.read).read(&mut buf) {
                Ok(0) => break,
                Ok(_) => continue,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    }

    pub(crate) fn read_fd(&self) -> RawFd {
        self.read.as_raw_fd()
    }
}

/// Combined condvar + self-pipe wake signal for the satellite persistent mTLS loop (`TASK-140`).
/// Calling [`Self::notify`] wakes BOTH a connected `poll_readable_either` wait AND a disconnected
/// reconnect backoff wait in under 0.1 ms.
pub(crate) struct SatSyncWake {
    gen: Mutex<u64>,
    cv: Condvar,
    pipe: Option<WakePipe>,
}

impl Default for SatSyncWake {
    fn default() -> Self {
        Self {
            gen: Mutex::new(0),
            cv: Condvar::new(),
            pipe: WakePipe::new().ok(),
        }
    }
}

impl SatSyncWake {
    pub(crate) fn notify(&self) {
        let mut g = self.gen.lock().unwrap_or_else(|p| p.into_inner());
        *g = g.saturating_add(1);
        self.cv.notify_all();
        drop(g);
        if let Some(pipe) = &self.pipe {
            pipe.notify();
        }
    }

    pub(crate) fn generation(&self) -> u64 {
        *self.gen.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub(crate) fn read_fd(&self) -> Option<RawFd> {
        self.pipe.as_ref().map(WakePipe::read_fd)
    }

    pub(crate) fn drain_pipe(&self) {
        if let Some(pipe) = &self.pipe {
            pipe.drain();
        }
    }

    pub(crate) fn wait_or_stop(&self, seen: u64, timeout: Duration, stop: &AtomicBool) -> u64 {
        let deadline = Instant::now() + timeout;
        let mut g = self.gen.lock().unwrap_or_else(|p| p.into_inner());
        while *g == seen && !stop.load(Ordering::SeqCst) {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            let step = deadline
                .saturating_duration_since(now)
                .min(Duration::from_millis(100));
            let (next, _) = self
                .cv
                .wait_timeout(g, step)
                .unwrap_or_else(|p| p.into_inner());
            g = next;
        }
        *g
    }
}

#[cfg(test)]
mod tests;
