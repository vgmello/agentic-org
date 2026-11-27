//! Browser HTTP/1.1 & SSE routes for Ask Tess, turn cancellation, session reset, and
//! human-authenticated two-phase proposal apply/dismiss (`TASK-80.4` Part B).
//!
//! Routes strictly through [`crate::ports::OfficeTransport`] and [`crate::event_hub::EventBus`].

mod stream;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use hyper::{http::request::Parts, Method, StatusCode};
use org_proto::tess::{
    SuggestionApply, SuggestionApplyParams, SuggestionDismiss, SuggestionDismissParams,
    SuggestionList, SuggestionListParams, SuggestionStatus, TessAsk, TessAskParams, TessAskResult,
    TessAskStatus, TessSessionReset, TessSessionResetParams, TessTurnCancel, TessTurnCancelParams,
};
use org_proto::web::{ReadResponse, WebError, WebErrorCode};
use serde::Deserialize;
use serde_json::Value;

use super::hq_context::{resolve_hq_office, revalidate_and_resolve_hq};
use super::{
    failure, invalid, not_found, result, unavailable, Call, CallerAuth, Route, SessionApi,
};
use crate::host::Failure;
use crate::office::OfficeFailure;
use crate::service::{ApiReply, PreparedCall, WebReply};

pub enum TessRoute {
    Ask(CallerAuth, String),
    TurnStream {
        caller: CallerAuth,
        org_id: String,
        turn_id: String,
        last_event_id: Option<u64>,
    },
    TurnCancel(CallerAuth, String, String),
    ProposalsList(CallerAuth, String, SuggestionListParams),
    ProposalApply(CallerAuth, String, String),
    ProposalDismiss(CallerAuth, String, String),
    SessionReset(CallerAuth, String),
}

