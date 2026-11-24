//! Bounded, disk-backed, paged organization snapshot transfer over Org-CA mTLS (`TASK-52` P20-#4 / P21-#1 / P21-#2).
//!
//! - Both HQ and satellites enforce identical per-page and per-chunk limits
//!   (`MAX_SNAPSHOT_PAGE_CHUNKS * SNAPSHOT_CHUNK_RAW_BYTES == SNAPSHOT_PAGE_MAX_BYTES`).
//! - Paging is negotiated explicitly via `OrgSnapshotRequest::supports_paging`: legacy peers never
//!   receive page 0 of a multi-page snapshot.
//! - HQ bounds concurrent transfers, process-wide staged disk bytes, and idle lifetimes, streaming
//!   pages to/from disk so idle sessions retain negligible memory.
//! - Satellites validate and stage every page to disk (within a strict byte cap) BEFORE mutating
//!   `state/`, then apply staged pages in dependency order with bounded memory (`<= 8 MiB` per
//!   `org-store` commit).

use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use org_office::{ExportedSnapshotPages, Office, OfficeError, SnapshotExportProgress};
use org_sync::organization::{install_snapshot, ValidatedSnapshot};
use org_sync_proto::{write_sync_frame, Codec, Digest, Epoch, Position, SyncFrame};
use rustls::{ServerConnection, StreamOwned};
use sha2::{Digest as _, Sha256};

use crate::federation::{
    collect_pending_work_paths_checked, deterministic_uuid_v7, hex_lower,
    write_sync_frame_if_peer_active, HqSessionPeer, SatelliteSyncSession, FIRST_FRAME_DEADLINE,
};

pub(crate) const SNAPSHOT_CHUNK_RAW_BYTES: usize = 256 * 1024;
pub(crate) const SNAPSHOT_PAGE_MAX_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_SNAPSHOT_PAGE_CHUNKS: u32 =
    (SNAPSHOT_PAGE_MAX_BYTES / SNAPSHOT_CHUNK_RAW_BYTES) as u32;
pub(crate) const MAX_SNAPSHOT_PAGES: u32 = 1024;
pub(crate) const MAX_CONCURRENT_SNAPSHOT_TRANSFERS: usize = 4;
pub(crate) const MAX_HQ_STAGED_SNAPSHOT_BYTES: u64 = 512 * 1024 * 1024;
pub(crate) const MAX_SATELLITE_STAGED_SNAPSHOT_BYTES: u64 = 512 * 1024 * 1024;
pub(crate) const SNAPSHOT_TRANSFER_IDLE_TTL: Duration = Duration::from_secs(30);
pub(crate) const SNAPSHOT_PREPARE_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(3);
pub(crate) const SNAPSHOT_PREPARE_DEADLINE: Duration = Duration::from_secs(180);
const SNAPSHOT_PREPARE_LEGACY_POLL: Duration = Duration::from_millis(250);
const MAX_NATIVE_BATCH_BYTES: usize = 8 * 1024 * 1024;

const TIER_BIT_ORG_STRUCT: u16 = 1 << 0;
const TIER_BIT_ORG_EMP: u16 = 1 << 1;

type SnapshotFileBatch = Vec<(String, Option<Vec<u8>>)>;

#[derive(Default)]
struct SnapshotBudgetState {
    active_transfers: usize,
    staged_bytes: u64,
}

/// Process-wide HQ concurrency and staged-disk budget for active snapshot exports (`TASK-52` P21-#2).
#[derive(Clone, Default)]
pub(crate) struct HqSnapshotTransferBudget {
    inner: Arc<Mutex<SnapshotBudgetState>>,
    max_concurrent: usize,
    max_staged_bytes: u64,
}

impl HqSnapshotTransferBudget {
    pub(crate) fn production() -> Self {
        Self::with_limits(
            MAX_CONCURRENT_SNAPSHOT_TRANSFERS,
            MAX_HQ_STAGED_SNAPSHOT_BYTES,
        )
    }

    pub(crate) fn with_limits(max_concurrent: usize, max_staged_bytes: u64) -> Self {
        Self {
            inner: Arc::new(Mutex::new(SnapshotBudgetState::default())),
            max_concurrent,
            max_staged_bytes,
        }
    }

    pub(crate) fn try_begin(&self) -> Option<SnapshotTransferPermit> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if guard.active_transfers >= self.max_concurrent {
            return None;
        }
        guard.active_transfers += 1;
        Some(SnapshotTransferPermit {
            budget: self.clone(),
            reserved_bytes: 0,
        })
    }
}

pub(crate) struct SnapshotTransferPermit {
    budget: HqSnapshotTransferBudget,
    reserved_bytes: u64,
}

