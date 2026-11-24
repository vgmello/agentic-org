//! SQLite-backed [`ProviderPoolStore`] with isolated `0700`/`0600` `protected/providers.db`
//! rebuilt automatically from durable office-local bindings and HQ authority files
//! (`TASK-54`, `doc-88` §3/§6, `doc-89`, `TASK-2` non-blocking in-memory permit heartbeats).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use org_proto::providers::{
    ConnectionQuotaStatus, ConnectionReadinessStatus, DiscoveredModelEntitlement, PermitAuthority,
    TurnPermitState,
};
use org_proto::{ApiError, ErrorCode};
use rusqlite::{params, Connection};

use crate::error::OfficeError;
use crate::private_fs::{
    absolute, private_dir, private_dir_synced, tighten_dir, write_private_atomic,
};
use crate::providers::{
    ConnectionQuotaState, OfficeConnectionBinding, OfficeConnectionModel, OfficeConnectionState,
    PermitAcquireOutcome, PermitAcquireRequest, PermitSnapshot, ProviderPoolStore,
    TurnPermitRecord, PERMIT_LEASE_MS,
};

use super::companion::open_and_reconcile_companion_db;

const PROVIDERS_ADAPTER_FORMAT: u32 = 1;
const PROVIDERS_PRAGMAS: &str = "
PRAGMA journal_mode = WAL;
PRAGMA synchronous = FULL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
";

const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS coordinator_meta (
  singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
  coordinator_generation INTEGER NOT NULL CHECK (coordinator_generation >= 1)
) STRICT;

CREATE TABLE IF NOT EXISTS office_connection_bindings (
  organization_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  revision INTEGER NOT NULL CHECK (revision >= 1),
  auth_context_ref TEXT NOT NULL CHECK (length(auth_context_ref) > 0),
  runtime_binary_path TEXT,
  account_identity_ref TEXT,
  updated_at TEXT NOT NULL,
  last_operation_id TEXT,
  last_operation_hash TEXT,
  PRIMARY KEY (organization_id, office_id, connection_id)
) STRICT;

CREATE TABLE IF NOT EXISTS office_connection_state (
  organization_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('unknown','ready','auth_expired','unavailable')),
  binding_revision INTEGER NOT NULL CHECK (binding_revision >= 1),
  driver_version TEXT,
  observed_at TEXT,
  valid_until TEXT,
  issue_code TEXT,
  PRIMARY KEY (organization_id, office_id, connection_id),
  CHECK (status <> 'ready' OR (driver_version IS NOT NULL AND observed_at IS NOT NULL AND valid_until IS NOT NULL))
) STRICT;

CREATE TABLE IF NOT EXISTS office_connection_models (
  organization_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  model_id TEXT NOT NULL,
  driver_model_id TEXT NOT NULL,
  entitlement TEXT NOT NULL CHECK (entitlement IN ('confirmed','denied','unknown')),
  capabilities_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(capabilities_json) AND json_type(capabilities_json) = 'object'),
  observed_at TEXT NOT NULL,
  valid_until TEXT NOT NULL,
  PRIMARY KEY (organization_id, office_id, connection_id, model_id)
) STRICT;

CREATE TABLE IF NOT EXISTS connection_quota_state (
  organization_id TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  scope_key TEXT NOT NULL,
  model_id TEXT,
  status TEXT NOT NULL CHECK (status IN ('unknown','open','cooldown')),
  cooldown_until TEXT,
  observed_at TEXT NOT NULL,
  observed_by_office_id TEXT NOT NULL,
  coordinator_generation INTEGER NOT NULL CHECK (coordinator_generation >= 1),
  PRIMARY KEY (organization_id, connection_id, scope_key),
  CHECK ((model_id IS NULL AND scope_key = 'connection') OR (model_id IS NOT NULL AND scope_key = 'model:' || model_id)),
  CHECK (status <> 'cooldown' OR cooldown_until IS NOT NULL)
) STRICT;

