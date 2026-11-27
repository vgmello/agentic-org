//! HTTP BFF routes for GitHub App integration (`TASK-272`):
//! - Signed external webhook ingress (`POST /api/v1/organizations/{org}/integrations/github/webhook`)
//!   requiring zero browser cookie while keeping CSRF/Origin/MediaType gates intact on all
//!   browser-authenticated management and read routes.
//! - Owner-only GitHub App config/status/revoke and repository bind/unbind routes.
//! - Project-scoped PR list, inspect, CI failure analysis, and on-demand sync routes.

use hyper::{http::request::Parts, Method, StatusCode};
use org_proto::github::{
    GitHubAppConfigure, GitHubAppConfigureParams, GitHubAppRevoke, GitHubAppStatus, GitHubPrChecks,
    GitHubPrChecksParams, GitHubPrInspect, GitHubPrInspectParams, GitHubPrList, GitHubPrListParams,
    GitHubPrState, GitHubPrSync, GitHubPrSyncParams, GitHubRepoBind, GitHubRepoBindParams,
    GitHubRepoUnbind, GitHubRepoUnbindParams, GitHubWebhookIngest, GitHubWebhookIngestParams,
    SecretText, GITHUB_PR_LIST_MAX_LIMIT, GITHUB_WEBHOOK_MAX_BYTES,
};
use org_proto::web::{ReadResponse, WebError, WebErrorCode};
use org_proto::Empty;
use serde::Deserialize;
use serde_json::Value;

use super::hq_context::{resolve_hq_office, revalidate_and_resolve_hq};
use super::{
    failure, invalid, not_found, result, unavailable, Call, CallerAuth, Route, SessionApi,
};
use crate::host::Failure;
use crate::office::OfficeFailure;
use crate::service::{ApiReply, PreparedCall};

#[cfg(test)]
mod tests;

pub enum GitHubRoute {
    Webhook {
        org: String,
        event: String,
        delivery_id: String,
        signature_256: SecretText,
    },
    AppStatus(CallerAuth, String),
    AppConfigure(CallerAuth, String),
    AppRevoke(CallerAuth, String),
    RepoBind(CallerAuth, String),
    RepoUnbind(CallerAuth, String),
    PrList(CallerAuth, String, GitHubPrListParams),
    PrInspect(CallerAuth, String, GitHubPrInspectParams),
    PrChecks(CallerAuth, String, GitHubPrChecksParams),
    PrSync(CallerAuth, String, String),
}

