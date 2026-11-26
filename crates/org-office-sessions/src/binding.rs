//! Per-run cryptographic `AGENT_OS_SESSION_TOKEN` registry and kernel descendant check
//! (`CLI-9`, `CLI-10`, `CLI-12`, `SUB-3`, `B2 Q9`).
//!
//! Two binding modes share one registry and one constant-time token check, in separate maps so
//! a managed teardown never touches an external lease (and the reverse):
//!
//! - [`BindingMode::Managed`]: an office-launched run. Token + kernel peer-PID ancestry.
//! - [`BindingMode::External`]: a BYOA workspace holding an approved lease (`TASK-137`).
//!   Token + an unexpired RAM entry that exists only while the lease is active; the same-UID
//!   peer check is the office socket's accept gate (`peer_allowed`). See [`external`].

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use ring::rand::{SecureRandom, SystemRandom};
use zeroize::Zeroizing;

mod external;

/// Constant-time equality of two secrets (tokens, code halves) through `subtle`, the audited
/// constant-time primitive (`ring`'s slice comparison is deprecated and makes no side-channel
/// promise). Unequal lengths are unequal at once: a fixed-size token's length is not secret.
pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    use subtle::ConstantTimeEq as _;
    bool::from(a.ct_eq(b))
}

/// One accepted `agent-os` command recorded for a live run (`ADR-0030` decision 8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedCommandRecord {
    pub method: String,
    pub summary: String,
}

/// How a live binding authenticates the peer that presents its token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingMode {
    /// An office-launched run: the peer must be `root_pid` or one of its live descendants.
    Managed { root_pid: libc::pid_t },
    /// An external lease's fast token: valid until `expires_at_ms` (the earlier of the token's
    /// own lifetime and the lease's) and only while the lease is active.
    External { expires_at_ms: u64 },
}

/// Live run binding authorizing `agent-os` invocations from an agent process or its descendants
/// (managed) or from an approved external workspace (external).
#[derive(Clone)]
pub struct ActiveRunBinding {
    /// Wiped from memory when the binding (and every clone of it) is dropped.
    token: Zeroizing<String>,
    pub employee_id: String,
    pub run_id: String,
    pub session_id: String,
    pub mode: BindingMode,
    pub accepted_commands: Vec<AcceptedCommandRecord>,
}

impl fmt::Debug for ActiveRunBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActiveRunBinding")
            .field("token", &"<redacted>")
            .field("employee_id", &self.employee_id)
            .field("run_id", &self.run_id)
            .field("session_id", &self.session_id)
            .field("mode", &self.mode)
            .field("accepted_commands_len", &self.accepted_commands.len())
            .finish()
    }
}

impl ActiveRunBinding {
    pub fn expose_token(&self) -> &str {
        self.token.as_str()
    }

    /// The managed run's root process, `None` for an external binding.
    pub fn root_pid(&self) -> Option<libc::pid_t> {
        match self.mode {
            BindingMode::Managed { root_pid } => Some(root_pid),
            BindingMode::External { .. } => None,
        }
    }

    /// Whether this binding is an external (BYOA) lease's fast token.
    pub fn is_external(&self) -> bool {
        matches!(self.mode, BindingMode::External { .. })
    }
}

/// Why a run-binding verification failed (always mapped to opaque `not_authorized` on the socket).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingRejection {
    MissingToken,
    UnknownToken,
    MemberMismatch,
    MissingPeerPid,
    NotDescendant,
    /// An external fast token past its lifetime (or its lease's): refresh by challenge-response.
    Expired,
}

/// Thread-safe registry of live per-run `AGENT_OS_SESSION_TOKEN` bindings on one office: the
/// managed runs and, separately, the external leases' fast tokens (RAM-only, never persisted).
#[derive(Clone, Default)]
pub struct RunBindingRegistry {
    inner: Arc<Mutex<HashMap<String, ActiveRunBinding>>>,
    external: Arc<Mutex<HashMap<String, ActiveRunBinding>>>,
}

