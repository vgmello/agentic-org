//! Native observations needed to prove membership in a recorded spawn's job.
use std::io;
use std::time::Instant;

use super::{ProcessIdentity, RecoveryContext};
use crate::ChildIdentity;

pub(super) struct JobObservation {
    pub identity: ProcessIdentity,
    pub order: (u64, u64),
    pub group: u32,
    pub session: u32,
    pub safe: bool,
}

/// Read target membership before re-proving its exact live leader. Native start
/// ordering stays here as an observation, never decoded by the policy layer.
pub(super) fn verify(
    target: &ProcessIdentity,
    leader: &ChildIdentity,
    context: &RecoveryContext,
    deadline: Instant,
    mut now: impl FnMut() -> Instant,
    mut read: impl FnMut(&ProcessIdentity) -> io::Result<Option<JobObservation>>,
) -> io::Result<bool> {
    let proof = (|| -> io::Result<bool> {
        let expected_leader = ProcessIdentity {
            pid: leader.pid,
            started: leader.started.clone(),
            boot: leader.boot.clone(),
        };
        let valid = |id: &ProcessIdentity| {
            context.uid != 0 && id.valid() && id.pid != context.pid && id.boot == context.boot
        };
        if leader.pgid != leader.pid || !valid(target) || !valid(&expected_leader) {
            return Ok(false);
        }
        let mut budget = || {
            if now() >= deadline {
                Err(io::Error::from(io::ErrorKind::TimedOut))
            } else {
                Ok(())
            }
        };
        budget()?;
        let Some(observed) = read(target)? else {
            return Ok(false);
        };
        budget()?;
        if observed.identity != *target
            || !observed.safe
            || (observed.group != leader.pid && observed.session != leader.pid)
        {
            return Ok(false);
        }
        let Some(anchor) = read(&expected_leader)? else {
            return Ok(false);
        };
        budget()?;
        Ok(anchor.identity == expected_leader
            && anchor.safe
            && anchor.group == leader.pid
            && observed.order >= anchor.order)
    })();
    // Optional job evidence must fail closed as S0, including I/O and deadlines.
    Ok(proof.unwrap_or(false))
}

#[cfg(test)]
mod tests;
