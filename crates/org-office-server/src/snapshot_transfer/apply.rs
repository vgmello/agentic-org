//! Applying a staged snapshot in dependency order with bounded native batches, and the initial
//! paged snapshot sync.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::sync::Arc;

use crate::federation_link::{
    collect_pending_work_paths_checked, deterministic_uuid_v7, SatelliteSyncSession,
};
use org_office::tasks::replica::SnapshotTasks;
use org_office::Office;
use org_sync::organization::{install_snapshot, ValidatedSnapshot};
use org_sync_proto::{Codec, Digest, Epoch, Position};

use super::moves::superseded_paths;
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
    /// The snapshot's tasks: each batch names the effective revision the whole snapshot
    /// brings its tasks to, so a batch holding a task's `task.md` apart from its later
    /// history is never judged stale alone (`doc-79`).
    tasks: &'a SnapshotTasks,
    hq_office_id: &'a str,
    operation_id: &'a str,
    single_commit_mode: bool,
    /// The most a batch holds, unless one record's group alone is larger.
    max_batch_bytes: usize,
    /// For each path the snapshot writes, the local paths of the same record at the team
    /// chain it left (`MoveTeam`): removed in the same batch, just before the write.
    superseded: BTreeMap<String, Vec<String>>,
    /// The superseded paths already pushed for removal: reconciliation never removes them
    /// again (one batch never changes a path twice).
    removed: BTreeSet<String>,
    cur: SnapshotFileBatch,
    cur_bytes: usize,
    batch_idx: usize,
}

fn item_bytes(item: &(String, Option<Vec<u8>>)) -> usize {
    item.0.len() + item.1.as_ref().map_or(0, Vec::len) + 64
}

impl<'a> BoundedBatchCommitter<'a> {
    fn new(
        office: &'a Office,
        tasks: &'a SnapshotTasks,
        hq_office_id: &'a str,
        operation_id: &'a str,
        single_commit_mode: bool,
        max_batch_bytes: usize,
    ) -> Self {
        Self {
            office,
            tasks,
            hq_office_id,
            operation_id,
            single_commit_mode,
            max_batch_bytes,
            superseded: BTreeMap::new(),
            removed: BTreeSet::new(),
            cur: Vec::new(),
            cur_bytes: 0,
            batch_idx: 0,
        }
    }

    /// Pushes files that must land in one batch, in order: a batch is flushed first when
    /// they do not fit beside it.
    fn push_group(&mut self, group: SnapshotFileBatch) -> Result<(), String> {
        let group_bytes = group.iter().map(item_bytes).sum::<usize>();
        if !self.cur.is_empty() && self.cur_bytes.saturating_add(group_bytes) > self.max_batch_bytes
        {
            self.flush()?;
        }
        self.cur_bytes = self.cur_bytes.saturating_add(group_bytes);
        self.cur.extend(group);
        Ok(())
    }

    /// Pushes a snapshot file; the first write of a record the snapshot moved to another
    /// team chain carries the removal of its old paths, just before it, in the same batch.
    fn push_put(&mut self, path: String, bytes: Vec<u8>) -> Result<(), String> {
        let old = self.superseded.remove(&path).unwrap_or_default();
        let mut group = Vec::with_capacity(old.len() + 1);
        for old_path in old {
            self.removed.insert(old_path.clone());
            group.push((old_path, None));
        }
        group.push((path, Some(bytes)));
        self.push_group(group)
    }

    fn push_removal(&mut self, path: String) -> Result<(), String> {
        self.push_group(vec![(path, None)])
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
        let revisions = self
            .tasks
            .revisions_of(batch.iter().map(|(p, _)| p.as_str()));
        self.office
            .apply_replicated_snapshot_batch(&op_id, batch, revisions)
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
            committer.push_put(path, payload)?;
        }
    }
    Ok(())
}

fn apply_staged_org_core(
    staged: &StagedSnapshotImport,
    committer: &mut BoundedBatchCommitter<'_>,
) -> Result<(), String> {
    if staged.org_core_bytes <= committer.max_batch_bytes {
        stream_org_core_pass(staged, committer, false, None, false)?;
        stream_org_core_pass(staged, committer, true, None, false)?;
        if !committer.single_commit_mode {
            committer.flush()?;
        }
        return Ok(());
    }
    // Oversized org_core (> 8 MiB in production): stream 4 cycle-breaking passes from disk
    // without buffering all files in RAM (`TASK-52` P6-#4 / P21-#2).
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
                committer.push_put(path, bytes)?;
            }
        }
        if !committer.single_commit_mode {
            committer.flush()?;
        }
    }
    Ok(())
}

