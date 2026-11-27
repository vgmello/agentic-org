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
pub mod delivery_ops;
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

use org_store::{Store, StoreError};

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
    Standing, StructureOp, TeamLimitRequestsList, TopTeamId, UpdatePersonOp, UpdateTeamOp,
    WatchingModelRead,
};

#[cfg(any(test, feature = "testkit"))]
pub use super::backend::Testing;
pub use super::backend::{EngineChoice, StoreConfig, PROJECTION_FILE};
use crate::attachments::ports::{AttachmentsEngine, AttachmentsRead};
use crate::delivery::ports::{DeliveryEngine, DeliveryRead};
use crate::project_docs::ports::{ProjectDocsEngine, ProjectDocsRead};
use crate::reviews::ports::{ReviewsEngine, ReviewsRead};
use crate::search::ports::{SearchEngine, SearchRead};
use crate::tasks::ports::{TasksEngine, TasksRead};

/// An engine every family of the office runs on. It grows by one bound per family (and one
/// for search, which reads them all).
pub trait OfficeEngine:
    TasksEngine + AttachmentsEngine + ProjectDocsEngine + DeliveryEngine + ReviewsEngine + SearchEngine
{
}

impl<E> OfficeEngine for E where
    E: TasksEngine
        + AttachmentsEngine
        + ProjectDocsEngine
        + DeliveryEngine
        + ReviewsEngine
        + SearchEngine
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
    fn read<
        S: TasksRead + AttachmentsRead + ProjectDocsRead + DeliveryRead + ReviewsRead + SearchRead,
    >(
        self,
        snapshot: &S,
    ) -> Result<Self::Out, StoreError>;
}
