//! Browser HTTP routes for unified typed tasks, checklists, comments, watchers, history,
//! dependency hierarchy, searchable labels, reference/modified files, and ordering
//! (`TASK-49` B4/B5 + `TASK-77` B4b), final summaries and default definitions of done
//! (`TASK-88`). Private task notes are `note_routes`'.

use hyper::{http::request::Parts, Method, StatusCode};
use org_proto::tasks::{
    LabelMatchMode, OrganizationDefaultDodGet, OrganizationDefaultDodGetParams,
    OrganizationDefaultDodSet, OrganizationDefaultDodSetParams, ProjectDefaultDodSet,
    ProjectDefaultDodSetParams, TaskSortMode, TaskStatus, TaskStatusScope, TasksAddDependency,
    TasksAddDependencyParams, TasksAssign, TasksAssignParams, TasksChecklistAdd,
    TasksChecklistAddParams, TasksChecklistCheck, TasksChecklistCheckParams, TasksChecklistEdit,
    TasksChecklistEditParams, TasksChecklistMove, TasksChecklistMoveParams, TasksChecklistRemove,
    TasksChecklistRemoveParams, TasksComment, TasksCommentParams, TasksComments,
    TasksCommentsParams, TasksCreate, TasksCreateParams, TasksDependencies,
    TasksDependenciesParams, TasksFinalSummary, TasksFinalSummaryParams, TasksHistory,
    TasksHistoryParams, TasksInspect, TasksInspectParams, TasksLabels, TasksLabelsParams,
    TasksList, TasksListParams, TasksRemoveDependency, TasksRemoveDependencyParams,
    TasksTransition, TasksTransitionParams, TasksUnwatch, TasksUnwatchParams, TasksUpdate,
    TasksUpdateParams, TasksWatch, TasksWatchParams,
};
use org_proto::web::{ReadResponse, WebError, WebErrorCode};
use serde_json::Value;

use super::hq_context::{resolve_hq_office, revalidate_and_resolve_hq};
use super::{
    failure, invalid, not_found, result, unavailable, Call, CallerAuth, Route, SessionApi,
};
use crate::host::Failure;
use crate::office::OfficeFailure;
use crate::service::{ApiReply, PreparedCall};

pub enum TaskRoute {
    List(CallerAuth, String, Box<TasksListParams>),
    Create(CallerAuth, String),
    Labels(CallerAuth, String, TasksLabelsParams),
    Inspect(CallerAuth, String, String),
    Update(CallerAuth, String),
    Transition(CallerAuth, String),
    Assign(CallerAuth, String),
    Comment(CallerAuth, String),
    Comments(CallerAuth, String, String),
    Watch(CallerAuth, String),
    Unwatch(CallerAuth, String),
    History(CallerAuth, String, TasksHistoryParams),
    Dependencies(CallerAuth, String, String),
    AddDependency(CallerAuth, String),
    RemoveDependency(CallerAuth, String),
    ChecklistAdd(CallerAuth, String),
    ChecklistCheck(CallerAuth, String),
    ChecklistEdit(CallerAuth, String),
    ChecklistRemove(CallerAuth, String),
    ChecklistMove(CallerAuth, String),
    FinalSummary(CallerAuth, String),
    DefaultDod(CallerAuth, String),
    SetDefaultDod(CallerAuth, String),
    SetProjectDefaultDod(CallerAuth, String),
}

