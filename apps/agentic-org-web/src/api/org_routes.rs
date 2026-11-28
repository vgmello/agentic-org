//! Organization snapshot, mutation, and operation-lookup handlers for [`SessionApi`].
//! Extracted from `api.rs` to keep the session/host API surface decoupled from
//! browser-to-office organization routing and `web-api-v1` §9 authorization rechecks.

use hyper::{http::request::Parts, Method, StatusCode};
use org_proto::web::{
    self as gateway,
    org_mutations::{classify_org_reply, OrgOperation, OrgReplyClassification},
    organizations, MutationOutcome, OperationId, OperationKind, OperationLookup, Scope,
    WebErrorCode,
};

use super::{
    bad_input, failure, invalid, not_found, result, unavailable, Call, CallerAuth, Route,
    SessionApi,
};
use crate::{
    office::OfficeFailure,
    service::{ApiReply, PreparedCall},
};

mod office_calls;
mod org_calls;
mod project_calls;

pub(super) use office_calls::{execute_office_restart, execute_org_offices};
pub(super) use org_calls::{
    execute_cli_approval_decide, execute_org_mutation, execute_org_operation, execute_org_snapshot,
};
pub(super) use project_calls::{
    execute_project_create, execute_project_inspect, execute_projects_list,
};

fn valid_org_id(id: &str) -> Result<(), ApiReply> {
    org_office_launch::is_storage_id(id)
        .then_some(())
        .ok_or_else(invalid)
}

fn route_projects(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    tail: &[&str],
    post: bool,
    query: bool,
) -> Result<Route, ApiReply> {
    let is_get = matches!(request.method, Method::GET | Method::HEAD);
    match tail {
        [] if is_get => {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            let include_archived = match request.uri.query() {
                None | Some("include_archived=false") => false,
                Some("include_archived=true") => true,
                Some(_) => return Err(invalid()),
            };
            Ok(Route::OrgProjects(pair, org.to_owned(), include_archived))
        }
        [] if post => {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            Ok(Route::OrgProjectCreate(pair, org.to_owned()))
        }
        [project] if is_get => {
            if query || matches!(*project, "add-team" | "remove-team") {
                return Err(invalid());
            }
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            if project.is_empty() || project.len() > 128 {
                return Err(invalid());
            }
            Ok(Route::OrgProjectInspect(
                pair,
                org.to_owned(),
                (*project).to_owned(),
            ))
        }
        [action] if post => {
            let Some(op) = route_org_action("projects", action) else {
                return Err(not_found());
            };
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            Ok(Route::OrgMutation(pair, org.to_owned(), op))
        }
        [] | ["add-team" | "remove-team"] => Err(invalid()),
        _ => Err(not_found()),
    }
}

fn try_route_organization_subroutes(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    tail: &[&str],
    get: bool,
    post: bool,
    query: bool,
) -> Option<Result<Route, ApiReply>> {
    super::office_routes::try_route(api, request, org, tail, get, post, query)
        .or_else(|| super::provider_routes::try_route(api, request, org, tail, get, post, query))
        .or_else(|| super::hiring_routes::try_route(api, request, org, tail, get, post, query))
        .or_else(|| super::session_routes::try_route(api, request, org, tail, get, post, query))
        .or_else(|| super::git_routes::try_route(api, request, org, tail, get, post, query))
        .or_else(|| super::process_routes::try_route(api, request, org, tail, get, post, query))
        .or_else(|| super::cost_routes::try_route(api, request, org, tail, get, post, query))
        .or_else(|| super::copilot_routes::try_route(api, request, org, tail, get, post, query))
        .or_else(|| super::chat_routes::try_route(api, request, org, tail, get, post, query))
        .or_else(|| super::note_routes::try_route(api, request, org, tail, post, query))
        .or_else(|| super::task_routes::try_route(api, request, org, tail, get, post, query))
        .or_else(|| super::attachment_routes::try_route(api, request, org, tail, get, post, query))
        .or_else(|| super::tess_routes::try_route(api, request, org, tail, get, post, query))
        .or_else(|| {
            super::external_auth_routes::try_route(api, request, org, tail, get, post, query)
        })
        .or_else(|| super::review_routes::try_route(api, request, org, tail, get, post, query))
}

