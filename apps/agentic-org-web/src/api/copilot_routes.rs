//! Browser HTTP routes for live Copilot smart-drafting (`TASK-60` AC #3):
//! - `GET  /api/v1/organizations/{org}/copilot/features` (`copilot.features`)
//! - `POST /api/v1/organizations/{org}/copilot/draft`    (`copilot.draft`)
//!
//! Forwards authenticated browser requests to the organization's HQ office (its first office when
//! none is HQ) using only `org-proto` wire types. Drafts never persist or execute actions; when
//! no tool-less runtime is configured on the office, `features` returns an empty list and `draft`
//! fails closed with `503 copilot_unavailable`.

use std::time::Duration;

use hyper::{http::request::Parts, StatusCode};
use org_proto::copilot::{CopilotDraft, CopilotDraftParams, CopilotFeatures};
use org_proto::web::{ReadResponse, WebError, WebErrorCode};
use org_proto::Empty;
use serde_json::Value;

use super::hq_context::{resolve_hq_office, revalidate_and_resolve_hq};
use super::{
    failure, invalid, not_found, result, unavailable, Call, CallerAuth, Route, SessionApi,
};
use crate::host::Failure;
use crate::office::OfficeFailure;
use crate::service::{ApiReply, PreparedCall};

/// Upper bound for `copilot.draft` (60s office runtime deadline + 5s IPC grace).
pub(super) const COPILOT_DRAFT_DEADLINE: Duration = Duration::from_secs(65);

const MAX_FEATURE_ID_BYTES: usize = 64;
const MAX_CONTEXT_ITEMS: usize = 8;

pub enum CopilotRoute {
    Features(CallerAuth, String),
    Draft(CallerAuth, String),
}

pub enum CopilotCall {
    Features(CallerAuth, String),
    Draft(CallerAuth, String, Box<CopilotDraftParams>),
}

impl CopilotCall {
    pub(super) fn timeout(&self) -> Option<Duration> {
        match self {
            Self::Features(_, _) => None,
            Self::Draft(_, _, _) => Some(COPILOT_DRAFT_DEADLINE),
        }
    }
}

fn valid_org_id(id: &str) -> Result<(), ApiReply> {
    org_office_launch::is_storage_id(id)
        .then_some(())
        .ok_or_else(invalid)
}

fn valid_feature_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_FEATURE_ID_BYTES
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_'))
}

/// Attempts to route `/api/v1/organizations/{org}/copilot/...` paths.
pub(super) fn try_route(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    tail: &[&str],
    get: bool,
    post: bool,
    _query: bool,
) -> Option<Result<Route, ApiReply>> {
    let ["copilot", rest @ ..] = tail else {
        return None;
    };
    let routed = match rest {
        ["features"] if get => {
            let caller = match api.caller_auth(request) {
                Ok(c) => c,
                Err(reply) => return Some(Err(reply)),
            };
            if let Err(reply) = valid_org_id(org) {
                return Some(Err(reply));
            }
            Ok(CopilotRoute::Features(caller, org.to_owned()))
        }
        ["draft"] if post => {
            let caller = match api.caller_auth(request) {
                Ok(c) => c,
                Err(reply) => return Some(Err(reply)),
            };
            if let Err(reply) = valid_org_id(org) {
                return Some(Err(reply));
            }
            Ok(CopilotRoute::Draft(caller, org.to_owned()))
        }
        ["features"] | ["draft"] => Err(invalid()),
        _ => Err(not_found()),
    };
    Some(routed.map(Route::Copilot))
}

