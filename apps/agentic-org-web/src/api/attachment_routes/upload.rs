//! `POST /api/v1/organizations/{org}/attachments`: one streamed upload into the caller's
//! staging area. The declared file is begun, each body chunk is forwarded as soon as it
//! arrives (at most 512 KiB per `attachment.upload_chunk`), and the complete upload is
//! committed (validated and sanitized by the office). Any failure after the begin aborts
//! the staged upload, so nothing half-sent stays behind.

use std::time::Duration;

use base64::prelude::*;
use hyper::StatusCode;
use org_proto::attachments::{
    AttachmentUploadAbort, AttachmentUploadBegin, AttachmentUploadBeginParams,
    AttachmentUploadChunk, AttachmentUploadChunkParams, AttachmentUploadCommit,
    AttachmentUploadRef, StagedUploadView, ATTACHMENT_CHUNK_MAX_BYTES,
};
use org_proto::web::{ReadResponse, WebErrorCode};
use serde::Serialize;

use super::office::{office_reply, AttachmentOffice};
use crate::body::stream::{StreamError, StreamedBody};
use crate::service::ApiReply;

/// How long a best-effort abort of a failed upload may take.
const ABORT_DEADLINE: Duration = Duration::from_secs(3);

/// The file the browser declared in its headers, already checked.
pub struct UploadFile {
    pub(super) filename: String,
    pub(super) mime: &'static str,
}

/// The `201 Created` result: the committed upload, ready for `attachment.attach`.
#[derive(Serialize)]
struct UploadedFile {
    upload_id: String,
    mime_type: String,
    byte_count: u64,
    width: Option<u32>,
    height: Option<u32>,
    page_count: Option<u32>,
    filename: String,
    content_sha256: Option<String>,
}

impl From<StagedUploadView> for UploadedFile {
    fn from(view: StagedUploadView) -> Self {
        Self {
            upload_id: view.upload_id,
            mime_type: view.mime_type,
            byte_count: view.byte_count,
            width: view.width,
            height: view.height,
            page_count: view.page_count,
            filename: view.filename,
            content_sha256: view.content_sha256,
        }
    }
}

/// Streams `body` into a new staged upload and commits it.
pub(super) async fn upload<O: AttachmentOffice>(
    office: &O,
    file: UploadFile,
    mut body: StreamedBody,
) -> ApiReply {
    let begin = AttachmentUploadBeginParams {
        filename: file.filename,
        mime_type: file.mime.to_owned(),
        byte_count: body.declared(),
    };
    let begun = match budget(&body) {
        Ok(deadline) => {
            office
                .call::<AttachmentUploadBegin>(begin, false, deadline)
                .await
        }
        Err(reply) => return reply,
    };
    let begun = match begun {
        Ok(begun) => begun,
        Err(error) => return office_reply(error),
    };
    let upload_id = begun.upload.upload_id;
    let piece = begun.chunk_size_bytes.clamp(1, ATTACHMENT_CHUNK_MAX_BYTES) as usize;
    if let Err(reply) = send_chunks(office, &upload_id, piece, &mut body).await {
        abort(office, upload_id).await;
        return reply;
    }
    let commit = AttachmentUploadRef {
        upload_id: upload_id.clone(),
    };
    let committed = match budget(&body) {
        Ok(deadline) => office
            .call::<AttachmentUploadCommit>(commit, false, deadline)
            .await
            .map_err(office_reply),
        Err(reply) => Err(reply),
    };
    match committed {
        Ok(committed) => ApiReply::json(
            StatusCode::CREATED,
            serde_json::to_value(ReadResponse::Result(UploadedFile::from(committed.upload)))
                .expect("upload result serializes"),
        ),
        Err(reply) => {
            abort(office, upload_id).await;
            reply
        }
    }
}

/// Forwards every chunk at the offset the office has acknowledged so far.
async fn send_chunks<O: AttachmentOffice>(
    office: &O,
    upload_id: &str,
    piece: usize,
    body: &mut StreamedBody,
) -> Result<(), ApiReply> {
    let mut offset: u64 = 0;
    while let Some(next) = body.next_chunk().await {
        let chunk = next.map_err(stream_reply)?;
        for part in chunk.chunks(piece) {
            let params = AttachmentUploadChunkParams {
                upload_id: upload_id.to_owned(),
                offset,
                data_base64: BASE64_STANDARD.encode(part),
            };
            let appended = office
                .call::<AttachmentUploadChunk>(params, false, budget(body)?)
                .await
                .map_err(office_reply)?;
            offset += part.len() as u64;
            if appended.received_bytes != offset {
                return Err(super::super::unavailable());
            }
        }
    }
    if offset != body.declared() {
        return Err(stream_reply(StreamError::LengthMismatch));
    }
    Ok(())
}

/// Removes a staged upload that will not be committed; its answer changes nothing.
async fn abort<O: AttachmentOffice>(office: &O, upload_id: String) {
    let _ = office
        .call::<AttachmentUploadAbort>(AttachmentUploadRef { upload_id }, false, ABORT_DEADLINE)
        .await;
}

/// What is left of the upload's one deadline, for the next office call.
fn budget(body: &StreamedBody) -> Result<Duration, ApiReply> {
    let remaining = body.remaining();
    if remaining.is_zero() {
        return Err(stream_reply(StreamError::TimedOut));
    }
    Ok(remaining)
}

fn stream_reply(error: StreamError) -> ApiReply {
    match error {
        StreamError::TimedOut => ApiReply::error(
            StatusCode::SERVICE_UNAVAILABLE,
            WebErrorCode::Timeout,
            "The upload timed out.",
        ),
        StreamError::LengthMismatch => ApiReply::error(
            StatusCode::BAD_REQUEST,
            WebErrorCode::InvalidRequest,
            "The upload's length does not match its Content-Length.",
        ),
        StreamError::Malformed => ApiReply::error(
            StatusCode::BAD_REQUEST,
            WebErrorCode::InvalidRequest,
            "Invalid upload body.",
        ),
    }
}
