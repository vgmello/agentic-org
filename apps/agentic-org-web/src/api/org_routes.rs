//! Organization snapshot, mutation, and operation-lookup handlers for [`SessionApi`].
//! Extracted from `api.rs` to keep the session/host API surface decoupled from
//! browser-to-office organization routing and `web-api-v1` §9 authorization rechecks.

use hyper::{http::request::Parts, Method, StatusCode};
use org_host_control::{
    browser::{BrowserOfficeRestart, BrowserOfficeRestartParams},
    organization::{self as host_org, BrowserAuth},
    HostOpResult,
};
use org_proto::web::{
    self as gateway,
    org_mutations::{classify_org_reply, OrgMutation, OrgOperation, OrgReplyClassification},
    organizations, MutationOutcome, OfficeList, OfficeRestartResult, OfficeView, OperationId,
    OperationKind, OperationLookup, ResumeCursor, Scope, SnapshotResponse, WebError, WebErrorCode,
};

use super::{
    bad_input, failure, invalid, mutation, not_found, result, signed_out, unavailable, Call,
    PresentedPair, Route, SessionApi,
};
use crate::{
    host::Failure,
    office::OfficeFailure,
    service::{ApiReply, PreparedCall},
    sse::HubEvent,
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
            let pair = api.pair(request)?;
            valid_org_id(org)?;
            let include_archived = match request.uri.query() {
                None | Some("include_archived=false") => false,
                Some("include_archived=true") => true,
                Some(_) => return Err(invalid()),
            };
            Ok(Route::OrgProjects(pair, org.to_owned(), include_archived))
        }
        [] if post => {
            let pair = api.pair(request)?;
            valid_org_id(org)?;
            Ok(Route::OrgProjectCreate(pair, org.to_owned()))
        }
        [project] if is_get => {
            if query || matches!(*project, "add-team" | "remove-team") {
                return Err(invalid());
            }
            let pair = api.pair(request)?;
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
            let pair = api.pair(request)?;
            valid_org_id(org)?;
            Ok(Route::OrgMutation(pair, org.to_owned(), op))
        }
        [] | ["add-team" | "remove-team"] => Err(invalid()),
        _ => Err(not_found()),
    }
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
    if let ["projects", rest @ ..] = tail {
        return route_projects(api, request, org, rest, post, query);
    }
    Ok(match tail {
        ["snapshot"] if get => {
            let pair = api.pair(request)?;
            valid_org_id(org)?;
            Route::OrgSnapshot(pair, org.to_owned())
        }
        ["offices"] if get => {
            let pair = api.pair(request)?;
            valid_org_id(org)?;
            Route::OrgOffices(pair, org.to_owned())
        }
        ["offices", office, "restart"] if post => {
            let pair = api.pair(request)?;
            valid_org_id(org)?;
            valid_org_id(office)?;
            Route::OfficeRestart(pair, org.to_owned(), (*office).to_owned())
        }
        ["teams"] if post => {
            let pair = api.pair(request)?;
            valid_org_id(org)?;
            Route::OrgMutation(pair, org.to_owned(), OrgOperation::TeamCreate)
        }
        [resource @ ("teams" | "members"), action] if post => {
            let Some(op) = route_org_action(resource, action) else {
                return Err(not_found());
            };
            let pair = api.pair(request)?;
            valid_org_id(org)?;
            Route::OrgMutation(pair, org.to_owned(), op)
        }
        ["snapshot"]
        | ["offices"]
        | ["offices", _, "restart"]
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
    pair: PresentedPair,
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
    pair: PresentedPair,
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

fn find_hq_office(list: host_org::ListResult, org_id: &str) -> Result<String, Failure> {
    let org = list
        .organizations
        .into_iter()
        .find(|org| org.id == org_id)
        .ok_or(Failure::NotFound)?;
    org.offices
        .iter()
        .find(|office| office.role == org_proto::OfficeRole::Hq)
        .or_else(|| org.offices.first())
        .map(|office| office.id.clone())
        .ok_or(Failure::Unavailable)
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

pub(super) async fn execute_org_snapshot(
    api: &SessionApi,
    pair: PresentedPair,
    org_id: &str,
) -> ApiReply {
    let mut office_id = match resolve_hq_office(api, pair, org_id).await {
        Ok(id) => id,
        Err(Failure::Refused(_)) => return unavailable(),
        Err(error) => return failure(error),
    };
    let auth = api.auth(&pair);
    let mut reply = api.offices.org_snapshot(&auth, &office_id).await;
    if matches!(
        &reply,
        Err(OfficeFailure::Refused(e)) if e.code == org_proto::ErrorCode::NotAuthorized
    ) {
        office_id = match revalidate_and_resolve_hq(api, pair, org_id).await {
            Ok(id) => id,
            Err(Failure::Refused(_)) => return unavailable(),
            Err(error) => return failure(error),
        };
        reply = api.offices.org_snapshot(&auth, &office_id).await;
    }
    let snapshot = match reply {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return map_read_office_error(
                error,
                "You are not authorized to view this organization.",
            );
        }
    };
    let cursor = format!(
        "org:{}:{}:{}",
        org_id, snapshot.position.epoch, snapshot.position.seq
    );
    match ResumeCursor::parse(cursor) {
        Ok(resume_cursor) => result(SnapshotResponse {
            snapshot,
            resume_cursor,
        }),
        Err(_) => unavailable(),
    }
}

pub(super) async fn execute_org_mutation(
    api: &SessionApi,
    pair: PresentedPair,
    org_id: &str,
    command: OrgMutation,
) -> ApiReply {
    let op_id = command.operation_id().clone();
    let office_id = match resolve_hq_office(api, pair, org_id).await {
        Ok(id) => id,
        Err(error) => return mutation_setup_failure(error, op_id),
    };
    let auth = api.auth(&pair);
    let mut classified = match dispatch_org_mutation(api, &auth, &office_id, &command, &op_id).await
    {
        Ok(c) => c,
        Err(reply) => return reply,
    };
    if matches!(
        classified,
        OrgReplyClassification::NeedsAuthorizationRecheck
    ) {
        let fresh_office = match revalidate_and_resolve_hq(api, pair, org_id).await {
            Ok(id) => id,
            Err(error) => return mutation_setup_failure(error, op_id),
        };
        classified = match dispatch_org_mutation(api, &auth, &fresh_office, &command, &op_id).await
        {
            Ok(c) => c,
            Err(reply) => return reply,
        };
    }
    finish_org_mutation(api, org_id, &command, op_id, classified)
}

pub(super) async fn execute_projects_list(
    api: &SessionApi,
    pair: PresentedPair,
    org_id: &str,
    include_archived: bool,
) -> ApiReply {
    let mut office_id = match resolve_hq_office(api, pair, org_id).await {
        Ok(id) => id,
        Err(Failure::Refused(_)) => return unavailable(),
        Err(error) => return failure(error),
    };
    let auth = api.auth(&pair);
    let mut reply = api
        .offices
        .projects_list(&auth, &office_id, include_archived)
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
            .projects_list(&auth, &office_id, include_archived)
            .await;
    }
    match reply {
        Ok(listed) => result(listed),
        Err(error) => map_read_office_error(
            error,
            "You are not authorized to view projects in this organization.",
        ),
    }
}

pub(super) async fn execute_project_inspect(
    api: &SessionApi,
    pair: PresentedPair,
    org_id: &str,
    project_ref: &str,
) -> ApiReply {
    let mut office_id = match resolve_hq_office(api, pair, org_id).await {
        Ok(id) => id,
        Err(Failure::Refused(_)) => return unavailable(),
        Err(error) => return failure(error),
    };
    let auth = api.auth(&pair);
    let mut reply = api
        .offices
        .project_inspect(&auth, &office_id, project_ref)
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
            .project_inspect(&auth, &office_id, project_ref)
            .await;
    }
    match reply {
        Ok(project) => result(project),
        Err(error) => map_read_office_error(error, "You are not authorized to view this project."),
    }
}

