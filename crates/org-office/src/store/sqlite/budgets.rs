//! Isolated SQLite + durable-file adapter for `L3` budgets (`TASK-53`, `doc-92`, `ADR-0016`).
//!
//! Loads `docs/design/organization-data/budgets-schema.sql` VERBATIM into
//! `<runtime>/<office-id>/protected/budgets.db` (owner-only 0700/0600, never `office.db`).
//! Durable source files live under `<durable>/budgets/<budget-id>/` (never in shared org Git),
//! enabling full delete-and-rebuild of `protected/budgets.db`.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
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
    BudgetPolicyVersion, BudgetRecord, BudgetStore,
};
use crate::error::OfficeError;
use crate::private_fs::{absolute, private_dir, private_file, tighten_dir};

/// Verbatim DDL from `docs/design/organization-data/budgets-schema.sql` (`doc-92`, `TASK-10`).
const BUDGETS_DDL: &str =
    include_str!("../../../../../docs/design/organization-data/budgets-schema.sql");

/// Isolated durable + SQLite [`BudgetStore`].
pub struct SqliteBudgetStore {
    durable_root: PathBuf,
    conn: Mutex<Connection>,
}

impl SqliteBudgetStore {
    /// Opens `<runtime_root>/<office_id>/protected/budgets.db`, rebuilding automatically from
    /// `<durable_root>/budgets/` if the projection is missing or empty.
    pub fn open(
        durable_root: &Path,
        runtime_root: &Path,
        office_id: &str,
        org_id: &str,
    ) -> Result<Self, OfficeError> {
        let durable_budgets = durable_root.join("budgets");
        private_dir(&durable_budgets).map_err(OfficeError::storage)?;
        let office_dir = runtime_root.join(office_id);
        private_dir(&office_dir).map_err(OfficeError::storage)?;
        let protected = office_dir.join("protected");
        private_dir(&protected).map_err(OfficeError::storage)?;
        tighten_dir(&protected).map_err(OfficeError::storage)?;

        let db_path = absolute(protected.join("budgets.db")).map_err(OfficeError::storage)?;
        drop(private_file(&db_path).map_err(OfficeError::storage)?);
        let conn = Connection::open(&db_path).map_err(OfficeError::storage)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;
             PRAGMA foreign_keys = ON;
             PRAGMA recursive_triggers = ON;
             PRAGMA busy_timeout = 5000;",
        )
        .map_err(OfficeError::storage)?;
        ensure_schema_initialized(&conn)?;
        tighten_db_and_sidecars(&db_path)?;

        let store = Self {
            durable_root: durable_budgets,
            conn: Mutex::new(conn),
        };
        store.reconcile_durable(org_id)?;
        Ok(store)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, OfficeError> {
        self.conn
            .lock()
            .map_err(|_| OfficeError::Invalid("budgets sqlite connection poisoned"))
    }

