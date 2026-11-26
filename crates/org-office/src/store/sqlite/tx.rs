//! The writer's transaction ending without a commit, and its [`CoreRead`] and [`CorePort`].

use org_store::{CorePort, CoreRead, Cursor, DurablePath, EngineError, Id, Mark, Receipt, Sha256};

use super::references::check_references;
use super::{failed, lock, receipts, SqliteTx};

impl Drop for SqliteTx {
    fn drop(&mut self) {
        let (Some(db), Some(msg_db)) = (self.db.take(), self.msg_db.take()) else {
            return;
        };
        if !db.is_autocommit() {
            let _ = db.execute_batch("ROLLBACK");
        }
        if !msg_db.is_autocommit() {
            let _ = msg_db.execute_batch("ROLLBACK");
        }
        if db.is_autocommit() && msg_db.is_autocommit() {
            *lock(&self.home) = Some((db, msg_db));
        }
    }
}

impl CoreRead for SqliteTx {
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

impl CorePort for SqliteTx {
    fn mark(&mut self) -> Result<Mark, EngineError> {
        let db = self.db();
        let msg_db = self.msg_db();
        let sql = match self.mark {
            Some(_) => "RELEASE mark; SAVEPOINT mark;",
            None => "SAVEPOINT mark;",
        };
        db.execute_batch(sql)
            .map_err(|error| failed(db, "marking", &error))?;
        msg_db
            .execute_batch(sql)
            .map_err(|error| failed(msg_db, "marking messaging.db", &error))?;
        self.marks += 1;
        self.mark = Some(self.marks);
        Ok(Mark(self.marks))
    }

    fn undo_to(&mut self, mark: Mark) -> Result<(), EngineError> {
        if self.mark != Some(mark.0) {
            return Err(EngineError::new(
                "only the most recent mark can be undone to",
            ));
        }
        let db = self.db();
        let msg_db = self.msg_db();
        db.execute_batch("ROLLBACK TO mark; RELEASE mark;")
            .map_err(|error| failed(db, "undoing to a mark", &error))?;
        msg_db
            .execute_batch("ROLLBACK TO mark; RELEASE mark;")
            .map_err(|error| failed(msg_db, "undoing messaging.db to a mark", &error))?;
        self.mark = None;
        Ok(())
    }

    fn set_source(&mut self, path: &DurablePath, hash: Option<Sha256>) -> Result<(), EngineError> {
        receipts::set_source(self.db(), path, hash)
    }

    fn put_receipt(&mut self, receipt: &Receipt) -> Result<(), EngineError> {
        receipts::put_receipt(self.db(), receipt)
    }

    fn check(&mut self) -> Result<(), EngineError> {
        let db = self.db();
        let msg_db = self.msg_db();
        if db.is_autocommit() || msg_db.is_autocommit() {
            return Err(EngineError::ended("the transaction has ended"));
        }
        check_references(db)?;
        check_references(msg_db)
    }
}