pub(super) async fn execute_project_create(
    api: &SessionApi,
    pair: PresentedPair,
    org_id: &str,
    op_id: OperationId,
    params: org_proto::projects::ProjectCreateWebParams,
) -> ApiReply {
    let office_id = match resolve_hq_office(api, pair, org_id).await {
        Ok(id) => id,
        Err(error) => return mutation_setup_failure(error, op_id),
    };
    let auth = api.auth(&pair);
    let native = org_proto::org::ProjectCreateParams {
        project_id: params.project_id,
        name: params.name,
        key: None,
        description: None,
        teams: Vec::new(),
        general_area_id: String::new(),
        general_area_name: None,
        repo: None,
        integration_branch: None,
        slug: params.slug,
        owner_team_id: params.owner_team_id,
        create_team_id: params.create_team_id,
        parent_team_id: params.parent_team_id,
        contributing_team_ids: params.contributing_team_ids,
        command_id: op_id.as_str().to_owned(),
    };
    let mut classified =
        match dispatch_project_create(api, &auth, &office_id, &native, &op_id).await {
            Ok(c) => c,
            Err(reply) => return reply,
        };
    if matches!(
        classified,
        OrgReplyClassification::NeedsAuthorizationRecheck
    ) {
        let fresh_office = match revalidate_and_resolve_hq(api, pair, org_id).await {
            Ok(id) => id,
            Err(error) => return mutation_setup_failure(error, op_id),
        };
        classified = match dispatch_project_create(api, &auth, &fresh_office, &native, &op_id).await
        {
            Ok(c) => c,
            Err(reply) => return reply,
        };
    }
    finish_named_org_mutation(api, org_id, "project.create", op_id, classified)
}

