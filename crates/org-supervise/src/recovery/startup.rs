use std::io;
use std::time::{Duration, Instant};

use super::capture::{capture_targets, confirm_absent, CaptureScope, ValidatedSnapshot};
use super::checkpoint::{Journal, TargetCheckpoint};
use super::fixed_point;
use super::RecoveryCheckpoint;
use super::{
    check_deadline, checked_context, ProcessIdentity, RecoveryContext, RecoveryProcesses,
    RecoveryRecord, RecoveryReport, RecoverySignal, ScanFailure, ScanPort, TargetEvidence,
    VerifiedTarget,
};

/// Sealed policy completion, bound to one persisted mark. The consumer must still
/// compare that mark with its row under the appropriate lock/transaction. Commit the
/// probe outcome before retiring the owned-process reference. S0 unreadable warnings
/// are available in the report; this is not proof that undiscoverable processes cannot exist.
///
/// ```compile_fail,E0451
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
    /// Completion counts and S0 warnings. No storage row has been retired.
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

fn checkpoint_capture(
    mark: &str,
    snapshot: &ValidatedSnapshot,
    context: &RecoveryContext,
    ops: &mut dyn RecoveryProcesses,
    deadline: Instant,
    report: &mut RecoveryReport,
    inventory: &mut dyn TargetCheckpoint,
) -> io::Result<Vec<VerifiedTarget>> {
    let mut observed_report = RecoveryReport::default();
    let targets = capture_targets(
        CaptureScope {
            mark,
            excluded: None,
            saved: inventory.saved(),
            leader: inventory.leader(),
        },
        snapshot,
        context,
        ops,
        deadline,
        &mut observed_report,
    );
    observed_report.found_live = 0; // pre-signal capture is not a validated scan pass
    report.append(observed_report);
    let targets = targets?;
    check_deadline(ops, deadline)?;
    inventory.save(&targets, deadline)?;
    Ok(targets)
}

fn stop_recorded(
    record: &RecoveryRecord,
    context: &RecoveryContext,
    ops: &mut dyn RecoveryProcesses,
    deadline: Instant,
    report: &mut RecoveryReport,
    inventory: &mut dyn TargetCheckpoint,
) -> io::Result<()> {
    let Some(expected) = leader_identity(record) else {
        return Ok(());
    };
    // Reboot invalidates numeric group/session hints. The full current mark scan still runs.
    if expected.boot != context.boot {
        return Ok(());
    }
    check_deadline(ops, deadline)?;
    let snapshot = ValidatedSnapshot::capture(ops, deadline)?;
    let Some(observed) = snapshot.get(expected.pid) else {
        return confirm_absent(&expected, ops, deadline);
    };
    if observed.zombie {
        return Ok(());
    }
    let Some(identity) = &observed.identity else {
        report.unreadable_identities += 1;
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "recorded leader identity is unreadable",
        ));
    };
    if identity != &expected {
        return Ok(());
    }
    // Preserve every target while the recorded leader can still prove its job.
    let targets = checkpoint_capture(
        record.mark(),
        &snapshot,
        context,
        ops,
        deadline,
        report,
        inventory,
    )?;
    let Some(target) = targets.iter().find(|target| target.identity == expected) else {
        report.refused += 1;
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "recorded leader cannot be verified",
        ));
    };
    let leader = VerifiedTarget {
        identity: expected.clone(),
        mark: record.mark().into(),
        evidence: match &target.evidence {
            TargetEvidence::Marked => TargetEvidence::RecordedLeader,
            other => other.clone(),
        },
    };
    check_deadline(ops, deadline)?;
    report.signals += 1;
    ops.signal_checked(&leader, RecoverySignal::Terminate, deadline)?;
    check_deadline(ops, deadline)?;
    let grace = (ops.now() + Duration::from_secs(5)).min(deadline);
    let leader_gone = ops.wait_exited(&expected, grace)?;
    check_deadline(ops, deadline)?;
    if !leader_gone {
        // The leader can start new tools during grace. Refresh the entire checkpoint
        // before KILL, then signal only the freshly verified leader identity. The full
        // scan handles other saved survivors after the leader exits.
        let snapshot = ValidatedSnapshot::capture(ops, deadline)?;
        let targets = checkpoint_capture(
            record.mark(),
            &snapshot,
            context,
            ops,
            deadline,
            report,
            inventory,
        )?;
        if let Some(leader) = targets.iter().find(|target| target.identity == expected) {
            check_deadline(ops, deadline)?;
            report.signals += 1;
            ops.signal_checked(leader, RecoverySignal::Kill, deadline)?;
            check_deadline(ops, deadline)?;
            let until = (ops.now() + Duration::from_secs(1)).min(deadline);
            if !ops.wait_exited(&expected, until)? {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "recorded leader still live after kill",
                ));
            }
        }
    }
    check_deadline(ops, deadline)
}

