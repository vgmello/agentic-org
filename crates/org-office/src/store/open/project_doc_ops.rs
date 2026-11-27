//! Engine-independent `ReadOp` and `StoreOp` wrappers for the documentation tree (`TASK-55`):
//! reads run on one snapshot, writes are submitted as one operation each, keyed by the
//! caller's `command_id` (an exact retry is answered from its receipt; the same ID with other
//! params is `operation_reused`).

use org_proto::docs::{
    DocsCreateParams, DocsIndexParams, DocsIndexResult, DocsInspectParams, DocsInspectResult,
    DocsListParams, DocsListResult, DocsMoveParams, DocsPutMediaParams, DocsPutMediaResult,
    DocsUpdateParams, DocsWriteResult,
};
use org_proto::{ApiError, ErrorCode};
use org_store::{
    Actor, Change, Id, Operation, Outcome, Refusal, Sha256, Store, StoreError, UnitOfWork,
};
use serde::{de::DeserializeOwned, Serialize};

use crate::project_docs::access::Denied;
use crate::project_docs::features::{
    create, decode_rejection, derive_id, put_media, read, refusal, update,
};
use crate::project_docs::ports::ProjectDocsRead;

use super::{OfficeEngine, ReadOp, StoreOp};

type Answer<T> = Result<Result<T, ApiError>, StoreError>;

fn settle<T>(result: Result<T, Denied>) -> Answer<T> {
    match result {
        Ok(value) => Ok(Ok(value)),
        Err(Denied::Refused(error)) => Ok(Err(error)),
        Err(Denied::Storage(error)) => Err(error.into()),
    }
}

/// The store operation of one write: its `command_id`, its actor and a fingerprint of all its
/// params.
fn operation<P: Serialize>(
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
    let fields = serde_json::to_vec(params).expect("documentation params encode");
    Ok(Operation {
        id,
        kind,
        actor: Id::parse(caller).map_or(Actor::System, Actor::Employee),
        fingerprint: Sha256::of_parts(&[kind.as_bytes(), b"\0", &fields]),
    })
}

fn submit<E, T, F>(store: &Store<E>, op: Operation, decide: F) -> Answer<T>
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
        return Ok(Err(decode_rejection(&reply.bytes)));
    }
    serde_json::from_slice(&reply.bytes)
        .map(Ok)
        .map_err(|e| StoreError::Invalid(format!("corrupt documentation reply: {e}")))
}

/// `docs.index` on one snapshot.
pub struct DocsIndexRead {
    pub caller: String,
    pub params: DocsIndexParams,
}

impl ReadOp for DocsIndexRead {
    type Out = Result<DocsIndexResult, ApiError>;

    fn read<S: ProjectDocsRead>(self, snapshot: &S) -> Answer<DocsIndexResult> {
        settle(read::index(snapshot, &self.caller, &self.params))
    }
}

/// `docs.list` on one snapshot.
pub struct DocsListRead {
    pub caller: String,
    pub params: DocsListParams,
}

impl ReadOp for DocsListRead {
    type Out = Result<DocsListResult, ApiError>;

    fn read<S: ProjectDocsRead>(self, snapshot: &S) -> Answer<DocsListResult> {
        settle(read::list(snapshot, &self.caller, &self.params))
    }
}

/// `docs.inspect` on one snapshot.
pub struct DocsInspectRead {
    pub caller: String,
    pub params: DocsInspectParams,
}

impl ReadOp for DocsInspectRead {
    type Out = Result<DocsInspectResult, ApiError>;

    fn read<S: ProjectDocsRead>(self, snapshot: &S) -> Answer<DocsInspectResult> {
        settle(read::inspect(snapshot, &self.caller, &self.params))
    }
}

/// `docs.create`.
pub struct DocsCreateOp {
    pub caller: String,
    pub params: DocsCreateParams,
}

impl StoreOp for DocsCreateOp {
    type Out = Answer<DocsWriteResult>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let op = match operation(
            "docs.create",
            &self.params.command_id,
            &self.caller,
            &self.params,
        ) {
            Ok(op) => op,
            Err(error) => return Ok(Err(error)),
        };
        let Self { caller, params } = self;
        submit(store, op, move |uow| {
            let id = derive_id("project-doc", &params.command_id, uow.now().0);
            create::decide(uow.view(), uow.now(), &caller, &params, id).map_err(refusal)
        })
    }
}

/// `docs.update`.
pub struct DocsUpdateOp {
    pub caller: String,
    pub params: DocsUpdateParams,
}

impl StoreOp for DocsUpdateOp {
    type Out = Answer<DocsWriteResult>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let op = match operation(
            "docs.update",
            &self.params.command_id,
            &self.caller,
            &self.params,
        ) {
            Ok(op) => op,
            Err(error) => return Ok(Err(error)),
        };
        let Self { caller, params } = self;
        submit(store, op, move |uow| {
            update::decide_update(uow.view(), uow.now(), &caller, &params).map_err(refusal)
        })
    }
}

/// `docs.move`.
pub struct DocsMoveOp {
    pub caller: String,
    pub params: DocsMoveParams,
}

impl StoreOp for DocsMoveOp {
    type Out = Answer<DocsWriteResult>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let op = match operation(
            "docs.move",
            &self.params.command_id,
            &self.caller,
            &self.params,
        ) {
            Ok(op) => op,
            Err(error) => return Ok(Err(error)),
        };
        let Self { caller, params } = self;
        submit(store, op, move |uow| {
            update::decide_move(uow.view(), uow.now(), &caller, &params).map_err(refusal)
        })
    }
}

/// `docs.put_media`: the bytes are checked and sanitized before the write is submitted.
pub struct DocsPutMediaOp {
    pub caller: String,
    pub params: DocsPutMediaParams,
}

impl StoreOp for DocsPutMediaOp {
    type Out = Answer<DocsPutMediaResult>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let op = match operation(
            "docs.put_media",
            &self.params.command_id,
            &self.caller,
            &self.params,
        ) {
            Ok(op) => op,
            Err(error) => return Ok(Err(error)),
        };
        let prepared = match settle(put_media::prepare(&self.params))? {
            Ok(prepared) => prepared,
            Err(error) => return Ok(Err(error)),
        };
        let Self { caller, params } = self;
        submit(store, op, move |uow| {
            let id = derive_id("project-doc-media", &params.command_id, uow.now().0);
            put_media::decide(uow.view(), uow.now(), &caller, &params, &prepared, id)
                .map_err(refusal)
        })
    }
}