pub enum TaskCall {
    List(CallerAuth, String, Box<TasksListParams>),
    Create(CallerAuth, String, Box<TasksCreateParams>),
    Labels(CallerAuth, String, TasksLabelsParams),
    Inspect(CallerAuth, String, String),
    Update(CallerAuth, String, Box<TasksUpdateParams>),
    Transition(CallerAuth, String, Box<TasksTransitionParams>),
    Assign(CallerAuth, String, Box<TasksAssignParams>),
    Comment(CallerAuth, String, Box<TasksCommentParams>),
    Comments(CallerAuth, String, String),
    Watch(CallerAuth, String, TasksWatchParams),
    Unwatch(CallerAuth, String, TasksUnwatchParams),
    History(CallerAuth, String, TasksHistoryParams),
    Dependencies(CallerAuth, String, String),
    AddDependency(CallerAuth, String, TasksAddDependencyParams),
    RemoveDependency(CallerAuth, String, TasksRemoveDependencyParams),
    ChecklistAdd(CallerAuth, String, TasksChecklistAddParams),
    ChecklistCheck(CallerAuth, String, TasksChecklistCheckParams),
    ChecklistEdit(CallerAuth, String, TasksChecklistEditParams),
    ChecklistRemove(CallerAuth, String, TasksChecklistRemoveParams),
    ChecklistMove(CallerAuth, String, TasksChecklistMoveParams),
    FinalSummary(CallerAuth, String, Box<TasksFinalSummaryParams>),
    DefaultDod(CallerAuth, String),
    SetDefaultDod(CallerAuth, String, Box<OrganizationDefaultDodSetParams>),
    SetProjectDefaultDod(CallerAuth, String, Box<ProjectDefaultDodSetParams>),
}

fn valid_org_id(id: &str) -> Result<(), ApiReply> {
    org_office_launch::is_storage_id(id)
        .then_some(())
        .ok_or_else(invalid)
}

fn valid_task_ref(task: &str) -> Result<(), ApiReply> {
    if task.is_empty() || task.len() > 128 {
        return Err(invalid());
    }
    Ok(())
}

fn is_reserved_task_action(segment: &str) -> bool {
    matches!(
        segment,
        "update"
            | "transition"
            | "assign"
            | "comment"
            | "watch"
            | "unwatch"
            | "dependencies"
            | "checklist"
            | "notes"
            | "final-summary"
    )
}

fn decode_query_value(raw: &str) -> Result<String, ApiReply> {
    let mut out = Vec::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
        } else if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err(invalid());
            }
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).map_err(|_| invalid())?;
            let b = u8::from_str_radix(hex, 16).map_err(|_| invalid())?;
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| invalid())
}

fn parse_bool_query(raw: &str) -> Result<bool, ApiReply> {
    match raw {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => Err(invalid()),
    }
}

fn parse_history_query(task: &str, query: Option<&str>) -> Result<TasksHistoryParams, ApiReply> {
    let mut params = TasksHistoryParams::new(task);
    let Some(q) = query.filter(|s| !s.is_empty()) else {
        return Ok(params);
    };
    for pair in q.split('&') {
        let Some((k, raw_v)) = pair.split_once('=') else {
            return Err(invalid());
        };
        let v = decode_query_value(raw_v)?;
        match k {
            "event_type" => {
                let trimmed = v.trim();
                if trimmed.is_empty() {
                    return Err(invalid());
                }
                params.event_type = Some(trimmed.to_owned());
            }
            "blocks_only" => params.blocks_only = parse_bool_query(&v)?,
            _ => return Err(invalid()),
        }
    }
    if params.blocks_only
        && params
            .event_type
            .as_deref()
            .is_some_and(|kind| kind != "status_changed")
    {
        return Err(invalid());
    }
    Ok(params)
}

