//! Browser operations relayed by the web process: admitting them, their session
//! lifecycle and the office channels they open.

use std::sync::Arc;

use org_proto::{ApiError, Call, ErrorCode, Request, Response};

use super::authentication::Authentication;
use super::browser_sessions::{browser_now_ms, finish_ended_browser_sessions};
use super::listener::Connection;
use super::reply::{not_authorized, ok, secret_response};
use super::Service;
use super::{listener, local_access, multi_person, office_ops};

impl Service {
    /// Browser-session ops (A3 B4, B2 Q8), only on the admitted web process's control
    /// connection. Each `session.ended` push is written before the reply on the
    /// connection's one ordered writer, after the record's delete.
    pub(super) fn browser_op(&self, request: &Request, connection: &Arc<Connection>) -> bool {
        use org_host_control::browser::{
            BrowserAuthEnable, BrowserCliApprovalDecide, BrowserLocalSession, BrowserOfficeRestart,
            BrowserOpenChannel, BrowserSessionGrant, BrowserTlsEnable, InstanceView,
        };
        if !is_browser_op(request) || connection.web_listen().is_none() {
            return false;
        }
        #[cfg(test)]
        self.browser_ops
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        if request.is::<InstanceView>() {
            let reply = request
                .params::<InstanceView>()
                .map_err(|_| {
                    ApiError::new(ErrorCode::InvalidRequest, "invalid instance view request")
                })
                .and_then(|_| self.instance_view())
                .map(|r| ok::<InstanceView>(request.id, &r));
            connection.send_response(reply.unwrap_or_else(|e| Response::error(request.id, e)));
            return true;
        }
        let Some(authentication) = &self.authentication else {
            connection.send_response(not_authorized(request.id));
            return true;
        };
        if request.is::<BrowserOpenChannel>() {
            self.open_browser_channel(authentication, request, connection);
        } else if request.is::<BrowserSessionGrant>() {
            self.grant_browser_channel(authentication, request, connection);
        } else if request.is::<BrowserOfficeRestart>() {
            office_ops::restart_browser_office(self, authentication, request, connection);
        } else if request.is::<BrowserCliApprovalDecide>() {
            self.decide_cli_approval(authentication, request, connection);
        } else if request.is::<BrowserLocalSession>() {
            local_access::handle_local_session(authentication, request, connection);
        } else if request.is::<BrowserAuthEnable>() {
            local_access::handle_auth_enable(self, authentication, request, connection);
        } else if request.is::<BrowserTlsEnable>() {
            local_access::handle_tls_enable(self, authentication, request, connection);
        } else if multi_person::dispatch(self, authentication, request, connection) {
            return true;
        } else {
            browser_session_op(self, authentication, request, connection);
        }
        true
    }
    fn open_browser_channel(
        &self,
        authentication: &Authentication,
        request: &Request,
        connection: &Arc<Connection>,
    ) {
        use org_host_control::browser::BrowserOpenChannel;
        let mut ended = Vec::new();
        let result = browser_now_ms().and_then(|now| {
            let p = request
                .params::<BrowserOpenChannel>()
                .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "invalid channel request"))?;
            let account =
                authentication.browser_auth_account(connection, &p.auth, now, &mut ended)?;
            let office = self
                .offices
                .offices()
                .into_iter()
                .find(|o| o.id == p.office_id)
                .filter(|o| {
                    account
                        .links()
                        .iter()
                        .any(|l| l.org_id() == o.organization_id)
                })
                .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "office not found"))?;
            let endpoint = self.offices.endpoint(&p.office_id).ok_or_else(|| {
                ApiError::new(ErrorCode::Unavailable, "office channel unavailable")
            })?;
            authentication
                .open_browser_channel(
                    connection,
                    &p,
                    endpoint,
                    &office.organization_id,
                    now,
                    &mut ended,
                )
                .map(|(id, socket, witness)| (id, socket, witness, p.office_id))
        });
        finish_ended_browser_sessions(authentication, connection, ended);
        match result {
            Ok((id, socket, witness, office)) => {
                deliver_channel(connection, request.id, id, socket, &office, witness);
            }
            Err(error) => connection.send_response(Response::error(request.id, error)),
        }
    }
    fn grant_browser_channel(
        &self,
        authentication: &Authentication,
        request: &Request,
        connection: &Arc<Connection>,
    ) {
        use org_host_control::browser::BrowserSessionGrant;
        let mut ended = Vec::new();
        let result = browser_now_ms().and_then(|now| {
            let p = request
                .params::<BrowserSessionGrant>()
                .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "invalid grant request"))?;
            authentication.grant_browser(connection, &p, now, &mut ended)
        });
        finish_ended_browser_sessions(authentication, connection, ended);
        match result {
            Ok(result) => secret_response(connection, request.id, &result),
            Err(error) => connection.send_response(Response::error(request.id, error)),
        }
    }
}