/// The local files snapshot reconciliation keeps although the snapshot does not name them.
struct Kept<'a> {
    /// Work still in this satellite's outbox, not at HQ yet.
    pending_work_paths: &'a BTreeSet<String>,
    /// A shared file still in this satellite's outbox is not at HQ yet: its record stays, or
    /// its bytes would wait in the outbox for a record that is gone (`TASK-98.6`).
    unshipped_files: BTreeSet<String>,
}

impl<'a> Kept<'a> {
    fn load(office: &Office, pending_work_paths: &'a BTreeSet<String>) -> Result<Self, String> {
        let unshipped_files = office.pending_blob_outbox_state_paths().map_err(|e| {
            format!("failed listing un-shipped attachment records for snapshot reconciliation: {e}")
        })?;
        Ok(Self {
            pending_work_paths,
            unshipped_files,
        })
    }

    /// Whether reconciliation may remove the local file `path` the snapshot does not name.
    /// Local task files and append-only history are never removed (a satellite's own tasks
    /// may not be at HQ yet, `TASK-52` H-9), except the copies in a moved task's old folder
    /// (`relocated`, `MoveTeam`, `doc-77`).
    fn may_remove(&self, path: &str, relocated: &BTreeSet<String>) -> bool {
        !self.pending_work_paths.contains(path)
            && !self.unshipped_files.contains(path)
            && (relocated.contains(path)
                || (!org_office::is_immutable_state_path(path)
                    && org_office::task_id_for_work_path(path).is_none()))
    }
}

fn list_local_state_paths(office: &Office) -> Result<Vec<String>, String> {
    office
        .list_local_state_paths()
        .map_err(|e| format!("failed listing local state paths for snapshot reconciliation: {e}"))
}

/// The local paths of each record the snapshot moved to another team chain, by the path the
/// snapshot writes it at, judged on the local files before any batch applies (`TASK-224`).
fn moved_record_paths(
    office: &Office,
    staged: &StagedSnapshotImport,
    pending_work_paths: &BTreeSet<String>,
) -> Result<BTreeMap<String, Vec<String>>, String> {
    let local_paths = list_local_state_paths(office)?;
    let kept = Kept::load(office, pending_work_paths)?;
    let no_task_moves = BTreeSet::new();
    Ok(superseded_paths(
        &local_paths,
        &staged.incoming_paths,
        |path| kept.may_remove(path, &no_task_moves),
    ))
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
    let local_paths = list_local_state_paths(office)?;
    // The copies a snapshot moved to another folder (`MoveTeam`, `doc-77`) of a task whose
    // incoming files were applied (not one the satellite protected).
    let mut relocated = org_office::relocated_task_paths(&local_paths, &staged.incoming_paths);
    relocated.retain(|path| {
        org_office::task_id_for_work_path(path).is_none_or(|tid| !protected_tasks.contains(&tid))
    });
    let kept = Kept::load(office, pending_work_paths)?;
    // A moved record's old path went with the batch that wrote its new one: removing it
    // again would remove the record (`TASK-224`).
    let mut deletes: Vec<String> = local_paths
        .into_iter()
        .filter(|existing| {
            !staged.incoming_paths.contains(existing)
                && !committer.removed.contains(existing)
                && kept.may_remove(existing, &relocated)
        })
        .collect();
    deletes.sort_by(|a, b| {
        snapshot_dependency_tier(a, false)
            .cmp(&snapshot_dependency_tier(b, false))
            .then_with(|| a.cmp(b))
    });
    for path in deletes {
        committer.push_removal(path)?;
    }
    committer.flush()
}

/// Streams, validates, stages to disk, applies in bounded dependency-safe batches, and installs
/// HQ's authoritative snapshot without retaining the whole snapshot in RAM (`TASK-52` P21-#2).
pub(crate) fn sync_initial_snapshot_paged(
    session: &mut SatelliteSyncSession,
    operation_id: &str,
) -> Result<usize, String> {
    sync_initial_snapshot_in_batches(session, operation_id, MAX_NATIVE_BATCH_BYTES)
}

/// [`sync_initial_snapshot_paged`] committing at most `max_batch_bytes` per native batch:
/// tests force a small snapshot through several batches and the oversized org-core passes.
pub(crate) fn sync_initial_snapshot_in_batches(
    session: &mut SatelliteSyncSession,
    operation_id: &str,
    max_batch_bytes: usize,
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
    let single_commit_mode =
        staged.total_decoded_bytes <= max_batch_bytes && staged.org_core_bytes <= max_batch_bytes;
    let mut committer = BoundedBatchCommitter::new(
        &session.office,
        &staged.tasks,
        session.hq_peer.office_id.as_str(),
        operation_id,
        single_commit_mode,
        max_batch_bytes,
    );
    committer.superseded = moved_record_paths(&session.office, &staged, &pending_work_paths)?;
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
