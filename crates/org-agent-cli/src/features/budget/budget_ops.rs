//! Budget read commands: `budget status`, `budget list`, and `budget inspect`.

use org_command::{Answer, Command, Example, Field, Kind, Refusal, Request, Type};
use org_proto::budgets::{
    BudgetAgentOwn, BudgetAgentOwnParams, BudgetAgentOwnResult, BudgetGet, BudgetGetParams,
    BudgetGetResult, BudgetList, BudgetListParams, BudgetListResult, BudgetPeriodKind,
    BudgetScopeKind,
};
use org_proto::ErrorCode;
use serde_json::Value;

use super::BUDGET_ERRORS;
use crate::runtime::domain::CommandCue;
use crate::runtime::input::{call_rpc, encode, opt_str, opt_string};
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport};

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

pub(super) const COMMANDS: &[Command] = &[
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

pub(super) const CUES: &[CommandCue] = &[
    (
        "budget",
        "status",
        "checking your own daily, monthly, or task budget status before starting expensive work.",
    ),
    (
        "budget",
        "list",
        "listing active task, agent, or release budgets and checking for warnings or execution holds.",
    ),
    (
        "budget",
        "inspect",
        "inspecting one budget's exact limit, spend, hold state, and extension eligibility.",
    ),
];

fn parse_scope(raw: Option<&str>) -> Result<Option<BudgetScopeKind>, Refusal> {
    match raw {
        None => Ok(None),
        Some("task") => Ok(Some(BudgetScopeKind::Task)),
        Some("agent") => Ok(Some(BudgetScopeKind::Agent)),
        Some("release") => Ok(Some(BudgetScopeKind::Release)),
        Some(other) => Err(Refusal::new(
            ErrorCode::InvalidRequest,
            format!("unknown budget scope `{other}`"),
            "Pass --scope-kind task, agent, or release.",
        )),
    }
}

fn parse_period(raw: Option<&str>) -> Result<Option<BudgetPeriodKind>, Refusal> {
    match raw {
        None => Ok(None),
        Some("lifetime") => Ok(Some(BudgetPeriodKind::Lifetime)),
        Some("day") => Ok(Some(BudgetPeriodKind::Day)),
        Some("month") => Ok(Some(BudgetPeriodKind::Month)),
        Some(other) => Err(Refusal::new(
            ErrorCode::InvalidRequest,
            format!("unknown budget period `{other}`"),
            "Pass --period-kind lifetime, day, or month.",
        )),
    }
}

pub(super) fn read_budget_status<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = BudgetAgentOwnParams {
        task_id: opt_string(request, "task"),
    };
    let res: BudgetAgentOwnResult = call_rpc::<BudgetAgentOwn, _>(transport, &params)?;
    Ok(Answer::completed(encode(&res, "budget.agent_own")?))
}

pub(super) fn read_budget_list<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    parse_cursor(request.cursor.as_deref())?;
    let params = BudgetListParams {
        scope_kind: parse_scope(opt_str(request, "scope_kind"))?,
        task_id: opt_string(request, "task"),
        agent_id: opt_string(request, "agent"),
        release_id: opt_string(request, "release"),
    };
    let res: BudgetListResult = call_rpc::<BudgetList, _>(transport, &params)?;
    let items = match encode(&res.budgets, "budget.list")? {
        Value::Array(items) => items,
        _ => Vec::new(),
    };
    paginate(request, items)
}

pub(super) fn read_budget_inspect<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let budget_id = opt_string(request, "budget");
    let scope_kind = parse_scope(opt_str(request, "scope_kind"))?;
    if budget_id.is_none() && scope_kind.is_none() {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "`budget inspect` requires --budget or --scope-kind.",
            "Pass --budget <id> or --scope-kind <task|agent|release> with its target ID.",
        ));
    }
    let params = BudgetGetParams {
        budget_id,
        scope_kind,
        task_id: opt_string(request, "task"),
        agent_id: opt_string(request, "agent"),
        release_id: opt_string(request, "release"),
        period_kind: parse_period(opt_str(request, "period_kind"))?,
    };
    let res: BudgetGetResult = call_rpc::<BudgetGet, _>(transport, &params)?;
    let mut data = serde_json::Map::new();
    if let Some(budget) = res.budget {
        data.insert("budget".into(), encode(&budget, "budget.get")?);
    }
    Ok(Answer::completed(Value::Object(data)))
}
