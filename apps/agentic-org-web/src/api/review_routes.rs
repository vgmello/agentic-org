//! Authenticated browser HTTP routes for reviews (`TASK-92.2` AC #2):
//!
//! - `GET  /api/v1/organizations/{org}/reviews`                       -> `review.list`
//! - `POST /api/v1/organizations/{org}/reviews`                       -> `review.request`
//! - `GET  /api/v1/organizations/{org}/reviews/{review_ref}`          -> `review.inspect`
//! - `GET  /api/v1/organizations/{org}/reviews/{review_ref}/diff`     -> `review.diff`
//! - `POST /api/v1/organizations/{org}/reviews/{review_ref}/route`    -> `review.route`
//! - `POST /api/v1/organizations/{org}/reviews/{review_ref}/findings` -> `review.finding`
//! - `POST /api/v1/organizations/{org}/reviews/{review_ref}/verdict`  -> `review.verdict`
//! - `POST /api/v1/organizations/{org}/reviews/{review_ref}/resubmit` -> `review.resubmit`
//! - `POST /api/v1/organizations/{org}/reviews/{review_ref}/close`    -> `review.close`
//!
//! Every route authenticates the browser caller before touching the office, validates path/query
//! and `deny_unknown_fields` JSON inputs, forwards over [`crate::ports::OfficeTransport`], and
//! suppresses internal/storage errors behind `503 Service unavailable.`.

use hyper::{http::request::Parts, Method, StatusCode};
use org_proto::reviews::{
    ReviewClose, ReviewCloseParams, ReviewDiff, ReviewDiffParams, ReviewFinding,
    ReviewFindingParams, ReviewInspect, ReviewInspectParams, ReviewList, ReviewListParams,
    ReviewRequest, ReviewRequestParams, ReviewResubmit, ReviewResubmitParams,
    ReviewRoute as ReviewRouteCall, ReviewRouteParams, ReviewVerdictCall, ReviewVerdictParams,
};
use org_proto::web::{ReadResponse, WebError, WebErrorCode};
use org_proto::Call as ProtoCall;
use serde_json::Value;

use super::hq_context::{resolve_hq_office, revalidate_and_resolve_hq};
use super::{
    failure, invalid, not_found, result, unavailable, Call, CallerAuth, Route, SessionApi,
};
use crate::host::Failure;
use crate::office::OfficeFailure;
use crate::ports::{OfficeCallOptions, OFFICE_DEADLINE};
use crate::service::{ApiReply, PreparedCall};

mod bodies;
mod query;
#[cfg(test)]
mod tests;

use bodies::{
    decode_close_body, decode_finding_body, decode_request_body, decode_resubmit_body,
    decode_route_body, decode_verdict_body,
};
use query::{
    filter_diff_by_line_range, filter_inspect_round, parse_diff_query, parse_inspect_query,
    parse_list_query, valid_org_id, valid_ref, DiffQuery, InspectQuery,
};

pub enum ReviewRoute {
    List(CallerAuth, String, Box<ReviewListParams>),
    Request(CallerAuth, String),
    Inspect(CallerAuth, String, Box<ReviewInspectParams>, Option<u32>),
    Diff {
        caller: CallerAuth,
        org_id: String,
        params: Box<ReviewDiffParams>,
        start_line: Option<u32>,
        end_line: Option<u32>,
    },
    Route(CallerAuth, String, String),
    Finding(CallerAuth, String, String),
    Verdict(CallerAuth, String, String),
    Resubmit(CallerAuth, String, String),
    Close(CallerAuth, String, String),
}

pub enum ReviewCall {
    List(CallerAuth, String, Box<ReviewListParams>),
    Request(CallerAuth, String, Box<ReviewRequestParams>),
    Inspect(CallerAuth, String, Box<ReviewInspectParams>, Option<u32>),
    Diff {
        caller: CallerAuth,
        org_id: String,
        params: Box<ReviewDiffParams>,
        start_line: Option<u32>,
        end_line: Option<u32>,
    },
    Route(CallerAuth, String, Box<ReviewRouteParams>),
    Finding(CallerAuth, String, Box<ReviewFindingParams>),
    Verdict(CallerAuth, String, Box<ReviewVerdictParams>),
    Resubmit(CallerAuth, String, Box<ReviewResubmitParams>),
    Close(CallerAuth, String, Box<ReviewCloseParams>),
}

