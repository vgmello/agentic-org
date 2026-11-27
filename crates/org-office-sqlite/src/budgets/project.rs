//! Projects a [`BudgetAggregate`] into `budgets.db` rows.

use rusqlite::params;

use org_office::budgets::BudgetAggregate;
use org_office::OfficeError;

pub(super) fn project_aggregate(
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
