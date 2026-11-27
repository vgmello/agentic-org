//! [`ProviderPoolStore`] implementation over the isolated `providers.db` projection.

use std::sync::atomic::Ordering;

use org_proto::providers::TurnPermitState;
use org_proto::{ApiError, ErrorCode};
use rusqlite::{params, OptionalExtension};

use org_office::private_fs::private_dir_synced;
use org_office::providers::{
    ConnectionQuotaState, OfficeConnectionBinding, OfficeConnectionModel, OfficeConnectionState,
    PermitAcquireOutcome, PermitAcquireRequest, PermitSnapshot, ProviderPoolStore,
    TurnPermitRecord, PERMIT_LEASE_MS,
};
use org_office::OfficeError;

use super::files::write_json_atomic;
use super::queries::{
    query_active_cooldown, query_all_cooldowns, query_all_permits, row_to_binding, row_to_model,
    row_to_state,
};
use super::writes::{
    expire_overdue_sqlite, upsert_binding_tx, upsert_cooldown_tx, upsert_permit_tx,
};
use super::SqliteProviderPoolStore;

impl ProviderPoolStore for SqliteProviderPoolStore {
    fn get_binding(
        &self,
        org_id: &str,
        office_id: &str,
        connection_id: &str,
    ) -> Result<Option<OfficeConnectionBinding>, OfficeError> {
        let conn = self.lock_db();
        let mut stmt = conn
            .prepare_cached(
                "SELECT organization_id, office_id, connection_id, revision, auth_context_ref, \
                 runtime_binary_path, account_identity_ref, updated_at, last_operation_id, last_operation_hash \
                 FROM office_connection_bindings WHERE organization_id=?1 AND office_id=?2 AND connection_id=?3",
            )
            .map_err(OfficeError::storage)?;
        let mut rows = stmt
            .query_map(params![org_id, office_id, connection_id], row_to_binding)
            .map_err(OfficeError::storage)?;
        rows.next().transpose().map_err(OfficeError::storage)
    }

