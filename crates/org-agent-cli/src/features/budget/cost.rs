//! Cost and subscription usage read commands: `cost query`, `cost prices`, and
//! `cost subscription-usage`.

use org_command::{Answer, Refusal, Request};
use org_proto::costs::{
    CostBreakdownGroup, CostPeriodPreset, CostPriceList, CostPriceListParams, CostPriceListResult,
    CostQuery, CostQueryParams, CostQueryResult, CostSubscriptionUsageList,
    CostSubscriptionUsageListParams, CostSubscriptionUsageListResult, UsageScopeKind,
};
use org_proto::ErrorCode;
use serde_json::Value;

use crate::runtime::domain::CommandCue;
use crate::runtime::input::{call_rpc, encode, is_set, opt_str, opt_string, opt_u64};
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport};

pub(super) const CUES: &[CommandCue] = &[
    (
        "cost",
        "query",
        "querying organization, project, team, agent, or task token usage and micro-USD cost totals with optional breakdowns.",
    ),
    (
        "cost",
        "prices",
        "listing versioned model pricing rates used to value token usage.",
    ),
    (
        "cost",
        "subscription-usage",
        "checking provider subscription window utilization and per-model usage snapshots.",
    ),
];

fn parse_usage_scope(raw: Option<&str>) -> Result<Option<UsageScopeKind>, Refusal> {
    match raw {
        None => Ok(None),
        Some("task") => Ok(Some(UsageScopeKind::Task)),
        Some("project") => Ok(Some(UsageScopeKind::Project)),
        Some("team") => Ok(Some(UsageScopeKind::Team)),
        Some("organization") => Ok(Some(UsageScopeKind::Organization)),
        Some(other) => Err(Refusal::new(
            ErrorCode::InvalidRequest,
            format!("unknown usage scope `{other}`"),
            "Pass --scope-kind task, project, team, or organization.",
        )),
    }
}

fn parse_period_preset(raw: Option<&str>) -> Result<Option<CostPeriodPreset>, Refusal> {
    match raw {
        None => Ok(None),
        Some("today") => Ok(Some(CostPeriodPreset::Today)),
        Some("this_week") => Ok(Some(CostPeriodPreset::ThisWeek)),
        Some("this_month") => Ok(Some(CostPeriodPreset::ThisMonth)),
        Some("last_month") => Ok(Some(CostPeriodPreset::LastMonth)),
        Some("rolling_7d") => Ok(Some(CostPeriodPreset::Rolling7d)),
        Some("rolling_30d") => Ok(Some(CostPeriodPreset::Rolling30d)),
        Some("rolling_90d") => Ok(Some(CostPeriodPreset::Rolling90d)),
        Some("all_time") => Ok(Some(CostPeriodPreset::AllTime)),
        Some("custom") => Ok(Some(CostPeriodPreset::Custom)),
        Some("release") => Ok(Some(CostPeriodPreset::Release)),
        Some(other) => Err(Refusal::new(
            ErrorCode::InvalidRequest,
            format!("unknown cost period `{other}`"),
            "Run `agent-os cost query --help` for valid --period presets.",
        )),
    }
}

fn parse_group_by(raw: Option<&str>) -> Result<Option<CostBreakdownGroup>, Refusal> {
    match raw {
        None => Ok(None),
        Some("project") => Ok(Some(CostBreakdownGroup::Project)),
        Some("team") => Ok(Some(CostBreakdownGroup::Team)),
        Some("consumer") => Ok(Some(CostBreakdownGroup::Consumer)),
        Some("model") => Ok(Some(CostBreakdownGroup::Model)),
        Some("task_type") => Ok(Some(CostBreakdownGroup::TaskType)),
        Some("work_type") => Ok(Some(CostBreakdownGroup::WorkType)),
        Some("period") => Ok(Some(CostBreakdownGroup::Period)),
        Some("task") => Ok(Some(CostBreakdownGroup::Task)),
        Some(other) => Err(Refusal::new(
            ErrorCode::InvalidRequest,
            format!("unknown group-by dimension `{other}`"),
            "Run `agent-os cost query --help` for valid --group-by values.",
        )),
    }
}

fn opt_u32(request: &Request, field: &str) -> Result<Option<u32>, Refusal> {
    let Some(val) = opt_u64(request, field)? else {
        return Ok(None);
    };
    u32::try_from(val).map(Some).map_err(|_| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            format!("`--{}` is out of range.", field.replace('_', "-")),
            "Pass a smaller non-negative integer.",
        )
    })
}

fn strip_null_fields(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|_, v| !v.is_null());
            for v in map.values_mut() {
                strip_null_fields(v);
            }
        }
        Value::Array(items) => {
            for item in items {
                strip_null_fields(item);
            }
        }
        _ => {}
    }
}

pub(super) fn read_cost_query<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = CostQueryParams {
        scope_kind: parse_usage_scope(opt_str(request, "scope_kind"))?,
        task_id: opt_string(request, "task"),
        project_id: opt_string(request, "project"),
        team_id: opt_string(request, "team"),
        agent_id: opt_string(request, "agent"),
        release_id: opt_string(request, "release"),
        milestone_id: opt_string(request, "milestone"),
        period: parse_period_preset(opt_str(request, "period"))?,
        from_date: opt_string(request, "from_date"),
        to_date: opt_string(request, "to_date"),
        time_zone: opt_string(request, "time_zone"),
        group_by: parse_group_by(opt_str(request, "group_by"))?,
        include_records: is_set(request, "include_records"),
        limit: opt_u32(request, "record_limit")?,
    };
    let res: CostQueryResult = call_rpc::<CostQuery, _>(transport, &params)?;
    let mut data = encode(&res, "cost.query")?;
    strip_null_fields(&mut data);
    Ok(Answer::completed(data))
}

pub(super) fn read_cost_prices<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    parse_cursor(request.cursor.as_deref())?;
    let params = CostPriceListParams {
        provider: opt_string(request, "provider"),
        model: opt_string(request, "model"),
    };
    let res: CostPriceListResult = call_rpc::<CostPriceList, _>(transport, &params)?;
    let mut raw = encode(&res.prices, "cost.price.list")?;
    strip_null_fields(&mut raw);
    let items = match raw {
        Value::Array(items) => items,
        _ => Vec::new(),
    };
    paginate(request, items)
}

pub(super) fn read_subscription_usage<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = CostSubscriptionUsageListParams {
        connection_id: opt_string(request, "connection_id"),
        provider_type: opt_string(request, "provider_type"),
        include_archived: is_set(request, "include_archived"),
        limit: opt_u32(request, "snapshot_limit")?,
        cursor: opt_string(request, "snapshot_cursor"),
        totals_cursor: opt_string(request, "totals_cursor"),
    };
    let res: CostSubscriptionUsageListResult =
        call_rpc::<CostSubscriptionUsageList, _>(transport, &params)?;
    let mut data = encode(&res, "cost.subscription_usage.list")?;
    strip_null_fields(&mut data);
    Ok(Answer::completed(data))
}
