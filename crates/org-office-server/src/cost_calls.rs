//! Office RPC dispatcher and owner Inbox source for `L2` usage/cost accounting and `L3`
//! budgets & execution holds (`TASK-53`, `doc-87`, `doc-92`, `doc-51`, `FC-084..093`).

use std::sync::Arc;

use org_office::Office;
use org_office_sessions::SessionManager;
use org_proto::budgets::{
    BudgetAgentOwn, BudgetClear, BudgetExtendOnce, BudgetGet, BudgetKeepPaused, BudgetList,
    BudgetScopeKind, BudgetSet, BudgetStop,
};
use org_proto::costs::{
    CostExportCsv, CostPriceList, CostPriceSet, CostQuery, CostSubscriptionUsageCollect,
    CostSubscriptionUsageCredentialRevoke, CostSubscriptionUsageCredentialSet,
    CostSubscriptionUsageList, CostUsageCorrect, CostUsageRecord,
};
use org_proto::inbox::{
    Action, InboxClears, InboxCounts, InboxItem, InboxKind, InboxState, Open, OpenTarget,
};
use org_proto::{ApiError, Call, ErrorCode, Request, Response};
use serde_json::{json, Map};

use crate::inbox::{Query, Source, SourceFailure, SourceRead};
use crate::live::LiveEvents;

/// Whether `request` belongs to the `L2`/`L3` `cost.*` or `budget.*` RPC families.
pub(crate) fn is_cost_or_budget_method(request: &Request) -> bool {
    is_cost_method(request) || is_subscription_usage_method(request) || is_budget_method(request)
}

fn is_cost_method(request: &Request) -> bool {
    request.is::<CostPriceSet>()
        || request.is::<CostPriceList>()
        || request.is::<CostUsageRecord>()
        || request.is::<CostUsageCorrect>()
        || request.is::<CostQuery>()
        || request.is::<CostExportCsv>()
}

fn is_subscription_usage_method(request: &Request) -> bool {
    request.is::<CostSubscriptionUsageList>()
        || request.is::<CostSubscriptionUsageCollect>()
        || request.is::<CostSubscriptionUsageCredentialSet>()
        || request.is::<CostSubscriptionUsageCredentialRevoke>()
}

fn is_budget_method(request: &Request) -> bool {
    request.is::<BudgetSet>()
        || request.is::<BudgetClear>()
        || request.is::<BudgetExtendOnce>()
        || request.is::<BudgetStop>()
        || request.is::<BudgetKeepPaused>()
        || request.is::<BudgetGet>()
        || request.is::<BudgetList>()
        || request.is::<BudgetAgentOwn>()
}

/// Dispatches one `cost.*` or `budget.*` RPC against [`Office`], notifying live inbox hints
/// and retrying budget-held sessions when holds clear (`ASK-8`, `doc-92`).
pub(crate) fn dispatch_cost_budget_rpc(
    office: &Office,
    sessions: Option<&Arc<SessionManager>>,
    live: &Arc<LiveEvents>,
    member: Option<&str>,
    request: &Request,
) -> Option<Response> {
    if !is_cost_or_budget_method(request) {
        return None;
    }
    let Some(caller) = member else {
        return Some(Response::error(
            request.id,
            ApiError::new(ErrorCode::NotBound, "connection is not bound"),
        ));
    };
    if let Some(resp) = dispatch_cost_rpc(office, sessions, live, caller, request) {
        return Some(resp);
    }
    dispatch_budget_rpc(office, sessions, live, caller, request)
}

fn dispatch_cost_rpc(
    office: &Office,
    sessions: Option<&Arc<SessionManager>>,
    live: &Arc<LiveEvents>,
    caller: &str,
    request: &Request,
) -> Option<Response> {
    if request.is::<CostPriceSet>() {
        return Some(run_call::<CostPriceSet>(request, |p| {
            map_office(office.cost_price_set(caller, p))
        }));
    }
    if request.is::<CostPriceList>() {
        return Some(run_call::<CostPriceList>(request, |p| {
            map_office(office.cost_price_list(caller, p))
        }));
    }
    if request.is::<CostUsageRecord>() {
        return Some(run_call::<CostUsageRecord>(request, |p| {
            let res = map_office(office.cost_usage_record(caller, p))?;
            live.record_mailbox_change();
            Ok(res)
        }));
    }
    if request.is::<CostUsageCorrect>() {
        return Some(run_call::<CostUsageCorrect>(request, |p| {
            let res = map_office(office.cost_usage_correct(caller, p))?;
            live.record_mailbox_change();
            if let Some(mgr) = sessions {
                mgr.retry_budget_held();
            }
            Ok(res)
        }));
    }
    if request.is::<CostQuery>() {
        return Some(run_call::<CostQuery>(request, |p| {
            map_office(office.cost_query(caller, p))
        }));
    }
    if request.is::<CostExportCsv>() {
        return Some(run_call::<CostExportCsv>(request, |p| {
            map_office(office.cost_export_csv(caller, p))
        }));
    }
    dispatch_subscription_usage_rpc(office, sessions, caller, request)
}

