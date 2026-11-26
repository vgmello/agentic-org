//! Browser HTTP routes for `L4` provider connection pools, office-local bindings,
//! per-connection model discovery, readiness, and quota/cooldown status
//! (`TASK-54`, `FC-098`, `FC-099`, `doc-88`, `doc-89`).
//!
//! All mutations use `POST` (`agentic-org-web` ingress policy).

use hyper::{http::request::Parts, StatusCode};
use org_proto::providers::{
    AgentReadinessGet, AgentReadinessGetParams, ModelProfileCreate, ModelProfileCreateParams,
    ModelProfileModels, ModelProfileModelsParams, ModelProfileUpdate, ModelProfileUpdateParams,
    ProviderBindingList, ProviderBindingListParams, ProviderBindingSet, ProviderBindingSetParams,
    ProviderConnectionList, ProviderConnectionListParams, ProviderConnectionRefresh,
    ProviderConnectionRefreshParams, ProviderConnectionUpdate, ProviderConnectionUpdateParams,
    ProviderCooldownRecord, ProviderCooldownRecordParams, ProviderQuotaStatus,
    ProviderQuotaStatusParams,
};
use org_proto::web::{ReadResponse, WebError, WebErrorCode};
use serde_json::Value;

use super::hq_context::{resolve_hq_office, revalidate_and_resolve_hq};
use super::{failure, invalid, result, unavailable, Call, CallerAuth, Route, SessionApi};
use crate::host::Failure;
use crate::office::OfficeFailure;
use crate::service::{ApiReply, PreparedCall};

pub enum ProviderRoute {
    ConnectionList(CallerAuth, String, ProviderConnectionListParams),
    ConnectionUpdate(CallerAuth, String),
    BindingList(CallerAuth, String, ProviderBindingListParams),
    BindingSet(CallerAuth, String),
    ConnectionRefresh(CallerAuth, String),
    QuotaStatus(CallerAuth, String, ProviderQuotaStatusParams),
    CooldownRecord(CallerAuth, String),
    ProfileCreate(CallerAuth, String),
    ProfileUpdate(CallerAuth, String),
    ProfileModels(CallerAuth, String, String),
    AgentReadiness(CallerAuth, String, AgentReadinessGetParams),
}

pub enum ProviderCall {
    ConnectionList(CallerAuth, String, ProviderConnectionListParams),
    ConnectionUpdate(CallerAuth, String, ProviderConnectionUpdateParams),
    BindingList(CallerAuth, String, ProviderBindingListParams),
    BindingSet(CallerAuth, String, ProviderBindingSetParams),
    ConnectionRefresh(CallerAuth, String, ProviderConnectionRefreshParams),
    QuotaStatus(CallerAuth, String, ProviderQuotaStatusParams),
    CooldownRecord(CallerAuth, String, ProviderCooldownRecordParams),
    ProfileCreate(CallerAuth, String, ModelProfileCreateParams),
    ProfileUpdate(CallerAuth, String, ModelProfileUpdateParams),
    ProfileModels(CallerAuth, String, ModelProfileModelsParams),
    AgentReadiness(CallerAuth, String, AgentReadinessGetParams),
}

fn valid_id(id: &str) -> Result<(), ApiReply> {
    org_office_launch::is_storage_id(id)
        .then_some(())
        .ok_or_else(invalid)
}

fn parse_query_pairs(query: Option<&str>) -> Result<Vec<(String, String)>, ApiReply> {
    let Some(q) = query.filter(|s| !s.is_empty()) else {
        return Ok(Vec::new());
    };
    let mut pairs = Vec::new();
    for part in q.split('&') {
        let Some((k, v)) = part.split_once('=') else {
            return Err(invalid());
        };
        if pairs.iter().any(|(ek, _)| ek == k) {
            return Err(invalid());
        }
        pairs.push((k.to_owned(), v.to_owned()));
    }
    Ok(pairs)
}

fn parse_connection_list_query(
    query: Option<&str>,
) -> Result<ProviderConnectionListParams, ApiReply> {
    let mut out = ProviderConnectionListParams::default();
    for (k, v) in parse_query_pairs(query)? {
        match (k.as_str(), v.as_str()) {
            ("include_archived", "true") => out.include_archived = true,
            ("include_archived", "false") => out.include_archived = false,
            _ => return Err(invalid()),
        }
    }
    Ok(out)
}

fn parse_optional_connection_id(query: Option<&str>) -> Result<Option<String>, ApiReply> {
    let mut conn_id = None;
    for (k, v) in parse_query_pairs(query)? {
        match k.as_str() {
            "connection_id" if !v.is_empty() && v.len() <= 128 => conn_id = Some(v),
            _ => return Err(invalid()),
        }
    }
    Ok(conn_id)
}

fn parse_optional_task_id(query: Option<&str>) -> Result<Option<String>, ApiReply> {
    let mut task_id = None;
    for (k, v) in parse_query_pairs(query)? {
        match k.as_str() {
            "task_id" if !v.is_empty() && v.len() <= 128 => task_id = Some(v),
            _ => return Err(invalid()),
        }
    }
    Ok(task_id)
}

