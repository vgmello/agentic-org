//! Browser HTTP routes for `L2` usage/cost accounting & CSV export and `L3` budget controls
//! (`TASK-53`, `doc-87`, `doc-92`, `doc-51`, `FC-084..093`).
//!
//! All mutations use `POST` (`agentic-org-web` ingress policy).

use hyper::{http::request::Parts, StatusCode};
use org_proto::budgets::{
    BudgetClear, BudgetClearParams, BudgetExtendOnce, BudgetExtendOnceParams, BudgetGet,
    BudgetGetParams, BudgetKeepPaused, BudgetKeepPausedParams, BudgetList, BudgetListParams,
    BudgetScopeKind, BudgetSet, BudgetSetParams, BudgetStop, BudgetStopParams,
};
use org_proto::costs::{
    CostExportCsv, CostPriceList, CostPriceListParams, CostPriceSet, CostPriceSetParams, CostQuery,
    CostQueryParams, CostUsageCorrect, CostUsageCorrectParams, CostUsageRecord,
    CostUsageRecordParams,
};
use org_proto::web::{ReadResponse, WebError, WebErrorCode};
use serde_json::Value;

use super::org_routes::{resolve_hq_office, revalidate_and_resolve_hq};
use super::{failure, invalid, result, unavailable, Call, CallerAuth, Route, SessionApi};
use crate::host::Failure;
use crate::office::OfficeFailure;
use crate::service::{ApiReply, PreparedCall};

pub enum CostRoute {
    PriceList(CallerAuth, String, CostPriceListParams),
    PriceSet(CallerAuth, String),
    CostQuery(CallerAuth, String),
    CostExport(CallerAuth, String),
    UsageRecord(CallerAuth, String),
    UsageCorrect(CallerAuth, String),
    BudgetList(CallerAuth, String, BudgetListParams),
    BudgetGet(CallerAuth, String),
    BudgetSet(CallerAuth, String),
    BudgetClear(CallerAuth, String),
    BudgetExtendOnce(CallerAuth, String),
    BudgetStop(CallerAuth, String),
    BudgetKeepPaused(CallerAuth, String),
}

pub enum CostCall {
    PriceList(CallerAuth, String, CostPriceListParams),
    PriceSet(CallerAuth, String, CostPriceSetParams),
    CostQuery(CallerAuth, String, CostQueryParams),
    CostExport(CallerAuth, String, CostQueryParams),
    UsageRecord(CallerAuth, String, Box<CostUsageRecordParams>),
    UsageCorrect(CallerAuth, String, CostUsageCorrectParams),
    BudgetList(CallerAuth, String, BudgetListParams),
    BudgetGet(CallerAuth, String, BudgetGetParams),
    BudgetSet(CallerAuth, String, BudgetSetParams),
    BudgetClear(CallerAuth, String, BudgetClearParams),
    BudgetExtendOnce(CallerAuth, String, BudgetExtendOnceParams),
    BudgetStop(CallerAuth, String, BudgetStopParams),
    BudgetKeepPaused(CallerAuth, String, BudgetKeepPausedParams),
}

fn valid_org_id(id: &str) -> Result<(), ApiReply> {
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
        pairs.push((k.to_owned(), decode_query_value(v)?));
    }
    Ok(pairs)
}

fn decode_query_value(raw: &str) -> Result<String, ApiReply> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                match (hi, lo) {
                    (Some(h), Some(l)) => {
                        out.push(((h << 4) | l) as u8);
                        i += 3;
                    }
                    _ => return Err(invalid()),
                }
            }
            b'%' => return Err(invalid()),
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| invalid())
}

fn parse_price_list_query(query: Option<&str>) -> Result<CostPriceListParams, ApiReply> {
    let mut out = CostPriceListParams::default();
    for (k, v) in parse_query_pairs(query)? {
        match k.as_str() {
            "provider" => out.provider = Some(v),
            "model" => out.model = Some(v),
            _ => return Err(invalid()),
        }
    }
    Ok(out)
}