    fn list_bindings(
        &self,
        org_id: &str,
        office_id: &str,
    ) -> Result<Vec<OfficeConnectionBinding>, OfficeError> {
        let conn = self.lock_db();
        let mut stmt = conn
            .prepare_cached(
                "SELECT organization_id, office_id, connection_id, revision, auth_context_ref, \
                 runtime_binary_path, account_identity_ref, updated_at, last_operation_id, last_operation_hash \
                 FROM office_connection_bindings WHERE organization_id=?1 AND office_id=?2 ORDER BY connection_id",
            )
            .map_err(OfficeError::storage)?;
        let rows = stmt
            .query_map(params![org_id, office_id], row_to_binding)
            .map_err(OfficeError::storage)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(OfficeError::storage)?);
        }
        Ok(out)
    }

    fn save_binding_and_invalidate(
        &self,
        binding: &OfficeConnectionBinding,
    ) -> Result<(), OfficeError> {
        let mut conn = self.lock_db();
        let path = self.binding_path(&binding.connection_id);
        if let Some(parent) = path.parent() {
            private_dir_synced(&self.bindings_dir, parent).map_err(OfficeError::storage)?;
        }
        write_json_atomic(&path, binding)?;
        let tx = conn.transaction().map_err(OfficeError::storage)?;
        upsert_binding_tx(&tx, binding)?;
        tx.execute(
            "DELETE FROM office_connection_state WHERE organization_id=?1 AND office_id=?2 AND connection_id=?3",
            params![&binding.organization_id, &binding.office_id, &binding.connection_id],
        )
        .map_err(OfficeError::storage)?;
        tx.execute(
            "DELETE FROM office_connection_models WHERE organization_id=?1 AND office_id=?2 AND connection_id=?3",
            params![&binding.organization_id, &binding.office_id, &binding.connection_id],
        )
        .map_err(OfficeError::storage)?;
        tx.commit().map_err(OfficeError::storage)?;
        self.durable_writes.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn get_state(
        &self,
        org_id: &str,
        office_id: &str,
        connection_id: &str,
    ) -> Result<Option<OfficeConnectionState>, OfficeError> {
        let conn = self.lock_db();
        let mut stmt = conn
            .prepare_cached(
                "SELECT organization_id, office_id, connection_id, status, binding_revision, \
                 driver_version, observed_at, valid_until, issue_code \
                 FROM office_connection_state WHERE organization_id=?1 AND office_id=?2 AND connection_id=?3",
            )
            .map_err(OfficeError::storage)?;
        let mut rows = stmt
            .query_map(params![org_id, office_id, connection_id], row_to_state)
            .map_err(OfficeError::storage)?;
        rows.next().transpose().map_err(OfficeError::storage)
    }

    fn list_models(
        &self,
        org_id: &str,
        office_id: &str,
        connection_id: &str,
    ) -> Result<Vec<OfficeConnectionModel>, OfficeError> {
        let conn = self.lock_db();
        let mut stmt = conn
            .prepare_cached(
                "SELECT organization_id, office_id, connection_id, model_id, driver_model_id, \
                 entitlement, capabilities_json, observed_at, valid_until \
                 FROM office_connection_models WHERE organization_id=?1 AND office_id=?2 AND connection_id=?3 \
                 ORDER BY model_id",
            )
            .map_err(OfficeError::storage)?;
        let rows = stmt
            .query_map(params![org_id, office_id, connection_id], row_to_model)
            .map_err(OfficeError::storage)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(OfficeError::storage)?);
        }
        Ok(out)
    }

    fn save_observation(
        &self,
        state: &OfficeConnectionState,
        models: &[OfficeConnectionModel],
    ) -> Result<(), OfficeError> {
        let mut conn = self.lock_db();
        let tx = conn.transaction().map_err(OfficeError::storage)?;
        tx.execute(
            "INSERT INTO office_connection_state(\
             organization_id, office_id, connection_id, status, binding_revision, driver_version, \
             observed_at, valid_until, issue_code) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9) \
             ON CONFLICT(organization_id, office_id, connection_id) DO UPDATE SET \
             status=excluded.status, binding_revision=excluded.binding_revision, \
             driver_version=excluded.driver_version, observed_at=excluded.observed_at, \
             valid_until=excluded.valid_until, issue_code=excluded.issue_code",
            params![
                &state.organization_id,
                &state.office_id,
                &state.connection_id,
                state.status.as_str(),
                state.binding_revision as i64,
                state.driver_version.as_deref(),
                state.observed_at.as_deref(),
                state.valid_until.as_deref(),
                state.issue_code.as_deref(),
            ],
        )
        .map_err(OfficeError::storage)?;
        tx.execute(
            "DELETE FROM office_connection_models WHERE organization_id=?1 AND office_id=?2 AND connection_id=?3",
            params![&state.organization_id, &state.office_id, &state.connection_id],
        )
        .map_err(OfficeError::storage)?;
        for m in models {
            tx.execute(
                "INSERT INTO office_connection_models(\
                 organization_id, office_id, connection_id, model_id, driver_model_id, \
                 entitlement, capabilities_json, observed_at, valid_until) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![
                    &m.organization_id,
                    &m.office_id,
                    &m.connection_id,
                    &m.model_id,
                    &m.driver_model_id,
                    m.entitlement.as_str(),
                    &m.capabilities_json,
                    &m.observed_at,
                    &m.valid_until,
                ],
            )
            .map_err(OfficeError::storage)?;
        }
        tx.commit().map_err(OfficeError::storage)?;
        Ok(())
    }

    fn renew_ready_observation_if_unchanged(
        &self,
        expected_binding_revision: u64,
        state: &OfficeConnectionState,
        models: &[OfficeConnectionModel],
    ) -> Result<bool, OfficeError> {
        let mut conn = self.lock_db();
        let tx = conn.transaction().map_err(OfficeError::storage)?;
        let current_binding_rev: Option<i64> = tx
            .query_row(
                "SELECT revision FROM office_connection_bindings \
                 WHERE organization_id=?1 AND office_id=?2 AND connection_id=?3",
                params![
                    &state.organization_id,
                    &state.office_id,
                    &state.connection_id
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(OfficeError::storage)?;
        let current_state: Option<(String, i64)> = tx
            .query_row(
                "SELECT status, binding_revision FROM office_connection_state \
                 WHERE organization_id=?1 AND office_id=?2 AND connection_id=?3",
                params![
                    &state.organization_id,
                    &state.office_id,
                    &state.connection_id
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(OfficeError::storage)?;
        let expected = expected_binding_revision as i64;
        if current_binding_rev != Some(expected)
            || current_state
                .as_ref()
                .is_none_or(|(status, rev)| status != "ready" || *rev != expected)
        {
            return Ok(false);
        }
        tx.execute(
            "UPDATE office_connection_state SET \
             driver_version=?4, observed_at=?5, valid_until=?6, issue_code=?7 \
             WHERE organization_id=?1 AND office_id=?2 AND connection_id=?3",
            params![
                &state.organization_id,
                &state.office_id,
                &state.connection_id,
                state.driver_version.as_deref(),
                state.observed_at.as_deref(),
                state.valid_until.as_deref(),
                state.issue_code.as_deref(),
            ],
        )
        .map_err(OfficeError::storage)?;
        tx.execute(
            "DELETE FROM office_connection_models WHERE organization_id=?1 AND office_id=?2 AND connection_id=?3",
            params![&state.organization_id, &state.office_id, &state.connection_id],
        )
        .map_err(OfficeError::storage)?;
        for m in models {
            tx.execute(
                "INSERT INTO office_connection_models(\
                 organization_id, office_id, connection_id, model_id, driver_model_id, \
                 entitlement, capabilities_json, observed_at, valid_until) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![
                    &m.organization_id,
                    &m.office_id,
                    &m.connection_id,
                    &m.model_id,
                    &m.driver_model_id,
                    m.entitlement.as_str(),
                    &m.capabilities_json,
                    &m.observed_at,
                    &m.valid_until,
                ],
            )
            .map_err(OfficeError::storage)?;
        }
        tx.commit().map_err(OfficeError::storage)?;
        Ok(true)
    }

    fn coordinator_generation(&self) -> Result<u64, OfficeError> {
        Ok(self.lock_leases().coordinator_generation)
    }

    fn acquire_permit(
        &self,
        req: PermitAcquireRequest,
    ) -> Result<PermitAcquireOutcome, OfficeError> {
        let mut conn = self.lock_db();
        expire_overdue_sqlite(&mut conn, &self.leases, req.now_ms, |tx| {
            self.persist_authority_permits(tx)
        })?;
        if let Some(until) = query_active_cooldown(
            &conn,
            &req.organization_id,
            &req.connection_id,
            &req.model_id,
            req.now_ms,
        )? {
            return Ok(PermitAcquireOutcome::CoolingDown {
                cooldown_until: until,
            });
        }
        let occupied: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM connection_turn_permits \
                 WHERE organization_id=?1 AND connection_id=?2 AND state <> 'released'",
                params![&req.organization_id, &req.connection_id],
                |r| r.get(0),
            )
            .map_err(OfficeError::storage)?;
        if (occupied as u64) >= req.max_concurrent_turns {
            return Ok(PermitAcquireOutcome::CapacityFull {
                occupied: occupied as u64,
                max: req.max_concurrent_turns,
            });
        }
        let coordinator_generation = self.lock_leases().coordinator_generation;
        let permit = TurnPermitRecord {
            organization_id: req.organization_id,
            id: req.permit_id,
            connection_id: req.connection_id,
            office_id: req.office_id,
            employee_id: req.employee_id,
            placement_generation: req.placement_generation,
            model_id: req.model_id,
            authority: req.authority,
            state: TurnPermitState::Active,
            issued_at: req.issued_at,
            expires_at: req.expires_at,
            coordinator_generation,
        };
        let tx = conn.transaction().map_err(OfficeError::storage)?;
        upsert_permit_tx(&tx, &permit)?;
        // Durable-first (`Non-Negotiable #1`): persist authority file BEFORE committing SQLite.
        self.persist_authority_permits(&tx)?;
        tx.commit().map_err(OfficeError::storage)?;
        self.lock_leases().active.insert(
            permit.id.clone(),
            (
                coordinator_generation,
                req.now_ms.saturating_add(PERMIT_LEASE_MS),
            ),
        );
        Ok(PermitAcquireOutcome::Granted(permit))
    }

    fn renew_permit_in_memory(
        &self,
        permit_id: &str,
        coordinator_generation: u64,
        now_ms: u64,
    ) -> Result<Result<u64, ApiError>, OfficeError> {
        let mut leases = self.lock_leases();
        if leases.coordinator_generation != coordinator_generation {
            return Ok(Err(ApiError::new(
                ErrorCode::Conflict,
                "coordinator generation mismatch",
            )));
        }
        let Some(&(p_gen, current_deadline)) = leases.active.get(permit_id) else {
            return Ok(Err(ApiError::new(
                ErrorCode::NotFound,
                "active permit not found",
            )));
        };
        if p_gen != coordinator_generation {
            return Ok(Err(ApiError::new(
                ErrorCode::Conflict,
                "permit generation mismatch",
            )));
        }
        if now_ms >= current_deadline {
            leases.active.remove(permit_id);
            return Ok(Err(ApiError::new(
                ErrorCode::Conflict,
                "PERMIT_EXPIRED: turn permit lease has already expired",
            )));
        }
        let next_deadline = now_ms.saturating_add(PERMIT_LEASE_MS);
        leases
            .active
            .insert(permit_id.to_owned(), (p_gen, next_deadline));
        Ok(Ok(next_deadline))
    }

    fn mark_permit_release_pending(&self, permit_id: &str) -> Result<bool, OfficeError> {
        let mut conn = self.lock_db();
        let tx = conn.transaction().map_err(OfficeError::storage)?;
        let changed = tx
            .execute(
                "UPDATE connection_turn_permits SET state='release_pending' \
                 WHERE id=?1 AND state='active'",
                params![permit_id],
            )
            .map_err(OfficeError::storage)?;
        if changed > 0 {
            self.persist_authority_permits(&tx)?;
            tx.commit().map_err(OfficeError::storage)?;
            self.lock_leases().active.remove(permit_id);
            return Ok(true);
        }
        tx.commit().map_err(OfficeError::storage)?;
        Ok(false)
    }

    fn release_permit(&self, permit_id: &str) -> Result<bool, OfficeError> {
        let mut conn = self.lock_db();
        let tx = conn.transaction().map_err(OfficeError::storage)?;
        let changed = tx
            .execute(
                "UPDATE connection_turn_permits SET state='released' \
                 WHERE id=?1 AND state <> 'released'",
                params![permit_id],
            )
            .map_err(OfficeError::storage)?;
        if changed > 0 {
            self.persist_authority_permits(&tx)?;
            tx.commit().map_err(OfficeError::storage)?;
            self.lock_leases().active.remove(permit_id);
            return Ok(true);
        }
        tx.commit().map_err(OfficeError::storage)?;
        Ok(false)
    }

    fn record_cooldown(&self, state: &ConnectionQuotaState) -> Result<(), OfficeError> {
        let mut conn = self.lock_db();
        let mut next = state.clone();
        next.coordinator_generation = self.lock_leases().coordinator_generation;
        let tx = conn.transaction().map_err(OfficeError::storage)?;
        upsert_cooldown_tx(&tx, &next)?;
        self.persist_authority_cooldowns(&tx)?;
        tx.commit().map_err(OfficeError::storage)?;
        Ok(())
    }

    fn quota_snapshot(
        &self,
        org_id: &str,
        connection_id: Option<&str>,
        now_ms: u64,
    ) -> Result<PermitSnapshot, OfficeError> {
        let mut conn = self.lock_db();
        expire_overdue_sqlite(&mut conn, &self.leases, now_ms, |tx| {
            self.persist_authority_permits(tx)
        })?;
        let coordinator_generation = self.lock_leases().coordinator_generation;
        let cooldowns: Vec<ConnectionQuotaState> = query_all_cooldowns(&conn)?
            .into_iter()
            .filter(|c| {
                c.organization_id == org_id
                    && connection_id.is_none_or(|cid| c.connection_id == cid)
            })
            .collect();
        let permits: Vec<TurnPermitRecord> = query_all_permits(&conn)?
            .into_iter()
            .filter(|p| {
                p.organization_id == org_id
                    && p.state != TurnPermitState::Released
                    && connection_id.is_none_or(|cid| p.connection_id == cid)
            })
            .collect();
        Ok(PermitSnapshot {
            coordinator_generation,
            cooldowns,
            permits,
        })
    }

    fn durable_write_count(&self) -> u64 {
        self.durable_writes.load(Ordering::Relaxed)
    }
}