pub(super) fn route_organization(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    tail: &[&str],
    get: bool,
    post: bool,
    query: bool,
) -> Result<Route, ApiReply> {
    if org == "open" {
        return match tail {
            ["preview"] if post => Ok(Route::OpenOrg(super::open_routes::OpenRoute::Preview(
                api.caller_auth(request)?,
            ))),
            [] if post => Ok(Route::OpenOrg(super::open_routes::OpenRoute::Open(
                api.caller_auth(request)?,
            ))),
            [] | ["preview"] => Err(invalid()),
            _ => Err(not_found()),
        };
    }
    if let Some(routed) = super::doc_routes::try_route(api, request, org, tail) {
        return routed;
    }
    if let Some(routed) = super::search_routes::try_route(api, request, org, tail) {
        return routed;
    }
    if let ["projects", rest @ ..] = tail {
        return route_projects(api, request, org, rest, post, query);
    }
    if let Some(routed) =
        try_route_organization_subroutes(api, request, org, tail, get, post, query)
    {
        return routed;
    }
    Ok(match tail {
        ["snapshot"] if get => {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            Route::OrgSnapshot(pair, org.to_owned())
        }
        ["offices"] if get => {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            Route::OrgOffices(pair, org.to_owned())
        }
        ["offices", office, "restart"] if post => {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            valid_org_id(office)?;
            Route::OfficeRestart(pair, org.to_owned(), (*office).to_owned())
        }
        ["access"] if get => {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            Route::Account(super::account_routes::AccountRoute::OrgAccessList(
                pair,
                org.to_owned(),
            ))
        }
        ["access", "invite"] if post => {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            Route::Account(super::account_routes::AccountRoute::OrgInvite(
                pair,
                org.to_owned(),
            ))
        }
        ["access", "reset-link"] if post => {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            Route::Account(super::account_routes::AccountRoute::OrgResetLink(
                pair,
                org.to_owned(),
            ))
        }
        ["access", "remove"] if post => {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            Route::Account(super::account_routes::AccountRoute::OrgRemoveAccess(
                pair,
                org.to_owned(),
            ))
        }
        ["owner", "handover"] if post => {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            Route::Account(super::account_routes::AccountRoute::OrgOwnerHandover(
                pair,
                org.to_owned(),
            ))
        }
        ["teams"] if post => {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            Route::OrgMutation(pair, org.to_owned(), OrgOperation::TeamCreate)
        }
        [resource @ ("teams" | "members"), action] if post => {
            let Some(op) = route_org_action(resource, action) else {
                return Err(not_found());
            };
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            Route::OrgMutation(pair, org.to_owned(), op)
        }
        ["snapshot"]
        | ["offices"]
        | ["offices", _, "restart"]
        | ["access"]
        | ["access", "invite" | "reset-link" | "remove"]
        | ["owner", "handover"]
        | ["teams"]
        | ["teams", "create" | "rename" | "move" | "set-lead" | "delete" | "set-title"]
        | ["members", "add-person" | "update-person" | "remove" | "set-reports-to" | "move"] => {
            return Err(invalid())
        }
        _ => return Err(not_found()),
    })
}

