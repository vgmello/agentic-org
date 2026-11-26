//! Browser HTTP routes for C7 repository registration, project attachment, cache status,
//! isolated task worktrees, and office Git push (`TASK-51`, `doc-35`).

use hyper::{http::request::Parts, Method, StatusCode};
use org_proto::git::{
    GitPush, GitPushParams, RepoAttach, RepoAttachParams, RepoList, RepoListParams, RepoRegister,
    RepoRegisterParams, RepoStatus, RepoStatusParams, WorktreeList, WorktreeListParams,
    WorktreePrepare, WorktreePrepareParams,
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

pub enum GitRoute {
    RepoList(CallerAuth, String, RepoListParams),
    RepoRegister(CallerAuth, String),
    RepoAttach(CallerAuth, String),
    RepoStatus(CallerAuth, String, String),
    WorktreeList(CallerAuth, String, WorktreeListParams),
    WorktreePrepare(CallerAuth, String),
    GitPush(CallerAuth, String),
}

pub enum GitCall {
    RepoList(CallerAuth, String, RepoListParams),
    RepoRegister(CallerAuth, String, RepoRegisterParams),
    RepoAttach(CallerAuth, String, RepoAttachParams),
    RepoStatus(CallerAuth, String, String),
    WorktreeList(CallerAuth, String, WorktreeListParams),
    WorktreePrepare(CallerAuth, String, WorktreePrepareParams),
    GitPush(CallerAuth, String, GitPushParams),
}

fn valid_org_id(id: &str) -> Result<(), ApiReply> {
    org_office_launch::is_storage_id(id)
        .then_some(())
        .ok_or_else(invalid)
}

fn valid_ref_segment(raw: &str) -> Result<(), ApiReply> {
    if raw.is_empty() || raw.len() > 128 {
        return Err(invalid());
    }
    Ok(())
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

fn parse_single_query(query: Option<&str>, expected_key: &str) -> Result<Option<String>, ApiReply> {
    let Some(q) = query else {
        return Ok(None);
    };
    if q.is_empty() {
        return Ok(None);
    }
    let mut value = None;
    for pair in q.split('&') {
        let Some((k, v)) = pair.split_once('=') else {
            return Err(invalid());
        };
        if k != expected_key || value.is_some() {
            return Err(invalid());
        }
        let decoded = decode_query_value(v)?;
        if !decoded.is_empty() {
            value = Some(decoded);
        }
    }
    Ok(value)
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
        ["repositories"] if is_get => Some((|| {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            let project = parse_single_query(request.uri.query(), "project")?;
            Ok(Route::Git(GitRoute::RepoList(
                pair,
                org.to_owned(),
                RepoListParams { project },
            )))
        })()),
        ["repositories"] if post => Some((|| {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            Ok(Route::Git(GitRoute::RepoRegister(pair, org.to_owned())))
        })()),
        ["repositories", "attach"] if post => Some((|| {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            Ok(Route::Git(GitRoute::RepoAttach(pair, org.to_owned())))
        })()),
        ["repositories", repo, "status"] if is_get && !query => Some((|| {
            if *repo == "attach" {
                return Err(invalid());
            }
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            valid_ref_segment(repo)?;
            Ok(Route::Git(GitRoute::RepoStatus(
                pair,
                org.to_owned(),
                (*repo).to_owned(),
            )))
        })()),
        ["worktrees"] if is_get => Some((|| {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            let task = parse_single_query(request.uri.query(), "task")?;
            Ok(Route::Git(GitRoute::WorktreeList(
                pair,
                org.to_owned(),
                WorktreeListParams { task },
            )))
        })()),
        ["worktrees", "prepare"] if post => Some((|| {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            Ok(Route::Git(GitRoute::WorktreePrepare(pair, org.to_owned())))
        })()),
        ["git", "push"] if post => Some((|| {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            Ok(Route::Git(GitRoute::GitPush(pair, org.to_owned())))
        })()),
        ["repositories"]
        | ["repositories", "attach"]
        | ["repositories", _, "status"]
        | ["worktrees"]
        | ["worktrees", "prepare"]
        | ["git", "push"] => Some(Err(invalid())),
        ["repositories", ..] | ["worktrees", ..] | ["git", ..] => Some(Err(not_found())),
        _ => None,
    }
}

fn decode_body<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, ApiReply> {
    serde_json::from_slice(bytes).map_err(|_| invalid())
}

pub(super) fn prepare(route: GitRoute, bytes: &[u8]) -> Result<PreparedCall<Call>, ApiReply> {
    let empty = || bytes.is_empty().then_some(()).ok_or_else(invalid);
    let call = match route {
        GitRoute::RepoList(pair, org, params) => {
            empty()?;
            GitCall::RepoList(pair, org, params)
        }
        GitRoute::RepoRegister(pair, org) => GitCall::RepoRegister(pair, org, decode_body(bytes)?),
        GitRoute::RepoAttach(pair, org) => GitCall::RepoAttach(pair, org, decode_body(bytes)?),
        GitRoute::RepoStatus(pair, org, repo) => {
            empty()?;
            GitCall::RepoStatus(pair, org, repo)
        }
        GitRoute::WorktreeList(pair, org, params) => {
            empty()?;
            GitCall::WorktreeList(pair, org, params)
        }
        GitRoute::WorktreePrepare(pair, org) => {
            GitCall::WorktreePrepare(pair, org, decode_body(bytes)?)
        }
        GitRoute::GitPush(pair, org) => GitCall::GitPush(pair, org, decode_body(bytes)?),
    };
    Ok(PreparedCall::read(Call::Git(call), bytes.len()))
}

pub(super) async fn execute(api: &SessionApi, call: GitCall) -> ApiReply {
    match call {
        GitCall::RepoList(pair, org, params) => {
            run_office::<RepoList>(api, pair, &org, &params, true).await
        }
        GitCall::RepoRegister(pair, org, params) => {
            run_office::<RepoRegister>(api, pair, &org, &params, false).await
        }
        GitCall::RepoAttach(pair, org, params) => {
            run_office::<RepoAttach>(api, pair, &org, &params, false).await
        }
        GitCall::RepoStatus(pair, org, repo) => {
            run_office::<RepoStatus>(
                api,
                pair,
                &org,
                &RepoStatusParams { repository: repo },
                true,
            )
            .await
        }
        GitCall::WorktreeList(pair, org, params) => {
            run_office::<WorktreeList>(api, pair, &org, &params, true).await
        }
        GitCall::WorktreePrepare(pair, org, params) => {
            run_office::<WorktreePrepare>(api, pair, &org, &params, false).await
        }
        GitCall::GitPush(pair, org, params) => {
            run_office::<GitPush>(api, pair, &org, &params, false).await
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

#[cfg(test)]
mod tests {
    use super::*;
    use org_proto::{ApiError, ErrorCode};

    #[test]
    fn query_decoding_and_office_error_mapping_preserve_specific_not_found() {
        assert_eq!(
            parse_single_query(Some("project=caf%C3%A9+core"), "project")
                .ok()
                .flatten()
                .as_deref(),
            Some("café core")
        );
        assert!(parse_single_query(Some("other=1"), "project").is_err());
        assert!(parse_single_query(Some("project=%ZZ"), "project").is_err());
        assert!(parse_single_query(Some("project=a&project=b"), "project").is_err());

        let nf = map_office_failure(OfficeFailure::Refused(ApiError::new(
            ErrorCode::NotFound,
            "integration branch `release` does not exist on the repository origin",
        )));
        assert_eq!(nf.status(), StatusCode::NOT_FOUND);
        let nf_body = std::str::from_utf8(nf.body_bytes()).unwrap();
        assert!(
            nf_body.contains("integration branch `release`"),
            "NotFound reply should preserve office message: {nf_body}"
        );

        let na = map_office_failure(OfficeFailure::Refused(ApiError::new(
            ErrorCode::NotAuthorized,
            "owner only",
        )));
        assert_eq!(na.status(), StatusCode::FORBIDDEN);
    }
}
