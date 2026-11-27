//! Account sign-in on `host.sock`: authentication, browser-approved CLI sign-in and the
//! session lifecycle calls.

use std::sync::Arc;

use org_proto::{ApiError, ErrorCode, Request, Response};

use super::browser::deliver_channel;
use super::browser_sessions::{browser_now_ms, finish_ended_browser_sessions};
use super::reply::{answer, not_authorized, ok, secret_response};
use super::Service;
use super::{cli_approvals, native_org, office_ops};
use crate::authentication::Authentication;
use crate::control_listener::Connection;

impl Service {
    pub(super) fn decide_cli_approval(
        &self,
        authentication: &Authentication,
        request: &Request,
        connection: &Arc<Connection>,
    ) {
        use org_host_control::browser::{BrowserCliApprovalDecide, BrowserCliApprovalDecideResult};
        let mut ended = Vec::new();
        let result = browser_now_ms().and_then(|now| {
            let Some(instance_id) = self.instance_id.as_deref() else {
                return Err(ApiError::new(
                    ErrorCode::Unavailable,
                    "host instance is not registered",
                ));
            };
            let p = request.params::<BrowserCliApprovalDecide>().map_err(|_| {
                ApiError::new(ErrorCode::InvalidRequest, "invalid cli approval decision")
            })?;
            let account =
                authentication.browser_auth_account(connection, &p.auth, now, &mut ended)?;
            let account_id = crate::accounts::identity::AccountId::parse(account.account_id())
                .map_err(|_| ApiError::new(ErrorCode::Unavailable, "invalid account id"))?;
            let status =
                self.cli_approvals
                    .decide(instance_id, &p.link, account_id, p.approve, now)?;
            Ok(BrowserCliApprovalDecideResult { status })
        });
        finish_ended_browser_sessions(authentication, connection, ended);
        match result {
            Ok(res) => connection.send_response(ok::<BrowserCliApprovalDecide>(request.id, &res)),
            Err(err) => connection.send_response(Response::error(request.id, err)),
        }
    }
    fn create_cli_approval(&self, request: &Request, connection: &Arc<Connection>) {
        use org_host_control::ops::CliApprovalCreate;
        if connection.web_listen().is_some() {
            connection.send_response(not_authorized(request.id));
            return;
        }
        let result = browser_now_ms().and_then(|now| {
            request.params::<CliApprovalCreate>().map_err(|_| {
                ApiError::new(ErrorCode::InvalidRequest, "invalid cli approval request")
            })?;
            let Some(instance_id) = self.instance_id.as_deref() else {
                return Err(ApiError::new(
                    ErrorCode::Unavailable,
                    "host instance is not registered",
                ));
            };
            let origin = self
                .admin
                .get()
                .map(|a| a.origin())
                .or(self.browser_origin.as_deref())
                .ok_or_else(|| ApiError::new(ErrorCode::Unavailable, "web origin unavailable"))?;
            self.cli_approvals
                .create(instance_id, origin, connection, now)
        });
        match result {
            Ok(created) => secret_response(connection, request.id, &created),
            Err(err) => connection.send_response(Response::error(request.id, err)),
        }
    }
    fn poll_cli_approval(
        &self,
        authentication: &Authentication,
        request: &Request,
        connection: &Arc<Connection>,
    ) {
        use org_host_control::ops::{CliApprovalPoll, CliApprovalPollResult};
        if connection.web_listen().is_some() {
            connection.send_response(not_authorized(request.id));
            return;
        }
        let result = browser_now_ms().and_then(|now| {
            let p = request.params::<CliApprovalPoll>().map_err(|_| {
                ApiError::new(ErrorCode::InvalidRequest, "invalid cli approval poll")
            })?;
            match self.cli_approvals.poll(connection, &p.approval_id, now)? {
                cli_approvals::PollOutcome::Pending => Ok(CliApprovalPollResult::Pending),
                cli_approvals::PollOutcome::Declined => Ok(CliApprovalPollResult::Declined),
                cli_approvals::PollOutcome::Expired => Ok(CliApprovalPollResult::Expired),
                cli_approvals::PollOutcome::Approved(account_id) => {
                    let mut issued = authentication.issue_for_account(connection, &account_id)?;
                    let offices = self.offices.offices();
                    for link in &mut issued.orgs {
                        link.office_ids = offices
                            .iter()
                            .filter(|o| o.organization_id == link.org_id)
                            .map(|o| o.id.clone())
                            .collect();
                    }
                    Ok(CliApprovalPollResult::Approved {
                        session: issued.session,
                        account: issued.account,
                        admin: issued.admin,
                        orgs: issued.orgs,
                    })
                }
            }
        });
        match result {
            Ok(res) => secret_response(connection, request.id, &res),
            Err(err) => connection.send_response(Response::error(request.id, err)),
        }
    }
    pub(super) fn authenticate(&self, request: &Request, connection: &Arc<Connection>) -> bool {
        use org_host_control::ops::{
            AccountVerify, CliApprovalCreate, CliApprovalPoll, FdsReceived, OfficeOpenChannel,
        };
        if request.is::<FdsReceived>() {
            acknowledge_descriptors(request, connection);
            return true;
        }
        if !session_auth_method(request) {
            return false;
        }
        let Some(authentication) = &self.authentication else {
            // An inactive protected registry grants no session-method authority.
            // Refuse before interpreting either valid or malformed credentials.
            connection.send_response(not_authorized(request.id));
            return true;
        };
        if request.is::<AccountVerify>() {
            self.sign_in(authentication, request, connection);
        } else if request.is::<CliApprovalCreate>() {
            self.create_cli_approval(request, connection);
        } else if request.is::<CliApprovalPoll>() {
            self.poll_cli_approval(authentication, request, connection);
        } else if request.is::<OfficeOpenChannel>() {
            self.open_authenticated_channel(authentication, request, connection);
        } else if native_org::dispatch(self, authentication, request, connection) {
            return true;
        } else if !office_ops::dispatch_session_office_op(self, authentication, request, connection)
        {
            dispatch_session_lifecycle(self, authentication, request, connection);
        }
        true
    }
    fn sign_in(
        &self,
        authentication: &Authentication,
        request: &Request,
        connection: &Arc<Connection>,
    ) {
        use org_host_control::ops::AccountVerify;
        let result = request
            .params::<AccountVerify>()
            .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "invalid sign-in request"))
            .and_then(|params| {
                let mut result =
                    authentication.verify(connection, &params.login, &params.password)?;
                let offices = self.offices.offices();
                for link in &mut result.orgs {
                    link.office_ids = offices
                        .iter()
                        .filter(|o| o.organization_id == link.org_id)
                        .map(|o| o.id.clone())
                        .collect();
                }
                Ok(result)
            });
        match result {
            Ok(result) => {
                secret_response(connection, request.id, &result);
            }
            Err(error) => connection.send_response(Response::error(request.id, error)),
        }
    }
    fn open_authenticated_channel(
        &self,
        authentication: &Authentication,
        request: &Request,
        connection: &Arc<Connection>,
    ) {
        use org_host_control::ops::OfficeOpenChannel;
        let result = request
            .params::<OfficeOpenChannel>()
            .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "invalid channel request"))
            .and_then(|p| {
                let account = authentication.authorize(connection, &p.session)?;
                let allow_unlinked_satellite =
                    connection.channel_kind() == org_office_launch::ChannelKind::Local;
                let office = self
                    .offices
                    .offices()
                    .into_iter()
                    .find(|o| o.id == p.office_id)
                    .filter(|o| {
                        office_ops::office_visible_to_account(&account, o, allow_unlinked_satellite)
                    })
                    .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "office not found"))?;
                let endpoint = self.offices.endpoint(&p.office_id).ok_or_else(|| {
                    ApiError::new(ErrorCode::Unavailable, "office channel unavailable")
                })?;
                let linked = account
                    .links()
                    .iter()
                    .any(|l| l.org_id() == office.organization_id);
                let opened = if linked {
                    authentication.open_channel(
                        connection,
                        &p.session,
                        endpoint,
                        &office.organization_id,
                    )
                } else {
                    authentication.open_satellite_operator_channel(
                        connection,
                        &p.session,
                        endpoint,
                        &office.organization_id,
                    )
                };
                opened.map(|(id, socket, witness)| (id, socket, witness, p.office_id))
            });
        match result {
            Ok((id, socket, witness, office)) => {
                deliver_channel(connection, request.id, id, socket, &office, witness);
            }
            Err(error) => connection.send_response(Response::error(request.id, error)),
        }
    }
}

