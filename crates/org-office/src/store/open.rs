//! Opening an office's store (ADR-0018, ADR-0021, `docs/design/office-store.md`): the one
//! place that names an engine. Everything above it holds an [`OfficeStore`] and passes it
//! operations; nothing above it is generic over the engine, and nothing but this module
//! (`store/open.rs` and `store/open/`) and `store/sqlite/` knows which engine there is.
//!
//! Dispatch is static (design open point 1): [`OfficeStore`] is an enum with one arm per
//! engine, and an operation is a [`StoreOp`] or a [`ReadOp`], generic over the engine and
//! compiled once per arm.
//!
//! - Adding an operation: one `StoreOp` in `open/operations.rs`; this module and the
//!   engines do not change.
//! - Adding a family: one bound on [`OfficeEngine`] and one projector in `open_on`
//!   (`open/office_store.rs`).
//! - Adding an engine: one arm of [`EngineChoice`] and of [`OfficeStore`].

pub(crate) mod attachment_ops;
mod companions;
pub(crate) mod dod_ops;
pub(crate) mod git_ops;
mod import_validation;
pub(crate) mod messaging_ops;
mod office_store;
mod operations;
pub(crate) mod process_ops;
pub(crate) mod project_doc_ops;
pub(crate) mod search_ops;
pub(crate) mod task_ops;
#[cfg(test)]
mod tests;

use std::{path::PathBuf, sync::Arc, time::Duration};

use org_store::{Clock, OfficeIdentity, Opening, Store, StoreError, WriterAuthority};

pub(crate) use companions::{
    open_accounting, open_attachments, open_budgets, open_private_notes, open_providers,
    open_replication, ReplicationOpenSpec,
};
pub use dod_ops::{
    OrganizationDefaultDodRead, SetOrganizationDefaultDodOp, SetProjectDefaultDodOp,
};
pub use git_ops::{
    AttachProjectRepositoryOp, RecordTaskRepoBranchOp, RegisterRepositoryOp, RepositoriesList,
    RepositoryResolve, TaskGitContext, TaskGitContextRead,
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
pub use office_store::{open_store, OfficeStore};
pub use operations::{
    AddPersonOp, AddProjectTeamOp, AgentExecutionById, ApplyReplicatedFilesOp, ArchiveTeamOp,
    CheckHrMessageAccess, CheckOp, CreateOrganizationOp, CreateProjectOp, CreateTeamOp,
    DeactivatePersonOp, DirectoryGeneration, DirectorySnapshot, EmployeeById, EmployeesList,
    HireInspectById, HiresList, ImportOrganizationOp, ModelProfileById, ModelProfilesList,
    MovePersonOp, MoveTeamOp, OfficeIdsList, OfficeStanding, OfficesCount, OfficesList,
    ProjectCheckOp, ProjectInspectByRef, ProjectsList, ProviderConnectionById,
    ProviderConnectionsList, ReceiptById, RegisterSatelliteOfficeOp, RemoveProjectTeamOp, RoleById,
    RoleInspectByRef, RolesList, Standing, StructureOp, TopTeamId, UpdatePersonOp, UpdateTeamOp,
    WatchingModelRead,
};

use crate::attachments::ports::{AttachmentsEngine, AttachmentsRead};
use crate::project_docs::ports::{ProjectDocsEngine, ProjectDocsRead};
use crate::search::ports::{SearchEngine, SearchRead};
use crate::tasks::ports::{TasksEngine, TasksRead};

/// The file of an office's projection, under `<runtime-root>/<office-id>/`.
pub(crate) const PROJECTION_FILE: &str = "office.db";

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
    /// Override for [`StoreConfig::flush_interval`]: crash-point tests set an hour so the
    /// 50 ms background timer cannot fire mid-commit under load and race with writer crashes.
    #[cfg(test)]
    pub(crate) flush_interval: Option<Duration>,
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

/// An engine every family of the office runs on. It grows by one bound per family (and one
/// for search, which reads them all).
pub trait OfficeEngine: TasksEngine + AttachmentsEngine + ProjectDocsEngine + SearchEngine {}

impl<E> OfficeEngine for E where
    E: TasksEngine + AttachmentsEngine + ProjectDocsEngine + SearchEngine
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
    fn read<S: TasksRead + AttachmentsRead + ProjectDocsRead + SearchRead>(
        self,
        snapshot: &S,
    ) -> Result<Self::Out, StoreError>;
}
