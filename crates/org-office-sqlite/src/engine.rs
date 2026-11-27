//! [`Engine`] for [`SqliteEngine`]: transactions, snapshots, the flusher's checkpoint
//! round and replacing the projection.

use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

use org_store::{Cursor, Engine, EngineError, StoreError};

use super::connection::{beside, connect_reader, remove_database};
use super::{lock, messaging, refused, Pool, SqliteEngine, SqliteSnapshot, SqliteTx};

/// How long `replace` waits for open snapshots to end before it fails.
const DRAIN_WAIT: Duration = Duration::from_secs(30);

/// Consecutive failed or reader-blocked background WAL checkpoints before surfacing a persistent
/// upkeep warning through [`Engine::upkeep_error`] (`TASK-33` / B1c).
const CHECKPOINT_FAILURE_THRESHOLD: u32 = 3;

impl Engine for SqliteEngine {
    type Tx = SqliteTx;
    type Snapshot = SqliteSnapshot;

    fn begin(&self) -> Result<SqliteTx, EngineError> {
        if self.diverged.load(Ordering::Acquire) {
            return Err(EngineError::new(
                "projection diverged after partial commit; reopen required",
            ));
        }
        let Some((db, msg_db)) = lock(&self.writer).take() else {
            return Err(EngineError::new("a transaction is already open"));
        };
        self.transaction(db, msg_db, Arc::clone(&self.writer))
    }

    fn commit(&self, tx: SqliteTx, cursor: &Cursor) -> Result<(), EngineError> {
        let (db, msg_db) = tx.finish(cursor)?;
        *lock(&self.writer) = Some((db, msg_db));
        Ok(())
    }

    fn snapshot(&self) -> Result<SqliteSnapshot, EngineError> {
        if self.diverged.load(Ordering::Acquire) {
            return Err(EngineError::new(
                "projection diverged after partial commit; reopen required",
            ));
        }
        let pooled = {
            let mut pool = lock(&self.readers.pool);
            pool.open += 1;
            pool.idle.pop()
        };
        // From here the snapshot's drop gives the count back, with or without a connection.
        let mut snapshot = SqliteSnapshot {
            db: None,
            msg_db: None,
            organization: self.organization.clone(),
            readers: Arc::clone(&self.readers),
        };
        let (db, msg_db) = match pooled {
            Some(pair) => pair,
            None => (
                connect_reader(&self.path)?,
                messaging::open_messaging_reader(&self.msg_path)?,
            ),
        };
        // Hold the leaf commit gate in shared mode ONLY across the two `BEGIN` statements
        // so both connections fix their read view at the exact same dual-DB commit cursor,
        // and refuse any read once a partial commit has marked the engine diverged (`TASK-97`).
        {
            let _gate = self.commit_gate.read().unwrap_or_else(|p| p.into_inner());
            if self.diverged.load(Ordering::Acquire) {
                return Err(EngineError::new(
                    "projection diverged after partial commit; reopen required",
                ));
            }
            db.execute_batch("BEGIN; SELECT id FROM store_schema;")
                .map_err(|error| refused("beginning a read", error))?;
            msg_db
                .execute_batch("BEGIN; SELECT id FROM messaging_schema;")
                .map_err(|error| {
                    let _ = db.execute_batch("ROLLBACK");
                    refused("beginning a messaging read", error)
                })?;
        }
        snapshot.db = Some(db);
        snapshot.msg_db = Some(msg_db);
        Ok(snapshot)
    }

