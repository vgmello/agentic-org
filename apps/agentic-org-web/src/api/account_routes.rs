//! Multi-person account, invitation, password reset, access removal, and ownership/admin
//! handover HTTP routes (`TASK-38` / A6):
//! - `POST /api/v1/links/inspect`
//! - `POST /api/v1/links/register/sign-in` (SIGN-35)
//! - `POST /api/v1/links/set-password`
//! - `POST /api/v1/account/password`
//! - `POST /api/v1/instance/admin/handover`
//! - `GET  /api/v1/organizations/{org}/access`
//! - `POST /api/v1/organizations/{org}/access/invite`
//! - `POST /api/v1/organizations/{org}/access/reset-link`
//! - `POST /api/v1/organizations/{org}/access/remove`
//! - `POST /api/v1/organizations/{org}/owner/handover`

use std::net::IpAddr;

use hyper::StatusCode;
use org_host_control::{
    browser::{SessionValidate, SessionValidateParams},
    browser_accounts::{
        BrowserAdminHandover, BrowserAdminHandoverParams, BrowserChangePassword,
        BrowserChangePasswordParams, BrowserOrgAccessList, BrowserOrgAccessListParams,
        BrowserOrgInvite, BrowserOrgInviteParams, BrowserOrgOwnerHandover,
        BrowserOrgOwnerHandoverParams, BrowserOrgPasswordResetLink,
        BrowserOrgPasswordResetLinkParams, BrowserOrgRemoveAccess, BrowserOrgRemoveAccessParams,
        InspectLinkKind, LinkInspect, LinkInspectParams, LinkRegisterAttach,
        LinkRegisterAttachParams, LinkSetPassword, LinkSetPasswordParams,
    },
    LinkToken, Password,
};
use org_proto::web::{ReadResponse, WebError, WebErrorCode};
use serde::Deserialize;

use super::{
    failure, invalid, not_found, result, signed_out, unavailable, Call, CallerAuth, Credentials,
    SessionApi,
};
use crate::{
    host::Failure,
    service::{ApiReply, PreparedCall},
};

pub enum AccountRoute {
    LinkInspect,
    LinkRegisterAttach(IpAddr),
    LinkSetPassword(IpAddr),
    ChangePassword(CallerAuth),
    AdminHandover(CallerAuth),
    OrgAccessList(CallerAuth, String),
    OrgInvite(CallerAuth, String),
    OrgResetLink(CallerAuth, String),
    OrgRemoveAccess(CallerAuth, String),
    OrgOwnerHandover(CallerAuth, String),
}

