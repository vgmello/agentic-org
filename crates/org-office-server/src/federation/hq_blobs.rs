//! HQ's side of shared attachment bytes over federation (`TASK-98.6`, ADR-0037).
//!
//! - **Upload.** A satellite announces one outbox file (`BlobHasRequest`). HQ checks its record
//!   ([`Office::attachment_ingest_check`]) and answers `BlobHasResponse`; when it lacks the
//!   bytes it reassembles the satellite's `BlobChunk`s in order, verifies their SHA-256, keeps
//!   them write-once and binds the record ([`Office::attachment_ingest_commit`]), and only then
//!   sends an accepted `BlobAck`, then wakes every satellite session so the new record goes
//!   out at once. Uploads held in memory are bounded per HQ server (64 MiB) and per satellite
//!   (one upload at a time, so at most one 25 MiB document), and an upload that goes quiet
//!   ([`UPLOAD_IDLE`] without a chunk) or drags on ([`UPLOAD_TOTAL_MAX`] in all) is dropped by
//!   the session loop whether or not another frame ever arrives.
//! - **Fetch.** A satellite asks for one shared file by `(org_id, attachment_id)`
//!   (`BlobFetchRequest`); HQ serves it in `BlobChunk`s from its shared store, or refuses with
//!   a `BlobAck`. Nothing here ever reads HQ's private space.
//!
//! Every office call runs under the satellite's peer-activity fence, so a removed or revoked
//! satellite gets nothing.

use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use org_office::attachments::remote::ShippedAttachment;
use org_proto::{ApiError, ErrorCode};
use org_sync_proto::{BlobAssembler, SyncFrame, BLOB_CHUNK_MAX_BYTES};
use rustls::{ServerConnection, StreamOwned};

use crate::federation_link::hq_peer::{write_sync_frame_if_peer_active, HqSessionPeer};
use crate::federation_stream::HqPeerRegistry;

/// The most bytes of satellite uploads one HQ server holds in memory at once.
pub(super) const MAX_HQ_BLOB_BYTES_IN_FLIGHT: u64 = 64 * 1024 * 1024;
/// How long an upload may wait for its next chunk before HQ drops it.
const UPLOAD_IDLE: Duration = Duration::from_secs(60);
/// The longest one upload may take in all, however steadily its chunks trickle in: a 25 MiB
/// document needs about 85 KiB/s.
const UPLOAD_TOTAL_MAX: Duration = Duration::from_secs(300);
/// The longest base64 record a satellite may announce (a record is a few hundred bytes).
const MAX_RECORD_B64: usize = 128 * 1024;

type Tls = StreamOwned<ServerConnection, TcpStream>;

/// Room one satellite's upload holds in the HQ server's budget, returned when the upload ends
/// (completed, refused, replaced, expired, or its session gone).
struct BudgetHold {
    peers: Arc<HqPeerRegistry>,
    sat_id: String,
}

impl Drop for BudgetHold {
    fn drop(&mut self) {
        self.peers.release_blob_bytes(&self.sat_id);
    }
}

/// One satellite upload in progress on a session.
pub(super) struct HqBlobUpload {
    request_id: String,
    shipped: ShippedAttachment,
    assembler: BlobAssembler,
    started_at: Instant,
    last_chunk: Instant,
    /// How long it may wait for its next chunk ([`UPLOAD_IDLE`] unless the server says less).
    idle: Duration,
    _hold: BudgetHold,
}

impl HqBlobUpload {
    /// Drops the upload in `slot` (and its room in the budget) once it waited [`UPLOAD_IDLE`]
    /// for a chunk or ran [`UPLOAD_TOTAL_MAX`] in all. The session loop calls it on every
    /// turn, so a satellite that announces a file and goes quiet holds nothing for long.
    pub(super) fn expire_if_idle(slot: &mut Option<Self>) {
        if slot.as_ref().is_some_and(|upload| {
            upload.last_chunk.elapsed() >= upload.idle
                || upload.started_at.elapsed() >= UPLOAD_TOTAL_MAX
        }) {
            *slot = None;
        }
    }
}

