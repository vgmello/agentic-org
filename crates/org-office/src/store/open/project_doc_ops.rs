//! Engine-independent `ReadOp` and `StoreOp` wrappers for the documentation tree (`TASK-55`):
//! reads run on one snapshot, writes are submitted as one operation each, keyed by the
//! caller's `command_id` (an exact retry is answered from its receipt; the same ID with other
//! params is `operation_reused`).

use org_proto::docs::{
    DocsCreateParams, DocsDeleteParams, DocsDeleteResult, DocsIndexParams, DocsIndexResult,
    DocsInspectParams, DocsInspectResult, DocsListParams, DocsListResult, DocsMoveParams,
    DocsPutMediaParams, DocsPutMediaResult, DocsUpdateParams, DocsWriteResult,
};
use org_proto::{ApiError, ErrorCode};
use org_store::{
    Actor, Change, Id, Operation, Outcome, Refusal, Sha256, Store, StoreError, UnitOfWork,
};
use serde::{de::DeserializeOwned, Serialize};

use crate::project_docs::access::{authorize, Denied, DocsAccess};
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

macro_rules! docs_read_op {
    ($name:ident, $params:ty, $res:ty, $func:path) => {
        pub struct $name {
            pub caller: String,
            pub params: $params,
        }
        impl ReadOp for $name {
            type Out = Result<$res, ApiError>;
            fn read<S: ProjectDocsRead>(self, snapshot: &S) -> Answer<$res> {
                settle($func(snapshot, &self.caller, &self.params))
            }
        }
    };
}

macro_rules! docs_write_op {
    ($name:ident, $method:literal, $params:ty, |$uow:ident, $caller:ident, $p:ident| $body:expr) => {
        pub struct $name {
            pub caller: String,
            pub params: $params,
        }
        impl StoreOp for $name {
            type Out = Answer<DocsWriteResult>;
            fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
                let op =
                    match operation($method, &self.params.command_id, &self.caller, &self.params) {
                        Ok(op) => op,
                        Err(error) => return Ok(Err(error)),
                    };
                let Self {
                    caller: $caller,
                    params: $p,
                } = self;
                submit(store, op, move |$uow| ($body).map_err(refusal))
            }
        }
    };
}

docs_read_op!(DocsIndexRead, DocsIndexParams, DocsIndexResult, read::index);
docs_read_op!(DocsListRead, DocsListParams, DocsListResult, read::list);
docs_read_op!(
    DocsInspectRead,
    DocsInspectParams,
    DocsInspectResult,
    read::inspect
);

docs_write_op!(
    DocsCreateOp,
    "docs.create",
    DocsCreateParams,
    |uow, caller, params| {
        let id = derive_id("project-doc", &params.command_id, uow.now().0);
        create::decide(uow.view(), uow.now(), &caller, &params, id)
    }
);
docs_write_op!(
    DocsUpdateOp,
    "docs.update",
    DocsUpdateParams,
    |uow, caller, params| { update::decide_update(uow.view(), uow.now(), &caller, &params) }
);
docs_write_op!(
    DocsMoveOp,
    "docs.move",
    DocsMoveParams,
    |uow, caller, params| { update::decide_move(uow.view(), uow.now(), &caller, &params) }
);

/// `docs.delete`: removes a project document and regenerates `docs/index.yaml`.
pub struct DocsDeleteOp {
    pub caller: String,
    pub params: DocsDeleteParams,
}

impl StoreOp for DocsDeleteOp {
    type Out = Answer<DocsDeleteResult>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let op = match operation(
            "docs.delete",
            &self.params.command_id,
            &self.caller,
            &self.params,
        ) {
            Ok(op) => op,
            Err(error) => return Ok(Err(error)),
        };
        let Self { caller, params } = self;
        submit(store, op, move |uow| {
            update::decide_delete(uow.view(), &caller, &params).map_err(refusal)
        })
    }
}

/// `docs.put_media`: the caller is authorized on a snapshot before a byte of the asset is
/// decoded, so an outsider or an inactive caller never reaches the image decoder; then the
/// bytes are checked and sanitized, and the write is submitted, authorized again under the
/// writer.
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
        let admitted = store.read(|snapshot| {
            settle(authorize(
                snapshot,
                &self.caller,
                &self.params.project,
                DocsAccess::Write,
            ))
        })?;
        if let (Err(refusal), _) = admitted {
            return Ok(Err(refusal));
        }
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
