//! Browser HTTP routes for private notes, plans and decisions (`TASK-50`, `TASK-111` ADEC-4,
//! `ADR-0031`), forwarded to the organization's HQ office, which alone holds them:
//!
//! - `GET  /api/v1/organizations/{org}/members/{member_id}/decisions` lists a member's private
//!   working decisions;
//! - `GET  /api/v1/organizations/{org}/notes` lists visible private notes across tasks;
//! - `GET  /api/v1/organizations/{org}/notes/{note_id}` reads one;
//! - `POST /api/v1/organizations/{org}/notes/{add|update|remove|publish}` writes;
//! - `GET  /api/v1/organizations/{org}/tasks/{task}/notes` lists a task's visible notes;
//! - `GET  /api/v1/organizations/{org}/tasks/notes/{note_id}` reads one;
//! - `POST /api/v1/organizations/{org}/tasks/notes/{add|update|remove|publish}` writes.
//!
//! Strict `ADR-0031` privacy: every route requires a verified browser session cookie and its
//! proof header (`CallerAuth::Pair`, never sign-in-less mode). The office decides visibility from
//! the caller's own employee: an active person reads every author's notes, an agent only its own,
//! and only the author writes. This slice never widens that, never logs private note content, and
//! never caches an answer.

use hyper::{http::request::Parts, Method, StatusCode};
use org_proto::private_notes::{
    PrivateNoteAddParams, PrivateNoteField, PrivateNoteKind, PrivateNoteListParams,
    PrivateNoteUpdateParams, TasksNotesAdd, TasksNotesGet, TasksNotesGetParams, TasksNotesList,
    TasksNotesPublish, TasksNotesPublishParams, TasksNotesRemove, TasksNotesRemoveParams,
    TasksNotesUpdate, PRIVATE_NOTE_LIST_MAX_LIMIT,
};
use org_proto::web::{ReadResponse, WebError, WebErrorCode};
use serde_json::Value;

use super::hq_context::{resolve_hq_office, revalidate_and_resolve_hq};
use super::{
    failure, invalid, not_found, result, unavailable, Call, CallerAuth, Route, SessionApi,
};
use crate::host::Failure;
use crate::office::OfficeFailure;
use crate::service::{ApiReply, PreparedCall};

pub enum NoteRoute {
    List(CallerAuth, String, Box<PrivateNoteListParams>),
    Get(CallerAuth, String, String),
    Add(CallerAuth, String),
    Update(CallerAuth, String),
    Remove(CallerAuth, String),
    Publish(CallerAuth, String),
}

pub enum NoteCall {
    List(CallerAuth, String, Box<PrivateNoteListParams>),
    Get(CallerAuth, String, String),
    Add(CallerAuth, String, Box<PrivateNoteAddParams>),
    Update(CallerAuth, String, Box<PrivateNoteUpdateParams>),
    Remove(CallerAuth, String, TasksNotesRemoveParams),
    Publish(CallerAuth, String, TasksNotesPublishParams),
}

fn valid_org_id(id: &str) -> Result<(), ApiReply> {
    org_office_launch::is_storage_id(id)
        .then_some(())
        .ok_or_else(invalid)
}

/// A task reference or text filter: non-blank, at most 128 bytes, and free of NUL bytes.
fn valid_ref(value: &str) -> Result<(), ApiReply> {
    if value.trim().is_empty() || value.len() > 128 || value.contains('\0') {
        return Err(invalid());
    }
    Ok(())
}

/// A member, project or note ID: a canonical lowercase UUIDv7.
fn valid_uuid(value: &str) -> Result<(), ApiReply> {
    org_proto::is_uuid_v7(value)
        .then_some(())
        .ok_or_else(invalid)
}

/// A page size within the office's 1 to 500.
fn valid_limit(value: &str) -> Result<u32, ApiReply> {
    value
        .parse()
        .ok()
        .filter(|limit| (1..=PRIVATE_NOTE_LIST_MAX_LIMIT).contains(limit))
        .ok_or_else(invalid)
}

/// `value` after checking it with `check`.
fn checked(value: String, check: fn(&str) -> Result<(), ApiReply>) -> Result<String, ApiReply> {
    check(&value)?;
    Ok(value)
}

