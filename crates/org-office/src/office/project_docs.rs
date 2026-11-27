//! The office's `docs.*` calls (`TASK-55`, `L11`, `FC-180`): the project documentation tree
//! read on one snapshot or written as one operation, each authorized for `caller` by the tree's
//! own choke-point (`crate::project_docs::access`).

use org_proto::docs::{
    DocsCreateParams, DocsIndexParams, DocsIndexResult, DocsInspectParams, DocsInspectResult,
    DocsListParams, DocsListResult, DocsMoveParams, DocsPutMediaParams, DocsPutMediaResult,
    DocsUpdateParams, DocsWriteResult,
};
use org_proto::ApiError;

use crate::error::OfficeError;
use crate::store::open::project_doc_ops::{
    DocsCreateOp, DocsIndexRead, DocsInspectRead, DocsListRead, DocsMoveOp, DocsPutMediaOp,
    DocsUpdateOp,
};

use super::{storage, Office};

type Answer<T> = Result<Result<T, ApiError>, OfficeError>;

impl Office {
    /// `docs.index`: the project's whole documentation tree.
    pub fn docs_index(&self, caller: &str, params: DocsIndexParams) -> Answer<DocsIndexResult> {
        let read = DocsIndexRead {
            caller: caller.to_owned(),
            params,
        };
        Ok(self.store.read(read).map_err(storage)?.0)
    }

    /// `docs.list`: documents filtered by section, kind and status, a page at a time.
    pub fn docs_list(&self, caller: &str, params: DocsListParams) -> Answer<DocsListResult> {
        let read = DocsListRead {
            caller: caller.to_owned(),
            params,
        };
        Ok(self.store.read(read).map_err(storage)?.0)
    }

    /// `docs.inspect`: one document with its body.
    pub fn docs_inspect(
        &self,
        caller: &str,
        params: DocsInspectParams,
    ) -> Answer<DocsInspectResult> {
        let read = DocsInspectRead {
            caller: caller.to_owned(),
            params,
        };
        Ok(self.store.read(read).map_err(storage)?.0)
    }

    /// `docs.create`.
    pub fn docs_create(&self, caller: &str, params: DocsCreateParams) -> Answer<DocsWriteResult> {
        self.store
            .run(DocsCreateOp {
                caller: caller.to_owned(),
                params,
            })
            .map_err(storage)
    }

    /// `docs.update`.
    pub fn docs_update(&self, caller: &str, params: DocsUpdateParams) -> Answer<DocsWriteResult> {
        self.store
            .run(DocsUpdateOp {
                caller: caller.to_owned(),
                params,
            })
            .map_err(storage)
    }

    /// `docs.move`.
    pub fn docs_move(&self, caller: &str, params: DocsMoveParams) -> Answer<DocsWriteResult> {
        self.store
            .run(DocsMoveOp {
                caller: caller.to_owned(),
                params,
            })
            .map_err(storage)
    }

    /// `docs.put_media`.
    pub fn docs_put_media(
        &self,
        caller: &str,
        params: DocsPutMediaParams,
    ) -> Answer<DocsPutMediaResult> {
        self.store
            .run(DocsPutMediaOp {
                caller: caller.to_owned(),
                params,
            })
            .map_err(storage)
    }
}