fn parse_list_query(query: Option<&str>) -> Result<TasksListParams, ApiReply> {
    let mut params = TasksListParams::default();
    let Some(q) = query.filter(|s| !s.is_empty()) else {
        return Ok(params);
    };
    for pair in q.split('&') {
        let Some((k, raw_v)) = pair.split_once('=') else {
            return Err(invalid());
        };
        let v = decode_query_value(raw_v)?;
        match k {
            "type" => params.task_type = Some(v),
            "status" => params.status = Some(TaskStatus::parse(&v).ok_or_else(invalid)?),
            "team_id" => params.team_id = Some(v),
            "assigned_team_id" => params.assigned_team_id = Some(v),
            "assignee_id" => params.assignee_id = Some(v),
            "project_id" => params.project_id = Some(v),
            "label" => params.labels.push(v),
            "labels" => {
                for item in v.split(',').filter(|s| !s.trim().is_empty()) {
                    params.labels.push(item.trim().to_owned());
                }
            }
            "label_match" => {
                params.label_match = match v.as_str() {
                    "all" | "and" => LabelMatchMode::All,
                    "any" | "or" => LabelMatchMode::Any,
                    _ => return Err(invalid()),
                };
            }
            "depends_on" => params.depends_on = Some(v),
            "dependent_of" => params.dependent_of = Some(v),
            "q" | "query" => params.query = Some(v),
            "sort" => {
                params.sort = match v.as_str() {
                    "order" => TaskSortMode::Order,
                    "number" => TaskSortMode::Number,
                    "updated_desc" => TaskSortMode::UpdatedDesc,
                    _ => return Err(invalid()),
                };
            }
            "limit" => params.limit = Some(v.parse::<u32>().map_err(|_| invalid())?),
            "offset" => params.offset = Some(v.parse::<u64>().map_err(|_| invalid())?),
            // `TASK-105`: strict RFC 3339 validation matching `org-office`.
            "status_scope" => {
                params.status_scope = Some(TaskStatusScope::parse(&v).ok_or_else(invalid)?);
            }
            "done_since" => {
                let trimmed = v.trim();
                if org_proto::parse_rfc3339_ms(trimmed).is_none() {
                    return Err(invalid());
                }
                params.done_since = Some(trimmed.to_owned());
            }
            // `TASK-84`: two-kind blocking filters (`ready` and `dependency_blocked`).
            "ready" => params.ready = parse_bool_query(&v)?,
            "dependency_blocked" => params.dependency_blocked = parse_bool_query(&v)?,
            _ => return Err(invalid()),
        }
    }
    if params.ready && params.dependency_blocked {
        return Err(invalid());
    }
    Ok(params)
}

fn parse_labels_query(query: Option<&str>) -> Result<TasksLabelsParams, ApiReply> {
    let mut params = TasksLabelsParams::default();
    let Some(q) = query.filter(|s| !s.is_empty()) else {
        return Ok(params);
    };
    for pair in q.split('&') {
        let Some((k, raw_v)) = pair.split_once('=') else {
            return Err(invalid());
        };
        if matches!(k, "q" | "query") {
            params.query = Some(decode_query_value(raw_v)?);
        } else {
            return Err(invalid());
        }
    }
    Ok(params)
}

fn auth_org(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    build: fn(CallerAuth, String) -> TaskRoute,
) -> Result<Route, ApiReply> {
    let pair = api.caller_auth(request)?;
    valid_org_id(org)?;
    Ok(Route::Tasks(build(pair, org.to_owned())))
}

/// Attempts to route unified task paths under `/api/v1/organizations/{org}/...`.
pub(super) fn try_route(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    tail: &[&str],
    _get: bool,
    post: bool,
    query: bool,
) -> Option<Result<Route, ApiReply>> {
    let is_get = matches!(request.method, Method::GET | Method::HEAD);
    match tail {
        // `TASK-88`: the organization's default definition of done, and a project's.
        ["task-defaults", "dod"] if is_get && !query => {
            Some(auth_org(api, request, org, TaskRoute::DefaultDod))
        }
        ["task-defaults", "dod"] if post => {
            Some(auth_org(api, request, org, TaskRoute::SetDefaultDod))
        }
        ["task-defaults", "dod", "project"] if post => {
            Some(auth_org(api, request, org, TaskRoute::SetProjectDefaultDod))
        }
        ["task-defaults", ..] => Some(Err(invalid())),
        ["task-labels"] if is_get => Some((|| {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            let params = parse_labels_query(request.uri.query())?;
            Ok(Route::Tasks(TaskRoute::Labels(
                pair,
                org.to_owned(),
                params,
            )))
        })()),
        ["tasks"] if is_get => Some((|| {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            let params = parse_list_query(request.uri.query())?;
            Ok(Route::Tasks(TaskRoute::List(
                pair,
                org.to_owned(),
                Box::new(params),
            )))
        })()),
        ["tasks"] if post => Some(auth_org(api, request, org, TaskRoute::Create)),
        ["tasks", action] if post => route_task_post_action(api, request, org, action),
        ["tasks", "dependencies", "add"] if post => {
            Some(auth_org(api, request, org, TaskRoute::AddDependency))
        }
        ["tasks", "dependencies", "remove"] if post => {
            Some(auth_org(api, request, org, TaskRoute::RemoveDependency))
        }
        ["tasks", "checklist", action] if post => {
            route_checklist_post_action(api, request, org, action)
        }
        ["tasks", task] if is_get && !query => Some((|| {
            if is_reserved_task_action(task) {
                return Err(invalid());
            }
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            valid_task_ref(task)?;
            Ok(Route::Tasks(TaskRoute::Inspect(
                pair,
                org.to_owned(),
                (*task).to_owned(),
            )))
        })()),
        ["tasks", task, "history"] if is_get => Some((|| {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            valid_task_ref(task)?;
            let params = parse_history_query(task, request.uri.query())?;
            Ok(Route::Tasks(TaskRoute::History(
                pair,
                org.to_owned(),
                params,
            )))
        })()),
        ["tasks", task, sub] if is_get && !query => Some((|| {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            valid_task_ref(task)?;
            let route = match *sub {
                "comments" => TaskRoute::Comments(pair, org.to_owned(), (*task).to_owned()),
                "dependencies" => TaskRoute::Dependencies(pair, org.to_owned(), (*task).to_owned()),
                _ => return Err(not_found()),
            };
            Ok(Route::Tasks(route))
        })()),
        ["tasks", ..] | ["task-labels", ..] => Some(Err(invalid())),
        _ => None,
    }
}