fn dispatch_subscription_usage_rpc(
    office: &Office,
    sessions: Option<&Arc<SessionManager>>,
    caller: &str,
    request: &Request,
) -> Option<Response> {
    if request.is::<CostSubscriptionUsageList>() {
        return Some(run_call::<CostSubscriptionUsageList>(request, |p| {
            if let Some(mgr) = sessions {
                mgr.subscription_usage_list(caller, p)
            } else {
                map_office(office.cost_subscription_usage_list(caller, p))
            }
        }));
    }
    if request.is::<CostSubscriptionUsageCollect>() {
        return Some(run_call::<CostSubscriptionUsageCollect>(request, |p| {
            let mgr = sessions.ok_or_else(|| {
                ApiError::new(
                    ErrorCode::Unsupported,
                    "session manager unavailable for subscription usage collection",
                )
            })?;
            mgr.subscription_usage_collect(caller, p)
        }));
    }
    if request.is::<CostSubscriptionUsageCredentialSet>() {
        return Some(run_call::<CostSubscriptionUsageCredentialSet>(
            request,
            |p| {
                let mgr = sessions.ok_or_else(|| {
                    ApiError::new(
                        ErrorCode::Unsupported,
                        "session manager unavailable for usage credential custody",
                    )
                })?;
                mgr.subscription_usage_credential_set(caller, p)
            },
        ));
    }
    if request.is::<CostSubscriptionUsageCredentialRevoke>() {
        return Some(run_call::<CostSubscriptionUsageCredentialRevoke>(
            request,
            |p| {
                let mgr = sessions.ok_or_else(|| {
                    ApiError::new(
                        ErrorCode::Unsupported,
                        "session manager unavailable for usage credential custody",
                    )
                })?;
                mgr.subscription_usage_credential_revoke(caller, p)
            },
        ));
    }
    None
}

fn dispatch_budget_rpc(
    office: &Office,
    sessions: Option<&Arc<SessionManager>>,
    live: &Arc<LiveEvents>,
    caller: &str,
    request: &Request,
) -> Option<Response> {
    if request.is::<BudgetSet>() {
        return Some(run_call::<BudgetSet>(request, |p| {
            let res = map_office(office.budget_set(caller, p))?;
            notify_budget_change(sessions, live);
            Ok(res)
        }));
    }
    if request.is::<BudgetClear>() {
        return Some(run_call::<BudgetClear>(request, |p| {
            let res = map_office(office.budget_clear(caller, p))?;
            notify_budget_change(sessions, live);
            Ok(res)
        }));
    }
    if request.is::<BudgetExtendOnce>() {
        return Some(run_call::<BudgetExtendOnce>(request, |p| {
            let res = map_office(office.budget_extend_once(caller, p))?;
            notify_budget_change(sessions, live);
            Ok(res)
        }));
    }
    if request.is::<BudgetStop>() {
        return Some(run_call::<BudgetStop>(request, |p| {
            let res = map_office(office.budget_stop(caller, p))?;
            live.record_mailbox_change();
            Ok(res)
        }));
    }
    if request.is::<BudgetKeepPaused>() {
        return Some(run_call::<BudgetKeepPaused>(request, |p| {
            let res = map_office(office.budget_keep_paused(caller, p))?;
            live.record_mailbox_change();
            Ok(res)
        }));
    }
    if request.is::<BudgetGet>() {
        return Some(run_call::<BudgetGet>(request, |p| {
            map_office(office.budget_get(caller, p))
        }));
    }
    if request.is::<BudgetList>() {
        return Some(run_call::<BudgetList>(request, |p| {
            map_office(office.budget_list(caller, p))
        }));
    }
    if request.is::<BudgetAgentOwn>() {
        return Some(run_call::<BudgetAgentOwn>(request, |p| {
            map_office(office.budget_agent_own(caller, p))
        }));
    }
    None
}

fn notify_budget_change(sessions: Option<&Arc<SessionManager>>, live: &Arc<LiveEvents>) {
    live.record_mailbox_change();
    if let Some(mgr) = sessions {
        mgr.retry_budget_held();
    }
}

fn map_office<T>(res: Result<Result<T, ApiError>, org_office::OfficeError>) -> Result<T, ApiError> {
    res.map_err(|e| ApiError::new(ErrorCode::StorageError, e.to_string()))
        .and_then(|r| r)
}

fn run_call<C: Call>(
    request: &Request,
    handler: impl FnOnce(C::Params) -> Result<C::Result, ApiError>,
) -> Response {
    let params = match request.params::<C>() {
        Ok(p) => p,
        Err(_) => {
            return Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::InvalidRequest,
                    format!("invalid parameters for `{}`", C::METHOD),
                ),
            );
        }
    };
    match handler(params) {
        Ok(result) => Response::ok::<C>(request.id, &result).unwrap_or_else(|_| {
            Response::error(
                request.id,
                ApiError::new(ErrorCode::StorageError, "failed encoding response"),
            )
        }),
        Err(err) => Response::error(request.id, err),
    }
}

