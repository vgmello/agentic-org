//! Browser HTTP routes for attachments and standalone files (`TASK-98.5` AC #1, ADR-0037):
//!
//! - `POST /api/v1/organizations/{org}/attachments`: a streamed upload into the caller's
//!   staging area ([`upload`]); the one route whose body bypasses the 1 MiB JSON buffer.
//! - `GET|HEAD /api/v1/organizations/{org}/attachments/{id}`: an authenticated, streamed
//!   download ([`download`]).
//! - `POST /api/v1/organizations/{org}/files`, `GET .../files` and `PATCH .../files/{id}`:
//!   commit, list and edit standalone files ([`files`]).
//!
//! Uploads and downloads take only a full cookie and `X-Org-Session-Proof` pair (never the
//! cookie alone, never local-authless); every header rule is checked before any body byte is
//! read or any office call is made.

mod download;
mod files;
mod media;
mod office;
mod upload;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use hyper::header::IF_NONE_MATCH;
use hyper::{http::request::Parts, Method};
use org_proto::attachments::{AttachmentAttachParams, AttachmentUpdateMetaParams};

use super::{invalid, not_found, Call, CallerAuth, Route, SessionApi};
use crate::body::stream::StreamedBody;
use crate::service::{ApiReply, PreparedCall, WebReply};

pub use download::DownloadRequest;
pub use files::FileListQuery;
pub use upload::UploadFile;

use office::HqOffice;

pub enum AttachmentRoute {
    Upload(CallerAuth, String, UploadFile, u64),
    Download(CallerAuth, String, DownloadRequest),
    FileList(CallerAuth, String, FileListQuery),
    FileCommit(CallerAuth, String),
    FileUpdate(CallerAuth, String, String),
}

pub enum AttachmentCall {
    Upload(CallerAuth, String, UploadFile, StreamedBody),
    Download(CallerAuth, String, DownloadRequest),
    FileList(CallerAuth, String, FileListQuery),
    FileCommit(CallerAuth, String, AttachmentAttachParams),
    FileUpdate(CallerAuth, String, AttachmentUpdateMetaParams),
}

fn valid_org_id(id: &str) -> Result<(), ApiReply> {
    org_office_launch::is_storage_id(id)
        .then_some(())
        .ok_or_else(invalid)
}

/// Attachments and files address one binding by its canonical lowercase UUIDv7.
fn valid_attachment_id(id: &str) -> Result<(), ApiReply> {
    org_proto::is_uuid_v7(id).then_some(()).ok_or_else(invalid)
}

pub(super) fn try_route(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    tail: &[&str],
    get: bool,
    post: bool,
    query: bool,
) -> Option<Result<Route, ApiReply>> {
    let patch = request.method == Method::PATCH && !query;
    let list = matches!(request.method, Method::GET | Method::HEAD);
    let routed = match tail {
        ["attachments"] if post => route_upload(api, request, org),
        ["attachments", id] if get => (|| {
            let caller = CallerAuth::Pair(api.pair(request)?);
            valid_org_id(org)?;
            valid_attachment_id(id)?;
            let tags: Vec<&str> = request
                .headers
                .get_all(IF_NONE_MATCH)
                .iter()
                .filter_map(|value| value.to_str().ok())
                .collect();
            let download = DownloadRequest {
                attachment_id: (*id).to_owned(),
                if_none_match: (!tags.is_empty()).then(|| tags.join(", ")),
                head: request.method == Method::HEAD,
            };
            Ok(AttachmentRoute::Download(caller, org.to_owned(), download))
        })(),
        ["files"] if list => (|| {
            let caller = api.caller_auth(request)?;
            valid_org_id(org)?;
            let query = files::list_query(request.uri.query())?;
            Ok(AttachmentRoute::FileList(caller, org.to_owned(), query))
        })(),
        ["files"] if post => (|| {
            let caller = api.caller_auth(request)?;
            valid_org_id(org)?;
            Ok(AttachmentRoute::FileCommit(caller, org.to_owned()))
        })(),
        ["files", id] if patch => (|| {
            let caller = api.caller_auth(request)?;
            valid_org_id(org)?;
            valid_attachment_id(id)?;
            Ok(AttachmentRoute::FileUpdate(
                caller,
                org.to_owned(),
                (*id).to_owned(),
            ))
        })(),
        ["attachments"] | ["attachments", _] | ["files"] | ["files", _] => Err(invalid()),
        ["attachments", ..] | ["files", ..] => Err(not_found()),
        _ => return None,
    };
    Some(routed.map(Route::Attachments))
}

