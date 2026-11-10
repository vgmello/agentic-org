//! Crash recovery policy over explicit process and record ports.
//!
//! This module does not implement native process discovery or open a database. Native
//! adapters must pin/revalidate identities as documented on [`RecoveryProcesses`]; a
//! successful fake run is not permission to use real runtimes. The caller serializes
//! recovery against spawning with the same mark and retains the session lock. It commits
//! the probe/session outcome before retiring the matching persisted owned-process row.
//! `RecoveryGone` proves completion of this policy, subject to S0's explicit limitation:
//! unreadable environments are reported and skipped, and an environment-clearing double
//! fork can escape discovery. No numeric process group/session ID authorizes a signal.
//!
//! One-pass scanning cannot be driven directly by an external caller:
//! ```compile_fail
//! use org_supervise::recovery::scan_one_pass;
//! ```

mod scan;
mod startup;

pub(crate) use scan::fixed_point;
pub use scan::{ScanFailure, ScanPass, ScanPort, ScanRequest};
pub use startup::{recover_startup, RecoveryBatch, RecoveryGone};

use std::io;
use std::time::Instant;

use crate::{invalid, ChildIdentity, SessionMark};

// Completion proofs must not be obtainable from caller-invented process observations.
// Only this crate's native adapters and test fakes may implement these ports.
mod sealed {
    pub trait Processes {}
    pub trait Scan {}
}

/// Immutable kernel identity, rather than a reusable numeric PID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessIdentity {
    /// PID greater than one and representable as a native signed PID.
    pub pid: u32,
    /// Opaque platform-native start identity.
    pub started: String,
    /// Opaque boot identity.
    pub boot: String,
}

impl ProcessIdentity {
    fn valid(&self) -> bool {
        valid_pid(self.pid) && !self.started.is_empty() && !self.boot.is_empty()
    }
}

fn valid_pid(pid: u32) -> bool {
    pid > 1 && pid <= i32::MAX as u32
}

/// Persisted input. A mark-only record covers the crash between spawn and identity write.
#[derive(Clone, Debug)]
pub struct RecoveryRecord {
    mark: String,
    leader: Option<ChildIdentity>,
    tool_sessions: Vec<u32>,
}

impl RecoveryRecord {
    /// Validate the durable mark, optional leader and non-authoritative session hints.
    pub fn new(
        mark: String,
        leader: Option<ChildIdentity>,
        tool_sessions: Vec<u32>,
    ) -> io::Result<Self> {
        SessionMark::new(mark.clone())?;
        if let Some(leader) = &leader {
            if !valid_pid(leader.pid)
                || leader.pgid != leader.pid
                || leader.started.is_empty()
                || leader.boot.is_empty()
                || leader.mark != mark
            {
                return Err(invalid("invalid recovery leader identity"));
            }
        }
        if tool_sessions.iter().any(|pid| !valid_pid(*pid)) {
            return Err(invalid("invalid tool session hint"));
        }
        Ok(Self {
            mark,
            leader,
            tool_sessions,
        })
    }

    /// Persisted, non-secret session mark.
    pub fn mark(&self) -> &str {
        &self.mark
    }
    /// Recorded runtime leader, when its identity write committed.
    pub fn leader(&self) -> Option<&ChildIdentity> {
        self.leader.as_ref()
    }
    /// In-memory hints, never sufficient authority to kill a process.
    pub fn tool_sessions(&self) -> &[u32] {
        &self.tool_sessions
    }
}

/// Consumer-owned read port. Recovery never deletes rows, releases locks or writes SQL.
pub trait RecoveryRecords {
    /// Caller key used to bind each result to its original row.
    type Key;
    /// Read pending records. Read failure aborts before any process operation.
    fn pending(&mut self) -> io::Result<Vec<(Self::Key, RecoveryRecord)>>;
}

