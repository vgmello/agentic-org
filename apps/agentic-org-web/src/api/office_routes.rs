//! Browser HTTP routes for HQ satellite office invitations, certificate revocation/removal,
//! and multi-office sync status (`TASK-52`, `FC-020`, `FC-024`, `FC-145`).
//!
//! All mutations use `POST` (`agentic-org-web` ingress policy).

use hyper::{http::request::Parts, StatusCode};
use org_proto::federation::{
    OfficeInvitationActionParams, OfficeInvitationApprove, OfficeInvitationApproveParams,
    OfficeInvitationCancel, OfficeInvitationCreate, OfficeInvitationCreateParams,
    OfficeInvitationDecline, OfficeInvitationList, OfficeInvitationListParams, OfficeRemove,
    OfficeRemoveParams, OfficeSyncStatus, OfficeSyncStatusParams,
};
use org_proto::web::{ReadResponse, WebError, WebErrorCode};
use serde::Deserialize;
use serde_json::Value;

use super::hq_context::{resolve_hq_office, revalidate_and_resolve_hq};
use super::{failure, invalid, result, unavailable, Call, CallerAuth, Route, SessionApi};
use crate::host::Failure;
use crate::office::OfficeFailure;
use crate::service::{ApiReply, PreparedCall};

pub enum OfficeFederationRoute {
    InvitationList(CallerAuth, String),
    InvitationCreate(CallerAuth, String),
    InvitationApprove(CallerAuth, String, String),
    InvitationDecline(CallerAuth, String, String),
    InvitationCancel(CallerAuth, String, String),
    OfficeRemove(CallerAuth, String, String),
    SyncStatus(CallerAuth, String),
}

pub enum OfficeFederationCall {
    InvitationList(CallerAuth, String),
    InvitationCreate(CallerAuth, String, OfficeInvitationCreateParams),
    InvitationApprove(CallerAuth, String, OfficeInvitationApproveParams),
    InvitationDecline(CallerAuth, String, OfficeInvitationActionParams),
    InvitationCancel(CallerAuth, String, OfficeInvitationActionParams),
    OfficeRemove(CallerAuth, String, OfficeRemoveParams),
    SyncStatus(CallerAuth, String),
}

fn valid_id(id: &str) -> Result<(), ApiReply> {
    org_office_launch::is_storage_id(id)
        .then_some(())
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
    match tail {
        ["offices", "invitations"] if get => Some(route_simple(
            api,
            request,
            org,
            query,
            OfficeFederationRoute::InvitationList,
        )),
        ["offices", "invitations"] if post => Some(route_simple(
            api,
            request,
            org,
            query,
            OfficeFederationRoute::InvitationCreate,
        )),
        ["offices", "invitations", id, "approve"] if post => Some(route_with_id(
            api,
            request,
            org,
            id,
            query,
            OfficeFederationRoute::InvitationApprove,
        )),
        ["offices", "invitations", id, "decline"] if post => Some(route_with_id(
            api,
            request,
            org,
            id,
            query,
            OfficeFederationRoute::InvitationDecline,
        )),
        ["offices", "invitations", id, "cancel"] if post => Some(route_with_id(
            api,
            request,
            org,
            id,
            query,
            OfficeFederationRoute::InvitationCancel,
        )),
        ["offices", office_id, "remove"] if post => Some(route_with_id(
            api,
            request,
            org,
            office_id,
            query,
            OfficeFederationRoute::OfficeRemove,
        )),
        ["offices", "sync-status"] if get => Some(route_simple(
            api,
            request,
            org,
            query,
            OfficeFederationRoute::SyncStatus,
        )),
        _ => None,
    }
}

fn route_simple(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    query: bool,
    ctor: fn(CallerAuth, String) -> OfficeFederationRoute,
) -> Result<Route, ApiReply> {
    if query {
        return Err(invalid());
    }
    let pair = api.caller_auth(request)?;
    valid_id(org)?;
    Ok(Route::OfficeFederation(ctor(pair, org.to_owned())))
}

