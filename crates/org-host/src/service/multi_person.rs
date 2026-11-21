//! Browser multi-person account handlers on the admitted web process control connection
//! (`TASK-38` / A6): link inspection, existing-account invite attach (SIGN-35), set-password
//! link redemption, own password change, org access listing, invites, reset links, access
//! removal, organization owner handover, and instance administrator handover.

use org_host_control::browser::SessionValidateParams;
use org_host_control::browser_accounts::{
    BrowserAdminHandover, BrowserChangePassword, BrowserOrgAccessList, BrowserOrgAccessListResult,
    BrowserOrgInvite, BrowserOrgOwnerHandover, BrowserOrgPasswordResetLink, BrowserOrgRemoveAccess,
    IssuedLinkResult, LinkInspect, LinkRegisterAttach, LinkSetPassword,
};
use org_host_control::OneTimeLinkUrl;
use org_proto::{ApiError, Call, Empty, ErrorCode, Request, Response};

use super::authentication::Authentication;
use super::listener::Connection;
use super::{browser_now_ms, finish_ended_browser_sessions, ok, secret_response, Service};
use crate::accounts::identity::AccountId;
use crate::hostdb::sqlite::accounts::multi_person::{
    AdminHandoverOutcome, InviteIssueError, MemberResetIssueError, OrgOwnerHandoverOutcome,
    RemoveAccessOutcome,
};
use crate::links::Issued;

pub(super) fn is_multi_person_op(request: &Request) -> bool {
    const METHODS: &[&str] = &[
        LinkInspect::METHOD,
        LinkRegisterAttach::METHOD,
        LinkSetPassword::METHOD,
        BrowserChangePassword::METHOD,
        BrowserOrgAccessList::METHOD,
        BrowserOrgInvite::METHOD,
        BrowserOrgPasswordResetLink::METHOD,
        BrowserOrgRemoveAccess::METHOD,
        BrowserOrgOwnerHandover::METHOD,
        BrowserAdminHandover::METHOD,
    ];
    METHODS.contains(&request.method.as_str())
}

pub(super) fn dispatch(
    service: &Service,
    authentication: &Authentication,
    request: &Request,
    connection: &Connection,
) -> bool {
    if !is_multi_person_op(request) {
        return false;
    }
    if request.is::<LinkInspect>() {
        handle_link_inspect(service, authentication, request, connection);
    } else if request.is::<LinkRegisterAttach>() {
        handle_link_register_attach(authentication, request, connection);
    } else if request.is::<LinkSetPassword>() {
        handle_link_set_password(authentication, request, connection);
    } else if request.is::<BrowserChangePassword>() {
        handle_change_password(authentication, request, connection);
    } else if request.is::<BrowserOrgAccessList>() {
        handle_org_access_list(authentication, request, connection);
    } else if request.is::<BrowserOrgInvite>() {
        handle_org_invite(service, authentication, request, connection);
    } else if request.is::<BrowserOrgPasswordResetLink>() {
        handle_org_password_reset_link(service, authentication, request, connection);
    } else if request.is::<BrowserOrgRemoveAccess>() {
        handle_org_remove_access(authentication, request, connection);
    } else if request.is::<BrowserOrgOwnerHandover>() {
        handle_org_owner_handover(service, authentication, request, connection);
    } else {
        handle_admin_handover(authentication, request, connection);
    }
    true
}

fn invalid() -> ApiError {
    ApiError::new(ErrorCode::InvalidRequest, "invalid account request")
}

fn forbidden(msg: &'static str) -> ApiError {
    ApiError::new(ErrorCode::NotAuthorized, msg)
}

fn host_origin(service: &Service) -> Result<&str, ApiError> {
    service
        .admin
        .get()
        .map(|a| a.origin())
        .or(service.browser_origin.as_deref())
        .ok_or_else(|| ApiError::new(ErrorCode::Unavailable, "web origin unavailable"))
}

fn build_issued_link(
    service: &Service,
    path_prefix: &str,
    issued: Issued,
) -> Result<IssuedLinkResult, ApiError> {
    let origin = host_origin(service)?.trim_end_matches('/');
    let token = issued.token.encode();
    let mut raw = String::with_capacity(origin.len() + path_prefix.len() + token.len());
    raw.push_str(origin);
    raw.push_str(path_prefix);
    raw.push_str(&token);
    let url = OneTimeLinkUrl::new(raw)
        .map_err(|_| ApiError::new(ErrorCode::Unavailable, "could not format link"))?;
    Ok(IssuedLinkResult {
        url,
        expires_at_ms: issued.expires_at_ms,
    })
}