/// Whether `frame` is one this module answers.
pub(super) fn is_blob_frame(frame: &SyncFrame) -> bool {
    matches!(
        frame,
        SyncFrame::BlobHasRequest { .. }
            | SyncFrame::BlobChunk { .. }
            | SyncFrame::BlobFetchRequest { .. }
    )
}

/// The wire name of an error code (`not_found`, `not_authorized`, ...).
fn code_name(code: ErrorCode) -> String {
    serde_json::to_value(code)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

struct Reply<'a> {
    request_id: &'a str,
    attachment_id: &'a str,
    content_sha256: &'a str,
}

impl Reply<'_> {
    fn ack(&self) -> SyncFrame {
        SyncFrame::BlobAck {
            request_id: self.request_id.to_owned(),
            attachment_id: self.attachment_id.to_owned(),
            content_sha256: self.content_sha256.to_owned(),
            accepted: true,
            code: None,
            message: None,
        }
    }

    fn refuse(&self, code: &str, message: impl Into<String>) -> SyncFrame {
        SyncFrame::BlobAck {
            request_id: self.request_id.to_owned(),
            attachment_id: self.attachment_id.to_owned(),
            content_sha256: self.content_sha256.to_owned(),
            accepted: false,
            code: Some(code.to_owned()),
            message: Some(message.into()),
        }
    }

    fn refused(&self, error: &ApiError) -> SyncFrame {
        self.refuse(&code_name(error.code), error.message.clone())
    }
}

/// Runs an office call under the satellite's peer fence: `None` when the satellite was removed
/// or revoked (the session must end), else the refusal frame or the value.
fn fenced<T>(
    peer: &HqSessionPeer<'_>,
    reply: &Reply<'_>,
    call: impl FnOnce() -> Result<Result<T, ApiError>, org_office::OfficeError>,
) -> Option<Result<T, SyncFrame>> {
    match org_office::run_if_peer_active(
        peer.office,
        peer.sat_id.as_str(),
        Some(peer.cert_serial_hex),
        call,
    ) {
        Ok(Some(Ok(Ok(value)))) => Some(Ok(value)),
        Ok(Some(Ok(Err(refusal)))) => Some(Err(reply.refused(&refusal))),
        Ok(Some(Err(_))) => Some(Err(
            reply.refuse("storage_error", "HQ could not handle the file")
        )),
        Ok(None) | Err(_) => None,
    }
}

/// Answers one blob frame; `false` ends the session.
pub(super) fn handle_hq_blob_frame(
    tls: &mut Tls,
    peer: &HqSessionPeer<'_>,
    peers: &Arc<HqPeerRegistry>,
    upload: &mut Option<HqBlobUpload>,
    frame: SyncFrame,
) -> bool {
    HqBlobUpload::expire_if_idle(upload);
    match frame {
        SyncFrame::BlobHasRequest {
            request_id,
            org_id,
            attachment_id,
            content_sha256,
            byte_count,
            record_b64,
        } => {
            // A new announcement ends any upload the satellite gave up on.
            *upload = None;
            let reply = Reply {
                request_id: &request_id,
                attachment_id: &attachment_id,
                content_sha256: &content_sha256,
            };
            let announced = Announced {
                org_id: &org_id,
                byte_count,
                record_b64: &record_b64,
            };
            handle_has(tls, peer, peers, upload, &reply, &announced)
        }
        SyncFrame::BlobChunk {
            request_id,
            content_sha256,
            offset,
            total_bytes,
            data_b64,
        } => handle_chunk(
            tls,
            peer,
            peers,
            upload,
            &request_id,
            Chunk {
                content_sha256: &content_sha256,
                offset,
                total_bytes,
                data_b64: &data_b64,
            },
        ),
        SyncFrame::BlobFetchRequest {
            request_id,
            org_id,
            attachment_id,
        } => handle_fetch(tls, peer, &request_id, &org_id, &attachment_id),
        _ => true,
    }
}

