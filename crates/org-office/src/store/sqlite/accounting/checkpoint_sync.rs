//! Fixed-point advancement and sync of durable stream checkpoints after reconcile.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use org_proto::costs::UsageRecordView;

use crate::accounting::UsageStreamCheckpoint;
use crate::error::OfficeError;
use crate::private_fs::{full_sync, private_dir_synced};

use super::files::{safe_segment, write_atomic};

struct ReconciledStreamInfo<'a> {
    origin: String,
    producer: String,
    existing_idx: usize,
    existing_hw: u64,
    missing_chain: bool,
    prefix_ok: bool,
    contiguous: bool,
    full_chain: Option<String>,
    prefix_chain: Option<String>,
    recs: Vec<&'a UsageRecordView>,
    advanced: bool,
}

fn analyze_reconciled_streams<'a>(
    org_id: &str,
    records: &'a [UsageRecordView],
    checkpoints: &[UsageStreamCheckpoint],
) -> (Vec<ReconciledStreamInfo<'a>>, Vec<&'a UsageRecordView>) {
    let mut by_stream: BTreeMap<(String, String), Vec<&'a UsageRecordView>> = BTreeMap::new();
    for rec in records {
        by_stream
            .entry((rec.origin_office_id.clone(), rec.producer_id.clone()))
            .or_default()
            .push(rec);
    }
    let mut infos = Vec::new();
    let mut prefix_candidates = Vec::new();
    for ((origin, producer), mut stream_recs) in by_stream {
        stream_recs.sort_by_key(|r| r.source_sequence);
        let Some(existing_idx) = checkpoints.iter().position(|c| {
            c.organization_id == org_id && c.origin_office_id == origin && c.producer_id == producer
        }) else {
            continue;
        };
        let existing_cp = &checkpoints[existing_idx];
        let existing_hw = existing_cp.high_water_sequence;
        let (prefix_ok, contiguous, full_chain, prefix_chain) =
            scan_reconciled_stream_chain(existing_cp, &stream_recs);
        if prefix_ok && (stream_recs.len() as u64) >= existing_hw {
            prefix_candidates.extend(stream_recs.iter().take(existing_hw as usize).copied());
        }
        infos.push(ReconciledStreamInfo {
            origin,
            producer,
            existing_idx,
            existing_hw,
            missing_chain: existing_hw > 0 && existing_cp.chain_sha256.is_none(),
            prefix_ok,
            contiguous,
            full_chain,
            prefix_chain,
            recs: stream_recs,
            advanced: false,
        });
    }
    (infos, prefix_candidates)
}

fn scan_reconciled_stream_chain(
    existing_cp: &UsageStreamCheckpoint,
    stream_recs: &[&UsageRecordView],
) -> (bool, bool, Option<String>, Option<String>) {
    let existing_hw = existing_cp.high_water_sequence;
    let mut expected_seq = 0u64;
    let mut running_chain: Option<String> = None;
    let mut prefix_chain: Option<String> = None;
    let mut prefix_ok = existing_hw == 0 && existing_cp.head_sha256.is_none();
    let mut contiguous = true;
    for rec in stream_recs {
        expected_seq += 1;
        if rec.source_sequence != expected_seq {
            contiguous = false;
            break;
        }
        running_chain = Some(crate::accounting::chain_usage_stream_head(
            running_chain.as_deref(),
            rec,
        ));
        if expected_seq == existing_hw {
            let head_ok = existing_cp.head_sha256.as_deref() == Some(rec.payload_sha256.as_str());
            let chain_ok = existing_cp
                .chain_sha256
                .as_deref()
                .is_none_or(|c| running_chain.as_deref() == Some(c));
            prefix_ok = head_ok && chain_ok;
            prefix_chain = running_chain.clone();
        }
    }
    (prefix_ok, contiguous, running_chain, prefix_chain)
}

fn expand_covered_prefix_ids<'a>(
    prefix_candidates: &[&'a UsageRecordView],
    covered_ids: &mut BTreeSet<&'a str>,
) {
    loop {
        let mut dep_progressed = false;
        for rec in prefix_candidates {
            if !covered_ids.contains(rec.id.as_str())
                && rec
                    .supersedes_id
                    .as_deref()
                    .is_none_or(|pid| covered_ids.contains(pid))
            {
                covered_ids.insert(rec.id.as_str());
                dep_progressed = true;
            }
        }
        if !dep_progressed {
            break;
        }
    }
}

