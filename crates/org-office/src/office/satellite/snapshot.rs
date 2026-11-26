//! Authoritative `state/` snapshot capture and the cancellable paged snapshot export for
//! joining satellites.

use std::path::{Path, PathBuf};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use org_store::{DurablePath, Files};

use crate::error::OfficeError;
use crate::office::{storage, Office};

use super::replication::{sync_org_journal_locked_up_to, ConsistentStateSnapshot};

impl Office {
    /// Captures HQ's checkpoint and `state/` snapshot at the exact same `org-store` materialization
    /// boundary so concurrent writers can never interleave between checkpoint and walk (`TASK-52` H-11).
    pub fn collect_authoritative_state_snapshot_consistent(
        &self,
    ) -> Result<ConsistentStateSnapshot, OfficeError> {
        let _guard = self
            .federation_io_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut captured = None;
        let mut capture_err: Option<OfficeError> = None;
        self.store
            .with_consistent_materialized(|mat| {
                let res = (|| -> Result<ConsistentStateSnapshot, OfficeError> {
                    let _ = sync_org_journal_locked_up_to(self, mat)?;
                    let checkpoint = self.replication.org_checkpoint()?;
                    let files = walk_state_files(&self.durable)?;
                    Ok(ConsistentStateSnapshot { checkpoint, files })
                })();
                match res {
                    Ok(snap) => captured = Some(snap),
                    Err(e) => capture_err = Some(e),
                }
                Ok(())
            })
            .map_err(storage)?;
        if let Some(err) = capture_err {
            return Err(err);
        }
        captured.ok_or(OfficeError::Invalid("snapshot capture missing result"))
    }

    /// Lists all current `state/` paths on this office (used by satellites during D25 snapshot
    /// installation to delete stale files absent from a replacement HQ, `TASK-52` P2-8).
    pub fn list_local_state_paths(&self) -> Result<Vec<String>, OfficeError> {
        self.store.sync().map_err(storage)?;
        let files = Files::open(&self.durable).map_err(OfficeError::storage)?;
        let state_prefix = DurablePath::parse("state").map_err(storage)?;
        let mut out = Vec::new();
        for path in files.walk(&state_prefix).map_err(OfficeError::storage)? {
            let s = path.as_str();
            if s.starts_with("state/") {
                out.push(s.to_owned());
            }
        }
        out.sort();
        Ok(out)
    }

    /// Collects all current authoritative `state/` files from HQ's durable tree for initial
    /// snapshot streaming to a joining satellite (`TASK-52`).
    pub fn collect_authoritative_state_snapshot(
        &self,
    ) -> Result<Vec<(String, Vec<u8>)>, OfficeError> {
        self.store.sync().map_err(storage)?;
        let files = Files::open(&self.durable).map_err(OfficeError::storage)?;
        let state_prefix = DurablePath::parse("state").map_err(storage)?;
        let mut entries = Vec::new();
        for path in files.walk(&state_prefix).map_err(OfficeError::storage)? {
            let s = path.as_str();
            if !s.starts_with("state/") {
                continue;
            }
            if let Some(bytes) = files.read(&path).map_err(OfficeError::storage)? {
                entries.push((s.to_owned(), bytes.to_vec()));
            }
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(entries)
    }
}

/// Progress and disk-budget events emitted by [`export_authoritative_state_snapshot_pages`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotExportProgress {
    /// Periodic cooperative cancellation / keepalive check while scanning files.
    Heartbeat,
    /// Requests reservation of `bytes` in the caller's staging budget before writing a page.
    ReservePageBytes(u64),
}

/// Metadata returned by [`export_authoritative_state_snapshot_pages`] after streaming a consistent
/// HQ snapshot into bounded JSON page files on disk.
pub struct ExportedSnapshotPages {
    pub checkpoint: org_sync::organization::OrgCheckpoint,
    pub total_pages: u32,
    pub total_bytes: u64,
    pub manifest_sha256_hex: String,
}

fn write_ephemeral_private_file(path: &Path, buf: &[u8]) -> std::io::Result<()> {
    org_platform::fs::write_owner_only_file(path, buf)
}

fn flush_exported_snapshot_page(
    staging_dir: &Path,
    page_idx: u32,
    max_pages: u32,
    buf: &[u8],
    total_bytes: &mut u64,
    manifest_inputs: &mut Vec<u8>,
    on_progress: &mut impl FnMut(SnapshotExportProgress) -> bool,
) -> Result<(), OfficeError> {
    if page_idx >= max_pages {
        return Err(OfficeError::Invalid("snapshot exceeds maximum page count"));
    }
    let len = buf.len() as u64;
    if !on_progress(SnapshotExportProgress::ReservePageBytes(len)) {
        return Err(OfficeError::Invalid("snapshot staging budget exceeded"));
    }
    *total_bytes = total_bytes.saturating_add(len);
    manifest_inputs.extend_from_slice(org_pki::cert::cert_fingerprint_hex(buf).as_bytes());
    let path = staging_dir.join(format!("page-{page_idx:06}.json"));
    // Ephemeral `runtime/` staging files are rebuilt on retry and removed on startup/drop, so
    // private `0o600` unfsynced writes avoid per-page disk-sync stalls (`TASK-52` P22-#2).
    write_ephemeral_private_file(&path, buf).map_err(OfficeError::storage)
}

