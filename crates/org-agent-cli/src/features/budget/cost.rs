//! Cost and subscription usage read commands: `cost query`, `cost prices`, and
//! `cost subscription-usage`.

use org_command::{Answer, Command, Example, Field, Kind, Refusal, Request, Type};
use org_proto::costs::{
    CostBreakdownGroup, CostPeriodPreset, CostPriceList, CostPriceListParams, CostPriceListResult,
    CostQuery, CostQueryParams, CostQueryResult, CostSubscriptionUsageList,
    CostSubscriptionUsageListParams, CostSubscriptionUsageListResult, UsageScopeKind,
};
use org_proto::ErrorCode;
use serde_json::Value;

use super::BUDGET_ERRORS;
use crate::runtime::domain::CommandCue;
use crate::runtime::input::{call_rpc, encode, is_set, opt_str, opt_string, opt_u64};
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport};

const USAGE_SCOPE_ENUM: &[&str] = &["task", "project", "team", "organization"];
const PERIOD_PRESET_ENUM: &[&str] = &[
    "today",
    "this_week",
    "this_month",
    "last_month",
    "rolling_7d",
    "rolling_30d",
    "rolling_90d",
    "all_time",
    "custom",
    "release",
];
const GROUP_BY_ENUM: &[&str] = &[
    "project",
    "team",
    "consumer",
    "model",
    "task_type",
    "work_type",
    "period",
    "task",
];

const JSON_TYPE: Type = Type::Json;

const COST_SUMMARY_FIELDS: &[Field] = &[
    Field::optional(
        "known_cost_micro_usd",
        Type::Integer,
        "Known cost in micro-USD",
    ),
    Field::optional("known_tokens", Type::Integer, "Known total tokens"),
    Field::optional("input_tokens", Type::Integer, "Known input tokens"),
    Field::optional("output_tokens", Type::Integer, "Known output tokens"),
    Field::optional(
        "cache_read_tokens",
        Type::Integer,
        "Known cache-read tokens",
    ),
    Field::optional(
        "cache_write_tokens",
        Type::Integer,
        "Known cache-write tokens",
    ),
    Field::optional("reasoning_tokens", Type::Integer, "Known reasoning tokens"),
    Field::required(
        "record_count",
        Type::Integer,
        "Effective usage record count",
    ),
    Field::required("unpriced_records", Type::Integer, "Unpriced record count"),
    Field::required(
        "incomplete_records",
        Type::Integer,
        "Incomplete record count",
    ),
    Field::required(
        "includes_estimates",
        Type::Boolean,
        "Whether estimates are included",
    ),
    Field::required(
        "includes_api_equivalent",
        Type::Boolean,
        "Whether API-equivalent estimates are included",
    ),
    Field::required(
        "incomplete_streams",
        Type::Integer,
        "Count of incomplete streams",
    ),
    Field::optional(
        "coverage_as_of",
        Type::String,
        "RFC 3339 timestamp of coverage",
    ),
];

const COST_BREAKDOWN_FIELDS: &[Field] = &[
    Field::required("key", Type::String, "Breakdown bucket key"),
    Field::required("label", Type::String, "Breakdown bucket label"),
    Field::required(
        "summary",
        Type::Object(COST_SUMMARY_FIELDS),
        "Aggregated cost summary for this bucket",
    ),
];
const COST_BREAKDOWN_TYPE: Type = Type::Object(COST_BREAKDOWN_FIELDS);

const COST_QUERY_INPUT: &[Field] = &[
    Field::optional(
        "scope_kind",
        Type::Enum(USAGE_SCOPE_ENUM),
        "Scope kind: task, project, team, or organization",
    ),
    Field::optional("task", Type::String, "Filter by task ID"),
    Field::optional("project", Type::String, "Filter by project ID"),
    Field::optional("team", Type::String, "Filter by team ID"),
    Field::optional("agent", Type::String, "Filter by agent employee ID"),
    Field::optional("release", Type::String, "Filter by release ID"),
    Field::optional("milestone", Type::String, "Filter by milestone ID"),
    Field::optional(
        "period",
        Type::Enum(PERIOD_PRESET_ENUM),
        "Period preset (default rolling or custom with --from-date/--to-date)",
    ),
    Field::optional("from_date", Type::String, "Custom period start date"),
    Field::optional("to_date", Type::String, "Custom period end date"),
    Field::optional("time_zone", Type::String, "IANA time zone"),
    Field::optional(
        "group_by",
        Type::Enum(GROUP_BY_ENUM),
        "Breakdown grouping dimension",
    ),
    Field::optional(
        "include_records",
        Type::Boolean,
        "Include individual usage records",
    ),
    Field::optional(
        "record_limit",
        Type::Integer,
        "Maximum usage records to return when --include-records is set",
    ),
];

const COST_QUERY_OUTPUT: &[Field] = &[
    Field::required("time_zone", Type::String, "Applied IANA time zone"),
    Field::optional("starts_at", Type::String, "Period start timestamp"),
    Field::optional("ends_at", Type::String, "Period end timestamp"),
    Field::required(
        "scoped_to_projects",
        Type::Boolean,
        "Whether the query was narrowed to authorized projects",
    ),
    Field::required(
        "summary",
        Type::Object(COST_SUMMARY_FIELDS),
        "Overall effective usage and cost summary",
    ),
    Field::required(
        "breakdown",
        Type::List(&COST_BREAKDOWN_TYPE),
        "Grouped cost breakdown rows",
    ),
    Field::optional(
        "records",
        Type::List(&JSON_TYPE),
        "Effective usage records when --include-records was requested",
    ),
];