/// Attempts to route `L4` provider-pool paths under `/api/v1/organizations/{org}/...`.
/// Positioned BEFORE `hiring_routes` so `GET /provider-connections` and `POST /model-profiles`
/// match here while `POST /provider-connections` and `GET /model-profiles` continue to `hiring_routes`.
pub(super) fn try_route(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    tail: &[&str],
    _get: bool,
    post: bool,
    query: bool,
) -> Option<Result<Route, ApiReply>> {
    let is_get = matches!(request.method, hyper::Method::GET | hyper::Method::HEAD);
    match tail {
        ["provider-connections"] if is_get => Some(route_connection_list(api, request, org)),
        ["provider-connections", "update"] if post && !query => Some(route_post(
            api,
            request,
            org,
            ProviderRoute::ConnectionUpdate,
        )),
        ["provider-connections", "bindings"] if is_get => {
            Some(route_binding_list(api, request, org))
        }
        ["provider-connections", "bindings"] if post && !query => {
            Some(route_post(api, request, org, ProviderRoute::BindingSet))
        }
        ["provider-connections", "refresh"] if post && !query => Some(route_post(
            api,
            request,
            org,
            ProviderRoute::ConnectionRefresh,
        )),
        ["provider-connections", "quota"] if is_get => Some(route_quota_status(api, request, org)),
        ["provider-connections", "cooldown"] if post && !query => {
            Some(route_post(api, request, org, ProviderRoute::CooldownRecord))
        }
        ["model-profiles"] if post && !query => {
            Some(route_post(api, request, org, ProviderRoute::ProfileCreate))
        }
        ["model-profiles", "update"] if post && !query => {
            Some(route_post(api, request, org, ProviderRoute::ProfileUpdate))
        }
        ["model-profiles", id, "models"] if is_get && !query => {
            Some(route_profile_models(api, request, org, id))
        }
        ["agents", id, "readiness"] if is_get => Some(route_agent_readiness(api, request, org, id)),
        ["provider-connections", "update" | "bindings" | "refresh" | "quota" | "cooldown"]
        | ["model-profiles", "update"]
        | ["model-profiles", _, "models"]
        | ["agents", _, "readiness"] => Some(Err(invalid())),
        _ => None,
    }
}

fn route_connection_list(api: &SessionApi, request: &Parts, org: &str) -> Result<Route, ApiReply> {
    let pair = api.caller_auth(request)?;
    valid_id(org)?;
    let params = parse_connection_list_query(request.uri.query())?;
    Ok(Route::Provider(ProviderRoute::ConnectionList(
        pair,
        org.to_owned(),
        params,
    )))
}

fn route_binding_list(api: &SessionApi, request: &Parts, org: &str) -> Result<Route, ApiReply> {
    let pair = api.caller_auth(request)?;
    valid_id(org)?;
    let connection_id = parse_optional_connection_id(request.uri.query())?;
    Ok(Route::Provider(ProviderRoute::BindingList(
        pair,
        org.to_owned(),
        ProviderBindingListParams { connection_id },
    )))
}

fn route_quota_status(api: &SessionApi, request: &Parts, org: &str) -> Result<Route, ApiReply> {
    let pair = api.caller_auth(request)?;
    valid_id(org)?;
    let connection_id = parse_optional_connection_id(request.uri.query())?;
    Ok(Route::Provider(ProviderRoute::QuotaStatus(
        pair,
        org.to_owned(),
        ProviderQuotaStatusParams { connection_id },
    )))
}

fn route_profile_models(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    profile_id: &str,
) -> Result<Route, ApiReply> {
    let pair = api.caller_auth(request)?;
    valid_id(org)?;
    if profile_id.is_empty() || profile_id.len() > 128 {
        return Err(invalid());
    }
    Ok(Route::Provider(ProviderRoute::ProfileModels(
        pair,
        org.to_owned(),
        profile_id.to_owned(),
    )))
}

fn route_agent_readiness(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    employee_id: &str,
) -> Result<Route, ApiReply> {
    let pair = api.caller_auth(request)?;
    valid_id(org)?;
    valid_id(employee_id)?;
    let task_id = parse_optional_task_id(request.uri.query())?;
    Ok(Route::Provider(ProviderRoute::AgentReadiness(
        pair,
        org.to_owned(),
        AgentReadinessGetParams {
            employee_id: employee_id.to_owned(),
            task_id,
        },
    )))
}

fn route_post(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    ctor: fn(CallerAuth, String) -> ProviderRoute,
) -> Result<Route, ApiReply> {
    let pair = api.caller_auth(request)?;
    valid_id(org)?;
    Ok(Route::Provider(ctor(pair, org.to_owned())))
}

fn decode_body<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, ApiReply> {
    serde_json::from_slice(bytes).map_err(|_| invalid())
}

