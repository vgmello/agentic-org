//! Browser HTTP routes for opening an existing organization from Git files (`TASK-39` / R1):
//! - `POST /api/v1/organizations/open/preview`
//! - `POST /api/v1/organizations/open`

use hyper::StatusCode;
use org_host_control::organization::{
    OpenExisting, OpenExistingParams, OpenPreview, OpenPreviewParams,
};
use org_proto::selected_bootstrap::CreatorV1;
use org_proto::web::{organizations, WebError, WebErrorCode};
use serde::Deserialize;

use super::account_routes::handle_denied;
use super::{bad_input, failure, invalid, result, Call, CallerAuth, SessionApi};
use crate::host::Failure;
use crate::service::{ApiReply, PreparedCall};

pub enum OpenRoute {
    Preview(CallerAuth),
    Open(CallerAuth),
}

pub enum OpenCall {
    Preview(CallerAuth, String, String),
    Open(CallerAuth, OpenRequestBody),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviewRequestBody {
    repository_remote_url: String,
    state_branch: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenRequestBody {
    pub operation_id: String,
    pub repository_remote_url: String,
    pub state_branch: String,
    pub owner_member_id: String,
    #[serde(default)]
    pub new_owner_person: Option<CreatorV1>,
}

pub(super) fn prepare(route: OpenRoute, bytes: &[u8]) -> Result<PreparedCall<Call>, ApiReply> {
    match route {
        OpenRoute::Preview(caller) => {
            let body: PreviewRequestBody = serde_json::from_slice(bytes).map_err(|_| invalid())?;
            Ok(PreparedCall::read(
                Call::OpenOrg(OpenCall::Preview(
                    caller,
                    body.repository_remote_url,
                    body.state_branch,
                )),
                bytes.len(),
            ))
        }
        OpenRoute::Open(caller) => {
            let body: OpenRequestBody = serde_json::from_slice(bytes).map_err(|_| invalid())?;
            let id = organizations::host_operation_id(&body.operation_id).map_err(bad_input)?;
            Ok(PreparedCall::mutation(
                Call::OpenOrg(OpenCall::Open(caller, body)),
                id,
                bytes.len(),
            ))
        }
    }
}

pub(super) fn execute(api: &SessionApi, call: OpenCall) -> ApiReply {
    match call {
        OpenCall::Preview(caller, repository_remote_url, state_branch) => {
            if let Err(error) = api.validate(caller) {
                return failure(error);
            }
            match api.host.request::<OpenPreview>(&OpenPreviewParams {
                auth: api.auth(&caller),
                repository_remote_url,
                state_branch,
            }) {
                Ok(res) => result(res),
                Err(Failure::Denied) => map_denied(api, caller),
                Err(error) => map_open_failure(error),
            }
        }
        OpenCall::Open(caller, body) => {
            if let Err(error) = api.validate(caller) {
                return failure(error);
            }
            match api.host.request::<OpenExisting>(&OpenExistingParams {
                auth: api.auth(&caller),
                operation_id: body.operation_id,
                repository_remote_url: body.repository_remote_url,
                state_branch: body.state_branch,
                owner_member_id: body.owner_member_id,
                new_owner_person: body.new_owner_person,
            }) {
                Ok(res) => result(res),
                Err(Failure::Denied) => map_denied(api, caller),
                Err(error) => map_open_failure(error),
            }
        }
    }
}

fn map_denied(api: &SessionApi, caller: CallerAuth) -> ApiReply {
    handle_denied(
        api,
        caller,
        "Only an instance administrator can open an existing organization.",
    )
}

fn map_open_failure(error: Failure) -> ApiReply {
    match error {
        Failure::Refused(err) => {
            let web = WebError::from((*err).clone());
            let (status, message, field, rule) = match web.code {
                WebErrorCode::InvalidRequest => {
                    (StatusCode::BAD_REQUEST, err.message, err.field, err.rule)
                }
                WebErrorCode::AlreadyExists
                | WebErrorCode::Conflict
                | WebErrorCode::OperationReused => {
                    (StatusCode::CONFLICT, err.message, err.field, err.rule)
                }
                WebErrorCode::NotAuthorized => (
                    StatusCode::FORBIDDEN,
                    "Only an instance administrator can open an existing organization.".into(),
                    None,
                    None,
                ),
                WebErrorCode::NotFound => (StatusCode::NOT_FOUND, err.message, None, None),
                WebErrorCode::RateLimited => (
                    StatusCode::TOO_MANY_REQUESTS,
                    "Too many requests; try again later.".into(),
                    None,
                    None,
                ),
                WebErrorCode::Unavailable | WebErrorCode::StorageError | WebErrorCode::NotReady => {
                    (
                        StatusCode::SERVICE_UNAVAILABLE,
                        "The organization could not be opened right now. Try again.".into(),
                        None,
                        None,
                    )
                }
                _ => (
                    StatusCode::BAD_REQUEST,
                    "Invalid open-organization request.".into(),
                    None,
                    None,
                ),
            };
            let mut body = serde_json::Map::new();
            body.insert(
                "code".into(),
                serde_json::to_value(&web.code).expect("code serializes"),
            );
            body.insert("message".into(), serde_json::Value::String(message));
            if let (Some(field), Some(rule)) = (field, rule) {
                body.insert("field".into(), serde_json::Value::String(field));
                body.insert("rule".into(), serde_json::Value::String(rule));
            }
            ApiReply::json(status, serde_json::json!({ "error": body }))
        }
        other => failure(other),
    }
}