impl SnapshotTransferPermit {
    pub(crate) fn try_reserve_bytes(&mut self, bytes: u64) -> bool {
        let mut guard = self.budget.inner.lock().unwrap_or_else(|e| e.into_inner());
        let Some(next) = guard.staged_bytes.checked_add(bytes) else {
            return false;
        };
        if next > self.budget.max_staged_bytes {
            return false;
        }
        guard.staged_bytes = next;
        self.reserved_bytes = self.reserved_bytes.saturating_add(bytes);
        true
    }
}

impl Drop for SnapshotTransferPermit {
    fn drop(&mut self) {
        let mut guard = self.budget.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.active_transfers = guard.active_transfers.saturating_sub(1);
        guard.staged_bytes = guard.staged_bytes.saturating_sub(self.reserved_bytes);
    }
}

/// Disk-backed active HQ snapshot transfer with RAII cleanup and idle expiry.
pub(crate) struct ActiveSnapshotTransfer {
    snapshot_id: String,
    checkpoint: org_sync::organization::OrgCheckpoint,
    staging_dir: PathBuf,
    total_pages: u32,
    last_activity: Instant,
    _permit: SnapshotTransferPermit,
}

impl ActiveSnapshotTransfer {
    pub(crate) fn is_expired(&self) -> bool {
        self.last_activity.elapsed() >= SNAPSHOT_TRANSFER_IDLE_TTL
    }

    pub(crate) fn expire_if_idle(slot: &mut Option<Self>) {
        if slot.as_ref().is_some_and(Self::is_expired) {
            *slot = None;
        }
    }
}

impl Drop for ActiveSnapshotTransfer {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.staging_dir);
    }
}

struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

static ACTIVE_SATELLITE_IMPORTS: OnceLock<Mutex<BTreeSet<PathBuf>>> = OnceLock::new();

fn active_satellite_imports() -> &'static Mutex<BTreeSet<PathBuf>> {
    ACTIVE_SATELLITE_IMPORTS.get_or_init(|| Mutex::new(BTreeSet::new()))
}

/// RAII staging directory for satellite snapshot imports that reconciles abandoned prior import
/// directories before creating a new one (`TASK-52` P22-#3).
struct StagedImportGuard(PathBuf);

fn remove_abandoned_staging_entry(path: &Path) -> Result<(), String> {
    let remove_res = if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    match remove_res {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!(
            "failed reconciling abandoned snapshot staging {}: {err}",
            path.display()
        )),
    }
}

impl StagedImportGuard {
    fn open_reconciled(import_root: &Path, nonce: &str) -> Result<Self, String> {
        let mut active = active_satellite_imports()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        match fs::read_dir(import_root) {
            Ok(entries) => {
                for entry in entries {
                    let entry = entry
                        .map_err(|e| format!("failed scanning snapshot staging directory: {e}"))?;
                    let path = entry.path();
                    if !active.contains(&path) {
                        remove_abandoned_staging_entry(&path)?;
                    }
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(format!("failed reading snapshot staging root: {err}"));
            }
        }
        let staging_dir = import_root.join(nonce);
        create_private_dir(&staging_dir)
            .map_err(|e| format!("failed creating snapshot staging dir: {e}"))?;
        active.insert(staging_dir.clone());
        Ok(Self(staging_dir))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for StagedImportGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
        active_satellite_imports()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.0);
    }
}

/// Removes leftover snapshot export directories under `runtime/snapshot-export` on HQ startup.
pub(crate) fn cleanup_stale_hq_snapshot_exports(office: &Office) {
    let dir = office.runtime_root().join("snapshot-export");
    let _ = fs::remove_dir_all(dir);
}

fn create_private_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(path)
    }
}

fn write_private_bytes(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut opts = fs::OpenOptions::new();
    opts.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(path)?;
    file.write_all(bytes)
}

