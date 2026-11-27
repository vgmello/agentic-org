//! The open [`OfficeStore`]: one arm per engine, opening with every family's projector,
//! the clean-stop marker, the orderly close, and the companion-store prefixes the
//! journal never owns.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use org_store::{
    Ack, Change, FeedPosition, Files, FilesError, Id, Projector, Status, Store, StoreError,
    StoreParts,
};

use super::super::erased::{DynOfficeEngine, OfficeEngineAdapter};
use super::{EngineChoice, OfficeEngine, ReadOp, StoreConfig, StoreOp};
use crate::{
    attachments::projector::AttachmentFiles,
    messaging::projector::MessagingFiles,
    office_local::{clean_stop, projector::OfficeLocalFiles, records::CleanStop},
    organization::projector::OrganizationFiles,
    processes::projector::ProcessFiles,
    project_docs::projector::ProjectDocFiles,
    projects::projector::ProjectFiles,
    reviews::files::ReviewFiles,
    tasks::{features::TaskNumbering, ports::TasksRead, projector::TaskFiles, records},
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
    /// The office's task numbering (`ADR-0019`): when the last stop was not, task numbers up
    /// to the counter's reservation may have been issued and lost, so the next task create
    /// jumps past it, or a clean stop that comes first spends them.
    task_numbering: Arc<TaskNumbering>,
}

enum OfficeStoreInner {
    Adapter(Opened<DynOfficeEngine>),
    #[cfg(any(test, feature = "testkit"))]
    Memory(Opened<org_store::testkit::MemoryEngine>),
}

/// An office's open store. Engine variants stay private so callers cannot bypass [`open_store`].
pub struct OfficeStore(OfficeStoreInner);

/// The same expression on whichever arm is open.
macro_rules! on_store {
    ($office:expr, $opened:ident => $body:expr) => {
        match &$office.0 {
            OfficeStoreInner::Adapter($opened) => $body,
            #[cfg(any(test, feature = "testkit"))]
            OfficeStoreInner::Memory($opened) => $body,
        }
    };
}

/// The memory projection: always empty when opened, so the store rebuilds it.
#[cfg(any(test, feature = "testkit"))]
pub fn memory_engine() -> org_store::testkit::MemoryEngine {
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
    let tasks: Arc<dyn Projector<E>> =
        Arc::new(TaskFiles::new(organization.clone(), here.office_id.clone()));
    let processes: Arc<dyn Projector<E>> = Arc::new(ProcessFiles::new());
    let attachments: Arc<dyn Projector<E>> = Arc::new(AttachmentFiles::new());
    let docs: Arc<dyn Projector<E>> = Arc::new(ProjectDocFiles);
    let reviews: Arc<dyn Projector<E>> = Arc::new(ReviewFiles::new(here.office_id.clone()));
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
        attachments,
        docs,
        reviews,
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
    #[cfg(any(test, feature = "testkit"))]
    if let Some(flush_interval) = config.testing.flush_interval {
        parts.flush_interval = flush_interval;
    }
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
    let (counter, _) = store.read(office_counter)?;
    let task_numbering = Arc::new(TaskNumbering::opened(counter, stopped_cleanly));
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
        task_numbering,
    })
}

/// This office's task counter: the projection holds this office's alone.
fn office_counter<S: TasksRead>(
    snapshot: &S,
) -> Result<Option<records::OfficeTaskCounterRecord>, StoreError> {
    Ok(snapshot.office_task_counters()?.into_iter().next())
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
        self.settle_task_counter()?;
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

    /// After a stop that was not clean and no task create since, the numbers up to the
    /// counter's reservation may have been issued and lost: they are spent, durably, before
    /// the clean-stop marker says the next opening may trust the counter (`ADR-0019`
    /// Decision 7). The caller owns the store, so nothing changes the counter meanwhile.
    fn settle_task_counter(&self) -> Result<(), StoreError> {
        let (counter, _) = self.store.read(office_counter)?;
        let Some(settled) = counter.and_then(|c| self.task_numbering.settled(&c)) else {
            return Ok(());
        };
        let path = records::counter_path(&self.office);
        let bytes = records::encode_counter(&self.office, &settled);
        self.store.submit(None, true, move |_| {
            Ok(Change::new(Ack::Durable).put(path.clone(), bytes.clone()))
        })?;
        self.store.sync()
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
    match &config.engine {
        EngineChoice::Sqlite(backend) => {
            let adapter = backend.open_projection(config)?;
            open_store_with_adapter(config, adapter)
        }
        #[cfg(any(test, feature = "testkit"))]
        EngineChoice::Memory => Ok(OfficeStore(OfficeStoreInner::Memory(open_on(
            config,
            memory_engine(),
        )?))),
    }
}

/// Opens the office's store directly over an injected [`OfficeEngineAdapter`].
pub fn open_store_with_adapter(
    config: &StoreConfig,
    adapter: Arc<dyn OfficeEngineAdapter>,
) -> Result<OfficeStore, StoreError> {
    Ok(OfficeStore(OfficeStoreInner::Adapter(open_on(
        config,
        DynOfficeEngine::new(adapter),
    )?)))
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

    /// The office's task numbering (`ADR-0019`), shared with task creation: whether numbers
    /// up to the counter's `reserved_through` may have been issued by a run that did not
    /// stop cleanly, and how far a reservation is known flushed.
    pub fn task_numbering(&self) -> Arc<TaskNumbering> {
        on_store!(self, opened => Arc::clone(&opened.task_numbering))
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

    /// The orderly stop. The projection engine's close upkeep (such as folding a SQLite WAL)
    /// runs after the store closes, and the clean-stop marker is written last. A store that is
    /// dropped instead does no such work and loses nothing by it, and the next opening is told
    /// the stop was not clean.
    pub fn close(self) -> Result<(), StoreError> {
        let marker = match self.0 {
            OfficeStoreInner::Adapter(opened) => {
                let (engine, marker) = opened.close_store()?;
                engine.checkpoint()?;
                marker
            }
            #[cfg(any(test, feature = "testkit"))]
            OfficeStoreInner::Memory(opened) => opened.close_store()?.1,
        };
        marker.write()
    }

    /// The digest of the DDL the projection is made from, when the engine tracks one.
    #[cfg(any(test, feature = "testkit"))]
    pub fn schema_digest(&self) -> Option<String> {
        match &self.0 {
            OfficeStoreInner::Adapter(opened) => opened.engine.schema_digest(),
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

/// The companion stores of the office ([`crate::companion_paths`]): written beside the journal,
/// never materialized from it.
fn companion_store_exclusions(office_id: &Id) -> Vec<org_store::DurablePath> {
    crate::companion_paths::companion_store_paths(office_id.as_str())
        .into_iter()
        .map(|c| org_store::DurablePath::parse(&c.path).expect("valid companion store prefix"))
        .collect()
}
