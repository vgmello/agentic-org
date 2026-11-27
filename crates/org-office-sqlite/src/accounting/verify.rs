//! Dedupe replay, supersede validation and stream checkpoint/coverage verification.

use std::collections::BTreeSet;

use org_proto::costs::UsageRecordView;
use org_proto::{ApiError, ErrorCode};
use rusqlite::{params, Connection, OptionalExtension};

use org_office::accounting::VerifiedStreamWatermark;
use org_office::OfficeError;

use super::{AccountingState, CheckpointCoverageRow};

type DedupReplay = Option<Result<(UsageRecordView, bool), ApiError>>;

pub(super) fn check_existing_usage(
    conn: &Connection,
    org_id: &str,
    rec: &UsageRecordView,
) -> Result<DedupReplay, OfficeError> {
    let staged_match: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM staged_usage_records \
             WHERE organization_id=?1 AND (\
               id=?2 OR (origin_office_id=?3 AND producer_id=?4 AND source_event_id=?5)\
             ))",
            params![
                org_id,
                &rec.id,
                &rec.origin_office_id,
                &rec.producer_id,
                &rec.source_event_id,
            ],
            |r| r.get(0),
        )
        .map_err(OfficeError::storage)?;
    if staged_match {
        return Ok(Some(Err(ApiError::new(
            ErrorCode::Conflict,
            "usage record belongs to a staged or unverified stream",
        ))));
    }
    let by_source: Option<(String, String)> = conn
        .query_row(
            "SELECT payload_sha256, json_payload FROM usage_records \
             WHERE organization_id=?1 AND origin_office_id=?2 AND producer_id=?3 AND source_event_id=?4",
            params![org_id, &rec.origin_office_id, &rec.producer_id, &rec.source_event_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(OfficeError::storage)?;
    if let Some((sha, payload)) = by_source {
        if sha == rec.payload_sha256 {
            let existing: UsageRecordView =
                serde_json::from_str(&payload).map_err(OfficeError::storage)?;
            return Ok(Some(Ok((existing, true))));
        }
        return Ok(Some(Err(ApiError::new(
            ErrorCode::Conflict,
            "source_event_id already recorded with a different payload",
        ))));
    }
    let by_id: Option<(String, String)> = conn
        .query_row(
            "SELECT payload_sha256, json_payload FROM usage_records \
             WHERE organization_id=?1 AND id=?2",
            params![org_id, &rec.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(OfficeError::storage)?;
    if let Some((sha, payload)) = by_id {
        if sha == rec.payload_sha256 {
            let existing: UsageRecordView =
                serde_json::from_str(&payload).map_err(OfficeError::storage)?;
            return Ok(Some(Ok((existing, true))));
        }
        return Ok(Some(Err(ApiError::new(
            ErrorCode::Conflict,
            "usage record id already exists with a different payload",
        ))));
    }
    Ok(None)
}

pub(super) fn check_supersedes_valid(
    conn: &Connection,
    org_id: &str,
    rec: &UsageRecordView,
) -> Result<Option<ApiError>, OfficeError> {
    let Some(parent_id) = rec.supersedes_id.as_deref() else {
        return Ok(None);
    };
    let parent_staged: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM staged_usage_records WHERE organization_id=?1 AND id=?2)",
            params![org_id, parent_id],
            |r| r.get(0),
        )
        .map_err(OfficeError::storage)?;
    if parent_staged {
        return Ok(Some(ApiError::new(
            ErrorCode::Conflict,
            "superseded usage record belongs to a staged or unverified stream",
        )));
    }
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM usage_records WHERE organization_id=?1 AND id=?2)",
            params![org_id, parent_id],
            |r| r.get(0),
        )
        .map_err(OfficeError::storage)?;
    if !exists {
        return Ok(Some(ApiError::new(
            ErrorCode::NotFound,
            "superseded usage record does not exist",
        )));
    }
    let already_superseded: bool = conn
        .query_row(
            "SELECT EXISTS(\
                SELECT 1 FROM usage_records WHERE organization_id=?1 AND supersedes_id=?2 \
                UNION ALL \
                SELECT 1 FROM staged_usage_records WHERE organization_id=?1 AND supersedes_id=?2\
             )",
            params![org_id, parent_id],
            |r| r.get(0),
        )
        .map_err(OfficeError::storage)?;
    if already_superseded {
        return Ok(Some(ApiError::new(
            ErrorCode::Conflict,
            "usage record is already superseded; correct the current leaf instead",
        )));
    }
    Ok(None)
}