fn parse_budget_list_query(query: Option<&str>) -> Result<BudgetListParams, ApiReply> {
    let mut out = BudgetListParams::default();
    for (k, v) in parse_query_pairs(query)? {
        match k.as_str() {
            "scope_kind" => {
                out.scope_kind = Some(match v.as_str() {
                    "task" => BudgetScopeKind::Task,
                    "agent" => BudgetScopeKind::Agent,
                    "release" => BudgetScopeKind::Release,
                    _ => return Err(invalid()),
                });
            }
            "task_id" => out.task_id = Some(v),
            "agent_id" => out.agent_id = Some(v),
            "release_id" => out.release_id = Some(v),
            _ => return Err(invalid()),
        }
    }
    Ok(out)
}

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
        ["costs", "prices"] if is_get => Some(route_price_list(api, request, org)),
        ["costs", "prices"] if post && !query => {
            Some(route_post(api, request, org, CostRoute::PriceSet))
        }
        ["costs", "query"] if post && !query => {
            Some(route_post(api, request, org, CostRoute::CostQuery))
        }
        ["costs", "export"] if post && !query => {
            Some(route_post(api, request, org, CostRoute::CostExport))
        }
        ["costs", "usage", "record"] if post && !query => {
            Some(route_post(api, request, org, CostRoute::UsageRecord))
        }
        ["costs", "usage", "correct"] if post && !query => {
            Some(route_post(api, request, org, CostRoute::UsageCorrect))
        }
        ["budgets"] if is_get => Some(route_budget_list(api, request, org)),
        ["budgets", "get"] if post && !query => {
            Some(route_post(api, request, org, CostRoute::BudgetGet))
        }
        ["budgets", "set"] if post && !query => {
            Some(route_post(api, request, org, CostRoute::BudgetSet))
        }
        ["budgets", "clear"] if post && !query => {
            Some(route_post(api, request, org, CostRoute::BudgetClear))
        }
        ["budgets", "extend-once"] if post && !query => {
            Some(route_post(api, request, org, CostRoute::BudgetExtendOnce))
        }
        ["budgets", "stop"] if post && !query => {
            Some(route_post(api, request, org, CostRoute::BudgetStop))
        }
        ["budgets", "keep-paused"] if post && !query => {
            Some(route_post(api, request, org, CostRoute::BudgetKeepPaused))
        }
        _ => None,
    }
}

fn route_price_list(api: &SessionApi, request: &Parts, org: &str) -> Result<Route, ApiReply> {
    let pair = api.caller_auth(request)?;
    valid_org_id(org)?;
    let params = parse_price_list_query(request.uri.query())?;
    Ok(Route::Cost(CostRoute::PriceList(
        pair,
        org.to_owned(),
        params,
    )))
}

fn route_budget_list(api: &SessionApi, request: &Parts, org: &str) -> Result<Route, ApiReply> {
    let pair = api.caller_auth(request)?;
    valid_org_id(org)?;
    let params = parse_budget_list_query(request.uri.query())?;
    Ok(Route::Cost(CostRoute::BudgetList(
        pair,
        org.to_owned(),
        params,
    )))
}

fn route_post(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    ctor: fn(CallerAuth, String) -> CostRoute,
) -> Result<Route, ApiReply> {
    let pair = api.caller_auth(request)?;
    valid_org_id(org)?;
    Ok(Route::Cost(ctor(pair, org.to_owned())))
}

fn decode_body<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, ApiReply> {
    serde_json::from_slice(bytes).map_err(|_| invalid())
}

