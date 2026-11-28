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

use org_proto::{ApiError, ErrorCode};
use org_store::{
    Actor, Change, Id, Operation, Outcome, Refusal, Sha256, Store, StoreError, UnitOfWork,
};
use serde::{de::DeserializeOwned, Serialize};

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

pub(super) fn build_operation<P: Serialize>(
    kind: &'static str,
    command_id: &str,
    caller: &str,
    params: &P,
) -> Result<Operation, ApiError> {
    let id = org_proto::is_uuid_v7(command_id)
        .then(|| Id::parse(command_id).ok())
        .flatten()
        .ok_or_else(|| {
            ApiError::new(ErrorCode::InvalidRequest, "command_id: a lowercase UUIDv7")
        })?;
    let fields = serde_json::to_vec(params).expect("operation params encode");
    Ok(Operation {
        id,
        kind,
        actor: Id::parse(caller).map_or(Actor::System, Actor::Employee),
        fingerprint: Sha256::of_parts(&[kind.as_bytes(), b"\0", &fields]),
    })
}

pub(super) fn submit_api_op<E, T, F>(
    store: &Store<E>,
    op: Operation,
    label: &str,
    decode_err: fn(&[u8]) -> ApiError,
    decide: F,
) -> Result<Result<T, ApiError>, StoreError>
where
    E: OfficeEngine,
    T: DeserializeOwned,
    F: FnMut(&mut UnitOfWork<'_, E>) -> Result<Change, Refusal> + Send + 'static,
{
    let reply = match store.submit(Some(op), false, decide) {
        Ok(reply) => reply,
        Err(StoreError::Conflict) => {
            return Ok(Err(ApiError::new(
                ErrorCode::OperationReused,
                "command_id reused with a different request",
            )))
        }
        Err(error) => return Err(error),
    };
    if reply.outcome == Outcome::Rejected {
        return Ok(Err(decode_err(&reply.bytes)));
    }
    serde_json::from_slice(&reply.bytes)
        .map(Ok)
        .map_err(|e| StoreError::Invalid(format!("corrupt {label} reply: {e}")))
}