struct Announced<'a> {
    org_id: &'a str,
    byte_count: u64,
    record_b64: &'a str,
}

fn handle_has(
    tls: &mut Tls,
    peer: &HqSessionPeer<'_>,
    peers: &Arc<HqPeerRegistry>,
    upload: &mut Option<HqBlobUpload>,
    reply: &Reply<'_>,
    announced: &Announced<'_>,
) -> bool {
    let send = |tls: &mut Tls, frame: &SyncFrame| write_sync_frame_if_peer_active(tls, peer, frame);
    if announced.org_id != peer.org_id.as_str() {
        return send(
            tls,
            &reply.refuse("not_authorized", "the file is another organization's"),
        );
    }
    let record_yaml = match (announced.record_b64.len() <= MAX_RECORD_B64)
        .then(|| STANDARD.decode(announced.record_b64.as_bytes()).ok())
        .flatten()
    {
        Some(yaml) => yaml,
        None => return send(tls, &reply.refuse("invalid_request", "unreadable record")),
    };
    let checked = fenced(peer, reply, || {
        peer.office.attachment_ingest_check(
            peer.sat_id.as_str(),
            reply.attachment_id,
            reply.content_sha256,
            announced.byte_count,
            &record_yaml,
        )
    });
    let shipped = match checked {
        None => return false,
        Some(Err(refusal)) => return send(tls, &refusal),
        Some(Ok(shipped)) => shipped,
    };
    let present = SyncFrame::BlobHasResponse {
        request_id: reply.request_id.to_owned(),
        content_sha256: reply.content_sha256.to_owned(),
        present: shipped.present,
    };
    if shipped.present {
        return send(tls, &present) && commit_and_ack(tls, peer, peers, reply, &shipped, None);
    }
    if !peers.try_reserve_blob_bytes(
        peer.sat_id.as_str(),
        shipped.byte_count(),
        MAX_HQ_BLOB_BYTES_IN_FLIGHT,
    ) {
        return send(
            tls,
            &reply.refuse(
                "unavailable",
                "HQ is receiving other files; try again shortly",
            ),
        );
    }
    let hold = BudgetHold {
        peers: Arc::clone(peers),
        sat_id: peer.sat_id.as_str().to_owned(),
    };
    let assembler = match BlobAssembler::new(
        reply.content_sha256,
        shipped.byte_count(),
        shipped.max_bytes(),
    ) {
        Ok(assembler) => assembler,
        Err(error) => return send(tls, &reply.refuse("invalid_request", error.to_string())),
    };
    let now = Instant::now();
    *upload = Some(HqBlobUpload {
        request_id: reply.request_id.to_owned(),
        shipped,
        assembler,
        started_at: now,
        last_chunk: now,
        idle: peers.blob_upload_idle().unwrap_or(UPLOAD_IDLE),
        _hold: hold,
    });
    send(tls, &present)
}

struct Chunk<'a> {
    content_sha256: &'a str,
    offset: u64,
    total_bytes: u64,
    data_b64: &'a str,
}

