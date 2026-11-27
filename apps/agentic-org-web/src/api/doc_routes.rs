//! Browser HTTP routes for the project documentation tree (`TASK-55`, `L11`, `FC-180`), each
//! forwarded to the organization's HQ office as the signed-in member, whose access the office
//! decides:
//!
//! - `GET    /api/v1/organizations/{org}/projects/{project}/docs` (`docs.list`; `section`,
//!   `kind`, `status`, `limit`, `cursor` query parameters)
//! - `GET    .../projects/{project}/docs/index` (`docs.index`)
//! - `GET    .../projects/{project}/docs/{doc_id}` (`docs.inspect`)
//! - `POST   .../projects/{project}/docs` (`docs.create`)
//! - `PATCH  .../projects/{project}/docs/{doc_id}` (`docs.update`)
//! - `POST   .../projects/{project}/docs/{doc_id}/move` (`docs.move`)
//! - `POST   .../projects/{project}/docs/{doc_id}/delete` (`docs.delete`)
//! - `POST   .../projects/{project}/docs/media` (`docs.put_media`)
//!
//! The project and document in the path always win over any in the body.

use hyper::{http::request::Parts, Method, StatusCode};
use org_proto::docs::{
    DocStatusWire, DocsCreate, DocsCreateParams, DocsDelete, DocsDeleteParams, DocsIndex,
    DocsIndexParams, DocsInspect, DocsInspectParams, DocsList, DocsListParams, DocsMove,
    DocsMoveParams, DocsPutMedia, DocsPutMediaParams, DocsUpdate, DocsUpdateParams,
    DOC_KIND_MAX_BYTES, DOC_LIST_MAX_LIMIT, DOC_PATH_MAX_BYTES, DOC_REF_MAX_BYTES,
    DOC_SECTION_MAX_BYTES,
};
use org_proto::web::{ReadResponse, WebError, WebErrorCode};
use serde_json::Value;

use super::hq_context::{resolve_hq_office, revalidate_and_resolve_hq};
use super::{failure, invalid, result, unavailable, Call, CallerAuth, Route, SessionApi};
use crate::host::Failure;
use crate::office::OfficeFailure;
use crate::service::{ApiReply, PreparedCall};

pub enum DocRoute {
    List(CallerAuth, String, DocsListParams),
    Index(CallerAuth, String, String),
    Inspect(CallerAuth, String, String, String),
    Create(CallerAuth, String, String),
    Update(CallerAuth, String, String, String),
    Move(CallerAuth, String, String, String),
    Delete(CallerAuth, String, String, String),
    PutMedia(CallerAuth, String, String),
}

pub enum DocCall {
    List(CallerAuth, String, DocsListParams),
    Index(CallerAuth, String, DocsIndexParams),
    Inspect(CallerAuth, String, DocsInspectParams),
    Create(CallerAuth, String, DocsCreateParams),
    Update(CallerAuth, String, DocsUpdateParams),
    Move(CallerAuth, String, DocsMoveParams),
    Delete(CallerAuth, String, DocsDeleteParams),
    PutMedia(CallerAuth, String, DocsPutMediaParams),
}

pub(super) fn valid_org_id(id: &str) -> Result<(), ApiReply> {
    org_office_launch::is_storage_id(id)
        .then_some(())
        .ok_or_else(invalid)
}

/// A project reference in a path: its UUIDv7, `P-<number>` or slug.
pub(super) fn valid_project(project: &str) -> Result<String, ApiReply> {
    let ok = !project.is_empty()
        && project.len() <= 128
        && project
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-');
    ok.then(|| project.to_owned()).ok_or_else(invalid)
}

pub(super) fn valid_doc_id(doc: &str) -> Result<String, ApiReply> {
    org_proto::is_uuid_v7(doc)
        .then(|| doc.to_owned())
        .ok_or_else(invalid)
}

