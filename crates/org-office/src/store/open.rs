//! Opening an office's store (ADR-0018, ADR-0021, `docs/design/office-store.md`): the one
//! place that dispatches across storage engines. Everything above it holds an [`OfficeStore`]
//! and passes it operations; nothing above it is generic over the engine, and `org-office`
//! itself holds zero database drivers or SQL (`TASK-255`, ENG-1).
//!
//! - Adding an operation: one `StoreOp` in `open/operations.rs`; this module and the
//!   engines do not change.
//! - Adding a family: one bound on [`OfficeEngine`] and one projector in `open_on`
//!   (`open/office_store.rs`).
//! - Plugging in a concrete engine: implement [`crate::OfficeStorageBackend`] /
//!   [`crate::OfficeEngineAdapter`] in an external adapter crate such as `org-office-sqlite`.

pub mod attachment_ops;
mod companions;
pub mod dod_ops;
pub mod git_ops;
mod import_validation;
pub mod messaging_ops;
mod office_store;
mod operations;
pub mod process_ops;
pub mod project_doc_ops;
pub mod review_ops;
pub mod search_ops;
pub mod task_ops;
#[cfg(test)]
mod tests;

use std::{path::PathBuf, sync::Arc, time::Duration};

use org_store::{Clock, OfficeIdentity, Opening, Store, StoreError, WriterAuthority};

#[cfg(any(test, feature = "testkit"))]
pub use companions::{
    open_accounting, open_attachments, open_budgets, open_private_notes, open_providers,
    open_replication, ReplicationOpenSpec,
};
#[cfg(not(any(test, feature = "testkit")))]
pub(crate) use companions::{
    open_accounting, open_attachments, open_budgets, open_private_notes, open_providers,
    open_replication, ReplicationOpenSpec,
};
pub use dod_ops::{
    OrganizationDefaultDodRead, SetOrganizationDefaultDodOp, SetProjectDefaultDodOp,
};
pub use git_ops::{
    RecordTaskRepoBranchOp, RepositoriesList, RepositoryResolve, TaskGitContext, TaskGitContextRead,
};
pub(crate) use import_validation::validate_imported_files;
pub use messaging_ops::{
    ChannelCatchupRead, ChannelCreateOp, ChannelMembershipOp, ChannelMessagesForCallerRead,
    ChannelMessagesRawRead, ChatAnswerOp, ChatInboxRead, ChatMarkReadOp, ChatMessagesRead,
    ChatMuteOp, ChatReactOp, ChatReplyOp, ChatSendOp, ClaimMailboxReadOp, CommitMailboxMessageOp,
    ConversationByIdRead, DeliverAnswerCandidatesOp, DmOpenOp, FindReplyForAskRead,
    GetMailboxMessageRead, InspectAskStatusRead, ListActiveSnoozeDeadlinesRead, ListChannelsRead,
    ListConversationsRead, ListMailboxHeadersRead, MailboxCheckNotifyOp, MailboxDeferOp,
    MailboxUnsnoozeOp, MailboxUpdateStateOp, MsgAcceptOp, MsgAskOp, MsgWithdrawOp,
    OpenOwnerInboxRead, OrgChangesAfterRead, PeekUnreadRead,
};
#[cfg(any(test, feature = "testkit"))]
pub use office_store::memory_engine;
pub use office_store::{open_store, open_store_with_adapter, OfficeStore};
pub use operations::{
    AddPersonOp, AddProjectTeamOp, AgentExecutionById, ApplyReplicatedFilesOp, ArchiveTeamOp,
    AttachProjectRepositoryOp, CheckHrMessageAccess, CheckOp, CreateOrganizationOp,
    CreateProjectOp, CreateTeamOp, DeactivatePersonOp, DirectoryGeneration, DirectorySnapshot,
    EmployeeById, EmployeesList, HireInspectById, HiresList, ImportOrganizationOp,
    ModelProfileById, ModelProfilesList, MovePersonOp, MoveTeamOp, OfficeIdsList, OfficeStanding,
    OfficesCount, OfficesList, ProjectCheckOp, ProjectInspectByRef, ProjectsList,
    ProviderConnectionById, ProviderConnectionsList, ReceiptById, RegisterRepositoryOp,
    RegisterSatelliteOfficeOp, RemoveProjectTeamOp, RoleById, RoleInspectByRef, RolesList,
    Standing, StructureOp, TopTeamId, UpdatePersonOp, UpdateTeamOp, WatchingModelRead,
};

use super::backend::OfficeStorageBackend;
use crate::attachments::ports::{AttachmentsEngine, AttachmentsRead};
use crate::project_docs::ports::{ProjectDocsEngine, ProjectDocsRead};
use crate::reviews::ports::{ReviewsEngine, ReviewsRead};
use crate::search::ports::{SearchEngine, SearchRead};
use crate::tasks::ports::{TasksEngine, TasksRead};

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

/// An engine every family of the office runs on. It grows by one bound per family (and one
/// for search, which reads them all).
pub trait OfficeEngine:
    TasksEngine + AttachmentsEngine + ProjectDocsEngine + ReviewsEngine + SearchEngine
{
}

impl<E> OfficeEngine for E where
    E: TasksEngine + AttachmentsEngine + ProjectDocsEngine + ReviewsEngine + SearchEngine
{
}

/// One operation on the store, whichever engine holds the projection.
pub trait StoreOp {
    type Out;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out;
}

/// One read of a consistent snapshot, whichever engine holds the projection.
pub trait ReadOp {
    type Out;
    fn read<S: TasksRead + AttachmentsRead + ProjectDocsRead + ReviewsRead + SearchRead>(
        self,
        snapshot: &S,
    ) -> Result<Self::Out, StoreError>;
}
