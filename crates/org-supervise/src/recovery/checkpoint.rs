//! Private journal adapter for the consumer's write-ahead contract.
//! Only fakes implement storage in this slice.

use std::io;
use std::time::Instant;

use super::{
    validate_inventory, CheckpointTarget, RecoveryCheckpoint, RecoveryRecord, TargetEvidence,
    VerifiedTarget,
};
use crate::ChildIdentity;

pub trait TargetCheckpoint {
    fn mark(&self) -> &str;
    fn saved(&self) -> &[CheckpointTarget];
    fn leader(&self) -> Option<&ChildIdentity>;
    fn save(&mut self, targets: &[VerifiedTarget], deadline: Instant) -> io::Result<()>;
}

pub(super) struct Journal<'a, R: RecoveryCheckpoint> {
    store: &'a mut R,
    key: &'a R::Key,
    mark: String,
    inventory: Vec<CheckpointTarget>,
    leader: Option<ChildIdentity>,
    blocked: bool,
}

impl<'a, R: RecoveryCheckpoint> Journal<'a, R> {
    pub(super) fn new(store: &'a mut R, key: &'a R::Key, record: &RecoveryRecord) -> Self {
        Self {
            store,
            key,
            mark: record.mark.clone(),
            inventory: record.inventory.clone(),
            leader: record.leader.clone(),
            blocked: false,
        }
    }
    pub(super) fn refuse_signals(&mut self) {
        self.blocked = true;
    }
}

impl<R: RecoveryCheckpoint> TargetCheckpoint for Journal<'_, R> {
    fn mark(&self) -> &str {
        &self.mark
    }
    fn saved(&self) -> &[CheckpointTarget] {
        &self.inventory
    }
    fn leader(&self) -> Option<&ChildIdentity> {
        self.leader.as_ref()
    }
    fn save(&mut self, targets: &[VerifiedTarget], deadline: Instant) -> io::Result<()> {
        if self.blocked {
            return Err(io::Error::other(
                "recorded cleanup failed; scan cannot authorize signals",
            ));
        }
        let mut inventory = self.inventory.clone();
        for target in targets {
            if target.mark != self.mark {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "checkpoint mark mismatch",
                ));
            }
            if !inventory.iter().any(|old| old.identity == target.identity) {
                let ancestry = match &target.evidence {
                    TargetEvidence::Descendant(chain)
                    | TargetEvidence::CheckpointDescendant(chain)
                    | TargetEvidence::Checkpoint(chain) => chain.clone(),
                    _ => vec![],
                };
                inventory.push(CheckpointTarget {
                    identity: target.identity.clone(),
                    ancestry,
                });
            }
        }
        validate_inventory(&inventory)?;
        // Publish in memory only after the consumer-owned durable write commits.
        self.store
            .checkpoint(self.key, &self.mark, &inventory, deadline)?;
        self.inventory = inventory;
        Ok(())
    }
}