fn handle_chunk(
    tls: &mut Tls,
    peer: &HqSessionPeer<'_>,
    peers: &HqPeerRegistry,
    upload: &mut Option<HqBlobUpload>,
    request_id: &str,
    chunk: Chunk<'_>,
) -> bool {
    let Some(active) = upload.as_mut().filter(|u| u.request_id == request_id) else {
        let reply = Reply {
            request_id,
            attachment_id: "",
            content_sha256: chunk.content_sha256,
        };
        return write_sync_frame_if_peer_active(
            tls,
            peer,
            &reply.refuse("conflict", "no upload is in progress for this request"),
        );
    };
    let pushed = active.assembler.push(
        chunk.content_sha256,
        chunk.offset,
        chunk.total_bytes,
        chunk.data_b64,
    );
    active.last_chunk = Instant::now();
    match pushed {
        Ok(false) => true,
        Ok(true) => {
            let Some(done) = upload.take() else {
                return false;
            };
            let attachment_id = done.shipped.attachment_id();
            let reply = Reply {
                request_id,
                attachment_id: &attachment_id,
                content_sha256: chunk.content_sha256,
            };
            let bytes = done.assembler.into_verified_bytes();
            // The budget's room is held through the check: the bytes are still in memory.
            commit_and_ack(tls, peer, peers, &reply, &done.shipped, bytes)
        }
        Err(error) => {
            let attachment_id = active.shipped.attachment_id();
            *upload = None;
            let reply = Reply {
                request_id,
                attachment_id: &attachment_id,
                content_sha256: chunk.content_sha256,
            };
            write_sync_frame_if_peer_active(
                tls,
                peer,
                &reply.refuse("invalid_request", error.to_string()),
            )
        }
    }
}

fn commit_and_ack(
    tls: &mut Tls,
    peer: &HqSessionPeer<'_>,
    peers: &HqPeerRegistry,
    reply: &Reply<'_>,
    shipped: &ShippedAttachment,
    bytes: Option<Vec<u8>>,
) -> bool {
    let frame = match fenced(peer, reply, || {
        peer.office.attachment_ingest_commit(shipped, bytes)
    }) {
        None => return false,
        Some(Ok(())) => {
            // The record is bound: every satellite session sends it on now, not at its next
            // poll.
            peers.notify_all();
            reply.ack()
        }
        Some(Err(refusal)) => refusal,
    };
    write_sync_frame_if_peer_active(tls, peer, &frame)
}

fn handle_fetch(
    tls: &mut Tls,
    peer: &HqSessionPeer<'_>,
    request_id: &str,
    org_id: &str,
    attachment_id: &str,
) -> bool {
    let reply = Reply {
        request_id,
        attachment_id,
        content_sha256: "",
    };
    if org_id != peer.org_id.as_str() {
        return write_sync_frame_if_peer_active(
            tls,
            peer,
            &reply.refuse("not_found", "attachment not found"),
        );
    }
    let blob = match fenced(peer, &reply, || {
        peer.office.attachment_peer_fetch(attachment_id)
    }) {
        None => return false,
        Some(Err(refusal)) => return write_sync_frame_if_peer_active(tls, peer, &refusal),
        Some(Ok(blob)) => blob,
    };
    let content_sha256 = blob.content_sha256.to_string();
    let mut offset = 0u64;
    while offset < blob.byte_count {
        // Never more than one chunk, nor more than the file has left.
        let max = usize::try_from(blob.byte_count - offset)
            .unwrap_or(usize::MAX)
            .min(BLOB_CHUNK_MAX_BYTES);
        let piece = match fenced(peer, &reply, || {
            peer.office
                .attachment_peer_range(&blob, offset, max)
                .map(Ok::<_, ApiError>)
        }) {
            None => return false,
            Some(Ok(Some(piece))) if !piece.is_empty() && piece.len() <= max => piece,
            Some(Ok(_)) => {
                return write_sync_frame_if_peer_active(
                    tls,
                    peer,
                    &reply.refuse("unavailable", "HQ could not read the file"),
                )
            }
            Some(Err(refusal)) => return write_sync_frame_if_peer_active(tls, peer, &refusal),
        };
        let frame = SyncFrame::BlobChunk {
            request_id: request_id.to_owned(),
            content_sha256: content_sha256.clone(),
            offset,
            total_bytes: blob.byte_count,
            data_b64: STANDARD.encode(&piece),
        };
        if !write_sync_frame_if_peer_active(tls, peer, &frame) {
            return false;
        }
        offset += piece.len() as u64;
    }
    true
}

#[cfg(test)]
#[path = "hq_blobs_tests.rs"]
mod tests;