/// Identity of the supervising user/process and current boot.
#[derive(Clone, Debug)]
pub struct RecoveryContext {
    /// Real user ID. Root recovery is refused.
    pub uid: u32,
    /// Supervisor PID, never a signal target.
    pub pid: u32,
    /// Current boot identity.
    pub boot: String,
}

/// Environment evidence. Never retain the rest of a process's environment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkEvidence {
    /// Environment read successfully; its mark is this non-secret value.
    Present(String),
    /// Environment read successfully; no mark was present.
    Absent,
    /// S0: log/count and skip, without guessing or failing a scan.
    Unreadable,
}

/// One process in a complete snapshot of the user's processes.
#[derive(Clone, Debug)]
pub struct ProcessObservation {
    /// Numeric PID, also available when the rest of its identity is unreadable.
    pub pid: u32,
    /// Full start/boot identity, or None if it could not be read.
    pub identity: Option<ProcessIdentity>,
    /// Parent PID at snapshot time.
    pub parent: u32,
    /// Process group; a hint only.
    pub group: u32,
    /// Session ID; a hint only.
    pub session: u32,
    /// Real user ID.
    pub uid: u32,
    /// True only if real/effective/saved credentials are the same non-root user,
    /// with no set-id transition. Native adapters recheck this before signalling.
    pub credentials_safe: bool,
    /// Zombies are always skipped before reading their environment.
    pub zombie: bool,
    /// Mark-only environment evidence. A zombie need not have readable evidence.
    pub mark: MarkEvidence,
}

/// Authority captured before the first kill in a scan.
#[derive(Clone, Debug)]
pub enum TargetEvidence {
    /// Exact persisted leader identity and matching mark.
    RecordedLeader,
    /// This process's readable environment contains the mark.
    Marked,
    /// Snapshot ancestry from a marked process to the target (excluding the target).
    /// Native adapters must preserve this evidence while pinning target identities.
    Descendant(Vec<ProcessIdentity>),
}

/// Policy-issued authority for one individual process. No raw PID/group constructor.
#[derive(Debug)]
pub struct VerifiedTarget {
    identity: ProcessIdentity,
    mark: String,
    evidence: TargetEvidence,
}

impl VerifiedTarget {
    /// Exact identity that must still match (or already be gone).
    pub fn identity(&self) -> &ProcessIdentity {
        &self.identity
    }
    /// Expected non-secret session mark.
    pub fn mark(&self) -> &str {
        &self.mark
    }
    /// Why this process is eligible, in addition to identity and credential checks.
    pub fn evidence(&self) -> &TargetEvidence {
        &self.evidence
    }
}

/// Individual recovery signal. There is deliberately no group signal operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoverySignal {
    /// Give the runtime leader the opportunity to stop its tools first.
    Terminate,
    /// Final kill after identity validation.
    Kill,
}

/// A checked signal either reached that identity or found it already gone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalOutcome {
    /// Signal submitted to the validated identity, not proof of exit.
    Signalled,
    /// That precise identity no longer exists; a replacement must not be signalled.
    Gone,
}

