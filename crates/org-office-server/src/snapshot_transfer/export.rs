//! The HQ side: exporting snapshot pages with prepare keepalives, and sending them in bounded chunks.

use std::fs;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Instant;

use crate::federation_link::{hex_lower, write_sync_frame_if_peer_active, HqSessionPeer};
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use org_office::{ExportedSnapshotPages, Office, OfficeError, SnapshotExportProgress};
use org_sync_proto::SyncFrame;
use rustls::{ServerConnection, StreamOwned};
use sha2::{Digest as _, Sha256};

use super::budget::{ActiveSnapshotTransfer, HqSnapshotTransferBudget, SnapshotTransferPermit};
use super::{
    page_file_path, MAX_SNAPSHOT_PAGES, MAX_SNAPSHOT_PAGE_CHUNKS, SNAPSHOT_CHUNK_RAW_BYTES,
    SNAPSHOT_PAGE_MAX_BYTES, SNAPSHOT_PREPARE_DEADLINE, SNAPSHOT_PREPARE_KEEPALIVE_INTERVAL,
    SNAPSHOT_PREPARE_LEGACY_POLL,
};

struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Removes leftover snapshot export directories under `runtime/snapshot-export` on HQ startup.
pub(crate) fn cleanup_stale_hq_snapshot_exports(office: &Office) {
    let dir = office.runtime_root().join("snapshot-export");
    let _ = fs::remove_dir_all(dir);
}

fn send_hq_snapshot_page(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    peer: &HqSessionPeer<'_>,
    transfer: &ActiveSnapshotTransfer,
    page_index: u32,
) -> bool {
    if page_index >= transfer.total_pages {
        return false;
    }
    let Ok(raw) = fs::read(page_file_path(&transfer.staging_dir, page_index)) else {
        return false;
    };
    if raw.len() > SNAPSHOT_PAGE_MAX_BYTES {
        return false;
    }
    let sha256_hex = hex_lower(&Sha256::digest(&raw));
    let slices: Vec<&[u8]> = if raw.is_empty() {
        vec![&[]]
    } else {
        raw.chunks(SNAPSHOT_CHUNK_RAW_BYTES).collect()
    };
    let Ok(total_chunks) = u32::try_from(slices.len()) else {
        return false;
    };
    if total_chunks > MAX_SNAPSHOT_PAGE_CHUNKS {
        return false;
    }
    let cp = &transfer.checkpoint;
    for (idx, part) in slices.into_iter().enumerate() {
        let frame = SyncFrame::OrgSnapshotChunk {
            snapshot_id: transfer.snapshot_id.clone(),
            chunk_index: idx as u32,
            total_chunks,
            sha256_hex: sha256_hex.clone(),
            data_b64: STANDARD.encode(part),
            final_epoch: cp.position.epoch.get(),
            final_seq: cp.position.seq,
            final_digest_hex: cp.digest.to_hex(),
            protocol_level: cp.protocol_level,
            page_index,
            total_pages: transfer.total_pages,
        };
        if !write_sync_frame_if_peer_active(tls, peer, &frame) {
            return false;
        }
    }
    true
}

enum PrepareExportOutcome {
    Ready(Box<ExportedSnapshotPages>),
    ExportFailed,
    SessionClosed,
}

fn check_prepare_tick_and_keepalive(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    peer: &HqSessionPeer<'_>,
    supports_paging: bool,
    prep_started: Instant,
    last_keepalive: &mut Instant,
) -> Option<PrepareExportOutcome> {
    if prep_started.elapsed() >= SNAPSHOT_PREPARE_DEADLINE {
        return Some(PrepareExportOutcome::ExportFailed);
    }
    let peer_stopped = peer.server_stop.load(Ordering::Acquire)
        || peer.session_stop.load(Ordering::Acquire)
        || org_office::is_peer_revoked(
            peer.office,
            peer.sat_id.as_str(),
            Some(peer.cert_serial_hex),
        )
        .unwrap_or(true);
    if peer_stopped {
        return Some(PrepareExportOutcome::SessionClosed);
    }
    if supports_paging && last_keepalive.elapsed() >= SNAPSHOT_PREPARE_KEEPALIVE_INTERVAL {
        if !write_sync_frame_if_peer_active(tls, peer, &SyncFrame::Pong { nonce: 0 }) {
            return Some(PrepareExportOutcome::SessionClosed);
        }
        *last_keepalive = Instant::now();
    }
    None
}