/// Every upload rule that headers can decide, in order: the full session pair (`401`), the
/// organization id, the file type (`415`), the file name, then the exact length (`411`,
/// `413`). Nothing here reads the body or calls the host or an office.
fn route_upload(api: &SessionApi, request: &Parts, org: &str) -> Result<AttachmentRoute, ApiReply> {
    let caller = CallerAuth::Pair(api.pair(request)?);
    valid_org_id(org)?;
    let media = media::upload_media(&request.headers)?;
    let filename = media::upload_filename(&request.headers)?;
    let declared = media::upload_length(&request.headers, media)?;
    let file = UploadFile {
        filename,
        mime: media.mime,
    };
    Ok(AttachmentRoute::Upload(
        caller,
        org.to_owned(),
        file,
        declared,
    ))
}

/// The buffered JSON routes. An upload never reaches here (the service streams it).
pub(super) fn prepare(
    route: AttachmentRoute,
    bytes: &[u8],
) -> Result<PreparedCall<Call>, ApiReply> {
    let empty = || bytes.is_empty().then_some(()).ok_or_else(invalid);
    let read = |call| Ok(PreparedCall::read(Call::Attachments(call), bytes.len()));
    match route {
        AttachmentRoute::Upload(..) => Err(invalid()),
        AttachmentRoute::Download(caller, org, download) => {
            empty()?;
            read(AttachmentCall::Download(caller, org, download))
        }
        AttachmentRoute::FileList(caller, org, query) => {
            empty()?;
            read(AttachmentCall::FileList(caller, org, query))
        }
        AttachmentRoute::FileCommit(caller, org) => {
            let (id, params) = files::commit_params(bytes)?;
            Ok(PreparedCall::mutation(
                Call::Attachments(AttachmentCall::FileCommit(caller, org, params)),
                id,
                bytes.len(),
            ))
        }
        AttachmentRoute::FileUpdate(caller, org, attachment_id) => {
            let (id, params) = files::update_params(attachment_id, bytes)?;
            Ok(PreparedCall::mutation(
                Call::Attachments(AttachmentCall::FileUpdate(caller, org, params)),
                id,
                bytes.len(),
            ))
        }
    }
}

/// The admitted length of an upload route's streamed body.
pub(super) fn streamed_body(route: &AttachmentRoute) -> Option<u64> {
    match route {
        AttachmentRoute::Upload(.., declared) => Some(*declared),
        _ => None,
    }
}

pub(super) fn prepare_streamed(
    route: AttachmentRoute,
    body: StreamedBody,
) -> Result<PreparedCall<Call>, ApiReply> {
    match route {
        AttachmentRoute::Upload(caller, org, file, _) => Ok(PreparedCall::read(
            Call::Attachments(AttachmentCall::Upload(caller, org, file, body)),
            0,
        )),
        _ => Err(invalid()),
    }
}

/// Resolves the caller's HQ office (validating the session), then runs the call.
pub(super) async fn execute(api: Arc<SessionApi>, call: AttachmentCall) -> WebReply {
    let (caller, org) = match &call {
        AttachmentCall::Upload(caller, org, ..)
        | AttachmentCall::Download(caller, org, _)
        | AttachmentCall::FileList(caller, org, _)
        | AttachmentCall::FileCommit(caller, org, _)
        | AttachmentCall::FileUpdate(caller, org, _) => (*caller, org.clone()),
    };
    let office = match HqOffice::resolve(api, caller, org).await {
        Ok(office) => office,
        Err(reply) => return WebReply::Json(reply),
    };
    WebReply::Json(match call {
        AttachmentCall::Upload(_, _, file, body) => upload::upload(&office, file, body).await,
        AttachmentCall::Download(_, _, request) => {
            return download::download(Arc::new(office), request).await;
        }
        AttachmentCall::FileList(_, _, query) => files::list(&office, query).await,
        AttachmentCall::FileCommit(_, _, params) => files::commit(&office, params).await,
        AttachmentCall::FileUpdate(_, _, params) => files::update(&office, params).await,
    })
}