pub(super) fn prepare(route: CopilotRoute, bytes: &[u8]) -> Result<PreparedCall<Call>, ApiReply> {
    let call = match route {
        CopilotRoute::Features(caller, org) => {
            if !bytes.is_empty() {
                return Err(invalid());
            }
            CopilotCall::Features(caller, org)
        }
        CopilotRoute::Draft(caller, org) => {
            let params: CopilotDraftParams =
                serde_json::from_slice(bytes).map_err(|_| invalid())?;
            if !valid_feature_id(&params.feature) || !params.input.is_object() {
                return Err(invalid());
            }
            if params.context.len() > MAX_CONTEXT_ITEMS {
                return Err(ApiReply::error(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    WebErrorCode::TooLarge,
                    "Too many copilot context items.",
                ));
            }
            CopilotCall::Draft(caller, org, Box::new(params))
        }
    };
    Ok(PreparedCall::read(Call::Copilot(call), bytes.len()))
}

pub(super) async fn execute(api: &SessionApi, call: CopilotCall) -> ApiReply {
    match call {
        CopilotCall::Features(caller, org_id) => {
            run_office::<CopilotFeatures>(api, caller, &org_id, &Empty {}, true, None).await
        }
        CopilotCall::Draft(caller, org_id, params) => {
            run_office::<CopilotDraft>(
                api,
                caller,
                &org_id,
                &params,
                false,
                Some(COPILOT_DRAFT_DEADLINE),
            )
            .await
        }
    }
}

async fn run_office<C: org_proto::Call>(
    api: &SessionApi,
    caller: CallerAuth,
    org_id: &str,
    params: &C::Params,
    retry_on_transport: bool,
    deadline: Option<Duration>,
) -> ApiReply {
    if let Err(error) = api.validate_async(caller).await {
        return failure(error);
    }
    let mut office_id = match resolve_hq_office(api, caller, org_id).await {
        Ok(id) => id,
        Err(Failure::Refused(_)) => return unavailable(),
        Err(error) => return failure(error),
    };
    let auth = api.auth(&caller);
    let call_once = |office: &str| {
        let auth = auth.clone();
        let office = office.to_owned();
        async move {
            match deadline {
                Some(limit) => {
                    api.offices
                        .call_typed_with_timeout::<C>(
                            &auth,
                            &office,
                            params,
                            retry_on_transport,
                            limit,
                        )
                        .await
                }
                None => {
                    api.offices
                        .call_typed::<C>(&auth, &office, params, retry_on_transport)
                        .await
                }
            }
        }
    };
    let mut reply = call_once(&office_id).await;
    if matches!(
        &reply,
        Err(OfficeFailure::Refused(e)) if e.code == org_proto::ErrorCode::NotAuthorized
    ) {
        office_id = match revalidate_and_resolve_hq(api, caller, org_id).await {
            Ok(id) => id,
            Err(Failure::Refused(_)) => return unavailable(),
            Err(error) => return failure(error),
        };
        reply = call_once(&office_id).await;
    }
    match reply {
        Ok(res) => result(res),
        Err(error) => map_office_failure(error),
    }
}

fn map_office_failure(error: OfficeFailure) -> ApiReply {
    match error {
        OfficeFailure::BeforeDispatch(e) => failure(e),
        OfficeFailure::Transport => unavailable(),
        OfficeFailure::Refused(err) => {
            let web = WebError::from(err);
            let status = match web.code {
                WebErrorCode::InvalidRequest => StatusCode::BAD_REQUEST,
                WebErrorCode::NotAuthorized => StatusCode::FORBIDDEN,
                WebErrorCode::NotFound => return not_found(),
                WebErrorCode::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
                WebErrorCode::CopilotBusy | WebErrorCode::RateLimited => {
                    StatusCode::TOO_MANY_REQUESTS
                }
                WebErrorCode::CopilotUnavailable
                | WebErrorCode::CopilotFailed
                | WebErrorCode::Timeout
                | WebErrorCode::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
                _ => return unavailable(),
            };
            ApiReply::json(
                status,
                serde_json::to_value(ReadResponse::<Value>::Error(web)).unwrap_or(Value::Null),
            )
        }
    }
}

#[cfg(test)]
#[path = "copilot_routes_tests.rs"]
mod tests;