fn page_file_path(dir: &Path, page_index: u32) -> PathBuf {
    dir.join(format!("page-{page_index:06}.json"))
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

#[derive(Clone, PartialEq, Eq)]
struct SnapshotPageMeta {
    snapshot_id: String,
    total_pages: u32,
    final_epoch: u64,
    final_seq: u64,
    final_digest_hex: String,
    protocol_level: u32,
}

struct AssembledSnapshotPage {
    meta: SnapshotPageMeta,
    sha256_hex: String,
    raw: Vec<u8>,
}

struct SnapshotChunkHeader {
    total_chunks: u32,
    sha256_hex: String,
    meta: SnapshotPageMeta,
}

struct StagedSnapshotImport {
    dir_guard: StagedImportGuard,
    meta: SnapshotPageMeta,
    page_sha256s: Vec<String>,
    page_tiers: Vec<u16>,
    org_core_bytes: usize,
    total_decoded_bytes: usize,
    max_struct_depth: usize,
    total_files: usize,
    incoming_paths: BTreeSet<String>,
    protected_tasks: BTreeSet<String>,
}

fn validate_first_snapshot_page_meta(
    cp: &org_sync::organization::OrgCheckpoint,
    meta: &SnapshotPageMeta,
) -> Result<(), String> {
    if meta.total_pages == 0 || meta.total_pages > MAX_SNAPSHOT_PAGES {
        return Err("invalid snapshot page count".into());
    }
    if meta.final_epoch < cp.highest_epoch.get() {
        return Err("refused stale snapshot with regressed epoch".into());
    }
    if meta.final_epoch == cp.position.epoch.get() && meta.final_seq < cp.position.seq {
        return Err("refused stale snapshot with regressed seq".into());
    }
    if meta.protocol_level != 1 {
        return Err(format!(
            "unsupported snapshot protocol level {}",
            meta.protocol_level
        ));
    }
    Ok(())
}

fn validate_chunk_envelope(
    chunk_index: u32,
    expected_idx: u32,
    total_chunks: u32,
    page_index: u32,
    expected_page_index: u32,
    total_pages: u32,
) -> Result<(), String> {
    let valid = total_chunks > 0
        && total_chunks <= MAX_SNAPSHOT_PAGE_CHUNKS
        && chunk_index == expected_idx
        && page_index == expected_page_index
        && total_pages > 0
        && total_pages <= MAX_SNAPSHOT_PAGES;
    if !valid {
        return Err("invalid or out-of-order snapshot chunk".into());
    }
    Ok(())
}

fn read_next_snapshot_chunk_frame(
    session: &mut SatelliteSyncSession,
    expected_idx: u32,
    prep_started: Instant,
) -> Result<SyncFrame, String> {
    loop {
        match session.read_correlated_frame(FIRST_FRAME_DEADLINE, None)? {
            Some(SyncFrame::Pong { nonce: 0 }) if expected_idx == 0 => {
                if prep_started.elapsed() >= SNAPSHOT_PREPARE_DEADLINE {
                    return Err("timed out waiting for HQ snapshot preparation".into());
                }
            }
            Some(frame @ SyncFrame::OrgSnapshotChunk { .. }) => return Ok(frame),
            _ => return Err("unexpected frame while receiving snapshot chunks".into()),
        }
    }
}

fn receive_snapshot_page_chunks(
    session: &mut SatelliteSyncSession,
    expected_page_index: u32,
) -> Result<AssembledSnapshotPage, String> {
    let prep_started = Instant::now();
    let mut expected_idx = 0u32;
    let mut header: Option<SnapshotChunkHeader> = None;
    let mut raw = Vec::new();
    loop {
        let SyncFrame::OrgSnapshotChunk {
            snapshot_id,
            chunk_index,
            total_chunks,
            sha256_hex,
            data_b64,
            final_epoch,
            final_seq,
            final_digest_hex,
            protocol_level,
            page_index,
            total_pages,
        } = read_next_snapshot_chunk_frame(session, expected_idx, prep_started)?
        else {
            unreachable!();
        };
        validate_chunk_envelope(
            chunk_index,
            expected_idx,
            total_chunks,
            page_index,
            expected_page_index,
            total_pages,
        )?;
        let cur_meta = SnapshotPageMeta {
            snapshot_id,
            total_pages,
            final_epoch,
            final_seq,
            final_digest_hex,
            protocol_level,
        };
        match &header {
            None => {
                header = Some(SnapshotChunkHeader {
                    total_chunks,
                    sha256_hex,
                    meta: cur_meta,
                });
            }
            Some(h)
                if h.total_chunks != total_chunks
                    || h.sha256_hex != sha256_hex
                    || h.meta != cur_meta =>
            {
                return Err("inconsistent snapshot chunk metadata".into());
            }
            Some(_) => {}
        }
        let part = STANDARD
            .decode(&data_b64)
            .map_err(|_| "invalid snapshot chunk base64".to_owned())?;
        if raw.len().saturating_add(part.len()) > SNAPSHOT_PAGE_MAX_BYTES {
            return Err("assembled snapshot page exceeds shared size bound".into());
        }
        raw.extend_from_slice(&part);
        expected_idx += 1;
        if header
            .as_ref()
            .is_some_and(|h| h.total_chunks == expected_idx)
        {
            let h = header.ok_or_else(|| "missing chunk header".to_owned())?;
            return Ok(AssembledSnapshotPage {
                meta: h.meta,
                sha256_hex: h.sha256_hex,
                raw,
            });
        }
    }
}

fn decode_and_validate_snapshot_files(
    encoded_files: Vec<(String, String)>,
) -> Result<SnapshotFileBatch, String> {
    let mut batch = Vec::with_capacity(encoded_files.len());
    for (path, b64) in encoded_files {
        let allowed = path.starts_with("state/")
            || (path.starts_with("messaging/conversations/")
                && path.ends_with("/conversation.json"));
        if !allowed || path.contains("..") {
            return Err(format!("refused unsafe snapshot path `{path}`"));
        }
        let bytes = STANDARD
            .decode(&b64)
            .map_err(|_| format!("invalid base64 for snapshot file {path}"))?;
        batch.push((path, Some(bytes)));
    }
    Ok(batch)
}

struct PageScanAcc {
    page_tiers: Vec<u16>,
    org_core_bytes: usize,
    total_decoded_bytes: usize,
    max_struct_depth: usize,
    total_files: usize,
    incoming_paths: BTreeSet<String>,
    protected_tasks: BTreeSet<String>,
}

fn index_validated_snapshot_page(
    office: &Office,
    page_raw: &[u8],
    acc: &mut PageScanAcc,
) -> Result<(), String> {
    let encoded: Vec<(String, String)> =
        serde_json::from_slice(page_raw).map_err(|e| format!("malformed snapshot payload: {e}"))?;
    let decoded = decode_and_validate_snapshot_files(encoded)?;
    let mut mask = 0u16;
    for (path, opt_bytes) in decoded {
        let Some(bytes) = opt_bytes else {
            continue;
        };
        let entry_bytes = path.len() + bytes.len() + 64;
        acc.total_decoded_bytes = acc.total_decoded_bytes.saturating_add(entry_bytes);
        acc.total_files = acc.total_files.saturating_add(1);
        let (tier, depth) = snapshot_dependency_tier(&path, true);
        if tier < 16 {
            mask |= 1u16 << tier;
        }
        if is_org_core_put(&path, true) {
            acc.org_core_bytes = acc.org_core_bytes.saturating_add(entry_bytes);
            if !is_employee_profile_path(&path) {
                acc.max_struct_depth = acc.max_struct_depth.max(depth);
            }
        }
        if let Some(tid) = org_office::is_stale_snapshot_task_entry(office, &path, &bytes)
            .map_err(|e| format!("failed checking local task revision for {path}: {e}"))?
        {
            acc.protected_tasks.insert(tid);
        }
        acc.incoming_paths.insert(path);
    }
    acc.page_tiers.push(mask);
    Ok(())
}

fn fetch_and_stage_snapshot_pages(
    session: &mut SatelliteSyncSession,
    cp: &org_sync::organization::OrgCheckpoint,
) -> Result<StagedSnapshotImport, String> {
    let nonce = session.office.next_uuid_v7();
    let import_root = session.office.runtime_root().join("snapshot-import");
    // Reconcile any abandoned import directories left by an interrupted/killed satellite restart
    // BEFORE opening a new staging directory (`TASK-52` P22-#3).
    let dir_guard = StagedImportGuard::open_reconciled(&import_root, &nonce)?;
    let mut header: Option<SnapshotPageMeta> = None;
    let mut page_sha256s = Vec::new();
    let mut staged_bytes = 0u64;
    let mut acc = PageScanAcc {
        page_tiers: Vec::new(),
        org_core_bytes: 0,
        total_decoded_bytes: 0,
        max_struct_depth: 0,
        total_files: 0,
        incoming_paths: BTreeSet::new(),
        protected_tasks: BTreeSet::new(),
    };
    let mut page_idx = 0u32;
    loop {
        write_sync_frame(
            &mut session.tls,
            &SyncFrame::OrgSnapshotRequest {
                after_epoch: cp.position.epoch.get(),
                after_seq: cp.position.seq,
                snapshot_id: header.as_ref().map(|m| m.snapshot_id.clone()),
                page_index: page_idx,
                supports_paging: true,
            },
        )
        .map_err(|e| format!("failed sending OrgSnapshotRequest: {e}"))?;
        let page = receive_snapshot_page_chunks(session, page_idx)?;
        match &header {
            None => {
                validate_first_snapshot_page_meta(cp, &page.meta)?;
                header = Some(page.meta);
            }
            Some(expected) if *expected != page.meta => {
                return Err("inconsistent snapshot page metadata".into());
            }
            Some(_) => {}
        }
        if hex_lower(&Sha256::digest(&page.raw)) != page.sha256_hex {
            return Err("snapshot SHA-256 mismatch".into());
        }
        staged_bytes = staged_bytes.saturating_add(page.raw.len() as u64);
        if staged_bytes > MAX_SATELLITE_STAGED_SNAPSHOT_BYTES {
            return Err("staged snapshot exceeds satellite disk budget".into());
        }
        index_validated_snapshot_page(&session.office, &page.raw, &mut acc)?;
        write_private_bytes(&page_file_path(dir_guard.path(), page_idx), &page.raw)
            .map_err(|e| format!("failed staging snapshot page {page_idx}: {e}"))?;
        page_sha256s.push(page.sha256_hex);
        page_idx += 1;
        let expected_total = header.as_ref().map_or(0, |m| m.total_pages);
        if page_idx == expected_total {
            let meta = header.ok_or_else(|| "missing snapshot header".to_owned())?;
            return Ok(StagedSnapshotImport {
                dir_guard,
                meta,
                page_sha256s,
                page_tiers: acc.page_tiers,
                org_core_bytes: acc.org_core_bytes,
                total_decoded_bytes: acc.total_decoded_bytes,
                max_struct_depth: acc.max_struct_depth,
                total_files: acc.total_files,
                incoming_paths: acc.incoming_paths,
                protected_tasks: acc.protected_tasks,
            });
        }
    }
}

struct BoundedBatchCommitter<'a> {
    office: &'a Office,
    hq_office_id: &'a str,
    operation_id: &'a str,
    single_commit_mode: bool,
    cur: SnapshotFileBatch,
    cur_bytes: usize,
    batch_idx: usize,
}