async fn dispatch_project_create(
    api: &SessionApi,
    auth: &BrowserAuth,
    office_id: &str,
    params: &org_proto::org::ProjectCreateParams,
    op_id: &OperationId,
) -> Result<OrgReplyClassification, ApiReply> {
    match api.offices.project_create(auth, office_id, params).await {
        Ok(reply) => Ok(classify_org_reply(op_id.clone(), reply.as_ref())),
        Err(OfficeFailure::BeforeDispatch(error)) => {
            Err(mutation_setup_failure(error, op_id.clone()))
        }
        Err(_) => Err(mutation(
            MutationOutcome::<org_proto::Empty>::Rejected {
                operation_id: op_id.clone(),
                error: WebError::new(WebErrorCode::Unavailable, "Service unavailable."),
            },
            false,
        )),
    }
}

pub(super) async fn execute_org_operation(
    api: &SessionApi,
    pair: PresentedPair,
    org_id: &str,
    op_id: OperationId,
) -> ApiReply {
    let mut office_id = match resolve_hq_office(api, pair, org_id).await {
        Ok(id) => id,
        Err(Failure::Refused(_)) => return unavailable(),
        Err(error) => return failure(error),
    };
    let auth = api.auth(&pair);
    let mut reply = api
        .offices
        .org_operation_get(&auth, &office_id, op_id.as_str())
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
            .org_operation_get(&auth, &office_id, op_id.as_str())
            .await;
    }
    match reply {
        Ok(outcome) => classify_stored_outcome(op_id, outcome),
        Err(error) => {
            map_read_office_error(error, "You are not authorized to view this operation.")
        }
    }
}

async fn resolve_hq_office(
    api: &SessionApi,
    pair: PresentedPair,
    org_id: &str,
) -> Result<String, Failure> {
    api.validate_async(pair).await?;
    let list = api
        .host
        .request_async::<host_org::List>(&host_org::ListParams {
            auth: api.auth(&pair),
        })
        .await?;
    find_hq_office(list, org_id)
}

async fn revalidate_and_resolve_hq(
    api: &SessionApi,
    pair: PresentedPair,
    org_id: &str,
) -> Result<String, Failure> {
    api.revalidate_session_async(pair).await?;
    let list = api
        .host
        .request_async::<host_org::List>(&host_org::ListParams {
            auth: api.auth(&pair),
        })
        .await?;
    find_hq_office(list, org_id)
}

fn mutation_setup_failure(error: Failure, op_id: OperationId) -> ApiReply {
    match error {
        Failure::Denied => signed_out(),
        Failure::NotFound => not_found(),
        _ => mutation(
            MutationOutcome::<org_proto::Empty>::Rejected {
                operation_id: op_id,
                error: WebError::new(WebErrorCode::Unavailable, "Service unavailable."),
            },
            false,
        ),
    }
}

async fn dispatch_org_mutation(
    api: &SessionApi,
    auth: &BrowserAuth,
    office_id: &str,
    command: &OrgMutation,
    op_id: &OperationId,
) -> Result<OrgReplyClassification, ApiReply> {
    match api.offices.org_mutation(auth, office_id, command).await {
        Ok(reply) => Ok(classify_org_reply(op_id.clone(), reply.as_ref())),
        Err(OfficeFailure::BeforeDispatch(error)) => {
            Err(mutation_setup_failure(error, op_id.clone()))
        }
        Err(_) => Err(mutation(
            MutationOutcome::<org_proto::Empty>::Rejected {
                operation_id: op_id.clone(),
                error: WebError::new(WebErrorCode::Unavailable, "Service unavailable."),
            },
            false,
        )),
    }
}

fn finish_org_mutation(
    api: &SessionApi,
    org_id: &str,
    command: &OrgMutation,
    op_id: OperationId,
    classified: OrgReplyClassification,
) -> ApiReply {
    finish_named_org_mutation(api, org_id, command.operation().name(), op_id, classified)
}

