//! The one snapshot read of `search.query` (`TASK-91`): its references resolved, and the
//! shared and message sources asked, at one consistent point of the store.

use org_proto::ApiError;
use org_store::{Id, StoreError};

use super::ReadOp;
use crate::attachments::ports::AttachmentsRead;
use crate::search::features::query::{read_sources, Resolved, SearchRequest};
use crate::search::ports::SearchRead;
use crate::tasks::ports::TasksRead;

/// Resolves a validated search for `caller` and asks the snapshot's sources.
pub(crate) struct SearchSnapshotRead {
    pub request: SearchRequest,
    pub owner: Id,
    pub caller: Id,
    pub now_utc: String,
}

impl ReadOp for SearchSnapshotRead {
    type Out = Result<Resolved, ApiError>;

    fn read<S: TasksRead + AttachmentsRead + SearchRead>(
        self,
        snapshot: &S,
    ) -> Result<Self::Out, StoreError> {
        let owner = crate::organization::owner::effective_owner_id(snapshot, &self.owner)?;
        Ok(read_sources(
            snapshot,
            self.request,
            &owner,
            &self.caller,
            &self.now_utc,
        )?)
    }
}
