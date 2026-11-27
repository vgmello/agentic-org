//! The engine seam (ADR-0018). `Engine` and `CorePort` are implemented once per engine;
//! `Projector` once per family, for every engine at once.

use serde::{Deserialize, Serialize};

use crate::{
    change::{Id, Receipt, Sha256},
    error::{EngineError, ProjectionError, StoreError},
    path::DurablePath,
    sequences::{FeedPosition, HighWaters},
};

/// How far the projection has applied the journal. Committed with every batch, so a read
/// returns the positions of its own snapshot.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cursor {
    /// The last applied entry; zero is the journal's anchor.
    pub ordinal: u64,
    pub sha256: Sha256,
    pub feed: FeedPosition,
    /// The origin high-waters as of that entry. `organization` is the directory generation.
    pub high_waters: HighWaters,
}

/// A savepoint in the writer's transaction. Only the most recent mark can be undone to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Mark(pub u64);

/// The database the projection lives in.
pub trait Engine: Send + Sync + 'static {
    /// The writer's open transaction. Dropping it without `commit` rolls it back.
    type Tx: CorePort + Send;
    /// A consistent read view, taken between batches.
    type Snapshot: CoreRead;

    fn begin(&self) -> Result<Self::Tx, EngineError>;
    fn commit(&self, tx: Self::Tx, cursor: &Cursor) -> Result<(), EngineError>;
    fn snapshot(&self) -> Result<Self::Snapshot, EngineError>;
    /// Builds a new, empty projection with `build` and installs it atomically in place of
    /// this one. If `build` fails the current projection stays.
    fn replace(
        &self,
        build: &mut dyn FnMut(&mut Self::Tx) -> Result<Cursor, StoreError>,
    ) -> Result<(), StoreError>;
    /// The engine's own upkeep, for what would otherwise grow for as long as the store
    /// stays open (a write-ahead log to fold into its file). The flusher calls it after
    /// each round that wrote files and set the mark, before anyone waiting for that round
    /// is told: never on the writer's thread and never inside a transaction, though the
    /// writer may hold one meanwhile. It must not wait for a reader or for the writer's
    /// transactions; it may wait for a `replace`, which runs only at open, before the
    /// flusher starts, so the two never meet in a running store.
    ///
    /// The store depends on nothing it does, so it returns nothing: an engine whose
    /// upkeep fails leaves it for the next round. Persistent failures are reported through
    /// [`Engine::upkeep_error`] so callers and health checks observe them instead of dropping
    /// errors silently. The default does nothing.
    fn after_round(&self) {}

    /// Persistent engine upkeep failure (for example repeated SQLite WAL checkpoint failures),
    /// surfaced via [`crate::Status::upkeep_warning`]. Transient single-round contention stays
    /// `None`; a subsequent healthy upkeep clears it.
    ///
    /// Called while [`crate::Store::status`] / [`crate::Store::wait_for`] holds the store's flow
    /// mutex: implementations MUST be fast and non-blocking, never acquire I/O or database
    /// connection locks, and never re-enter [`crate::Store`].
    fn upkeep_error(&self) -> Option<EngineError> {
        None
    }
}

/// The store's own rows: the applied cursor, the hash of every source file, the receipts.
pub trait CoreRead {
    fn cursor(&self) -> Result<Option<Cursor>, EngineError>;
    fn source_hash(&self, path: &DurablePath) -> Result<Option<Sha256>, EngineError>;
    /// Every source at or under `prefix`, in byte order of path.
    fn sources_under(&self, prefix: &DurablePath) -> Result<Vec<DurablePath>, EngineError>;
    fn receipt(&self, id: &Id) -> Result<Option<Receipt>, EngineError>;

    /// Lists all stored receipts whose `kind` starts with `prefix`, ordered by ascending
    /// `(ordinal, operation_id)`. Used by `OrgFeed` to replay committed changes after restart.
    fn list_receipts_by_prefix(&self, _prefix: &str) -> Result<Vec<Receipt>, EngineError> {
        Ok(Vec::new())
    }
}

pub trait CorePort: CoreRead {
    fn mark(&mut self) -> Result<Mark, EngineError>;
    fn undo_to(&mut self, mark: Mark) -> Result<(), EngineError>;
    fn set_source(&mut self, path: &DurablePath, hash: Option<Sha256>) -> Result<(), EngineError>;
    /// Records a receipt. A receipt is never replaced: a second one for the same operation
    /// ID is an error.
    fn put_receipt(&mut self, receipt: &Receipt) -> Result<(), EngineError>;
    /// Whether the transaction, as it stands, could commit: an engine that checks
    /// references only at commit reports a pending violation here. The writer calls it
    /// after each operation, before the operation's entry is framed, because nothing may
    /// fail after the append.
    fn check(&mut self) -> Result<(), EngineError>;
}

/// A family's projection of its own files: the same function on the write path and in
/// rebuild. It keeps no state and reads nothing but the path and the bytes.
pub trait Projector<E: Engine>: Send + Sync {
    /// Every source path has exactly one owner.
    fn owns(&self, path: &DurablePath) -> bool;
    /// Replaces every row this file contributes; `None` when the file is removed.
    fn apply(
        &self,
        tx: &mut E::Tx,
        path: &DurablePath,
        bytes: Option<&[u8]>,
    ) -> Result<(), ProjectionError>;
    /// After a rebuild: checks that span files.
    fn validate(&self, _tx: &E::Tx) -> Result<(), ProjectionError> {
        Ok(())
    }
}
