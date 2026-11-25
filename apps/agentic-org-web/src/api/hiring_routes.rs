//! Browser HTTP routes for C2 HR hiring, roles catalog, team member limits, `may_hire`,
//! agent lifecycle, and execution configuration (`TASK-41` / C2).

use hyper::{http::request::Parts, Method, StatusCode};
use org_proto::hiring::{
    AgentExecutionGet, AgentExecutionGetParams, AgentExecutionSet, AgentExecutionSetParams,
    AgentMove, AgentMoveParams, AgentOffboard, AgentOffboardParams, AgentSetStatus,
    AgentSetStatusParams, AgentUpdate, AgentUpdateParams, HrHire, HrHireConfirm,
    HrHireConfirmParams, HrHireDraft, HrHireDraftParams, HrHireInspect, HrHireInspectParams,
    HrHireList, HrHireListParams, HrHireParams, HrHireWithdraw, HrHireWithdrawParams,
    MemberSetMayHire, MemberSetMayHireParams, ModelProfileList, ModelProfileListParams,
    ProviderConnectionCreate, ProviderConnectionCreateParams, RoleCreate, RoleCreateParams,
    RoleInspect, RoleInspectParams, RoleList, RoleListParams, RoleUpdate, RoleUpdateParams,
    TeamSetMaxMembers, TeamSetMaxMembersParams,
};
use org_proto::web::{organizations, ReadResponse, WebError, WebErrorCode};
use serde_json::Value;

use super::org_routes::{resolve_hq_office, revalidate_and_resolve_hq};
use super::{
    bad_input, failure, invalid, not_found, result, unavailable, Call, CallerAuth, Route,
    SessionApi,
};
use crate::host::Failure;
use crate::office::OfficeFailure;
use crate::service::{ApiReply, PreparedCall};

pub enum HiringRoute {
    RoleList(CallerAuth, String),
    RoleInspect(CallerAuth, String, String),
    RoleCreate(CallerAuth, String),
    RoleUpdate(CallerAuth, String),
    HireList(CallerAuth, String),
    HireInspect(CallerAuth, String, String),
    HireDraft(CallerAuth, String),
    HireDirect(CallerAuth, String),
    HireConfirm(CallerAuth, String),
    HireWithdraw(CallerAuth, String),
    TeamSetMaxMembers(CallerAuth, String),
    MemberSetMayHire(CallerAuth, String),
    AgentUpdate(CallerAuth, String),
    AgentMove(CallerAuth, String),
    AgentSetStatus(CallerAuth, String),
    AgentOffboard(CallerAuth, String),
    AgentExecutionGet(CallerAuth, String, String),
    AgentExecutionSet(CallerAuth, String),
    ProviderConnectionCreate(CallerAuth, String),
    ModelProfileList(CallerAuth, String),
}

pub enum HiringCall {
    RoleList(CallerAuth, String),
    RoleInspect(CallerAuth, String, String),
    RoleCreate(CallerAuth, String, RoleCreateParams),
    RoleUpdate(CallerAuth, String, RoleUpdateParams),
    HireList(CallerAuth, String),
    HireInspect(CallerAuth, String, String),
    HireDraft(CallerAuth, String, Box<HrHireDraftParams>),
    HireDirect(CallerAuth, String, Box<HrHireParams>),
    HireConfirm(CallerAuth, String, HrHireConfirmParams),
    HireWithdraw(CallerAuth, String, HrHireWithdrawParams),
    TeamSetMaxMembers(CallerAuth, String, TeamSetMaxMembersParams),
    MemberSetMayHire(CallerAuth, String, MemberSetMayHireParams),
    AgentUpdate(CallerAuth, String, Box<AgentUpdateParams>),
    AgentMove(CallerAuth, String, AgentMoveParams),
    AgentSetStatus(CallerAuth, String, AgentSetStatusParams),
    AgentOffboard(CallerAuth, String, AgentOffboardParams),
    AgentExecutionGet(CallerAuth, String, String),
    AgentExecutionSet(CallerAuth, String, Box<AgentExecutionSetParams>),
    ProviderConnectionCreate(CallerAuth, String, Box<ProviderConnectionCreateParams>),
    ModelProfileList(CallerAuth, String),
}

fn valid_org_id(id: &str) -> Result<(), ApiReply> {
    org_office_launch::is_storage_id(id)
        .then_some(())
        .ok_or_else(invalid)
}

