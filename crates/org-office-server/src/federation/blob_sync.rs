//! A satellite's side of shared attachment bytes over federation (`TASK-98.6`, ADR-0037).
//!
//! - **Shipping.** A dedicated thread ([`spawn_satellite_blob_shipper`]) opens its own mTLS
//!   session whenever the office's outbox holds files ready to go (and on every reconnect of
//!   the persistent session), so a large upload never stalls messaging. For each file it reads
//!   and checks the bytes here first (a missing or damaged one is skipped, never holding up the
//!   rest), sends `BlobHasRequest`, streams `BlobChunk`s unless HQ already bound the file, and
//!   drops the outbox entry only on an accepted `BlobAck` for exactly that file. A shutdown
//!   ends the pass within one socket read slice (250 ms), between files, chunks and replies.
//! - **Pulling.** [`HqBlobRemote`] is the office's [`BlobRemote`]: one mTLS session per pull,
//!   one `BlobFetchRequest` by `(org_id, attachment_id)`, the chunks reassembled and verified
//!   against the record's SHA-256 before the office caches them.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use org_office::attachments::blobs::ContentHash;
use org_office::attachments::cache::DEFAULT_BLOB_CACHE_MAX_BYTES;
use org_office::attachments::remote::{BlobRemote, OutboxShipment, PullFailure};
use org_office::Office;
use org_sync_proto::{blob_chunk_frames, BlobAssembler, SyncFrame, BLOB_TRANSFER_MAX_BYTES};

use super::SatelliteSyncSession;
use crate::federation_stream::SatSyncWake;

/// How long HQ may take to answer one step of a transfer (checking and keeping a 25 MiB
/// document included).
const BLOB_REPLY_DEADLINE: Duration = Duration::from_secs(60);
/// How often the shipper looks at the outbox when nothing woke it.
const SHIP_IDLE_POLL: Duration = Duration::from_secs(2);
/// The longest wait between attempts while HQ cannot take the files.
const SHIP_BACKOFF_MAX: Duration = Duration::from_secs(10);
/// The least time between two failed attempts, however often the office wakes the shipper.
const SHIP_RETRY_MIN: Duration = Duration::from_secs(1);
/// Sets the satellite cache's cap in bytes (default 1 GiB).
pub const BLOB_CACHE_MAX_BYTES_ENV: &str = "AGENTIC_ORG_BLOB_CACHE_MAX_BYTES";

/// The satellite cache's cap: [`BLOB_CACHE_MAX_BYTES_ENV`] when it is a positive number of
/// bytes, else [`DEFAULT_BLOB_CACHE_MAX_BYTES`].
pub fn blob_cache_max_bytes() -> u64 {
    static CAP: OnceLock<u64> = OnceLock::new();
    *CAP.get_or_init(|| {
        std::env::var(BLOB_CACHE_MAX_BYTES_ENV)
            .ok()
            .and_then(|text| text.trim().parse::<u64>().ok())
            .filter(|cap| *cap > 0)
            .unwrap_or(DEFAULT_BLOB_CACHE_MAX_BYTES)
    })
}

/// What one pass over the outbox did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OutboxFlush {
    /// Files HQ acknowledged, now gone from the outbox.
    pub shipped: usize,
    /// Files HQ refused for now; they stay in the outbox.
    pub refused: usize,
}

fn request_id_of(frame: &SyncFrame) -> Option<&str> {
    match frame {
        SyncFrame::BlobHasResponse { request_id, .. }
        | SyncFrame::BlobAck { request_id, .. }
        | SyncFrame::BlobChunk { request_id, .. } => Some(request_id),
        _ => None,
    }
}

fn stopped(stop: Option<&AtomicBool>) -> bool {
    stop.is_some_and(|stop| stop.load(Ordering::Acquire))
}

const CANCELLED: &str = "blob shipping cancelled by shutdown";