fn dispatch_session_lifecycle(
    service: &Service,
    authentication: &Authentication,
    request: &Request,
    connection: &Arc<Connection>,
) {
    use org_host_control::ops::{SessionEnd, SessionGrant, SessionInspect, SessionRenew};
    if request.is::<SessionGrant>() {
        let result = request
            .params::<SessionGrant>()
            .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "invalid grant request"))
            .and_then(|p| {
                authentication.grant(connection, &p.session, &p.office_id, &p.channel_id)
            });
        match result {
            Ok(result) => secret_response(connection, request.id, &result),
            Err(error) => connection.send_response(Response::error(request.id, error)),
        }
    } else if request.is::<SessionInspect>() {
        connection.send_response(answer::<SessionInspect>(request, |params| {
            let mut result = authentication.inspect(connection, &params.session)?;
            let offices = service.offices.offices();
            for link in &mut result.orgs {
                link.office_ids = offices
                    .iter()
                    .filter(|o| o.organization_id == link.org_id)
                    .map(|o| o.id.clone())
                    .collect();
            }
            Ok(result)
        }));
    } else if request.is::<SessionRenew>() {
        connection.send_response(answer::<SessionRenew>(request, |params| {
            authentication.authorize(connection, &params.session)?;
            Ok(org_proto::Empty {})
        }));
    } else {
        connection.send_response(answer::<SessionEnd>(request, |params| {
            // Dropping the bound session sends revocation to each worker;
            // a failed transfer closes that worker's authenticated channel.
            connection.clear_recovery_permission();
            authentication.end(connection, &params.session)?;
            Ok(org_proto::Empty {})
        }));
    }
}

