//! Consumer-owned storage backend port for plugging external database engines (such as
//! `org-office-sqlite`) into `org-office` without inverting the dependency direction (`TASK-255`).
//!
//! Callers may inject an [`OfficeStorageBackend`] explicitly on [`StoreConfig`] / `Office::*_with_backend`
//! or install a process-wide default SQLite backend once at the composition root via
//! [`install_sqlite_backend`]. If no backend is provided or installed, opening a SQLite store
//! fails closed.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use org_store::{Clock, EngineError, Id, OfficeIdentity, Opening, StoreError, WriterAuthority};

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

use super::erased::OfficeEngineAdapter;

/// The file of an office's projection, under `<runtime-root>/<office-id>/`.
pub const PROJECTION_FILE: &str = "office.db";

/// Which engine holds the projection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EngineChoice {
    Sqlite,
    /// The memory engine: tests only, never part of a production build.
    #[cfg(any(test, feature = "testkit"))]
    Memory,
}

impl EngineChoice {
    /// The engine a configuration names. A name with no engine built in is refused: no
    /// engine is ever chosen in another's place.
    pub fn named(name: &str) -> Result<Self, StoreError> {
        match name {
            "sqlite" => Ok(Self::Sqlite),
            #[cfg(any(test, feature = "testkit"))]
            "memory" => Ok(Self::Memory),
            _ => Err(StoreError::Invalid(format!(
                "no storage engine named {name:?} is built into this office"
            ))),
        }
    }
}

/// What tests put under a store: a disk that fails and dies on request, and the probe
/// family beside the organization's.
#[cfg(any(test, feature = "testkit"))]
#[derive(Clone, Default)]
pub struct Testing {
    pub faults: Option<Arc<org_store::testkit::Faults>>,
    pub probe: Option<org_store::testkit::Probe>,
    /// DDL after this build's: the schema of another build.
    pub extra_ddl: Option<&'static str>,
    /// Override for [`StoreConfig::flush_interval`]: crash-point tests set an hour so the
    /// 50 ms background timer cannot fire mid-commit under load and race with writer crashes.
    pub flush_interval: Option<Duration>,
}

/// Everything an office's store is opened from.
#[derive(Clone)]
pub struct StoreConfig {
    pub engine: EngineChoice,
    /// `<data-root>/<organization-id>/durable`, absolute: the journal and the files.
    pub durable_root: PathBuf,
    /// The runtime root, absolute. The projection is `<runtime-root>/<office-id>/office.db`;
    /// that folder can be deleted, and the store rebuilds it.
    pub runtime_root: PathBuf,
    pub identity: OfficeIdentity,
    pub custody: Arc<dyn WriterAuthority>,
    pub clock: Arc<dyn Clock>,
    /// Optional explicit storage backend overriding the installed process default.
    pub backend: Option<Arc<dyn OfficeStorageBackend>>,
    /// How long appended entries may wait for a flush nobody asked for.
    pub flush_interval: Duration,
    pub opening: Opening,
    #[cfg(any(test, feature = "testkit"))]
    pub testing: Testing,
}

impl StoreConfig {
    /// An existing office's store, flushing as the store's default says.
    pub fn new(
        engine: EngineChoice,
        durable_root: PathBuf,
        runtime_root: PathBuf,
        identity: OfficeIdentity,
        custody: Arc<dyn WriterAuthority>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            engine,
            durable_root,
            runtime_root,
            identity,
            custody,
            clock,
            backend: None,
            flush_interval: Duration::from_millis(50),
            opening: Opening::Existing,
            #[cfg(any(test, feature = "testkit"))]
            testing: Testing::default(),
        }
    }

    /// Attaches an explicit [`OfficeStorageBackend`] to this configuration.
    pub fn with_backend(mut self, backend: Arc<dyn OfficeStorageBackend>) -> Self {
        self.backend = Some(backend);
        self
    }

    /// The projection's file.
    pub fn projection_path(&self) -> PathBuf {
        let folder = self.runtime_root.join(self.identity.office_id.as_str());
        folder.join(PROJECTION_FILE)
    }
}

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

/// Installs the process-wide SQLite storage backend used when [`EngineChoice::Sqlite`]
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
