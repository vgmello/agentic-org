//! Opening an office's store (ADR-0018, ADR-0021, `docs/design/office-store.md`): the one
//! place that names an engine. Everything above it holds an [`OfficeStore`] and passes it
//! operations; nothing above it is generic over the engine, and nothing but this file and
//! `store/sqlite/` knows which engine there is.
//!
//! Dispatch is static (design open point 1): [`OfficeStore`] is an enum with one arm per
//! engine, and an operation is a [`StoreOp`] or a [`ReadOp`], generic over the engine and
//! compiled once per arm.
//!
//! - Adding an operation: one `StoreOp` in `open/operations.rs`; this file and the
//!   engines do not change.
//! - Adding a family: one bound on [`OfficeEngine`] and one projector in `open_on`.
//! - Adding an engine: one arm of [`EngineChoice`] and of [`OfficeStore`].

mod operations;
#[cfg(test)]
mod tests;

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use org_store::{
    Clock, EngineError, FeedPosition, Files, FilesError, Id, OfficeIdentity, Opening, Projector,
    Status, Store, StoreError, StoreParts, WriterAuthority,
};

pub use operations::{
    AddPersonOp, AddProjectTeamOp, ArchiveTeamOp, CheckOp, CreateOrganizationOp, CreateProjectOp,
    CreateTeamOp, DeactivatePersonOp, DirectoryGeneration, DirectorySnapshot, ImportOrganizationOp,
    MovePersonOp, MoveTeamOp, OfficeStanding, ProjectCheckOp, ProjectInspectByRef, ProjectsList,
    ReceiptById, RemoveProjectTeamOp, Standing, StructureOp, TopTeamId, UpdatePersonOp,
    UpdateTeamOp,
};