/// Recover pending records with durable target checkpoints, without retiring rows. The caller excludes concurrent
/// reuse of their marks and retains locks until its own outcome/row update commits.
/// A matching leader gets TERM first and up to five seconds; captured survivors are
/// checked/killed individually. The full scan always runs, even after leader failure.
/// Only a final validated empty pass can produce a mark-bound [`RecoveryGone`].
pub fn recover_startup<R: RecoveryCheckpoint>(
    records: &mut R,
    ops: &mut dyn RecoveryProcesses,
    deadline: Instant,
) -> io::Result<RecoveryBatch<R::Key>> {
    recover_with_scan(records, ops, &mut super::scan::MarkedScan, deadline)
}

/// Recover one existing row from a live cleanup worker after its handle is gone.
/// Reload the durable inventory every call; do not sweep other pending records.
/// The caller holds the record's cleanup lock through recovery and outcome commit,
/// and must bind the returned mark before retiring the row. The same checked process
/// policy and final-empty proof used at startup apply here.
pub fn recover_record<R: RecoveryCheckpoint>(
    records: &mut R,
    key: &R::Key,
    ops: &mut dyn RecoveryProcesses,
    deadline: Instant,
) -> Result<RecoveryGone, ScanFailure> {
    let before_scan = |error| ScanFailure {
        error,
        report: RecoveryReport::default(),
    };
    check_deadline(ops, deadline).map_err(before_scan)?;
    let record = records
        .load(key, deadline)
        .map_err(before_scan)?
        .ok_or_else(|| {
            before_scan(io::Error::new(
                io::ErrorKind::NotFound,
                "recovery record missing",
            ))
        })?;
    let context = checked_context(ops).map_err(before_scan)?;
    recover_loaded(
        records,
        key,
        record,
        &context,
        ops,
        &mut super::scan::MarkedScan,
        deadline,
    )
}

pub(super) fn recover_with_scan<R: RecoveryCheckpoint>(
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
        match recover_loaded(records, &key, record, &context, ops, scan, deadline) {
            Ok(gone) => batch.gone.push((key, gone)),
            Err(failure) => batch.unresolved.push((key, failure)),
        }
    }
    Ok(batch)
}

fn recover_loaded<R: RecoveryCheckpoint>(
    records: &mut R,
    key: &R::Key,
    record: RecoveryRecord,
    context: &RecoveryContext,
    ops: &mut dyn RecoveryProcesses,
    scan: &mut dyn ScanPort,
    deadline: Instant,
) -> Result<RecoveryGone, ScanFailure> {
    let mut report = RecoveryReport::default();
    let mut inventory = Journal::new(records, key, &record);
    let leader = stop_recorded(&record, context, ops, deadline, &mut report, &mut inventory);
    if leader.is_err() {
        inventory.refuse_signals();
    }
    let scan_result = fixed_point(
        record.mark(),
        None,
        record.tool_sessions(),
        ops,
        scan,
        deadline,
        &mut inventory,
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
        Err(ScanFailure { error, report })
    } else {
        Ok(RecoveryGone {
            mark: record.mark,
            report,
        })
    }
}
