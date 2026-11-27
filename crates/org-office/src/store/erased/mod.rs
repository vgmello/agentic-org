//! Type-erased office store engine seam (`TASK-255`, `ADR-0018`).
//!
//! Keeps `org-office` independent of concrete external storage engine crates such as
//! `org-office-sqlite`: external engines implement [`OfficeEngineAdapter`] (returning
//! [`BoxedTx`] and [`BoxedSnapshot`]) and are mounted inside [`DynOfficeEngine`], which
//! satisfies `org_store::Engine` and `OfficeEngine` without introducing a crate cycle.

mod delegate;

use std::any::Any;
use std::sync::Arc;

use org_store::{CorePort, Cursor, Engine, EngineError, StoreError};

use crate::{
    attachments::ports::{AttachmentsRead, AttachmentsWrite},
    project_docs::ports::{ProjectDocsRead, ProjectDocsWrite},
    reviews::ports::{ReviewsRead, ReviewsWrite},
    search::ports::SearchRead,
    tasks::ports::{TasksRead, TasksWrite},
};

/// Object-safe super-trait covering every read port of an office store snapshot.
pub trait OfficeRead:
    TasksRead + AttachmentsRead + ProjectDocsRead + ReviewsRead + SearchRead
{
}

impl<T> OfficeRead for T where
    T: TasksRead + AttachmentsRead + ProjectDocsRead + ReviewsRead + SearchRead + ?Sized
{
}

/// Object-safe super-trait covering every read and write port of an office store transaction.
pub trait OfficeWrite:
    CorePort + TasksWrite + AttachmentsWrite + ProjectDocsWrite + ReviewsWrite + Send
{
}

impl<T> OfficeWrite for T where
    T: CorePort + TasksWrite + AttachmentsWrite + ProjectDocsWrite + ReviewsWrite + Send + ?Sized
{
}

trait OfficeWriteAny: OfficeWrite + Any {
    fn into_any(self: Box<Self>) -> Box<dyn Any>;
}

impl<T: OfficeWrite + 'static> OfficeWriteAny for T {
    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

/// Type-erased open write transaction for an [`OfficeEngineAdapter`].
pub struct BoxedTx(Box<dyn OfficeWriteAny>);

impl BoxedTx {
    /// Wraps a concrete engine transaction.
    pub fn new<T: OfficeWrite + 'static>(tx: T) -> Self {
        Self(Box::new(tx))
    }

    /// Recovers the concrete transaction type previously wrapped with [`Self::new`].
    pub fn into_inner<T: 'static>(self) -> Result<T, EngineError> {
        self.0
            .into_any()
            .downcast::<T>()
            .map(|boxed| *boxed)
            .map_err(|_| EngineError::new("transaction type mismatch for storage engine adapter"))
    }
}

/// Type-erased read snapshot for an [`OfficeEngineAdapter`].
pub struct BoxedSnapshot(Box<dyn OfficeRead>);

impl BoxedSnapshot {
    /// Wraps a concrete engine snapshot.
    pub fn new<T: OfficeRead + 'static>(snapshot: T) -> Self {
        Self(Box::new(snapshot))
    }
}

/// Object-safe engine adapter contract implemented by external storage crates (for example
/// `org-office-sqlite`) and hosted by [`DynOfficeEngine`].
pub trait OfficeEngineAdapter: Send + Sync + 'static {
    fn begin(&self) -> Result<BoxedTx, EngineError>;
    fn commit(&self, tx: BoxedTx, cursor: &Cursor) -> Result<(), EngineError>;
    fn snapshot(&self) -> Result<BoxedSnapshot, EngineError>;
    fn replace(
        &self,
        build: &mut dyn FnMut(&mut BoxedTx) -> Result<Cursor, StoreError>,
    ) -> Result<(), StoreError>;

    fn after_round(&self) {}

    fn upkeep_error(&self) -> Option<EngineError> {
        None
    }

    /// Orderly close upkeep (for example folding a WAL into the database file).
    fn checkpoint(&self) -> Result<(), EngineError> {
        Ok(())
    }

    /// Deterministic row dump for cross-engine conformance and rebuild parity tests.
    #[cfg(any(test, feature = "testkit"))]
    fn dump(&self) -> Vec<String> {
        Vec::new()
    }

    /// Schema digest of the projection, if the engine tracks one.
    #[cfg(any(test, feature = "testkit"))]
    fn schema_digest(&self) -> Option<String> {
        None
    }
}

/// `org_store::Engine` wrapper over an [`OfficeEngineAdapter`].
#[derive(Clone)]
pub struct DynOfficeEngine {
    inner: Arc<dyn OfficeEngineAdapter>,
}

impl DynOfficeEngine {
    pub fn new(inner: Arc<dyn OfficeEngineAdapter>) -> Self {
        Self { inner }
    }

    pub fn checkpoint(&self) -> Result<(), EngineError> {
        self.inner.checkpoint()
    }

    #[cfg(any(test, feature = "testkit"))]
    pub fn schema_digest(&self) -> Option<String> {
        self.inner.schema_digest()
    }
}

impl Engine for DynOfficeEngine {
    type Tx = BoxedTx;
    type Snapshot = BoxedSnapshot;

    fn begin(&self) -> Result<BoxedTx, EngineError> {
        self.inner.begin()
    }

    fn commit(&self, tx: BoxedTx, cursor: &Cursor) -> Result<(), EngineError> {
        self.inner.commit(tx, cursor)
    }

    fn snapshot(&self) -> Result<BoxedSnapshot, EngineError> {
        self.inner.snapshot()
    }

    fn replace(
        &self,
        build: &mut dyn FnMut(&mut BoxedTx) -> Result<Cursor, StoreError>,
    ) -> Result<(), StoreError> {
        self.inner.replace(build)
    }

    fn after_round(&self) {
        self.inner.after_round();
    }

    fn upkeep_error(&self) -> Option<EngineError> {
        self.inner.upkeep_error()
    }
}

#[cfg(any(test, feature = "testkit"))]
impl org_store::testkit::Dump for DynOfficeEngine {
    fn dump(&self) -> Vec<String> {
        self.inner.dump()
    }
}
