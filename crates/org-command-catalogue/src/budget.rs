//! Declarative command rows for `budget status|list|inspect` and `cost query|prices|subscription-usage`.

use org_command::{Command, Example, Field, Kind, Type};
use org_proto::ErrorCode;

use crate::shared::JSON_TYPE;

const BUDGET_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotBound,
    ErrorCode::NotFound,
    ErrorCode::NotAuthorized,
    ErrorCode::Unavailable,
];

const SCOPE_ENUM: &[&str] = &["task", "agent", "release"];
const PERIOD_ENUM: &[&str] = &["lifetime", "day", "month"];

const BUDGET_VIEW_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Budget ID"),
    Field::required("scope_kind", Type::String, "Scope kind"),
    Field::optional("task_id", Type::String, "Task ID for a task budget"),
    Field::optional("agent_id", Type::String, "Agent ID for an agent budget"),
    Field::optional(
        "release_id",
        Type::String,
        "Release ID for a release budget",
    ),
    Field::required("period_kind", Type::String, "Period kind"),
    Field::optional("current_revision", Type::Integer, "Policy revision"),
    Field::required("state", Type::String, "Policy state"),
    Field::optional("time_zone", Type::String, "IANA time zone"),
    Field::required("state_summary", Type::String, "Budget status summary"),
    Field::optional("hold_state", Type::String, "Execution hold state"),
    Field::required(
        "amounts_visible",
        Type::Boolean,
        "Whether amounts are visible",
    ),
    Field::optional("unit", Type::String, "micro_usd or tokens"),
    Field::optional("limit_amount", Type::Integer, "Base limit amount"),
    Field::optional("effective_limit", Type::Integer, "Effective limit"),
    Field::optional("used_amount", Type::Integer, "Known used amount"),
    Field::optional("remaining_amount", Type::Integer, "Remaining amount"),
];

const BUDGET_STATUS_INPUT: &[Field] = &[Field::optional(
    "task",
    Type::String,
    "Current task ID or T-<CODE><N> ref",
)];

const BUDGET_STATUS_OUTPUT: &[Field] = &[
    Field::required("agent_id", Type::String, "Bound agent ID"),
    Field::optional(
        "daily",
        Type::Object(BUDGET_VIEW_FIELDS),
        "Own daily budget",
    ),
    Field::optional(
        "monthly",
        Type::Object(BUDGET_VIEW_FIELDS),
        "Own monthly budget",
    ),
    Field::optional("task", Type::Object(BUDGET_VIEW_FIELDS), "Task budget"),
];

const BUDGET_LIST_INPUT: &[Field] = &[
    Field::optional(
        "scope_kind",
        Type::Enum(SCOPE_ENUM),
        "Filter by scope: task, agent, or release",
    ),
    Field::optional("task", Type::String, "Filter by task ID"),
    Field::optional("agent", Type::String, "Filter by agent employee ID"),
    Field::optional("release", Type::String, "Filter by release ID"),
];

const BUDGET_INSPECT_INPUT: &[Field] = &[
    Field::optional("budget", Type::String, "Budget UUIDv7"),
    Field::optional(
        "scope_kind",
        Type::Enum(SCOPE_ENUM),
        "Scope kind when looking up by target instead of --budget",
    ),
    Field::optional("task", Type::String, "Task ID for scope lookup"),
    Field::optional("agent", Type::String, "Agent ID for scope lookup"),
    Field::optional("release", Type::String, "Release ID for scope lookup"),
    Field::optional(
        "period_kind",
        Type::Enum(PERIOD_ENUM),
        "Period kind: lifetime, day, or month",
    ),
];

const BUDGET_INSPECT_OUTPUT: &[Field] = &[Field::optional(
    "budget",
    Type::Object(BUDGET_VIEW_FIELDS),
    "Matching budget view, if present",
)];

/// The `budget` rows.
pub const BUDGET_COMMANDS: &[Command] = &[
    Command {
        resource: "budget",
        verb: "status",
        summary: "Check own budget status",
        call: "budget.agent_own",
        kind: Kind::Read,
        input: BUDGET_STATUS_INPUT,
        content_field: None,
        output: BUDGET_STATUS_OUTPUT,
        errors: BUDGET_ERRORS,
        example: Example {
            input: "{}",
            output: r#"{"agent_id":"0193a5c0-0000-7000-8000-000000000003"}"#,
        },
    },
    Command {
        resource: "budget",
        verb: "list",
        summary: "List visible budgets",
        call: "budget.list",
        kind: Kind::List,
        input: BUDGET_LIST_INPUT,
        content_field: None,
        output: BUDGET_VIEW_FIELDS,
        errors: BUDGET_ERRORS,
        example: Example {
            input: "{}",
            output: r#"[{"id":"0193a5c0-0000-7000-8000-000000000701","scope_kind":"agent","period_kind":"day","state":"active","state_summary":"ok","amounts_visible":true,"unit":"micro_usd","limit_amount":5000000}]"#,
        },
    },
    Command {
        resource: "budget",
        verb: "inspect",
        summary: "Inspect one budget by ID or target scope",
        call: "budget.get",
        kind: Kind::Read,
        input: BUDGET_INSPECT_INPUT,
        content_field: None,
        output: BUDGET_INSPECT_OUTPUT,
        errors: BUDGET_ERRORS,
        example: Example {
            input: r#"{"budget":"0193a5c0-0000-7000-8000-000000000701"}"#,
            output: r#"{"budget":{"id":"0193a5c0-0000-7000-8000-000000000701","scope_kind":"agent","period_kind":"day","state":"active","state_summary":"ok","amounts_visible":true,"unit":"micro_usd","limit_amount":5000000}}"#,
        },
    },
];

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

/// The `cost` rows.
pub const COST_COMMANDS: &[Command] = &[
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

/// All command row slices in the budget & cost domain.
pub const ALL: &[&[Command]] = &[BUDGET_COMMANDS, COST_COMMANDS];