/// Attempts to route `/api/v1/organizations/{org}/reviews*` paths.
pub(super) fn try_route(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    tail: &[&str],
    _get: bool,
    post: bool,
    _query: bool,
) -> Option<Result<Route, ApiReply>> {
    let is_get = matches!(request.method, Method::GET | Method::HEAD);
    match tail {
        ["reviews"] if is_get => Some((|| {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            let params = parse_list_query(request.uri.query())?;
            Ok(Route::Reviews(ReviewRoute::List(
                pair,
                org.to_owned(),
                Box::new(params),
            )))
        })()),
        ["reviews"] if post => Some((|| {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            Ok(Route::Reviews(ReviewRoute::Request(pair, org.to_owned())))
        })()),
        ["reviews", review_ref] if is_get => Some((|| {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            valid_ref(review_ref)?;
            let InspectQuery { params, round } =
                parse_inspect_query((*review_ref).to_owned(), request.uri.query())?;
            Ok(Route::Reviews(ReviewRoute::Inspect(
                pair,
                org.to_owned(),
                Box::new(params),
                round,
            )))
        })()),
        ["reviews", review_ref, "diff"] if is_get => Some((|| {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            valid_ref(review_ref)?;
            let DiffQuery {
                params,
                start_line,
                end_line,
            } = parse_diff_query((*review_ref).to_owned(), request.uri.query())?;
            Ok(Route::Reviews(ReviewRoute::Diff {
                caller: pair,
                org_id: org.to_owned(),
                params: Box::new(params),
                start_line,
                end_line,
            }))
        })()),
        ["reviews", review_ref, action @ ("route" | "findings" | "verdict" | "resubmit" | "close")]
            if post =>
        {
            Some(route_review_post_action(
                api, request, org, review_ref, action,
            ))
        }
        ["reviews"]
        | ["reviews", _]
        | ["reviews", _, "diff" | "route" | "findings" | "verdict" | "resubmit" | "close"] => {
            Some(Err(invalid()))
        }
        ["reviews", ..] => Some(Err(not_found())),
        _ => None,
    }
}

fn route_review_post_action(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    review_ref: &str,
    action: &str,
) -> Result<Route, ApiReply> {
    let pair = api.caller_auth(request)?;
    valid_org_id(org)?;
    valid_ref(review_ref)?;
    let org = org.to_owned();
    let review = review_ref.to_owned();
    let route = match action {
        "route" => ReviewRoute::Route(pair, org, review),
        "findings" => ReviewRoute::Finding(pair, org, review),
        "verdict" => ReviewRoute::Verdict(pair, org, review),
        "resubmit" => ReviewRoute::Resubmit(pair, org, review),
        "close" => ReviewRoute::Close(pair, org, review),
        _ => return Err(not_found()),
    };
    Ok(Route::Reviews(route))
}

fn empty(bytes: &[u8]) -> Result<(), ApiReply> {
    bytes.is_empty().then_some(()).ok_or_else(invalid)
}

pub(super) fn prepare(route: ReviewRoute, bytes: &[u8]) -> Result<PreparedCall<Call>, ApiReply> {
    let call = match route {
        ReviewRoute::List(c, o, p) => {
            empty(bytes)?;
            ReviewCall::List(c, o, p)
        }
        ReviewRoute::Request(c, o) => {
            ReviewCall::Request(c, o, Box::new(decode_request_body(bytes)?))
        }
        ReviewRoute::Inspect(c, o, p, round) => {
            empty(bytes)?;
            ReviewCall::Inspect(c, o, p, round)
        }
        ReviewRoute::Diff {
            caller,
            org_id,
            params,
            start_line,
            end_line,
        } => {
            empty(bytes)?;
            ReviewCall::Diff {
                caller,
                org_id,
                params,
                start_line,
                end_line,
            }
        }
        ReviewRoute::Route(c, o, review) => {
            ReviewCall::Route(c, o, Box::new(decode_route_body(review, bytes)?))
        }
        ReviewRoute::Finding(c, o, review) => {
            ReviewCall::Finding(c, o, Box::new(decode_finding_body(review, bytes)?))
        }
        ReviewRoute::Verdict(c, o, review) => {
            ReviewCall::Verdict(c, o, Box::new(decode_verdict_body(review, bytes)?))
        }
        ReviewRoute::Resubmit(c, o, review) => {
            ReviewCall::Resubmit(c, o, Box::new(decode_resubmit_body(review, bytes)?))
        }
        ReviewRoute::Close(c, o, review) => {
            ReviewCall::Close(c, o, Box::new(decode_close_body(review, bytes)?))
        }
    };
    Ok(PreparedCall::read(Call::Reviews(call), bytes.len()))
}