impl SatelliteSyncSession {
    /// The next blob frame HQ sends for `request_id`, skipping anything else it pushes; a
    /// shutdown (`stop`) ends the wait within one socket read slice.
    pub(crate) fn read_blob_reply(
        &mut self,
        request_id: &str,
        stop: Option<&AtomicBool>,
    ) -> Result<SyncFrame, String> {
        let deadline = Instant::now() + BLOB_REPLY_DEADLINE;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err("timed out waiting for HQ's blob reply".into());
            }
            match self.read_correlated_frame_cancellable(left, stop)? {
                Some(frame) if request_id_of(&frame) == Some(request_id) => return Ok(frame),
                Some(SyncFrame::Error { code, message }) => {
                    return Err(format!("HQ refused the session ({code}): {message}"));
                }
                Some(_) => {}
                None => return Err("HQ closed the session".into()),
            }
        }
    }

    /// Ships every outbox file ready to go, one after another, until `stop`. A file HQ
    /// refuses, or whose bytes here are missing or damaged, stays in the outbox for a later
    /// pass (counted as refused) and never holds up the files after it; a broken session or a
    /// shutdown ends the pass with an error.
    pub fn flush_blob_outbox(
        &mut self,
        cache_max_bytes: u64,
        stop: Option<&AtomicBool>,
    ) -> Result<OutboxFlush, String> {
        let pending = self
            .office
            .attachment_outbox_pending()
            .map_err(|e| format!("reading the blob outbox: {e}"))?;
        let mut flush = OutboxFlush::default();
        for shipment in pending {
            if stopped(stop) {
                return Err(CANCELLED.into());
            }
            let bytes = match self.office.attachment_outbox_bytes(&shipment) {
                Ok(Some(bytes)) => bytes,
                Ok(None) => {
                    eprintln!(
                        "agentic-org-office: attachment {} waits in the blob outbox without its bytes; skipped",
                        shipment.attachment_id
                    );
                    flush.refused += 1;
                    continue;
                }
                Err(error) => {
                    crate::failure::logged("attachment.ship", &error);
                    flush.refused += 1;
                    continue;
                }
            };
            if self.ship_one(&shipment, &bytes, stop)? {
                self.office
                    .attachment_outbox_acknowledged(&shipment, cache_max_bytes)
                    .map_err(|e| format!("releasing an acknowledged outbox file: {e}"))?;
                flush.shipped += 1;
            } else {
                flush.refused += 1;
            }
        }
        Ok(flush)
    }

    /// Ships one file whose `bytes` were read and checked here; `Ok(true)` only on HQ's
    /// accepted `BlobAck` naming exactly this file.
    fn ship_one(
        &mut self,
        shipment: &OutboxShipment,
        bytes: &[u8],
        stop: Option<&AtomicBool>,
    ) -> Result<bool, String> {
        let request_id = self.office.next_uuid_v7();
        self.write_frame(&SyncFrame::BlobHasRequest {
            request_id: request_id.clone(),
            org_id: self.office.info().org_id.clone(),
            attachment_id: shipment.attachment_id.clone(),
            content_sha256: shipment.content_sha256.clone(),
            byte_count: shipment.byte_count,
            record_b64: STANDARD.encode(&shipment.record_yaml),
        })?;
        match self.read_blob_reply(&request_id, stop)? {
            SyncFrame::BlobHasResponse { present: true, .. } => {}
            SyncFrame::BlobHasResponse { present: false, .. } => {
                for frame in blob_chunk_frames(&request_id, &shipment.content_sha256, bytes) {
                    if stopped(stop) {
                        return Err(CANCELLED.into());
                    }
                    self.write_frame(&frame)?;
                }
            }
            reply => return Ok(is_accepted(&reply, shipment)),
        }
        let reply = self.read_blob_reply(&request_id, stop)?;
        Ok(is_accepted(&reply, shipment))
    }

    /// Fetches one shared file's bytes from HQ by its attachment ID, verified against
    /// `content_sha256` and `byte_count`.
    pub fn fetch_shared_blob(
        &mut self,
        attachment_id: &str,
        content_sha256: &str,
        byte_count: u64,
    ) -> Result<Vec<u8>, PullFailure> {
        let mut assembler = BlobAssembler::new(content_sha256, byte_count, BLOB_TRANSFER_MAX_BYTES)
            .map_err(|e| PullFailure::Corrupt(e.to_string()))?;
        let request_id = self.office.next_uuid_v7();
        self.write_frame(&SyncFrame::BlobFetchRequest {
            request_id: request_id.clone(),
            org_id: self.office.info().org_id.clone(),
            attachment_id: attachment_id.to_owned(),
        })
        .map_err(PullFailure::Unreachable)?;
        loop {
            match self
                .read_blob_reply(&request_id, None)
                .map_err(PullFailure::Unreachable)?
            {
                SyncFrame::BlobChunk {
                    content_sha256,
                    offset,
                    total_bytes,
                    data_b64,
                    ..
                } => match assembler.push(&content_sha256, offset, total_bytes, &data_b64) {
                    Ok(false) => {}
                    Ok(true) => {
                        return assembler
                            .into_verified_bytes()
                            .ok_or_else(|| PullFailure::Corrupt("the blob did not verify".into()))
                    }
                    Err(error) => return Err(PullFailure::Corrupt(error.to_string())),
                },
                SyncFrame::BlobAck { code, message, .. } => {
                    return Err(PullFailure::Refused {
                        code: code.unwrap_or_default(),
                        message: message.unwrap_or_default(),
                    })
                }
                _ => return Err(PullFailure::Corrupt("an unexpected reply".into())),
            }
        }
    }
}

