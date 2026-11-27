//! The office facade of `search.query` (`TASK-91`): who may search, one consistent snapshot of
//! the shared and message sources, then HQ's private index when the request asks for private
//! notes or decisions on their own.

use org_proto::search::{SearchQueryParams, SearchQueryResult};
use org_proto::ApiError;
use org_store::Id;

use crate::error::OfficeError;
use crate::search::features::query::{answer, not_authorized, searcher, validate};
use crate::search::ports::PrivateScope;
use crate::store::open::search_ops::SearchSnapshotRead;
use crate::time::format_rfc3339_ms;

use super::{storage, Office};

impl Office {
    /// `search.query`: the caller's visible matches across tasks, processes and standards,
    /// projects, members, teams and roles and the messages it may read, or, asked for on their
    /// own, the private notes and decisions `ADR-0031` lets it read. An unknown, inactive,
    /// paused or offboarded caller is refused `NotAuthorized` before anything else; a
    /// satellite answers private types with nothing and says so in the coverage.
    pub fn search(
        &self,
        actor: &str,
        params: SearchQueryParams,
    ) -> Result<Result<SearchQueryResult, ApiError>, OfficeError> {
        let Ok(caller) = Id::parse(actor) else {
            return Ok(Err(not_authorized()));
        };
        if searcher(self.load_employee(actor)?.as_ref()).is_none() {
            return Ok(Err(not_authorized()));
        }
        let request = match validate(params) {
            Ok(request) => request,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let read = SearchSnapshotRead {
            request,
            owner: self.effective_owner_for_ops(),
            caller,
            now_utc: format_rfc3339_ms(self.monotonic_now_ms()),
        };
        let mut resolved = match self.store.read(read).map_err(storage)?.0 {
            Ok(resolved) => resolved,
            Err(refusal) => return Ok(Err(refusal)),
        };
        if resolved.searches_private() {
            if !self.private_search.available() {
                resolved.private_omitted();
            } else if resolved.plan().terms.is_empty() {
                resolved.private_searched(None);
            } else {
                let scope = PrivateScope {
                    org_id: &self.info.org_id,
                    caller_id: actor,
                    caller_is_person: resolved.caller_is_person(),
                };
                let page = self.private_search.search(resolved.plan(), &scope)?;
                resolved.private_searched(Some(page));
            }
        }
        Ok(Ok(answer(resolved)))
    }
}
