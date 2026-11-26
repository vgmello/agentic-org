//! The satellite side of a paged snapshot: receiving and validating page chunks, and staging the
//! pages in a private, exclusively-held import directory.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use crate::federation_link::{hex_lower, SatelliteSyncSession, FIRST_FRAME_DEADLINE};
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use org_office::Office;
use org_sync_proto::{write_sync_frame, SyncFrame};
use sha2::{Digest as _, Sha256};

use super::tiers::{is_employee_profile_path, is_org_core_put, snapshot_dependency_tier};
use super::{
    create_private_dir, page_file_path, write_private_bytes, SnapshotFileBatch,
    MAX_SATELLITE_STAGED_SNAPSHOT_BYTES, MAX_SNAPSHOT_PAGES, MAX_SNAPSHOT_PAGE_CHUNKS,
    SNAPSHOT_PAGE_MAX_BYTES, SNAPSHOT_PREPARE_DEADLINE,
};

static ACTIVE_SATELLITE_IMPORTS: OnceLock<Mutex<BTreeSet<PathBuf>>> = OnceLock::new();

fn active_satellite_imports() -> &'static Mutex<BTreeSet<PathBuf>> {
    ACTIVE_SATELLITE_IMPORTS.get_or_init(|| Mutex::new(BTreeSet::new()))
}

/// RAII staging directory for satellite snapshot imports that reconciles abandoned prior import
/// directories before creating a new one (`TASK-52` P22-#3).
pub(super) struct StagedImportGuard(PathBuf);

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
    pub(super) fn open_reconciled(import_root: &Path, nonce: &str) -> Result<Self, String> {
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

    pub(super) fn path(&self) -> &Path {
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

#[derive(Clone, PartialEq, Eq)]
pub(super) struct SnapshotPageMeta {
    pub(super) snapshot_id: String,
    total_pages: u32,
    pub(super) final_epoch: u64,
    pub(super) final_seq: u64,
    pub(super) final_digest_hex: String,
    pub(super) protocol_level: u32,
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

pub(super) struct StagedSnapshotImport {
    pub(super) dir_guard: StagedImportGuard,
    pub(super) meta: SnapshotPageMeta,
    pub(super) page_sha256s: Vec<String>,
    pub(super) page_tiers: Vec<u16>,
    pub(super) org_core_bytes: usize,
    pub(super) total_decoded_bytes: usize,
    pub(super) max_struct_depth: usize,
    pub(super) total_files: usize,
    pub(super) incoming_paths: BTreeSet<String>,
    pub(super) protected_tasks: BTreeSet<String>,
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

pub(super) fn decode_and_validate_snapshot_files(
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

pub(super) fn fetch_and_stage_snapshot_pages(
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
