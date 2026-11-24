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

use org_proto::budgets::{
    BudgetHoldState, BudgetPeriodKind, BudgetPolicyState, BudgetScopeKind, BudgetStage,
    BudgetTimeZoneSource, BudgetUnit,
};
use org_proto::costs::CompletenessStatus;
use org_proto::ApiError;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::budgets::{
    BudgetAggregate, BudgetEventRecord, BudgetHoldRecord, BudgetNoticeRecord, BudgetPeriodRecord,
    BudgetPolicyVersion, BudgetRecord, BudgetStore, PendingStopIntent,
};
use crate::error::OfficeError;
use crate::private_fs::{
    absolute, full_sync, private_dir, private_dir_synced, sync_dir, tighten_dir,
    write_private_atomic,
};

use super::companion::open_and_reconcile_companion_db;

/// Verbatim DDL from `docs/design/organization-data/budgets-schema.sql` (`doc-92`, `TASK-10`).
const BUDGETS_DDL: &str =
    include_str!("../../../../../docs/design/organization-data/budgets-schema.sql");
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
        let durable_budgets = durable_root.join("budgets");
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

impl BudgetStore for SqliteBudgetStore {
    fn save_aggregate(
        &self,
        agg: &BudgetAggregate,
        employee_kinds: &[(String, &'static str)],
    ) -> Result<Result<(), ApiError>, OfficeError> {
        let mut conn = self.lock()?;
        self.ensure_reconciled(&mut conn, &agg.budget.organization_id)?;
        // Validate integer bounds, target uniqueness, and monotonic revision under the writer
        // lock BEFORE writing anything so an invalid aggregate can never poison durable storage
        // (`TASK-97` / Pass 4 #2 & Final Gate #1).
        if let Some(err) = validate_aggregate_sqlite_bounds(agg) {
            return Ok(Err(err));
        }
        if let Some(conflict) = check_target_and_revision_under_lock(&conn, agg)? {
            return Ok(Err(conflict));
        }
        // Stage and verify all SQLite CHECK, UNIQUE, trigger, and deferred FK constraints inside
        // an uncommitted transaction BEFORE publishing `aggregate.v1.json` so a rejected
        // aggregate can never brick future office reopens (`TASK-97` / Final Gate #1).
        let tx = conn.transaction().map_err(OfficeError::storage)?;
        project_aggregate(&tx, agg, employee_kinds)?;
        let fk_violation: Option<String> = tx
            .query_row("PRAGMA foreign_key_check", [], |r| r.get(0))
            .optional()
            .map_err(OfficeError::storage)?;
        if let Some(table) = fk_violation {
            return Err(OfficeError::storage(std::io::Error::other(format!(
                "budget projection foreign_key_check failed on table `{table}`"
            ))));
        }
        if let Err(err) = self.write_durable(agg) {
            self.needs_reconcile.store(true, Ordering::Release);
            return Err(err);
        }
        if let Err(err) = tx.commit().map_err(OfficeError::storage) {
            self.needs_reconcile.store(true, Ordering::Release);
            return Err(err);
        }
        Ok(Ok(()))
    }

    fn get_by_id(
        &self,
        org_id: &str,
        budget_id: &str,
    ) -> Result<Option<BudgetAggregate>, OfficeError> {
        let mut conn = self.lock()?;
        self.ensure_reconciled(&mut conn, org_id)?;
        load_aggregate_from_db(&conn, org_id, budget_id)
    }

    fn find_by_target(
        &self,
        org_id: &str,
        scope_kind: BudgetScopeKind,
        task_id: Option<&str>,
        agent_id: Option<&str>,
        release_id: Option<&str>,
        period_kind: BudgetPeriodKind,
    ) -> Result<Option<BudgetAggregate>, OfficeError> {
        let mut conn = self.lock()?;
        self.ensure_reconciled(&mut conn, org_id)?;
        let found: Option<String> = match scope_kind {
            BudgetScopeKind::Task => conn
                .query_row(
                    "SELECT id FROM budgets WHERE organization_id=?1 AND scope_kind='task' AND task_id=?2",
                    params![org_id, task_id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(OfficeError::storage)?,
            BudgetScopeKind::Agent => conn
                .query_row(
                    "SELECT id FROM budgets WHERE organization_id=?1 AND scope_kind='agent' AND agent_id=?2 AND period_kind=?3",
                    params![org_id, agent_id, period_kind.as_str()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(OfficeError::storage)?,
            BudgetScopeKind::Release => conn
                .query_row(
                    "SELECT id FROM budgets WHERE organization_id=?1 AND scope_kind='release' AND release_id=?2",
                    params![org_id, release_id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(OfficeError::storage)?,
            _ => None,
        };
        match found {
            Some(id) => load_aggregate_from_db(&conn, org_id, &id),
            None => Ok(None),
        }
    }

    fn list_all(&self, org_id: &str) -> Result<Vec<BudgetAggregate>, OfficeError> {
        let mut conn = self.lock()?;
        self.ensure_reconciled(&mut conn, org_id)?;
        let mut stmt = conn
            .prepare(
                "SELECT id FROM budgets WHERE organization_id=?1 ORDER BY created_at ASC, id ASC",
            )
            .map_err(OfficeError::storage)?;
        let ids: Vec<String> = stmt
            .query_map([org_id], |r| r.get(0))
            .map_err(OfficeError::storage)?
            .collect::<Result<_, _>>()
            .map_err(OfficeError::storage)?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(agg) = load_aggregate_from_db(&conn, org_id, &id)? {
                out.push(agg);
            }
        }
        Ok(out)
    }
}

fn project_aggregate(
    tx: &rusqlite::Transaction<'_>,
    agg: &BudgetAggregate,
    employee_kinds: &[(String, &'static str)],
) -> Result<(), OfficeError> {
    upsert_mirrors(tx, agg, employee_kinds)?;
    upsert_versions(tx, agg)?;
    upsert_budget_row(tx, agg)?;
    upsert_periods(tx, agg)?;
    upsert_events(tx, agg)?;
    upsert_notices(tx, agg)?;
    upsert_holds(tx, agg)?;
    upsert_pending_stop(tx, agg)
}

fn upsert_mirrors(
    tx: &rusqlite::Transaction<'_>,
    agg: &BudgetAggregate,
    employee_kinds: &[(String, &'static str)],
) -> Result<(), OfficeError> {
    let org = &agg.budget.organization_id;
    tx.execute(
        "INSERT OR IGNORE INTO budget_organizations(id) VALUES (?1)",
        [org],
    )
    .map_err(OfficeError::storage)?;
    if let Some(tid) = agg.budget.task_id.as_deref() {
        tx.execute(
            "INSERT OR IGNORE INTO budget_tasks(organization_id, kind, id, is_deleted) VALUES (?1, 'task', ?2, 0)",
            params![org, tid],
        )
        .map_err(OfficeError::storage)?;
    }
    if let Some(rid) = agg.budget.release_id.as_deref() {
        tx.execute(
            "INSERT OR IGNORE INTO budget_releases(organization_id, id) VALUES (?1, ?2)",
            params![org, rid],
        )
        .map_err(OfficeError::storage)?;
    }
    for (eid, kind) in employee_kinds {
        tx.execute(
            "INSERT INTO budget_employees(organization_id, id, kind) VALUES (?1, ?2, ?3) \
             ON CONFLICT(organization_id, id) DO UPDATE SET kind = 'agent' WHERE excluded.kind = 'agent'",
            params![org, eid, kind],
        )
        .map_err(OfficeError::storage)?;
    }
    Ok(())
}

fn upsert_versions(
    tx: &rusqlite::Transaction<'_>,
    agg: &BudgetAggregate,
) -> Result<(), OfficeError> {
    for v in &agg.versions {
        tx.execute(
            "INSERT OR IGNORE INTO budget_policy_versions(\
             organization_id, budget_id, revision, state, unit, limit_amount, time_zone, \
             time_zone_source, time_zone_subject_ref, changed_by_id, changed_at, reason) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                &v.organization_id,
                &v.budget_id,
                v.revision as i64,
                v.state.as_str(),
                v.unit.as_str(),
                v.limit_amount.map(|n| n as i64),
                &v.time_zone,
                v.time_zone_source.as_str(),
                v.time_zone_subject_ref.as_deref(),
                &v.changed_by_id,
                &v.changed_at,
                v.reason.as_deref(),
            ],
        )
        .map_err(OfficeError::storage)?;
    }
    Ok(())
}

fn upsert_budget_row(
    tx: &rusqlite::Transaction<'_>,
    agg: &BudgetAggregate,
) -> Result<(), OfficeError> {
    let org = &agg.budget.organization_id;
    let exists: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM budgets WHERE organization_id=?1 AND id=?2)",
            params![org, &agg.budget.id],
            |r| r.get(0),
        )
        .map_err(OfficeError::storage)?;
    if exists {
        tx.execute(
            "UPDATE budgets SET current_revision=?1, time_zone=?2, time_zone_source=?3, time_zone_subject_ref=?4 \
             WHERE organization_id=?5 AND id=?6",
            params![
                agg.budget.current_revision as i64,
                &agg.budget.time_zone,
                agg.budget.time_zone_source.as_str(),
                agg.budget.time_zone_subject_ref.as_deref(),
                org,
                &agg.budget.id,
            ],
        )
        .map_err(OfficeError::storage)?;
    } else {
        tx.execute(
            "INSERT INTO budgets(\
             organization_id, id, schema_version, current_revision, scope_kind, task_id, task_kind, \
             agent_id, release_id, period_kind, time_zone, time_zone_source, time_zone_subject_ref, created_at) \
             VALUES (?1,?2,1,?3,?4,?5,'task',?6,?7,?8,?9,?10,?11,?12)",
            params![
                org,
                &agg.budget.id,
                agg.budget.current_revision as i64,
                agg.budget.scope_kind.as_str(),
                agg.budget.task_id.as_deref(),
                agg.budget.agent_id.as_deref(),
                agg.budget.release_id.as_deref(),
                agg.budget.period_kind.as_str(),
                &agg.budget.time_zone,
                agg.budget.time_zone_source.as_str(),
                agg.budget.time_zone_subject_ref.as_deref(),
                &agg.budget.created_at,
            ],
        )
        .map_err(OfficeError::storage)?;
    }
    Ok(())
}

fn upsert_periods(
    tx: &rusqlite::Transaction<'_>,
    agg: &BudgetAggregate,
) -> Result<(), OfficeError> {
    for p in &agg.periods {
        tx.execute(
            "INSERT INTO budget_periods(\
             organization_id, budget_id, id, policy_revision, period_key, starts_at, ends_at, \
             used_amount, usage_status, includes_estimates, includes_api_equivalent, \
             unpriced_records, incomplete_records, extension_used, kept_paused, state, \
             evaluated_at, ledger_checkpoint_ref) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18) \
             ON CONFLICT(organization_id, id) DO UPDATE SET \
             policy_revision=excluded.policy_revision, used_amount=excluded.used_amount, \
             usage_status=excluded.usage_status, includes_estimates=excluded.includes_estimates, \
             includes_api_equivalent=excluded.includes_api_equivalent, \
             unpriced_records=excluded.unpriced_records, incomplete_records=excluded.incomplete_records, \
             extension_used=excluded.extension_used, kept_paused=excluded.kept_paused, \
             state=excluded.state, evaluated_at=excluded.evaluated_at, \
             ledger_checkpoint_ref=excluded.ledger_checkpoint_ref",
            params![
                &p.organization_id,
                &p.budget_id,
                &p.id,
                p.policy_revision as i64,
                &p.period_key,
                p.starts_at.as_deref(),
                p.ends_at.as_deref(),
                p.used_amount as i64,
                p.usage_status.as_str(),
                i64::from(p.includes_estimates),
                i64::from(p.includes_api_equivalent),
                p.unpriced_records as i64,
                p.incomplete_records as i64,
                i64::from(p.extension_used),
                i64::from(p.kept_paused),
                &p.state,
                p.evaluated_at.as_deref(),
                p.ledger_checkpoint_ref.as_deref(),
            ],
        )
        .map_err(OfficeError::storage)?;
    }
    Ok(())
}

fn upsert_events(tx: &rusqlite::Transaction<'_>, agg: &BudgetAggregate) -> Result<(), OfficeError> {
    for e in &agg.events {
        tx.execute(
            "INSERT INTO budget_events(\
             organization_id, id, budget_id, period_id, operation_id, kind, policy_revision, \
             actor_kind, actor_id, occurred_at, payload_json) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11) \
             ON CONFLICT(organization_id, id) DO NOTHING",
            params![
                &e.organization_id,
                &e.id,
                &e.budget_id,
                &e.period_id,
                &e.operation_id,
                &e.kind,
                e.policy_revision as i64,
                &e.actor_kind,
                e.actor_id.as_deref(),
                &e.occurred_at,
                &e.payload_json,
            ],
        )
        .map_err(OfficeError::storage)?;
    }
    Ok(())
}

fn upsert_notices(
    tx: &rusqlite::Transaction<'_>,
    agg: &BudgetAggregate,
) -> Result<(), OfficeError> {
    for n in &agg.notices {
        tx.execute(
            "INSERT INTO budget_notices(\
             organization_id, id, budget_id, period_id, policy_revision, stage, threshold, \
             recipient_kind, recipient_id, kind, delivery_state, created_at, resolved_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13) \
             ON CONFLICT(organization_id, id) DO UPDATE SET \
             delivery_state=excluded.delivery_state, resolved_at=excluded.resolved_at",
            params![
                &n.organization_id,
                &n.id,
                &n.budget_id,
                &n.period_id,
                n.policy_revision as i64,
                n.stage.as_str(),
                i64::from(n.threshold),
                &n.recipient_kind,
                n.recipient_id.as_deref(),
                &n.kind,
                &n.delivery_state,
                &n.created_at,
                n.resolved_at.as_deref(),
            ],
        )
        .map_err(OfficeError::storage)?;
    }
    Ok(())
}

fn upsert_holds(tx: &rusqlite::Transaction<'_>, agg: &BudgetAggregate) -> Result<(), OfficeError> {
    for h in &agg.holds {
        tx.execute(
            "INSERT INTO budget_holds(\
             organization_id, budget_id, period_id, policy_revision, stage, state, \
             created_at, updated_at, release_reason) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9) \
             ON CONFLICT(organization_id, budget_id, period_id, policy_revision, stage) DO UPDATE SET \
             state=excluded.state, updated_at=excluded.updated_at, release_reason=excluded.release_reason",
            params![
                &h.organization_id,
                &h.budget_id,
                &h.period_id,
                h.policy_revision as i64,
                h.stage.as_str(),
                h.state.as_str(),
                &h.created_at,
                &h.updated_at,
                h.release_reason.as_deref(),
            ],
        )
        .map_err(OfficeError::storage)?;
    }
    Ok(())
}

fn upsert_pending_stop(
    tx: &rusqlite::Transaction<'_>,
    agg: &BudgetAggregate,
) -> Result<(), OfficeError> {
    if let Some(intent) = &agg.pending_stop {
        let json = serde_json::to_string(intent).map_err(OfficeError::storage)?;
        tx.execute(
            "INSERT INTO budget_stop_intents(organization_id, budget_id, intent_json) \
             VALUES (?1, ?2, ?3) \
             ON CONFLICT(organization_id, budget_id) DO UPDATE SET intent_json = excluded.intent_json",
            params![&agg.budget.organization_id, &agg.budget.id, &json],
        )
        .map_err(OfficeError::storage)?;
    } else {
        tx.execute(
            "DELETE FROM budget_stop_intents WHERE organization_id=?1 AND budget_id=?2",
            params![&agg.budget.organization_id, &agg.budget.id],
        )
        .map_err(OfficeError::storage)?;
    }
    Ok(())
}

fn load_pending_stop(
    conn: &Connection,
    org_id: &str,
    budget_id: &str,
) -> Result<Option<PendingStopIntent>, OfficeError> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT intent_json FROM budget_stop_intents WHERE organization_id=?1 AND budget_id=?2",
            params![org_id, budget_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(OfficeError::storage)?;
    match raw {
        Some(json) => serde_json::from_str(&json)
            .map(Some)
            .map_err(OfficeError::storage),
        None => Ok(None),
    }
}

fn load_aggregate_from_db(
    conn: &Connection,
    org_id: &str,
    budget_id: &str,
) -> Result<Option<BudgetAggregate>, OfficeError> {
    let budget: Option<BudgetRecord> = conn
        .query_row(
            "SELECT organization_id, id, current_revision, scope_kind, task_id, agent_id, \
             release_id, period_kind, time_zone, time_zone_source, time_zone_subject_ref, created_at \
             FROM budgets WHERE organization_id=?1 AND id=?2",
            params![org_id, budget_id],
            |r| {
                Ok(BudgetRecord {
                    organization_id: r.get(0)?,
                    id: r.get(1)?,
                    current_revision: r.get::<_, i64>(2)? as u64,
                    scope_kind: parse_scope(&r.get::<_, String>(3)?),
                    task_id: r.get(4)?,
                    agent_id: r.get(5)?,
                    release_id: r.get(6)?,
                    period_kind: parse_period(&r.get::<_, String>(7)?),
                    time_zone: r.get(8)?,
                    time_zone_source: parse_tz_source(&r.get::<_, String>(9)?),
                    time_zone_subject_ref: r.get(10)?,
                    created_at: r.get(11)?,
                })
            },
        )
        .optional()
        .map_err(OfficeError::storage)?;
    let Some(budget) = budget else {
        return Ok(None);
    };
    let versions = load_versions(conn, org_id, budget_id)?;
    let periods = load_periods(conn, org_id, budget_id)?;
    let events = load_events(conn, org_id, budget_id)?;
    let notices = load_notices(conn, org_id, budget_id)?;
    let holds = load_holds(conn, org_id, budget_id)?;
    let pending_stop = load_pending_stop(conn, org_id, budget_id)?;
    Ok(Some(BudgetAggregate {
        budget,
        versions,
        periods,
        events,
        notices,
        holds,
        pending_stop,
    }))
}

fn load_versions(
    conn: &Connection,
    org_id: &str,
    budget_id: &str,
) -> Result<Vec<BudgetPolicyVersion>, OfficeError> {
    let mut stmt = conn
        .prepare(
            "SELECT organization_id, budget_id, revision, state, unit, limit_amount, time_zone, \
             time_zone_source, time_zone_subject_ref, changed_by_id, changed_at, reason \
             FROM budget_policy_versions WHERE organization_id=?1 AND budget_id=?2 ORDER BY revision ASC",
        )
        .map_err(OfficeError::storage)?;
    let rows = stmt
        .query_map(params![org_id, budget_id], |r| {
            Ok(BudgetPolicyVersion {
                organization_id: r.get(0)?,
                budget_id: r.get(1)?,
                revision: r.get::<_, i64>(2)? as u64,
                state: parse_policy_state(&r.get::<_, String>(3)?),
                unit: parse_unit(&r.get::<_, String>(4)?),
                limit_amount: r.get::<_, Option<i64>>(5)?.map(|n| n as u64),
                time_zone: r.get(6)?,
                time_zone_source: parse_tz_source(&r.get::<_, String>(7)?),
                time_zone_subject_ref: r.get(8)?,
                changed_by_id: r.get(9)?,
                changed_at: r.get(10)?,
                reason: r.get(11)?,
            })
        })
        .map_err(OfficeError::storage)?;
    rows.collect::<Result<_, _>>().map_err(OfficeError::storage)
}

fn load_periods(
    conn: &Connection,
    org_id: &str,
    budget_id: &str,
) -> Result<Vec<BudgetPeriodRecord>, OfficeError> {
    let mut stmt = conn
        .prepare(
            "SELECT organization_id, budget_id, id, policy_revision, period_key, starts_at, ends_at, \
             used_amount, usage_status, includes_estimates, includes_api_equivalent, \
             unpriced_records, incomplete_records, extension_used, kept_paused, state, \
             evaluated_at, ledger_checkpoint_ref \
             FROM budget_periods WHERE organization_id=?1 AND budget_id=?2 ORDER BY period_key ASC",
        )
        .map_err(OfficeError::storage)?;
    let rows = stmt
        .query_map(params![org_id, budget_id], |r| {
            Ok(BudgetPeriodRecord {
                organization_id: r.get(0)?,
                budget_id: r.get(1)?,
                id: r.get(2)?,
                policy_revision: r.get::<_, i64>(3)? as u64,
                period_key: r.get(4)?,
                starts_at: r.get(5)?,
                ends_at: r.get(6)?,
                used_amount: r.get::<_, i64>(7)? as u64,
                usage_status: parse_completeness(&r.get::<_, String>(8)?),
                includes_estimates: r.get::<_, i64>(9)? != 0,
                includes_api_equivalent: r.get::<_, i64>(10)? != 0,
                unpriced_records: r.get::<_, i64>(11)? as u64,
                incomplete_records: r.get::<_, i64>(12)? as u64,
                extension_used: r.get::<_, i64>(13)? != 0,
                kept_paused: r.get::<_, i64>(14)? != 0,
                state: r.get(15)?,
                evaluated_at: r.get(16)?,
                ledger_checkpoint_ref: r.get(17)?,
            })
        })
        .map_err(OfficeError::storage)?;
    rows.collect::<Result<_, _>>().map_err(OfficeError::storage)
}

fn load_events(
    conn: &Connection,
    org_id: &str,
    budget_id: &str,
) -> Result<Vec<BudgetEventRecord>, OfficeError> {
    let mut stmt = conn
        .prepare(
            "SELECT organization_id, id, budget_id, period_id, operation_id, kind, policy_revision, \
             actor_kind, actor_id, occurred_at, payload_json \
             FROM budget_events WHERE organization_id=?1 AND budget_id=?2 ORDER BY occurred_at ASC, id ASC",
        )
        .map_err(OfficeError::storage)?;
    let rows = stmt
        .query_map(params![org_id, budget_id], |r| {
            Ok(BudgetEventRecord {
                organization_id: r.get(0)?,
                id: r.get(1)?,
                budget_id: r.get(2)?,
                period_id: r.get(3)?,
                operation_id: r.get(4)?,
                kind: r.get(5)?,
                policy_revision: r.get::<_, i64>(6)? as u64,
                actor_kind: r.get(7)?,
                actor_id: r.get(8)?,
                occurred_at: r.get(9)?,
                payload_json: r.get(10)?,
            })
        })
        .map_err(OfficeError::storage)?;
    rows.collect::<Result<_, _>>().map_err(OfficeError::storage)
}

fn load_notices(
    conn: &Connection,
    org_id: &str,
    budget_id: &str,
) -> Result<Vec<BudgetNoticeRecord>, OfficeError> {
    let mut stmt = conn
        .prepare(
            "SELECT organization_id, id, budget_id, period_id, policy_revision, stage, threshold, \
             recipient_kind, recipient_id, kind, delivery_state, created_at, resolved_at \
             FROM budget_notices WHERE organization_id=?1 AND budget_id=?2 ORDER BY created_at ASC, id ASC",
        )
        .map_err(OfficeError::storage)?;
    let rows = stmt
        .query_map(params![org_id, budget_id], |r| {
            Ok(BudgetNoticeRecord {
                organization_id: r.get(0)?,
                id: r.get(1)?,
                budget_id: r.get(2)?,
                period_id: r.get(3)?,
                policy_revision: r.get::<_, i64>(4)? as u64,
                stage: parse_stage(&r.get::<_, String>(5)?),
                threshold: r.get::<_, i64>(6)? as u8,
                recipient_kind: r.get(7)?,
                recipient_id: r.get(8)?,
                kind: r.get(9)?,
                delivery_state: r.get(10)?,
                created_at: r.get(11)?,
                resolved_at: r.get(12)?,
            })
        })
        .map_err(OfficeError::storage)?;
    rows.collect::<Result<_, _>>().map_err(OfficeError::storage)
}

fn load_holds(
    conn: &Connection,
    org_id: &str,
    budget_id: &str,
) -> Result<Vec<BudgetHoldRecord>, OfficeError> {
    let mut stmt = conn
        .prepare(
            "SELECT organization_id, budget_id, period_id, policy_revision, stage, state, \
             created_at, updated_at, release_reason \
             FROM budget_holds WHERE organization_id=?1 AND budget_id=?2 ORDER BY created_at ASC",
        )
        .map_err(OfficeError::storage)?;
    let rows = stmt
        .query_map(params![org_id, budget_id], |r| {
            Ok(BudgetHoldRecord {
                organization_id: r.get(0)?,
                budget_id: r.get(1)?,
                period_id: r.get(2)?,
                policy_revision: r.get::<_, i64>(3)? as u64,
                stage: parse_stage(&r.get::<_, String>(4)?),
                state: parse_hold_state(&r.get::<_, String>(5)?),
                created_at: r.get(6)?,
                updated_at: r.get(7)?,
                release_reason: r.get(8)?,
            })
        })
        .map_err(OfficeError::storage)?;
    rows.collect::<Result<_, _>>().map_err(OfficeError::storage)
}

fn reconcile_durable_conn(
    conn: &mut Connection,
    durable_root: &Path,
    org_id: &str,
) -> Result<(), OfficeError> {
    let aggregates = load_all_durable(durable_root, org_id)?;
    if aggregates.is_empty() {
        return Ok(());
    }
    let known_agents: std::collections::BTreeSet<String> = aggregates
        .iter()
        .filter_map(|a| a.budget.agent_id.clone())
        .collect();
    let tx = conn.transaction().map_err(OfficeError::storage)?;
    for agg in &aggregates {
        let inferred_emps = infer_employee_mirrors(agg, &known_agents);
        project_aggregate(&tx, agg, &inferred_emps)?;
    }
    tx.commit().map_err(OfficeError::storage)?;
    Ok(())
}

fn invalid_sqlite_bound(field: &str) -> ApiError {
    ApiError::new(
        org_proto::ErrorCode::InvalidRequest,
        format!("budget field `{field}` exceeds signed 64-bit SQLite integer range"),
    )
}

fn validate_versions_and_periods_bounds(agg: &BudgetAggregate) -> Option<ApiError> {
    for v in &agg.versions {
        if i64::try_from(v.revision).is_err() {
            return Some(invalid_sqlite_bound("versions.revision"));
        }
        if v.limit_amount
            .is_some_and(|lim| lim == 0 || i64::try_from(lim).is_err())
        {
            return Some(invalid_sqlite_bound("limit_amount"));
        }
    }
    let bad_period = agg.periods.iter().any(|p| {
        i64::try_from(p.policy_revision).is_err()
            || i64::try_from(p.used_amount).is_err()
            || i64::try_from(p.unpriced_records).is_err()
            || i64::try_from(p.incomplete_records).is_err()
    });
    bad_period.then(|| invalid_sqlite_bound("periods"))
}

fn validate_aggregate_sqlite_bounds(agg: &BudgetAggregate) -> Option<ApiError> {
    if i64::try_from(agg.budget.current_revision).is_err() {
        return Some(invalid_sqlite_bound("current_revision"));
    }
    if let Some(err) = validate_versions_and_periods_bounds(agg) {
        return Some(err);
    }
    if agg
        .events
        .iter()
        .any(|e| i64::try_from(e.policy_revision).is_err())
    {
        return Some(invalid_sqlite_bound("events.policy_revision"));
    }
    if agg
        .notices
        .iter()
        .any(|n| i64::try_from(n.policy_revision).is_err())
    {
        return Some(invalid_sqlite_bound("notices.policy_revision"));
    }
    if agg
        .holds
        .iter()
        .any(|h| i64::try_from(h.policy_revision).is_err())
    {
        return Some(invalid_sqlite_bound("holds.policy_revision"));
    }
    None
}

fn check_target_and_revision_under_lock(
    conn: &Connection,
    agg: &BudgetAggregate,
) -> Result<Option<ApiError>, OfficeError> {
    let budget = &agg.budget;
    let existing_target_id: Option<String> = match budget.scope_kind {
        BudgetScopeKind::Task => conn
            .query_row(
                "SELECT id FROM budgets WHERE organization_id=?1 AND scope_kind='task' AND task_id=?2",
                params![&budget.organization_id, &budget.task_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(OfficeError::storage)?,
        BudgetScopeKind::Agent => conn
            .query_row(
                "SELECT id FROM budgets WHERE organization_id=?1 AND scope_kind='agent' AND agent_id=?2 AND period_kind=?3",
                params![&budget.organization_id, &budget.agent_id, budget.period_kind.as_str()],
                |r| r.get(0),
            )
            .optional()
            .map_err(OfficeError::storage)?,
        BudgetScopeKind::Release => conn
            .query_row(
                "SELECT id FROM budgets WHERE organization_id=?1 AND scope_kind='release' AND release_id=?2",
                params![&budget.organization_id, &budget.release_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(OfficeError::storage)?,
        _ => None,
    };
    if let Some(other_id) = existing_target_id {
        if other_id != budget.id {
            return Ok(Some(ApiError::new(
                org_proto::ErrorCode::Conflict,
                "a budget already exists for this target",
            )));
        }
    }
    let Some(existing) = load_aggregate_from_db(conn, &budget.organization_id, &budget.id)? else {
        return Ok(None);
    };
    if budget.current_revision < existing.budget.current_revision {
        return Ok(Some(ApiError::new(
            org_proto::ErrorCode::Conflict,
            "stale budget policy revision",
        )));
    }
    // Every already-committed policy version, event receipt, and notice must remain preserved
    // in the new aggregate so a concurrent save at the same policy revision can never erase an
    // acknowledged operation (`TASK-97` / Pass 5 Finding #1).
    let versions_preserved = existing
        .versions
        .iter()
        .all(|old_v| agg.versions.iter().find(|v| v.revision == old_v.revision) == Some(old_v));
    let events_preserved = existing
        .events
        .iter()
        .all(|old_e| agg.events.iter().any(|e| e.id == old_e.id));
    let notices_preserved = existing
        .notices
        .iter()
        .all(|old_n| agg.notices.iter().any(|n| n.id == old_n.id));
    // A `Released` hold may legitimately re-trigger to `StopPending` if new spend crosses the
    // same policy revision again, so only block direct `Held -> StopPending` regression.
    let holds_valid = existing.holds.iter().all(|old_h| {
        agg.holds
            .iter()
            .find(|h| {
                h.period_id == old_h.period_id
                    && h.policy_revision == old_h.policy_revision
                    && h.stage == old_h.stage
            })
            .is_some_and(|new_h| {
                !(old_h.state == BudgetHoldState::Held
                    && new_h.state == BudgetHoldState::StopPending)
            })
    });
    if !versions_preserved || !events_preserved || !notices_preserved || !holds_valid {
        return Ok(Some(ApiError::new(
            org_proto::ErrorCode::Conflict,
            "stale budget aggregate: concurrent modification detected",
        )));
    }
    Ok(None)
}

fn load_all_durable(root: &Path, org_id: &str) -> Result<Vec<BudgetAggregate>, OfficeError> {
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(root).map_err(OfficeError::storage)? {
        let dir = entry.map_err(OfficeError::storage)?.path();
        let agg_path = dir.join("aggregate.v1.json");
        if !agg_path.is_file() {
            continue;
        }
        // Durably flush each recovered aggregate file and its ancestor chain before
        // projecting its receipts so an interrupted rename/dir-sync can never be
        // acknowledged without a completed flush (`TASK-97` / Pass 4 Finding #1).
        let f = fs::File::open(&agg_path).map_err(OfficeError::storage)?;
        full_sync(&f).map_err(OfficeError::storage)?;
        private_dir_synced(root, &dir).map_err(OfficeError::storage)?;
        let mut agg: BudgetAggregate =
            serde_json::from_slice(&fs::read(&agg_path).map_err(OfficeError::storage)?)
                .map_err(OfficeError::storage)?;
        if agg.budget.organization_id != org_id {
            continue;
        }
        agg.versions.sort_by_key(|v| v.revision);
        agg.periods.sort_by(|a, b| a.period_key.cmp(&b.period_key));
        agg.events.sort_by(|a, b| {
            a.occurred_at
                .cmp(&b.occurred_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        agg.notices.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        agg.holds.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        out.push(agg);
    }
    Ok(out)
}

fn infer_employee_mirrors(
    agg: &BudgetAggregate,
    known_agents: &std::collections::BTreeSet<String>,
) -> Vec<(String, &'static str)> {
    let mut ids = std::collections::BTreeSet::new();
    ids.extend(agg.budget.agent_id.iter().cloned());
    ids.extend(agg.budget.time_zone_subject_ref.iter().cloned());
    for v in &agg.versions {
        ids.insert(v.changed_by_id.clone());
        ids.extend(v.time_zone_subject_ref.iter().cloned());
    }
    ids.extend(agg.events.iter().filter_map(|e| e.actor_id.clone()));
    ids.extend(agg.notices.iter().filter_map(|n| n.recipient_id.clone()));
    ids.into_iter()
        .map(|id| {
            let kind = if known_agents.contains(&id) {
                "agent"
            } else {
                "person"
            };
            (id, kind)
        })
        .collect()
}

fn write_json_atomic<T: Serialize>(path: &Path, val: &T) -> Result<(), OfficeError> {
    let bytes = serde_json::to_vec_pretty(val).map_err(OfficeError::storage)?;
    write_private_atomic(path, &bytes).map_err(OfficeError::storage)
}

fn parse_scope(s: &str) -> BudgetScopeKind {
    match s {
        "task" => BudgetScopeKind::Task,
        "agent" => BudgetScopeKind::Agent,
        "release" => BudgetScopeKind::Release,
        _ => BudgetScopeKind::Unknown,
    }
}

fn parse_period(s: &str) -> BudgetPeriodKind {
    match s {
        "lifetime" => BudgetPeriodKind::Lifetime,
        "day" => BudgetPeriodKind::Day,
        "month" => BudgetPeriodKind::Month,
        _ => BudgetPeriodKind::Unknown,
    }
}

fn parse_tz_source(s: &str) -> BudgetTimeZoneSource {
    match s {
        "owner_person" => BudgetTimeZoneSource::OwnerPerson,
        "hq" => BudgetTimeZoneSource::Hq,
        _ => BudgetTimeZoneSource::Unknown,
    }
}

fn parse_policy_state(s: &str) -> BudgetPolicyState {
    match s {
        "active" => BudgetPolicyState::Active,
        "cleared" => BudgetPolicyState::Cleared,
        _ => BudgetPolicyState::Unknown,
    }
}

fn parse_unit(s: &str) -> BudgetUnit {
    match s {
        "micro_usd" => BudgetUnit::MicroUsd,
        "tokens" => BudgetUnit::Tokens,
        _ => BudgetUnit::Unknown,
    }
}

fn parse_completeness(s: &str) -> CompletenessStatus {
    match s {
        "complete" => CompletenessStatus::Complete,
        "partial" => CompletenessStatus::Partial,
        "unavailable" => CompletenessStatus::Unavailable,
        _ => CompletenessStatus::Unknown,
    }
}

fn parse_stage(s: &str) -> BudgetStage {
    match s {
        "base" => BudgetStage::Base,
        "extended" => BudgetStage::Extended,
        _ => BudgetStage::Unknown,
    }
}

fn parse_hold_state(s: &str) -> BudgetHoldState {
    match s {
        "stop_pending" => BudgetHoldState::StopPending,
        "held" => BudgetHoldState::Held,
        "released" => BudgetHoldState::Released,
        _ => BudgetHoldState::Unknown,
    }
}
