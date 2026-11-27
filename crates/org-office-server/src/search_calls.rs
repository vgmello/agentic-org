//! Office RPC of unified cross-entity search (`TASK-91`): `search.query`, answered by
//! [`Office::search`] in one consistent snapshot of the shared and message indexes and, on
//! HQ, the private notes index.
//!
//! An unbound connection is refused `NotAuthorized`, exactly as an unknown, inactive, paused
//! or offboarded member is. A storage failure is logged and answers a fixed `StorageError`:
//! never an SQL text, a path or a word of what was searched.

use org_office::Office;
use org_proto::search::SearchQuery;
use org_proto::{ApiError, Call, ErrorCode, Request, Response};

/// Whether `request` is `search.query`.
pub(crate) fn is_search_method(request: &Request) -> bool {
    request.is::<SearchQuery>()
}

/// The refusal of a caller who may not search, bound to nobody included.
pub(crate) fn not_authorized(id: u64) -> Response {
    Response::error(
        id,
        ApiError::new(
            ErrorCode::NotAuthorized,
            "only an active member of this organization can search it",
        ),
    )
}

fn storage_error(id: u64) -> Response {
    Response::error(
        id,
        ApiError::new(ErrorCode::StorageError, "the office could not answer"),
    )
}

/// Dispatches `search.query` for the bound `member` against [`Office`].
pub(crate) fn dispatch_search_rpc(
    office: &Office,
    member: Option<&str>,
    request: &Request,
) -> Option<Response> {
    if !is_search_method(request) {
        return None;
    }
    let Some(member) = member else {
        return Some(not_authorized(request.id));
    };
    let Ok(params) = request.params::<SearchQuery>() else {
        return Some(Response::error(
            request.id,
            ApiError::new(
                ErrorCode::InvalidRequest,
                format!("the params of `{}` are not valid", SearchQuery::METHOD),
            ),
        ));
    };
    Some(match office.search(member, params) {
        Ok(Ok(result)) => Response::ok::<SearchQuery>(request.id, &result)
            .unwrap_or_else(|_| storage_error(request.id)),
        Ok(Err(refusal)) => Response::error(request.id, refusal),
        Err(error) => {
            crate::logged(SearchQuery::METHOD, &error);
            storage_error(request.id)
        }
    })
}

#[cfg(test)]
mod tests;