pub enum TessCall {
    Ask(CallerAuth, String, TessAskParams),
    TurnStream {
        caller: CallerAuth,
        org_id: String,
        turn_id: String,
        last_event_id: Option<u64>,
    },
    TurnCancel(CallerAuth, String, String),
    ProposalsList(CallerAuth, String, SuggestionListParams),
    ProposalApply(CallerAuth, String, SuggestionApplyParams),
    ProposalDismiss(CallerAuth, String, SuggestionDismissParams),
    SessionReset(CallerAuth, String),
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyBody {
    #[serde(default)]
    expected_revision: Option<u64>,
    #[serde(default)]
    if_match: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct DismissBody {
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyObject {}

pub(super) fn is_streaming(call: &TessCall) -> bool {
    matches!(call, TessCall::TurnStream { .. })
}

fn valid_org_id(id: &str) -> Result<(), ApiReply> {
    org_office_launch::is_storage_id(id)
        .then_some(())
        .ok_or_else(invalid)
}

fn valid_tess_id(id: &str) -> Result<(), ApiReply> {
    let ok = !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'));
    ok.then_some(()).ok_or_else(invalid)
}

fn parse_last_event_id(request: &Parts) -> Result<Option<u64>, ApiReply> {
    let mut values = request.headers.get_all("last-event-id").iter();
    let Some(raw) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(invalid());
    }
    let text = raw.to_str().map_err(|_| invalid())?.trim();
    if text.is_empty() {
        return Ok(None);
    }
    text.parse::<u64>().map(Some).map_err(|_| invalid())
}

fn parse_proposals_query(query: Option<&str>) -> Result<SuggestionListParams, ApiReply> {
    let mut params = SuggestionListParams::default();
    let Some(raw) = query.filter(|q| !q.is_empty()) else {
        return Ok(params);
    };
    for pair in raw.split('&') {
        let (k, v) = pair.split_once('=').ok_or_else(invalid)?;
        match k {
            "status" if params.status.is_none() => {
                let status = match v {
                    "pending" => SuggestionStatus::Pending,
                    "applied" => SuggestionStatus::Applied,
                    "withdrawn" => SuggestionStatus::Withdrawn,
                    "dismissed" => SuggestionStatus::Dismissed,
                    "expired" => SuggestionStatus::Expired,
                    _ => return Err(invalid()),
                };
                params.status = Some(status);
            }
            "limit" if params.limit.is_none() => {
                let limit = v.parse::<u32>().map_err(|_| invalid())?;
                if limit == 0 {
                    return Err(invalid());
                }
                params.limit = Some(limit);
            }
            _ => return Err(invalid()),
        }
    }
    Ok(params)
}

/// Attempts to route `/api/v1/organizations/{org}/tess/...` paths.
pub(super) fn try_route(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    tail: &[&str],
    get: bool,
    post: bool,
    query: bool,
) -> Option<Result<Route, ApiReply>> {
    let ["tess", rest @ ..] = tail else {
        return None;
    };
    let is_get_only = request.method == Method::GET && !query;
    let is_get_or_head = matches!(request.method, Method::GET | Method::HEAD);
    let routed = match rest {
        ["ask"] if post => route_simple(api, request, org, TessRoute::Ask),
        ["turns", id, "stream"] if is_get_only => route_stream(api, request, org, id),
        ["turns", id, "cancel"] if post => route_id(api, request, org, id, TessRoute::TurnCancel),
        ["proposals"] if is_get_or_head => route_proposals_list(api, request, org),
        ["proposals", id, "apply"] if post => {
            route_id(api, request, org, id, TessRoute::ProposalApply)
        }
        ["proposals", id, "dismiss"] if post => {
            route_id(api, request, org, id, TessRoute::ProposalDismiss)
        }
        ["session", "reset"] if post => route_simple(api, request, org, TessRoute::SessionReset),
        ["ask"]
        | ["turns", _, "stream"]
        | ["turns", _, "cancel"]
        | ["proposals"]
        | ["proposals", _, "apply" | "dismiss"]
        | ["session", "reset"] => {
            let _ = get;
            Err(invalid())
        }
        _ => Err(not_found()),
    };
    Some(routed.map(Route::Tess))
}

fn route_simple(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    build: fn(CallerAuth, String) -> TessRoute,
) -> Result<TessRoute, ApiReply> {
    let caller = api.caller_auth(request)?;
    valid_org_id(org)?;
    Ok(build(caller, org.to_owned()))
}

fn route_id(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    id: &str,
    build: fn(CallerAuth, String, String) -> TessRoute,
) -> Result<TessRoute, ApiReply> {
    let caller = api.caller_auth(request)?;
    valid_org_id(org)?;
    valid_tess_id(id)?;
    Ok(build(caller, org.to_owned(), id.to_owned()))
}

fn route_stream(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    id: &str,
) -> Result<TessRoute, ApiReply> {
    let caller = api.caller_auth(request)?;
    valid_org_id(org)?;
    valid_tess_id(id)?;
    let last_event_id = parse_last_event_id(request)?;
    Ok(TessRoute::TurnStream {
        caller,
        org_id: org.to_owned(),
        turn_id: id.to_owned(),
        last_event_id,
    })
}

fn route_proposals_list(
    api: &SessionApi,
    request: &Parts,
    org: &str,
) -> Result<TessRoute, ApiReply> {
    let caller = api.caller_auth(request)?;
    valid_org_id(org)?;
    let params = parse_proposals_query(request.uri.query())?;
    Ok(TessRoute::ProposalsList(caller, org.to_owned(), params))
}

fn decode_optional_json<T: Default + serde::de::DeserializeOwned>(
    bytes: &[u8],
) -> Result<T, ApiReply> {
    if bytes.is_empty() {
        Ok(T::default())
    } else {
        serde_json::from_slice(bytes).map_err(|_| invalid())
    }
}

pub(super) fn prepare(route: TessRoute, bytes: &[u8]) -> Result<PreparedCall<Call>, ApiReply> {
    let empty = || bytes.is_empty().then_some(()).ok_or_else(invalid);
    let call = match route {
        TessRoute::Ask(caller, org_id) => {
            let params: TessAskParams = serde_json::from_slice(bytes).map_err(|_| invalid())?;
            TessCall::Ask(caller, org_id, params)
        }
        TessRoute::TurnStream {
            caller,
            org_id,
            turn_id,
            last_event_id,
        } => {
            empty()?;
            TessCall::TurnStream {
                caller,
                org_id,
                turn_id,
                last_event_id,
            }
        }
        TessRoute::TurnCancel(caller, org_id, turn_id) => {
            let _: EmptyObject = decode_optional_json(bytes)?;
            TessCall::TurnCancel(caller, org_id, turn_id)
        }
        TessRoute::ProposalsList(caller, org_id, params) => {
            empty()?;
            TessCall::ProposalsList(caller, org_id, params)
        }
        TessRoute::ProposalApply(caller, org_id, suggestion_id) => {
            let body: ApplyBody = decode_optional_json(bytes)?;
            TessCall::ProposalApply(
                caller,
                org_id,
                SuggestionApplyParams {
                    suggestion_id,
                    expected_revision: body.expected_revision,
                    if_match: body.if_match,
                },
            )
        }
        TessRoute::ProposalDismiss(caller, org_id, suggestion_id) => {
            let body: DismissBody = decode_optional_json(bytes)?;
            TessCall::ProposalDismiss(
                caller,
                org_id,
                SuggestionDismissParams {
                    suggestion_id,
                    reason: body.reason,
                },
            )
        }
        TessRoute::SessionReset(caller, org_id) => {
            let _: EmptyObject = decode_optional_json(bytes)?;
            TessCall::SessionReset(caller, org_id)
        }
    };
    Ok(PreparedCall::read(Call::Tess(call), bytes.len()))
}

pub(super) async fn execute(api: Arc<SessionApi>, call: TessCall) -> WebReply {
    match call {
        TessCall::Ask(caller, org_id, params) => {
            WebReply::Json(execute_ask(&api, caller, &org_id, &params).await)
        }
        TessCall::TurnStream {
            caller,
            org_id,
            turn_id,
            last_event_id,
        } => stream::execute_turn_stream(&api, caller, org_id, turn_id, last_event_id).await,
        TessCall::TurnCancel(caller, org_id, turn_id) => {
            let params = TessTurnCancelParams { turn_id };
            WebReply::Json(
                run_office::<TessTurnCancel>(&api, caller, &org_id, &params, false).await,
            )
        }
        TessCall::ProposalsList(caller, org_id, params) => {
            WebReply::Json(run_office::<SuggestionList>(&api, caller, &org_id, &params, true).await)
        }
        TessCall::ProposalApply(caller, org_id, params) => WebReply::Json(
            run_office::<SuggestionApply>(&api, caller, &org_id, &params, false).await,
        ),
        TessCall::ProposalDismiss(caller, org_id, params) => WebReply::Json(
            run_office::<SuggestionDismiss>(&api, caller, &org_id, &params, false).await,
        ),
        TessCall::SessionReset(caller, org_id) => WebReply::Json(
            run_office::<TessSessionReset>(
                &api,
                caller,
                &org_id,
                &TessSessionResetParams {},
                false,
            )
            .await,
        ),
    }
}

async fn execute_ask(
    api: &SessionApi,
    caller: CallerAuth,
    org_id: &str,
    params: &TessAskParams,
) -> ApiReply {
    match call_office_with_retry::<TessAsk>(api, caller, org_id, params, false).await {
        Ok(res) => result(ask_wire_json(org_id, res)),
        Err(reply) => reply,
    }
}

fn ask_wire_json(org_id: &str, res: TessAskResult) -> Value {
    let stream_url = (res.status == TessAskStatus::Streaming)
        .then_some(res.turn_id.as_deref())
        .flatten()
        .map(|tid| format!("/api/v1/organizations/{org_id}/tess/turns/{tid}/stream"));
    let mut obj = serde_json::to_value(&res).unwrap_or(Value::Null);
    if let (Some(map), Some(url)) = (obj.as_object_mut(), stream_url) {
        map.insert("stream_url".to_owned(), Value::String(url));
    }
    obj
}

async fn run_office<C: org_proto::Call>(
    api: &SessionApi,
    caller: CallerAuth,
    org_id: &str,
    params: &C::Params,
    is_read: bool,
) -> ApiReply {
    match call_office_with_retry::<C>(api, caller, org_id, params, is_read).await {
        Ok(res) => result(res),
        Err(reply) => reply,
    }
}

pub(super) async fn call_office_with_retry<C: org_proto::Call>(
    api: &SessionApi,
    caller: CallerAuth,
    org_id: &str,
    params: &C::Params,
    is_read: bool,
) -> Result<C::Result, ApiReply> {
    if let Err(error) = api.validate_async(caller).await {
        return Err(failure(error));
    }
    let mut office_id = match resolve_hq_office(api, caller, org_id).await {
        Ok(id) => id,
        Err(Failure::Refused(_)) => return Err(unavailable()),
        Err(error) => return Err(failure(error)),
    };
    let auth = api.auth(&caller);
    let mut reply = api
        .offices
        .call_typed::<C>(&auth, &office_id, params, is_read)
        .await;
    if matches!(
        &reply,
        Err(OfficeFailure::Refused(e)) if e.code == org_proto::ErrorCode::NotAuthorized
    ) {
        office_id = match revalidate_and_resolve_hq(api, caller, org_id).await {
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

pub(super) fn map_office_failure(error: OfficeFailure) -> ApiReply {
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
                WebErrorCode::Conflict
                | WebErrorCode::AlreadyExists
                | WebErrorCode::NotEmpty
                | WebErrorCode::OperationReused => StatusCode::CONFLICT,
                WebErrorCode::Expired => StatusCode::GONE,
                WebErrorCode::RateLimited => StatusCode::TOO_MANY_REQUESTS,
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
