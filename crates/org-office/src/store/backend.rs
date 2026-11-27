//! Consumer-owned storage backend port for plugging external database engines (such as
//! `org-office-sqlite`) into `org-office` without inverting the dependency direction (`TASK-255`,
//! `TASK-282`).
//!
//! Callers inject an explicit `Arc<dyn OfficeStorageBackend>` through [`EngineChoice::Sqlite`]
//! and `Office::open*` / `Office::provision*` / `Office::import_hq` — no process-global singleton
//! or ambient state exists.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
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
#[derive(Clone)]
pub enum EngineChoice {
    /// SQLite projection + companion stores provided by an explicitly injected backend.
    Sqlite(Arc<dyn OfficeStorageBackend>),
    /// The memory engine: tests only, never part of a production build.
    #[cfg(any(test, feature = "testkit"))]
    Memory,
}

impl fmt::Debug for EngineChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(_) => f.write_str("EngineChoice::Sqlite(..)"),
            #[cfg(any(test, feature = "testkit"))]
            Self::Memory => f.write_str("EngineChoice::Memory"),
        }
    }
}

impl EngineChoice {
    /// The engine a configuration names, bound to an explicit SQLite backend when `"sqlite"`
    /// is chosen. A name with no engine built in (or `"sqlite"` without an injected backend)
    /// is refused: no engine is ever chosen in another's place.
    pub fn named(
        name: &str,
        sqlite_backend: Option<Arc<dyn OfficeStorageBackend>>,
    ) -> Result<Self, StoreError> {
        match name {
            "sqlite" => sqlite_backend.map(Self::Sqlite).ok_or_else(|| {
                StoreError::Invalid(
                    "SQLite storage engine requires an explicit OfficeStorageBackend".into(),
                )
            }),
            #[cfg(any(test, feature = "testkit"))]
            "memory" => Ok(Self::Memory),
            _ => Err(StoreError::Invalid(format!(
                "no storage engine named {name:?} is built into this office"
            ))),
        }
    }

    /// Whether this choice selects the SQLite backend.
    pub fn is_sqlite(&self) -> bool {
        matches!(self, Self::Sqlite(_))
    }

    /// Whether this choice selects the test-only memory engine.
    #[cfg(any(test, feature = "testkit"))]
    pub fn is_memory(&self) -> bool {
        matches!(self, Self::Memory)
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
            flush_interval: Duration::from_millis(50),
            opening: Opening::Existing,
            #[cfg(any(test, feature = "testkit"))]
            testing: Testing::default(),
        }
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