pub(super) fn prepare(route: ProviderRoute, bytes: &[u8]) -> Result<PreparedCall<Call>, ApiReply> {
    let call = match route {
        ProviderRoute::ConnectionList(pair, org, params) => {
            if !bytes.is_empty() {
                return Err(invalid());
            }
            ProviderCall::ConnectionList(pair, org, params)
        }
        ProviderRoute::ConnectionUpdate(pair, org) => {
            ProviderCall::ConnectionUpdate(pair, org, decode_body(bytes)?)
        }
        ProviderRoute::BindingList(pair, org, params) => {
            if !bytes.is_empty() {
                return Err(invalid());
            }
            ProviderCall::BindingList(pair, org, params)
        }
        ProviderRoute::BindingSet(pair, org) => {
            ProviderCall::BindingSet(pair, org, decode_body(bytes)?)
        }
        ProviderRoute::ConnectionRefresh(pair, org) => {
            ProviderCall::ConnectionRefresh(pair, org, decode_body(bytes)?)
        }
        ProviderRoute::QuotaStatus(pair, org, params) => {
            if !bytes.is_empty() {
                return Err(invalid());
            }
            ProviderCall::QuotaStatus(pair, org, params)
        }
        ProviderRoute::CooldownRecord(pair, org) => {
            ProviderCall::CooldownRecord(pair, org, decode_body(bytes)?)
        }
        ProviderRoute::ProfileCreate(pair, org) => {
            ProviderCall::ProfileCreate(pair, org, decode_body(bytes)?)
        }
        ProviderRoute::ProfileUpdate(pair, org) => {
            ProviderCall::ProfileUpdate(pair, org, decode_body(bytes)?)
        }
        ProviderRoute::ProfileModels(pair, org, model_profile_id) => {
            if !bytes.is_empty() {
                return Err(invalid());
            }
            ProviderCall::ProfileModels(pair, org, ModelProfileModelsParams { model_profile_id })
        }
        ProviderRoute::AgentReadiness(pair, org, params) => {
            if !bytes.is_empty() {
                return Err(invalid());
            }
            ProviderCall::AgentReadiness(pair, org, params)
        }
    };
    Ok(PreparedCall::read(Call::Provider(call), bytes.len()))
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
                WebErrorCode::NotFound => StatusCode::NOT_FOUND,
                WebErrorCode::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
                WebErrorCode::Conflict
                | WebErrorCode::AlreadyExists
                | WebErrorCode::NotEmpty
                | WebErrorCode::OperationReused => StatusCode::CONFLICT,
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

async fn run_office<C: org_proto::Call>(
    api: &SessionApi,
    pair: CallerAuth,
    org_id: &str,
    params: &C::Params,
    is_read: bool,
) -> ApiReply {
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

pub(super) async fn execute(api: &SessionApi, call: ProviderCall) -> ApiReply {
    match call {
        ProviderCall::ConnectionList(pair, org, params) => {
            run_office::<ProviderConnectionList>(api, pair, &org, &params, true).await
        }
        ProviderCall::ConnectionUpdate(pair, org, params) => {
            run_office::<ProviderConnectionUpdate>(api, pair, &org, &params, false).await
        }
        ProviderCall::BindingList(pair, org, params) => {
            run_office::<ProviderBindingList>(api, pair, &org, &params, true).await
        }
        ProviderCall::BindingSet(pair, org, params) => {
            run_office::<ProviderBindingSet>(api, pair, &org, &params, false).await
        }
        ProviderCall::ConnectionRefresh(pair, org, params) => {
            run_office::<ProviderConnectionRefresh>(api, pair, &org, &params, false).await
        }
        ProviderCall::QuotaStatus(pair, org, params) => {
            run_office::<ProviderQuotaStatus>(api, pair, &org, &params, true).await
        }
        ProviderCall::CooldownRecord(pair, org, params) => {
            run_office::<ProviderCooldownRecord>(api, pair, &org, &params, false).await
        }
        ProviderCall::ProfileCreate(pair, org, params) => {
            run_office::<ModelProfileCreate>(api, pair, &org, &params, false).await
        }
        ProviderCall::ProfileUpdate(pair, org, params) => {
            run_office::<ModelProfileUpdate>(api, pair, &org, &params, false).await
        }
        ProviderCall::ProfileModels(pair, org, params) => {
            run_office::<ModelProfileModels>(api, pair, &org, &params, true).await
        }
        ProviderCall::AgentReadiness(pair, org, params) => {
            run_office::<AgentReadinessGet>(api, pair, &org, &params, true).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_provider_queries_and_rejects_unknown_params() {
        let list = parse_connection_list_query(Some("include_archived=true"))
            .ok()
            .expect("valid include_archived");
        assert!(list.include_archived);
        assert!(parse_connection_list_query(Some("include_archived=maybe")).is_err());
        assert!(parse_connection_list_query(Some("unknown=1")).is_err());

        assert_eq!(
            parse_optional_connection_id(Some("connection_id=c1"))
                .ok()
                .flatten()
                .as_deref(),
            Some("c1")
        );
        assert!(parse_optional_connection_id(Some("other=1")).is_err());
        assert_eq!(
            parse_optional_task_id(Some("task_id=t1"))
                .ok()
                .flatten()
                .as_deref(),
            Some("t1")
        );
        assert!(parse_optional_task_id(Some("other=1")).is_err());
    }
}
