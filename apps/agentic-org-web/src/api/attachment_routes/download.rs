//! `GET /api/v1/organizations/{org}/attachments/{id}`: one authenticated, streamed file.
//!
//! `attachment.inspect` runs first (the office's live read check). Its `content_sha256` is
//! the strong `ETag`: a matching `If-None-Match` is `304` without reading a byte. Otherwise
//! the first chunk is read before any header is sent (so a refusal is still a status, with
//! `Retry-After` when the office says when), and the rest streams one chunk at a time over
//! a one-slot channel: the web process never holds the whole file. A chunk that disagrees
//! with the inspected file ends the body short of its `Content-Length`, which the client
//! sees as a failed download, never as a different file.

use std::sync::Arc;

use base64::prelude::*;
use hyper::body::Bytes;
use hyper::header::{
    HeaderValue, CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE, ETAG,
};
use hyper::{Response, StatusCode};
use org_proto::attachments::{
    AttachmentInspect, AttachmentInspectParams, AttachmentReadChunk, AttachmentReadChunkParams,
    AttachmentView, ATTACHMENT_CHUNK_MAX_BASE64_CHARS, ATTACHMENT_CHUNK_MAX_BYTES,
};
use tokio::sync::mpsc;

use super::media::{content_disposition, served_type};
use super::office::{office_reply, AttachmentOffice, OFFICE_CALL};
use crate::service::{ApiReply, WebReply};
use crate::sse::WebBody;

/// What every download answer carries, `304` included.
const SANDBOX_POLICY: &str = "default-src 'none'; sandbox";
const CACHE_POLICY: &str = "private, no-cache";

/// One download as the route read it.
pub struct DownloadRequest {
    pub(super) attachment_id: String,
    /// Every `If-None-Match` value, joined.
    pub(super) if_none_match: Option<String>,
    pub(super) head: bool,
}

/// Inspects, then answers `304`, the headers alone (`HEAD`), or the streamed file.
pub(super) async fn download<O: AttachmentOffice>(
    office: Arc<O>,
    request: DownloadRequest,
) -> WebReply {
    let inspect = AttachmentInspectParams {
        attachment_id: request.attachment_id.clone(),
    };
    let view = match office
        .call::<AttachmentInspect>(inspect, true, OFFICE_CALL)
        .await
    {
        Ok(found) => found.attachment,
        Err(error) => return WebReply::Json(office_reply(error)),
    };
    let Some(etag) = strong_etag(&view.content_sha256).filter(|_| view.id == request.attachment_id)
    else {
        return WebReply::Json(super::super::unavailable());
    };
    let (content_type, inline) = served_type(&view.mime_type);
    let mut response = Response::builder()
        .header(ETAG, etag)
        .header(CACHE_CONTROL, CACHE_POLICY)
        .header("content-security-policy", SANDBOX_POLICY)
        .header("x-content-type-options", "nosniff")
        .header("cross-origin-resource-policy", "same-origin")
        .header(
            CONTENT_DISPOSITION,
            content_disposition(&view.filename, inline),
        );
    if request
        .if_none_match
        .as_deref()
        .is_some_and(|tags| matches_etag(tags, &view.content_sha256))
    {
        return WebReply::Stream(finish(
            response.status(StatusCode::NOT_MODIFIED),
            WebBody::from(http_body_util::Full::new(Bytes::new())),
        ));
    }
    response = response
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, content_type)
        .header(CONTENT_LENGTH, view.byte_count);
    if request.head || view.byte_count == 0 {
        return WebReply::Stream(finish(
            response,
            WebBody::from(http_body_util::Full::new(Bytes::new())),
        ));
    }
    let (first, eof) = match read_chunk(&*office, &view, 0).await {
        Ok(chunk) => chunk,
        Err(reply) => return WebReply::Json(reply),
    };
    let (sender, receiver) = mpsc::channel::<Bytes>(1);
    let mut offset = first.len() as u64;
    // The one slot is free: the first chunk is queued before the response leaves.
    let _ = sender.try_send(first);
    if !eof {
        tokio::spawn(async move {
            loop {
                let Ok((chunk, eof)) = read_chunk(&*office, &view, offset).await else {
                    return;
                };
                offset += chunk.len() as u64;
                if sender.send(chunk).await.is_err() || eof {
                    return;
                }
            }
        });
    }
    WebReply::Stream(finish(response, WebBody::channel(receiver)))
}

fn finish(builder: hyper::http::response::Builder, body: WebBody) -> Response<WebBody> {
    builder.body(body).unwrap_or_else(|_| {
        let mut failed = Response::new(WebBody::from(http_body_util::Full::new(Bytes::new())));
        *failed.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
        failed
    })
}

/// `"<sha256>"` for a 64-digit lowercase hex digest; anything else is not served.
fn strong_etag(sha256: &str) -> Option<HeaderValue> {
    (sha256.len() == 64
        && sha256
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
    .then(|| HeaderValue::from_str(&format!("\"{sha256}\"")).ok())
    .flatten()
}

/// RFC 9110 `If-None-Match`: `*`, or any listed tag that weakly matches ours.
fn matches_etag(tags: &str, sha256: &str) -> bool {
    let ours = format!("\"{sha256}\"");
    tags.split(',')
        .map(str::trim)
        .any(|tag| tag == "*" || tag.strip_prefix("W/").unwrap_or(tag) == ours)
}

/// One chunk at `offset`, checked against the inspected file: its offset, total, digest,
/// size and end. Anything else is the office's fault, never served.
async fn read_chunk<O: AttachmentOffice>(
    office: &O,
    view: &AttachmentView,
    offset: u64,
) -> Result<(Bytes, bool), ApiReply> {
    let params = AttachmentReadChunkParams {
        attachment_id: view.id.clone(),
        offset,
        max_bytes: Some(ATTACHMENT_CHUNK_MAX_BYTES),
    };
    let chunk = office
        .call::<AttachmentReadChunk>(params, true, OFFICE_CALL)
        .await
        .map_err(office_reply)?;
    let unavailable = super::super::unavailable;
    if chunk.offset != offset
        || chunk.total_bytes != view.byte_count
        || chunk.content_sha256 != view.content_sha256
        || chunk.data_base64.len() > ATTACHMENT_CHUNK_MAX_BASE64_CHARS
    {
        return Err(unavailable());
    }
    let data = BASE64_STANDARD
        .decode(chunk.data_base64.as_bytes())
        .map_err(|_| unavailable())?;
    let end = offset
        .checked_add(data.len() as u64)
        .ok_or_else(unavailable)?;
    if data.len() > ATTACHMENT_CHUNK_MAX_BYTES as usize
        || end > view.byte_count
        || chunk.eof != (end == view.byte_count)
        || (data.is_empty() && !chunk.eof)
    {
        return Err(unavailable());
    }
    Ok((Bytes::from(data), chunk.eof))
}
