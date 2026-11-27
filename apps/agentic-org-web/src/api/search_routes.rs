//! Browser HTTP route of unified cross-entity search (`TASK-91`):
//!
//! - `GET /api/v1/organizations/{org}/search?q=...` (`search.query`; `types`, `project`,
//!   `channel`, `status`, `modified_file`, `assignee`, `label`, `author_id`, `count_only`,
//!   `limit`, `cursor` and `fields` query parameters, lists comma-separated)
//!
//! forwarded to the organization's HQ office (its first office when none is HQ) as the
//! signed-in member, whose visibility the office decides: which messages, and which private
//! notes and decisions, a search may reach. This slice never widens that and never caches an
//! answer.
//!
//! Everything is checked before the request leaves this process: the browser session's cookie
//! AND its `X-Org-Session-Proof` header (always; a search reads DMs and private notes, so it
//! has no sign-in-less mode), every query key at most once and none unknown, `q` present and
//! at most 512 bytes, `limit` 1 to 100, each reference bounded, and no body. An office failure
//! reaches the browser as its fixed code; a storage failure as a bare `503`, never its text.

use hyper::{http::request::Parts, Method, StatusCode};
use org_proto::search::{
    SearchEntityType, SearchHitField, SearchQuery, SearchQueryParams, SEARCH_MAX_CURSOR_BYTES,
    SEARCH_MAX_LIMIT, SEARCH_MAX_PATH_BYTES, SEARCH_MAX_QUERY_BYTES, SEARCH_MAX_REF_BYTES,
};
use org_proto::web::{ReadResponse, WebError, WebErrorCode};
use serde_json::Value;

use super::hq_context::{resolve_hq_office, revalidate_and_resolve_hq};
use super::{failure, invalid, result, unavailable, Call, CallerAuth, Route, SessionApi};
use crate::host::Failure;
use crate::office::OfficeFailure;
use crate::service::{ApiReply, PreparedCall};

pub enum SearchRoute {
    Query(CallerAuth, String, Box<SearchQueryParams>),
}

pub enum SearchCall {
    Query(CallerAuth, String, Box<SearchQueryParams>),
}

fn valid_org_id(id: &str) -> Result<(), ApiReply> {
    org_office_launch::is_storage_id(id)
        .then_some(())
        .ok_or_else(invalid)
}

fn decode_query_value(raw: &str) -> Result<String, ApiReply> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' => {
                let hex = bytes.get(i + 1..i + 3).ok_or_else(invalid)?;
                let text = std::str::from_utf8(hex).map_err(|_| invalid())?;
                out.push(u8::from_str_radix(text, 16).map_err(|_| invalid())?);
                i += 2;
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8(out).map_err(|_| invalid())
}

/// A text value: not blank, without NUL, and at most `max` bytes.
fn bounded(value: String, max: usize) -> Result<String, ApiReply> {
    let ok = !value.trim().is_empty() && value.len() <= max && !value.contains('\0');
    ok.then_some(value).ok_or_else(invalid)
}

fn uuid(value: String) -> Result<String, ApiReply> {
    org_proto::is_uuid_v7(&value)
        .then_some(value)
        .ok_or_else(invalid)
}

/// A comma-separated list of the values `parse` knows, none of them empty.
fn list<T>(value: &str, parse: fn(&str) -> Option<T>) -> Result<Vec<T>, ApiReply> {
    value
        .split(',')
        .map(|item| parse(item.trim()).ok_or_else(invalid))
        .collect()
}

fn flag(value: &str) -> Result<bool, ApiReply> {
    match value {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => Err(invalid()),
    }
}

fn limit(value: &str) -> Result<u32, ApiReply> {
    value
        .parse::<u32>()
        .ok()
        .filter(|limit| (1..=SEARCH_MAX_LIMIT).contains(limit))
        .ok_or_else(invalid)
}