pub enum GitHubCall {
    Webhook(String, GitHubWebhookIngestParams),
    AppStatus(CallerAuth, String),
    AppConfigure(CallerAuth, String, GitHubAppConfigureParams),
    AppRevoke(CallerAuth, String),
    RepoBind(CallerAuth, String, GitHubRepoBindParams),
    RepoUnbind(CallerAuth, String, GitHubRepoUnbindParams),
    PrList(CallerAuth, String, GitHubPrListParams),
    PrInspect(CallerAuth, String, GitHubPrInspectParams),
    PrChecks(CallerAuth, String, GitHubPrChecksParams),
    PrSync(CallerAuth, String, GitHubPrSyncParams),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PrSyncBody {
    repository: String,
    #[serde(default)]
    pr_number: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyObject {}

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

fn single_header(request: &Parts, name: &'static str, max_len: usize) -> Result<String, ApiReply> {
    let mut values = request.headers.get_all(name).iter();
    let (Some(val), None) = (values.next(), values.next()) else {
        return Err(webhook_unauth());
    };
    let text = val.to_str().map_err(|_| webhook_unauth())?.trim();
    if text.is_empty() || text.len() > max_len {
        return Err(webhook_unauth());
    }
    Ok(text.to_owned())
}

fn webhook_unauth() -> ApiReply {
    ApiReply::error(
        StatusCode::UNAUTHORIZED,
        WebErrorCode::NotAuthorized,
        "Webhook authentication failed.",
    )
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

fn parse_pr_list_query(project: &str, query: Option<&str>) -> Result<GitHubPrListParams, ApiReply> {
    let mut params = GitHubPrListParams {
        project: project.to_owned(),
        ..Default::default()
    };
    let Some(q) = query.filter(|q| !q.is_empty()) else {
        return Ok(params);
    };
    for pair in q.split('&') {
        let (k, v) = pair.split_once('=').ok_or_else(invalid)?;
        let val = decode_query_value(v)?;
        match k {
            "repository" if params.repository.is_none() => {
                if !val.is_empty() {
                    params.repository = Some(val);
                }
            }
            "task" if params.task.is_none() => {
                if !val.is_empty() {
                    params.task = Some(val);
                }
            }
            "state" if params.state.is_none() => {
                params.state = Some(match val.as_str() {
                    "open" => GitHubPrState::Open,
                    "closed" => GitHubPrState::Closed,
                    "merged" => GitHubPrState::Merged,
                    _ => return Err(invalid()),
                });
            }
            "limit" if params.limit.is_none() => {
                let n: u32 = val.parse().map_err(|_| invalid())?;
                if n == 0 || n > GITHUB_PR_LIST_MAX_LIMIT {
                    return Err(invalid());
                }
                params.limit = Some(n);
            }
            _ => return Err(invalid()),
        }
    }
    Ok(params)
}

fn parse_pr_inspect_query(
    project: &str,
    pr_number: u64,
    query: Option<&str>,
) -> Result<GitHubPrInspectParams, ApiReply> {
    let mut repository = None;
    if let Some(q) = query.filter(|q| !q.is_empty()) {
        for pair in q.split('&') {
            let (k, v) = pair.split_once('=').ok_or_else(invalid)?;
            if k != "repository" || repository.is_some() {
                return Err(invalid());
            }
            let val = decode_query_value(v)?;
            if !val.is_empty() {
                repository = Some(val);
            }
        }
    }
    Ok(GitHubPrInspectParams {
        project: project.to_owned(),
        repository,
        pr_number,
    })
}

fn parse_pr_checks_query(
    project: &str,
    pr_number: u64,
    query: Option<&str>,
) -> Result<GitHubPrChecksParams, ApiReply> {
    let mut repository = None;
    let mut only_failures = None;
    if let Some(q) = query.filter(|q| !q.is_empty()) {
        for pair in q.split('&') {
            let (k, v) = pair.split_once('=').ok_or_else(invalid)?;
            let val = decode_query_value(v)?;
            match k {
                "repository" if repository.is_none() => {
                    if !val.is_empty() {
                        repository = Some(val);
                    }
                }
                "only_failures" if only_failures.is_none() => {
                    only_failures = Some(match val.as_str() {
                        "true" | "1" => true,
                        "false" | "0" => false,
                        _ => return Err(invalid()),
                    });
                }
                _ => return Err(invalid()),
            }
        }
    }
    Ok(GitHubPrChecksParams {
        project: project.to_owned(),
        repository,
        pr_number,
        only_failures: only_failures.unwrap_or(false),
    })
}

fn parse_pr_number(raw: &str) -> Result<u64, ApiReply> {
    raw.parse::<u64>()
        .ok()
        .filter(|&n| n > 0)
        .ok_or_else(invalid)
}

pub(super) fn try_route(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    tail: &[&str],
    get: bool,
    post: bool,
    query: bool,
) -> Option<Result<Route, ApiReply>> {
    let is_get = matches!(request.method, Method::GET | Method::HEAD);
    match tail {
        ["integrations", "github", "webhook"] if post => Some((|| {
            valid_org_id(org)?;
            let event = single_header(request, "x-github-event", 64)?;
            let delivery_id = single_header(request, "x-github-delivery", 64)?;
            let signature_256 =
                SecretText::new(single_header(request, "x-hub-signature-256", 128)?);
            Ok(Route::GitHub(GitHubRoute::Webhook {
                org: org.to_owned(),
                event,
                delivery_id,
                signature_256,
            }))
        })()),
        ["integrations", "github", "status"] if get => Some((|| {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            Ok(Route::GitHub(GitHubRoute::AppStatus(pair, org.to_owned())))
        })()),
        ["integrations", "github", "configure"] if post => Some((|| {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            Ok(Route::GitHub(GitHubRoute::AppConfigure(
                pair,
                org.to_owned(),
            )))
        })()),
        ["integrations", "github", "revoke"] if post => Some((|| {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            Ok(Route::GitHub(GitHubRoute::AppRevoke(pair, org.to_owned())))
        })()),
        ["integrations", "github", "repos", "bind"] if post => Some((|| {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            Ok(Route::GitHub(GitHubRoute::RepoBind(pair, org.to_owned())))
        })()),
        ["integrations", "github", "repos", "unbind"] if post => Some((|| {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            Ok(Route::GitHub(GitHubRoute::RepoUnbind(pair, org.to_owned())))
        })()),
        ["projects", project, "github", "prs"] if is_get => Some((|| {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            valid_ref_segment(project)?;
            let params = parse_pr_list_query(project, request.uri.query())?;
            Ok(Route::GitHub(GitHubRoute::PrList(
                pair,
                org.to_owned(),
                params,
            )))
        })()),
        ["projects", project, "github", "prs", "sync"] if post => Some((|| {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            valid_ref_segment(project)?;
            Ok(Route::GitHub(GitHubRoute::PrSync(
                pair,
                org.to_owned(),
                (*project).to_owned(),
            )))
        })()),
        ["projects", project, "github", "prs", number] if is_get => Some((|| {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            valid_ref_segment(project)?;
            let pr_number = parse_pr_number(number)?;
            let params = parse_pr_inspect_query(project, pr_number, request.uri.query())?;
            Ok(Route::GitHub(GitHubRoute::PrInspect(
                pair,
                org.to_owned(),
                params,
            )))
        })()),
        ["projects", project, "github", "prs", number, "checks"] if is_get => Some((|| {
            let pair = api.caller_auth(request)?;
            valid_org_id(org)?;
            valid_ref_segment(project)?;
            let pr_number = parse_pr_number(number)?;
            let params = parse_pr_checks_query(project, pr_number, request.uri.query())?;
            Ok(Route::GitHub(GitHubRoute::PrChecks(
                pair,
                org.to_owned(),
                params,
            )))
        })()),
        ["integrations", "github", "webhook" | "status" | "configure" | "revoke"]
        | ["integrations", "github", "repos", "bind" | "unbind"]
        | ["projects", _, "github", "prs"]
        | ["projects", _, "github", "prs", "sync"]
        | ["projects", _, "github", "prs", _]
        | ["projects", _, "github", "prs", _, "checks"] => {
            let _ = query;
            Some(Err(invalid()))
        }
        ["integrations", "github", ..] | ["projects", _, "github", ..] => Some(Err(not_found())),
        _ => None,
    }
}

fn decode_body<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, ApiReply> {
    serde_json::from_slice(bytes).map_err(|_| invalid())
}

pub(super) fn prepare(route: GitHubRoute, bytes: &[u8]) -> Result<PreparedCall<Call>, ApiReply> {
    let empty = || bytes.is_empty().then_some(()).ok_or_else(invalid);
    let call = match route {
        GitHubRoute::Webhook {
            org,
            event,
            delivery_id,
            signature_256,
        } => {
            if bytes.len() > GITHUB_WEBHOOK_MAX_BYTES {
                return Err(ApiReply::error(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    WebErrorCode::TooLarge,
                    "Webhook payload is too large.",
                ));
            }
            let payload = std::str::from_utf8(bytes)
                .map_err(|_| invalid())?
                .to_owned();
            GitHubCall::Webhook(
                org,
                GitHubWebhookIngestParams {
                    event,
                    delivery_id,
                    signature_256,
                    payload,
                },
            )
        }
        GitHubRoute::AppStatus(pair, org) => {
            empty()?;
            GitHubCall::AppStatus(pair, org)
        }
        GitHubRoute::AppConfigure(pair, org) => {
            GitHubCall::AppConfigure(pair, org, decode_body(bytes)?)
        }
        GitHubRoute::AppRevoke(pair, org) => {
            if !bytes.is_empty() {
                let _: EmptyObject = decode_body(bytes)?;
            }
            GitHubCall::AppRevoke(pair, org)
        }
        GitHubRoute::RepoBind(pair, org) => GitHubCall::RepoBind(pair, org, decode_body(bytes)?),
        GitHubRoute::RepoUnbind(pair, org) => {
            GitHubCall::RepoUnbind(pair, org, decode_body(bytes)?)
        }
        GitHubRoute::PrList(pair, org, params) => {
            empty()?;
            GitHubCall::PrList(pair, org, params)
        }
        GitHubRoute::PrInspect(pair, org, params) => {
            empty()?;
            GitHubCall::PrInspect(pair, org, params)
        }
        GitHubRoute::PrChecks(pair, org, params) => {
            empty()?;
            GitHubCall::PrChecks(pair, org, params)
        }
        GitHubRoute::PrSync(pair, org, project) => {
            let body: PrSyncBody = decode_body(bytes)?;
            GitHubCall::PrSync(
                pair,
                org,
                GitHubPrSyncParams {
                    project,
                    repository: body.repository,
                    pr_number: body.pr_number,
                },
            )
        }
    };
    Ok(PreparedCall::read(Call::GitHub(call), bytes.len()))
}

pub(super) async fn execute(api: &SessionApi, call: GitHubCall) -> ApiReply {
    match call {
        GitHubCall::Webhook(org, params) => execute_webhook(api, &org, &params).await,
        GitHubCall::AppStatus(pair, org) => {
            run_office::<GitHubAppStatus>(api, pair, &org, &Empty {}, true).await
        }
        GitHubCall::AppConfigure(pair, org, params) => {
            run_office::<GitHubAppConfigure>(api, pair, &org, &params, false).await
        }
        GitHubCall::AppRevoke(pair, org) => {
            run_office::<GitHubAppRevoke>(api, pair, &org, &Empty {}, false).await
        }
        GitHubCall::RepoBind(pair, org, params) => {
            run_office::<GitHubRepoBind>(api, pair, &org, &params, false).await
        }
        GitHubCall::RepoUnbind(pair, org, params) => {
            run_office::<GitHubRepoUnbind>(api, pair, &org, &params, false).await
        }
        GitHubCall::PrList(pair, org, params) => {
            run_office::<GitHubPrList>(api, pair, &org, &params, true).await
        }
        GitHubCall::PrInspect(pair, org, params) => {
            run_office::<GitHubPrInspect>(api, pair, &org, &params, true).await
        }
        GitHubCall::PrChecks(pair, org, params) => {
            run_office::<GitHubPrChecks>(api, pair, &org, &params, true).await
        }
        GitHubCall::PrSync(pair, org, params) => {
            run_office::<GitHubPrSync>(api, pair, &org, &params, false).await
        }
    }
}

async fn execute_webhook(
    api: &SessionApi,
    org_id: &str,
    params: &GitHubWebhookIngestParams,
) -> ApiReply {
    match api
        .offices
        .call_webhook_typed::<GitHubWebhookIngest>(org_id, params)
        .await
    {
        Ok(res) => result(res),
        Err(OfficeFailure::BeforeDispatch(Failure::Denied | Failure::NotFound)) => webhook_unauth(),
        Err(OfficeFailure::Refused(err))
            if matches!(
                err.code,
                org_proto::ErrorCode::NotAuthorized | org_proto::ErrorCode::NotFound
            ) =>
        {
            webhook_unauth()
        }
        Err(OfficeFailure::Refused(err)) if err.code == org_proto::ErrorCode::TooLarge => {
            ApiReply::error(
                StatusCode::PAYLOAD_TOO_LARGE,
                WebErrorCode::TooLarge,
                "Webhook payload is too large.",
            )
        }
        Err(OfficeFailure::Refused(err)) if err.code == org_proto::ErrorCode::InvalidRequest => {
            ApiReply::json(
                StatusCode::BAD_REQUEST,
                serde_json::to_value(ReadResponse::<Value>::Error(WebError::from(err)))
                    .unwrap_or(Value::Null),
            )
        }
        Err(_) => unavailable(),
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
                WebErrorCode::RateLimited => StatusCode::TOO_MANY_REQUESTS,
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
