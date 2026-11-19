//! B1-12: org-supervise's crash-recovery record ports over the office's owned-process rows.
//! Every record is built through `RecoveryRecord::new` and `with_inventory`, so org-supervise's
//! rules and limits (the 256 identities included) apply to what is read and what is saved.

use std::io;
use std::time::Instant;

use org_office::{
    Office, OfficeError, OwnedProcess, OwnedProcessKey, SavedIdentity, SavedLeader, SavedTarget,
};
use org_supervise::recovery::{
    CheckpointTarget, ProcessIdentity, RecoveryCheckpoint, RecoveryRecord, RecoveryRecords,
};
use org_supervise::{ChildIdentity, SessionMark};

/// The office's rows as recovery's records. The caller holds the cleanup lock and retires a
/// row (`Office::retire_owned_process`) only after recovery proved its processes gone.
pub struct OfficeRecoveryRecords<'a>(pub &'a Office);

/// An office error as recovery sees it: a refusal is `InvalidInput`; a storage error keeps the
/// kind of the I/O error inside it (a missed deadline is `TimedOut`). Any error may follow a
/// commit, so recovery signals nothing and reloads.
fn io_error(error: OfficeError) -> io::Error {
    let kind = match &error {
        OfficeError::Invalid(_) | OfficeError::AlreadyExists => io::ErrorKind::InvalidInput,
        OfficeError::Storage(cause) => cause
            .downcast_ref::<io::Error>()
            .map_or(io::ErrorKind::Other, io::Error::kind),
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, error)
}

fn identity(saved: &SavedIdentity) -> ProcessIdentity {
    ProcessIdentity {
        pid: saved.pid,
        started: saved.started.clone(),
        boot: saved.boot.clone(),
    }
}

fn saved(identity: &ProcessIdentity) -> SavedIdentity {
    SavedIdentity {
        pid: identity.pid,
        started: identity.started.clone(),
        boot: identity.boot.clone(),
    }
}

fn record(row: OwnedProcess) -> io::Result<RecoveryRecord> {
    let leader = row.leader.map(
        |SavedLeader {
             pid,
             pgid,
             started,
             boot,
         }| ChildIdentity {
            pid,
            pgid,
            started,
            boot,
            mark: row.mark.clone(),
        },
    );
    let inventory = row
        .inventory
        .iter()
        .map(|t| CheckpointTarget {
            identity: identity(&t.identity),
            ancestry: t.ancestry.iter().map(identity).collect(),
        })
        .collect();
    RecoveryRecord::new(row.mark, leader, row.tool_sessions)?.with_inventory(inventory)
}

/// Records `mark`'s row before the spawn that carries it, only if org-supervise accepts the mark.
pub fn record_mark(office: &Office, mark: &str) -> io::Result<OwnedProcessKey> {
    SessionMark::new(mark.to_owned())?;
    office.record_owned_mark(mark).map_err(io_error)
}

/// Records the spawned leader before its first byte, only if recovery would accept it, so one bad
/// row can't fail every later pending read.
pub fn record_leader(
    office: &Office,
    key: OwnedProcessKey,
    leader: &ChildIdentity,
) -> io::Result<()> {
    let row = office.load_owned_process(key).map_err(io_error)?;
    if row.is_none_or(|row| row.mark != leader.mark) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "no row with the leader's mark",
        ));
    }
    RecoveryRecord::new(leader.mark.clone(), Some(leader.clone()), vec![])?;
    let ChildIdentity {
        pid,
        pgid,
        started,
        boot,
        ..
    } = leader.clone();
    office
        .set_owned_leader(
            key,
            &SavedLeader {
                pid,
                pgid,
                started,
                boot,
            },
        )
        .map_err(io_error)
}

impl RecoveryRecords for OfficeRecoveryRecords<'_> {
    type Key = OwnedProcessKey;

    fn pending(&mut self) -> io::Result<Vec<(OwnedProcessKey, RecoveryRecord)>> {
        let rows = self.0.pending_owned_processes().map_err(io_error)?;
        rows.into_iter()
            .map(|row| Ok((row.key, record(row)?)))
            .collect()
    }

    fn load(
        &mut self,
        key: &OwnedProcessKey,
        deadline: Instant,
    ) -> io::Result<Option<RecoveryRecord>> {
        let row = self.0.load_owned_process(*key).map_err(io_error)?;
        if Instant::now() >= deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        row.map(record).transpose()
    }
}

impl RecoveryCheckpoint for OfficeRecoveryRecords<'_> {
    fn checkpoint(
        &mut self,
        key: &OwnedProcessKey,
        mark: &str,
        inventory: &[CheckpointTarget],
        deadline: Instant,
    ) -> io::Result<()> {
        // org-supervise's checks, before anything is written.
        RecoveryRecord::new(mark.to_owned(), None, vec![])?.with_inventory(inventory.to_vec())?;
        let targets: Vec<SavedTarget> = inventory
            .iter()
            .map(|t| SavedTarget {
                identity: saved(&t.identity),
                ancestry: t.ancestry.iter().map(saved).collect(),
            })
            .collect();
        self.0
            .checkpoint_owned_inventory(*key, mark, &targets, deadline)
            .map_err(io_error)
    }
}

#[cfg(test)]
mod tests;