pub(super) async fn execute(api: &SessionApi, call: ReviewCall) -> ApiReply {
    match call {
        ReviewCall::List(c, o, p) => run_office_typed::<ReviewList>(api, c, &o, &p, true).await,
        ReviewCall::Request(c, o, p) => {
            run_office_typed::<ReviewRequest>(api, c, &o, &p, false).await
        }
        ReviewCall::Inspect(c, o, p, round) => run_inspect(api, c, &o, &p, round).await,
        ReviewCall::Diff {
            caller,
            org_id,
            params,
            start_line,
            end_line,
        } => run_diff(api, caller, &org_id, &params, start_line, end_line).await,
        ReviewCall::Route(c, o, p) => {
            run_office_typed::<ReviewRouteCall>(api, c, &o, &p, false).await
        }
        ReviewCall::Finding(c, o, p) => {
            run_office_typed::<ReviewFinding>(api, c, &o, &p, false).await
        }
        ReviewCall::Verdict(c, o, p) => {
            run_office_typed::<ReviewVerdictCall>(api, c, &o, &p, false).await
        }
        ReviewCall::Resubmit(c, o, p) => {
            run_office_typed::<ReviewResubmit>(api, c, &o, &p, false).await
        }
        ReviewCall::Close(c, o, p) => run_office_typed::<ReviewClose>(api, c, &o, &p, false).await,
    }
}

async fn resolve_office(
    api: &SessionApi,
    pair: CallerAuth,
    org_id: &str,
) -> Result<String, ApiReply> {
    if let Err(error) = api.validate_async(pair).await {
        return Err(failure(error));
    }
    match resolve_hq_office(api, pair, org_id).await {
        Ok(id) => Ok(id),
        Err(Failure::Refused(_)) => Err(unavailable()),
        Err(error) => Err(failure(error)),
    }
}

async fn retry_resolve_office(
    api: &SessionApi,
    pair: CallerAuth,
    org_id: &str,
) -> Result<String, ApiReply> {
    match revalidate_and_resolve_hq(api, pair, org_id).await {
        Ok(id) => Ok(id),
        Err(Failure::Refused(_)) => Err(unavailable()),
        Err(error) => Err(failure(error)),
    }
}

async fn run_office_typed<C: ProtoCall>(
    api: &SessionApi,
    pair: CallerAuth,
    org_id: &str,
    params: &C::Params,
    is_read: bool,
) -> ApiReply {
    let mut office_id = match resolve_office(api, pair, org_id).await {
        Ok(id) => id,
        Err(reply) => return reply,
    };
    let auth = api.auth(&pair);
    let mut reply = api
        .offices
        .call_typed::<C>(&auth, &office_id, params, is_read)
        .await;
    if is_not_authorized(&reply) {
        office_id = match retry_resolve_office(api, pair, org_id).await {
            Ok(id) => id,
            Err(reply) => return reply,
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

async fn run_inspect(
    api: &SessionApi,
    pair: CallerAuth,
    org_id: &str,
    params: &ReviewInspectParams,
    round: Option<u32>,
) -> ApiReply {
    let mut office_id = match resolve_office(api, pair, org_id).await {
        Ok(id) => id,
        Err(reply) => return reply,
    };
    let Ok(params_val) = serde_json::to_value(params) else {
        return unavailable();
    };
    let auth = api.auth(&pair);
    let opts = OfficeCallOptions::multiplexed(true, OFFICE_DEADLINE);
    let mut reply = api
        .offices
        .call_raw(
            &auth,
            &office_id,
            ReviewInspect::METHOD,
            params_val.clone(),
            opts,
        )
        .await;
    if is_not_authorized(&reply) {
        office_id = match retry_resolve_office(api, pair, org_id).await {
            Ok(id) => id,
            Err(reply) => return reply,
        };
        reply = api
            .offices
            .call_raw(&auth, &office_id, ReviewInspect::METHOD, params_val, opts)
            .await;
    }
    let mut val = match reply {
        Ok(val) => val,
        Err(err) => return map_office_failure(err),
    };
    if let Some(round_num) = round {
        if let Err(err_reply) = filter_inspect_round(&mut val, round_num) {
            return err_reply;
        }
    }
    result(val)
}

async fn run_diff(
    api: &SessionApi,
    pair: CallerAuth,
    org_id: &str,
    params: &ReviewDiffParams,
    start_line: Option<u32>,
    end_line: Option<u32>,
) -> ApiReply {
    let mut office_id = match resolve_office(api, pair, org_id).await {
        Ok(id) => id,
        Err(reply) => return reply,
    };
    let auth = api.auth(&pair);
    let mut reply = api
        .offices
        .call_typed::<ReviewDiff>(&auth, &office_id, params, true)
        .await;
    if is_not_authorized(&reply) {
        office_id = match retry_resolve_office(api, pair, org_id).await {
            Ok(id) => id,
            Err(reply) => return reply,
        };
        reply = api
            .offices
            .call_typed::<ReviewDiff>(&auth, &office_id, params, true)
            .await;
    }
    match reply {
        Ok(mut diff) => {
            filter_diff_by_line_range(&mut diff, start_line, end_line);
            result(diff)
        }
        Err(err) => map_office_failure(err),
    }
}

fn is_not_authorized<T>(reply: &Result<T, OfficeFailure>) -> bool {
    matches!(
        reply,
        Err(OfficeFailure::Refused(e)) if e.code == org_proto::ErrorCode::NotAuthorized
    )
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