pub(super) fn decode_query_value(raw: &str) -> Result<String, ApiReply> {
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

fn parse_status(raw: &str) -> Result<DocStatusWire, ApiReply> {
    match raw {
        "draft" => Ok(DocStatusWire::Draft),
        "current" => Ok(DocStatusWire::Current),
        "deprecated" => Ok(DocStatusWire::Deprecated),
        _ => Err(invalid()),
    }
}

/// A query value of at most `max` bytes.
fn bounded(value: String, max: usize) -> Result<String, ApiReply> {
    (value.len() <= max).then_some(value).ok_or_else(invalid)
}

/// A page size from 1 to [`DOC_LIST_MAX_LIMIT`].
fn parse_limit(raw: &str) -> Result<u32, ApiReply> {
    raw.parse::<u32>()
        .ok()
        .filter(|limit| (1..=DOC_LIST_MAX_LIMIT).contains(limit))
        .ok_or_else(invalid)
}

/// A write's `command_id`: a canonical lowercase UUIDv7, checked before the office is asked.
pub(super) fn valid_command_id(command_id: &str) -> Result<(), ApiReply> {
    org_proto::is_uuid_v7(command_id)
        .then_some(())
        .ok_or_else(invalid)
}

/// `docs.list`'s filters from the query string: each key at most once, none unknown, each
/// value within its field's bound (`section` 256, `kind` 64, `cursor` 512 bytes; `limit` 1 to
/// 200).
fn parse_list_query(project: String, query: Option<&str>) -> Result<DocsListParams, ApiReply> {
    let mut params = DocsListParams {
        project,
        ..Default::default()
    };
    let Some(query) = query.filter(|q| !q.is_empty()) else {
        return Ok(params);
    };
    let mut seen: Vec<&str> = Vec::new();
    for pair in query.split('&') {
        let (key, raw) = pair.split_once('=').ok_or_else(invalid)?;
        if seen.contains(&key) {
            return Err(invalid());
        }
        seen.push(key);
        let value = decode_query_value(raw)?;
        if value.len() > DOC_REF_MAX_BYTES {
            return Err(invalid());
        }
        match key {
            "section" => params.section = Some(bounded(value, DOC_SECTION_MAX_BYTES)?),
            "kind" => params.kind = Some(bounded(value, DOC_KIND_MAX_BYTES)?),
            "status" => params.status = Some(parse_status(&value)?),
            "limit" => params.limit = Some(parse_limit(&value)?),
            "cursor" => params.cursor = Some(bounded(value, DOC_PATH_MAX_BYTES)?),
            _ => return Err(invalid()),
        }
    }
    Ok(params)
}

/// Routes `projects/{project}/docs/...` below an organization; `None` for any other path.
pub(super) fn try_route(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    tail: &[&str],
) -> Option<Result<Route, ApiReply>> {
    let ["projects", project, "docs", rest @ ..] = tail else {
        return None;
    };
    let query = request.uri.query().is_some();
    let method = &request.method;
    let get = matches!(*method, Method::GET | Method::HEAD);
    let body = !query && matches!(*method, Method::POST | Method::PATCH);
    let route = |make: &dyn Fn(CallerAuth, String, String) -> Result<DocRoute, ApiReply>| {
        let pair = api.caller_auth(request)?;
        valid_org_id(org)?;
        let project = valid_project(project)?;
        Ok(Route::Docs(make(pair, org.to_owned(), project)?))
    };
    Some(match rest {
        [] if get => route(&|pair, org, project| {
            Ok(DocRoute::List(
                pair,
                org,
                parse_list_query(project, request.uri.query())?,
            ))
        }),
        [] if body && *method == Method::POST => {
            route(&|pair, org, project| Ok(DocRoute::Create(pair, org, project)))
        }
        ["index"] if get && !query => {
            route(&|pair, org, project| Ok(DocRoute::Index(pair, org, project)))
        }
        ["media"] if body && *method == Method::POST => {
            route(&|pair, org, project| Ok(DocRoute::PutMedia(pair, org, project)))
        }
        [doc] if get && !query => route(&|pair, org, project| {
            Ok(DocRoute::Inspect(pair, org, project, valid_doc_id(doc)?))
        }),
        [doc] if body && *method == Method::PATCH => route(&|pair, org, project| {
            Ok(DocRoute::Update(pair, org, project, valid_doc_id(doc)?))
        }),
        [doc, "move"] if body && *method == Method::POST => {
            route(&|pair, org, project| Ok(DocRoute::Move(pair, org, project, valid_doc_id(doc)?)))
        }
        [doc, "delete"] if body && *method == Method::POST => route(&|pair, org, project| {
            Ok(DocRoute::Delete(pair, org, project, valid_doc_id(doc)?))
        }),
        [] | ["index"] | ["media"] | [_] | [_, "move" | "delete"] => Err(invalid()),
        _ => Err(super::not_found()),
    })
}

pub(super) fn decode_body<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, ApiReply> {
    serde_json::from_slice(bytes).map_err(|_| invalid())
}

pub(super) fn prepare(route: DocRoute, bytes: &[u8]) -> Result<PreparedCall<Call>, ApiReply> {
    let call = to_call(route, bytes)?;
    Ok(PreparedCall::read(Call::Docs(call), bytes.len()))
}

/// The office call a route and its body make: reads take no body, writes a JSON one.
fn to_call(route: DocRoute, bytes: &[u8]) -> Result<DocCall, ApiReply> {
    let empty = || bytes.is_empty().then_some(()).ok_or_else(invalid);
    Ok(match route {
        DocRoute::List(pair, org, params) => {
            empty()?;
            DocCall::List(pair, org, params)
        }
        DocRoute::Index(pair, org, project) => {
            empty()?;
            DocCall::Index(pair, org, DocsIndexParams { project })
        }
        DocRoute::Inspect(pair, org, project, document) => {
            empty()?;
            DocCall::Inspect(pair, org, DocsInspectParams { project, document })
        }
        DocRoute::Create(pair, org, project) => {
            let mut params: DocsCreateParams = decode_body(bytes)?;
            valid_command_id(&params.command_id)?;
            params.project = project;
            DocCall::Create(pair, org, params)
        }
        DocRoute::Update(pair, org, project, document_id) => {
            let mut params: DocsUpdateParams = decode_body(bytes)?;
            valid_command_id(&params.command_id)?;
            (params.project, params.document_id) = (project, document_id);
            DocCall::Update(pair, org, params)
        }
        DocRoute::Move(pair, org, project, document_id) => {
            let mut params: DocsMoveParams = decode_body(bytes)?;
            valid_command_id(&params.command_id)?;
            (params.project, params.document_id) = (project, document_id);
            DocCall::Move(pair, org, params)
        }
        DocRoute::Delete(pair, org, project, document_id) => {
            let mut params: DocsDeleteParams = decode_body(bytes)?;
            valid_command_id(&params.command_id)?;
            (params.project, params.document_id) = (project, document_id);
            DocCall::Delete(pair, org, params)
        }
        DocRoute::PutMedia(pair, org, project) => {
            let mut params: DocsPutMediaParams = decode_body(bytes)?;
            valid_command_id(&params.command_id)?;
            params.project = project;
            DocCall::PutMedia(pair, org, params)
        }
    })
}

/// An office refusal as the browser sees it: its code and message, with the HTTP class.
pub(super) fn map_office_failure(error: OfficeFailure) -> ApiReply {
    match error {
        OfficeFailure::BeforeDispatch(e) => failure(e),
        OfficeFailure::Transport => unavailable(),
        OfficeFailure::Refused(err) => {
            let web = WebError::from(err);
            let status = match web.code {
                WebErrorCode::InvalidRequest => StatusCode::BAD_REQUEST,
                WebErrorCode::NotAuthorized => StatusCode::FORBIDDEN,
                WebErrorCode::NotFound => StatusCode::NOT_FOUND,
                WebErrorCode::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
                WebErrorCode::Conflict
                | WebErrorCode::AlreadyExists
                | WebErrorCode::OperationReused => StatusCode::CONFLICT,
                _ => return unavailable(),
            };
            ApiReply::json(
                status,
                serde_json::to_value(ReadResponse::<Value>::Error(web)).unwrap_or(Value::Null),
            )
        }
    }
}

pub(super) async fn run_office<C: org_proto::Call>(
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

pub(super) async fn execute(api: &SessionApi, call: DocCall) -> ApiReply {
    match call {
        DocCall::List(pair, org, params) => {
            run_office::<DocsList>(api, pair, &org, &params, true).await
        }
        DocCall::Index(pair, org, params) => {
            run_office::<DocsIndex>(api, pair, &org, &params, true).await
        }
        DocCall::Inspect(pair, org, params) => {
            run_office::<DocsInspect>(api, pair, &org, &params, true).await
        }
        DocCall::Create(pair, org, params) => {
            run_office::<DocsCreate>(api, pair, &org, &params, false).await
        }
        DocCall::Update(pair, org, params) => {
            run_office::<DocsUpdate>(api, pair, &org, &params, false).await
        }
        DocCall::Move(pair, org, params) => {
            run_office::<DocsMove>(api, pair, &org, &params, false).await
        }
        DocCall::Delete(pair, org, params) => {
            run_office::<DocsDelete>(api, pair, &org, &params, false).await
        }
        DocCall::PutMedia(pair, org, params) => {
            run_office::<DocsPutMedia>(api, pair, &org, &params, false).await
        }
    }
}

#[cfg(test)]
#[path = "doc_routes_tests.rs"]
mod tests;