fn check_export_heartbeat(
    seen: &mut usize,
    on_progress: &mut impl FnMut(SnapshotExportProgress) -> bool,
) -> Result<(), OfficeError> {
    *seen = seen.saturating_add(1);
    if (*seen).is_multiple_of(32) && !on_progress(SnapshotExportProgress::Heartbeat) {
        return Err(OfficeError::Invalid("snapshot export cancelled"));
    }
    Ok(())
}

fn probe_real_dir_chain(root: &Path, components: &[&str]) -> Result<Option<PathBuf>, OfficeError> {
    // Validate `root` (`durable/`) itself first via `symlink_metadata` so a missing or
    // symlinked durable root fails closed instead of treating missing children as empty
    // (`TASK-52` P30-#1).
    let root_meta = std::fs::symlink_metadata(root).map_err(OfficeError::storage)?;
    if !root_meta.is_dir() {
        return Err(OfficeError::Invalid("durable root is not a real directory"));
    }
    let mut cur = root.to_path_buf();
    for comp in components {
        cur.push(comp);
        match std::fs::symlink_metadata(&cur) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => {
                return Err(OfficeError::Invalid(
                    "durable snapshot directory component is not a real directory",
                ));
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(OfficeError::storage(err)),
        }
    }
    Ok(Some(cur))
}

fn walk_state_paths_cancellable(
    durable: &Path,
    on_progress: &mut impl FnMut(SnapshotExportProgress) -> bool,
) -> Result<Vec<DurablePath>, OfficeError> {
    // Use fallible component-wise `symlink_metadata` so I/O, permission, or symlink errors fail
    // closed rather than publishing an empty snapshot (`TASK-52` P26-#2 / P29-#1).
    let Some(state_root) = probe_real_dir_chain(durable, &["state"])? else {
        return Ok(Vec::new());
    };
    let state_prefix = DurablePath::parse("state").map_err(storage)?;
    let mut paths = Vec::new();
    let mut stack = vec![(state_root, state_prefix)];
    let mut seen = 0usize;
    while let Some((dir, prefix)) = stack.pop() {
        check_export_heartbeat(&mut seen, on_progress)?;
        let entries = std::fs::read_dir(&dir).map_err(OfficeError::storage)?;
        for entry in entries {
            check_export_heartbeat(&mut seen, on_progress)?;
            let entry = entry.map_err(OfficeError::storage)?;
            let ft = entry.file_type().map_err(OfficeError::storage)?;
            let name = entry.file_name();
            let name_str = name
                .to_str()
                .ok_or(OfficeError::Invalid("non-UTF-8 entry under state/"))?;
            if ft.is_file() && name_str.starts_with(".store-tmp-") {
                continue;
            }
            // Build `DurablePath` from validated filesystem components (matching `Files::walk`)
            // without rewriting separators, and enforce `DurablePath::MAX_BYTES` so HQ never
            // exports a path that `DurablePath::parse` rejects (`TASK-52` P27-#2 / P28-#2).
            let dp = prefix.join(name_str).map_err(storage)?;
            let dp = DurablePath::parse(dp.as_str()).map_err(storage)?;
            let full = entry.path();
            if ft.is_dir() {
                stack.push((full, dp));
            } else if ft.is_file() {
                paths.push(dp);
            } else {
                return Err(OfficeError::Invalid(
                    "unexpected non-file entry under state/",
                ));
            }
        }
    }
    Ok(paths)
}