impl<'a> BoundedBatchCommitter<'a> {
    fn new(
        office: &'a Office,
        hq_office_id: &'a str,
        operation_id: &'a str,
        single_commit_mode: bool,
    ) -> Self {
        Self {
            office,
            hq_office_id,
            operation_id,
            single_commit_mode,
            cur: Vec::new(),
            cur_bytes: 0,
            batch_idx: 0,
        }
    }

    fn push(&mut self, item: (String, Option<Vec<u8>>)) -> Result<(), String> {
        let item_bytes = item.0.len() + item.1.as_ref().map_or(0, Vec::len) + 64;
        if !self.cur.is_empty()
            && self.cur_bytes.saturating_add(item_bytes) > MAX_NATIVE_BATCH_BYTES
        {
            self.flush()?;
        }
        self.cur_bytes = self.cur_bytes.saturating_add(item_bytes);
        self.cur.push(item);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), String> {
        if self.cur.is_empty() {
            return Ok(());
        }
        let batch = std::mem::take(&mut self.cur);
        self.cur_bytes = 0;
        let idx = self.batch_idx;
        self.batch_idx += 1;
        let op_id = if self.single_commit_mode && idx == 0 {
            self.operation_id.to_owned()
        } else {
            deterministic_uuid_v7(&format!(
                "snapshot:{}:{}:{idx}",
                self.hq_office_id, self.operation_id
            ))
        };
        self.office
            .apply_replicated_state_files(&op_id, batch)
            .map_err(|e| format!("failed applying snapshot batch {idx} to satellite store: {e}"))
    }
}