fn decode_query_value(raw: &str) -> Result<String, ApiReply> {
    let mut out = Vec::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' => {
                let hex = bytes.get(i + 1..i + 3).ok_or_else(invalid)?;
                let hex = std::str::from_utf8(hex).map_err(|_| invalid())?;
                out.push(u8::from_str_radix(hex, 16).map_err(|_| invalid())?);
                i += 3;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| invalid())
}

/// Which list a query string belongs to: cross-task notes, one task's notes, or a member's
/// decisions.
#[derive(Clone, Copy)]
enum ListScope {
    All,
    Task,
    Decisions,
}

/// Parses a notes list query. `All` accepts every filter; `Task` takes `author_id` and `kind`
/// (its task is the path's); `Decisions` takes `task_id`/`task` (its author and kind are the
/// path's). IDs are canonical UUIDv7s, `limit` is 1 to 500, text filters are 1 to 128 bytes
/// without NUL, and each key comes at most once. Anything else is refused before leaving here.
fn parse_list_query(
    scope: ListScope,
    query: Option<&str>,
    mut params: PrivateNoteListParams,
) -> Result<PrivateNoteListParams, ApiReply> {
    let Some(q) = query.filter(|s| !s.is_empty()) else {
        return Ok(params);
    };
    let mut seen: Vec<&str> = Vec::new();
    for pair in q.split('&') {
        let (key, raw) = pair.split_once('=').ok_or_else(invalid)?;
        if seen.contains(&key) {
            return Err(invalid());
        }
        seen.push(key);
        let value = decode_query_value(raw)?;
        match (scope, key) {
            (ListScope::All | ListScope::Task, "author_id") => {
                params.author_id = Some(checked(value, valid_uuid)?);
            }
            (ListScope::All | ListScope::Task, "kind") => {
                params.kind = Some(PrivateNoteKind::parse(&value).ok_or_else(invalid)?);
            }
            (ListScope::All | ListScope::Decisions, "task_id" | "task") => {
                if params.task.is_some() {
                    return Err(invalid());
                }
                params.task = Some(checked(value, valid_ref)?);
            }
            (_, "project_id") => params.project_id = Some(checked(value, valid_uuid)?),
            (_, "tag") => params.tag = Some(checked(value, valid_ref)?),
            (_, "since") => params.since = Some(checked(value, valid_ref)?),
            (_, "cursor") => params.cursor = Some(checked(value, valid_ref)?),
            (_, "limit") => params.limit = Some(valid_limit(&value)?),
            (_, "fields") => {
                params.fields = value
                    .split(',')
                    .map(|field| PrivateNoteField::parse(field.trim()).ok_or_else(invalid))
                    .collect::<Result<_, _>>()?;
            }
            _ => return Err(invalid()),
        }
    }
    Ok(params)
}

/// The `tasks.notes.list` request of `GET .../members/{member_id}/decisions`: the member's
/// own decisions, whatever the query says.
fn decisions_params(
    member_id: &str,
    query: Option<&str>,
) -> Result<PrivateNoteListParams, ApiReply> {
    valid_uuid(member_id)?;
    let base = PrivateNoteListParams {
        author_id: Some(member_id.to_owned()),
        kind: Some(PrivateNoteKind::Decision),
        ..PrivateNoteListParams::default()
    };
    parse_list_query(ListScope::Decisions, query, base)
}

fn routed(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    build: impl FnOnce(CallerAuth, String) -> Result<NoteRoute, ApiReply>,
) -> Option<Result<Route, ApiReply>> {
    Some((|| {
        // Strict ADR-0031 privacy: always require an authenticated browser session cookie
        // paired with its proof header; never allow sign-in-less mode.
        let pair = CallerAuth::Pair(api.pair(request)?);
        valid_org_id(org)?;
        Ok(Route::Notes(build(pair, org.to_owned())?))
    })())
}

fn route_note_post_action(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    action: &str,
) -> Option<Result<Route, ApiReply>> {
    let build: fn(CallerAuth, String) -> NoteRoute = match action {
        "add" => NoteRoute::Add,
        "update" => NoteRoute::Update,
        "remove" => NoteRoute::Remove,
        "publish" => NoteRoute::Publish,
        _ => return Some(Err(not_found())),
    };
    routed(api, request, org, |pair, org| Ok(build(pair, org)))
}

