//! Capture identity/ancestry before any signal can cause a process to reparent.
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io;
use std::time::Instant;

use super::{
    check_deadline, safe_identity, validate_target_count, CheckpointTarget, MarkEvidence,
    ProcessIdentity, ProcessObservation, RecoveryContext, RecoveryProcesses, RecoveryReport,
    TargetEvidence, VerifiedTarget,
};

/// The only snapshot type consumed by recovery policy. Leader eligibility and
/// traversal can never inspect an unchecked capture, even on early-return paths.
pub(super) struct ValidatedSnapshot {
    processes: BTreeMap<u32, ProcessObservation>,
    parents: BTreeMap<u32, u32>,
}

impl ValidatedSnapshot {
    pub(super) fn capture(ops: &mut dyn RecoveryProcesses, deadline: Instant) -> io::Result<Self> {
        check_deadline(ops, deadline)?;
        let mut processes = BTreeMap::new();
        for process in ops.snapshot(deadline)? {
            if processes.insert(process.pid, process).is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "duplicate PID in process snapshot",
                ));
            }
        }
        let parents = validated_parent_links(&processes, ops, deadline)?;
        check_deadline(ops, deadline)?;
        Ok(Self { processes, parents })
    }

    pub(super) fn get(&self, pid: u32) -> Option<&ProcessObservation> {
        self.processes.get(&pid)
    }
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

// Validate topology against the entire snapshot. Filtering for signal eligibility
// first could hide evidence of a reused PID (for example, a zombie replacement).
fn validated_parent_links(
    snapshot: &BTreeMap<u32, ProcessObservation>,
    ops: &mut dyn RecoveryProcesses,
    deadline: Instant,
) -> io::Result<BTreeMap<u32, u32>> {
    let mut parents = BTreeMap::new();
    for child in snapshot.values() {
        check_deadline(ops, deadline)?;
        let Some(parent) = &child.parent else {
            continue;
        };
        if !parent.valid() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid captured parent identity",
            ));
        }
        let Some(observed) = snapshot.get(&parent.pid) else {
            continue;
        };
        let Some(identity) = &observed.identity else {
            continue;
        };
        if identity != parent {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "captured parent identity changed",
            ));
        }
        parents.insert(child.pid, parent.pid);
    }
    let mut finished = BTreeSet::new();
    for mut pid in parents.keys().copied() {
        check_deadline(ops, deadline)?;
        let mut path = BTreeSet::new();
        while let Some(parent) = parents.get(&pid) {
            if finished.contains(&pid) {
                break;
            }
            if !path.insert(pid) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "cyclic captured process ancestry",
                ));
            }
            pid = *parent;
        }
        finished.extend(path);
    }
    Ok(parents)
}

/// Mark roots and any known earlier targets, plus an optional exact owned leader exclusion.
pub(super) struct CaptureScope<'a> {
    pub mark: &'a str,
    pub excluded: Option<&'a ProcessIdentity>,
    pub saved: &'a [CheckpointTarget],
}

pub(super) fn capture_targets(
    scope: CaptureScope<'_>,
    snapshot: &ValidatedSnapshot,
    context: &RecoveryContext,
    ops: &mut dyn RecoveryProcesses,
    deadline: Instant,
    report: &mut RecoveryReport,
) -> io::Result<Vec<VerifiedTarget>> {
    let mut eligible = BTreeMap::new();
    for (pid, p) in &snapshot.processes {
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
        }
        if p.identity.is_none() {
            report.unreadable_identities += 1;
        }
        eligible.insert(*pid, p);
    }
    let mut targets = BTreeMap::new();
    // The traversal graph also includes verified identity-only links. They never
    // become signal targets merely because their environments are unreadable.
    let mut ancestry_nodes = BTreeMap::new();
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
            ancestry_nodes.insert(*pid, (None, id));
            queue.push_back((*pid, false));
        }
    }
    // Restore targets observed before earlier parent kills, even after reparenting
    // or a process crash. Numeric PID reuse is never authority for the replacement.
    let mut pending_unreadable = 0;
    for saved in scope.saved {
        if saved.identity.boot != context.boot {
            continue;
        }
        let Some(current) = snapshot.get(saved.identity.pid) else {
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
            if let Some(id) = safe_identity(current, context) {
                ancestry_nodes.insert(saved.identity.pid, (None, id));
                queue.push_back((saved.identity.pid, true));
            }
            continue;
        }
        let id = safe_identity(current, context).ok_or_else(|| {
            report.refused += 1;
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "saved target identity cannot be verified",
            )
        })?;
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
        // The saved ancestry explains this anchor's authority, not its current parent
        // links. Descendants discovered now start a new chain at the live anchor.
        ancestry_nodes.insert(saved.identity.pid, (None, id));
        queue.push_back((saved.identity.pid, true));
    }
    let mut children: BTreeMap<u32, Vec<&ProcessObservation>> = BTreeMap::new();
    for (&child, &parent) in &snapshot.parents {
        let (Some(child), Some(observed)) = (eligible.get(&child), eligible.get(&parent)) else {
            continue;
        };
        if safe_identity(observed, context).is_some() {
            children.entry(parent).or_default().push(child);
        }
    }
    // Mark each observed node once. Reconstruct a
    // bounded ancestry chain only for a readable target, not for each S0 link.
    while let Some((parent, from_checkpoint)) = queue.pop_front() {
        check_deadline(ops, deadline)?;
        for child in children.get(&parent).into_iter().flatten() {
            let pid = child.pid;
            if ancestry_nodes.contains_key(&pid) {
                continue;
            }
            let Some(id) = safe_identity(child, context) else {
                if child.mark == MarkEvidence::Unreadable {
                    continue;
                }
                report.refused += 1;
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "unsafe descendant identity",
                ));
            };
            ancestry_nodes.insert(pid, (Some(parent), id));
            queue.push_back((pid, from_checkpoint));
            if child.mark == MarkEvidence::Unreadable {
                continue;
            }
            validate_target_count(targets.len() + 1)?;
            let mut ancestry = Vec::new();
            let mut cursor = Some(parent);
            while let Some(ancestor) = cursor {
                validate_target_count(ancestry.len() + 2)?; // ancestor plus the target
                let (previous, identity) = ancestry_nodes[&ancestor];
                ancestry.push(identity.clone());
                cursor = previous;
            }
            ancestry.reverse();
            targets.insert(
                pid,
                VerifiedTarget {
                    identity: id.clone(),
                    mark: scope.mark.into(),
                    evidence: if from_checkpoint {
                        TargetEvidence::CheckpointDescendant(ancestry)
                    } else {
                        TargetEvidence::Descendant(ancestry)
                    },
                },
            );
        }
    }
    report.found_live = targets.len() + pending_unreadable;
    Ok(targets.into_values().collect())
}
