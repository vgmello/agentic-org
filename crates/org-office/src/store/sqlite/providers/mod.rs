//! SQLite-backed [`ProviderPoolStore`] with isolated `0700`/`0600` `protected/providers.db`
//! rebuilt automatically from durable office-local bindings and HQ authority files
//! (`TASK-54`, `doc-88` §3/§6, `doc-89`, `TASK-2` non-blocking in-memory permit heartbeats).
//!
//! [`ProviderPoolStore`]: crate::providers::ProviderPoolStore

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use org_proto::providers::TurnPermitState;
use rusqlite::Connection;

use crate::error::OfficeError;
use crate::private_fs::{absolute, private_dir_synced};
use crate::providers::{ConnectionQuotaState, TurnPermitRecord};
use crate::store::layout;

use super::companion::open_and_reconcile_companion_db;

mod files;
mod pool_store;
mod queries;
mod schema;
#[cfg(test)]
mod sqlite_renewal_cas_tests;
mod writes;

use self::files::{
    ensure_dir_0700, load_all_durable_bindings, read_json_optional, write_json_atomic,
};
use self::queries::{query_all_cooldowns, query_non_released_permits};
use self::schema::{PROVIDERS_ADAPTER_FORMAT, PROVIDERS_PRAGMAS, SCHEMA_SQL};
use self::writes::reconcile_db;

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
        let bindings_dir = layout::provider_bindings(durable_root, office_id);
        // Only HQ owns `<durable>/authority/agent-execution/`; satellites persist local offline
        // permits and replicated cooldown caches strictly under their own office-local subtree
        // (`TASK-52` / `ARCH-02`).
        let authority_dir = layout::execution_authority(durable_root, office_id, is_hq);
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
