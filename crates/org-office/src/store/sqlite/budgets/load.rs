//! Loads a [`BudgetAggregate`] back from `budgets.db` rows and parses stored enum text.

use org_proto::budgets::{
    BudgetHoldState, BudgetPeriodKind, BudgetPolicyState, BudgetScopeKind, BudgetStage,
    BudgetTimeZoneSource, BudgetUnit,
};
use org_proto::costs::CompletenessStatus;
use rusqlite::{params, Connection, OptionalExtension};

use crate::budgets::{
    BudgetAggregate, BudgetEventRecord, BudgetHoldRecord, BudgetNoticeRecord, BudgetPeriodRecord,
    BudgetPolicyVersion, BudgetRecord, PendingStopIntent,
};
use crate::error::OfficeError;

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

pub(super) fn load_aggregate_from_db(
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
