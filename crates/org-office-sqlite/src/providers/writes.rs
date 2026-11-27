//! SQL writes: overdue permit expiry, durable reconcile and binding/cooldown/permit upserts.

use std::sync::Mutex;

use org_proto::providers::TurnPermitState;
use rusqlite::{params, Connection};

use org_office::providers::{ConnectionQuotaState, OfficeConnectionBinding, TurnPermitRecord};
use org_office::OfficeError;

use super::InMemoryLeases;

pub(super) fn expire_overdue_sqlite(
    conn: &mut Connection,
    leases: &Mutex<InMemoryLeases>,
    now_ms: u64,
    persist: impl FnOnce(&Connection) -> Result<(), OfficeError>,
) -> Result<bool, OfficeError> {
    let active_rows: Vec<(String, String)> = {
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, expires_at FROM connection_turn_permits WHERE state='active'",
            )
            .map_err(OfficeError::storage)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(OfficeError::storage)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(OfficeError::storage)?);
        }
        out
    };
    let mut to_expire = Vec::new();
    {
        let guard = leases.lock().unwrap_or_else(|p| p.into_inner());
        for (id, expires_at) in active_rows {
            let deadline = guard
                .active
                .get(&id)
                .map(|(_, d)| *d)
                .or_else(|| org_proto::parse_rfc3339_ms(&expires_at))
                .unwrap_or(0);
            if now_ms >= deadline {
                to_expire.push(id);
            }
        }
    }
    if to_expire.is_empty() {
        return Ok(false);
    }
    let tx = conn.transaction().map_err(OfficeError::storage)?;
    for id in &to_expire {
        tx.execute(
            "UPDATE connection_turn_permits SET state='release_pending' WHERE id=?1 AND state='active'",
            params![id],
        )
        .map_err(OfficeError::storage)?;
    }
    persist(&tx)?;
    tx.commit().map_err(OfficeError::storage)?;
    let mut guard = leases.lock().unwrap_or_else(|p| p.into_inner());
    for id in &to_expire {
        guard.active.remove(id);
    }
    Ok(true)
}

pub(super) fn reconcile_db(
    conn: &mut Connection,
    coordinator_generation: u64,
    bindings: &[OfficeConnectionBinding],
    cooldowns: &[ConnectionQuotaState],
    permits: &[TurnPermitRecord],
) -> Result<(), OfficeError> {
    let tx = conn.transaction().map_err(OfficeError::storage)?;
    // Rebuild from authoritative durable files so no stale rows survive a crash between writes.
    tx.execute("DELETE FROM office_connection_bindings", [])
        .map_err(OfficeError::storage)?;
    tx.execute("DELETE FROM connection_quota_state", [])
        .map_err(OfficeError::storage)?;
    tx.execute("DELETE FROM connection_turn_permits", [])
        .map_err(OfficeError::storage)?;
    tx.execute(
        "INSERT INTO coordinator_meta(singleton, coordinator_generation) VALUES (1, ?1) \
         ON CONFLICT(singleton) DO UPDATE SET coordinator_generation=excluded.coordinator_generation",
        params![coordinator_generation as i64],
    )
    .map_err(OfficeError::storage)?;
    for b in bindings {
        upsert_binding_tx(&tx, b)?;
    }
    for c in cooldowns {
        upsert_cooldown_tx(&tx, c)?;
    }
    for p in permits {
        if p.state != TurnPermitState::Released {
            upsert_permit_tx(&tx, p)?;
        }
    }
    tx.commit().map_err(OfficeError::storage)?;
    Ok(())
}

pub(super) fn upsert_binding_tx(
    tx: &rusqlite::Transaction<'_>,
    b: &OfficeConnectionBinding,
) -> Result<(), OfficeError> {
    tx.execute(
        "INSERT INTO office_connection_bindings(\
         organization_id, office_id, connection_id, revision, auth_context_ref, \
         runtime_binary_path, account_identity_ref, updated_at, last_operation_id, last_operation_hash) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) \
         ON CONFLICT(organization_id, office_id, connection_id) DO UPDATE SET \
         revision=excluded.revision, auth_context_ref=excluded.auth_context_ref, \
         runtime_binary_path=excluded.runtime_binary_path, \
         account_identity_ref=excluded.account_identity_ref, updated_at=excluded.updated_at, \
         last_operation_id=excluded.last_operation_id, last_operation_hash=excluded.last_operation_hash",
        params![
            &b.organization_id,
            &b.office_id,
            &b.connection_id,
            b.revision as i64,
            &b.auth_context_ref,
            b.runtime_binary_path.as_deref(),
            b.account_identity_ref.as_deref(),
            &b.updated_at,
            b.last_operation_id.as_deref(),
            b.last_operation_hash.as_deref(),
        ],
    )
    .map_err(OfficeError::storage)?;
    Ok(())
}

pub(super) fn upsert_cooldown_tx(
    tx: &rusqlite::Transaction<'_>,
    c: &ConnectionQuotaState,
) -> Result<(), OfficeError> {
    tx.execute(
        "INSERT INTO connection_quota_state(\
         organization_id, connection_id, scope_key, model_id, status, cooldown_until, \
         observed_at, observed_by_office_id, coordinator_generation) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9) \
         ON CONFLICT(organization_id, connection_id, scope_key) DO UPDATE SET \
         model_id=excluded.model_id, status=excluded.status, cooldown_until=excluded.cooldown_until, \
         observed_at=excluded.observed_at, observed_by_office_id=excluded.observed_by_office_id, \
         coordinator_generation=excluded.coordinator_generation",
        params![
            &c.organization_id,
            &c.connection_id,
            &c.scope_key,
            c.model_id.as_deref(),
            c.status.as_str(),
            c.cooldown_until.as_deref(),
            &c.observed_at,
            &c.observed_by_office_id,
            c.coordinator_generation as i64,
        ],
    )
    .map_err(OfficeError::storage)?;
    Ok(())
}

pub(super) fn upsert_permit_tx(
    tx: &rusqlite::Transaction<'_>,
    p: &TurnPermitRecord,
) -> Result<(), OfficeError> {
    tx.execute(
        "INSERT INTO connection_turn_permits(\
         organization_id, id, connection_id, office_id, employee_id, placement_generation, \
         model_id, authority, state, issued_at, expires_at, coordinator_generation) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12) \
         ON CONFLICT(organization_id, id) DO UPDATE SET \
         state=excluded.state, expires_at=excluded.expires_at, \
         coordinator_generation=excluded.coordinator_generation",
        params![
            &p.organization_id,
            &p.id,
            &p.connection_id,
            &p.office_id,
            &p.employee_id,
            p.placement_generation as i64,
            &p.model_id,
            p.authority.as_str(),
            p.state.as_str(),
            &p.issued_at,
            &p.expires_at,
            p.coordinator_generation as i64,
        ],
    )
    .map_err(OfficeError::storage)?;
    Ok(())
}