const COST_PRICES_INPUT: &[Field] = &[
    Field::optional("provider", Type::String, "Filter by provider name"),
    Field::optional("model", Type::String, "Filter by model identifier"),
];

const MODEL_PRICE_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Model price version UUIDv7"),
    Field::required("provider", Type::String, "Provider name"),
    Field::required("model", Type::String, "Model identifier"),
    Field::required(
        "effective_from",
        Type::String,
        "Effective RFC 3339 timestamp",
    ),
    Field::required("currency", Type::String, "Currency code (USD)"),
    Field::optional(
        "input_micro_usd_per_million",
        Type::Integer,
        "Input token rate in micro-USD per 1M tokens",
    ),
    Field::optional(
        "output_micro_usd_per_million",
        Type::Integer,
        "Output token rate in micro-USD per 1M tokens",
    ),
    Field::optional(
        "cache_read_micro_usd_per_million",
        Type::Integer,
        "Cache-read rate in micro-USD per 1M tokens",
    ),
    Field::optional(
        "cache_write_micro_usd_per_million",
        Type::Integer,
        "Cache-write rate in micro-USD per 1M tokens",
    ),
    Field::required("created_at", Type::String, "Creation timestamp"),
    Field::required("created_by_kind", Type::String, "Creator kind"),
    Field::optional("created_by_id", Type::String, "Creator member ID"),
];

const SUBSCRIPTION_USAGE_INPUT: &[Field] = &[
    Field::optional(
        "connection_id",
        Type::String,
        "Filter by provider connection ID",
    ),
    Field::optional("provider_type", Type::String, "Filter by provider type"),
    Field::optional(
        "include_archived",
        Type::Boolean,
        "Include archived provider connections",
    ),
    Field::optional(
        "snapshot_limit",
        Type::Integer,
        "Maximum snapshots per page",
    ),
    Field::optional(
        "snapshot_cursor",
        Type::String,
        "Pagination cursor for snapshots",
    ),
    Field::optional(
        "totals_cursor",
        Type::String,
        "Pagination cursor for per-model totals",
    ),
];

const SUBSCRIPTION_USAGE_OUTPUT: &[Field] = &[
    Field::required(
        "snapshots",
        Type::List(&JSON_TYPE),
        "Per-connection subscription usage snapshots",
    ),
    Field::required(
        "totals_by_model",
        Type::List(&JSON_TYPE),
        "Aggregated subscription usage totals by model",
    ),
    Field::optional(
        "collected_as_of",
        Type::String,
        "Timestamp of freshest collected snapshot",
    ),
    Field::optional(
        "next_cursor",
        Type::String,
        "Opaque cursor for next snapshot page",
    ),
    Field::optional(
        "next_totals_cursor",
        Type::String,
        "Opaque cursor for next totals page",
    ),
    Field::required(
        "partial_totals",
        Type::Boolean,
        "Whether any snapshot was excluded or partial",
    ),
    Field::required(
        "excluded_snapshot_count",
        Type::Integer,
        "Excluded snapshot count",
    ),
    Field::required(
        "is_api_equivalent_estimate",
        Type::Boolean,
        "Always false for direct provider subscription usage",
    ),
];

pub(super) const COMMANDS: &[Command] = &[
    Command {
        resource: "cost",
        verb: "query",
        summary: "Query scoped token usage and cost totals",
        call: "cost.query",
        kind: Kind::Read,
        input: COST_QUERY_INPUT,
        content_field: None,
        output: COST_QUERY_OUTPUT,
        errors: BUDGET_ERRORS,
        example: Example {
            input: r#"{"period":"this_month","group_by":"project"}"#,
            output: r#"{"time_zone":"UTC","scoped_to_projects":false,"summary":{"record_count":0,"unpriced_records":0,"incomplete_records":0,"includes_estimates":false,"includes_api_equivalent":false,"incomplete_streams":0},"breakdown":[]}"#,
        },
    },
    Command {
        resource: "cost",
        verb: "prices",
        summary: "List versioned USD model prices",
        call: "cost.price.list",
        kind: Kind::List,
        input: COST_PRICES_INPUT,
        content_field: None,
        output: MODEL_PRICE_FIELDS,
        errors: BUDGET_ERRORS,
        example: Example {
            input: "{}",
            output: r#"[{"id":"0193a5c0-0000-7000-8000-000000000711","provider":"example","model":"model-a","effective_from":"2026-11-01T00:00:00Z","currency":"USD","created_at":"2026-11-01T00:00:00Z","created_by_kind":"person"}]"#,
        },
    },
    Command {
        resource: "cost",
        verb: "subscription-usage",
        summary: "Read provider subscription usage snapshots",
        call: "cost.subscription_usage.list",
        kind: Kind::Read,
        input: SUBSCRIPTION_USAGE_INPUT,
        content_field: None,
        output: SUBSCRIPTION_USAGE_OUTPUT,
        errors: BUDGET_ERRORS,
        example: Example {
            input: "{}",
            output: r#"{"snapshots":[],"totals_by_model":[],"partial_totals":false,"excluded_snapshot_count":0,"is_api_equivalent_estimate":false}"#,
        },
    },
];

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