/// Attempts to route C2 paths under `/api/v1/organizations/{org}/...`.
/// Returns `None` if `tail` does not match a C2 resource so `org_routes` continues.
pub(super) fn try_route(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    tail: &[&str],
    _get: bool,
    post: bool,
    query: bool,
) -> Option<Result<Route, ApiReply>> {
    let is_get = matches!(request.method, Method::GET | Method::HEAD) && !query;
    match tail {
        ["roles"] if is_get => Some(auth_org(api, request, org, HiringRoute::RoleList)),
        ["roles"] if post => Some(auth_org(api, request, org, HiringRoute::RoleCreate)),
        ["roles", role] if is_get && *role != "update" => Some(auth_org_role_uuid_arg(
            api,
            request,
            org,
            role,
            HiringRoute::RoleInspect,
        )),
        ["roles", "update"] if post => Some(auth_org(api, request, org, HiringRoute::RoleUpdate)),
        ["hires"] if is_get => Some(auth_org(api, request, org, HiringRoute::HireList)),
        ["hires"] if post => Some(auth_org(api, request, org, HiringRoute::HireDraft)),
        ["hires", "direct"] if post => Some(auth_org(api, request, org, HiringRoute::HireDirect)),
        ["hires", "confirm"] if post => Some(auth_org(api, request, org, HiringRoute::HireConfirm)),
        ["hires", "withdraw"] if post => {
            Some(auth_org(api, request, org, HiringRoute::HireWithdraw))
        }
        ["hires", id] if is_get && !matches!(*id, "direct" | "confirm" | "withdraw") => Some(
            auth_org_uuid_arg(api, request, org, id, HiringRoute::HireInspect),
        ),
        ["teams", "set-max-members"] if post => {
            Some(auth_org(api, request, org, HiringRoute::TeamSetMaxMembers))
        }
        ["members", "set-may-hire"] if post => {
            Some(auth_org(api, request, org, HiringRoute::MemberSetMayHire))
        }
        ["agents", "update"] if post => Some(auth_org(api, request, org, HiringRoute::AgentUpdate)),
        ["agents", "move"] if post => Some(auth_org(api, request, org, HiringRoute::AgentMove)),
        ["agents", "set-status"] if post => {
            Some(auth_org(api, request, org, HiringRoute::AgentSetStatus))
        }
        ["agents", "offboard"] if post => {
            Some(auth_org(api, request, org, HiringRoute::AgentOffboard))
        }
        ["agents", id, "execution"] if is_get => Some(auth_org_uuid_arg(
            api,
            request,
            org,
            id,
            HiringRoute::AgentExecutionGet,
        )),
        ["agents", "execution"] if post => {
            Some(auth_org(api, request, org, HiringRoute::AgentExecutionSet))
        }
        ["provider-connections"] if post => Some(auth_org(
            api,
            request,
            org,
            HiringRoute::ProviderConnectionCreate,
        )),
        ["model-profiles"] if is_get => {
            Some(auth_org(api, request, org, HiringRoute::ModelProfileList))
        }
        ["roles"]
        | ["roles", _]
        | ["hires"]
        | ["hires", _]
        | ["teams", "set-max-members"]
        | ["members", "set-may-hire"]
        | ["agents", "update"]
        | ["agents", "move"]
        | ["agents", "set-status"]
        | ["agents", "offboard"]
        | ["agents", "execution"]
        | ["agents", _, "execution"]
        | ["provider-connections"]
        | ["model-profiles"] => Some(Err(invalid())),
        _ => None,
    }
}

fn auth_org_role_uuid_arg(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    arg: &str,
    build: fn(CallerAuth, String, String) -> HiringRoute,
) -> Result<Route, ApiReply> {
    let caller = api.caller_auth(request)?;
    valid_org_id(org)?;
    if !org_proto::is_uuid_v7(arg) {
        return Err(invalid());
    }
    Ok(Route::Hiring(build(caller, org.to_owned(), arg.to_owned())))
}

fn auth_org_uuid_arg(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    arg: &str,
    build: fn(CallerAuth, String, String) -> HiringRoute,
) -> Result<Route, ApiReply> {
    let caller = api.caller_auth(request)?;
    valid_org_id(org)?;
    if !org_office_launch::is_storage_id(arg) {
        return Err(invalid());
    }
    Ok(Route::Hiring(build(caller, org.to_owned(), arg.to_owned())))
}

fn auth_org(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    build: fn(CallerAuth, String) -> HiringRoute,
) -> Result<Route, ApiReply> {
    let caller = api.caller_auth(request)?;
    valid_org_id(org)?;
    Ok(Route::Hiring(build(caller, org.to_owned())))
}

fn decode_mutation<P: serde::de::DeserializeOwned>(
    bytes: &[u8],
    command_id: impl FnOnce(&P) -> &str,
    wrap: impl FnOnce(P) -> HiringCall,
) -> Result<PreparedCall<Call>, ApiReply> {
    let params: P = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    let id = organizations::host_operation_id(command_id(&params)).map_err(bad_input)?;
    Ok(PreparedCall::mutation(
        Call::Hiring(wrap(params)),
        id,
        bytes.len(),
    ))
}

