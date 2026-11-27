//! Rebuilds the SQLite projection from the durable accounting files.

use std::path::Path;

use org_proto::costs::UsageRecordView;
use rusqlite::{params, Connection};

use org_office::OfficeError;

use super::checkpoint_sync::sync_checkpoints_for_reconciled_records;
use super::files::{
    load_durable_checkpoints, load_durable_prices, load_durable_subscription_usage,
    load_durable_usage_records, sync_usage_dir_and_parent,
};
use super::partition::verify_and_partition_streams;
use super::rows::{
    insert_price_row_ignore, insert_staged_usage_row, insert_usage_row_strict,
    upsert_checkpoint_row, upsert_subscription_usage_row,
};
use super::{PartitionedStreams, ReconcileMetadata};

pub(super) fn reconcile_durable_conn(
    conn: &mut Connection,
    durable_root: &Path,
    local_usage_dir: &Path,
    office_id: &str,
    org_id: &str,
    flush_before_reconcile: bool,
) -> Result<ReconcileMetadata, OfficeError> {
    let prices = load_durable_prices(durable_root, flush_before_reconcile)?;
    let all_records = load_durable_usage_records(durable_root, org_id, flush_before_reconcile)?;
    let mut checkpoints = load_durable_checkpoints(durable_root)?;
    if flush_before_reconcile {
        // Advance only already-anchored streams whose durable JSONL advanced ahead of their
        // checkpoint during an interrupted write and whose parents are already checkpoint-covered;
        // never fabricate a checkpoint for an unanchored restore (`TASK-97` / Pass 6 #1, Pass 8 #6).
        sync_checkpoints_for_reconciled_records(
            durable_root,
            org_id,
            &all_records,
            &mut checkpoints,
        )?;
    }
    // Verify complete streams against their durable checkpoints BEFORE staging orphan corrections,
    // so a missing parent for correction N never undoes an earlier valid correction in the same
    // stream (`TASK-97` / Final Gate Pass 5 #2, Pass 6 #2).
    let PartitionedStreams {
        projected: records,
        staged: staged_records,
        verified: verified_streams,
        verified_prefixes,
        staged_streams,
    } = verify_and_partition_streams(org_id, &checkpoints, all_records);
    let sub_snapshots = load_durable_subscription_usage(local_usage_dir, org_id, office_id)?;
    if !sub_snapshots.is_empty() {
        sync_usage_dir_and_parent(local_usage_dir)?;
    }
    let tx = conn.transaction().map_err(OfficeError::storage)?;
    // Rebuild the organization's projection tables cleanly from the durable archive so any
    // truncated/lost trailing JSONL record cannot linger in SQLite and mask a coverage gap
    // (`TASK-97` / Final Gate Finding #2).
    tx.execute(
        "DELETE FROM usage_records WHERE organization_id=?1",
        [org_id],
    )
    .map_err(OfficeError::storage)?;
    tx.execute(
        "DELETE FROM staged_usage_records WHERE organization_id=?1",
        [org_id],
    )
    .map_err(OfficeError::storage)?;
    tx.execute(
        "DELETE FROM usage_stream_checkpoints WHERE organization_id=?1",
        [org_id],
    )
    .map_err(OfficeError::storage)?;
    tx.execute(
        "DELETE FROM model_prices WHERE organization_id=?1",
        [org_id],
    )
    .map_err(OfficeError::storage)?;
    tx.execute(
        "DELETE FROM subscription_usage_snapshots WHERE organization_id=?1",
        [org_id],
    )
    .map_err(OfficeError::storage)?;
    for price in &prices {
        insert_price_row_ignore(&tx, org_id, price)?;
    }
    for (rec, chain_sha) in &records {
        reconcile_usage_row(&tx, org_id, rec, chain_sha.as_deref())?;
    }
    for rec in &staged_records {
        insert_staged_usage_row(&tx, org_id, rec)?;
    }
    for cp in &checkpoints {
        upsert_checkpoint_row(&tx, cp)?;
    }
    for snap in &sub_snapshots {
        upsert_subscription_usage_row(&tx, org_id, snap)?;
    }
    tx.commit().map_err(OfficeError::storage)?;
    Ok(ReconcileMetadata {
        verified_streams,
        verified_prefixes,
        staged_streams,
    })
}

/// Inserts a replayed durable usage row, skipping ONLY an exact full-record duplicate across
/// ALL unique identities (`id`, `(origin, producer, source_event_id)`, `(origin, producer,
/// source_sequence)`) and failing closed on any conflict (`TASK-97` / Pass 3 Finding #4).
fn reconcile_usage_row(
    tx: &rusqlite::Transaction<'_>,
    org_id: &str,
    rec: &UsageRecordView,
    chain_sha256: Option<&str>,
) -> Result<(), OfficeError> {
    let mut stmt = tx
        .prepare(
            "SELECT json_payload FROM usage_records \
             WHERE organization_id=?1 AND (\
               id=?2 OR \
               (origin_office_id=?3 AND producer_id=?4 AND source_event_id=?5) OR \
               (origin_office_id=?3 AND producer_id=?4 AND source_sequence=?6)\
             )",
        )
        .map_err(OfficeError::storage)?;
    let matches: Vec<String> = stmt
        .query_map(
            params![
                org_id,
                &rec.id,
                &rec.origin_office_id,
                &rec.producer_id,
                &rec.source_event_id,
                rec.source_sequence as i64,
            ],
            |r| r.get(0),
        )
        .map_err(OfficeError::storage)?
        .collect::<Result<_, _>>()
        .map_err(OfficeError::storage)?;
    if !matches.is_empty() {
        if matches.len() == 1 {
            let existing: UsageRecordView =
                serde_json::from_str(&matches[0]).map_err(OfficeError::storage)?;
            if existing == *rec {
                return Ok(());
            }
        }
        return Err(OfficeError::Invalid(
            "conflicting usage record detected during durable replay",
        ));
    }
    insert_usage_row_strict(tx, org_id, rec, chain_sha256)
}