fn append_channel_snapshot_paths_cancellable(
    durable: &Path,
    files: &Files,
    paths: &mut Vec<DurablePath>,
    on_progress: &mut impl FnMut(SnapshotExportProgress) -> bool,
) -> Result<(), OfficeError> {
    // Validate BOTH `durable/messaging` and `durable/messaging/conversations` as real
    // directories via `symlink_metadata` before accepting absence (`TASK-52` P28-#1 / P29-#1).
    let Some(convs_root) = probe_real_dir_chain(durable, &["messaging", "conversations"])? else {
        return Ok(());
    };
    let entries = std::fs::read_dir(&convs_root).map_err(OfficeError::storage)?;
    let convs_prefix = DurablePath::parse("messaging/conversations").map_err(storage)?;
    let mut seen = 0usize;
    for entry in entries {
        // Check cancellation on every conversation directory without descending into
        // `messages/` or `answers/` history, rejecting symlinks/unexpected entries and
        // propagating every I/O or decode error (`TASK-52` P24-#2 / P25-#1 / P28-#1 / P28-#2).
        check_export_heartbeat(&mut seen, on_progress)?;
        let entry = entry.map_err(OfficeError::storage)?;
        let ft = entry.file_type().map_err(OfficeError::storage)?;
        let cid = entry.file_name();
        let cid_str = cid
            .to_str()
            .ok_or(OfficeError::Invalid("non-UTF-8 conversation directory"))?;
        if ft.is_file() && cid_str.starts_with(".store-tmp-") {
            continue;
        }
        if !ft.is_dir() {
            return Err(OfficeError::Invalid(
                "unexpected non-directory entry under messaging/conversations",
            ));
        }
        let dp = convs_prefix
            .join(cid_str)
            .and_then(|d| d.join("conversation.json"))
            .map_err(storage)?;
        let dp = DurablePath::parse(dp.as_str()).map_err(storage)?;
        if !on_progress(SnapshotExportProgress::Heartbeat) {
            return Err(OfficeError::Invalid("snapshot export cancelled"));
        }
        let Some(bytes) = files.read(&dp).map_err(OfficeError::storage)? else {
            continue;
        };
        let conv: crate::messaging::records::ConversationRecord = serde_json::from_slice(&bytes)
            .map_err(|_| OfficeError::Invalid("malformed conversation.json in snapshot export"))?;
        if conv.kind == org_proto::chat::ConversationKind::Channel {
            paths.push(dp);
        }
    }
    Ok(())
}

fn collect_authoritative_snapshot_paths(
    durable: &Path,
    files: &Files,
    on_progress: &mut impl FnMut(SnapshotExportProgress) -> bool,
) -> Result<Vec<DurablePath>, OfficeError> {
    let mut paths = walk_state_paths_cancellable(durable, on_progress)?;
    append_channel_snapshot_paths_cancellable(durable, files, &mut paths, on_progress)?;
    if !on_progress(SnapshotExportProgress::Heartbeat) {
        return Err(OfficeError::Invalid("snapshot export cancelled"));
    }
    paths.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    Ok(paths)
}

fn acquire_snapshot_export_lock_cancellable<'a>(
    office: &'a Office,
    on_progress: &mut impl FnMut(SnapshotExportProgress) -> bool,
) -> Result<std::sync::MutexGuard<'a, ()>, OfficeError> {
    loop {
        if !on_progress(SnapshotExportProgress::Heartbeat) {
            return Err(OfficeError::Invalid("snapshot export cancelled"));
        }
        match office.snapshot_export_lock.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(std::sync::TryLockError::Poisoned(p)) => return Ok(p.into_inner()),
            Err(std::sync::TryLockError::WouldBlock) => {
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
        }
    }
}

fn stream_snapshot_paths_to_pages(
    durable: &Path,
    staging_dir: &Path,
    max_page_bytes: usize,
    max_pages: u32,
    on_progress: &mut impl FnMut(SnapshotExportProgress) -> bool,
) -> Result<(u32, u64, String), OfficeError> {
    let files = Files::open(durable).map_err(OfficeError::storage)?;
    let paths = collect_authoritative_snapshot_paths(durable, &files, on_progress)?;
    let mut page_idx = 0u32;
    let mut total_bytes = 0u64;
    let mut manifest_inputs = Vec::new();
    let mut cur = vec![b'['];
    for (file_idx, path) in paths.into_iter().enumerate() {
        if file_idx % 64 == 0 && !on_progress(SnapshotExportProgress::Heartbeat) {
            return Err(OfficeError::Invalid("snapshot export cancelled"));
        }
        let Some(bytes) = files.read(&path).map_err(OfficeError::storage)? else {
            return Err(OfficeError::Invalid("snapshot file vanished during export"));
        };
        let entry = serde_json::to_vec(&(path.as_str(), STANDARD.encode(bytes)))
            .map_err(OfficeError::storage)?;
        if entry.len().saturating_add(2) > max_page_bytes {
            return Err(OfficeError::Invalid(
                "snapshot file exceeds shared page limit",
            ));
        }
        let sep = usize::from(cur.len() > 1);
        if cur.len().saturating_add(sep + entry.len() + 1) > max_page_bytes {
            cur.push(b']');
            flush_exported_snapshot_page(
                staging_dir,
                page_idx,
                max_pages,
                &cur,
                &mut total_bytes,
                &mut manifest_inputs,
                on_progress,
            )?;
            page_idx = page_idx.saturating_add(1);
            cur = vec![b'['];
        }
        if cur.len() > 1 {
            cur.push(b',');
        }
        cur.extend_from_slice(&entry);
    }
    cur.push(b']');
    flush_exported_snapshot_page(
        staging_dir,
        page_idx,
        max_pages,
        &cur,
        &mut total_bytes,
        &mut manifest_inputs,
        on_progress,
    )?;
    let total_pages = page_idx.saturating_add(1);
    let manifest_sha256_hex = org_pki::cert::cert_fingerprint_hex(&manifest_inputs);
    Ok((total_pages, total_bytes, manifest_sha256_hex))
}

