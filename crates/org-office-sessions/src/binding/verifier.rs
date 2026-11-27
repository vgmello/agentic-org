//! Transport-neutral [`RunBindingVerifier`] seam, [`PeerIdentity`], and [`LocalPeerPidVerifier`]
//! (`TASK-233`).
//!
//! Isolates local Unix socket peer PID ancestry (`libc::pid_t`) behind a thin trait so server
//! request dispatch and connection binding stay transport- and OS-neutral.

use std::num::NonZeroU32;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{ActiveRunBinding, BindingRejection, RunBindingRegistry};

/// Transport-neutral identity of a connected peer presented to [`RunBindingVerifier`].
///
/// Local Unix sockets convert the kernel peer PID once at the accept boundary via
/// [`PeerIdentity::from_raw_pid`]; request routing and binding layers never reference
/// `libc::pid_t`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerIdentity {
    /// A peer on the local host identified by its positive OS process ID (`None` when the
    /// transport supplies no peer PID).
    LocalPid(Option<NonZeroU32>),
}

impl PeerIdentity {
    /// A peer identity carrying no local OS process ID.
    pub const fn none() -> Self {
        Self::LocalPid(None)
    }

    /// Converts a signed process ID (`i32`), failing closed to `None` for non-positive values.
    pub fn from_pid(pid: Option<i32>) -> Self {
        Self::LocalPid(
            pid.and_then(|p| u32::try_from(p).ok())
                .and_then(NonZeroU32::new),
        )
    }

    /// Converts a platform `libc::pid_t` at the local socket accept boundary, failing closed to
    /// `None` for non-positive values.
    pub fn from_raw_pid(pid: Option<libc::pid_t>) -> Self {
        Self::LocalPid(
            pid.and_then(|p| u32::try_from(p).ok())
                .and_then(NonZeroU32::new),
        )
    }

    /// Returns the positive local OS PID when present.
    pub const fn local_pid(&self) -> Option<u32> {
        match self {
            Self::LocalPid(Some(pid)) => Some(pid.get()),
            Self::LocalPid(None) => None,
        }
    }

    pub(crate) fn raw_pid(&self) -> Option<libc::pid_t> {
        self.local_pid()
            .and_then(|pid| libc::pid_t::try_from(pid).ok())
            .filter(|&p| p > 0)
    }
}

/// Verified outcome of binding a connecting agent peer.
#[derive(Debug, Clone)]
pub enum VerifiedRunBinding {
    /// An office-managed run verified against token and peer identity.
    Managed(ActiveRunBinding),
    /// An approved external (BYOA) workspace lease (`TASK-137`).
    External {
        employee_id: String,
        session_id: String,
    },
}

/// Consumer-oriented seam for verifying per-run `AGENT_OS_SESSION_TOKEN` bindings (`TASK-233`).
pub trait RunBindingVerifier: Send + Sync {
    /// Verifies a managed run binding for `(employee_id, presented_token, peer)`.
    fn verify(
        &self,
        employee_id: &str,
        presented_token: Option<&str>,
        peer: &PeerIdentity,
    ) -> Result<ActiveRunBinding, BindingRejection>;

    /// Verifies a connecting peer's run binding, allowing custom verifiers to distinguish managed
    /// runs from external workspace leases. Defaults to [`Self::verify`] mapped to
    /// [`VerifiedRunBinding::Managed`].
    fn verify_binding(
        &self,
        employee_id: &str,
        presented_token: Option<&str>,
        peer: &PeerIdentity,
    ) -> Result<VerifiedRunBinding, BindingRejection> {
        self.verify(employee_id, presented_token, peer)
            .map(VerifiedRunBinding::Managed)
    }

    /// Whether `employee_id` currently holds an active managed run binding.
    fn has_active(&self, employee_id: &str) -> bool;

    /// Whether `employee_id` currently holds an active external lease binding for `session_id`.
    /// Defaults to `false` (fail closed) for verifiers that only verify managed runs.
    fn has_active_external(&self, _employee_id: &str, _session_id: &str) -> bool {
        false
    }
}

fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .unwrap_or(u64::MAX)
}

/// Local-OS [`RunBindingVerifier`] backed by [`RunBindingRegistry`]: constant-time token check
/// plus kernel process-ancestry verification (`CLI-12`, `SUB-3`).
#[derive(Clone)]
pub struct LocalPeerPidVerifier {
    registry: RunBindingRegistry,
}

impl LocalPeerPidVerifier {
    pub fn new(registry: RunBindingRegistry) -> Self {
        Self { registry }
    }

    /// Same as [`RunBindingVerifier::verify`] with an injectable descendant predicate for
    /// deterministic unit and latency tests.
    pub fn verify_with(
        &self,
        employee_id: &str,
        presented_token: Option<&str>,
        peer: &PeerIdentity,
        is_descendant: impl FnOnce(u32, u32) -> bool,
    ) -> Result<ActiveRunBinding, BindingRejection> {
        self.registry.verify_peer_with(
            employee_id,
            presented_token,
            peer.raw_pid(),
            |peer_pid, root_pid| match (u32::try_from(peer_pid), u32::try_from(root_pid)) {
                (Ok(peer_u32), Ok(root_u32)) => is_descendant(peer_u32, root_u32),
                _ => false,
            },
        )
    }
}

impl RunBindingVerifier for LocalPeerPidVerifier {
    fn verify(
        &self,
        employee_id: &str,
        presented_token: Option<&str>,
        peer: &PeerIdentity,
    ) -> Result<ActiveRunBinding, BindingRejection> {
        self.registry
            .verify_peer(employee_id, presented_token, peer.raw_pid())
    }

    fn has_active(&self, employee_id: &str) -> bool {
        self.registry.has_active(employee_id)
    }

    fn has_active_external(&self, employee_id: &str, session_id: &str) -> bool {
        self.registry
            .external_active(employee_id, session_id, current_time_ms())
    }
}