pub enum AccountCall {
    LinkInspect(LinkToken, InspectLinkKind),
    LinkRegisterAttach(IpAddr, LinkToken, Credentials),
    LinkSetPassword(IpAddr, LinkToken, Password),
    ChangePassword(CallerAuth, Password, Password),
    AdminHandover(CallerAuth, String, Password),
    OrgAccessList(CallerAuth, String),
    OrgInvite(CallerAuth, String, String),
    OrgResetLink(CallerAuth, String, String),
    OrgRemoveAccess(CallerAuth, String, String),
    OrgOwnerHandover(CallerAuth, String, String, Password),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LinkInspectBody {
    #[serde(deserialize_with = "crate::secret_json::link")]
    link: LinkToken,
    kind: InspectLinkKind,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LinkRegisterAttachBody {
    #[serde(deserialize_with = "crate::secret_json::link")]
    link: LinkToken,
    login: String,
    #[serde(deserialize_with = "crate::secret_json::password")]
    password: Password,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LinkSetPasswordBody {
    #[serde(deserialize_with = "crate::secret_json::link")]
    link: LinkToken,
    #[serde(deserialize_with = "crate::secret_json::password")]
    password: Password,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChangePasswordBody {
    #[serde(deserialize_with = "crate::secret_json::password")]
    current_password: Password,
    #[serde(deserialize_with = "crate::secret_json::password")]
    new_password: Password,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MemberBody {
    member_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OrgOwnerHandoverBody {
    to_member_id: String,
    #[serde(deserialize_with = "crate::secret_json::password")]
    password: Password,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdminHandoverBody {
    to_account_id: String,
    #[serde(deserialize_with = "crate::secret_json::password")]
    password: Password,
}

pub(super) fn prepare(route: AccountRoute, bytes: &[u8]) -> Result<PreparedCall<Call>, ApiReply> {
    let empty = || bytes.is_empty().then_some(()).ok_or_else(invalid);
    let call = match route {
        AccountRoute::LinkInspect => {
            let LinkInspectBody { link, kind } =
                serde_json::from_slice(bytes).map_err(|_| invalid())?;
            AccountCall::LinkInspect(link, kind)
        }
        AccountRoute::LinkRegisterAttach(ip) => {
            let LinkRegisterAttachBody {
                link,
                login,
                password,
            } = serde_json::from_slice(bytes).map_err(|_| invalid())?;
            AccountCall::LinkRegisterAttach(ip, link, Credentials { login, password })
        }
        AccountRoute::LinkSetPassword(ip) => {
            let LinkSetPasswordBody { link, password } =
                serde_json::from_slice(bytes).map_err(|_| invalid())?;
            AccountCall::LinkSetPassword(ip, link, password)
        }
        AccountRoute::ChangePassword(caller) => {
            let ChangePasswordBody {
                current_password,
                new_password,
            } = serde_json::from_slice(bytes).map_err(|_| invalid())?;
            AccountCall::ChangePassword(caller, current_password, new_password)
        }
        AccountRoute::AdminHandover(caller) => {
            let AdminHandoverBody {
                to_account_id,
                password,
            } = serde_json::from_slice(bytes).map_err(|_| invalid())?;
            AccountCall::AdminHandover(caller, to_account_id, password)
        }
        AccountRoute::OrgAccessList(caller, org_id) => {
            empty()?;
            AccountCall::OrgAccessList(caller, org_id)
        }
        AccountRoute::OrgInvite(caller, org_id) => {
            let MemberBody { member_id } = serde_json::from_slice(bytes).map_err(|_| invalid())?;
            if !org_proto::is_valid_member_id(&member_id) {
                return Err(invalid());
            }
            AccountCall::OrgInvite(caller, org_id, member_id)
        }
        AccountRoute::OrgResetLink(caller, org_id) => {
            let MemberBody { member_id } = serde_json::from_slice(bytes).map_err(|_| invalid())?;
            if !org_proto::is_valid_member_id(&member_id) {
                return Err(invalid());
            }
            AccountCall::OrgResetLink(caller, org_id, member_id)
        }
        AccountRoute::OrgRemoveAccess(caller, org_id) => {
            let MemberBody { member_id } = serde_json::from_slice(bytes).map_err(|_| invalid())?;
            if !org_proto::is_valid_member_id(&member_id) {
                return Err(invalid());
            }
            AccountCall::OrgRemoveAccess(caller, org_id, member_id)
        }
        AccountRoute::OrgOwnerHandover(caller, org_id) => {
            let OrgOwnerHandoverBody {
                to_member_id,
                password,
            } = serde_json::from_slice(bytes).map_err(|_| invalid())?;
            if !org_proto::is_valid_member_id(&to_member_id) {
                return Err(invalid());
            }
            AccountCall::OrgOwnerHandover(caller, org_id, to_member_id, password)
        }
    };
    Ok(PreparedCall::read(Call::Account(call), bytes.len()))
}

pub(super) fn execute(api: &SessionApi, call: AccountCall) -> ApiReply {
    match call {
        AccountCall::LinkInspect(link, kind) => api
            .host
            .request::<LinkInspect>(&LinkInspectParams { link, kind })
            .map(result)
            .unwrap_or_else(failure),
        AccountCall::LinkRegisterAttach(client_address, link, credentials) => {
            exec_link_attach(api, client_address, link, credentials)
        }
        AccountCall::LinkSetPassword(client_address, link, password) => api
            .host
            .request::<LinkSetPassword>(&LinkSetPasswordParams {
                link,
                password,
                client_address,
            })
            .map(result)
            .unwrap_or_else(map_account_failure),
        AccountCall::ChangePassword(caller, current_password, new_password) => exec_validated(
            api,
            caller,
            "Current password is incorrect.",
            |auth| {
                api.host
                    .request::<BrowserChangePassword>(&BrowserChangePasswordParams {
                        auth,
                        current_password,
                        new_password,
                    })
            },
            result,
        ),
        AccountCall::AdminHandover(caller, to_account_id, password) => exec_validated(
            api,
            caller,
            "Administrator authorization or password confirmation failed.",
            |auth| {
                api.host
                    .request::<BrowserAdminHandover>(&BrowserAdminHandoverParams {
                        auth,
                        to_account_id,
                        password,
                    })
            },
            result,
        ),
        AccountCall::OrgAccessList(caller, org_id) => exec_validated(
            api,
            caller,
            "Only the organization owner or administrator can view organization access.",
            |auth| {
                api.host
                    .request::<BrowserOrgAccessList>(&BrowserOrgAccessListParams { auth, org_id })
            },
            result,
        ),
        AccountCall::OrgInvite(caller, org_id, member_id) => exec_validated(
            api,
            caller,
            "Only the organization owner or administrator can invite people.",
            |auth| {
                api.host
                    .request::<BrowserOrgInvite>(&BrowserOrgInviteParams {
                        auth,
                        org_id,
                        member_id,
                    })
            },
            issued_link_reply,
        ),
        AccountCall::OrgResetLink(caller, org_id, member_id) => exec_validated(
            api,
            caller,
            "Only the organization owner or administrator can issue sign-in links.",
            |auth| {
                api.host.request::<BrowserOrgPasswordResetLink>(
                    &BrowserOrgPasswordResetLinkParams {
                        auth,
                        org_id,
                        member_id,
                    },
                )
            },
            issued_link_reply,
        ),
        AccountCall::OrgRemoveAccess(caller, org_id, member_id) => exec_validated(
            api,
            caller,
            "Only the organization owner or administrator can remove access.",
            |auth| {
                api.host
                    .request::<BrowserOrgRemoveAccess>(&BrowserOrgRemoveAccessParams {
                        auth,
                        org_id,
                        member_id,
                    })
            },
            result,
        ),
        AccountCall::OrgOwnerHandover(caller, org_id, to_member_id, password) => exec_validated(
            api,
            caller,
            "Organization owner authorization or password confirmation failed.",
            |auth| {
                api.host
                    .request::<BrowserOrgOwnerHandover>(&BrowserOrgOwnerHandoverParams {
                        auth,
                        org_id,
                        to_member_id,
                        password,
                    })
            },
            result,
        ),
    }
}

fn exec_link_attach(
    api: &SessionApi,
    client_address: IpAddr,
    link: LinkToken,
    credentials: Credentials,
) -> ApiReply {
    api.establish(|pair| {
        api.host.request_then::<LinkRegisterAttach>(
            &LinkRegisterAttachParams {
                link,
                login: credentials.login,
                password: credentials.password,
                client_address,
                cookie_digest: pair.cookie_digest,
                proof_digest: pair.proof_digest,
                origin: api.origin.clone(),
            },
            api.caching(pair),
        )
    })
}

fn exec_validated<T>(
    api: &SessionApi,
    caller: CallerAuth,
    forbidden_message: &'static str,
    run: impl FnOnce(org_host_control::organization::BrowserAuth) -> Result<T, Failure>,
    ok: impl FnOnce(T) -> ApiReply,
) -> ApiReply {
    if let Err(error) = api.validate(caller) {
        return failure(error);
    }
    match run(api.auth(&caller)) {
        Ok(value) => ok(value),
        Err(Failure::Denied) => handle_denied(api, caller, forbidden_message),
        Err(error) => map_account_failure(error),
    }
}

fn issued_link_reply(res: org_host_control::browser_accounts::IssuedLinkResult) -> ApiReply {
    result(serde_json::json!({
        "url": res.url.expose(),
        "expires_at_ms": res.expires_at_ms,
    }))
}

fn handle_denied(
    api: &SessionApi,
    caller: CallerAuth,
    forbidden_message: &'static str,
) -> ApiReply {
    match caller {
        CallerAuth::LocalAuthless => {
            api.set_auth_enabled(true);
            signed_out()
        }
        CallerAuth::Pair(pair) => {
            let recheck = api.host.request_then::<SessionValidate>(
                &SessionValidateParams {
                    cookie_digest: pair.cookie_digest,
                    proof_digest: pair.proof_digest,
                    origin: api.origin.clone(),
                },
                api.caching(&pair),
            );
            match recheck {
                Ok(_) => ApiReply::error(
                    StatusCode::FORBIDDEN,
                    WebErrorCode::NotAuthorized,
                    forbidden_message,
                ),
                Err(error) => failure(error),
            }
        }
    }
}

fn map_account_failure(err: Failure) -> ApiReply {
    match err {
        Failure::NotFound => not_found(),
        Failure::Refused(error) => {
            let host = WebError::from(*error);
            match host.code {
                WebErrorCode::Conflict => ApiReply::error(
                    StatusCode::CONFLICT,
                    WebErrorCode::Conflict,
                    "This action conflicts with the current account state.",
                ),
                WebErrorCode::InvalidRequest => {
                    if let (Some(field), Some(rule)) = (host.field, host.rule) {
                        let mut web = WebError::new(
                            WebErrorCode::InvalidRequest,
                            "This doesn't meet the sign-in rules.",
                        );
                        web.field = Some(field);
                        web.rule = Some(rule);
                        ApiReply::json(
                            StatusCode::BAD_REQUEST,
                            serde_json::to_value(ReadResponse::<()>::Error(web))
                                .expect("gateway error serializes"),
                        )
                    } else {
                        invalid()
                    }
                }
                WebErrorCode::RateLimited if host.retry_after_ms.is_some() => {
                    let mut web = WebError::new(
                        WebErrorCode::RateLimited,
                        "Too many attempts. Try again later.",
                    );
                    web.retry_after_ms = host.retry_after_ms;
                    ApiReply::json(
                        StatusCode::TOO_MANY_REQUESTS,
                        serde_json::to_value(ReadResponse::<()>::Error(web))
                            .expect("gateway error serializes"),
                    )
                }
                _ => unavailable(),
            }
        }
        other => failure(other),
    }
}