/// `search.query`'s parameters from a query string: every key at most once, none unknown,
/// `q` required, every value within its bound.
fn parse_query(query: Option<&str>) -> Result<SearchQueryParams, ApiReply> {
    let mut params = SearchQueryParams::default();
    let mut seen: Vec<&str> = Vec::new();
    for pair in query
        .unwrap_or_default()
        .split('&')
        .filter(|p| !p.is_empty())
    {
        let (key, raw) = pair.split_once('=').ok_or_else(invalid)?;
        if seen.contains(&key) {
            return Err(invalid());
        }
        seen.push(key);
        let value = decode_query_value(raw)?;
        let reference = |value| bounded(value, SEARCH_MAX_REF_BYTES).map(Some);
        match key {
            "q" => params.q = bounded(value, SEARCH_MAX_QUERY_BYTES)?,
            "types" => params.types = list(&value, SearchEntityType::parse)?,
            "project" => params.project = reference(value)?,
            "channel" => params.channel = reference(value)?,
            "status" => params.status = reference(value)?,
            "assignee" => params.assignee = reference(value)?,
            "label" => params.label = reference(value)?,
            "modified_file" => {
                params.modified_file = Some(bounded(value, SEARCH_MAX_PATH_BYTES)?);
            }
            "author_id" => params.author_id = Some(uuid(value)?),
            "count_only" => params.count_only = flag(&value)?,
            "limit" => params.limit = Some(limit(&value)?),
            "cursor" => params.cursor = Some(bounded(value, SEARCH_MAX_CURSOR_BYTES)?),
            "fields" => params.fields = list(&value, SearchHitField::parse)?,
            _ => return Err(invalid()),
        }
    }
    if params.q.is_empty() {
        return Err(invalid());
    }
    Ok(params)
}

/// Routes `search` below an organization; `None` for any other path.
pub(super) fn try_route(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    tail: &[&str],
) -> Option<Result<Route, ApiReply>> {
    let ["search"] = tail else {
        return None;
    };
    Some((|| {
        // Never the sign-in-less mode: the cookie and its proof header, always.
        let pair = CallerAuth::Pair(api.pair(request)?);
        if !matches!(request.method, Method::GET | Method::HEAD) {
            return Err(invalid());
        }
        valid_org_id(org)?;
        let params = parse_query(request.uri.query())?;
        Ok(Route::Search(SearchRoute::Query(
            pair,
            org.to_owned(),
            Box::new(params),
        )))
    })())
}

pub(super) fn prepare(route: SearchRoute, bytes: &[u8]) -> Result<PreparedCall<Call>, ApiReply> {
    if !bytes.is_empty() {
        return Err(invalid());
    }
    let SearchRoute::Query(pair, org, params) = route;
    Ok(PreparedCall::read(
        Call::Search(SearchCall::Query(pair, org, params)),
        0,
    ))
}

/// An office refusal as the browser sees it: its fixed code and message with the HTTP class;
/// anything else (a storage failure included) is a bare `503`.
fn map_office_failure(error: OfficeFailure) -> ApiReply {
    match error {
        OfficeFailure::BeforeDispatch(e) => failure(e),
        OfficeFailure::Transport => unavailable(),
        OfficeFailure::Refused(err) => {
            let web = WebError::from(err);
            let status = match web.code {
                WebErrorCode::InvalidRequest => StatusCode::BAD_REQUEST,
                WebErrorCode::NotAuthorized => StatusCode::FORBIDDEN,
                WebErrorCode::NotFound => StatusCode::NOT_FOUND,
                WebErrorCode::Conflict => StatusCode::CONFLICT,
                _ => return unavailable(),
            };
            ApiReply::json(
                status,
                serde_json::to_value(ReadResponse::<Value>::Error(web)).unwrap_or(Value::Null),
            )
        }
    }
}

pub(super) async fn execute(api: &SessionApi, call: SearchCall) -> ApiReply {
    let SearchCall::Query(pair, org_id, params) = call;
    let mut office_id = match resolve_hq_office(api, pair, &org_id).await {
        Ok(id) => id,
        Err(Failure::Refused(_)) => return unavailable(),
        Err(error) => return failure(error),
    };
    let auth = api.auth(&pair);
    let mut reply = api
        .offices
        .call_typed::<SearchQuery>(&auth, &office_id, &params, true)
        .await;
    if matches!(
        &reply,
        Err(OfficeFailure::Refused(e)) if e.code == org_proto::ErrorCode::NotAuthorized
    ) {
        office_id = match revalidate_and_resolve_hq(api, pair, &org_id).await {
            Ok(id) => id,
            Err(Failure::Refused(_)) => return unavailable(),
            Err(error) => return failure(error),
        };
        reply = api
            .offices
            .call_typed::<SearchQuery>(&auth, &office_id, &params, true)
            .await;
    }
    match reply {
        Ok(found) => result(found),
        Err(error) => map_office_failure(error),
    }
}

#[cfg(test)]
#[path = "search_routes_tests.rs"]
mod tests;