/// Captures HQ's checkpoint and `state/` + channel files at ONE consistent `org-store`
/// materialization boundary, streaming files directly into bounded JSON page files
/// (`page-000000.json`, ...) under `staging_dir` so peak HQ RAM stays bounded to one page and
/// total staged disk bytes obey `on_progress` (`TASK-52` P21-#2 / P22-#2 / P24-#1 / P24-#2).
pub fn export_authoritative_state_snapshot_pages(
    office: &Office,
    staging_dir: &Path,
    max_page_bytes: usize,
    max_pages: u32,
    mut on_progress: impl FnMut(SnapshotExportProgress) -> bool,
) -> Result<ExportedSnapshotPages, OfficeError> {
    crate::private_fs::private_dir(staging_dir).map_err(OfficeError::storage)?;
    // Serialize concurrent exports BEFORE acquiring `federation_io_lock` or waiting on
    // `with_consistent_materialized`, polling cooperatively so queued exports cancel promptly on
    // shutdown without holding `federation_io_lock` (`TASK-52` P24-#1).
    let _export_guard = acquire_snapshot_export_lock_cancellable(office, &mut on_progress)?;
    let mut fed_guard = Some(
        office
            .federation_io_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner()),
    );
    let mut captured: Option<ExportedSnapshotPages> = None;
    let mut capture_err: Option<OfficeError> = None;
    office
        .store
        .with_consistent_materialized(|mat| {
            let res = (|| -> Result<ExportedSnapshotPages, OfficeError> {
                let _ = sync_org_journal_locked_up_to(office, mat)?;
                let checkpoint = office.replication.org_checkpoint()?;
                drop(fed_guard.take());
                let (total_pages, total_bytes, manifest_sha256_hex) =
                    stream_snapshot_paths_to_pages(
                        &office.durable,
                        staging_dir,
                        max_page_bytes,
                        max_pages,
                        &mut on_progress,
                    )?;
                Ok(ExportedSnapshotPages {
                    checkpoint,
                    total_pages,
                    total_bytes,
                    manifest_sha256_hex,
                })
            })();
            match res {
                Ok(out) => captured = Some(out),
                Err(err) => capture_err = Some(err),
            }
            Ok(())
        })
        .map_err(storage)?;
    if let Some(err) = capture_err {
        return Err(err);
    }
    captured.ok_or(OfficeError::Invalid("snapshot export missing result"))
}

fn walk_state_files(durable: &Path) -> Result<Vec<(String, Vec<u8>)>, OfficeError> {
    let files = Files::open(durable).map_err(OfficeError::storage)?;
    let state_prefix = DurablePath::parse("state").map_err(storage)?;
    let mut entries = Vec::new();
    for path in files.walk(&state_prefix).map_err(OfficeError::storage)? {
        let s = path.as_str();
        if !s.starts_with("state/") {
            continue;
        }
        if let Some(bytes) = files.read(&path).map_err(OfficeError::storage)? {
            entries.push((s.to_owned(), bytes.to_vec()));
        }
    }
    append_channel_snapshot_files(&files, &mut entries)?;
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(entries)
}

/// Appends HQ's authoritative `ConversationKind::Channel` definitions to snapshots so newly
/// enrolled or long-offline satellites immediately see all current channels (`TASK-52` P10-#7 / P25-#1).
fn append_channel_snapshot_files(
    files: &Files,
    entries: &mut Vec<(String, Vec<u8>)>,
) -> Result<(), OfficeError> {
    let msg_prefix = DurablePath::parse("messaging/conversations").map_err(storage)?;
    let msg_paths = files.walk(&msg_prefix).map_err(OfficeError::storage)?;
    for path in msg_paths {
        let is_conv = matches!(
            crate::messaging::records::kind_of(&path),
            Some(crate::messaging::records::Kind::Conversation(_))
        );
        if !is_conv {
            continue;
        }
        let Some(bytes) = files.read(&path).map_err(OfficeError::storage)? else {
            return Err(OfficeError::Invalid("snapshot file vanished during export"));
        };
        let conv: crate::messaging::records::ConversationRecord = serde_json::from_slice(&bytes)
            .map_err(|_| OfficeError::Invalid("malformed conversation.json in snapshot"))?;
        if conv.kind == org_proto::chat::ConversationKind::Channel {
            entries.push((path.as_str().to_owned(), bytes.to_vec()));
        }
    }
    Ok(())
}
