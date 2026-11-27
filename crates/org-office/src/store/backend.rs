//! Consumer-owned storage backend port for plugging external database engines (such as
//! `org-office-sqlite`) into `org-office` without inverting the dependency direction (`TASK-255`).
//!
//! Callers may inject an [`OfficeStorageBackend`] explicitly on [`StoreConfig`] / `Office::*_with_backend`
//! or install a process-wide default SQLite backend once at the composition root via
//! [`install_sqlite_backend`]. If no backend is provided or installed, opening a SQLite store
//! fails closed.

use std::path::Path;
use std::sync::{Arc, OnceLock};

use org_store::{EngineError, Id, StoreError};

use crate::{
    accounting::AccountingStore,
    attachments::user_space::UserSpaceStore,
    budgets::BudgetStore,
    error::OfficeError,
    private_notes::PrivateNotesStore,
    providers::ProviderPoolStore,
    replication::{durable::ReplicationProjection, memory::ReplicationCoreData},
    search::ports::PrivateNotesSearch,
};

use super::{erased::OfficeEngineAdapter, open::StoreConfig};

/// The private notes store and its disposable search index, opened together on HQ.
pub struct PrivateNotesOpened {
    pub store: Arc<dyn PrivateNotesStore>,
    pub search: Arc<dyn PrivateNotesSearch>,
}

/// Complete storage backend port covering the main office projection, companion stores,
/// import validation engine, and consistent private-database snapshots/checks.
pub trait OfficeStorageBackend: Send + Sync + 'static {
    /// Opens the main office projection engine (`office.db` + `protected/messaging.db`).
    fn open_projection(
        &self,
        config: &StoreConfig,
    ) -> Result<Arc<dyn OfficeEngineAdapter>, StoreError>;

    /// Opens a disposable projection engine at `db_path` scoped to `organization_id` for
    /// Git state import validation (`TASK-39` / R1).
    fn open_validation_engine(
        &self,
        db_path: &Path,
        organization_id: &Id,
    ) -> Result<Arc<dyn OfficeEngineAdapter>, EngineError>;

    /// Opens HQ's isolated private task notes store and its disposable search index.
    fn open_private_notes(
        &self,
        durable_root: &Path,
        runtime_root: &Path,
        office_id: &str,
        org_id: &str,
    ) -> Result<PrivateNotesOpened, OfficeError>;

    /// Opens HQ's isolated human-private file space store (`durable/private/user-space.db`).
    fn open_user_space(&self, durable_root: &Path) -> Result<Arc<dyn UserSpaceStore>, OfficeError>;

    /// Opens the isolated `L2` usage & cost accounting store.
    fn open_accounting(
        &self,
        durable_root: &Path,
        runtime_root: &Path,
        office_id: &str,
        org_id: &str,
    ) -> Result<Arc<dyn AccountingStore>, OfficeError>;

    /// Opens the isolated `L3` budgets & holds store.
    fn open_budgets(
        &self,
        durable_root: &Path,
        runtime_root: &Path,
        office_id: &str,
        org_id: &str,
    ) -> Result<Arc<dyn BudgetStore>, OfficeError>;

    /// Opens the isolated `L4` provider bindings and execution-authority store.
    fn open_providers(
        &self,
        durable_root: &Path,
        runtime_root: &Path,
        office_id: &str,
        org_id: &str,
        is_hq: bool,
    ) -> Result<Arc<dyn ProviderPoolStore>, OfficeError>;

    /// Opens the rebuildable replication projection over committed durable state.
    fn open_replication_projection(
        &self,
        runtime_root: &Path,
        local_office: &org_sync_proto::OfficeId,
        data: &ReplicationCoreData,
    ) -> Result<Box<dyn ReplicationProjection>, OfficeError>;

    /// Writes a consistent snapshot copy of `<durable>/private/task-notes.db`.
    fn snapshot_task_notes(&self, source: &Path, target: &Path) -> Result<(), OfficeError>;

    /// Runs integrity and schema verification on a copied private task notes database.
    fn check_task_notes(&self, path: &Path) -> Result<(), OfficeError>;

    /// Writes a consistent snapshot copy of `<durable>/private/user-space.db`.
    fn snapshot_user_space(&self, source: &Path, target: &Path) -> Result<(), OfficeError>;

    /// Runs integrity and schema verification on a copied private file space database.
    fn check_user_space(&self, path: &Path) -> Result<(), OfficeError>;
}

static SQLITE_BACKEND: OnceLock<Arc<dyn OfficeStorageBackend>> = OnceLock::new();

/// Installs the process-wide SQLite storage backend used when [`super::open::EngineChoice::Sqlite`]
/// is opened without an explicit backend. Called once at composition roots (for example
/// `apps/agentic-org-office` and `org-backup`). Subsequent calls are ignored and return `false`.
pub fn install_sqlite_backend(backend: Arc<dyn OfficeStorageBackend>) -> bool {
    SQLITE_BACKEND.set(backend).is_ok()
}

/// Returns the installed process-wide SQLite storage backend, failing closed if none was installed.
pub fn installed_sqlite_backend() -> Result<Arc<dyn OfficeStorageBackend>, StoreError> {
    SQLITE_BACKEND.get().cloned().ok_or_else(|| {
        StoreError::Invalid(
            "SQLite storage backend is not installed; call org_office_sqlite::install() \
             or pass an explicit OfficeStorageBackend"
                .into(),
        )
    })
}

/// Resolves an explicit backend if supplied, otherwise falls back to [`installed_sqlite_backend`].
pub(crate) fn resolve_sqlite_backend(
    explicit: Option<&Arc<dyn OfficeStorageBackend>>,
) -> Result<Arc<dyn OfficeStorageBackend>, StoreError> {
    match explicit {
        Some(backend) => Ok(Arc::clone(backend)),
        None => installed_sqlite_backend(),
    }
}
