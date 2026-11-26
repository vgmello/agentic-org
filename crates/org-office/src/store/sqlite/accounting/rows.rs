//! SQL row writers for prices, usage records, stream checkpoints and subscription usage.

use org_proto::costs::{ModelPriceView, UsageRecordView};
use rusqlite::{params, Connection};

use crate::accounting::{StoredSubscriptionUsageSnapshot, UsageStreamCheckpoint};
use crate::error::OfficeError;

pub(super) fn upsert_subscription_usage_row(
    conn: &Connection,
    org_id: &str,
    snap: &StoredSubscriptionUsageSnapshot,
) -> Result<(), OfficeError> {
    let payload = serde_json::to_string(snap).map_err(OfficeError::storage)?;
    conn.execute(
        "INSERT INTO subscription_usage_snapshots \
         (organization_id, connection_id, office_id, status, collected_at, fresh_until, json_payload) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
         ON CONFLICT(organization_id, connection_id) DO UPDATE SET \
         office_id=excluded.office_id, status=excluded.status, \
         collected_at=excluded.collected_at, fresh_until=excluded.fresh_until, \
         json_payload=excluded.json_payload",
        params![
            org_id,
            &snap.connection_id,
            &snap.office_id,
            snap.status.as_str(),
            &snap.collected_at,
            &snap.fresh_until,
            &payload,
        ],
    )
    .map_err(OfficeError::storage)?;
    Ok(())
}

pub(super) fn insert_price_row(
    tx: &rusqlite::Transaction<'_>,
    org_id: &str,
    price: &ModelPriceView,
) -> Result<(), OfficeError> {
    let payload = serde_json::to_string(price).map_err(OfficeError::storage)?;
    tx.execute(
        "INSERT INTO model_prices(organization_id, id, provider, model, effective_from, json_payload) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![org_id, &price.id, &price.provider, &price.model, &price.effective_from, payload],
    )
    .map_err(OfficeError::storage)?;
    Ok(())
}

pub(super) fn insert_price_row_ignore(
    tx: &rusqlite::Transaction<'_>,
    org_id: &str,
    price: &ModelPriceView,
) -> Result<(), OfficeError> {
    let payload = serde_json::to_string(price).map_err(OfficeError::storage)?;
    tx.execute(
        "INSERT OR IGNORE INTO model_prices(organization_id, id, provider, model, effective_from, json_payload) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![org_id, &price.id, &price.provider, &price.model, &price.effective_from, payload],
    )
    .map_err(OfficeError::storage)?;
    Ok(())
}

pub(super) fn insert_usage_row_strict(
    tx: &rusqlite::Transaction<'_>,
    org_id: &str,
    rec: &UsageRecordView,
    chain_sha256: Option<&str>,
) -> Result<(), OfficeError> {
    insert_usage_sql(tx, org_id, rec, chain_sha256, "INSERT INTO usage_records")
}

fn insert_usage_sql(
    tx: &rusqlite::Transaction<'_>,
    org_id: &str,
    rec: &UsageRecordView,
    chain_sha256: Option<&str>,
    verb: &str,
) -> Result<(), OfficeError> {
    let payload = serde_json::to_string(rec).map_err(OfficeError::storage)?;
    let sql = format!(
        "{verb}(organization_id, id, origin_office_id, producer_id, source_event_id, \
         source_sequence, occurred_at, payload_sha256, chain_sha256, supersedes_id, json_payload) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"
    );
    tx.execute(
        &sql,
        params![
            org_id,
            &rec.id,
            &rec.origin_office_id,
            &rec.producer_id,
            &rec.source_event_id,
            rec.source_sequence as i64,
            &rec.occurred_at,
            &rec.payload_sha256,
            chain_sha256,
            rec.supersedes_id.as_deref(),
            payload,
        ],
    )
    .map_err(OfficeError::storage)?;
    Ok(())
}

pub(super) fn insert_staged_usage_row(
    tx: &rusqlite::Transaction<'_>,
    org_id: &str,
    rec: &UsageRecordView,
) -> Result<(), OfficeError> {
    let payload = serde_json::to_string(rec).map_err(OfficeError::storage)?;
    tx.execute(
        "INSERT INTO staged_usage_records(\
         organization_id, id, origin_office_id, producer_id, source_event_id, \
         source_sequence, supersedes_id, json_payload) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            org_id,
            &rec.id,
            &rec.origin_office_id,
            &rec.producer_id,
            &rec.source_event_id,
            rec.source_sequence as i64,
            rec.supersedes_id.as_deref(),
            payload,
        ],
    )
    .map_err(OfficeError::storage)?;
    Ok(())
}

pub(super) fn upsert_checkpoint_row(
    tx: &rusqlite::Transaction<'_>,
    cp: &UsageStreamCheckpoint,
) -> Result<(), OfficeError> {
    tx.execute(
        "INSERT INTO usage_stream_checkpoints(\
         organization_id, origin_office_id, producer_id, high_water_sequence, head_sha256, \
         chain_sha256, state, checkpointed_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
         ON CONFLICT(organization_id, origin_office_id, producer_id) DO UPDATE SET \
         high_water_sequence = MAX(high_water_sequence, excluded.high_water_sequence), \
         head_sha256 = CASE WHEN excluded.high_water_sequence >= high_water_sequence THEN excluded.head_sha256 ELSE head_sha256 END, \
         chain_sha256 = CASE WHEN excluded.high_water_sequence >= high_water_sequence THEN excluded.chain_sha256 ELSE chain_sha256 END, \
         checkpointed_at = CASE WHEN excluded.high_water_sequence >= high_water_sequence THEN excluded.checkpointed_at ELSE checkpointed_at END",
        params![
            &cp.organization_id,
            &cp.origin_office_id,
            &cp.producer_id,
            cp.high_water_sequence as i64,
            cp.head_sha256.as_deref(),
            cp.chain_sha256.as_deref(),
            &cp.state,
            &cp.checkpointed_at,
        ],
    )
    .map_err(OfficeError::storage)?;
    Ok(())
}