/// Volatile process boundary. All operations honor the absolute deadline, including
/// interrupted syscalls. No operation reaps a child or signals a numeric process group.
///
/// Linux adapters require pidfd support and pin snapshot identities before any signal;
/// ENOSYS/EPERM must refuse, never fall back to kill(pid). macOS adapters recheck start,
/// boot, credentials and mark/ancestry evidence immediately before an individual kill;
/// the residual non-child PID race must remain explicit. Inability to verify a known
/// target is an error. Identity reuse may be reported Gone, without hitting the new PID.
///
/// External implementations are refused, including an otherwise complete fake:
/// ```compile_fail,E0277
/// use org_supervise::recovery::*;
/// use std::{io, time::Instant};
/// struct Forged;
/// impl RecoveryProcesses for Forged {
///     fn context(&mut self) -> io::Result<RecoveryContext> { unimplemented!() }
///     fn snapshot(&mut self, _: Instant) -> io::Result<Vec<ProcessObservation>> { unimplemented!() }
///     fn signal_checked(&mut self, _: &VerifiedTarget, _: RecoverySignal, _: Instant)
///         -> io::Result<SignalOutcome> { unimplemented!() }
///     fn wait_exited(&mut self, _: &ProcessIdentity, _: Instant) -> io::Result<bool> { unimplemented!() }
///     fn now(&mut self) -> Instant { unimplemented!() }
///     fn wait_until(&mut self, _: Instant) -> io::Result<()> { unimplemented!() }
/// }
/// ```
pub trait RecoveryProcesses: sealed::Processes {
    /// Capture supervisor identity, refusing an unsupported native mechanism.
    fn context(&mut self) -> io::Result<RecoveryContext>;
    /// Complete user-process snapshot, preserving identity/ancestry evidence before kills.
    /// A truncated or failed enumeration is an error, never an empty successful scan.
    fn snapshot(&mut self, deadline: Instant) -> io::Result<Vec<ProcessObservation>>;
    /// Revalidate/pin the policy-issued target and signal it individually.
    fn signal_checked(
        &mut self,
        target: &VerifiedTarget,
        signal: RecoverySignal,
        deadline: Instant,
    ) -> io::Result<SignalOutcome>;
    /// Wait without reaping until this identity is gone or the deadline is reached.
    /// A reused PID means the old identity is gone, never authority to signal its replacement.
    fn wait_exited(&mut self, identity: &ProcessIdentity, deadline: Instant) -> io::Result<bool>;
    /// Monotonic time, explicitly supplied so policy tests need no sleeps.
    fn now(&mut self) -> Instant;
    /// Wait until the deadline (or return an error). Used only between scan passes.
    fn wait_until(&mut self, deadline: Instant) -> io::Result<()>;
}

/// Counts and warnings, with no process environment bodies.
#[derive(Default, Debug, PartialEq, Eq)]
pub struct RecoveryReport {
    /// Validated scan passes completed.
    pub passes: usize,
    /// Live eligible processes found across those passes.
    pub found_live: usize,
    /// S0 environment reads skipped (summed across passes; a process can repeat).
    pub unreadable_environments: usize,
    /// Processes whose identity could not be read.
    pub unreadable_identities: usize,
    /// Zombies skipped before considering environment evidence.
    pub zombies: usize,
    /// Exact excluded identities skipped (the owned unreaped probe leader).
    pub excluded: usize,
    /// Known matching targets refused for unsafe identity/credentials.
    pub refused: usize,
    /// Individual signal attempts, including already-gone identities.
    pub signals: usize,
}

impl RecoveryReport {
    fn append(&mut self, other: Self) {
        self.passes += other.passes;
        self.found_live += other.found_live;
        self.unreadable_environments += other.unreadable_environments;
        self.unreadable_identities += other.unreadable_identities;
        self.zombies += other.zombies;
        self.excluded += other.excluded;
        self.refused += other.refused;
        self.signals += other.signals;
    }
}

fn check_deadline(ops: &mut dyn RecoveryProcesses, deadline: Instant) -> io::Result<()> {
    if ops.now() >= deadline {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "recovery deadline elapsed",
        ))
    } else {
        Ok(())
    }
}

fn checked_context(ops: &mut dyn RecoveryProcesses) -> io::Result<RecoveryContext> {
    let context = ops.context()?;
    if context.uid == 0 || !valid_pid(context.pid) || context.boot.is_empty() {
        Err(invalid("invalid or root recovery context"))
    } else {
        Ok(context)
    }
}

fn safe_identity<'a>(
    p: &'a ProcessObservation,
    c: &RecoveryContext,
) -> Option<&'a ProcessIdentity> {
    p.identity.as_ref().filter(|id| {
        id.valid()
            && id.pid == p.pid
            && id.boot == c.boot
            && p.pid != c.pid
            && p.uid == c.uid
            && p.uid != 0
            && p.credentials_safe
    })
}

#[cfg(test)]
mod tests;
