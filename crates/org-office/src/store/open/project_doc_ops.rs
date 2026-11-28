//! Engine-independent `ReadOp` and `StoreOp` wrappers for the documentation tree (`TASK-55`).

use org_docs::project_tree::ops as tree_ops;
use org_proto::docs::{
    DocsCreateParams, DocsDeleteParams, DocsDeleteResult, DocsIndexParams, DocsIndexResult,
    DocsInspectParams, DocsInspectResult, DocsListParams, DocsListResult, DocsMoveParams,
    DocsPutMediaParams, DocsPutMediaResult, DocsUpdateParams, DocsWriteResult,
};
use org_proto::{ApiError, ErrorCode};
use org_store::{Store, StoreError, Timestamp};

use crate::project_docs::access::{authorize, Denied, DocsAccess};
use crate::project_docs::features::{derive_id, put_media, run_read, run_write};
use crate::project_docs::ports::ProjectDocsRead;

use super::{build_operation, submit_api_op, OfficeEngine, ReadOp, StoreOp};

type Answer<T> = Result<Result<T, ApiError>, StoreError>;

fn decode_err(bytes: &[u8]) -> ApiError {
    Denied::decode_rejection(
        bytes,
        ErrorCode::InvalidRequest,
        "documentation write refused",
    )
}

fn settle<T>(result: Result<T, Denied>) -> Answer<T> {
    match result {
        Ok(value) => Ok(Ok(value)),
        Err(Denied::Refused(error)) => Ok(Err(error)),
        Err(Denied::Storage(error)) => Err(error.into()),
    }
}

macro_rules! docs_read_ops {
    ($($name:ident($params:ty) -> $res:ty => |$v:ident, $pid:ident, $p:ident| $body:expr;)*) => {$(
        pub struct $name {
            pub caller: String,
            pub params: $params,
        }
        impl ReadOp for $name {
            type Out = Result<$res, ApiError>;
            fn read<S: ProjectDocsRead>(self, snapshot: &S) -> Answer<$res> {
                let $p = self.params;
                settle(run_read(snapshot, &self.caller, &$p.project, |$v, $pid| $body))
            }
        }
    )*};
}

macro_rules! docs_write_ops {
    ($($name:ident($method:literal, $params:ty) -> $res:ty => |$uow:ident, $caller:ident, $p:ident| $body:expr;)*) => {$(
        pub struct $name {
            pub caller: String,
            pub params: $params,
        }
        impl StoreOp for $name {
            type Out = Answer<$res>;
            fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
                let op = match build_operation($method, &self.params.command_id, &self.caller, &self.params) {
                    Ok(op) => op,
                    Err(error) => return Ok(Err(error)),
                };
                let Self { caller: $caller, params: $p } = self;
                submit_api_op(store, op, "documentation", decode_err, move |$uow| ($body).map_err(Denied::into_refusal))
            }
        }
    )*};
}

docs_read_ops! {
    DocsIndexRead(DocsIndexParams) -> DocsIndexResult => |v, pid, _p| tree_ops::read_index(v, pid);
    DocsListRead(DocsListParams) -> DocsListResult => |v, pid, p| tree_ops::read_list(v, pid, &p);
    DocsInspectRead(DocsInspectParams) -> DocsInspectResult => |v, pid, p| tree_ops::read_inspect(v, pid, &p);
}

docs_write_ops! {
    DocsCreateOp("docs.create", DocsCreateParams) -> DocsWriteResult => |uow, caller, params| {
        let id = derive_id("project-doc", &params.command_id, uow.now().0).to_string();
        run_write(uow.view(), uow.now(), &caller, &params.project, |v, c| tree_ops::decide_create(v, c, &params, id))
    };
    DocsUpdateOp("docs.update", DocsUpdateParams) -> DocsWriteResult => |uow, caller, params| {
        run_write(uow.view(), uow.now(), &caller, &params.project, |v, c| tree_ops::decide_update(v, c, &params))
    };
    DocsMoveOp("docs.move", DocsMoveParams) -> DocsWriteResult => |uow, caller, params| {
        run_write(uow.view(), uow.now(), &caller, &params.project, |v, c| tree_ops::decide_move(v, c, &params))
    };
    DocsDeleteOp("docs.delete", DocsDeleteParams) -> DocsDeleteResult => |uow, caller, params| {
        run_write(uow.view(), Timestamp(0), &caller, &params.project, |v, c| tree_ops::decide_delete(v, c, &params))
    };
}

/// `docs.put_media`: authorizes on a snapshot before image decoding, then submits under the writer.
pub struct DocsPutMediaOp {
    pub caller: String,
    pub params: DocsPutMediaParams,
}

impl StoreOp for DocsPutMediaOp {
    type Out = Answer<DocsPutMediaResult>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let op = match build_operation(
            "docs.put_media",
            &self.params.command_id,
            &self.caller,
            &self.params,
        ) {
            Ok(op) => op,
            Err(error) => return Ok(Err(error)),
        };
        let admitted = store.read(|snap| {
            settle(authorize(
                snap,
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
        submit_api_op(store, op, "documentation", decode_err, move |uow| {
            let id = derive_id("project-doc-media", &params.command_id, uow.now().0);
            put_media::decide(uow.view(), uow.now(), &caller, &params, &prepared, id)
                .map_err(Denied::into_refusal)
        })
    }
}