fn session_auth_method(request: &Request) -> bool {
    use org_host_control::ops::{
        AccountVerify, CliApprovalCreate, CliApprovalPoll, OfficeJoin, OfficeList,
        OfficeOpenChannel, OfficeReEnroll, OfficeRemovePending, OfficeRestart, SessionEnd,
        SessionGrant, SessionInspect, SessionRenew,
    };
    request.is::<AccountVerify>()
        || request.is::<CliApprovalCreate>()
        || request.is::<CliApprovalPoll>()
        || native_org::is_native_org_or_local_session(request)
        || request.is::<SessionRenew>()
        || request.is::<SessionEnd>()
        || request.is::<OfficeOpenChannel>()
        || request.is::<OfficeList>()
        || request.is::<OfficeRestart>()
        || request.is::<OfficeJoin>()
        || request.is::<OfficeRemovePending>()
        || request.is::<OfficeReEnroll>()
        || request.is::<SessionGrant>()
        || request.is::<SessionInspect>()
}

pub(crate) fn acknowledge_descriptors(request: &Request, connection: &Arc<Connection>) {
    use org_host_control::ops::FdsReceived;
    let acknowledged = request
        .params::<FdsReceived>()
        .ok()
        .is_some_and(|p| connection.descriptor_received(&p.of));
    if acknowledged {
        connection.send_response(ok::<FdsReceived>(request.id, &org_proto::Empty {}));
    } else {
        connection.refuse_and_finish(not_authorized(request.id));
    }
}