fn load_staged_page_files(staging_dir: &Path, page_idx: u32) -> Result<SnapshotFileBatch, String> {
    let raw = fs::read(page_file_path(staging_dir, page_idx))
        .map_err(|e| format!("failed reading staged snapshot page {page_idx}: {e}"))?;
    let encoded: Vec<(String, String)> = serde_json::from_slice(&raw)
        .map_err(|e| format!("malformed staged snapshot page {page_idx}: {e}"))?;
    decode_and_validate_snapshot_files(encoded)
}

fn stream_org_core_pass(
    staged: &StagedSnapshotImport,
    committer: &mut BoundedBatchCommitter<'_>,
    want_employee: bool,
    want_depth: Option<usize>,
    neutralize: bool,
) -> Result<(), String> {
    let bit = if want_employee {
        TIER_BIT_ORG_EMP
    } else {
        TIER_BIT_ORG_STRUCT
    };
    for (idx, mask) in staged.page_tiers.iter().enumerate() {
        if (mask & bit) == 0 {
            continue;
        }
        for (path, opt_bytes) in load_staged_page_files(staged.dir_guard.path(), idx as u32)? {
            let Some(bytes) = opt_bytes else {
                continue;
            };
            if !is_org_core_put(&path, true) || is_employee_profile_path(&path) != want_employee {
                continue;
            }
            if want_depth.is_some_and(|d| path.bytes().filter(|&b| b == b'/').count() != d) {
                continue;
            }
            let payload = if neutralize {
                strip_org_core_employee_refs(&path, &bytes)
            } else {
                bytes
            };
            committer.push((path, Some(payload)))?;
        }
    }
    Ok(())
}

