//! The office's `docs.*` calls (`TASK-55`, `L11`, `FC-180`): HQ owns writes; satellites serve
//! reads from their replicated copy.

use org_proto::docs::{
    DocsCreateParams, DocsDeleteParams, DocsDeleteResult, DocsIndexParams, DocsIndexResult,
    DocsInspectParams, DocsInspectResult, DocsListParams, DocsListResult, DocsMoveParams,
    DocsPutMediaParams, DocsPutMediaResult, DocsUpdateParams, DocsWriteResult,
};
use org_proto::{ApiError, ErrorCode};

use crate::error::OfficeError;
use crate::model::OfficeRole;
use crate::store::open::project_doc_ops::{
    DocsCreateOp, DocsDeleteOp, DocsIndexRead, DocsInspectRead, DocsListRead, DocsMoveOp,
    DocsPutMediaOp, DocsUpdateOp,
};

use super::{storage, Office};

type Answer<T> = Result<Result<T, ApiError>, OfficeError>;

fn hq_only() -> ApiError {
    ApiError::new(
        ErrorCode::Unavailable,
        "project documentation is edited at HQ; this satellite office reads its replicated copy",
    )
}

macro_rules! office_docs_reads {
    ($($fn_name:ident($params:ty) -> $res:ty => $op:ident;)*) => {$(
        pub fn $fn_name(&self, caller: &str, params: $params) -> Answer<$res> {
            Ok(self.store.read($op { caller: caller.to_owned(), params }).map_err(storage)?.0)
        }
    )*};
}

macro_rules! office_docs_writes {
    ($($fn_name:ident($params:ty) -> $res:ty => $op:ident;)*) => {$(
        pub fn $fn_name(&self, caller: &str, params: $params) -> Answer<$res> {
            if self.info.role == OfficeRole::Satellite {
                return Ok(Err(hq_only()));
            }
            self.store.run($op { caller: caller.to_owned(), params }).map_err(storage)
        }
    )*};
}

impl Office {
    office_docs_reads! {
        docs_index(DocsIndexParams) -> DocsIndexResult => DocsIndexRead;
        docs_list(DocsListParams) -> DocsListResult => DocsListRead;
        docs_inspect(DocsInspectParams) -> DocsInspectResult => DocsInspectRead;
    }

    office_docs_writes! {
        docs_create(DocsCreateParams) -> DocsWriteResult => DocsCreateOp;
        docs_update(DocsUpdateParams) -> DocsWriteResult => DocsUpdateOp;
        docs_move(DocsMoveParams) -> DocsWriteResult => DocsMoveOp;
        docs_delete(DocsDeleteParams) -> DocsDeleteResult => DocsDeleteOp;
        docs_put_media(DocsPutMediaParams) -> DocsPutMediaResult => DocsPutMediaOp;
    }
}