fn require_org_manager(
    authentication: &Authentication,
    org_id: &str,
    account: &crate::hostdb::accounts::Account,
) -> Result<(), ApiError> {
    let (db, _) = authentication.provisioning_storage();
    let Some((owner_account, _)) = db
        .org_owner(org_id)
        .map_err(|_| ApiError::new(ErrorCode::Unavailable, "registry unavailable"))?
    else {
        return Err(ApiError::new(ErrorCode::NotFound, "organization not found"));
    };
    if owner_account.as_str() == account.account_id() || account.admin() {
        Ok(())
    } else {
        Err(forbidden("organization owner or administrator required"))
    }
}

fn notify_account_changed(connection: &Connection, account_id: &str) {
    connection.send_event(
        org_host_control::event::ACCOUNT_CHANGED,
        serde_json::json!({ "account_id": account_id }),
    );
}

fn handle_link_inspect(
    service: &Service,
    authentication: &Authentication,
    request: &Request,
    connection: &Connection,
) {
    let reply = browser_now_ms()
        .and_then(|now| {
            let params = request.params::<LinkInspect>().map_err(|_| invalid())?;
            authentication.inspect_link(&params, now, |org_id| {
                service
                    .offices
                    .organization_by_id(org_id)
                    .map(|o| o.name)
                    .unwrap_or_default()
            })
        })
        .map(|res| ok::<LinkInspect>(request.id, &res))
        .unwrap_or_else(|e| Response::error(request.id, e));
    connection.send_response(reply);
}

fn handle_link_register_attach(
    authentication: &Authentication,
    request: &Request,
    connection: &Connection,
) {
    let mut ended = Vec::new();
    let result = browser_now_ms().and_then(|now| {
        let params = request
            .params::<LinkRegisterAttach>()
            .map_err(|_| invalid())?;
        authentication.attach_invite(&params, now, &mut ended)
    });
    finish_ended_browser_sessions(authentication, connection, ended);
    match result {
        Ok(session) => {
            notify_account_changed(connection, &session.account.id);
            connection.send_response(ok::<LinkRegisterAttach>(request.id, &session));
        }
        Err(error) => connection.send_response(Response::error(request.id, error)),
    }
}

fn handle_link_set_password(
    authentication: &Authentication,
    request: &Request,
    connection: &Connection,
) {
    let mut ended = Vec::new();
    let result = browser_now_ms().and_then(|now| {
        let params = request.params::<LinkSetPassword>().map_err(|_| invalid())?;
        authentication.redeem_set_password(&params, now, &mut ended)
    });
    finish_ended_browser_sessions(authentication, connection, ended);
    match result {
        Ok(res) => connection.send_response(ok::<LinkSetPassword>(request.id, &res)),
        Err(error) => connection.send_response(Response::error(request.id, error)),
    }
}

fn handle_change_password(
    authentication: &Authentication,
    request: &Request,
    connection: &Connection,
) {
    let mut ended = Vec::new();
    let result = browser_now_ms().and_then(|now| {
        let params = request
            .params::<BrowserChangePassword>()
            .map_err(|_| invalid())?;
        if params.auth.local_authless {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "enable authentication before changing a password",
            ));
        }
        let session = authentication.validate_browser(
            &SessionValidateParams {
                cookie_digest: params.auth.cookie_digest,
                proof_digest: params.auth.proof_digest,
                origin: params.auth.origin.clone(),
            },
            now,
            &mut ended,
        )?;
        authentication.change_own_password(
            &session,
            &params.current_password,
            &params.new_password,
            now,
            &mut ended,
        )
    });
    finish_ended_browser_sessions(authentication, connection, ended);
    match result {
        Ok(()) => connection.send_response(ok::<BrowserChangePassword>(request.id, &Empty {})),
        Err(error) => connection.send_response(Response::error(request.id, error)),
    }
}

fn handle_org_access_list(
    authentication: &Authentication,
    request: &Request,
    connection: &Connection,
) {
    let mut ended = Vec::new();
    let result = browser_now_ms().and_then(|now| {
        let params = request
            .params::<BrowserOrgAccessList>()
            .map_err(|_| invalid())?;
        let account =
            authentication.browser_auth_account(connection, &params.auth, now, &mut ended)?;
        require_org_manager(authentication, &params.org_id, &account)?;
        let (db, _) = authentication.provisioning_storage();
        let entries = db
            .list_org_access(&params.org_id, now)
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "registry unavailable"))?;
        Ok(BrowserOrgAccessListResult { entries })
    });
    finish_ended_browser_sessions(authentication, connection, ended);
    match result {
        Ok(res) => connection.send_response(ok::<BrowserOrgAccessList>(request.id, &res)),
        Err(error) => connection.send_response(Response::error(request.id, error)),
    }
}