fn apply_staged_org_core(
    staged: &StagedSnapshotImport,
    committer: &mut BoundedBatchCommitter<'_>,
) -> Result<(), String> {
    if staged.org_core_bytes <= MAX_NATIVE_BATCH_BYTES {
        stream_org_core_pass(staged, committer, false, None, false)?;
        stream_org_core_pass(staged, committer, true, None, false)?;
        if !committer.single_commit_mode {
            committer.flush()?;
        }
        return Ok(());
    }
    // Oversized org_core (> 8 MiB): stream 4 cycle-breaking passes from disk without buffering
    // all files in RAM (`TASK-52` P6-#4 / P21-#2).
    for depth in 0..=staged.max_struct_depth {
        stream_org_core_pass(staged, committer, false, Some(depth), true)?;
    }
    committer.flush()?;
    stream_org_core_pass(staged, committer, true, None, true)?;
    committer.flush()?;
    stream_org_core_pass(staged, committer, true, None, false)?;
    committer.flush()?;
    for depth in 0..=staged.max_struct_depth {
        stream_org_core_pass(staged, committer, false, Some(depth), false)?;
    }
    committer.flush()
}

fn apply_staged_non_core_tiers(
    staged: &StagedSnapshotImport,
    pending_work_paths: &BTreeSet<String>,
    protected_tasks: &BTreeSet<String>,
    committer: &mut BoundedBatchCommitter<'_>,
) -> Result<(), String> {
    for tier in 2u8..=6u8 {
        let bit = 1u16 << tier;
        for (idx, mask) in staged.page_tiers.iter().enumerate() {
            if (mask & bit) == 0 {
                continue;
            }
            for (path, opt_bytes) in load_staged_page_files(staged.dir_guard.path(), idx as u32)? {
                let Some(bytes) = opt_bytes else {
                    continue;
                };
                if snapshot_dependency_tier(&path, true).0 != tier {
                    continue;
                }
                if pending_work_paths.contains(&path) {
                    continue;
                }
                if org_office::task_id_for_work_path(&path)
                    .is_some_and(|tid| protected_tasks.contains(&tid))
                {
                    continue;
                }
                committer.push((path, Some(bytes)))?;
            }
        }
        if !committer.single_commit_mode {
            committer.flush()?;
        }
    }
    Ok(())
}

fn apply_staged_deletions(
    office: &Office,
    staged: &StagedSnapshotImport,
    pending_work_paths: &BTreeSet<String>,
    committer: &mut BoundedBatchCommitter<'_>,
) -> Result<(), String> {
    // Propagate local state scan errors so a failed scan can never leave buffered writes
    // uncommitted before checkpoint installation (`TASK-52` P26-#1).
    let local_paths = office.list_local_state_paths().map_err(|e| {
        format!("failed listing local state paths for snapshot reconciliation: {e}")
    })?;
    let mut deletes: Vec<String> = local_paths
        .into_iter()
        .filter(|existing| {
            !staged.incoming_paths.contains(existing)
                && !pending_work_paths.contains(existing)
                && !org_office::is_immutable_state_path(existing)
                && !existing.starts_with("state/work/")
        })
        .collect();
    deletes.sort_by(|a, b| {
        snapshot_dependency_tier(a, false)
            .cmp(&snapshot_dependency_tier(b, false))
            .then_with(|| a.cmp(b))
    });
    for path in deletes {
        committer.push((path, None))?;
    }
    committer.flush()
}