pub(super) fn prepare(route: HiringRoute, bytes: &[u8]) -> Result<PreparedCall<Call>, ApiReply> {
    let read_call = |call: HiringCall| -> Result<PreparedCall<Call>, ApiReply> {
        if !bytes.is_empty() {
            return Err(invalid());
        }
        Ok(PreparedCall::read(Call::Hiring(call), 0))
    };
    match route {
        HiringRoute::RoleList(c, o) => read_call(HiringCall::RoleList(c, o)),
        HiringRoute::RoleInspect(c, o, r) => read_call(HiringCall::RoleInspect(c, o, r)),
        HiringRoute::HireList(c, o) => read_call(HiringCall::HireList(c, o)),
        HiringRoute::HireInspect(c, o, id) => read_call(HiringCall::HireInspect(c, o, id)),
        HiringRoute::AgentExecutionGet(c, o, id) => {
            read_call(HiringCall::AgentExecutionGet(c, o, id))
        }
        HiringRoute::ModelProfileList(c, o) => read_call(HiringCall::ModelProfileList(c, o)),
        HiringRoute::RoleCreate(c, o) => decode_mutation(
            bytes,
            |p: &RoleCreateParams| &p.command_id,
            |p| HiringCall::RoleCreate(c, o, p),
        ),
        HiringRoute::RoleUpdate(c, o) => decode_mutation(
            bytes,
            |p: &RoleUpdateParams| &p.command_id,
            |p| HiringCall::RoleUpdate(c, o, p),
        ),
        HiringRoute::HireDraft(c, o) => decode_mutation(
            bytes,
            |p: &HrHireDraftParams| &p.command_id,
            |p| HiringCall::HireDraft(c, o, Box::new(p)),
        ),
        HiringRoute::HireDirect(c, o) => decode_mutation(
            bytes,
            |p: &HrHireParams| &p.command_id,
            |p| HiringCall::HireDirect(c, o, Box::new(p)),
        ),
        HiringRoute::HireConfirm(c, o) => decode_mutation(
            bytes,
            |p: &HrHireConfirmParams| &p.command_id,
            |p| HiringCall::HireConfirm(c, o, p),
        ),
        HiringRoute::HireWithdraw(c, o) => decode_mutation(
            bytes,
            |p: &HrHireWithdrawParams| &p.command_id,
            |p| HiringCall::HireWithdraw(c, o, p),
        ),
        HiringRoute::TeamSetMaxMembers(c, o) => decode_mutation(
            bytes,
            |p: &TeamSetMaxMembersParams| &p.command_id,
            |p| HiringCall::TeamSetMaxMembers(c, o, p),
        ),
        HiringRoute::MemberSetMayHire(c, o) => decode_mutation(
            bytes,
            |p: &MemberSetMayHireParams| &p.command_id,
            |p| HiringCall::MemberSetMayHire(c, o, p),
        ),
        HiringRoute::AgentUpdate(c, o) => decode_mutation(
            bytes,
            |p: &AgentUpdateParams| &p.command_id,
            |p| HiringCall::AgentUpdate(c, o, Box::new(p)),
        ),
        HiringRoute::AgentMove(c, o) => decode_mutation(
            bytes,
            |p: &AgentMoveParams| &p.command_id,
            |p| HiringCall::AgentMove(c, o, p),
        ),
        HiringRoute::AgentSetStatus(c, o) => decode_mutation(
            bytes,
            |p: &AgentSetStatusParams| &p.command_id,
            |p| HiringCall::AgentSetStatus(c, o, p),
        ),
        HiringRoute::AgentOffboard(c, o) => decode_mutation(
            bytes,
            |p: &AgentOffboardParams| &p.command_id,
            |p| HiringCall::AgentOffboard(c, o, p),
        ),
        HiringRoute::AgentExecutionSet(c, o) => decode_mutation(
            bytes,
            |p: &AgentExecutionSetParams| &p.command_id,
            |p| HiringCall::AgentExecutionSet(c, o, Box::new(p)),
        ),
        HiringRoute::ProviderConnectionCreate(c, o) => decode_mutation(
            bytes,
            |p: &ProviderConnectionCreateParams| &p.command_id,
            |p| HiringCall::ProviderConnectionCreate(c, o, Box::new(p)),
        ),
    }
}

async fn call_office<C: org_proto::Call>(
    api: &SessionApi,
    pair: CallerAuth,
    org_id: &str,
    params: &C::Params,
    is_read: bool,
) -> Result<C::Result, ApiReply> {
    let mut office_id = match resolve_hq_office(api, pair, org_id).await {
        Ok(id) => id,
        Err(Failure::Refused(_)) => return Err(unavailable()),
        Err(error) => return Err(failure(error)),
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
            Err(Failure::Refused(_)) => return Err(unavailable()),
            Err(error) => return Err(failure(error)),
        };
        reply = api
            .offices
            .call_typed::<C>(&auth, &office_id, params, is_read)
            .await;
    }
    reply.map_err(map_office_failure)
}

