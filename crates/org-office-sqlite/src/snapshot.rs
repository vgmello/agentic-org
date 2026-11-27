//! A snapshot giving its connections back to the pool, and its [`CoreRead`].

use org_store::{CoreRead, Cursor, DurablePath, EngineError, Id, Receipt, Sha256};

use super::{lock, receipts, SqliteSnapshot, MAX_IDLE_READERS};

impl Drop for SqliteSnapshot {
    fn drop(&mut self) {
        let ended_office = self
            .db
            .take()
            .filter(|db| db.execute_batch("ROLLBACK").is_ok());
        let ended_msg = self
            .msg_db
            .take()
            .filter(|db| db.execute_batch("ROLLBACK").is_ok());
        let mut pool = lock(&self.readers.pool);
        pool.open -= 1;
        if let (Some(db), Some(msg_db)) = (ended_office, ended_msg) {
            if pool.idle.len() < MAX_IDLE_READERS {
                pool.idle.push((db, msg_db));
            }
        }
        drop(pool);
        self.readers.ended.notify_all();
    }
}

impl CoreRead for SqliteSnapshot {
    fn cursor(&self) -> Result<Option<Cursor>, EngineError> {
        receipts::cursor(self.db())
    }

    fn source_hash(&self, path: &DurablePath) -> Result<Option<Sha256>, EngineError> {
        receipts::source_hash(self.db(), path)
    }

    fn sources_under(&self, prefix: &DurablePath) -> Result<Vec<DurablePath>, EngineError> {
        receipts::sources_under(self.db(), prefix)
    }

    fn receipt(&self, id: &Id) -> Result<Option<Receipt>, EngineError> {
        receipts::receipt(self.db(), id)
    }

    fn list_receipts_by_prefix(&self, prefix: &str) -> Result<Vec<Receipt>, EngineError> {
        receipts::list_receipts_by_prefix(self.db(), prefix)
    }
}