    /// Folds the log into the file after every round of the flusher, so the log does not
    /// grow for as long as the store runs: once it is all folded in and no read is on
    /// it, the writer's next transaction starts it from its beginning. A long read only
    /// delays that. Repeated checkpoint failures or reader-blocked WAL folding
    /// (`busy != 0 || (log > 0 && checkpointed < log)`) are logged and surfaced via
    /// [`Engine::upkeep_error`] rather than silently dropped (`TASK-33` / B1c).
    fn after_round(&self) {
        let failure = match self.run_passive_checkpoint() {
            Ok((busy, log, checkpointed)) if busy != 0 || (log > 0 && checkpointed < log) => {
                Some(format!(
                    "WAL checkpoint incomplete (busy={busy}, {checkpointed}/{log} frames folded)"
                ))
            }
            Ok(_) => {
                self.record_checkpoint_ok();
                None
            }
            Err(error) => Some(error.message),
        };
        if let Some(detail) = failure {
            let count = self.record_checkpoint_failure(detail.clone());
            if count == 1 || count == CHECKPOINT_FAILURE_THRESHOLD || count.is_multiple_of(10) {
                eprintln!(
                    "agentic-org-office: sqlite WAL checkpoint warning ({count} consecutive): {detail}"
                );
            }
        }
    }

    fn upkeep_error(&self) -> Option<EngineError> {
        let health = lock(&self.checkpoint_health);
        if health.consecutive_failures < CHECKPOINT_FAILURE_THRESHOLD {
            return None;
        }
        let detail = health
            .last_error
            .as_deref()
            .unwrap_or("unknown checkpoint error");
        Some(EngineError::new(format!(
            "sqlite WAL checkpoint failed {} consecutive times: {detail}",
            health.consecutive_failures
        )))
    }

    fn replace(
        &self,
        build: &mut dyn FnMut(&mut SqliteTx) -> Result<Cursor, StoreError>,
    ) -> Result<(), StoreError> {
        self.replace_with(|new_path, new_msg_path| {
            self.build_replacement(new_path, new_msg_path, build)
        })
    }
}

impl SqliteEngine {
    /// Type-erased replacement path invoked by [`org_office::OfficeEngineAdapter::replace`].
    pub(crate) fn replace_boxed(
        &self,
        build: &mut dyn FnMut(&mut org_office::BoxedTx) -> Result<Cursor, StoreError>,
    ) -> Result<(), StoreError> {
        self.replace_with(|new_path, new_msg_path| {
            self.build_replacement_boxed(new_path, new_msg_path, build)
        })
    }

    fn replace_with(
        &self,
        run_build: impl FnOnce(&std::path::Path, &std::path::Path) -> Result<(), StoreError>,
    ) -> Result<(), StoreError> {
        // Stage `office.db.new` (and its DELETE-mode rollback journal) inside the `0700`
        // `protected/` directory so Linux `bwrap` directory masks always isolate rebuild files
        // while keeping `fs::rename` atomic on the same filesystem (`TASK-166`).
        let new_path = self
            .msg_path
            .parent()
            .expect("messaging.db sits inside protected/")
            .join("office.db.new");
        let new_msg_path = beside(&self.msg_path, ".new");
        remove_database(&new_path)?;
        remove_database(&new_msg_path)?;
        if let Err(error) = run_build(&new_path, &new_msg_path) {
            let _ = remove_database(&new_path);
            let _ = remove_database(&new_msg_path);
            return Err(error);
        }
        // Readers drained: wait for every open snapshot, and let none begin until the
        // new file has the name.
        let pool = lock(&self.readers.pool);
        let still_open = |pool: &mut Pool| pool.open > 0;
        let drained = self
            .readers
            .ended
            .wait_timeout_while(pool, DRAIN_WAIT, still_open);
        let (mut pool, waited) = drained.unwrap_or_else(|poisoned| poisoned.into_inner());
        let installed = if waited.timed_out() {
            Err(EngineError::new(
                "a read was still open when the projection was to be replaced",
            ))
        } else {
            pool.idle.clear();
            self.install(&new_path, &new_msg_path)
        };
        if installed.is_err() {
            let _ = remove_database(&new_path);
            let _ = remove_database(&new_msg_path);
        }
        installed.map_err(StoreError::from)
    }
}
