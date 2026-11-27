//! Budget read commands: `budget status`, `budget list`, and `budget inspect`.

use org_command::{Answer, Refusal, Request};
use org_proto::budgets::{
    BudgetAgentOwn, BudgetAgentOwnParams, BudgetAgentOwnResult, BudgetGet, BudgetGetParams,
    BudgetGetResult, BudgetList, BudgetListParams, BudgetListResult, BudgetPeriodKind,
    BudgetScopeKind,
};
use org_proto::ErrorCode;
use serde_json::Value;

use crate::runtime::domain::CommandCue;
use crate::runtime::input::{call_rpc, encode, opt_str, opt_string};
use crate::runtime::transport::{paginate, parse_cursor, OfficeTransport};

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
