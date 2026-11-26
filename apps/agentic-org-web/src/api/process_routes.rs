//! Browser HTTP routes for C5 organization processes, standards, and built-in skills
//! (`TASK-44`, `doc-34`, `FC-110`, `FC-111`, `FC-112`).

use hyper::{http::request::Parts, Method, StatusCode};
use org_proto::processes::{
    DocumentKindWire, ProcessEffective, ProcessEffectiveParams, ProcessForWire, ProcessScopeWire,
    ProcessShow, ProcessShowParams, ProcessView, ProcessViewParams, ProcessWhy, ProcessWhyParams,
    ProcessWrite, ProcessWriteParams, SkillList, SkillListParams, SkillRead, SkillReadParams,
};
use org_proto::web::{ReadResponse, WebError, WebErrorCode};
use serde_json::Value;

use super::hq_context::{resolve_hq_office, revalidate_and_resolve_hq};
use super::{failure, invalid, result, unavailable, Call, CallerAuth, Route, SessionApi};
use crate::host::Failure;
use crate::office::OfficeFailure;
use crate::service::{ApiReply, PreparedCall};

pub enum ProcessRoute {
    View(CallerAuth, String, ProcessViewParams),
    Effective(CallerAuth, String, ProcessEffectiveParams),
    Show(CallerAuth, String, ProcessShowParams),
    Why(CallerAuth, String, ProcessWhyParams),
    Create(CallerAuth, String),
    Update(CallerAuth, String, String),
    SkillList(CallerAuth, String),
    SkillRead(CallerAuth, String, String),
}

