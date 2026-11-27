//! The office's `attachment.*` RPCs (`TASK-98.1`, ADR-0037): nine chunked calls over
//! [`Office`], for people on a host channel and agents on the office socket alike. Who may
//! do what to which file is decided by the office's one `AuthorizationPolicy`; this module
//! only parses, routes and answers. A human-private file is never served to an agent.
//!
//! On a satellite, `attachment.read_chunk` of a shared file whose bytes are not here pulls
//! them from HQ over federation into the office's bounded cache (`TASK-98.6`), after the
//! policy allowed the read; an unreachable HQ is `unavailable`, worth retrying.

use std::sync::Arc;

use org_office::attachments::remote::BlobPull;
use org_office::{Office, OfficeError, OfficeRole};
use org_proto::attachments::{
    AttachmentAttach, AttachmentInspect, AttachmentList, AttachmentReadChunk, AttachmentUpdateMeta,
    AttachmentUploadAbort, AttachmentUploadBegin, AttachmentUploadChunk, AttachmentUploadCommit,
};
use org_proto::{ApiError, Call, ErrorCode, Outcome, Request, Response};

use crate::failure::logged;
use crate::federation::{blob_cache_max_bytes, HqBlobRemote};

/// Whether `request` is one of the nine attachment calls.
pub(crate) fn is_attachment_method(request: &Request) -> bool {
    request.is::<AttachmentUploadBegin>()
        || request.is::<AttachmentUploadChunk>()
        || request.is::<AttachmentUploadCommit>()
        || request.is::<AttachmentUploadAbort>()
        || request.is::<AttachmentAttach>()
        || request.is::<AttachmentInspect>()
        || request.is::<AttachmentList>()
        || request.is::<AttachmentUpdateMeta>()
        || request.is::<AttachmentReadChunk>()
}

/// Whether `response` answers an `attachment.attach` that bound a file. A satellite's new
/// shared file then waits in its outbox, so the caller wakes the shipper at once.
pub(crate) fn bound_a_file(request: &Request, response: &Response) -> bool {
    request.is::<AttachmentAttach>() && matches!(response.outcome, Outcome::Result(_))
}

/// Parses `request` as `C`, runs `handler`, and answers: the result, the caller's refusal, or
/// a fixed storage failure (the cause goes to the log, never to the wire).
fn run<C: Call>(
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
    let failed = || {
        Response::error(
            request.id,
            ApiError::new(ErrorCode::StorageError, "the office could not answer"),
        )
    };
    match handler(params) {
        Ok(Ok(result)) => Response::ok::<C>(request.id, &result).unwrap_or_else(|_| failed()),
        Ok(Err(refusal)) => Response::error(request.id, refusal),
        Err(error) => {
            logged(C::METHOD, &error);
            failed()
        }
    }
}

/// Answers one attachment call for the bound `member`, or `None` for any other method.
pub(crate) fn dispatch_attachment_rpc(
    office: &Arc<Office>,
    member: Option<&str>,
    request: &Request,
) -> Option<Response> {
    if !is_attachment_method(request) {
        return None;
    }
    let Some(caller) = member else {
        return Some(Response::error(
            request.id,
            ApiError::new(ErrorCode::NotBound, "connection is not bound"),
        ));
    };
    dispatch_uploads(office, caller, request).or_else(|| dispatch_files(office, caller, request))
}

fn dispatch_uploads(office: &Office, caller: &str, request: &Request) -> Option<Response> {
    if request.is::<AttachmentUploadBegin>() {
        return Some(run::<AttachmentUploadBegin>(request, |p| {
            office.attachment_upload_begin(caller, p)
        }));
    }
    if request.is::<AttachmentUploadChunk>() {
        return Some(run::<AttachmentUploadChunk>(request, |p| {
            office.attachment_upload_chunk(caller, p)
        }));
    }
    if request.is::<AttachmentUploadCommit>() {
        return Some(run::<AttachmentUploadCommit>(request, |p| {
            office.attachment_upload_commit(caller, p)
        }));
    }
    if request.is::<AttachmentUploadAbort>() {
        return Some(run::<AttachmentUploadAbort>(request, |p| {
            office.attachment_upload_abort(caller, p)
        }));
    }
    None
}

/// `attachment.read_chunk`, pulling a shared file's bytes from HQ on a satellite.
fn read_chunk(
    office: &Arc<Office>,
    caller: &str,
    params: org_proto::attachments::AttachmentReadChunkParams,
) -> Result<Result<org_proto::attachments::AttachmentReadChunkResult, ApiError>, OfficeError> {
    if office.info().role != OfficeRole::Satellite {
        return office.attachment_read_chunk(caller, params);
    }
    let remote = HqBlobRemote::new(Arc::clone(office));
    let pull = BlobPull {
        remote: &remote,
        cache_max_bytes: blob_cache_max_bytes(),
    };
    office.attachment_read_chunk_with(caller, params, Some(pull))
}

fn dispatch_files(office: &Arc<Office>, caller: &str, request: &Request) -> Option<Response> {
    if request.is::<AttachmentAttach>() {
        return Some(run::<AttachmentAttach>(request, |p| {
            office.attachment_attach(caller, p)
        }));
    }
    if request.is::<AttachmentInspect>() {
        return Some(run::<AttachmentInspect>(request, |p| {
            office.attachment_inspect(caller, p)
        }));
    }
    if request.is::<AttachmentList>() {
        return Some(run::<AttachmentList>(request, |p| {
            office.attachment_list(caller, p)
        }));
    }
    if request.is::<AttachmentUpdateMeta>() {
        return Some(run::<AttachmentUpdateMeta>(request, |p| {
            office.attachment_update_meta(caller, p)
        }));
    }
    if request.is::<AttachmentReadChunk>() {
        return Some(run::<AttachmentReadChunk>(request, |p| {
            read_chunk(office, caller, p)
        }));
    }
    None
}

#[cfg(test)]
mod tests;