/// Owner Inbox source for unresolved `budget_reached` notices (`doc-92`, `doc-51` §4).
pub(crate) struct BudgetInboxSource {
    office: Arc<Office>,
    office_id: String,
}

impl BudgetInboxSource {
    pub(crate) fn new(office: Arc<Office>) -> Self {
        let office_id = office.info().office_id.clone();
        Self { office, office_id }
    }
}

impl Source for BudgetInboxSource {
    fn name(&self) -> &'static str {
        "budget"
    }

    fn read(&self, query: &Query<'_>) -> Result<SourceRead, SourceFailure> {
        let notices = self.office.open_budget_inbox_notices().map_err(|err| {
            crate::logged("inbox.budget", &err);
            SourceFailure
        })?;
        let mut counts = InboxCounts::default();
        let mut items = Vec::new();
        for entry in notices {
            let item = build_budget_inbox_item(&self.office_id, entry);
            counts.problems = counts.problems.saturating_add(1);
            counts.total = counts.total.saturating_add(1);
            let include = query.state == InboxState::Open
                && query.kinds.is_none_or(|kinds| kinds.contains(&item.kind))
                && query.project.is_none()
                && query.team.is_none()
                && query.after.is_none_or(|after| after.admits(&item));
            if include {
                items.push(item);
            }
        }
        items.truncate(query.limit.min(500));
        Ok(SourceRead { items, counts })
    }
}

fn build_budget_inbox_item(
    office_id: &str,
    entry: org_office::budgets::OpenBudgetInboxNotice,
) -> InboxItem {
    let created_at_ms = org_office::parse_rfc3339_ms(&entry.notice.created_at);
    let status_prefix = match entry.hold_state {
        Some(org_proto::budgets::BudgetHoldState::StopPending) => "Stopping · Budget reached",
        Some(org_proto::budgets::BudgetHoldState::Held) => "Paused · Budget reached",
        _ => "Budget reached",
    };
    let summary = match entry.budget.scope_kind {
        BudgetScopeKind::Task => format!("{status_prefix} for task"),
        BudgetScopeKind::Agent => format!(
            "{status_prefix} for agent ({})",
            entry.budget.period_kind.as_str()
        ),
        _ => status_prefix.to_owned(),
    };
    let effective_limit = if entry.period.extension_used {
        entry
            .policy
            .limit_amount
            .and_then(org_proto::budgets::extended_limit)
            .or(entry.policy.limit_amount)
            .unwrap_or(0)
    } else {
        entry.policy.limit_amount.unwrap_or(0)
    };
    let mut actions = Vec::new();
    let can_extend = !entry.period.extension_used
        && entry
            .policy
            .limit_amount
            .and_then(org_proto::budgets::extended_limit)
            .is_some_and(|ext| ext > entry.period.used_amount);
    if can_extend {
        let mut params = Map::new();
        params.insert("budget_id".into(), json!(entry.budget.id));
        actions.push(Action::Call {
            label: "Allow 20% once".into(),
            op: "budget.extend_once".into(),
            params,
            fresh: vec!["operation_id".into()],
            input: None,
        });
    }
    if entry.budget.scope_kind == BudgetScopeKind::Task {
        let mut stop_params = Map::new();
        stop_params.insert("budget_id".into(), json!(entry.budget.id));
        actions.push(Action::Call {
            label: "Stop".into(),
            op: "budget.stop".into(),
            params: stop_params,
            fresh: vec!["operation_id".into()],
            input: None,
        });
        if let Some(tid) = entry.budget.task_id.clone() {
            actions.push(Action::Open {
                label: "Open task".into(),
                open: Open {
                    target: OpenTarget::WorkItem,
                    id: tid,
                },
            });
        }
    } else if entry.budget.scope_kind == BudgetScopeKind::Agent {
        let mut keep_params = Map::new();
        keep_params.insert("budget_id".into(), json!(entry.budget.id));
        actions.push(Action::Call {
            label: "Keep paused".into(),
            op: "budget.keep_paused".into(),
            params: keep_params,
            fresh: vec!["operation_id".into()],
            input: None,
        });
        if let Some(aid) = entry.budget.agent_id.clone() {
            actions.push(Action::Open {
                label: "Open agent".into(),
                open: Open {
                    target: OpenTarget::Member,
                    id: aid,
                },
            });
        }
    }
    let context = format!(
        "{status_prefix}: budget {} ({} used of {} {}).",
        entry.budget.id,
        entry.period.used_amount,
        effective_limit,
        entry.policy.unit.as_str()
    );
    InboxItem {
        reference: format!("{office_id}:budget:{}", entry.notice.id),
        source: "budget".into(),
        kind: InboxKind::Problem,
        member: entry.budget.agent_id,
        project: None,
        team: None,
        summary,
        context: Some(context),
        recommendation: None,
        options: None,
        created_at_ms,
        expires_at_ms: None,
        state: InboxState::Open,
        handled_at_ms: None,
        outcome: None,
        clears: InboxClears::Answered,
        dismissable: false,
        actions,
    }
}