/// Streams, validates, stages to disk, applies in bounded dependency-safe batches, and installs
/// HQ's authoritative snapshot without retaining the whole snapshot in RAM (`TASK-52` P21-#2).
pub(crate) fn sync_initial_snapshot_paged(
    session: &mut SatelliteSyncSession,
    operation_id: &str,
) -> Result<usize, String> {
    let cp = session
        .office
        .replication()
        .org_checkpoint()
        .map_err(|e| e.to_string())?;
    let staged = fetch_and_stage_snapshot_pages(session, &cp)?;
    let final_digest = Digest::parse_hex(&staged.meta.final_digest_hex)
        .map_err(|_| "invalid snapshot final_digest_hex".to_owned())?;
    let final_epoch = Epoch::new(staged.meta.final_epoch.max(1))
        .map_err(|_| "invalid snapshot epoch".to_owned())?;
    let pending_work_paths = collect_pending_work_paths_checked(&session.office)?;
    let mut protected_tasks = staged.protected_tasks.clone();
    for path in &pending_work_paths {
        if let Some(tid) = org_office::task_id_for_work_path(path) {
            protected_tasks.insert(tid);
        }
    }
    let single_commit_mode = staged.total_decoded_bytes <= MAX_NATIVE_BATCH_BYTES
        && staged.org_core_bytes <= MAX_NATIVE_BATCH_BYTES;
    let mut committer = BoundedBatchCommitter::new(
        &session.office,
        session.hq_peer.office_id.as_str(),
        operation_id,
        single_commit_mode,
    );
    apply_staged_org_core(&staged, &mut committer)?;
    apply_staged_non_core_tiers(
        &staged,
        &pending_work_paths,
        &protected_tasks,
        &mut committer,
    )?;
    apply_staged_deletions(
        &session.office,
        &staged,
        &pending_work_paths,
        &mut committer,
    )?;
    // Explicitly require an empty committed buffer before installing the checkpoint (`TASK-52` P26-#1).
    committer.flush()?;

    let manifest = serde_json::to_vec(&staged.page_sha256s)
        .map_err(|e| format!("failed encoding snapshot manifest: {e}"))?;
    session
        .office
        .replication()
        .stage_snapshot(&staged.meta.snapshot_id, vec![Arc::from(manifest)])
        .map_err(|e| e.to_string())?;
    let snap = ValidatedSnapshot {
        org_id: session.hq_peer.org_id.clone(),
        authority: final_epoch,
        position: Position {
            epoch: final_epoch,
            seq: staged.meta.final_seq,
        },
        digest: final_digest,
        codec: Codec::new("org.native.snapshot", 1).map_err(|_| "codec".to_owned())?,
        staging_id: staged.meta.snapshot_id.clone(),
        protocol_level: staged.meta.protocol_level,
    };
    install_snapshot(session.office.replication().as_ref(), snap)
        .map_err(|e| format!("failed installing org-sync snapshot: {e}"))?;
    Ok(staged.total_files)
}

fn is_org_core_put(path: &str, is_put: bool) -> bool {
    is_put
        && matches!(
            org_office::classify_replicated_snapshot_path(path),
            org_office::SnapshotPathClass::OrgCoreStructure
                | org_office::SnapshotPathClass::OrgCoreEmployee
        )
}

fn snapshot_dependency_tier(path: &str, is_put: bool) -> (u8, usize) {
    use org_office::SnapshotPathClass as Class;
    let depth = path.bytes().filter(|&b| b == b'/').count();
    let class = org_office::classify_replicated_snapshot_path(path);
    if !is_put {
        return match class {
            Class::WorkSidecar => (7, depth),
            Class::WorkCore => (8, depth),
            _ => (9, usize::MAX - depth),
        };
    }
    match class {
        Class::OrgCoreStructure => (0, depth),
        Class::OrgCoreEmployee => (1, depth),
        Class::ProvidersAndHires => (2, depth),
        Class::ModelProfilesAndProjects => (3, depth),
        Class::DependentExecutionDocsAndBudgets => (4, depth),
        Class::WorkCore => (5, depth),
        Class::WorkSidecar => (6, depth),
    }
}

fn is_employee_profile_path(path: &str) -> bool {
    org_office::classify_replicated_snapshot_path(path)
        == org_office::SnapshotPathClass::OrgCoreEmployee
}

fn strip_org_core_employee_refs(path: &str, bytes: &[u8]) -> Vec<u8> {
    if let Some(typed) = org_office::strip_org_core_employee_refs_typed(path, bytes) {
        return typed;
    }
    let is_markdown = path.ends_with(".md");
    let Ok(text) = std::str::from_utf8(bytes) else {
        return bytes.to_vec();
    };
    let mut out = String::with_capacity(text.len());
    let mut in_frontmatter = !is_markdown;
    let mut seen_open = false;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if is_markdown && trimmed == "---" {
            out.push_str(line);
            if !seen_open {
                seen_open = true;
                in_frontmatter = true;
            } else {
                in_frontmatter = false;
            }
            continue;
        }
        if in_frontmatter {
            let indent_len = trimmed.len() - trimmed.trim_start().len();
            let after_ws = &trimmed[indent_len..];
            let (prefix, key_line) = if let Some(rest) = after_ws.strip_prefix("- ") {
                (&trimmed[..indent_len + 2], rest)
            } else {
                (&trimmed[..indent_len], after_ws)
            };
            if let Some(replacement) = rewrite_cycle_breaking_yaml_line(key_line) {
                out.push_str(prefix);
                out.push_str(replacement);
                if line.ends_with("\r\n") {
                    out.push_str("\r\n");
                } else if line.ends_with('\n') {
                    out.push('\n');
                }
                continue;
            }
        }
        out.push_str(line);
    }
    out.into_bytes()
}