fn handle_org_invite(
    service: &Service,
    authentication: &Authentication,
    request: &Request,
    connection: &Connection,
) {
    let mut ended = Vec::new();
    let result = browser_now_ms().and_then(|now| {
        let params = request
            .params::<BrowserOrgInvite>()
            .map_err(|_| invalid())?;
        if params.auth.local_authless {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "enable authentication before inviting people",
            ));
        }
        let account =
            authentication.browser_auth_account(connection, &params.auth, now, &mut ended)?;
        require_org_manager(authentication, &params.org_id, &account)?;
        let (db, _) = authentication.provisioning_storage();
        let issued = db
            .issue_org_invite(&params.org_id, &params.member_id, now)
            .map_err(|err| match err {
                InviteIssueError::AlreadyLinked => ApiError::new(
                    ErrorCode::Conflict,
                    "this person already has an account in this organization",
                ),
                InviteIssueError::AuthDisabled => ApiError::new(
                    ErrorCode::Conflict,
                    "enable authentication before inviting people",
                ),
                InviteIssueError::Invalid => invalid(),
                InviteIssueError::Store => {
                    ApiError::new(ErrorCode::Unavailable, "registry unavailable")
                }
            })?;
        build_issued_link(service, "/register#", issued)
    });
    finish_ended_browser_sessions(authentication, connection, ended);
    match result {
        Ok(res) => secret_response(connection, request.id, &res),
        Err(error) => connection.send_response(Response::error(request.id, error)),
    }
}

fn handle_org_password_reset_link(
    service: &Service,
    authentication: &Authentication,
    request: &Request,
    connection: &Connection,
) {
    let mut ended = Vec::new();
    let result = browser_now_ms().and_then(|now| {
        let params = request
            .params::<BrowserOrgPasswordResetLink>()
            .map_err(|_| invalid())?;
        if params.auth.local_authless {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "enable authentication before issuing sign-in links",
            ));
        }
        let account =
            authentication.browser_auth_account(connection, &params.auth, now, &mut ended)?;
        require_org_manager(authentication, &params.org_id, &account)?;
        let (db, _) = authentication.provisioning_storage();
        let issued = db
            .issue_member_password_reset_link(&params.org_id, &params.member_id, now)
            .map_err(|err| match err {
                MemberResetIssueError::NotLinked => ApiError::new(
                    ErrorCode::NotFound,
                    "this person does not have an active account in this organization",
                ),
                MemberResetIssueError::Store => {
                    ApiError::new(ErrorCode::Unavailable, "registry unavailable")
                }
            })?;
        build_issued_link(service, "/set-password#", issued)
    });
    finish_ended_browser_sessions(authentication, connection, ended);
    match result {
        Ok(res) => secret_response(connection, request.id, &res),
        Err(error) => connection.send_response(Response::error(request.id, error)),
    }
}

fn handle_org_remove_access(
    authentication: &Authentication,
    request: &Request,
    connection: &Connection,
) {
    let mut ended = Vec::new();
    let mut changed_account: Option<String> = None;
    let result = browser_now_ms().and_then(|now| {
        let params = request
            .params::<BrowserOrgRemoveAccess>()
            .map_err(|_| invalid())?;
        if params.auth.local_authless {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "enable authentication before managing sign-in access",
            ));
        }
        let account =
            authentication.browser_auth_account(connection, &params.auth, now, &mut ended)?;
        require_org_manager(authentication, &params.org_id, &account)?;
        let (db, _) = authentication.provisioning_storage();
        match db
            .remove_org_access(&params.org_id, &params.member_id, now)
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "registry unavailable"))?
        {
            RemoveAccessOutcome::Removed {
                account_id,
                ended: gone,
            } => {
                authentication.forget_ended_touches(&gone);
                ended.extend(gone);
                changed_account = account_id.map(|id| id.as_str().to_owned());
                Ok(())
            }
            RemoveAccessOutcome::IsOwner => Err(ApiError::new(
                ErrorCode::Conflict,
                "hand ownership to someone else before removing the owner's access",
            )),
            RemoveAccessOutcome::NotFound => Err(ApiError::new(
                ErrorCode::NotFound,
                "this person has no sign-in access to remove",
            )),
        }
    });
    finish_ended_browser_sessions(authentication, connection, ended);
    match result {
        Ok(()) => {
            if let Some(id) = changed_account {
                notify_account_changed(connection, &id);
            }
            connection.send_response(ok::<BrowserOrgRemoveAccess>(request.id, &Empty {}));
        }
        Err(error) => connection.send_response(Response::error(request.id, error)),
    }
}