fn is_accepted(reply: &SyncFrame, shipment: &OutboxShipment) -> bool {
    matches!(
        reply,
        SyncFrame::BlobAck {
            accepted: true,
            attachment_id,
            content_sha256,
            ..
        } if *attachment_id == shipment.attachment_id && *content_sha256 == shipment.content_sha256
    )
}

/// The satellite's adapter of the office's [`BlobRemote`] port: one mTLS session per pull.
pub struct HqBlobRemote {
    office: Arc<Office>,
}

impl HqBlobRemote {
    pub fn new(office: Arc<Office>) -> Self {
        Self { office }
    }
}

impl BlobRemote for HqBlobRemote {
    fn fetch(
        &self,
        attachment_id: &str,
        content_sha256: &ContentHash,
        byte_count: u64,
    ) -> Result<Vec<u8>, PullFailure> {
        let mut session = SatelliteSyncSession::connect(Arc::clone(&self.office))
            .map_err(PullFailure::Unreachable)?;
        session.fetch_shared_blob(attachment_id, content_sha256.as_str(), byte_count)
    }
}

/// Ships what the outbox holds over a fresh session; nothing is opened when it is empty.
pub fn ship_blob_outbox(
    office: &Arc<Office>,
    stop: Option<&AtomicBool>,
    cache_max_bytes: u64,
) -> Result<OutboxFlush, String> {
    let ready = office
        .attachment_outbox_pending()
        .map_err(|e| format!("reading the blob outbox: {e}"))?;
    if ready.is_empty() {
        return Ok(OutboxFlush::default());
    }
    let mut session = SatelliteSyncSession::connect_cancellable(Arc::clone(office), stop)?;
    session.flush_blob_outbox(cache_max_bytes, stop)
}

/// Sleeps `wait`, ending early only on `stop`.
fn sleep_or_stop(wait: Duration, stop: &AtomicBool) {
    let deadline = Instant::now() + wait;
    while !stop.load(Ordering::SeqCst) {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return;
        }
        std::thread::sleep(left.min(Duration::from_millis(100)));
    }
}

/// Starts the satellite's outbox shipper (a satellite only): it ships whenever files are
/// ready, waking on the office's sync wake or every [`SHIP_IDLE_POLL`], and backs off while
/// HQ cannot take them.
pub(crate) fn spawn_satellite_blob_shipper(
    office: &Arc<Office>,
    stop: &Arc<AtomicBool>,
    wake: Arc<SatSyncWake>,
) -> Option<JoinHandle<()>> {
    if office.info().role != org_office::OfficeRole::Satellite {
        return None;
    }
    let office = Arc::clone(office);
    let stop = Arc::clone(stop);
    std::thread::Builder::new()
        .name("sat-blob-shipper".into())
        .spawn(move || {
            let mut backoff = SHIP_RETRY_MIN;
            while !stop.load(Ordering::SeqCst) {
                let seen = wake.generation();
                match ship_blob_outbox(&office, Some(&stop), blob_cache_max_bytes()) {
                    Ok(flush) if flush.refused == 0 => {
                        backoff = SHIP_RETRY_MIN;
                        let _ = wake.wait_or_stop(seen, SHIP_IDLE_POLL, &stop);
                    }
                    // Refused files or an unreachable HQ: wait at least a second whatever
                    // wakes the office, then until the next wake or the backoff.
                    _ => {
                        sleep_or_stop(SHIP_RETRY_MIN, &stop);
                        let _ = wake.wait_or_stop(seen, backoff, &stop);
                        backoff = (backoff * 2).min(SHIP_BACKOFF_MAX);
                    }
                }
            }
        })
        .ok()
}