pub(super) fn prepare(route: CostRoute, bytes: &[u8]) -> Result<PreparedCall<Call>, ApiReply> {
    let call = match route {
        CostRoute::PriceList(pair, org, params) => {
            if !bytes.is_empty() {
                return Err(invalid());
            }
            CostCall::PriceList(pair, org, params)
        }
        CostRoute::PriceSet(pair, org) => CostCall::PriceSet(pair, org, decode_body(bytes)?),
        CostRoute::CostQuery(pair, org) => CostCall::CostQuery(pair, org, decode_body(bytes)?),
        CostRoute::CostExport(pair, org) => CostCall::CostExport(pair, org, decode_body(bytes)?),
        CostRoute::UsageRecord(pair, org) => {
            CostCall::UsageRecord(pair, org, Box::new(decode_body(bytes)?))
        }
        CostRoute::UsageCorrect(pair, org) => {
            CostCall::UsageCorrect(pair, org, decode_body(bytes)?)
        }
        CostRoute::BudgetList(pair, org, params) => {
            if !bytes.is_empty() {
                return Err(invalid());
            }
            CostCall::BudgetList(pair, org, params)
        }
        CostRoute::BudgetGet(pair, org) => CostCall::BudgetGet(pair, org, decode_body(bytes)?),
        CostRoute::BudgetSet(pair, org) => CostCall::BudgetSet(pair, org, decode_body(bytes)?),
        CostRoute::BudgetClear(pair, org) => CostCall::BudgetClear(pair, org, decode_body(bytes)?),
        CostRoute::BudgetExtendOnce(pair, org) => {
            CostCall::BudgetExtendOnce(pair, org, decode_body(bytes)?)
        }
        CostRoute::BudgetStop(pair, org) => CostCall::BudgetStop(pair, org, decode_body(bytes)?),
        CostRoute::BudgetKeepPaused(pair, org) => {
            CostCall::BudgetKeepPaused(pair, org, decode_body(bytes)?)
        }
    };
    Ok(PreparedCall::read(Call::Cost(call), bytes.len()))
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

pub(super) async fn execute(api: &SessionApi, call: CostCall) -> ApiReply {
    match call {
        CostCall::PriceList(pair, org, params) => {
            run_office::<CostPriceList>(api, pair, &org, &params, true).await
        }
        CostCall::PriceSet(pair, org, params) => {
            run_office::<CostPriceSet>(api, pair, &org, &params, false).await
        }
        CostCall::CostQuery(pair, org, mut params) => {
            if params.time_zone.is_none() {
                params.time_zone = api.validate(pair).ok().and_then(|s| s.account.time_zone);
            }
            run_office::<CostQuery>(api, pair, &org, &params, true).await
        }
        CostCall::CostExport(pair, org, mut params) => {
            if params.time_zone.is_none() {
                params.time_zone = api.validate(pair).ok().and_then(|s| s.account.time_zone);
            }
            run_office::<CostExportCsv>(api, pair, &org, &params, true).await
        }
        CostCall::UsageRecord(pair, org, params) => {
            run_office::<CostUsageRecord>(api, pair, &org, params.as_ref(), false).await
        }
        CostCall::UsageCorrect(pair, org, params) => {
            run_office::<CostUsageCorrect>(api, pair, &org, &params, false).await
        }
        CostCall::BudgetList(pair, org, params) => {
            run_office::<BudgetList>(api, pair, &org, &params, true).await
        }
        CostCall::BudgetGet(pair, org, params) => {
            run_office::<BudgetGet>(api, pair, &org, &params, true).await
        }
        CostCall::BudgetSet(pair, org, mut params) => {
            if params.time_zone.is_none() && params.scope_kind == BudgetScopeKind::Agent {
                params.time_zone = api.validate(pair).ok().and_then(|s| s.account.time_zone);
            }
            run_office::<BudgetSet>(api, pair, &org, &params, false).await
        }
        CostCall::BudgetClear(pair, org, params) => {
            run_office::<BudgetClear>(api, pair, &org, &params, false).await
        }
        CostCall::BudgetExtendOnce(pair, org, params) => {
            run_office::<BudgetExtendOnce>(api, pair, &org, &params, false).await
        }
        CostCall::BudgetStop(pair, org, params) => {
            run_office::<BudgetStop>(api, pair, &org, &params, false).await
        }
        CostCall::BudgetKeepPaused(pair, org, params) => {
            run_office::<BudgetKeepPaused>(api, pair, &org, &params, false).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cost_and_budget_list_queries() {
        let prices = parse_price_list_query(Some("provider=anthropic&model=claude-sonnet-4"))
            .ok()
            .expect("parse prices query");
        assert_eq!(prices.provider.as_deref(), Some("anthropic"));
        assert_eq!(prices.model.as_deref(), Some("claude-sonnet-4"));
        assert!(parse_price_list_query(Some("unknown=1")).is_err());

        let budgets = parse_budget_list_query(Some("scope_kind=agent&agent_id=a1"))
            .ok()
            .expect("parse budgets query");
        assert_eq!(budgets.scope_kind, Some(BudgetScopeKind::Agent));
        assert_eq!(budgets.agent_id.as_deref(), Some("a1"));
        assert!(parse_budget_list_query(Some("scope_kind=project")).is_err());
    }
}
