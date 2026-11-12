//! B1-12: durable rows for the processes an office spawns, read by crash recovery. Plain data:
//! org-server adapts them to org-supervise's ports and owns inventory limits (`with_inventory`).
//! The SQL is `store::owned_processes`'s.

use serde::{Deserialize, Serialize};

use crate::error::OfficeError;
use crate::office::Office;
use crate::store::owned_processes as db;

/// A row's identity. `generation` is fixed when the row is made; ids are never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OwnedProcessKey {
    pub id: i64,
    pub generation: i64,
}

/// A process's PID with its start time and boot, as captured by org-supervise.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedIdentity {
    pub pid: u32,
    pub started: String,
    pub boot: String,
}

/// The spawned leader, recorded once before the first byte is sent to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedLeader {
    pub pid: u32,
    pub pgid: u32,
    pub started: String,
    pub boot: String,
}

/// One saved inventory entry: a process and its captured ancestry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedTarget {
    pub identity: SavedIdentity,
    pub ancestry: Vec<SavedIdentity>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedProcess {
    pub key: OwnedProcessKey,
    pub mark: String,
    pub leader: Option<SavedLeader>,
    /// Non-authoritative hints that narrow a scan after a restart; never a reason to signal.
    pub tool_sessions: Vec<u32>,
    pub inventory: Vec<SavedTarget>,
}

/// Every error from these calls may follow a commit that reached the log: the caller signals
/// nothing on an error and reloads the row before trying again.
impl Office {
    /// Makes a row for `mark` before the spawn that carries it. A mark is used once: a retry
    /// that gets `AlreadyExists` finds its row through `pending_owned_processes`.
    pub fn record_owned_mark(&self, mark: &str) -> Result<OwnedProcessKey, OfficeError> {
        let mark = mark.to_owned();
        let now = crate::time::format_epoch_ms(crate::time::now_ms());
        self.local_store()
            .write(move |tx| Ok((db::insert(tx, &mark, &now)?, None)))
    }

    /// Records the spawned leader once; the same leader again is a no-op, another is refused.
    pub fn set_owned_leader(
        &self,
        key: OwnedProcessKey,
        leader: &SavedLeader,
    ) -> Result<(), OfficeError> {
        let leader = leader.clone();
        self.local_store()
            .write(move |tx| Ok((db::set_leader(tx, key, &leader)?, None)))
    }

    /// Adds a tool session hint (a no-op if it is already saved).
    pub fn add_owned_tool_session(
        &self,
        key: OwnedProcessKey,
        session: u32,
    ) -> Result<(), OfficeError> {
        self.local_store()
            .write(move |tx| Ok((db::add_tool_session(tx, key, session)?, None)))
    }

    /// Every row not yet retired, oldest first. A row that can't be decoded fails the read.
    pub fn pending_owned_processes(&self) -> Result<Vec<OwnedProcess>, OfficeError> {
        self.local_store().read(|reader| db::select(reader, None))
    }

    /// The current row for `key`, or `None` if it was retired (never "its processes exited").
    pub fn load_owned_process(
        &self,
        key: OwnedProcessKey,
    ) -> Result<Option<OwnedProcess>, OfficeError> {
        self.local_store()
            .read(|reader| Ok(db::select(reader, Some(key))?.pop()))
    }

    /// Removes the row once its processes are proven gone. `false` if it was already gone.
    pub fn retire_owned_process(&self, key: OwnedProcessKey) -> Result<bool, OfficeError> {
        self.local_store()
            .write(move |tx| Ok((db::retire(tx, key)?, None)))
    }
}

#[cfg(test)]
pub(crate) mod tests;
