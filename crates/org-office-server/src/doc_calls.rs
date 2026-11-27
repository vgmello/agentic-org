//! Office RPC dispatcher for the project documentation tree (`TASK-55`, `L11`, `FC-180`):
//! `docs.index`, `docs.list`, `docs.inspect`, `docs.create`, `docs.update`, `docs.move` and
//! `docs.put_media`. Each runs as the bound member; who may see which tree is decided by the
//! office's one documentation choke-point, never here.

use org_office::{Office, OfficeError};
use org_proto::docs::{
    DocsCreate, DocsDelete, DocsIndex, DocsInspect, DocsList, DocsMove, DocsPutMedia, DocsUpdate,
};
use org_proto::{ApiError, Call, ErrorCode, Request, Response};

/// Whether `request` is one of the `docs.*` or delivery calls.
pub(crate) fn is_doc_method(request: &Request) -> bool {
    is_doc_tree_method(request) || crate::delivery_calls::is_delivery_method(request)
}

fn is_doc_tree_method(request: &Request) -> bool {
    request.is::<DocsIndex>()
        || request.is::<DocsList>()
        || request.is::<DocsInspect>()
        || request.is::<DocsCreate>()
        || request.is::<DocsUpdate>()
        || request.is::<DocsMove>()
        || request.is::<DocsDelete>()
        || request.is::<DocsPutMedia>()
}

/// Answers one `docs.*` or delivery call for `member` against `office`; `None` for any other method.
pub(crate) fn dispatch_doc_rpc(
    office: &Office,
    member: Option<&str>,
    request: &Request,
) -> Option<Response> {
    if let Some(resp) = crate::delivery_calls::dispatch_delivery_rpc(office, member, request) {
        return Some(resp);
    }
    if !is_doc_tree_method(request) {
        return None;
    }
    let Some(caller) = member else {
        return Some(Response::error(
            request.id,
            ApiError::new(ErrorCode::NotBound, "connection is not bound"),
        ));
    };
    Some(if request.is::<DocsIndex>() {
        serve::<DocsIndex>(request, |p| office.docs_index(caller, p))
    } else if request.is::<DocsList>() {
        serve::<DocsList>(request, |p| office.docs_list(caller, p))
    } else if request.is::<DocsInspect>() {
        serve::<DocsInspect>(request, |p| office.docs_inspect(caller, p))
    } else if request.is::<DocsCreate>() {
        serve::<DocsCreate>(request, |p| office.docs_create(caller, p))
    } else if request.is::<DocsUpdate>() {
        serve::<DocsUpdate>(request, |p| office.docs_update(caller, p))
    } else if request.is::<DocsMove>() {
        serve::<DocsMove>(request, |p| office.docs_move(caller, p))
    } else if request.is::<DocsDelete>() {
        serve::<DocsDelete>(request, |p| office.docs_delete(caller, p))
    } else {
        serve::<DocsPutMedia>(request, |p| office.docs_put_media(caller, p))
    })
}

fn serve<C: Call>(
    request: &Request,
    handler: impl FnOnce(C::Params) -> Result<Result<C::Result, ApiError>, OfficeError>,
) -> Response {
    let Ok(params) = request.params::<C>() else {
        return Response::error(
            request.id,
            ApiError::new(
                ErrorCode::InvalidRequest,
                format!("invalid parameters for `{}`", C::METHOD),
            ),
        );
    };
    let answer = handler(params)
        .map_err(|e| ApiError::new(ErrorCode::StorageError, e.to_string()))
        .and_then(|r| r);
    match answer {
        Ok(result) => Response::ok::<C>(request.id, &result).unwrap_or_else(|_| {
            Response::error(
                request.id,
                ApiError::new(ErrorCode::StorageError, "failed encoding response"),
            )
        }),
        Err(error) => Response::error(request.id, error),
    }
}

#[cfg(test)]
#[path = "doc_calls_tests.rs"]
mod tests;
