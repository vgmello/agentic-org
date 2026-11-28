//! `OfficeEngineAdapter` and `OfficeStorageBackend` implementations for SQLite (`TASK-255`,
//! `TASK-282`), explicitly injected at composition roots.

use std::path::Path;
use std::sync::Arc;

use org_office::{
    accounting::AccountingStore,
    attachments::user_space::UserSpaceStore,
    budgets::BudgetStore,
    private_notes::PrivateNotesStore,
    providers::ProviderPoolStore,
    replication::{durable::ReplicationProjection, memory::ReplicationCoreData},
    search::ports::PrivateNotesSearch,
    BoxedSnapshot, BoxedTx, OfficeEngineAdapter, OfficeError, OfficeStorageBackend,
    PrivateNotesOpened, StoreConfig,
};
use org_store::{Cursor, Engine, EngineError, Id, StoreError};

use crate::{
    accounting::SqliteAccountingStore, budgets::SqliteBudgetStore,
    private_notes::SqlitePrivateNotesStore, private_notes_copy, providers::SqliteProviderPoolStore,
    replication::SqliteReplicationProjection, search::private::SqlitePrivateNotesSearch,
    user_space::SqliteUserSpaceStore, SqliteEngine, SqliteTx,
};

impl OfficeEngineAdapter for SqliteEngine {
    fn begin(&self) -> Result<BoxedTx, EngineError> {
        Ok(BoxedTx::new(Engine::begin(self)?))
    }

    fn commit(&self, tx: BoxedTx, cursor: &Cursor) -> Result<(), EngineError> {
        let tx = tx.into_inner::<SqliteTx>()?;
        Engine::commit(self, tx, cursor)
    }

    fn snapshot(&self) -> Result<BoxedSnapshot, EngineError> {
        Ok(BoxedSnapshot::new(Engine::snapshot(self)?))
    }

    fn replace(
        &self,
        build: &mut dyn FnMut(&mut BoxedTx) -> Result<Cursor, StoreError>,
    ) -> Result<(), StoreError> {
        self.replace_boxed(build)
    }

    fn after_round(&self) {
        Engine::after_round(self);
    }

    fn upkeep_error(&self) -> Option<EngineError> {
        Engine::upkeep_error(self)
    }

    fn checkpoint(&self) -> Result<(), EngineError> {
        SqliteEngine::checkpoint(self)
    }

    #[cfg(any(test, feature = "testkit"))]
    fn dump(&self) -> Vec<String> {
        org_store::testkit::Dump::dump(self)
    }

    #[cfg(any(test, feature = "testkit"))]
    fn schema_digest(&self) -> Option<String> {
        Some(SqliteEngine::schema_digest(self))
    }
}

/// Concrete SQLite implementation of [`OfficeStorageBackend`].
#[derive(Clone, Copy, Debug, Default)]
pub struct SqliteStorageBackend;

impl OfficeStorageBackend for SqliteStorageBackend {
    fn open_projection(
        &self,
        config: &StoreConfig,
    ) -> Result<Arc<dyn OfficeEngineAdapter>, StoreError> {
        let engine = open_sqlite_projection(config)?;
        Ok(Arc::new(engine))
    }

    fn open_validation_engine(
        &self,
        db_path: &Path,
        organization_id: &Id,
    ) -> Result<Arc<dyn OfficeEngineAdapter>, EngineError> {
        let engine = SqliteEngine::open(db_path)?.for_organization(organization_id);
        Ok(Arc::new(engine))
    }

    fn open_private_notes(
        &self,
        durable_root: &Path,
        runtime_root: &Path,
        office_id: &str,
        org_id: &str,
    ) -> Result<PrivateNotesOpened, OfficeError> {
        let store = SqlitePrivateNotesStore::open(durable_root)?;
        let search = SqlitePrivateNotesSearch::open(durable_root, runtime_root, office_id, org_id)?;
        Ok(PrivateNotesOpened {
            store: Arc::new(store) as Arc<dyn PrivateNotesStore>,
            search: Arc::new(search) as Arc<dyn PrivateNotesSearch>,
        })
    }

    fn open_user_space(&self, durable_root: &Path) -> Result<Arc<dyn UserSpaceStore>, OfficeError> {
        Ok(Arc::new(SqliteUserSpaceStore::open(durable_root)?))
    }

    fn open_accounting(
        &self,
        durable_root: &Path,
        runtime_root: &Path,
        office_id: &str,
        org_id: &str,
    ) -> Result<Arc<dyn AccountingStore>, OfficeError> {
        Ok(Arc::new(SqliteAccountingStore::open(
            durable_root,
            runtime_root,
            office_id,
            org_id,
        )?))
    }

    fn open_budgets(
        &self,
        durable_root: &Path,
        runtime_root: &Path,
        office_id: &str,
        org_id: &str,
    ) -> Result<Arc<dyn BudgetStore>, OfficeError> {
        Ok(Arc::new(SqliteBudgetStore::open(
            durable_root,
            runtime_root,
            office_id,
            org_id,
        )?))
    }

    fn open_providers(
        &self,
        durable_root: &Path,
        runtime_root: &Path,
        office_id: &str,
        org_id: &str,
        is_hq: bool,
    ) -> Result<Arc<dyn ProviderPoolStore>, OfficeError> {
        Ok(Arc::new(SqliteProviderPoolStore::open(
            durable_root,
            runtime_root,
            office_id,
            org_id,
            is_hq,
        )?))
    }

    fn open_replication_projection(
        &self,
        runtime_root: &Path,
        local_office: &org_sync_proto::OfficeId,
        data: &ReplicationCoreData,
    ) -> Result<Box<dyn ReplicationProjection>, OfficeError> {
        Ok(Box::new(SqliteReplicationProjection::open(
            runtime_root,
            local_office,
            data,
        )?))
    }

    fn snapshot_task_notes(&self, source: &Path, target: &Path) -> Result<(), OfficeError> {
        private_notes_copy::snapshot_task_notes(source, target)
    }

    fn check_task_notes(&self, path: &Path) -> Result<(), OfficeError> {
        private_notes_copy::check_task_notes(path)
    }

    fn snapshot_user_space(&self, source: &Path, target: &Path) -> Result<(), OfficeError> {
        private_notes_copy::snapshot_user_space(source, target)
    }

    fn check_user_space(&self, path: &Path) -> Result<(), OfficeError> {
        private_notes_copy::check_user_space(path)
    }
}

/// Opens a [`SqliteEngine`] at `config.projection_path()` inside an owner-only runtime folder,
/// scoped to `config.identity.organization_id`.
pub fn open_sqlite_projection(config: &StoreConfig) -> Result<SqliteEngine, StoreError> {
    let path = config.projection_path();
    let folder = path
        .parent()
        .ok_or_else(|| EngineError::new("the projection path has no parent folder"))?;
    let made =
        std::fs::create_dir_all(folder).and_then(|()| org_platform::fs::set_owner_only_dir(folder));
    made.map_err(|error| EngineError::new(format!("making the runtime folder: {error}")))?;
    #[cfg(any(test, feature = "testkit"))]
    if let Some(ddl) = config.testing.extra_ddl {
        let engine = SqliteEngine::open_with(&path, ddl)?;
        return Ok(engine.for_organization(&config.identity.organization_id));
    }
    let engine = SqliteEngine::open(&path)?;
    Ok(engine.for_organization(&config.identity.organization_id))
}

/// Returns an `Arc<dyn OfficeStorageBackend>` backed by [`SqliteStorageBackend`].
pub fn sqlite_backend() -> Arc<dyn OfficeStorageBackend> {
    Arc::new(SqliteStorageBackend)
}