fn finish_named_org_mutation(
    api: &SessionApi,
    org_id: &str,
    event_name: &str,
    op_id: OperationId,
    classified: OrgReplyClassification,
) -> ApiReply {
    match classified {
        OrgReplyClassification::Outcome { outcome, issue } => {
            if let Some(issue) = issue {
                eprintln!("agentic-org-web: unexpected org mutation reply issue: {issue:?}");
            }
            if matches!(outcome, MutationOutcome::Completed { .. }) {
                api.hub.publish(HubEvent::OrganizationChanged {
                    org_id: org_id.to_owned(),
                    cursor: format!("org:{org_id}:{}", op_id.as_str()),
                    event: event_name.to_owned(),
                    data: serde_json::json!({ "operation_id": op_id.as_str() }),
                });
            }
            mutation(outcome, false)
        }
        OrgReplyClassification::NeedsAuthorizationRecheck => mutation(
            MutationOutcome::<org_proto::Empty>::Rejected {
                operation_id: op_id,
                error: WebError::new(
                    WebErrorCode::NotAuthorized,
                    "You are not authorized to perform this action.",
                ),
            },
            false,
        ),
    }
}

pub(super) async fn execute_org_offices(
    api: &SessionApi,
    pair: PresentedPair,
    org_id: &str,
) -> ApiReply {
    if let Err(error) = api.validate_async(pair).await {
        return failure(error);
    }
    let list = match api
        .host
        .request_async::<host_org::List>(&host_org::ListParams {
            auth: api.auth(&pair),
        })
        .await
    {
        Ok(list) => list,
        Err(Failure::Refused(_)) => return unavailable(),
        Err(error) => return failure(error),
    };
    let Some(org) = list.organizations.into_iter().find(|org| org.id == org_id) else {
        return not_found();
    };
    let offices = org.offices.into_iter().map(OfficeView::from).collect();
    result(OfficeList { offices })
}

fn office_restart_error(host: WebError) -> WebError {
    use WebErrorCode::*;
    let message = match host.code {
        InvalidRequest => "Invalid office restart request.",
        NotFound => "Office not found.",
        NotAuthorized => "Only the administrator or organization owner may restart an office.",
        Conflict => "This office is not offline or unavailable.",
        OperationReused => "This operation ID was already used for another request.",
        ClockSkew => "This device's clock differs too much from the server's.",
        NotReady => "This office is still starting or being created.",
        _ => return WebError::new(Unavailable, "Service unavailable."),
    };
    let mut error = WebError::new(host.code, message);
    (error.field, error.rule) = (host.field, host.rule);
    error
}

pub(super) async fn execute_office_restart(
    api: &SessionApi,
    pair: PresentedPair,
    org_id: &str,
    office_id: &str,
    op_id: OperationId,
) -> ApiReply {
    if let Err(error) = api.validate_async(pair).await {
        return failure(error);
    }
    let reply = api
        .host
        .request_async::<BrowserOfficeRestart>(&BrowserOfficeRestartParams {
            auth: api.auth(&pair),
            operation_id: op_id.as_str().to_owned(),
            org_id: org_id.to_owned(),
            office_id: office_id.to_owned(),
        })
        .await;
    if matches!(&reply, Err(Failure::Refused(e)) if e.code == org_proto::ErrorCode::Expired) {
        return ApiReply::error(
            StatusCode::GONE,
            WebErrorCode::Expired,
            "This request expired. Start again.",
        );
    }
    let outcome = match reply {
        Ok(HostOpResult::Completed { .. }) => MutationOutcome::Completed {
            operation_id: op_id,
            result: OfficeRestartResult {},
        },
        Ok(HostOpResult::Rejected { refusal }) => MutationOutcome::Rejected {
            operation_id: op_id,
            error: office_restart_error(WebError::from(refusal)),
        },
        Ok(_) => MutationOutcome::Unknown {
            operation_id: op_id,
        },
        Err(Failure::Denied) => {
            if let Err(error) = api.revalidate_session_async(pair).await {
                return failure(error);
            }
            MutationOutcome::Rejected {
                operation_id: op_id,
                error: office_restart_error(WebError::new(WebErrorCode::NotAuthorized, "")),
            }
        }
        Err(Failure::NotFound) => MutationOutcome::Rejected {
            operation_id: op_id,
            error: office_restart_error(WebError::new(WebErrorCode::NotFound, "")),
        },
        Err(Failure::Refused(error)) => {
            let error = WebError::from(*error);
            if matches!(
                error.code,
                WebErrorCode::InvalidRequest
                    | WebErrorCode::Conflict
                    | WebErrorCode::OperationReused
                    | WebErrorCode::ClockSkew
                    | WebErrorCode::NotReady
            ) {
                MutationOutcome::Rejected {
                    operation_id: op_id,
                    error: office_restart_error(error),
                }
            } else {
                MutationOutcome::Unknown {
                    operation_id: op_id,
                }
            }
        }
        Err(_) => MutationOutcome::Unknown {
            operation_id: op_id,
        },
    };
    mutation(outcome, false)
}
