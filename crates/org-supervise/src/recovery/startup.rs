use std::collections::BTreeMap;
use std::io;
use std::time::{Duration, Instant};

use super::fixed_point;
use super::{
    check_deadline, checked_context, safe_identity, MarkEvidence, ProcessIdentity,
    ProcessObservation, RecoveryContext, RecoveryProcesses, RecoveryRecord, RecoveryRecords,
    RecoveryReport, RecoverySignal, ScanFailure, ScanPort, TargetEvidence, VerifiedTarget,
};

/// Sealed policy completion, bound to one persisted mark. The consumer must still
/// compare that mark with its row under the appropriate lock/transaction. Commit the
/// probe outcome before retiring the owned-process reference. S0 unreadable warnings
/// are available in the report; this is not proof that undiscoverable processes cannot exist.
///
/// ```compile_fail
/// use org_supervise::recovery::{RecoveryGone, RecoveryReport};
/// let gone = RecoveryGone { mark: "forged".into(), report: RecoveryReport::default() };
/// ```
#[derive(Debug)]
pub struct RecoveryGone {
    mark: String,
    report: RecoveryReport,
}

impl RecoveryGone {
    /// Exact mark to compare with the consumer's owned-process row.
    pub fn mark(&self) -> &str {
        &self.mark
    }
    /// Completion counts and S0 warnings. No storage row has been changed.
    pub fn report(&self) -> &RecoveryReport {
        &self.report
    }
}

/// Per-record outcomes. A failure on one record does not hide other records' outcomes.
#[derive(Debug)]
pub struct RecoveryBatch<K> {
    /// Matching leader gone and a validated empty final pass completed.
    pub gone: Vec<(K, RecoveryGone)>,
    /// Incomplete records, retained for retry; these carry no completion capability.
    pub unresolved: Vec<(K, ScanFailure)>,
}

fn leader_identity(record: &RecoveryRecord) -> Option<ProcessIdentity> {
    record.leader().map(|id| ProcessIdentity {
        pid: id.pid,
        started: id.started.clone(),
        boot: id.boot.clone(),
    })
}

// Capture the recorded leader's descendants before TERM can orphan them. Other
// marked roots are still discovered by the mandatory post-kill full scan.
fn leader_targets(
    snapshot: Vec<ProcessObservation>,
    expected: &ProcessIdentity,
    record: &RecoveryRecord,
    context: &RecoveryContext,
    report: &mut RecoveryReport,
) -> io::Result<Vec<VerifiedTarget>> {
    let mut processes = BTreeMap::new();
    for process in snapshot {
        if processes.insert(process.pid, process).is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "duplicate PID in process snapshot",
            ));
        }
    }
    let Some(leader) = processes.get(&expected.pid) else {
        return Ok(vec![]);
    };
    if !leader.zombie && leader.identity.is_none() {
        report.unreadable_identities += 1;
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "recorded leader identity is unreadable",
        ));
    }
    if leader.identity.as_ref() != Some(expected) {
        return Ok(vec![]);
    }
    if leader.zombie {
        report.zombies += 1;
        return Ok(vec![]);
    }
    if safe_identity(leader, context).is_none()
        || leader.mark != MarkEvidence::Present(record.mark().into())
    {
        report.refused += 1;
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "recorded leader cannot be verified",
        ));
    }
    let mut targets = vec![VerifiedTarget {
        identity: expected.clone(),
        mark: record.mark().into(),
        evidence: TargetEvidence::RecordedLeader,
    }];
    let mut index = 0;
    while index < targets.len() {
        let parent = &targets[index];
        let mut ancestry = match &parent.evidence {
            TargetEvidence::Descendant(chain) => chain.clone(),
            _ => vec![],
        };
        ancestry.push(parent.identity.clone());
        let parent_pid = parent.identity.pid;
        for child in processes.values() {
            if child.parent != parent_pid
                || child.uid != context.uid
                || child.zombie
                || targets.iter().any(|t| t.identity.pid == child.pid)
            {
                continue;
            }
            if child.mark == MarkEvidence::Unreadable {
                report.unreadable_environments += 1;
                continue;
            }
            let identity = safe_identity(child, context).ok_or_else(|| {
                report.refused += 1;
                io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "recorded descendant cannot be verified",
                )
            })?;
            targets.push(VerifiedTarget {
                identity: identity.clone(),
                mark: record.mark().into(),
                evidence: TargetEvidence::Descendant(ancestry.clone()),
            });
        }
        index += 1;
    }
    Ok(targets)
}