    /// Reconciles durable budget aggregates on startup (both full rebuild and crash recovery),
    /// pre-scanning all agent IDs so agent PMs are always mirrored as `kind='agent'` (`Pass 1 High #8`).
    fn reconcile_durable(&self, org_id: &str) -> Result<(), OfficeError> {
        let mut conn = self.lock()?;
        let aggregates = load_all_durable(&self.durable_root, org_id)?;
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

    fn write_durable(&self, agg: &BudgetAggregate) -> Result<(), OfficeError> {
        let dir = self.durable_root.join(&agg.budget.id);
        private_dir(&dir).map_err(OfficeError::storage)?;
        write_json_atomic(&dir.join("budget.json"), &agg.budget)?;
        let versions_dir = dir.join("versions");
        private_dir(&versions_dir).map_err(OfficeError::storage)?;
        for v in &agg.versions {
            let p = versions_dir.join(format!("{}.json", v.revision));
            if !p.exists() {
                write_json_atomic(&p, v)?;
            }
        }
        let periods_dir = dir.join("periods");
        private_dir(&periods_dir).map_err(OfficeError::storage)?;
        for p in &agg.periods {
            write_json_atomic(&periods_dir.join(format!("{}.json", p.id)), p)?;
        }
        let events_dir = dir.join("events");
        private_dir(&events_dir).map_err(OfficeError::storage)?;
        for e in &agg.events {
            let p = events_dir.join(format!("{}.json", e.id));
            if !p.exists() {
                write_json_atomic(&p, e)?;
            }
        }
        let notices_dir = dir.join("notices");
        private_dir(&notices_dir).map_err(OfficeError::storage)?;
        for n in &agg.notices {
            write_json_atomic(&notices_dir.join(format!("{}.json", n.id)), n)?;
        }
        let holds_dir = dir.join("holds");
        private_dir(&holds_dir).map_err(OfficeError::storage)?;
        for h in &agg.holds {
            let name = format!(
                "{}-{}-{}.json",
                h.period_id,
                h.policy_revision,
                h.stage.as_str()
            );
            write_json_atomic(&holds_dir.join(name), h)?;
        }
        Ok(())
    }
}

impl BudgetStore for SqliteBudgetStore {
    fn save_aggregate(
        &self,
        agg: &BudgetAggregate,
        employee_kinds: &[(String, &'static str)],
    ) -> Result<Result<(), ApiError>, OfficeError> {
        let mut conn = self.lock()?;
        self.write_durable(agg)?;
        let tx = conn.transaction().map_err(OfficeError::storage)?;
        project_aggregate(&tx, agg, employee_kinds)?;
        tx.commit().map_err(OfficeError::storage)?;
        Ok(Ok(()))
    }

    fn get_by_id(
        &self,
        org_id: &str,
        budget_id: &str,
    ) -> Result<Option<BudgetAggregate>, OfficeError> {
        let conn = self.lock()?;
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
        let conn = self.lock()?;
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
        let conn = self.lock()?;
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

fn ensure_schema_initialized(conn: &Connection) -> Result<(), OfficeError> {
    let tables: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='budgets'",
            [],
            |r| r.get(0),
        )
        .map_err(OfficeError::storage)?;
    if tables > 0 {
        return Ok(());
    }
    conn.execute_batch("BEGIN IMMEDIATE;")
        .map_err(OfficeError::storage)?;
    if let Err(err) = conn.execute_batch(BUDGETS_DDL) {
        let _ = conn.execute_batch("ROLLBACK;");
        return Err(OfficeError::storage(err));
    }
    conn.execute_batch("COMMIT;")
        .map_err(OfficeError::storage)?;
    Ok(())
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
    upsert_holds(tx, agg)
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
    Ok(Some(BudgetAggregate {
        budget,
        versions,
        periods,
        events,
        notices,
        holds,
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

fn load_all_durable(root: &Path, org_id: &str) -> Result<Vec<BudgetAggregate>, OfficeError> {
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(root).map_err(OfficeError::storage)? {
        let dir = entry.map_err(OfficeError::storage)?.path();
        let budget_path = dir.join("budget.json");
        if !budget_path.is_file() {
            continue;
        }
        let budget: BudgetRecord =
            serde_json::from_slice(&fs::read(&budget_path).map_err(OfficeError::storage)?)
                .map_err(OfficeError::storage)?;
        if budget.organization_id != org_id {
            continue;
        }
        let mut versions: Vec<BudgetPolicyVersion> = read_json_dir(&dir.join("versions"))?;
        versions.sort_by_key(|v| v.revision);
        let mut periods: Vec<BudgetPeriodRecord> = read_json_dir(&dir.join("periods"))?;
        periods.sort_by(|a, b| a.period_key.cmp(&b.period_key));
        let mut events: Vec<BudgetEventRecord> = read_json_dir(&dir.join("events"))?;
        events.sort_by(|a, b| {
            a.occurred_at
                .cmp(&b.occurred_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        let mut notices: Vec<BudgetNoticeRecord> = read_json_dir(&dir.join("notices"))?;
        notices.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        let mut holds: Vec<BudgetHoldRecord> = read_json_dir(&dir.join("holds"))?;
        holds.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        out.push(BudgetAggregate {
            budget,
            versions,
            periods,
            events,
            notices,
            holds,
        });
    }
    Ok(out)
}

fn read_json_dir<T: serde::de::DeserializeOwned>(dir: &Path) -> Result<Vec<T>, OfficeError> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).map_err(OfficeError::storage)? {
        let path = entry.map_err(OfficeError::storage)?.path();
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let bytes = fs::read(&path).map_err(OfficeError::storage)?;
            out.push(serde_json::from_slice(&bytes).map_err(OfficeError::storage)?);
        }
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

static BUDGET_TMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn write_json_atomic<T: Serialize>(path: &Path, val: &T) -> Result<(), OfficeError> {
    let bytes = serde_json::to_vec_pretty(val).map_err(OfficeError::storage)?;
    let seq = BUDGET_TMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("tmp.{}.{seq}", std::process::id()));
    {
        let mut opts = OpenOptions::new();
        opts.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp).map_err(OfficeError::storage)?;
        f.write_all(&bytes).map_err(OfficeError::storage)?;
        f.sync_all().map_err(OfficeError::storage)?;
    }
    tighten_file(&tmp)?;
    fs::rename(&tmp, path).map_err(OfficeError::storage)?;
    if let Some(parent) = path.parent() {
        if let Ok(dir) = fs::File::open(parent) {
            let _ = dir.sync_all();
        }
    }
    tighten_file(path)
}

#[cfg(unix)]
fn tighten_file(path: &Path) -> Result<(), OfficeError> {
    use std::os::unix::fs::PermissionsExt;
    if path.exists() {
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(OfficeError::storage)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn tighten_file(_path: &Path) -> Result<(), OfficeError> {
    Err(OfficeError::Invalid(
        "protected budgets owner-only file permissions are unsupported on this OS",
    ))
}

fn tighten_db_and_sidecars(db_path: &Path) -> Result<(), OfficeError> {
    tighten_file(db_path)?;
    for suffix in ["-wal", "-shm"] {
        let mut s = db_path.as_os_str().to_owned();
        s.push(suffix);
        tighten_file(Path::new(&s))?;
    }
    Ok(())
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