async fn run_office<C: org_proto::Call>(
    api: &SessionApi,
    pair: CallerAuth,
    org_id: &str,
    params: &C::Params,
    is_read: bool,
) -> ApiReply {
    match call_office::<C>(api, pair, org_id, params, is_read).await {
        Ok(res) => result(res),
        Err(reply) => reply,
    }
}

fn map_office_failure(error: OfficeFailure) -> ApiReply {
    match error {
        OfficeFailure::BeforeDispatch(e) => failure(e),
        OfficeFailure::Transport => unavailable(),
        OfficeFailure::Refused(err) => {
            let web = WebError::from(err);
            let status = match web.code {
                WebErrorCode::InvalidRequest | WebErrorCode::InvalidName | WebErrorCode::Cycle => {
                    StatusCode::BAD_REQUEST
                }
                WebErrorCode::NotAuthorized | WebErrorCode::OwnerProtected => StatusCode::FORBIDDEN,
                WebErrorCode::NotFound => return not_found(),
                WebErrorCode::Conflict
                | WebErrorCode::AlreadyExists
                | WebErrorCode::NotEmpty
                | WebErrorCode::OperationReused => StatusCode::CONFLICT,
                WebErrorCode::Expired => StatusCode::GONE,
                _ => return unavailable(),
            };
            let payload = ReadResponse::<Value>::Error(web);
            ApiReply::json(
                status,
                serde_json::to_value(payload).expect("gateway error serializes"),
            )
        }
    }
}

pub(super) async fn execute(api: &SessionApi, call: HiringCall) -> ApiReply {
    match call {
        HiringCall::RoleList(c, o) => {
            run_office::<RoleList>(api, c, &o, &RoleListParams {}, true).await
        }
        HiringCall::RoleInspect(c, o, role) => {
            let expected_id = role.clone();
            match call_office::<RoleInspect>(api, c, &o, &RoleInspectParams { role }, true).await {
                Ok(record) if record.id == expected_id => result(record),
                Ok(_) => not_found(),
                Err(reply) => reply,
            }
        }
        HiringCall::RoleCreate(c, o, p) => run_office::<RoleCreate>(api, c, &o, &p, false).await,
        HiringCall::RoleUpdate(c, o, p) => run_office::<RoleUpdate>(api, c, &o, &p, false).await,
        HiringCall::HireList(c, o) => {
            run_office::<HrHireList>(api, c, &o, &HrHireListParams::default(), true).await
        }
        HiringCall::HireInspect(c, o, request_id) => {
            run_office::<HrHireInspect>(api, c, &o, &HrHireInspectParams { request_id }, true).await
        }
        HiringCall::HireDraft(c, o, p) => run_office::<HrHireDraft>(api, c, &o, &p, false).await,
        HiringCall::HireDirect(c, o, p) => run_office::<HrHire>(api, c, &o, &p, false).await,
        HiringCall::HireConfirm(c, o, p) => {
            run_office::<HrHireConfirm>(api, c, &o, &p, false).await
        }
        HiringCall::HireWithdraw(c, o, p) => {
            run_office::<HrHireWithdraw>(api, c, &o, &p, false).await
        }
        HiringCall::TeamSetMaxMembers(c, o, p) => {
            run_office::<TeamSetMaxMembers>(api, c, &o, &p, false).await
        }
        HiringCall::MemberSetMayHire(c, o, p) => {
            run_office::<MemberSetMayHire>(api, c, &o, &p, false).await
        }
        HiringCall::AgentUpdate(c, o, p) => run_office::<AgentUpdate>(api, c, &o, &p, false).await,
        HiringCall::AgentMove(c, o, p) => run_office::<AgentMove>(api, c, &o, &p, false).await,
        HiringCall::AgentSetStatus(c, o, p) => {
            run_office::<AgentSetStatus>(api, c, &o, &p, false).await
        }
        HiringCall::AgentOffboard(c, o, p) => {
            run_office::<AgentOffboard>(api, c, &o, &p, false).await
        }
        HiringCall::AgentExecutionGet(c, o, employee_id) => {
            run_office::<AgentExecutionGet>(
                api,
                c,
                &o,
                &AgentExecutionGetParams { employee_id },
                true,
            )
            .await
        }
        HiringCall::AgentExecutionSet(c, o, p) => {
            run_office::<AgentExecutionSet>(api, c, &o, &p, false).await
        }
        HiringCall::ProviderConnectionCreate(c, o, p) => {
            run_office::<ProviderConnectionCreate>(api, c, &o, &p, false).await
        }
        HiringCall::ModelProfileList(c, o) => {
            run_office::<ModelProfileList>(api, c, &o, &ModelProfileListParams {}, true).await
        }
    }
}