CREATE TABLE IF NOT EXISTS connection_turn_permits (
  organization_id TEXT NOT NULL,
  id TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  office_id TEXT NOT NULL,
  employee_id TEXT NOT NULL,
  placement_generation INTEGER NOT NULL CHECK (placement_generation >= 1),
  model_id TEXT NOT NULL,
  authority TEXT NOT NULL CHECK (authority IN ('hq_pool','office_local')),
  state TEXT NOT NULL CHECK (state IN ('active','release_pending','released')),
  issued_at TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  coordinator_generation INTEGER NOT NULL CHECK (coordinator_generation >= 1),
  PRIMARY KEY (organization_id, id)
) STRICT;
"#;

#[derive(serde::Serialize, serde::Deserialize)]
struct CoordinatorMetaFile {
    coordinator_generation: u64,
}

#[derive(Default)]
struct InMemoryLeases {
    coordinator_generation: u64,
    /// `permit_id -> (coordinator_generation, expires_at_ms)` for active permits.
    active: BTreeMap<String, (u64, u64)>,
}

pub struct SqliteProviderPoolStore {
    conn: Mutex<Connection>,
    /// Separate lightweight mutex for in-memory heartbeat renewals (ZERO SQLite/fsync blocking).
    leases: Mutex<InMemoryLeases>,
    bindings_dir: PathBuf,
    authority_dir: PathBuf,
    durable_writes: AtomicU64,
}

impl SqliteProviderPoolStore {
    pub fn open(
        durable_root: &Path,
        runtime_root: &Path,
        office_id: &str,
        org_id: &str,
        is_hq: bool,
    ) -> Result<Self, OfficeError> {
        let bindings_dir = durable_root
            .join("local")
            .join("offices")
            .join(office_id)
            .join("connections");
        // Only HQ owns `<durable>/authority/agent-execution/`; satellites persist local offline
        // permits and replicated cooldown caches strictly under their own office-local subtree
        // (`TASK-52` / `ARCH-02`).
        let authority_dir = if is_hq {
            durable_root.join("authority").join("agent-execution")
        } else {
            durable_root
                .join("local")
                .join("offices")
                .join(office_id)
                .join("local-execution")
        };
        let protected_dir = runtime_root.join(office_id).join("protected");
        private_dir_synced(durable_root, &bindings_dir).map_err(OfficeError::storage)?;
        private_dir_synced(durable_root, &authority_dir).map_err(OfficeError::storage)?;
        ensure_dir_0700(&protected_dir)?;

        // Coordinator startup (`doc-88` §6): increment generation and move prior `active` permits
        // to `release_pending` durably before admitting any new permit.
        let coord_path = authority_dir.join("coordinator.json");
        let prev_gen = read_json_optional::<CoordinatorMetaFile>(&coord_path)?
            .map_or(0, |m| m.coordinator_generation);
        let coordinator_generation = prev_gen.saturating_add(1).max(1);
        write_json_atomic(
            &coord_path,
            &CoordinatorMetaFile {
                coordinator_generation,
            },
        )?;

        let mut permits =
            read_json_optional::<Vec<TurnPermitRecord>>(&authority_dir.join("permits.json"))?
                .unwrap_or_default();
        let mut transitioned = false;
        for p in &mut permits {
            if p.state == TurnPermitState::Active {
                p.state = TurnPermitState::ReleasePending;
                transitioned = true;
            }
        }
        if transitioned {
            write_json_atomic(&authority_dir.join("permits.json"), &permits)?;
        }

        let cooldowns =
            read_json_optional::<Vec<ConnectionQuotaState>>(&authority_dir.join("cooldowns.json"))?
                .unwrap_or_default();
        let bindings = load_all_durable_bindings(&bindings_dir, org_id, office_id)?;

        let db_path = absolute(protected_dir.join("providers.db")).map_err(OfficeError::storage)?;
        let conn = open_and_reconcile_companion_db(
            &db_path,
            "providers",
            PROVIDERS_ADAPTER_FORMAT,
            SCHEMA_SQL,
            PROVIDERS_PRAGMAS,
            |conn| {
                reconcile_db(
                    conn,
                    coordinator_generation,
                    &bindings,
                    &cooldowns,
                    &permits,
                )
            },
        )?;

        Ok(Self {
            conn: Mutex::new(conn),
            leases: Mutex::new(InMemoryLeases {
                coordinator_generation,
                active: BTreeMap::new(),
            }),
            bindings_dir,
            authority_dir,
            durable_writes: AtomicU64::new(0),
        })
    }