fn route_task_post_action(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    action: &str,
) -> Option<Result<Route, ApiReply>> {
    let build: fn(CallerAuth, String) -> TaskRoute = match action {
        "update" => TaskRoute::Update,
        "transition" => TaskRoute::Transition,
        "assign" => TaskRoute::Assign,
        "comment" => TaskRoute::Comment,
        "watch" => TaskRoute::Watch,
        "unwatch" => TaskRoute::Unwatch,
        "final-summary" => TaskRoute::FinalSummary,
        _ => return Some(Err(not_found())),
    };
    Some(auth_org(api, request, org, build))
}

fn route_checklist_post_action(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    action: &str,
) -> Option<Result<Route, ApiReply>> {
    let build: fn(CallerAuth, String) -> TaskRoute = match action {
        "add" => TaskRoute::ChecklistAdd,
        "check" => TaskRoute::ChecklistCheck,
        "edit" => TaskRoute::ChecklistEdit,
        "remove" => TaskRoute::ChecklistRemove,
        "move" => TaskRoute::ChecklistMove,
        _ => return Some(Err(not_found())),
    };
    Some(auth_org(api, request, org, build))
}

fn empty(bytes: &[u8]) -> Result<(), ApiReply> {
    if bytes.is_empty() {
        Ok(())
    } else {
        Err(invalid())
    }
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, ApiReply> {
    serde_json::from_slice(bytes).map_err(|_| invalid())
}

pub(super) fn prepare(route: TaskRoute, bytes: &[u8]) -> Result<PreparedCall<Call>, ApiReply> {
    let call = match route {
        TaskRoute::List(c, o, p) => {
            empty(bytes)?;
            TaskCall::List(c, o, p)
        }
        TaskRoute::Create(c, o) => TaskCall::Create(c, o, Box::new(decode(bytes)?)),
        TaskRoute::Labels(c, o, p) => {
            empty(bytes)?;
            TaskCall::Labels(c, o, p)
        }
        TaskRoute::Inspect(c, o, t) => {
            empty(bytes)?;
            TaskCall::Inspect(c, o, t)
        }
        TaskRoute::Update(c, o) => TaskCall::Update(c, o, Box::new(decode(bytes)?)),
        TaskRoute::Transition(c, o) => TaskCall::Transition(c, o, Box::new(decode(bytes)?)),
        TaskRoute::Assign(c, o) => TaskCall::Assign(c, o, Box::new(decode(bytes)?)),
        TaskRoute::Comment(c, o) => TaskCall::Comment(c, o, Box::new(decode(bytes)?)),
        TaskRoute::Comments(c, o, t) => {
            empty(bytes)?;
            TaskCall::Comments(c, o, t)
        }
        TaskRoute::Watch(c, o) => TaskCall::Watch(c, o, decode(bytes)?),
        TaskRoute::Unwatch(c, o) => TaskCall::Unwatch(c, o, decode(bytes)?),
        TaskRoute::History(c, o, t) => {
            empty(bytes)?;
            TaskCall::History(c, o, t)
        }
        TaskRoute::Dependencies(c, o, t) => {
            empty(bytes)?;
            TaskCall::Dependencies(c, o, t)
        }
        TaskRoute::AddDependency(c, o) => TaskCall::AddDependency(c, o, decode(bytes)?),
        TaskRoute::RemoveDependency(c, o) => TaskCall::RemoveDependency(c, o, decode(bytes)?),
        TaskRoute::ChecklistAdd(c, o) => TaskCall::ChecklistAdd(c, o, decode(bytes)?),
        TaskRoute::ChecklistCheck(c, o) => TaskCall::ChecklistCheck(c, o, decode(bytes)?),
        TaskRoute::ChecklistEdit(c, o) => TaskCall::ChecklistEdit(c, o, decode(bytes)?),
        TaskRoute::ChecklistRemove(c, o) => TaskCall::ChecklistRemove(c, o, decode(bytes)?),
        TaskRoute::ChecklistMove(c, o) => TaskCall::ChecklistMove(c, o, decode(bytes)?),
        TaskRoute::FinalSummary(c, o) => TaskCall::FinalSummary(c, o, Box::new(decode(bytes)?)),
        TaskRoute::DefaultDod(c, o) => {
            empty(bytes)?;
            TaskCall::DefaultDod(c, o)
        }
        TaskRoute::SetDefaultDod(c, o) => TaskCall::SetDefaultDod(c, o, Box::new(decode(bytes)?)),
        TaskRoute::SetProjectDefaultDod(c, o) => {
            TaskCall::SetProjectDefaultDod(c, o, Box::new(decode(bytes)?))
        }
    };
    Ok(PreparedCall::read(Call::Tasks(call), bytes.len()))
}

async fn run_office<C: org_proto::Call>(
    api: &SessionApi,
    pair: CallerAuth,
    org_id: &str,
    params: &C::Params,
    is_read: bool,
) -> ApiReply {
    if let Err(error) = api.validate_async(pair).await {
        return failure(error);
    }
    let mut office_id = match resolve_hq_office(api, pair, org_id).await {
        Ok(id) => id,
        Err(Failure::Refused(_)) => return unavailable(),
        Err(error) => return failure(error),
    };
    let auth = api.auth(&pair);
    let mut reply = api
        .offices
        .call_typed::<C>(&auth, &office_id, params, is_read)
        .await;
    if matches!(
        &reply,
        Err(OfficeFailure::Refused(e)) if e.code == org_proto::ErrorCode::NotAuthorized
    ) {
        office_id = match revalidate_and_resolve_hq(api, pair, org_id).await {
            Ok(id) => id,
            Err(Failure::Refused(_)) => return unavailable(),
            Err(error) => return failure(error),
        };
        reply = api
            .offices
            .call_typed::<C>(&auth, &office_id, params, is_read)
            .await;
    }
    match reply {
        Ok(res) => result(res),
        Err(err) => map_office_failure(err),
    }
}

fn map_office_failure(error: OfficeFailure) -> ApiReply {
    match error {
        OfficeFailure::BeforeDispatch(e) => failure(e),
        OfficeFailure::Transport => unavailable(),
        OfficeFailure::Refused(err) => {
            let web = WebError::from(err);
            let status = match web.code {
                WebErrorCode::InvalidRequest
                | WebErrorCode::InvalidName
                | WebErrorCode::Cycle
                | WebErrorCode::UnknownRecipient => StatusCode::BAD_REQUEST,
                WebErrorCode::NotAuthorized | WebErrorCode::OwnerProtected => StatusCode::FORBIDDEN,
                WebErrorCode::NotFound => return not_found(),
                WebErrorCode::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
                // `TASK-88`: completing with required steps unticked conflicts with the task's
                // state; clients branch on the code ("Mark done anyway…").
                WebErrorCode::Conflict
                | WebErrorCode::AlreadyExists
                | WebErrorCode::NotEmpty
                | WebErrorCode::OperationReused
                | WebErrorCode::ChecklistIncomplete => StatusCode::CONFLICT,
                WebErrorCode::Expired => StatusCode::GONE,
                _ => return unavailable(),
            };
            ApiReply::json(
                status,
                serde_json::to_value(ReadResponse::<Value>::Error(web)).unwrap_or(Value::Null),
            )
        }
    }
}

pub(super) async fn execute(api: &SessionApi, call: TaskCall) -> ApiReply {
    match call {
        TaskCall::List(c, o, p) => run_office::<TasksList>(api, c, &o, &p, true).await,
        TaskCall::Create(c, o, p) => run_office::<TasksCreate>(api, c, &o, &p, false).await,
        TaskCall::Labels(c, o, p) => run_office::<TasksLabels>(api, c, &o, &p, true).await,
        TaskCall::Inspect(c, o, task) => {
            run_office::<TasksInspect>(api, c, &o, &TasksInspectParams::new(task), true).await
        }
        TaskCall::Update(c, o, p) => run_office::<TasksUpdate>(api, c, &o, &p, false).await,
        TaskCall::Transition(c, o, p) => run_office::<TasksTransition>(api, c, &o, &p, false).await,
        TaskCall::Assign(c, o, p) => run_office::<TasksAssign>(api, c, &o, &p, false).await,
        TaskCall::Comment(c, o, p) => run_office::<TasksComment>(api, c, &o, &p, false).await,
        TaskCall::Comments(c, o, task) => {
            run_office::<TasksComments>(api, c, &o, &TasksCommentsParams { task }, true).await
        }
        TaskCall::Watch(c, o, p) => run_office::<TasksWatch>(api, c, &o, &p, false).await,
        TaskCall::Unwatch(c, o, p) => run_office::<TasksUnwatch>(api, c, &o, &p, false).await,
        TaskCall::History(c, o, params) => {
            run_office::<TasksHistory>(api, c, &o, &params, true).await
        }
        TaskCall::Dependencies(c, o, task) => {
            run_office::<TasksDependencies>(api, c, &o, &TasksDependenciesParams { task }, true)
                .await
        }
        TaskCall::AddDependency(c, o, p) => {
            run_office::<TasksAddDependency>(api, c, &o, &p, false).await
        }
        TaskCall::RemoveDependency(c, o, p) => {
            run_office::<TasksRemoveDependency>(api, c, &o, &p, false).await
        }
        TaskCall::ChecklistAdd(c, o, p) => {
            run_office::<TasksChecklistAdd>(api, c, &o, &p, false).await
        }
        TaskCall::ChecklistCheck(c, o, p) => {
            run_office::<TasksChecklistCheck>(api, c, &o, &p, false).await
        }
        TaskCall::ChecklistEdit(c, o, p) => {
            run_office::<TasksChecklistEdit>(api, c, &o, &p, false).await
        }
        TaskCall::ChecklistRemove(c, o, p) => {
            run_office::<TasksChecklistRemove>(api, c, &o, &p, false).await
        }
        TaskCall::ChecklistMove(c, o, p) => {
            run_office::<TasksChecklistMove>(api, c, &o, &p, false).await
        }
        TaskCall::FinalSummary(c, o, p) => {
            run_office::<TasksFinalSummary>(api, c, &o, &p, false).await
        }
        TaskCall::DefaultDod(c, o) => {
            let params = OrganizationDefaultDodGetParams {};
            run_office::<OrganizationDefaultDodGet>(api, c, &o, &params, true).await
        }
        TaskCall::SetDefaultDod(c, o, p) => {
            run_office::<OrganizationDefaultDodSet>(api, c, &o, &p, false).await
        }
        TaskCall::SetProjectDefaultDod(c, o, p) => {
            run_office::<ProjectDefaultDodSet>(api, c, &o, &p, false).await
        }
    }
}

#[cfg(test)]
mod tests;
