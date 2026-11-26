//! SQL queries and row decoders for cooldowns, permits, bindings, states and models.

use org_proto::providers::{
    ConnectionQuotaStatus, ConnectionReadinessStatus, DiscoveredModelEntitlement, PermitAuthority,
    TurnPermitState,
};
use rusqlite::{params, Connection};

use crate::error::OfficeError;
use crate::providers::{
    ConnectionQuotaState, OfficeConnectionBinding, OfficeConnectionModel, OfficeConnectionState,
    TurnPermitRecord,
};

pub(super) fn query_active_cooldown(
    conn: &Connection,
    org_id: &str,
    connection_id: &str,
    model_id: &str,
    now_ms: u64,
) -> Result<Option<String>, OfficeError> {
    let model_scope = format!("model:{model_id}");
    let mut stmt = conn
        .prepare_cached(
            "SELECT cooldown_until FROM connection_quota_state \
             WHERE organization_id=?1 AND connection_id=?2 AND status='cooldown' \
             AND scope_key IN ('connection', ?3)",
        )
        .map_err(OfficeError::storage)?;
    let rows = stmt
        .query_map(params![org_id, connection_id, &model_scope], |r| {
            r.get::<_, Option<String>>(0)
        })
        .map_err(OfficeError::storage)?;
    let mut best: Option<(u64, String)> = None;
    for r in rows {
        if let Some(until) = r.map_err(OfficeError::storage)? {
            if let Some(ms) = org_proto::parse_rfc3339_ms(&until) {
                if now_ms < ms && best.as_ref().is_none_or(|(b, _)| ms > *b) {
                    best = Some((ms, until));
                }
            }
        }
    }
    Ok(best.map(|(_, s)| s))
}

pub(super) fn query_all_cooldowns(
    conn: &Connection,
) -> Result<Vec<ConnectionQuotaState>, OfficeError> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT organization_id, connection_id, scope_key, model_id, status, cooldown_until, \
             observed_at, observed_by_office_id, coordinator_generation \
             FROM connection_quota_state ORDER BY connection_id, scope_key",
        )
        .map_err(OfficeError::storage)?;
    let rows = stmt
        .query_map([], |r| {
            let status_s: String = r.get(4)?;
            Ok(ConnectionQuotaState {
                organization_id: r.get(0)?,
                connection_id: r.get(1)?,
                scope_key: r.get(2)?,
                model_id: r.get(3)?,
                status: ConnectionQuotaStatus::parse(&status_s)
                    .unwrap_or(ConnectionQuotaStatus::Unknown),
                cooldown_until: r.get(5)?,
                observed_at: r.get(6)?,
                observed_by_office_id: r.get(7)?,
                coordinator_generation: r.get::<_, i64>(8)? as u64,
            })
        })
        .map_err(OfficeError::storage)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(OfficeError::storage)?);
    }
    Ok(out)
}

pub(super) fn query_non_released_permits(
    conn: &Connection,
) -> Result<Vec<TurnPermitRecord>, OfficeError> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT organization_id, id, connection_id, office_id, employee_id, \
             placement_generation, model_id, authority, state, issued_at, expires_at, coordinator_generation \
             FROM connection_turn_permits WHERE state <> 'released' ORDER BY issued_at, id",
        )
        .map_err(OfficeError::storage)?;
    map_permit_rows(&mut stmt)
}

pub(super) fn query_all_permits(conn: &Connection) -> Result<Vec<TurnPermitRecord>, OfficeError> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT organization_id, id, connection_id, office_id, employee_id, \
             placement_generation, model_id, authority, state, issued_at, expires_at, coordinator_generation \
             FROM connection_turn_permits ORDER BY issued_at, id",
        )
        .map_err(OfficeError::storage)?;
    map_permit_rows(&mut stmt)
}

fn map_permit_rows(
    stmt: &mut rusqlite::CachedStatement<'_>,
) -> Result<Vec<TurnPermitRecord>, OfficeError> {
    let rows = stmt
        .query_map([], |r| {
            let auth_s: String = r.get(7)?;
            let state_s: String = r.get(8)?;
            Ok(TurnPermitRecord {
                organization_id: r.get(0)?,
                id: r.get(1)?,
                connection_id: r.get(2)?,
                office_id: r.get(3)?,
                employee_id: r.get(4)?,
                placement_generation: r.get::<_, i64>(5)? as u64,
                model_id: r.get(6)?,
                authority: PermitAuthority::parse(&auth_s).unwrap_or(PermitAuthority::OfficeLocal),
                state: TurnPermitState::parse(&state_s).unwrap_or(TurnPermitState::ReleasePending),
                issued_at: r.get(9)?,
                expires_at: r.get(10)?,
                coordinator_generation: r.get::<_, i64>(11)? as u64,
            })
        })
        .map_err(OfficeError::storage)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(OfficeError::storage)?);
    }
    Ok(out)
}

pub(super) fn row_to_binding(r: &rusqlite::Row<'_>) -> rusqlite::Result<OfficeConnectionBinding> {
    Ok(OfficeConnectionBinding {
        organization_id: r.get(0)?,
        office_id: r.get(1)?,
        connection_id: r.get(2)?,
        revision: r.get::<_, i64>(3)? as u64,
        auth_context_ref: r.get(4)?,
        runtime_binary_path: r.get(5)?,
        account_identity_ref: r.get(6)?,
        updated_at: r.get(7)?,
        last_operation_id: r.get(8)?,
        last_operation_hash: r.get(9)?,
    })
}

pub(super) fn row_to_state(r: &rusqlite::Row<'_>) -> rusqlite::Result<OfficeConnectionState> {
    let status_s: String = r.get(3)?;
    Ok(OfficeConnectionState {
        organization_id: r.get(0)?,
        office_id: r.get(1)?,
        connection_id: r.get(2)?,
        status: ConnectionReadinessStatus::parse(&status_s)
            .unwrap_or(ConnectionReadinessStatus::Unknown),
        binding_revision: r.get::<_, i64>(4)? as u64,
        driver_version: r.get(5)?,
        observed_at: r.get(6)?,
        valid_until: r.get(7)?,
        issue_code: r.get(8)?,
    })
}

pub(super) fn row_to_model(r: &rusqlite::Row<'_>) -> rusqlite::Result<OfficeConnectionModel> {
    let ent_s: String = r.get(5)?;
    Ok(OfficeConnectionModel {
        organization_id: r.get(0)?,
        office_id: r.get(1)?,
        connection_id: r.get(2)?,
        model_id: r.get(3)?,
        driver_model_id: r.get(4)?,
        entitlement: DiscoveredModelEntitlement::parse(&ent_s)
            .unwrap_or(DiscoveredModelEntitlement::Unknown),
        capabilities_json: r.get(6)?,
        observed_at: r.get(7)?,
        valid_until: r.get(8)?,
    })
}
