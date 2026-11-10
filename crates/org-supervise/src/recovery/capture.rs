//! Capture identity/ancestry before any signal can cause a process to reparent.
use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::time::Instant;

use super::{
    check_deadline, safe_identity, validate_target_count, CheckpointTarget, MarkEvidence,
    ProcessIdentity, ProcessObservation, RecoveryContext, RecoveryProcesses, RecoveryReport,
    TargetEvidence, VerifiedTarget,
};

pub(super) fn snapshot_map(
    snapshot: Vec<ProcessObservation>,
) -> io::Result<BTreeMap<u32, ProcessObservation>> {
    let mut map = BTreeMap::new();
    for process in snapshot {
        if map.insert(process.pid, process).is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "duplicate PID in process snapshot",
            ));
        }
    }
    Ok(map)
}

// User enumeration can omit a known PID after its credentials change. Confirm the
// exact identity independently rather than treating an absent row as proof of exit.
pub(super) fn confirm_absent(
    identity: &ProcessIdentity,
    ops: &mut dyn RecoveryProcesses,
    deadline: Instant,
) -> io::Result<()> {
    check_deadline(ops, deadline)?;
    let now = ops.now();
    if !ops.wait_exited(identity, now)? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "known process still live outside the user snapshot",
        ));
    }
    Ok(())
}

/// Mark roots and any known earlier targets, plus an optional exact owned leader exclusion.
pub(super) struct CaptureScope<'a> {
    pub mark: &'a str,
    pub excluded: Option<&'a ProcessIdentity>,
    pub saved: &'a [CheckpointTarget],
}

pub(super) fn capture_targets(
    scope: CaptureScope<'_>,
    snapshot: &BTreeMap<u32, ProcessObservation>,
    context: &RecoveryContext,
    ops: &mut dyn RecoveryProcesses,
    deadline: Instant,
    report: &mut RecoveryReport,
) -> io::Result<Vec<VerifiedTarget>> {
    let mut eligible = BTreeMap::new();
    for (pid, p) in snapshot {
        if p.uid != context.uid {
            continue;
        }
        if p.zombie {
            report.zombies += 1;
            continue;
        }
        if p.identity
            .as_ref()
            .is_some_and(|id| Some(id) == scope.excluded)
        {
            report.excluded += 1;
            continue;
        }
        if p.mark == MarkEvidence::Unreadable {
            report.unreadable_environments += 1;
            continue;
        }
        if p.identity.is_none() {
            report.unreadable_identities += 1;
        }
        eligible.insert(*pid, p);
    }
    let mut targets = BTreeMap::new();
    let mut queue = VecDeque::new();
    for (pid, p) in &eligible {
        if matches!(&p.mark, MarkEvidence::Present(mark) if mark.as_str() == scope.mark) {
            let id = safe_identity(p, context).ok_or_else(|| {
                report.refused += 1;
                io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "unsafe marked process identity",
                )
            })?;
            validate_target_count(targets.len() + 1)?;
            targets.insert(
                *pid,
                VerifiedTarget {
                    identity: id.clone(),
                    mark: scope.mark.into(),
                    evidence: TargetEvidence::Marked,
                },
            );
            queue.push_back((*pid, vec![id.clone()]));
        }
    }
    // Restore targets observed before earlier parent kills, even after reparenting
    // or a process crash. Numeric PID reuse is never authority for the replacement.
    let mut pending_unreadable = 0;
    for saved in scope.saved {
        if saved.identity.boot != context.boot {
            continue;
        }
        let Some(current) = snapshot.get(&saved.identity.pid) else {
            confirm_absent(&saved.identity, ops, deadline)?;
            continue;
        };
        if current.zombie {
            continue;
        }
        if let Some(id) = &current.identity {
            if !id.valid() || id.pid != current.pid {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid observed identity for saved target",
                ));
            }
            if id != &saved.identity {
                continue;
            }
        }
        if scope.excluded == Some(&saved.identity) {
            continue;
        }
        if current.mark == MarkEvidence::Unreadable {
            // S0: skip without failing this pass. A previously known live target
            // still prevents the coordinator from declaring a final empty pass.
            pending_unreadable += 1;
            continue;
        }
        if current.identity.is_none() || safe_identity(current, context).is_none() {
            report.refused += 1;
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "saved target identity cannot be verified",
            ));
        }
        if targets.contains_key(&saved.identity.pid) {
            continue;
        }
        validate_target_count(targets.len() + 1)?;
        targets.insert(
            saved.identity.pid,
            VerifiedTarget {
                identity: saved.identity.clone(),
                mark: scope.mark.into(),
                evidence: TargetEvidence::Checkpoint(saved.ancestry.clone()),
            },
        );
        let mut chain = saved.ancestry.clone();
        chain.push(saved.identity.clone());
        queue.push_back((saved.identity.pid, chain));
    }
    // Snapshot descendants before killing parents. A visited set also bounds malformed cycles.
    while let Some((parent, ancestry)) = queue.pop_front() {
        check_deadline(ops, deadline)?;
        for (pid, child) in &eligible {
            if child.parent != parent || targets.contains_key(pid) {
                continue;
            }
            let id = safe_identity(child, context).ok_or_else(|| {
                report.refused += 1;
                io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "unsafe descendant identity",
                )
            })?;
            validate_target_count(targets.len() + 1)?;
            targets.insert(
                *pid,
                VerifiedTarget {
                    identity: id.clone(),
                    mark: scope.mark.into(),
                    evidence: TargetEvidence::Descendant(ancestry.clone()),
                },
            );
            let mut next = ancestry.clone();
            next.push(id.clone());
            queue.push_back((*pid, next));
        }
    }
    report.found_live = targets.len() + pending_unreadable;
    Ok(targets.into_values().collect())
}