fn is_browser_op(request: &Request) -> bool {
    use org_host_control::browser::{
        BrowserAuthEnable, BrowserCliApprovalDecide, BrowserLocalSession, BrowserOfficeRestart,
        BrowserOpenChannel, BrowserSessionEnd, BrowserSessionGrant, BrowserTlsEnable, InstanceView,
        LinkRegister, SessionSignIn, SessionValidate,
    };
    const METHODS: &[&str] = &[
        LinkRegister::METHOD,
        SessionSignIn::METHOD,
        SessionValidate::METHOD,
        BrowserSessionEnd::METHOD,
        InstanceView::METHOD,
        BrowserOpenChannel::METHOD,
        BrowserSessionGrant::METHOD,
        BrowserOfficeRestart::METHOD,
        BrowserCliApprovalDecide::METHOD,
        BrowserLocalSession::METHOD,
        BrowserAuthEnable::METHOD,
        BrowserTlsEnable::METHOD,
    ];
    METHODS.contains(&request.method.as_str()) || multi_person::is_multi_person_op(request)
}

fn browser_session_op(
    service: &Service,
    authentication: &Authentication,
    request: &Request,
    connection: &Connection,
) {
    use org_host_control::browser::{
        AuthModeChanged, BrowserSessionEnd, LinkRegister, SessionSignIn, SessionValidate,
    };
    let invalid = |_| ApiError::new(ErrorCode::InvalidRequest, "invalid browser session request");
    let mut ended = Vec::new();
    let now = browser_now_ms();
    let was_authless =
        request.is::<LinkRegister>() && authentication.auth_enabled().is_ok_and(|e| !e);
    let reply = if request.is::<LinkRegister>() {
        now.and_then(|now| {
            let p = request.params::<LinkRegister>().map_err(invalid)?;
            authentication.register(&p, now, &mut ended)
        })
        .map(|r| ok::<LinkRegister>(request.id, &r))
    } else if request.is::<SessionSignIn>() {
        now.and_then(|now| {
            let p = request.params::<SessionSignIn>().map_err(invalid)?;
            authentication.sign_in_browser(&p, now, &mut ended)
        })
        .map(|r| ok::<SessionSignIn>(request.id, &r))
    } else if request.is::<SessionValidate>() {
        now.and_then(|now| {
            let p = request.params::<SessionValidate>().map_err(invalid)?;
            authentication.validate_browser(&p, now, &mut ended)
        })
        .map(|r| ok::<SessionValidate>(request.id, &r))
    } else {
        request
            .params::<BrowserSessionEnd>()
            .map_err(invalid)
            .and_then(|p| authentication.end_browser(&p.session_id))
            .map(|e| {
                ended.extend(e);
                ok::<BrowserSessionEnd>(request.id, &org_proto::Empty {})
            })
    };
    finish_ended_browser_sessions(authentication, connection, ended);
    if reply.is_ok() && was_authless && authentication.auth_enabled().is_ok_and(|e| e) {
        if let Some(provisioning) = &service.provisioning {
            if let Ok(data) = serde_json::to_value(AuthModeChanged { auth_enabled: true }) {
                provisioning
                    .web()
                    .publish_named(org_host_control::event::AUTH_MODE_CHANGED, data);
            }
        }
    }
    connection.send_response(reply.unwrap_or_else(|e| Response::error(request.id, e)));
}

/// Deliver an already authorized channel. A definite local reservation refusal
/// answers this request; uncertain transport still terminates the connection.
pub(super) fn deliver_channel(
    connection: &Connection,
    request_id: u64,
    id: String,
    socket: std::os::unix::net::UnixStream,
    office: &str,
    witness: crate::supervisor::channels::ChannelSlot,
) {
    let response = ok::<org_host_control::ops::OfficeOpenChannel>(
        request_id,
        &org_host_control::ops::OfficeOpenChannelResult {
            channel_id: id.clone(),
        },
    );
    match connection.send_descriptor(response, id, socket.into(), office, witness) {
        Ok(()) => {}
        Err(listener::DescriptorError::Capacity) => connection.send_response(Response::error(
            request_id,
            ApiError::new(ErrorCode::Unavailable, "office channel unavailable"),
        )),
        Err(listener::DescriptorError::Transport) => connection.close(),
    }
}

/// `web.serving` (GW-01 r4): only the admitted web process connection may report, once, after
/// hello and a successful `tls.fetch`. Anything else from it is a protocol error that closes
/// the control connection, so supervision fails the start rather than publishing `running`.
pub(super) fn web_reports_serving(request: &Request, connection: &Arc<Connection>) {
    use crate::service::listener::ServingReport;
    use org_host_control::ops::WebServing;
    let refused = |code, message| Response::error(request.id, ApiError::new(code, message));
    if connection.web_listen().is_none() {
        connection.send_response(refused(
            ErrorCode::NotAuthorized,
            "private gateway connection required",
        ));
        return;
    }
    if request.params::<WebServing>().is_err() {
        connection.refuse_serving(refused(
            ErrorCode::InvalidRequest,
            "the params don't fit this method",
        ));
        return;
    }
    match connection.report_serving() {
        ServingReport::Accepted => {
            connection.send_response(ok::<WebServing>(request.id, &Default::default()));
        }
        ServingReport::OutOfOrder | ServingReport::NotWeb => {
            connection.refuse_serving(refused(
                ErrorCode::InvalidRequest,
                "web.serving is sent once, after hello and tls.fetch",
            ));
        }
    }
}
