//! Applying a staged snapshot in dependency order with bounded native batches, and the initial
//! paged snapshot sync.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use crate::federation_link::{
    collect_pending_work_paths_checked, deterministic_uuid_v7, SatelliteSyncSession,
};
use org_office::Office;
use org_sync::organization::{install_snapshot, ValidatedSnapshot};
use org_sync_proto::{Codec, Digest, Epoch, Position};

use super::staging::{
    decode_and_validate_snapshot_files, fetch_and_stage_snapshot_pages, StagedSnapshotImport,
};
use super::tiers::{
    is_employee_profile_path, is_org_core_put, snapshot_dependency_tier,
    strip_org_core_employee_refs,
};
use super::{
    page_file_path, SnapshotFileBatch, MAX_NATIVE_BATCH_BYTES, TIER_BIT_ORG_EMP,
    TIER_BIT_ORG_STRUCT,
};

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
    protected_tasks: &BTreeSet<String>,
    committer: &mut BoundedBatchCommitter<'_>,
) -> Result<(), String> {
    // Propagate local state scan errors so a failed scan can never leave buffered writes
    // uncommitted before checkpoint installation (`TASK-52` P26-#1).
    let local_paths = office.list_local_state_paths().map_err(|e| {
        format!("failed listing local state paths for snapshot reconciliation: {e}")
    })?;
    // Local task files are never removed (a satellite's own tasks may not be at HQ yet),
    // except the copies a snapshot moved to another folder (`MoveTeam`, `doc-77`) of a task
    // whose incoming files were applied (not one the satellite protected).
    let mut relocated = org_office::relocated_task_paths(&local_paths, &staged.incoming_paths);
    relocated.retain(|path| {
        org_office::task_id_for_work_path(path).is_none_or(|tid| !protected_tasks.contains(&tid))
    });
    let mut deletes: Vec<String> = local_paths
        .into_iter()
        .filter(|existing| {
            !staged.incoming_paths.contains(existing)
                && !pending_work_paths.contains(existing)
                && (relocated.contains(existing)
                    || (!org_office::is_immutable_state_path(existing)
                        && org_office::task_id_for_work_path(existing).is_none()))
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
        &protected_tasks,
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
