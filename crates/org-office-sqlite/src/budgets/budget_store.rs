//! [`BudgetStore`] implementation over the isolated `budgets.db` projection.

use std::sync::atomic::Ordering;

use org_proto::budgets::{BudgetPeriodKind, BudgetScopeKind};
use org_proto::ApiError;
use rusqlite::{params, OptionalExtension};

use org_office::budgets::{BudgetAggregate, BudgetStore};
use org_office::OfficeError;

use super::guards::{check_target_and_revision_under_lock, validate_aggregate_sqlite_bounds};
use super::load::load_aggregate_from_db;
use super::project::project_aggregate;
use super::SqliteBudgetStore;

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