impl RunBindingRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Generates a cryptographically random 32-byte hex session token, wiped when dropped.
    pub fn mint_token() -> Result<Zeroizing<String>, String> {
        let mut bytes = Zeroizing::new([0u8; 32]);
        SystemRandom::new()
            .fill(bytes.as_mut())
            .map_err(|_| "system entropy unavailable".to_owned())?;
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = Zeroizing::new(String::with_capacity(64));
        for byte in bytes.iter() {
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        Ok(out)
    }

    /// Issues and stores a new run binding for `employee_id` rooted at `root_pid`.
    /// Replaces any older binding for the same `employee_id`.
    pub fn issue(
        &self,
        employee_id: impl Into<String>,
        run_id: impl Into<String>,
        session_id: impl Into<String>,
        root_pid: libc::pid_t,
    ) -> Result<ActiveRunBinding, String> {
        let token = Self::mint_token()?;
        let binding = ActiveRunBinding {
            token,
            employee_id: employee_id.into(),
            run_id: run_id.into(),
            session_id: session_id.into(),
            mode: BindingMode::Managed { root_pid },
            accepted_commands: Vec::new(),
        };
        let mut map = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        map.insert(binding.employee_id.clone(), binding.clone());
        Ok(binding)
    }

    /// Updates the `root_pid` of an issued binding right after the child spawns.
    pub fn set_root_pid(&self, employee_id: &str, run_id: &str, root_pid: libc::pid_t) {
        let mut map = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(binding) = map.get_mut(employee_id) {
            if binding.run_id == run_id {
                binding.mode = BindingMode::Managed { root_pid };
            }
        }
    }

    /// Revokes the active managed run binding for `employee_id` (if its `run_id` matches, or any
    /// if `None`). External leases are revoked only through [`Self::revoke_external`].
    pub fn revoke(&self, employee_id: &str, run_id: Option<&str>) {
        let mut map = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let matches = map
            .get(employee_id)
            .is_some_and(|b| run_id.is_none_or(|id| b.run_id == id));
        if matches {
            map.remove(employee_id);
        }
    }

    /// Verifies that `(employee_id, presented_token, peer_pid)` belongs to an active run and that
    /// `peer_pid` is the run's root process or a live descendant (covering sub-agents, `SUB-3`).
    pub fn verify_peer(
        &self,
        employee_id: &str,
        presented_token: Option<&str>,
        peer_pid: Option<libc::pid_t>,
    ) -> Result<ActiveRunBinding, BindingRejection> {
        self.verify_peer_with(employee_id, presented_token, peer_pid, |pid, ancestor| {
            org_platform::process::is_descendant_of(pid, ancestor).unwrap_or(false)
        })
    }

    /// Same as [`Self::verify_peer`] with an injectable descendant predicate for unit tests.
    pub fn verify_peer_with(
        &self,
        employee_id: &str,
        presented_token: Option<&str>,
        peer_pid: Option<libc::pid_t>,
        is_descendant: impl FnOnce(libc::pid_t, libc::pid_t) -> bool,
    ) -> Result<ActiveRunBinding, BindingRejection> {
        let Some(token) = presented_token.filter(|t| !t.is_empty()) else {
            return Err(BindingRejection::MissingToken);
        };
        let Some(peer_pid) = peer_pid.filter(|p| *p > 0) else {
            return Err(BindingRejection::MissingPeerPid);
        };
        let binding = {
            let map = self.inner.lock().unwrap_or_else(|p| p.into_inner());
            map.get(employee_id).cloned()
        };
        let Some(binding) = binding else {
            return Err(BindingRejection::UnknownToken);
        };
        if binding.employee_id != employee_id {
            return Err(BindingRejection::MemberMismatch);
        }
        if !constant_time_eq(binding.token.as_bytes(), token.as_bytes()) {
            return Err(BindingRejection::UnknownToken);
        }
        let root_pid = binding.root_pid().unwrap_or(0);
        if root_pid <= 0 || !is_descendant(peer_pid, root_pid) {
            return Err(BindingRejection::NotDescendant);
        }
        Ok(binding)
    }

    /// Records one accepted `agent-os` command for `employee_id`'s active managed run
    /// (`ADR-0030` d8), or else its external binding (bounded). Answers the mode it recorded
    /// under, `None` when `employee_id` holds no binding.
    pub fn record_accepted_command(
        &self,
        employee_id: &str,
        method: impl Into<String>,
        summary: impl Into<String>,
    ) -> Option<BindingMode> {
        let record = AcceptedCommandRecord {
            method: method.into(),
            summary: summary.into(),
        };
        {
            let mut map = self.inner.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(binding) = map.get_mut(employee_id) {
                binding.accepted_commands.push(record);
                return Some(binding.mode);
            }
        }
        self.record_external_command(employee_id, record)
    }

    /// Snapshot of accepted commands recorded on `employee_id`'s current run.
    pub fn accepted_commands(&self, employee_id: &str) -> Vec<AcceptedCommandRecord> {
        let map = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        map.get(employee_id)
            .map(|b| b.accepted_commands.clone())
            .unwrap_or_default()
    }

    /// Whether `employee_id` currently holds a live *managed* run binding (an external lease's
    /// token is [`Self::has_active_external`]).
    pub fn has_active(&self, employee_id: &str) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains_key(employee_id)
    }

    /// Collects all active session tokens, managed and external, so output/log redaction can
    /// scrub them (`ENROLL-3`).
    pub fn active_tokens(&self) -> Vec<String> {
        let mut out: Vec<String> = {
            let map = self.inner.lock().unwrap_or_else(|p| p.into_inner());
            map.values().map(|b| b.token.to_string()).collect()
        };
        let external = self.external.lock().unwrap_or_else(|p| p.into_inner());
        out.extend(external.values().map(|b| b.token.to_string()));
        out
    }
}
