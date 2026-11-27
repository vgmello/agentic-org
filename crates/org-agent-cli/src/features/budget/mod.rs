//! The budget & cost slice: `budget status|list|inspect` and `cost query|prices|subscription-usage`
//! (`TASK-53`, `TASK-80.4`, `doc-87`, `doc-92`, `doc-51`).

mod budget_ops;
mod cost;

use org_command::{Answer, CommandTable, Refusal, Request};

use crate::runtime::domain::Domain;
use crate::runtime::transport::OfficeTransport;

/// The budget & cost slice as the registry composes it.
pub(crate) const DOMAIN: Domain = Domain {
    commands: org_command_catalogue::budget::ALL,
    resources: &[
        (
            "budget",
            "checking daily, monthly, task, or release budget limits, holds, and remaining allowance.",
        ),
        (
            "cost",
            "querying scoped token usage and micro-USD cost totals, model pricing, and provider subscription snapshots.",
        ),
    ],
    cues: &[budget_ops::CUES, cost::CUES],
    dispatch,
};

fn dispatch(
    transport: &mut dyn OfficeTransport,
    request: &Request,
    _table: &CommandTable,
) -> Option<Result<Answer, Refusal>> {
    match (request.command.resource, request.command.verb) {
        ("budget", "status") => Some(budget_ops::read_budget_status(transport, request)),
        ("budget", "list") => Some(budget_ops::read_budget_list(transport, request)),
        ("budget", "inspect") => Some(budget_ops::read_budget_inspect(transport, request)),
        ("cost", "query") => Some(cost::read_cost_query(transport, request)),
        ("cost", "prices") => Some(cost::read_cost_prices(transport, request)),
        ("cost", "subscription-usage") => Some(cost::read_subscription_usage(transport, request)),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