use super::sqlite::SqliteEngine;
use crate::{
    office_local::{clean_stop, projector::OfficeLocalFiles, records::CleanStop},
    organization::projector::OrganizationFiles,
    projects::{
        ports::{ProjectsEngine, ProjectsRead},
        projector::ProjectFiles,
    },
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
pub trait OfficeEngine: ProjectsEngine {}

impl<E: ProjectsEngine> OfficeEngine for E {}

/// One operation on the store, whichever engine holds the projection.
pub trait StoreOp {
    type Out;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out;
}

/// One read of a consistent snapshot, whichever engine holds the projection.
pub trait ReadOp {
    type Out;
    fn read<S: ProjectsRead>(self, snapshot: &S) -> Result<Self::Out, StoreError>;
}

/// A store and the engine under it.
struct Opened<E: OfficeEngine> {
    store: Store<E>,
    engine: Arc<E>,
    /// Where an orderly stop leaves its marker, and for which journal.
    durable: PathBuf,
    office: Id,
    incarnation: Id,
    /// Whether the last stop before this opening was orderly.
    stopped_cleanly: bool,
}

enum OfficeStoreInner {
    Sqlite(Opened<SqliteEngine>),
    #[cfg(any(test, feature = "testkit"))]
    Memory(Opened<org_store::testkit::MemoryEngine>),
}

/// An office's open store. Engine variants stay private so callers cannot bypass [`open_store`].
pub struct OfficeStore(OfficeStoreInner);

/// The same expression on whichever arm is open.
macro_rules! on_store {
    ($office:expr, $opened:ident => $body:expr) => {
        match &$office.0 {
            OfficeStoreInner::Sqlite($opened) => $body,
            #[cfg(any(test, feature = "testkit"))]
            OfficeStoreInner::Memory($opened) => $body,
        }
    };
}

/// The SQLite projection of `config`'s office, in a folder only its owner can enter.
fn sqlite_engine(config: &StoreConfig) -> Result<SqliteEngine, StoreError> {
    let path = config.projection_path();
    let folder = path.parent().expect("the projection is in a folder");
    let made =
        std::fs::create_dir_all(folder).and_then(|()| org_platform::fs::set_owner_only_dir(folder));
    made.map_err(|error| EngineError::new(format!("making the runtime folder: {error}")))?;
    #[cfg(test)]
    if let Some(ddl) = config.testing.extra_ddl {
        let engine = SqliteEngine::open_with(&path, ddl)?;
        return Ok(engine.for_organization(&config.identity.organization_id));
    }
    let engine = SqliteEngine::open(&path)?;
    Ok(engine.for_organization(&config.identity.organization_id))
}

/// The memory projection: always empty when opened, so the store rebuilds it.
#[cfg(any(test, feature = "testkit"))]
fn memory_engine() -> org_store::testkit::MemoryEngine {
    org_store::testkit::MemoryEngine::new()
}

/// The store on `engine`, with every family's projector.
fn open_on<E: OfficeEngine>(config: &StoreConfig, engine: E) -> Result<Opened<E>, StoreError> {
    let engine = Arc::new(engine);
    let here = &config.identity;
    let organization = here.organization_id.clone();
    let files: Arc<dyn Projector<E>> = Arc::new(OrganizationFiles::new(organization.clone()));
    let projects: Arc<dyn Projector<E>> = Arc::new(ProjectFiles::new(organization.clone()));
    let local = OfficeLocalFiles::new(organization, here.office_id.clone());
    #[cfg_attr(not(any(test, feature = "testkit")), allow(unused_mut))]
    let mut projectors: Vec<Arc<dyn Projector<E>>> = vec![files, projects, Arc::new(local)];
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
    // The last stop's marker is read before the store opens, and removed only once it
    // has: an opening that fails leaves it for the next.
    let marker = clean_stop::read(&Files::open(&config.durable_root)?, &here.office_id);
    let store = Store::open(parts)?;
    let head = store.status().flushed;
    let stopped_cleanly = clean_stop::was_clean(marker.as_ref(), &here.journal_incarnation, head);
    if marker.is_some() {
        clean_stop::remove(&config.durable_root, &here.office_id)
            .map_err(|e| marker_error(&config.durable_root, "remove", &e))?;
    }
    Ok(Opened {
        store,
        engine,
        durable: config.durable_root.clone(),
        office: here.office_id.clone(),
        incarnation: here.journal_incarnation.clone(),
        stopped_cleanly,
    })
}

fn marker_error(durable: &Path, action: &'static str, error: &std::io::Error) -> StoreError {
    StoreError::Files(FilesError::Io {
        path: durable.display().to_string(),
        action,
        message: format!("the clean-stop marker: {error}"),
        flush: false,
    })
}

impl<E: OfficeEngine> Opened<E> {
    /// The orderly stop of the store itself: everything appended is flushed, confirmed
    /// and written to its files, and the threads are joined. Returns the marker to leave.
    fn close_store(self) -> Result<(Arc<E>, Marker), StoreError> {
        // The caller owns the store: no submission is in flight, so the head after this
        // sync is the head the store closes at.
        self.store.sync()?;
        let head = self.store.status().flushed;
        self.store.close()?;
        let marker = Marker {
            durable: self.durable,
            office: self.office,
            record: CleanStop {
                journal_incarnation: self.incarnation,
                head_ordinal: head,
            },
        };
        Ok((self.engine, marker))
    }
}

/// The marker an orderly stop leaves, once everything under it has succeeded.
struct Marker {
    durable: PathBuf,
    office: Id,
    record: CleanStop,
}

impl Marker {
    fn write(self) -> Result<(), StoreError> {
        clean_stop::write(&self.durable, &self.office, &self.record)
            .map_err(|e| marker_error(&self.durable, "write", &e))
    }
}

/// Opens the office's store on the engine the configuration names. The projection is
/// made, or rebuilt from the files and the journal, as it needs to be. The last stop's
/// clean-stop marker is consumed: [`OfficeStore::stopped_cleanly`] reports it.
pub fn open_store(config: &StoreConfig) -> Result<OfficeStore, StoreError> {
    match config.engine {
        EngineChoice::Sqlite => Ok(OfficeStore(OfficeStoreInner::Sqlite(open_on(
            config,
            sqlite_engine(config)?,
        )?))),
        #[cfg(any(test, feature = "testkit"))]
        EngineChoice::Memory => Ok(OfficeStore(OfficeStoreInner::Memory(open_on(
            config,
            memory_engine(),
        )?))),
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

    /// Why the store stopped accepting (a failed flush, append or projection), if it has.
    /// It accepts nothing more in this process.
    pub fn fatal(&self) -> Option<StoreError> {
        self.status().stopped
    }

    /// Whether the stop before this opening was orderly: [`OfficeStore::close`] left its
    /// marker, for this journal incarnation, at the head the store opened with. A kill, a
    /// drop, a fatal store or a failed close leaves none.
    pub fn stopped_cleanly(&self) -> bool {
        on_store!(self, opened => opened.stopped_cleanly)
    }

    /// Returns when every accepted entry is flushed, confirmed and written to its files.
    pub fn sync(&self) -> Result<(), StoreError> {
        on_store!(self, opened => opened.store.sync())
    }

    /// The orderly stop. The SQLite projection's log is then folded into its file, and the
    /// clean-stop marker is written last. A store that is dropped instead does no such
    /// work and loses nothing by it, and the next opening is told the stop was not clean.
    pub fn close(self) -> Result<(), StoreError> {
        let marker = match self.0 {
            OfficeStoreInner::Sqlite(opened) => {
                let (engine, marker) = opened.close_store()?;
                engine.checkpoint()?;
                marker
            }
            #[cfg(any(test, feature = "testkit"))]
            OfficeStoreInner::Memory(opened) => opened.close_store()?.1,
        };
        marker.write()
    }

    /// The digest of the DDL the SQLite projection is made from.
    #[cfg(test)]
    pub(crate) fn schema_digest(&self) -> Option<String> {
        match &self.0 {
            OfficeStoreInner::Sqlite(opened) => Some(opened.engine.schema_digest()),
            OfficeStoreInner::Memory(_) => None,
        }
    }

    /// Every row of the projection, to compare with another of the same engine.
    #[cfg(any(test, feature = "testkit"))]
    pub fn dump(&self) -> Vec<String> {
        use org_store::testkit::Dump;
        on_store!(self, opened => opened.engine.dump())
    }
}

fn apply_imported_to_engine(
    engine: &SqliteEngine,
    organization_id: &Id,
    files: &std::collections::BTreeMap<org_store::DurablePath, Vec<u8>>,
) -> Result<(), crate::organization::import::ImportError> {
    use crate::organization::import::ImportError;
    use org_store::{CorePort, Engine, ProjectionError, Sha256};

    let org_proj = OrganizationFiles::new(organization_id.clone());
    let proj_proj = ProjectFiles::new(organization_id.clone());
    engine
        .replace(&mut |tx| {
            for (path, bytes) in files {
                let owned_org =
                    <OrganizationFiles as Projector<SqliteEngine>>::owns(&org_proj, path);
                let owned_proj = <ProjectFiles as Projector<SqliteEngine>>::owns(&proj_proj, path);
                match (owned_org, owned_proj) {
                    (true, false) => <OrganizationFiles as Projector<SqliteEngine>>::apply(
                        &org_proj,
                        tx,
                        path,
                        Some(bytes),
                    )?,
                    (false, true) => <ProjectFiles as Projector<SqliteEngine>>::apply(
                        &proj_proj,
                        tx,
                        path,
                        Some(bytes),
                    )?,
                    (false, false) => {
                        return Err(ProjectionError::new(
                            path,
                            "unrecognized file in organization state",
                        )
                        .into());
                    }
                    (true, true) => {
                        return Err(ProjectionError::new(
                            path,
                            "ambiguous file owner in organization state",
                        )
                        .into());
                    }
                }
                tx.set_source(path, Some(Sha256::of(bytes)))?;
            }
            <OrganizationFiles as Projector<SqliteEngine>>::validate(&org_proj, tx)?;
            <ProjectFiles as Projector<SqliteEngine>>::validate(&proj_proj, tx)?;
            Ok(org_store::Cursor {
                ordinal: 0,
                sha256: Sha256::of(b"import-validation"),
                feed: org_store::FeedPosition(0),
                high_waters: org_store::HighWaters::default(),
            })
        })
        .map_err(|err| match err {
            StoreError::Projection(p) => ImportError::InvalidFile {
                path: p
                    .path
                    .map(|dp| dp.as_str().to_owned())
                    .unwrap_or_else(|| "state/organization.yaml".into()),
                reason: p.reason,
            },
            other => ImportError::Storage(other.to_string()),
        })
}

/// Validates a complete map of imported `state/` files against the production `OrganizationFiles`
/// and `ProjectFiles` projectors and SQLite constraints using a disposable private projection
/// database (`TASK-39` / R1). Any invalid or unrecognized file fails closed with its exact path
/// and reason.
pub(crate) fn validate_imported_files(
    organization_id: &Id,
    files: &std::collections::BTreeMap<org_store::DurablePath, Vec<u8>>,
) -> Result<crate::organization::import::ValidatedImport, crate::organization::import::ImportError>
{
    use crate::organization::import::{
        ImportError, ImportedPerson, PrivateTempDir, ValidatedImport,
    };
    use crate::organization::ports::OrganizationRead;
    use crate::organization::records::{EmployeeKind, EmployeeStatus, OfficeRole, TeamStatus};
    use org_store::Engine;

    let dir = PrivateTempDir::create("org-validate")?;
    let engine = SqliteEngine::open(&dir.path().join("validate.db"))
        .map_err(|e| ImportError::Storage(e.to_string()))?
        .for_organization(organization_id);
    apply_imported_to_engine(&engine, organization_id, files)?;

    let snap = engine
        .snapshot()
        .map_err(|e| ImportError::Storage(e.to_string()))?;
    let org = snap
        .organization()
        .map_err(|e| ImportError::Storage(e.to_string()))?
        .ok_or_else(|| ImportError::InvalidFile {
            path: "state/organization.yaml".into(),
            reason: "the organization record is missing".into(),
        })?;
    let repositories = snap
        .repositories()
        .map_err(|e| ImportError::Storage(e.to_string()))?;
    let canonical_remote_url = repositories
        .iter()
        .find(|r| r.id == org.organization_repository_id)
        .and_then(|r| r.remote_url.clone());
    let offices = snap
        .offices()
        .map_err(|e| ImportError::Storage(e.to_string()))?;
    let hq = offices
        .iter()
        .find(|o| o.role == OfficeRole::Hq)
        .ok_or_else(|| ImportError::InvalidFile {
            path: "state/organization.yaml".into(),
            reason: "no HQ office record found".into(),
        })?;
    let teams = snap
        .teams()
        .map_err(|e| ImportError::Storage(e.to_string()))?;
    let root_team = teams.iter().find(|t| t.parent_team_id.is_none());
    let root_team_id = root_team.map(|t| t.id.clone());
    let has_active_root_team = root_team.is_some_and(|t| t.status == TeamStatus::Active);
    let employees = snap
        .employees()
        .map_err(|e| ImportError::Storage(e.to_string()))?;
    let roles = snap
        .roles()
        .map_err(|e| ImportError::Storage(e.to_string()))?;
    let projects = snap
        .projects()
        .map_err(|e| ImportError::Storage(e.to_string()))?;
    let mut reserved_handles: Vec<String> = employees.iter().map(|e| e.handle.clone()).collect();
    reserved_handles.sort();
    reserved_handles.dedup();
    let mut reserved_entity_ids = std::collections::BTreeSet::new();
    reserved_entity_ids.insert(org.id.as_str().to_owned());
    for r in &repositories {
        reserved_entity_ids.insert(r.id.as_str().to_owned());
    }
    for o in &offices {
        reserved_entity_ids.insert(o.id.as_str().to_owned());
    }
    for t in &teams {
        reserved_entity_ids.insert(t.id.as_str().to_owned());
    }
    for e in &employees {
        reserved_entity_ids.insert(e.id.as_str().to_owned());
    }
    for r in &roles {
        reserved_entity_ids.insert(r.id.as_str().to_owned());
    }
    for p in &projects {
        reserved_entity_ids.insert(p.id.as_str().to_owned());
    }
    let mut people: Vec<ImportedPerson> = employees
        .iter()
        .filter(|e| e.kind == EmployeeKind::Person && e.status == EmployeeStatus::Active)
        .map(|e| ImportedPerson {
            id: e.id.as_str().to_owned(),
            name: e.name.clone(),
            handle: e.handle.clone(),
            home_team_id: e.home_team_id.as_str().to_owned(),
        })
        .collect();
    people.sort_by(|a, b| a.name.cmp(&b.name).then(a.handle.cmp(&b.handle)));
    let agents_count = employees
        .iter()
        .filter(|e| e.kind == EmployeeKind::Agent)
        .count();
    Ok(ValidatedImport {
        organization_id: org.id,
        name: org.name,
        slug: org.slug,
        canonical_remote_url,
        hq_office_id: hq.id.clone(),
        hq_office_name: hq.name.clone(),
        root_team_id,
        has_active_root_team,
        teams_count: teams.len(),
        projects_count: projects.len(),
        agents_count,
        people,
        reserved_handles,
        reserved_entity_ids: reserved_entity_ids.into_iter().collect(),
    })
}