fn handle_org_owner_handover(
    service: &Service,
    authentication: &Authentication,
    request: &Request,
    connection: &Connection,
) {
    let mut ended = Vec::new();
    let mut notify_accounts: Vec<String> = Vec::new();
    let result = browser_now_ms().and_then(|now| {
        let params = request
            .params::<BrowserOrgOwnerHandover>()
            .map_err(|_| invalid())?;
        if params.auth.local_authless {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "enable authentication before handing over ownership",
            ));
        }
        let account =
            authentication.browser_auth_account(connection, &params.auth, now, &mut ended)?;
        let from_id = AccountId::parse(account.account_id())
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "invalid account id"))?;
        authentication.verify_existing_password(
            &from_id,
            account.login(),
            &params.password,
            now,
        )?;
        let (db, _) = authentication.provisioning_storage();
        match db
            .handover_org_owner(&params.org_id, &from_id, &params.to_member_id)
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "registry unavailable"))?
        {
            OrgOwnerHandoverOutcome::Transferred {
                previous_member_id,
                to_account_id,
                to_member_id,
            } => {
                service.offices.reload_org_offices(
                    &params.org_id,
                    from_id.as_str(),
                    &previous_member_id,
                    to_account_id.as_str(),
                    &to_member_id,
                    || db.restore_org_owner(&params.org_id, &from_id, &previous_member_id),
                )?;
                notify_accounts.push(from_id.as_str().to_owned());
                notify_accounts.push(to_account_id.as_str().to_owned());
                Ok(())
            }
            OrgOwnerHandoverOutcome::NotOwner => Err(forbidden(
                "only the organization owner can hand over ownership",
            )),
            OrgOwnerHandoverOutcome::InvalidTarget => Err(ApiError::new(
                ErrorCode::Conflict,
                "target person must already have an active account in this organization",
            )),
        }
    });
    finish_ended_browser_sessions(authentication, connection, ended);
    match result {
        Ok(()) => {
            for id in notify_accounts {
                notify_account_changed(connection, &id);
            }
            connection.send_response(ok::<BrowserOrgOwnerHandover>(request.id, &Empty {}));
        }
        Err(error) => connection.send_response(Response::error(request.id, error)),
    }
}

fn handle_admin_handover(
    authentication: &Authentication,
    request: &Request,
    connection: &Connection,
) {
    let mut ended = Vec::new();
    let mut notify_accounts: Vec<String> = Vec::new();
    let result = browser_now_ms().and_then(|now| {
        let params = request
            .params::<BrowserAdminHandover>()
            .map_err(|_| invalid())?;
        if params.auth.local_authless {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "enable authentication before handing over administrator rights",
            ));
        }
        let account =
            authentication.browser_auth_account(connection, &params.auth, now, &mut ended)?;
        let from_id = AccountId::parse(account.account_id())
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "invalid account id"))?;
        let to_id = AccountId::parse(&params.to_account_id).map_err(|_| invalid())?;
        authentication.verify_existing_password(
            &from_id,
            account.login(),
            &params.password,
            now,
        )?;
        let (db, _) = authentication.provisioning_storage();
        match db
            .handover_administrator(&from_id, &to_id)
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "registry unavailable"))?
        {
            AdminHandoverOutcome::Transferred => {
                notify_accounts.push(from_id.as_str().to_owned());
                notify_accounts.push(to_id.as_str().to_owned());
                Ok(())
            }
            AdminHandoverOutcome::NotAdmin => Err(forbidden(
                "only the instance administrator can hand over administrator rights",
            )),
            AdminHandoverOutcome::InvalidTarget => Err(ApiError::new(
                ErrorCode::Conflict,
                "target must be another active account sharing at least one organization",
            )),
        }
    });
    finish_ended_browser_sessions(authentication, connection, ended);
    match result {
        Ok(()) => {
            for id in notify_accounts {
                notify_account_changed(connection, &id);
            }
            connection.send_response(ok::<BrowserAdminHandover>(request.id, &Empty {}));
        }
        Err(error) => connection.send_response(Response::error(request.id, error)),
    }
}