pub enum ProcessCall {
    View(CallerAuth, String, ProcessViewParams),
    Effective(CallerAuth, String, ProcessEffectiveParams),
    Show(CallerAuth, String, ProcessShowParams),
    Why(CallerAuth, String, ProcessWhyParams),
    Write(CallerAuth, String, ProcessWriteParams),
    SkillList(CallerAuth, String),
    SkillRead(CallerAuth, String, String),
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

fn parse_query_map(query: Option<&str>) -> Result<Vec<(String, String)>, ApiReply> {
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

fn parse_kind_param(raw: &str) -> Result<DocumentKindWire, ApiReply> {
    match raw {
        "process" => Ok(DocumentKindWire::Process),
        "standard" => Ok(DocumentKindWire::Standard),
        _ => Err(invalid()),
    }
}

fn parse_view_query(query: Option<&str>) -> Result<ProcessViewParams, ApiReply> {
    let pairs = parse_query_map(query)?;
    let mut level: Option<String> = None;
    let mut team_id: Option<String> = None;
    let mut project_id: Option<String> = None;
    let mut kind: Option<DocumentKindWire> = None;
    for (k, v) in pairs {
        match k.as_str() {
            "level" => level = Some(v),
            "team_id" => team_id = Some(v),
            "project_id" => project_id = Some(v),
            "kind" => kind = Some(parse_kind_param(&v)?),
            _ => return Err(invalid()),
        }
    }
    let scope = match (level.as_deref(), team_id, project_id) {
        (None | Some("organization"), None, None) => ProcessScopeWire::Organization,
        (None | Some("team"), Some(tid), None) => ProcessScopeWire::Team { team_id: tid },
        (None | Some("project"), None, Some(pid)) => ProcessScopeWire::Project { project_id: pid },
        _ => return Err(invalid()),
    };
    Ok(ProcessViewParams { scope, kind })
}

fn parse_effective_query(query: Option<&str>) -> Result<ProcessEffectiveParams, ApiReply> {
    let pairs = parse_query_map(query)?;
    let mut employee_id: Option<String> = None;
    let mut team_id: Option<String> = None;
    let mut project_id: Option<String> = None;
    let mut task_id: Option<String> = None;
    let mut kind: Option<DocumentKindWire> = None;
    for (k, v) in pairs {
        match k.as_str() {
            "employee_id" => employee_id = Some(v),
            "team_id" => team_id = Some(v),
            "project_id" => project_id = Some(v),
            "task_id" => task_id = Some(v),
            "kind" => kind = Some(parse_kind_param(&v)?),
            _ => return Err(invalid()),
        }
    }
    let target_for = parse_target_for(team_id, project_id, task_id)?;
    Ok(ProcessEffectiveParams {
        employee_id,
        r#for: target_for,
        kind,
    })
}

fn parse_target_for(
    team_id: Option<String>,
    project_id: Option<String>,
    task_id: Option<String>,
) -> Result<Option<ProcessForWire>, ApiReply> {
    match (team_id, project_id, task_id) {
        (None, None, None) => Ok(None),
        (Some(tid), None, None) => Ok(Some(ProcessForWire::Team { team_id: tid })),
        (None, Some(pid), None) => Ok(Some(ProcessForWire::Project { project_id: pid })),
        (None, None, Some(tid)) => Ok(Some(ProcessForWire::Task { task_id: tid })),
        _ => Err(invalid()),
    }
}

fn parse_why_query(query: Option<&str>, key: String) -> Result<ProcessWhyParams, ApiReply> {
    let pairs = parse_query_map(query)?;
    let mut employee_id: Option<String> = None;
    let mut team_id: Option<String> = None;
    let mut project_id: Option<String> = None;
    let mut task_id: Option<String> = None;
    for (k, v) in pairs {
        match k.as_str() {
            "employee_id" => employee_id = Some(v),
            "team_id" => team_id = Some(v),
            "project_id" => project_id = Some(v),
            "task_id" => task_id = Some(v),
            _ => return Err(invalid()),
        }
    }
    let target_for = parse_target_for(team_id, project_id, task_id)?;
    Ok(ProcessWhyParams {
        key,
        employee_id,
        r#for: target_for,
    })
}

fn parse_show_query(query: Option<&str>, id_or_key: &str) -> Result<ProcessShowParams, ApiReply> {
    let pairs = parse_query_map(query)?;
    let mut employee_id: Option<String> = None;
    let mut team_id: Option<String> = None;
    let mut project_id: Option<String> = None;
    let mut task_id: Option<String> = None;
    let mut offset: Option<usize> = None;
    let mut expected_revision: Option<u64> = None;
    for (k, v) in pairs {
        match k.as_str() {
            "employee_id" => employee_id = Some(v),
            "team_id" => team_id = Some(v),
            "project_id" => project_id = Some(v),
            "task_id" => task_id = Some(v),
            "offset" => offset = Some(v.parse::<usize>().map_err(|_| invalid())?),
            "revision" => expected_revision = Some(v.parse::<u64>().map_err(|_| invalid())?),
            _ => return Err(invalid()),
        }
    }
    let target_for = parse_target_for(team_id, project_id, task_id)?;
    let (key, document_id) = if org_proto::is_uuid_v7(id_or_key) {
        (None, Some(id_or_key.to_owned()))
    } else {
        (Some(id_or_key.to_owned()), None)
    };
    if expected_revision.is_some() && document_id.is_none() {
        return Err(invalid());
    }
    Ok(ProcessShowParams {
        key,
        document_id,
        employee_id,
        r#for: target_for,
        offset,
        revision: expected_revision,
    })
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
    let is_get = matches!(request.method, Method::GET | Method::HEAD);
    match tail {
        ["processes"] if is_get => Some((|| {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            let params = parse_view_query(request.uri.query())?;
            Ok(Route::Process(ProcessRoute::View(
                pair,
                org.to_owned(),
                params,
            )))
        })()),
        ["processes"] if post && !query => Some((|| {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            Ok(Route::Process(ProcessRoute::Create(pair, org.to_owned())))
        })()),
        ["processes", "effective"] if is_get => Some((|| {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            let params = parse_effective_query(request.uri.query())?;
            Ok(Route::Process(ProcessRoute::Effective(
                pair,
                org.to_owned(),
                params,
            )))
        })()),
        ["processes", key, "why"] if is_get => Some((|| {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            let params = parse_why_query(request.uri.query(), (*key).to_owned())?;
            Ok(Route::Process(ProcessRoute::Why(
                pair,
                org.to_owned(),
                params,
            )))
        })()),
        ["processes", id_or_key] if is_get => Some((|| {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            let params = parse_show_query(request.uri.query(), id_or_key)?;
            Ok(Route::Process(ProcessRoute::Show(
                pair,
                org.to_owned(),
                params,
            )))
        })()),
        ["processes", doc_id] if post && !query => Some((|| {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            if !org_proto::is_uuid_v7(doc_id) {
                return Err(invalid());
            }
            Ok(Route::Process(ProcessRoute::Update(
                pair,
                org.to_owned(),
                (*doc_id).to_owned(),
            )))
        })()),
        ["skills"] if is_get && !query => Some((|| {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            Ok(Route::Process(ProcessRoute::SkillList(
                pair,
                org.to_owned(),
            )))
        })()),
        ["skills", name] if is_get && !query => Some((|| {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            Ok(Route::Process(ProcessRoute::SkillRead(
                pair,
                org.to_owned(),
                (*name).to_owned(),
            )))
        })()),
        _ => None,
    }
}

fn decode_body<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, ApiReply> {
    serde_json::from_slice(bytes).map_err(|_| invalid())
}

pub(super) fn prepare(route: ProcessRoute, bytes: &[u8]) -> Result<PreparedCall<Call>, ApiReply> {
    let empty = || bytes.is_empty().then_some(()).ok_or_else(invalid);
    let call = match route {
        ProcessRoute::View(pair, org, params) => {
            empty()?;
            ProcessCall::View(pair, org, params)
        }
        ProcessRoute::Effective(pair, org, params) => {
            empty()?;
            ProcessCall::Effective(pair, org, params)
        }
        ProcessRoute::Show(pair, org, params) => {
            empty()?;
            ProcessCall::Show(pair, org, params)
        }
        ProcessRoute::Why(pair, org, params) => {
            empty()?;
            ProcessCall::Why(pair, org, params)
        }
        ProcessRoute::Create(pair, org) => ProcessCall::Write(pair, org, decode_body(bytes)?),
        ProcessRoute::Update(pair, org, doc_id) => {
            let mut params: ProcessWriteParams = decode_body(bytes)?;
            params.document_id = Some(doc_id);
            ProcessCall::Write(pair, org, params)
        }
        ProcessRoute::SkillList(pair, org) => {
            empty()?;
            ProcessCall::SkillList(pair, org)
        }
        ProcessRoute::SkillRead(pair, org, name) => {
            empty()?;
            ProcessCall::SkillRead(pair, org, name)
        }
    };
    Ok(PreparedCall::read(Call::Process(call), bytes.len()))
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

pub(super) async fn execute(api: &SessionApi, call: ProcessCall) -> ApiReply {
    match call {
        ProcessCall::View(pair, org, params) => {
            run_office::<ProcessView>(api, pair, &org, &params, true).await
        }
        ProcessCall::Effective(pair, org, params) => {
            run_office::<ProcessEffective>(api, pair, &org, &params, true).await
        }
        ProcessCall::Show(pair, org, params) => {
            run_office::<ProcessShow>(api, pair, &org, &params, true).await
        }
        ProcessCall::Why(pair, org, params) => {
            run_office::<ProcessWhy>(api, pair, &org, &params, true).await
        }
        ProcessCall::Write(pair, org, params) => {
            run_office::<ProcessWrite>(api, pair, &org, &params, false).await
        }
        ProcessCall::SkillList(pair, org) => {
            run_office::<SkillList>(api, pair, &org, &SkillListParams {}, true).await
        }
        ProcessCall::SkillRead(pair, org, name) => {
            run_office::<SkillRead>(api, pair, &org, &SkillReadParams { name }, true).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use org_proto::{ApiError, ErrorCode};

    #[test]
    fn process_query_parsing_and_office_error_mapping() {
        let view = parse_view_query(Some(
            "level=project&project_id=0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a1&kind=standard",
        ))
        .ok()
        .expect("parse view query");
        assert_eq!(
            view.scope,
            ProcessScopeWire::Project {
                project_id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a1".into()
            }
        );
        assert_eq!(view.kind, Some(DocumentKindWire::Standard));
        assert!(parse_view_query(Some("bad_pair_without_equals")).is_err());

        let eff = parse_effective_query(Some(
            "project_id=0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a1&kind=process",
        ))
        .ok()
        .expect("parse effective query");
        assert_eq!(
            eff.r#for,
            Some(ProcessForWire::Project {
                project_id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a1".into()
            })
        );
        assert_eq!(eff.kind, Some(DocumentKindWire::Process));

        let show = parse_show_query(
            Some("project_id=0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a1&offset=128"),
            "code-review",
        )
        .ok()
        .expect("parse show query with offset");
        assert_eq!(show.offset, Some(128));
        assert_eq!(show.key.as_deref(), Some("code-review"));
        assert!(parse_show_query(Some("kind=standard"), "code-review").is_err());

        let why = parse_why_query(
            Some("project_id=0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a1"),
            "code-review".into(),
        )
        .ok()
        .expect("parse why query");
        assert_eq!(why.key, "code-review");
        assert!(parse_why_query(Some("kind=process"), "code-review".into()).is_err());

        let sealed = map_office_failure(OfficeFailure::Refused(ApiError::new(
            ErrorCode::InvalidRequest,
            "sealed: non-overridable standard cannot be overridden",
        )));
        assert_eq!(sealed.status(), StatusCode::BAD_REQUEST);
        assert!(std::str::from_utf8(sealed.body_bytes())
            .unwrap()
            .contains("sealed"));

        let conflict = map_office_failure(OfficeFailure::Refused(ApiError::new(
            ErrorCode::AlreadyExists,
            "duplicate key in scope",
        )));
        assert_eq!(conflict.status(), StatusCode::CONFLICT);

        let forbidden = map_office_failure(OfficeFailure::Refused(ApiError::new(
            ErrorCode::NotAuthorized,
            "agents may only write overridable project documents",
        )));
        assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
    }
}