/// Verifies a stream's `1..=hw` prefix in `usage_records` against both `head_sha256` (last event's
/// `payload_sha256`, matching `docs/design/organization-data/schema.sql:1129`) and `chain_sha256`
/// (running SHA-256 chain over `id`, `source_sequence`, `recorded_at`, `payload_sha256`).
/// Returns `Ok(Some(running_chain))` when valid, or `Ok(None)` when incomplete or mismatched.
fn verify_stream_checkpoint(
    conn: &Connection,
    org_id: &str,
    office: &str,
    prod: &str,
    hw: i64,
    head_sha: Option<&str>,
    chain_sha: Option<&str>,
) -> Result<Option<Option<String>>, OfficeError> {
    if hw < 0 {
        return Ok(None);
    }
    let mut stmt = conn
        .prepare(
            "SELECT source_sequence, id, payload_sha256, json_payload FROM usage_records \
             WHERE organization_id=?1 AND origin_office_id=?2 AND producer_id=?3 \
             ORDER BY source_sequence ASC",
        )
        .map_err(OfficeError::storage)?;
    let rows = stmt
        .query_map(params![org_id, office, prod], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .map_err(OfficeError::storage)?;
    let mut count: i64 = 0;
    let mut last_payload_sha: Option<String> = None;
    let mut running_chain: Option<String> = None;
    for row in rows {
        let (seq, id, stored_sha, json) = row.map_err(OfficeError::storage)?;
        count += 1;
        if seq != count {
            return Ok(None);
        }
        let Ok(rec) = serde_json::from_str::<UsageRecordView>(&json) else {
            return Ok(None);
        };
        if rec.id != id
            || rec.source_sequence != (seq as u64)
            || rec.payload_sha256 != stored_sha
            || org_office::accounting::compute_usage_payload_sha256(org_id, &rec) != stored_sha
        {
            return Ok(None);
        }
        last_payload_sha = Some(stored_sha);
        running_chain = Some(org_office::accounting::chain_usage_stream_head(
            running_chain.as_deref(),
            &rec,
        ));
    }
    if count != hw {
        return Ok(None);
    }
    if last_payload_sha.as_deref() != head_sha {
        return Ok(None);
    }
    if let Some(expected_chain) = chain_sha {
        if running_chain.as_deref() != Some(expected_chain) {
            return Ok(None);
        }
    }
    Ok(Some(running_chain))
}

pub(super) fn load_checkpoint_coverage_rows(
    conn: &Connection,
    org_id: &str,
) -> Result<Vec<CheckpointCoverageRow>, OfficeError> {
    let mut stmt = conn
        .prepare(
            "SELECT origin_office_id, producer_id, high_water_sequence, head_sha256, \
                    chain_sha256, checkpointed_at \
             FROM usage_stream_checkpoints WHERE organization_id=?1 \
             ORDER BY origin_office_id, producer_id",
        )
        .map_err(OfficeError::storage)?;
    let rows = stmt
        .query_map([org_id], |r| {
            Ok(CheckpointCoverageRow {
                office: r.get(0)?,
                prod: r.get(1)?,
                hw: r.get(2)?,
                head_sha: r.get(3)?,
                chain_sha: r.get(4)?,
                at: r.get(5)?,
            })
        })
        .map_err(OfficeError::storage)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(OfficeError::storage)?);
    }
    Ok(out)
}

pub(super) fn evaluate_stream_coverage_row(
    conn: &AccountingState,
    org_id: &str,
    row: &CheckpointCoverageRow,
    incomplete: &mut BTreeSet<(String, String)>,
) -> Result<Option<VerifiedStreamWatermark>, OfficeError> {
    let key = (row.office.clone(), row.prod.clone());
    let stream_key = (org_id.to_owned(), row.office.clone(), row.prod.clone());
    let prefix_head = conn
        .verified_prefixes
        .get(&stream_key)
        .filter(|h| {
            row.hw >= 0
                && h.high_water_sequence == (row.hw as u64)
                && h.head_sha256 == row.head_sha
                && row
                    .chain_sha
                    .as_ref()
                    .is_none_or(|c| h.chain_sha256.as_ref() == Some(c))
        })
        .cloned();
    let stream_fully_verified = !incomplete.contains(&key)
        && row.hw >= 0
        && conn.verified_streams.get(&stream_key).is_some_and(|h| {
            h.high_water_sequence == (row.hw as u64)
                && h.head_sha256 == row.head_sha
                && h.chain_sha256 == row.chain_sha
        });
    let full_ok = if stream_fully_verified {
        true
    } else if incomplete.contains(&key) {
        false
    } else {
        verify_stream_checkpoint(
            &conn.conn,
            org_id,
            &row.office,
            &row.prod,
            row.hw,
            row.head_sha.as_deref(),
            row.chain_sha.as_deref(),
        )?
        .is_some()
    };
    if !full_ok {
        incomplete.insert(key);
    }
    if !(full_ok || prefix_head.is_some()) {
        return Ok(None);
    }
    let effective_chain = prefix_head
        .as_ref()
        .and_then(|h| h.chain_sha256.clone())
        .or_else(|| row.chain_sha.clone());
    Ok(Some(VerifiedStreamWatermark {
        origin_office_id: row.office.clone(),
        producer_id: row.prod.clone(),
        high_water_sequence: row.hw as u64,
        chain_sha256: effective_chain,
    }))
}