pub(super) fn prepare_project_create(
    api: &SessionApi,
    pair: CallerAuth,
    org: String,
    bytes: &[u8],
) -> Result<PreparedCall<Call>, ApiReply> {
    let raw: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    if !raw.is_object() {
        return Err(invalid());
    }
    let id_str = raw
        .get("operation_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(invalid)?;
    let id = organizations::host_operation_id(id_str).map_err(bad_input)?;
    let request: org_proto::projects::ProjectCreateWebRequest =
        gateway::decode_request(bytes).map_err(|_| invalid())?;
    let target = Scope::Organization {
        instance_id: api.host.instance().to_owned(),
        org_id: org.clone(),
    };
    request.scope.check_target(&target).map_err(|_| invalid())?;
    Ok(PreparedCall::mutation(
        Call::OrgProjectCreate(pair, org, id.clone(), request.params),
        id,
        bytes.len(),
    ))
}

pub(super) fn prepare_office_restart(
    api: &SessionApi,
    pair: CallerAuth,
    org: String,
    office: String,
    bytes: &[u8],
) -> Result<PreparedCall<Call>, ApiReply> {
    let raw: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    let id_str = raw
        .get("operation_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let id = organizations::host_operation_id(id_str).map_err(bad_input)?;
    let request: gateway::OfficeRestartRequest =
        gateway::decode_request(bytes).map_err(|_| invalid())?;
    let target = Scope::Office {
        instance_id: api.host.instance().to_owned(),
        org_id: org.clone(),
        office_id: office.clone(),
    };
    request.scope.check_target(&target).map_err(|_| invalid())?;
    Ok(PreparedCall::mutation(
        Call::OfficeRestart(pair, org, office, id.clone()),
        id,
        bytes.len(),
    ))
}

/// Maps the `(resource, action)` segments of `/api/v1/organizations/{org}/{resource}/{action}`
/// to a closed typed [`OrgOperation`].
pub(super) fn route_org_action(resource: &str, action: &str) -> Option<OrgOperation> {
    const TABLE: &[(&str, &str, OrgOperation)] = &[
        ("teams", "create", OrgOperation::TeamCreate),
        ("teams", "rename", OrgOperation::TeamRename),
        ("teams", "move", OrgOperation::TeamMove),
        ("teams", "set-lead", OrgOperation::TeamSetLead),
        ("teams", "delete", OrgOperation::TeamDelete),
        ("teams", "set-title", OrgOperation::TeamSetTitle),
        ("members", "add-person", OrgOperation::MemberAddPerson),
        ("members", "update-person", OrgOperation::MemberUpdatePerson),
        ("members", "remove", OrgOperation::MemberRemove),
        (
            "members",
            "set-reports-to",
            OrgOperation::MemberSetReportsTo,
        ),
        ("members", "move", OrgOperation::MemberMove),
        ("projects", "add-team", OrgOperation::ProjectAddTeam),
        ("projects", "remove-team", OrgOperation::ProjectRemoveTeam),
    ];
    TABLE
        .iter()
        .find(|(r, a, _)| *r == resource && *a == action)
        .map(|(_, _, op)| *op)
}

fn map_read_office_error(error: OfficeFailure, forbidden_message: &'static str) -> ApiReply {
    match error {
        OfficeFailure::BeforeDispatch(e) => failure(e),
        OfficeFailure::Refused(e) if e.code == org_proto::ErrorCode::NotAuthorized => {
            ApiReply::error(
                StatusCode::FORBIDDEN,
                WebErrorCode::NotAuthorized,
                forbidden_message,
            )
        }
        OfficeFailure::Refused(e) if e.code == org_proto::ErrorCode::NotFound => not_found(),
        OfficeFailure::Refused(e) if e.code == org_proto::ErrorCode::Expired => ApiReply::error(
            StatusCode::GONE,
            WebErrorCode::Expired,
            "This operation is too old to look up.",
        ),
        OfficeFailure::Refused(_) | OfficeFailure::Transport => unavailable(),
    }
}

fn classify_stored_outcome(op_id: OperationId, outcome: org_proto::OperationOutcome) -> ApiReply {
    let synthesized = match outcome {
        org_proto::OperationOutcome::Committed { result } => {
            serde_json::json!({ "id": 1, "result": result })
        }
        org_proto::OperationOutcome::Rejected { refusal } => {
            serde_json::json!({ "id": 1, "error": refusal })
        }
        _ => return unavailable(),
    };
    match classify_org_reply(op_id, Some(&synthesized)) {
        OrgReplyClassification::Outcome { outcome, issue }
            if matches!(
                outcome,
                MutationOutcome::Completed { .. } | MutationOutcome::Rejected { .. }
            ) =>
        {
            if let Some(issue) = issue {
                eprintln!("agentic-org-web: unexpected org lookup outcome issue: {issue:?}");
            }
            result(OperationLookup::Outcome {
                kind: OperationKind::Org,
                state: None,
                outcome,
            })
        }
        _ => unavailable(),
    }
}
