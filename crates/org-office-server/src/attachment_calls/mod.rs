//! The office's `attachment.*` RPCs (`TASK-98.1`, ADR-0037): nine chunked calls over
//! [`Office`], for people on a host channel and agents on the office socket alike. Who may
//! do what to which file is decided by the office's one `AuthorizationPolicy`; this module
//! only parses, routes and answers. A human-private file is never served to an agent.

use org_office::{Office, OfficeError};
use org_proto::attachments::{
    AttachmentAttach, AttachmentInspect, AttachmentList, AttachmentReadChunk, AttachmentUpdateMeta,
    AttachmentUploadAbort, AttachmentUploadBegin, AttachmentUploadChunk, AttachmentUploadCommit,
};
use org_proto::{ApiError, Call, ErrorCode, Request, Response};

use crate::failure::logged;

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
    office: &Office,
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

fn dispatch_files(office: &Office, caller: &str, request: &Request) -> Option<Response> {
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
            office.attachment_read_chunk(caller, p)
        }));
    }
    None
}

#[cfg(test)]
mod tests;