fn export_snapshot_pages_with_keepalive(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    peer: &HqSessionPeer<'_>,
    staging_dir: &Path,
    permit: &mut SnapshotTransferPermit,
    supports_paging: bool,
) -> PrepareExportOutcome {
    let cancel = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let (tx, rx) = mpsc::channel::<Result<ExportedSnapshotPages, OfficeError>>();
        let cancel_ref = &cancel;
        let session_stop_ref = peer.session_stop;
        let server_stop_ref = peer.server_stop;
        let office_ref = peer.office;
        scope.spawn(move || {
            let res = org_office::export_authoritative_state_snapshot_pages(
                office_ref,
                staging_dir,
                SNAPSHOT_PAGE_MAX_BYTES,
                MAX_SNAPSHOT_PAGES,
                |ev| {
                    if cancel_ref.load(Ordering::Acquire)
                        || session_stop_ref.load(Ordering::Acquire)
                        || server_stop_ref.load(Ordering::Acquire)
                    {
                        return false;
                    }
                    match ev {
                        SnapshotExportProgress::Heartbeat => true,
                        SnapshotExportProgress::ReservePageBytes(bytes) => {
                            permit.try_reserve_bytes(bytes)
                        }
                    }
                },
            );
            let _ = tx.send(res);
        });
        let prep_started = Instant::now();
        let mut last_keepalive = Instant::now();
        loop {
            match rx.recv_timeout(SNAPSHOT_PREPARE_LEGACY_POLL) {
                Ok(Ok(exported)) => return PrepareExportOutcome::Ready(Box::new(exported)),
                Ok(Err(_)) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return PrepareExportOutcome::ExportFailed;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if let Some(abort) = check_prepare_tick_and_keepalive(
                        tls,
                        peer,
                        supports_paging,
                        prep_started,
                        &mut last_keepalive,
                    ) {
                        cancel.store(true, Ordering::Release);
                        return abort;
                    }
                }
            }
        }
    })
}

fn begin_hq_snapshot_transfer(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    peer: &HqSessionPeer<'_>,
    budget: &HqSnapshotTransferBudget,
    supports_paging: bool,
) -> Result<Option<ActiveSnapshotTransfer>, bool> {
    let Some(mut permit) = budget.try_begin() else {
        let ok = write_sync_frame_if_peer_active(
            tls,
            peer,
            &SyncFrame::Error {
                code: "unavailable".into(),
                message: "HQ snapshot transfer budget busy; retry later".into(),
            },
        );
        return Err(ok);
    };
    let nonce = peer.office.next_uuid_v7();
    let staging_dir = peer
        .office
        .runtime_root()
        .join("snapshot-export")
        .join(&nonce);
    let temp_guard = TempDirGuard(staging_dir.clone());
    let exported = match export_snapshot_pages_with_keepalive(
        tls,
        peer,
        &staging_dir,
        &mut permit,
        supports_paging,
    ) {
        PrepareExportOutcome::Ready(exp) => *exp,
        PrepareExportOutcome::ExportFailed => {
            let ok = write_sync_frame_if_peer_active(
                tls,
                peer,
                &SyncFrame::Error {
                    code: "unavailable".into(),
                    message: "HQ snapshot export exceeded bounds or failed".into(),
                },
            );
            return Err(ok);
        }
        PrepareExportOutcome::SessionClosed => return Err(false),
    };
    // Refuse legacy non-paging peers BEFORE sending any chunk when snapshot spans > 1 page
    // (`TASK-52` P21-#1).
    if !supports_paging && exported.total_pages > 1 {
        let _ = write_sync_frame_if_peer_active(
            tls,
            peer,
            &SyncFrame::Error {
                code: "unsupported_version".into(),
                message: "snapshot requires paged transfer; upgrade satellite".into(),
            },
        );
        return Err(false);
    }
    std::mem::forget(temp_guard);
    let short = &exported.manifest_sha256_hex[..16.min(exported.manifest_sha256_hex.len())];
    let snapshot_id = format!(
        "snap{}s{}h{}",
        exported.checkpoint.position.epoch.get(),
        exported.checkpoint.position.seq,
        short
    );
    Ok(Some(ActiveSnapshotTransfer {
        snapshot_id,
        checkpoint: exported.checkpoint,
        staging_dir,
        total_pages: exported.total_pages,
        last_activity: Instant::now(),
        _permit: permit,
    }))
}

pub(crate) fn send_hq_snapshot(
    tls: &mut StreamOwned<ServerConnection, TcpStream>,
    peer: &HqSessionPeer<'_>,
    budget: &HqSnapshotTransferBudget,
    active_snapshot: &mut Option<ActiveSnapshotTransfer>,
    requested_snapshot_id: Option<&str>,
    page_index: u32,
    supports_paging: bool,
) -> bool {
    if page_index == 0 && requested_snapshot_id.is_none() {
        *active_snapshot = None;
        match begin_hq_snapshot_transfer(tls, peer, budget, supports_paging) {
            Ok(transfer) => *active_snapshot = transfer,
            Err(keep_open) => return keep_open,
        }
    } else if !supports_paging {
        *active_snapshot = None;
        return false;
    }
    let Some(transfer) = active_snapshot.as_mut() else {
        return false;
    };
    if transfer.is_expired() || requested_snapshot_id.is_some_and(|sid| sid != transfer.snapshot_id)
    {
        *active_snapshot = None;
        return false;
    }
    let ok = send_hq_snapshot_page(tls, peer, transfer, page_index);
    if !ok || page_index.saturating_add(1) >= transfer.total_pages {
        *active_snapshot = None;
    } else {
        // Start the idle window AFTER the page finishes transmitting so slow links never expire
        // an active transfer mid-stream (`TASK-52` P22-#1).
        transfer.last_activity = Instant::now();
    }
    ok
}
