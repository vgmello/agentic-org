//! The open [`OfficeStore`]: one arm per engine, opening with every family's projector,
//! the clean-stop marker, the orderly close, and the companion-store prefixes the
//! journal never owns.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use org_store::{
    EngineError, FeedPosition, Files, FilesError, Id, Projector, Status, Store, StoreError,
    StoreParts,
};

use super::super::sqlite::SqliteEngine;
use super::{EngineChoice, OfficeEngine, ReadOp, StoreConfig, StoreOp};
use crate::{
    messaging::projector::MessagingFiles,
    office_local::{clean_stop, projector::OfficeLocalFiles, records::CleanStop},
    organization::projector::OrganizationFiles,
    processes::projector::ProcessFiles,
    projects::projector::ProjectFiles,
    tasks::projector::TaskFiles,
};

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
pub(super) fn sqlite_engine(config: &StoreConfig) -> Result<SqliteEngine, StoreError> {
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
pub(super) fn memory_engine() -> org_store::testkit::MemoryEngine {
    org_store::testkit::MemoryEngine::new()
}

/// The store on `engine`, with every family's projector.
fn open_on<E: OfficeEngine>(config: &StoreConfig, engine: E) -> Result<Opened<E>, StoreError> {
    let engine = Arc::new(engine);
    let here = &config.identity;
    let organization = here.organization_id.clone();
    let files: Arc<dyn Projector<E>> = Arc::new(OrganizationFiles::new(organization.clone()));
    let projects: Arc<dyn Projector<E>> = Arc::new(ProjectFiles::new(organization.clone()));
    let messaging: Arc<dyn Projector<E>> = Arc::new(MessagingFiles::new(organization.clone()));
    let tasks: Arc<dyn Projector<E>> = Arc::new(TaskFiles::new());
    let processes: Arc<dyn Projector<E>> = Arc::new(ProcessFiles::new());
    let local = OfficeLocalFiles::new(organization, here.office_id.clone());
    #[cfg_attr(
        not(any(test, feature = "testkit")),
        allow(
            unused_mut,
            reason = "test and testkit builds push optional probe projectors below"
        )
    )]
    let mut projectors: Vec<Arc<dyn Projector<E>>> = vec![
        files,
        projects,
        messaging,
        tasks,
        processes,
        Arc::new(local),
    ];
    #[cfg(any(test, feature = "testkit"))]
    if let Some(probe) = &config.testing.probe {
        projectors.push(probe.projector());
    }
    let mut parts = StoreParts::new(
        config.durable_root.clone(),
        config.identity.clone(),
        Arc::clone(&config.custody),
        Arc::clone(&engine),
        projectors,
        Arc::clone(&config.clock),
    );
    parts.excluded_prefixes = companion_store_exclusions(&here.office_id);
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

    /// Asks the store to flush its buffered entries now and returns at once (`TASK-140`).
    pub fn request_flush(&self) {
        on_store!(self, opened => opened.store.request_flush())
    }

    /// Registers a durable journal consumer watermark (`ARCH-03` / `TASK-52`) so segment
    /// compaction never discards entries this consumer has not yet acknowledged.
    pub fn register_consumer(&self, consumer_id: &str) -> Result<u64, StoreError> {
        on_store!(self, opened => opened.store.register_consumer(consumer_id))
    }

    /// Advances a durable journal consumer watermark (`ARCH-03` / `TASK-52`).
    pub fn advance_consumer(&self, consumer_id: &str, ordinal: u64) -> Result<(), StoreError> {
        on_store!(self, opened => opened.store.advance_consumer(consumer_id, ordinal))
    }

    /// Returns the persisted watermark of `consumer_id`, if registered.
    pub fn consumer_watermark(&self, consumer_id: &str) -> Option<u64> {
        on_store!(self, opened => opened.store.consumer_watermark(consumer_id))
    }

    /// Reads all retained journal entries strictly after `ordinal` (`ARCH-03` / `TASK-52`).
    pub fn entries_after(&self, ordinal: u64) -> Vec<Arc<org_store::Entry>> {
        on_store!(self, opened => opened.store.entries_after(ordinal))
    }

    /// Reads retained journal entries strictly after `ordinal`, rejecting any gap (`TASK-52` M-15).
    pub fn entries_after_contiguous(
        &self,
        ordinal: u64,
    ) -> Result<Vec<Arc<org_store::Entry>>, StoreError> {
        on_store!(self, opened => opened.store.entries_after_contiguous(ordinal))
    }

    /// Syncs and runs `f(materialized_ordinal)` while holding the store's materialization lock so
    /// files under `durable/state/` cannot change during snapshot capture (`TASK-52` H-11).
    pub fn with_consistent_materialized<R>(
        &self,
        f: impl FnOnce(u64) -> Result<R, StoreError>,
    ) -> Result<R, StoreError> {
        on_store!(self, opened => opened.store.with_consistent_materialized(f))
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

fn companion_store_exclusions(office_id: &Id) -> Vec<org_store::DurablePath> {
    [
        "authority/agent-execution".to_owned(),
        "accounting".to_owned(),
        "budgets".to_owned(),
        format!("local/offices/{office_id}/connections"),
        format!("local/offices/{office_id}/auth"),
        format!("local/offices/{office_id}/subscription-usage"),
        format!("local/offices/{office_id}/agents"),
        format!("local/offices/{office_id}/sessions"),
        format!("local/offices/{office_id}/enroll-tmp"),
        format!("local/offices/{office_id}/replication"),
        format!("local/offices/{office_id}/hq-owner.v1.json"),
        format!("local/offices/{office_id}/watcher"),
        format!("local/offices/{office_id}/repositories"),
        format!("local/offices/{office_id}/close-recovery"),
        format!("local/offices/{office_id}/local-execution"),
        format!("local/offices/{office_id}/invitations"),
        format!("local/offices/{office_id}/tls"),
        format!("local/offices/{office_id}/security/git-https-helper"),
        format!("local/offices/{office_id}/security/git-ssh-key"),
        format!("local/offices/{office_id}/security/git-known-hosts"),
    ]
    .into_iter()
    .map(|p| org_store::DurablePath::parse(&p).expect("valid companion store prefix"))
    .collect()
}