    fn lock_db(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn lock_leases(&self) -> std::sync::MutexGuard<'_, InMemoryLeases> {
        self.leases.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn binding_path(&self, connection_id: &str) -> PathBuf {
        self.bindings_dir.join(connection_id).join("binding.yaml")
    }

    fn persist_authority_permits(&self, conn: &Connection) -> Result<(), OfficeError> {
        let all = query_non_released_permits(conn)?;
        write_json_atomic(&self.authority_dir.join("permits.json"), &all)?;
        self.durable_writes.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn persist_authority_cooldowns(&self, conn: &Connection) -> Result<(), OfficeError> {
        let all = query_all_cooldowns(conn)?;
        write_json_atomic(&self.authority_dir.join("cooldowns.json"), &all)?;
        self.durable_writes.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

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

fn expire_overdue_sqlite(
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

fn reconcile_db(
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

fn upsert_binding_tx(
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

fn upsert_cooldown_tx(
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

fn upsert_permit_tx(
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

fn query_active_cooldown(
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

fn query_all_cooldowns(conn: &Connection) -> Result<Vec<ConnectionQuotaState>, OfficeError> {
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

fn query_non_released_permits(conn: &Connection) -> Result<Vec<TurnPermitRecord>, OfficeError> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT organization_id, id, connection_id, office_id, employee_id, \
             placement_generation, model_id, authority, state, issued_at, expires_at, coordinator_generation \
             FROM connection_turn_permits WHERE state <> 'released' ORDER BY issued_at, id",
        )
        .map_err(OfficeError::storage)?;
    map_permit_rows(&mut stmt)
}

fn query_all_permits(conn: &Connection) -> Result<Vec<TurnPermitRecord>, OfficeError> {
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

fn row_to_binding(r: &rusqlite::Row<'_>) -> rusqlite::Result<OfficeConnectionBinding> {
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

fn row_to_state(r: &rusqlite::Row<'_>) -> rusqlite::Result<OfficeConnectionState> {
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

fn row_to_model(r: &rusqlite::Row<'_>) -> rusqlite::Result<OfficeConnectionModel> {
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

fn load_all_durable_bindings(
    bindings_dir: &Path,
    org_id: &str,
    office_id: &str,
) -> Result<Vec<OfficeConnectionBinding>, OfficeError> {
    let mut out = Vec::new();
    if !bindings_dir.is_dir() {
        return Ok(out);
    }
    for entry in fs::read_dir(bindings_dir).map_err(OfficeError::storage)? {
        let entry = entry.map_err(OfficeError::storage)?;
        let path = entry.path().join("binding.yaml");
        if path.is_file() {
            if let Some(b) = read_json_optional::<OfficeConnectionBinding>(&path)? {
                if b.organization_id == org_id && b.office_id == office_id {
                    out.push(b);
                }
            }
        }
    }
    Ok(out)
}

fn read_json_optional<T: serde::de::DeserializeOwned>(
    path: &Path,
) -> Result<Option<T>, OfficeError> {
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(path).map_err(OfficeError::storage)?;
    let parsed = serde_json::from_slice(&bytes).map_err(OfficeError::storage)?;
    Ok(Some(parsed))
}

fn write_json_atomic<T: serde::Serialize>(path: &Path, val: &T) -> Result<(), OfficeError> {
    let bytes = serde_json::to_vec_pretty(val).map_err(OfficeError::storage)?;
    write_private_atomic(path, &bytes).map_err(OfficeError::storage)
}

fn ensure_dir_0700(dir: &Path) -> Result<(), OfficeError> {
    private_dir(dir).map_err(OfficeError::storage)?;
    tighten_dir(dir).map_err(OfficeError::storage)
}