fn stop_recorded(
    record: &RecoveryRecord,
    context: &RecoveryContext,
    ops: &mut dyn RecoveryProcesses,
    deadline: Instant,
    report: &mut RecoveryReport,
) -> io::Result<()> {
    let Some(expected) = leader_identity(record) else {
        return Ok(());
    };
    // Reboot invalidates numeric group/session hints. The full current mark scan still runs.
    if expected.boot != context.boot {
        return Ok(());
    }
    check_deadline(ops, deadline)?;
    let targets = leader_targets(ops.snapshot(deadline)?, &expected, record, context, report)?;
    let Some(leader) = targets.first() else {
        return Ok(());
    };
    check_deadline(ops, deadline)?;
    report.signals += 1;
    ops.signal_checked(leader, RecoverySignal::Terminate, deadline)?;
    check_deadline(ops, deadline)?;
    let grace = (ops.now() + Duration::from_secs(5)).min(deadline);
    let leader_gone = ops.wait_exited(&expected, grace)?;
    check_deadline(ops, deadline)?;
    // No bare killpg for a non-child: every surviving captured member is checked individually.
    for (index, target) in targets.iter().enumerate() {
        if index == 0 && leader_gone {
            continue;
        }
        check_deadline(ops, deadline)?;
        report.signals += 1;
        ops.signal_checked(target, RecoverySignal::Kill, deadline)?;
        check_deadline(ops, deadline)?;
        let until = (ops.now() + Duration::from_secs(1)).min(deadline);
        if !ops.wait_exited(&target.identity, until)? {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "recorded process still live after kill",
            ));
        }
    }
    check_deadline(ops, deadline)
}

/// Recover pending records without mutating storage. The caller excludes concurrent
/// reuse of their marks and retains locks until its own outcome/row update commits.
/// A matching leader gets TERM first and up to five seconds; captured survivors are
/// checked/killed individually. The full scan always runs, even after leader failure.
/// Only a final validated empty pass can produce a mark-bound [`RecoveryGone`].
pub fn recover_startup<R: RecoveryRecords>(
    records: &mut R,
    ops: &mut dyn RecoveryProcesses,
    deadline: Instant,
) -> io::Result<RecoveryBatch<R::Key>> {
    recover_with_scan(records, ops, &mut super::scan::MarkedScan, deadline)
}

pub(super) fn recover_with_scan<R: RecoveryRecords>(
    records: &mut R,
    ops: &mut dyn RecoveryProcesses,
    scan: &mut dyn ScanPort,
    deadline: Instant,
) -> io::Result<RecoveryBatch<R::Key>> {
    let pending = records.pending()?;
    let context = checked_context(ops)?;
    let mut batch = RecoveryBatch {
        gone: vec![],
        unresolved: vec![],
    };
    for (key, record) in pending {
        let mut report = RecoveryReport::default();
        let leader = stop_recorded(&record, &context, ops, deadline, &mut report);
        let scan_result = fixed_point(
            record.mark(),
            None,
            record.tool_sessions(),
            ops,
            scan,
            deadline,
        );
        let failure = match scan_result {
            Ok(scan_report) => {
                report.append(scan_report);
                leader.err()
            }
            Err(failure) => {
                report.append(failure.report);
                leader.err().or(Some(failure.error))
            }
        };
        if let Some(error) = failure {
            batch.unresolved.push((key, ScanFailure { error, report }));
        } else {
            batch.gone.push((
                key,
                RecoveryGone {
                    mark: record.mark,
                    report,
                },
            ));
        }
    }
    Ok(batch)
}
