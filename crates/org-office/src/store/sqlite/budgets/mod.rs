//! Isolated SQLite + durable-file adapter for `L3` budgets (`TASK-53`, `doc-92`, `ADR-0016`).
//!
//! Loads `docs/design/organization-data/budgets-schema.sql` VERBATIM into
//! `<runtime>/<office-id>/protected/budgets.db` (owner-only 0700/0600, never `office.db`).
//! Durable source files live under `<durable>/budgets/<budget-id>/` (never in shared org Git),
//! enabling full delete-and-rebuild of `protected/budgets.db`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use rusqlite::Connection;

use crate::budgets::BudgetAggregate;
use crate::error::OfficeError;
use crate::private_fs::{absolute, private_dir, private_dir_synced, sync_dir, tighten_dir};
use crate::store::layout;

use super::companion::open_and_reconcile_companion_db;

mod budget_store;
mod durable;
mod guards;
mod load;
mod project;

use self::durable::{reconcile_durable_conn, write_json_atomic};

/// Verbatim DDL from `docs/design/organization-data/budgets-schema.sql` (`doc-92`, `TASK-10`).
const BUDGETS_DDL: &str =
    include_str!("../../../../../../docs/design/organization-data/budgets-schema.sql");
/// Adapter-owned crash-recovery table for in-flight `budget.stop` intents (`TASK-97`).
const BUDGETS_ADAPTER_DDL: &str = "
CREATE TABLE IF NOT EXISTS budget_stop_intents (
  organization_id TEXT NOT NULL,
  budget_id TEXT NOT NULL,
  intent_json TEXT NOT NULL,
  PRIMARY KEY (organization_id, budget_id)
) STRICT;
";
const BUDGETS_ADAPTER_FORMAT: u32 = 2;
const BUDGETS_PRAGMAS: &str = "
PRAGMA journal_mode = WAL;
PRAGMA synchronous = FULL;
PRAGMA foreign_keys = ON;
PRAGMA recursive_triggers = ON;
PRAGMA busy_timeout = 5000;
";

/// Isolated durable + SQLite [`BudgetStore`].
///
/// [`BudgetStore`]: crate::budgets::BudgetStore
pub struct SqliteBudgetStore {
    durable_root: PathBuf,
    needs_reconcile: AtomicBool,
    conn: Mutex<Connection>,
}

impl SqliteBudgetStore {
    /// Opens `<runtime_root>/<office_id>/protected/budgets.db`, rebuilding automatically from
    /// `<durable_root>/budgets/` if the projection is missing, empty, corrupt, or schema-mismatched.
    pub fn open(
        durable_root: &Path,
        runtime_root: &Path,
        office_id: &str,
        org_id: &str,
    ) -> Result<Self, OfficeError> {
        let durable_budgets = layout::budgets(durable_root);
        private_dir_synced(durable_root, &durable_budgets).map_err(OfficeError::storage)?;
        let office_dir = runtime_root.join(office_id);
        private_dir(&office_dir).map_err(OfficeError::storage)?;
        let protected = office_dir.join("protected");
        private_dir(&protected).map_err(OfficeError::storage)?;
        tighten_dir(&protected).map_err(OfficeError::storage)?;

        let db_path = absolute(protected.join("budgets.db")).map_err(OfficeError::storage)?;
        let full_ddl = format!("{BUDGETS_DDL}\n{BUDGETS_ADAPTER_DDL}");
        let conn = open_and_reconcile_companion_db(
            &db_path,
            "budgets",
            BUDGETS_ADAPTER_FORMAT,
            &full_ddl,
            BUDGETS_PRAGMAS,
            |conn| reconcile_durable_conn(conn, &durable_budgets, org_id),
        )?;

        Ok(Self {
            durable_root: durable_budgets,
            needs_reconcile: AtomicBool::new(false),
            conn: Mutex::new(conn),
        })
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, OfficeError> {
        self.conn
            .lock()
            .map_err(|_| OfficeError::Invalid("budgets sqlite connection poisoned"))
    }

    fn ensure_reconciled(&self, conn: &mut Connection, org_id: &str) -> Result<(), OfficeError> {
        if self.needs_reconcile.load(Ordering::Acquire) {
            reconcile_durable_conn(conn, &self.durable_root, org_id)?;
            self.needs_reconcile.store(false, Ordering::Release);
        }
        Ok(())
    }

    fn write_durable(&self, agg: &BudgetAggregate) -> Result<(), OfficeError> {
        let dir = self.durable_root.join(&agg.budget.id);
        private_dir_synced(&self.durable_root, &dir).map_err(OfficeError::storage)?;
        let agg_path = dir.join("aggregate.v1.json");
        if agg_path.is_file() {
            // Historical committed versions are immutable: fail closed if an update tries to
            // alter any already-committed policy revision (`TASK-97` / Pass 2 Finding #2).
            let prev: BudgetAggregate =
                serde_json::from_slice(&fs::read(&agg_path).map_err(OfficeError::storage)?)
                    .map_err(OfficeError::storage)?;
            for old_ver in &prev.versions {
                let matched = agg.versions.iter().find(|v| v.revision == old_ver.revision);
                if matched != Some(old_ver) {
                    return Err(OfficeError::Invalid(
                        "conflicting immutable budget policy version",
                    ));
                }
            }
        }
        // Single atomic rename + `F_FULLFSYNC` publishes budget, versions, periods, events,
        // notices, and holds together with zero torn-child window (`TASK-97` / Pass 2 Finding #2).
        write_json_atomic(&agg_path, agg)?;
        sync_dir(&self.durable_root).map_err(OfficeError::storage)
    }
}
