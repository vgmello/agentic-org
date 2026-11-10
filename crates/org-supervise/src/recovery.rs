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

mod capture;
#[cfg(target_os = "linux")]
#[path = "recovery/native/linux.rs"]
mod native;
#[cfg(target_os = "linux")]
pub use native::NativeRecovery;
mod checkpoint;
mod scan;
mod startup;

pub(crate) use scan::fixed_point;
pub use scan::{ScanFailure, ScanPass, ScanPort, ScanRequest};
pub use startup::{recover_record, recover_startup, RecoveryBatch, RecoveryGone};

use std::collections::BTreeSet;
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

    fn key(&self) -> (u32, &str, &str) {
        (self.pid, &self.started, &self.boot)
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
    // Written before any signal and retained until the consumer retires this row.
    inventory: Vec<CheckpointTarget>,
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
            inventory: vec![],
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

    /// Restore the cumulative, plain-data write-ahead inventory from trusted storage.
    /// Never omit previously saved entries while their owning row still exists.
    pub fn with_inventory(mut self, inventory: Vec<CheckpointTarget>) -> io::Result<Self> {
        validate_inventory(&inventory)?;
        self.inventory = inventory;
        Ok(self)
    }

    /// Cumulative inventory; resolved entries remain until the consumer retires the row.
    pub fn inventory(&self) -> &[CheckpointTarget] {
        &self.inventory
    }
}

/// Maximum distinct process identities in a record, including ancestry-only links.
pub const MAX_RECOVERY_TARGETS: usize = 256;

/// Plain persisted data, never a process-signalling capability. Restored only through
/// the consumer's trusted record store; policy rechecks each exact identity before use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckpointTarget {
    /// PID, start and boot identity captured before the first signal.
    pub identity: ProcessIdentity,
    /// Captured ancestry, excluding this target; empty for a directly marked root/leader.
    pub ancestry: Vec<ProcessIdentity>,
}

fn validate_target_count(count: usize) -> io::Result<()> {
    if count > MAX_RECOVERY_TARGETS {
        return Err(invalid("recovery inventory exceeds 256 identities"));
    }
    Ok(())
}

fn validate_inventory(inventory: &[CheckpointTarget]) -> io::Result<()> {
    validate_target_count(inventory.len())?;
    let mut identities = BTreeSet::new();
    for target in inventory {
        if !target.identity.valid()
            || target.ancestry.len() >= MAX_RECOVERY_TARGETS
            || !identities.insert(target.identity.key())
        {
            return Err(invalid("invalid or duplicate saved process identity"));
        }
    }
    for target in inventory {
        let mut seen = BTreeSet::new();
        for ancestor in &target.ancestry {
            if !ancestor.valid()
                || ancestor.boot != target.identity.boot
                || ancestor.pid == target.identity.pid
                || !seen.insert(ancestor.pid)
            {
                return Err(invalid("invalid saved process ancestry"));
            }
            identities.insert(ancestor.key());
            validate_target_count(identities.len())?;
        }
    }
    Ok(())
}

/// Consumer-owned read port. Recovery never deletes rows, releases locks or writes SQL.
pub trait RecoveryRecords {
    /// Caller key used to bind each result to its original row.
    type Key;
    /// Read pending records. Read failure aborts before any process operation.
    fn pending(&mut self) -> io::Result<Vec<(Self::Key, RecoveryRecord)>>;
    /// Read one current record and its full saved inventory under the caller's
    /// cleanup lock. None means the record is missing, never that its processes exited.
    fn load(&mut self, key: &Self::Key, deadline: Instant) -> io::Result<Option<RecoveryRecord>>;
}

/// Consumer-owned durable write-ahead inventory port. Slice 4a implements only a fake;
/// B1-12 supplies the office.db adapter. It carries plain data, never a capability.
///
/// An external store can implement this without either sealed process port:
/// ```no_run
/// use org_supervise::recovery::{CheckpointTarget, RecoveryCheckpoint, RecoveryRecord, RecoveryRecords};
/// use std::{io, time::Instant};
/// struct OfficeStore;
/// impl RecoveryRecords for OfficeStore {
///     type Key = u64;
///     fn pending(&mut self) -> io::Result<Vec<(u64, RecoveryRecord)>> { unimplemented!() }
///     fn load(&mut self, _: &u64, _: Instant) -> io::Result<Option<RecoveryRecord>> { unimplemented!() }
/// }
/// impl RecoveryCheckpoint for OfficeStore {
///     fn checkpoint(&mut self, _: &u64, _: &str, _: &[CheckpointTarget], _: Instant)
///         -> io::Result<()> { unimplemented!() }
/// }
/// ```
pub trait RecoveryCheckpoint: RecoveryRecords {
    /// Atomically and durably union this cumulative inventory into the existing
    /// matching key/mark, before returning success. Retain all earlier entries until
    /// the row is retired with Gone. Refuse a stale key/mark or an inventory that
    /// omits any existing saved entry, overflow or deadline.
    /// Never create/replace/release a row, release a lock, or change its generation.
    /// Pending reads must restore the union after a crash. An ambiguous write error
    /// permits no signal; retry by reloading the durable record.
    fn checkpoint(
        &mut self,
        key: &Self::Key,
        mark: &str,
        inventory: &[CheckpointTarget],
        deadline: Instant,
    ) -> io::Result<()>;
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
    /// Parent's exact identity from the same capture, or None if the link could not
    /// be verified. A numeric PPID alone is not ancestry evidence.
    pub parent: Option<ProcessIdentity>,
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
    /// One snapshot's ancestry from a verified, previously saved live anchor.
    /// The anchor's own historical parents are not links in this snapshot chain.
    CheckpointDescendant(Vec<ProcessIdentity>),
    /// Historical identity/ancestry from the trusted write-ahead inventory. An exact
    /// live start/boot match remains a target after its parents exit; native adapters
    /// still recheck/pin that process and its credentials before signalling.
    Checkpoint(Vec<ProcessIdentity>),
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
/// ENOSYS/EPERM must refuse, never fall back to kill(pid). macOS adapters recheck target
/// start, boot and credentials immediately before an individual kill, and the mark for
/// directly marked targets. Preserve descendant ancestry from its captured snapshot;
/// a captured parent may already have exited. Checkpoint ancestry is historical, not a
/// current parent chain. The residual non-child PID race must remain explicit. Inability to verify a known
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
    /// Parent links must carry the exact parent identity from the same capture; a
    /// numeric PPID from a stale observation cannot establish ancestry. A truncated or
    /// failed enumeration is an error, never an empty successful scan.
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
    /// A deadline of now requests one nonblocking identity check. Absence from the user
    /// snapshot or inability to read a known PID is not evidence it exited.
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

#[cfg(target_os = "linux")]
fn native_mark(bytes: &[u8]) -> MarkEvidence {
    if !bytes.is_empty() && bytes.last() != Some(&0) {
        return MarkEvidence::Unreadable;
    }
    let mut marks = bytes
        .split(|byte| *byte == 0)
        .filter_map(|entry| entry.strip_prefix(b"ORG_SESSION_MARK="));
    let Some(value) = marks.next() else {
        return MarkEvidence::Absent;
    };
    if marks.next().is_some() {
        return MarkEvidence::Unreadable;
    }
    let Ok(value) = std::str::from_utf8(value) else {
        return MarkEvidence::Unreadable;
    };
    if SessionMark::new(value.into()).is_err() {
        return MarkEvidence::Unreadable;
    }
    MarkEvidence::Present(value.into())
}