fn rewrite_cycle_breaking_yaml_line(trimmed: &str) -> Option<&'static str> {
    if trimmed.starts_with("lead_employee_id:") {
        Some("lead_employee_id: null")
    } else if trimmed.starts_with("reports_to_employee_id:") {
        Some("reports_to_employee_id: null")
    } else if trimmed.starts_with("created_by_kind:") {
        Some("created_by_kind: \"system\"")
    } else if trimmed.starts_with("created_by_id:") {
        Some("created_by_id: null")
    } else if trimmed.starts_with("updated_by_kind:") {
        Some("updated_by_kind: \"system\"")
    } else if trimmed.starts_with("updated_by_id:") {
        Some("updated_by_id: null")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_enforces_concurrency_and_staged_bytes_and_releases_on_drop() {
        let budget = HqSnapshotTransferBudget::with_limits(2, 1000);
        let mut p1 = budget.try_begin().expect("first permit");
        let mut p2 = budget.try_begin().expect("second permit");
        assert!(budget.try_begin().is_none(), "third permit must fail");

        assert!(p1.try_reserve_bytes(600));
        assert!(p2.try_reserve_bytes(400));
        assert!(!p2.try_reserve_bytes(1), "byte cap must reject overflow");

        drop(p1);
        let mut p3 = budget.try_begin().expect("released concurrency slot");
        assert!(p3.try_reserve_bytes(600), "released bytes available again");
        assert!(!p3.try_reserve_bytes(1));
    }

    #[test]
    fn legacy_request_without_supports_paging_deserializes_false() {
        let legacy_json = r#"{"type":"org_snapshot_request","after_epoch":1,"after_seq":0}"#;
        let frame: SyncFrame = serde_json::from_str(legacy_json).unwrap();
        assert!(matches!(
            frame,
            SyncFrame::OrgSnapshotRequest {
                supports_paging: false,
                page_index: 0,
                snapshot_id: None,
                ..
            }
        ));
    }

    #[test]
    fn shared_page_and_chunk_bounds_match_identically() {
        assert_eq!(
            (MAX_SNAPSHOT_PAGE_CHUNKS as usize) * SNAPSHOT_CHUNK_RAW_BYTES,
            SNAPSHOT_PAGE_MAX_BYTES
        );
        assert!(SNAPSHOT_PREPARE_KEEPALIVE_INTERVAL < FIRST_FRAME_DEADLINE);
        assert!(SNAPSHOT_PREPARE_DEADLINE > FIRST_FRAME_DEADLINE);
    }

    #[test]
    fn staged_import_guard_reconciles_abandoned_dirs_without_removing_active_siblings() {
        let tmp = tempfile::tempdir().unwrap();
        let import_root = tmp.path().join("snapshot-import");
        let abandoned = import_root.join("abandoned-from-crash");
        create_private_dir(&abandoned).unwrap();
        write_private_bytes(&abandoned.join("page-000000.json"), b"[]").unwrap();

        let active_a = StagedImportGuard::open_reconciled(&import_root, "run-a").unwrap();
        assert!(
            !abandoned.exists(),
            "abandoned prior import directory must be purged on next import"
        );
        assert!(active_a.path().exists());

        // Opening a second import while `active_a` is live must NOT remove `active_a`.
        let active_b = StagedImportGuard::open_reconciled(&import_root, "run-b").unwrap();
        assert!(active_a.path().exists());
        assert!(active_b.path().exists());

        let path_a = active_a.path().to_path_buf();
        let path_b = active_b.path().to_path_buf();
        drop(active_a);
        drop(active_b);
        assert!(!path_a.exists());
        assert!(!path_b.exists());
    }

    #[cfg(unix)]
    #[test]
    fn staged_import_guard_fails_closed_when_abandoned_cleanup_fails() {
        use std::os::unix::fs::PermissionsExt as _;
        let tmp = tempfile::tempdir().unwrap();
        let import_root = tmp.path().join("snapshot-import");
        let stuck = import_root.join("stuck-abandoned");
        create_private_dir(&stuck).unwrap();
        write_private_bytes(&stuck.join("page-000000.json"), b"[]").unwrap();
        // Make the abandoned child directory non-writable so `remove_dir_all` fails.
        fs::set_permissions(&stuck, fs::Permissions::from_mode(0o500)).unwrap();

        let err = StagedImportGuard::open_reconciled(&import_root, "next-run")
            .err()
            .expect("failed abandoned staging cleanup must fail closed");
        assert!(
            err.contains("failed reconciling abandoned snapshot staging"),
            "unexpected error: {err}"
        );

        // Restore mode so tempdir cleanup succeeds.
        fs::set_permissions(&stuck, fs::Permissions::from_mode(0o700)).unwrap();
    }
}
