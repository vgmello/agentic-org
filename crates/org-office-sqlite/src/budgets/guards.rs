//! Pre-write guards: SQLite integer bound validation and target/revision checks under the
//! connection lock.

use org_proto::budgets::{BudgetHoldState, BudgetScopeKind};
use org_proto::ApiError;
use rusqlite::{params, Connection, OptionalExtension};

use org_office::budgets::BudgetAggregate;
use org_office::OfficeError;

use super::load::load_aggregate_from_db;

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

pub(super) fn validate_aggregate_sqlite_bounds(agg: &BudgetAggregate) -> Option<ApiError> {
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

pub(super) fn check_target_and_revision_under_lock(
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