fn try_advance_stream_checkpoint<'a>(
    durable_root: &Path,
    org_id: &str,
    info: &mut ReconciledStreamInfo<'a>,
    checkpoints: &mut [UsageStreamCheckpoint],
    covered_ids: &mut BTreeSet<&'a str>,
) -> Result<bool, OfficeError> {
    if info.advanced || !info.prefix_ok || !info.contiguous {
        return Ok(false);
    }
    let Some(head) = info.recs.last().copied() else {
        return Ok(false);
    };
    if head.source_sequence <= info.existing_hw {
        return Ok(false);
    }
    let mut tentative: BTreeSet<&'a str> = BTreeSet::new();
    for rec in info.recs.iter().skip(info.existing_hw as usize) {
        let parent_ok = rec
            .supersedes_id
            .as_deref()
            .is_none_or(|pid| covered_ids.contains(pid) || tentative.contains(pid));
        if !parent_ok {
            return Ok(false);
        }
        tentative.insert(rec.id.as_str());
    }
    let cp_dir = durable_root
        .join("checkpoints")
        .join(safe_segment(&info.origin));
    private_dir_synced(durable_root, &cp_dir).map_err(OfficeError::storage)?;
    let cp_path = cp_dir.join(format!("{}.json", safe_segment(&info.producer)));
    let cp = UsageStreamCheckpoint {
        organization_id: org_id.to_owned(),
        origin_office_id: info.origin.clone(),
        producer_id: info.producer.clone(),
        high_water_sequence: head.source_sequence,
        head_sha256: Some(head.payload_sha256.clone()),
        chain_sha256: info.full_chain.clone(),
        state: "open".into(),
        checkpointed_at: head.recorded_at.clone(),
    };
    let cp_bytes = serde_json::to_vec_pretty(&cp).map_err(OfficeError::storage)?;
    write_atomic(&cp_path, &cp_bytes)?;
    checkpoints[info.existing_idx] = cp;
    covered_ids.extend(tentative);
    info.advanced = true;
    Ok(true)
}

fn sync_unadvanced_checkpoints(
    durable_root: &Path,
    infos: &[ReconciledStreamInfo<'_>],
    checkpoints: &mut [UsageStreamCheckpoint],
) -> Result<(), OfficeError> {
    for info in infos {
        if info.advanced {
            continue;
        }
        let cp_dir = durable_root
            .join("checkpoints")
            .join(safe_segment(&info.origin));
        private_dir_synced(durable_root, &cp_dir).map_err(OfficeError::storage)?;
        let cp_path = cp_dir.join(format!("{}.json", safe_segment(&info.producer)));
        if info.missing_chain
            && info.prefix_ok
            && (info.recs.len() as u64) >= info.existing_hw
            && info.prefix_chain.is_some()
        {
            let mut upgraded = checkpoints[info.existing_idx].clone();
            upgraded.chain_sha256 = info.prefix_chain.clone();
            let cp_bytes = serde_json::to_vec_pretty(&upgraded).map_err(OfficeError::storage)?;
            write_atomic(&cp_path, &cp_bytes)?;
            checkpoints[info.existing_idx] = upgraded;
        } else if cp_path.is_file() {
            let f = fs::File::open(&cp_path).map_err(OfficeError::storage)?;
            full_sync(&f).map_err(OfficeError::storage)?;
        }
    }
    Ok(())
}

/// Fixed-point checkpoint advancement (`TASK-97` / Pass 6 #1, Pass 7 #4, Pass 8 #6):
/// - Never fabricates a checkpoint for an unanchored stream.
/// - Seeds `covered_ids` ONLY from records ALREADY covered by verified `1..=existing_hw` prefixes.
/// - Advances a stream's checkpoint to `N` ONLY when its prefix matches, `1..=N` is contiguous,
///   and every tail record (`seq > existing_hw`) has its `supersedes_id` parent already covered by
///   an existing or newly advanced checkpoint (or an earlier record in the same advancing stream).
pub(super) fn sync_checkpoints_for_reconciled_records(
    durable_root: &Path,
    org_id: &str,
    records: &[UsageRecordView],
    checkpoints: &mut [UsageStreamCheckpoint],
) -> Result<(), OfficeError> {
    let (mut infos, prefix_candidates) = analyze_reconciled_streams(org_id, records, checkpoints);
    let mut covered_ids: BTreeSet<&str> = BTreeSet::new();
    loop {
        expand_covered_prefix_ids(&prefix_candidates, &mut covered_ids);
        let mut progressed = false;
        for info in &mut infos {
            if try_advance_stream_checkpoint(
                durable_root,
                org_id,
                info,
                checkpoints,
                &mut covered_ids,
            )? {
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }
    sync_unadvanced_checkpoints(durable_root, &infos, checkpoints)
}
