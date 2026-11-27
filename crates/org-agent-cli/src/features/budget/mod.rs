//! The budget slice: `budget status [--task <ref>]` (`TASK-53`, `doc-92`, `doc-51` §1), the
//! caller's own daily, monthly and task allowance.

use org_command::{Answer, Command, CommandTable, Example, Field, Kind, Refusal, Request, Type};
use org_proto::budgets::{BudgetAgentOwn, BudgetAgentOwnParams, BudgetAgentOwnResult};
use org_proto::ErrorCode;

use crate::runtime::domain::{CommandCue, Domain};
use crate::runtime::input::{call_rpc, encode, opt_string};
use crate::runtime::transport::OfficeTransport;

/// The budget slice as the registry composes it.
pub(crate) const DOMAIN: Domain = Domain {
    commands: &[COMMANDS],
    resources: &[(
        "budget",
        "checking your own daily, monthly, or task budget limits and remaining allowance.",
    )],
    cues: &[CUES],
    dispatch,
};

const BUDGET_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidRequest,
    ErrorCode::NotBound,
    ErrorCode::NotFound,
    ErrorCode::NotAuthorized,
    ErrorCode::Unavailable,
];

const BUDGET_VIEW_FIELDS: &[Field] = &[
    Field::required("id", Type::String, "Budget ID"),
    Field::required("scope_kind", Type::String, "Scope kind"),
    Field::required("period_kind", Type::String, "Period kind"),
    Field::required("state", Type::String, "Policy state"),
    Field::required("state_summary", Type::String, "Budget status summary"),
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

/// The `budget` rows.
const COMMANDS: &[Command] = &[Command {
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
}];

/// `Use when:` guidance of each `budget` command.
const CUES: &[CommandCue] = &[(
    "budget",
    "status",
    "checking your own daily, monthly, or task budget status before starting expensive work.",
)];

fn dispatch(
    transport: &mut dyn OfficeTransport,
    request: &Request,
    _table: &CommandTable,
) -> Option<Result<Answer, Refusal>> {
    if request.command.resource != "budget" || request.command.verb != "status" {
        return None;
    }
    Some(read_budget_status(transport, request))
}

fn read_budget_status<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = BudgetAgentOwnParams {
        task_id: opt_string(request, "task"),
    };
    let res: BudgetAgentOwnResult = call_rpc::<BudgetAgentOwn, _>(transport, &params)?;
    Ok(Answer::completed(encode(&res, "budget.agent_own")?))
}

#[cfg(test)]
mod tests;