fn route_with_id(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    target_id: &str,
    query: bool,
    ctor: fn(CallerAuth, String, String) -> OfficeFederationRoute,
) -> Result<Route, ApiReply> {
    if query {
        return Err(invalid());
    }
    let pair = api.caller_auth(request)?;
    valid_id(org)?;
    valid_id(target_id)?;
    Ok(Route::OfficeFederation(ctor(
        pair,
        org.to_owned(),
        target_id.to_owned(),
    )))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApproveBody {
    check_code: String,
}

fn decode_body<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, ApiReply> {
    serde_json::from_slice(bytes).map_err(|_| invalid())
}

pub(super) fn prepare(
    route: OfficeFederationRoute,
    bytes: &[u8],
) -> Result<PreparedCall<Call>, ApiReply> {
    let call = match route {
        OfficeFederationRoute::InvitationList(pair, org) => {
            if !bytes.is_empty() {
                return Err(invalid());
            }
            OfficeFederationCall::InvitationList(pair, org)
        }
        OfficeFederationRoute::InvitationCreate(pair, org) => {
            OfficeFederationCall::InvitationCreate(pair, org, decode_body(bytes)?)
        }
        OfficeFederationRoute::InvitationApprove(pair, org, invitation_id) => {
            let body: ApproveBody = decode_body(bytes)?;
            OfficeFederationCall::InvitationApprove(
                pair,
                org,
                OfficeInvitationApproveParams {
                    invitation_id,
                    check_code: body.check_code,
                },
            )
        }
        OfficeFederationRoute::InvitationDecline(pair, org, invitation_id) => {
            if !bytes.is_empty() && bytes != b"{}" {
                return Err(invalid());
            }
            OfficeFederationCall::InvitationDecline(
                pair,
                org,
                OfficeInvitationActionParams { invitation_id },
            )
        }
        OfficeFederationRoute::InvitationCancel(pair, org, invitation_id) => {
            if !bytes.is_empty() && bytes != b"{}" {
                return Err(invalid());
            }
            OfficeFederationCall::InvitationCancel(
                pair,
                org,
                OfficeInvitationActionParams { invitation_id },
            )
        }
        OfficeFederationRoute::OfficeRemove(pair, org, office_id) => {
            if !bytes.is_empty() && bytes != b"{}" {
                return Err(invalid());
            }
            OfficeFederationCall::OfficeRemove(pair, org, OfficeRemoveParams { office_id })
        }
        OfficeFederationRoute::SyncStatus(pair, org) => {
            if !bytes.is_empty() {
                return Err(invalid());
            }
            OfficeFederationCall::SyncStatus(pair, org)
        }
    };
    Ok(PreparedCall::read(
        Call::OfficeFederation(call),
        bytes.len(),
    ))
}

fn map_office_failure(error: OfficeFailure) -> ApiReply {
    match error {
        OfficeFailure::BeforeDispatch(e) => failure(e),
        OfficeFailure::Transport => unavailable(),
        OfficeFailure::Refused(err) => {
            let web = WebError::from(err);
            let status = match web.code {
                WebErrorCode::InvalidRequest | WebErrorCode::InvalidName => StatusCode::BAD_REQUEST,
                WebErrorCode::NotAuthorized => StatusCode::FORBIDDEN,
                WebErrorCode::NotFound => StatusCode::NOT_FOUND,
                WebErrorCode::Conflict | WebErrorCode::AlreadyExists => StatusCode::CONFLICT,
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

async fn run_hq<C: org_proto::Call>(
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

pub(super) async fn execute(api: &SessionApi, call: OfficeFederationCall) -> ApiReply {
    match call {
        OfficeFederationCall::InvitationList(pair, org) => {
            run_hq::<OfficeInvitationList>(api, pair, &org, &OfficeInvitationListParams {}, true)
                .await
        }
        OfficeFederationCall::InvitationCreate(pair, org, params) => {
            run_hq::<OfficeInvitationCreate>(api, pair, &org, &params, false).await
        }
        OfficeFederationCall::InvitationApprove(pair, org, params) => {
            run_hq::<OfficeInvitationApprove>(api, pair, &org, &params, false).await
        }
        OfficeFederationCall::InvitationDecline(pair, org, params) => {
            run_hq::<OfficeInvitationDecline>(api, pair, &org, &params, false).await
        }
        OfficeFederationCall::InvitationCancel(pair, org, params) => {
            run_hq::<OfficeInvitationCancel>(api, pair, &org, &params, false).await
        }
        OfficeFederationCall::OfficeRemove(pair, org, params) => {
            run_hq::<OfficeRemove>(api, pair, &org, &params, false).await
        }
        OfficeFederationCall::SyncStatus(pair, org) => {
            run_hq::<OfficeSyncStatus>(api, pair, &org, &OfficeSyncStatusParams {}, true).await
        }
    }
}
