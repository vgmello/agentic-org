//! Opening an office's store (ADR-0018, ADR-0021, `docs/design/office-store.md`): the one
//! place that names an engine. Everything above it holds an [`OfficeStore`] and passes it
//! operations; nothing above it is generic over the engine, and nothing but this file and
//! `store/sqlite/` knows which engine there is.
//!
//! Dispatch is static (design open point 1): [`OfficeStore`] is an enum with one arm per
//! engine, and an operation is a [`StoreOp`] or a [`ReadOp`], generic over the engine and
//! compiled once per arm.
//!
//! - Adding an operation: one `StoreOp`; no shared file changes.
//! - Adding a family: one bound on [`OfficeEngine`] and one projector in `open_on`.
//! - Adding an engine: one arm of [`EngineChoice`] and of [`OfficeStore`].

mod operations;
#[cfg(test)]
mod tests;

use std::{path::PathBuf, sync::Arc, time::Duration};

use org_store::{
    Clock, EngineError, FeedPosition, OfficeIdentity, Opening, Projector, Status, Store,
    StoreError, StoreParts, WriterAuthority,
};

pub use operations::{CreateOrganizationOp, CreateTeamOp, DirectoryGeneration};

use super::sqlite::SqliteEngine;
use crate::organization::{
    ports::{OrganizationEngine, OrganizationRead},
    projector::OrganizationFiles,
};

/// The file of an office's projection, under `<runtime-root>/<office-id>/`.
const PROJECTION_FILE: &str = "office.db";

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
    #[cfg(test)]
    pub(crate) extra_ddl: Option<&'static str>,
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

/// An engine every family of the office runs on. It grows by one bound per family.
pub trait OfficeEngine: OrganizationEngine {}

impl<E: OrganizationEngine> OfficeEngine for E {}

/// One operation on the store, whichever engine holds the projection.
pub trait StoreOp {
    type Out;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out;
}

/// One read of a consistent snapshot, whichever engine holds the projection.
pub trait ReadOp {
    type Out;
    fn read<S: OrganizationRead>(self, snapshot: &S) -> Result<Self::Out, StoreError>;
}

/// A store and the engine under it.
pub struct Opened<E: OfficeEngine> {
    store: Store<E>,
    engine: Arc<E>,
}

/// An office's open store.
pub enum OfficeStore {
    Sqlite(Opened<SqliteEngine>),
    #[cfg(any(test, feature = "testkit"))]
    Memory(Opened<org_store::testkit::MemoryEngine>),
}

/// The same expression on whichever arm is open.
macro_rules! on_store {
    ($office:expr, $opened:ident => $body:expr) => {
        match $office {
            OfficeStore::Sqlite($opened) => $body,
            #[cfg(any(test, feature = "testkit"))]
            OfficeStore::Memory($opened) => $body,
        }
    };
}

/// The SQLite projection of `config`'s office, in a folder only its owner can enter.
fn sqlite_engine(config: &StoreConfig) -> Result<SqliteEngine, StoreError> {
    let path = config.projection_path();
    let folder = path.parent().expect("the projection is in a folder");
    let made = std::fs::create_dir_all(folder).and_then(|()| owner_only(folder));
    made.map_err(|error| EngineError::new(format!("making the runtime folder: {error}")))?;
    #[cfg(test)]
    if let Some(ddl) = config.testing.extra_ddl {
        let engine = SqliteEngine::open_with(&path, ddl)?;
        return Ok(engine.for_organization(&config.identity.organization_id));
    }
    let engine = SqliteEngine::open(&path)?;
    Ok(engine.for_organization(&config.identity.organization_id))
}

#[cfg(unix)]
fn owner_only(folder: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn owner_only(_folder: &std::path::Path) -> std::io::Result<()> {
    Ok(())
}

/// The memory projection: always empty when opened, so the store rebuilds it.
#[cfg(any(test, feature = "testkit"))]
fn memory_engine() -> org_store::testkit::MemoryEngine {
    org_store::testkit::MemoryEngine::new()
}

/// The store on `engine`, with every family's projector.
fn open_on<E: OfficeEngine>(config: &StoreConfig, engine: E) -> Result<Opened<E>, StoreError> {
    let engine = Arc::new(engine);
    let organization = config.identity.organization_id.clone();
    let files: Arc<dyn Projector<E>> = Arc::new(OrganizationFiles::new(organization));
    #[cfg_attr(not(any(test, feature = "testkit")), allow(unused_mut))]
    let mut projectors = vec![files];
    #[cfg(any(test, feature = "testkit"))]
    if let Some(probe) = &config.testing.probe {
        projectors.push(probe.projector());
    }
    #[cfg_attr(not(any(test, feature = "testkit")), allow(unused_mut))]
    let mut parts = StoreParts::new(
        config.durable_root.clone(),
        config.identity.clone(),
        Arc::clone(&config.custody),
        Arc::clone(&engine),
        projectors,
        Arc::clone(&config.clock),
    );
    parts.flush_interval = config.flush_interval;
    parts.opening = config.opening.clone();
    #[cfg(any(test, feature = "testkit"))]
    if let Some(faults) = &config.testing.faults {
        parts = org_store::testkit::with_faults(parts, faults);
    }
    let store = Store::open(parts)?;
    Ok(Opened { store, engine })
}

/// Opens the office's store on the engine the configuration names. The projection is
/// made, or rebuilt from the files and the journal, as it needs to be.
pub fn open_store(config: &StoreConfig) -> Result<OfficeStore, StoreError> {
    match config.engine {
        EngineChoice::Sqlite => Ok(OfficeStore::Sqlite(open_on(
            config,
            sqlite_engine(config)?,
        )?)),
        #[cfg(any(test, feature = "testkit"))]
        EngineChoice::Memory => Ok(OfficeStore::Memory(open_on(config, memory_engine())?)),
    }
}

impl OfficeStore {
    pub fn run<O: StoreOp>(&self, op: O) -> O::Out {
        on_store!(self, opened => op.run(&opened.store))
    }

    /// One consistent snapshot, with the feed position it stands at. Touches no file, no
    /// journal, no writer and no host.
    pub fn read<R: ReadOp>(&self, op: R) -> Result<(R::Out, FeedPosition), StoreError> {
        on_store!(self, opened => opened.store.read(|snapshot| op.read(snapshot)))
    }

    pub fn status(&self) -> Status {
        on_store!(self, opened => opened.store.status())
    }

    /// Returns when every accepted entry is flushed, confirmed and written to its files.
    pub fn sync(&self) -> Result<(), StoreError> {
        on_store!(self, opened => opened.store.sync())
    }

    /// The orderly stop. The SQLite projection's log is then folded into its file; a
    /// store that is dropped instead does no such work and loses nothing by it.
    pub fn close(self) -> Result<(), StoreError> {
        match self {
            OfficeStore::Sqlite(opened) => {
                opened.store.close()?;
                Ok(opened.engine.checkpoint()?)
            }
            #[cfg(any(test, feature = "testkit"))]
            OfficeStore::Memory(opened) => opened.store.close(),
        }
    }

    /// The digest of the DDL the SQLite projection is made from.
    #[cfg(test)]
    pub(crate) fn schema_digest(&self) -> Option<String> {
        match self {
            OfficeStore::Sqlite(opened) => Some(opened.engine.schema_digest()),
            OfficeStore::Memory(_) => None,
        }
    }

    /// Every row of the projection, to compare with another of the same engine.
    #[cfg(any(test, feature = "testkit"))]
    pub fn dump(&self) -> Vec<String> {
        use org_store::testkit::Dump;
        on_store!(self, opened => opened.engine.dump())
    }
}