/// Attempts to route private notes paths under `/api/v1/organizations/{org}/...`.
pub(super) fn try_route(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    tail: &[&str],
    post: bool,
    query: bool,
) -> Option<Result<Route, ApiReply>> {
    let is_get = matches!(request.method, Method::GET | Method::HEAD);
    let raw_query = request.uri.query();
    match tail {
        ["members", member_id, "decisions"] if is_get => routed(api, request, org, |pair, org| {
            let params = decisions_params(member_id, raw_query)?;
            Ok(NoteRoute::List(pair, org, Box::new(params)))
        }),
        ["members", _, "decisions"] => Some(Err(invalid())),
        ["notes"] if is_get => routed(api, request, org, |pair, org| {
            let params =
                parse_list_query(ListScope::All, raw_query, PrivateNoteListParams::default())?;
            Ok(NoteRoute::List(pair, org, Box::new(params)))
        }),
        ["notes"] => Some(Err(invalid())),
        ["notes", action] if post => route_note_post_action(api, request, org, action),
        ["notes", note_id] if is_get && !query => routed(api, request, org, |pair, org| {
            valid_uuid(note_id)?;
            Ok(NoteRoute::Get(pair, org, (*note_id).to_owned()))
        }),
        ["notes", ..] => Some(Err(invalid())),
        ["tasks", "notes", action] if post => route_note_post_action(api, request, org, action),
        ["tasks", "notes", note_id] if is_get && !query => {
            routed(api, request, org, |pair, org| {
                valid_uuid(note_id)?;
                Ok(NoteRoute::Get(pair, org, (*note_id).to_owned()))
            })
        }
        ["tasks", task, "notes"] if is_get => routed(api, request, org, |pair, org| {
            valid_ref(task)?;
            let base = PrivateNoteListParams {
                task: Some((*task).to_owned()),
                ..PrivateNoteListParams::default()
            };
            let params = parse_list_query(ListScope::Task, raw_query, base)?;
            Ok(NoteRoute::List(pair, org, Box::new(params)))
        }),
        _ => None,
    }
}

fn empty(bytes: &[u8]) -> Result<(), ApiReply> {
    if bytes.is_empty() {
        Ok(())
    } else {
        Err(invalid())
    }
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, ApiReply> {
    serde_json::from_slice(bytes).map_err(|_| invalid())
}

pub(super) fn prepare(route: NoteRoute, bytes: &[u8]) -> Result<PreparedCall<Call>, ApiReply> {
    let call = match route {
        NoteRoute::List(c, o, p) => {
            empty(bytes)?;
            NoteCall::List(c, o, p)
        }
        NoteRoute::Get(c, o, id) => {
            empty(bytes)?;
            NoteCall::Get(c, o, id)
        }
        NoteRoute::Add(c, o) => NoteCall::Add(c, o, Box::new(decode(bytes)?)),
        NoteRoute::Update(c, o) => NoteCall::Update(c, o, Box::new(decode(bytes)?)),
        NoteRoute::Remove(c, o) => NoteCall::Remove(c, o, decode(bytes)?),
        NoteRoute::Publish(c, o) => NoteCall::Publish(c, o, decode(bytes)?),
    };
    Ok(PreparedCall::read(Call::Notes(call), bytes.len()))
}

async fn run_office<C: org_proto::Call>(
    api: &SessionApi,
    pair: CallerAuth,
    org_id: &str,
    params: &C::Params,
    is_read: bool,
) -> ApiReply {
    if let Err(error) = api.validate_async(pair).await {
        return failure(error);
    }
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

pub(super) async fn execute(api: &SessionApi, call: NoteCall) -> ApiReply {
    match call {
        NoteCall::List(c, o, p) => run_office::<TasksNotesList>(api, c, &o, &p, true).await,
        NoteCall::Get(c, o, note_id) => {
            run_office::<TasksNotesGet>(api, c, &o, &TasksNotesGetParams { note_id }, true).await
        }
        NoteCall::Add(c, o, p) => run_office::<TasksNotesAdd>(api, c, &o, &p, false).await,
        NoteCall::Update(c, o, p) => run_office::<TasksNotesUpdate>(api, c, &o, &p, false).await,
        NoteCall::Remove(c, o, p) => run_office::<TasksNotesRemove>(api, c, &o, &p, false).await,
        NoteCall::Publish(c, o, p) => run_office::<TasksNotesPublish>(api, c, &o, &p, false).await,
    }
}

#[cfg(test)]
mod tests;
