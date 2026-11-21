//! host.sock's private control boundary: registered-instance hello and count-only status.
//! Registered hosts additionally dispatch live account sign-in, renewal and logout, the
//! administrator link calls (`admin.link.initial`, `admin.reset`; see `crate::admin`) and,
//! for an `https` listener, the administrator's `tls.set` (`crate::web_tls`).

pub(crate) mod authentication;
pub(crate) mod listener;
pub(crate) mod organizations;
use authentication::Authentication;
mod request;

use std::sync::Arc;

use org_host_control::{
    Administrator, Hello, HelloResult, InstanceMode, InstanceStatus, InstanceStatusResult,
    OfficeCounts, WebState, WebStatus,
};
use org_proto::{ApiError, Call, ErrorCode, Request, Response};

use crate::broadcast::Broadcast;
use crate::offices::{Offices, THIS_MAC};
use crate::peer::Received;
use listener::{Connection, Handler};

pub(crate) struct Service {
    offices: Arc<Offices>,
    broadcast: Arc<Broadcast>,
    instance_id: Option<String>,
    pub(crate) authentication: Option<Arc<Authentication>>,
    gateway: Arc<std::sync::Mutex<WebState>>,
    provisioning: Option<Arc<crate::provisioning::Provisioning>>,
    recovery: Option<Arc<crate::recovery::Recovery>>,
    /// The `https` listener's pair: `tls.fetch`, `tls.set` and `instance.view`'s `tls`.
    /// `None` for a plain HTTP listener.
    web_tls: Option<Arc<crate::web_tls::WebTls>>,
    /// Installed once by the registered start after composition; until then the admin calls
    /// answer `unavailable`.
    admin: std::sync::OnceLock<Arc<crate::admin::Admin>>,
    /// The registered instance's name, installed once by the registered start; until then
    /// `instance.view` answers `unavailable`.
    instance_name: std::sync::OnceLock<String>,
    /// Browser ops this service accepted on a web connection; real-process tests prove
    /// that an ingress refusal reaches no host op.
    #[cfg(test)]
    browser_ops: std::sync::atomic::AtomicUsize,
    /// Organization calls answered on the admitted web connection; real-process tests
    /// prove that a refused request reaches none.
    #[cfg(test)]
    organization_ops: std::sync::atomic::AtomicUsize,
}

impl Service {
    pub(crate) fn new(
        offices: Arc<Offices>,
        broadcast: Arc<Broadcast>,
        instance_id: Option<String>,
    ) -> Self {
        Self {
            offices,
            broadcast,
            instance_id,
            authentication: None,
            provisioning: None,
            recovery: None,
            web_tls: None,
            admin: std::sync::OnceLock::new(),
            instance_name: std::sync::OnceLock::new(),
            #[cfg(test)]
            browser_ops: std::sync::atomic::AtomicUsize::new(0),
            #[cfg(test)]
            organization_ops: std::sync::atomic::AtomicUsize::new(0),
            gateway: Arc::new(std::sync::Mutex::new(WebState::NotConfigured)),
        }
    }
    pub(crate) fn with_authentication(
        mut self,
        authentication: Option<Arc<Authentication>>,
    ) -> Self {
        self.authentication = authentication;
        self
    }
    pub(crate) fn with_provisioning(
        mut self,
        provisioning: Option<Arc<crate::provisioning::Provisioning>>,
    ) -> Self {
        self.provisioning = provisioning;
        self
    }
    pub(crate) fn with_recovery(
        mut self,
        recovery: Option<Arc<crate::recovery::Recovery>>,
    ) -> Self {
        self.recovery = recovery;
        self
    }
    pub(crate) fn with_web_tls(mut self, web_tls: Option<Arc<crate::web_tls::WebTls>>) -> Self {
        self.web_tls = web_tls;
        self
    }
    #[cfg(test)]
    pub(crate) fn instance_id_for_test(&self) -> Option<&str> {
        self.instance_id.as_deref()
    }

    #[cfg(test)]
    pub(crate) fn web_tls_for_test(&self) -> Option<&Arc<crate::web_tls::WebTls>> {
        self.web_tls.as_ref()
    }
    /// `tls.renew` to the web process: its pair was replaced or renewed, and it fetches it
    /// again. Only the admitted web connection subscribes to these events.
    pub(crate) fn announce_tls_renewal(&self) {
        if let Some(provisioning) = &self.provisioning {
            provisioning
                .web()
                .publish_named(org_host_control::event::TLS_RENEW, serde_json::json!({}));
        }
    }
    /// The registered host's administrator link calls. Installed at most once.
    #[cfg(test)]
    pub(crate) fn admin(&self) -> Option<&Arc<crate::admin::Admin>> {
        self.admin.get()
    }
    pub(crate) fn install_admin(&self, admin: Arc<crate::admin::Admin>) -> bool {
        self.admin.set(admin).is_ok()
    }
    #[cfg(test)]
    pub(crate) fn browser_ops(&self) -> usize {
        self.browser_ops.load(std::sync::atomic::Ordering::Acquire)
    }
    /// The creation state a create thread keeps alive until it ends.
    #[cfg(test)]
    pub(crate) fn provisioning_for_test(&self) -> Option<&Arc<crate::provisioning::Provisioning>> {
        self.provisioning.as_ref()
    }
    #[cfg(test)]
    pub(crate) fn organization_ops(&self) -> usize {
        self.organization_ops
            .load(std::sync::atomic::Ordering::Acquire)
    }
    /// The registered name `instance.view` reports. Installed at most once.
    pub(crate) fn install_instance_name(&self, name: &str) -> bool {
        self.instance_name.set(name.to_owned()).is_ok()
    }
    /// `instance.view` on the web connection: the name, mode, web state and whether an
    /// administrator exists. Nothing else about the host.
    fn instance_view(&self) -> Result<org_host_control::browser::InstanceViewResult, ApiError> {
        let name = self.instance_name.get().cloned().ok_or_else(|| {
            ApiError::new(ErrorCode::Unavailable, "the instance view is unavailable")
        })?;
        Ok(org_host_control::browser::InstanceViewResult {
            name,
            status: InstanceMode::Service,
            web_status: *crate::sync::lock(&self.gateway),
            administrator: self.administrator()?,
            tls: self.web_tls.as_ref().map(|tls| {
                crate::web_tls::now()
                    .map_or(org_proto::web::TlsStatus::Missing, |now| tls.status(now))
            }),
        })
    }
    /// `tls.set` (`host.sock` only; the signed `agentic-org` peer; an administrator's
    /// session; journaled; X-02 §6.2): the pair is checked and installed by
    /// [`crate::web_tls::WebTls::set`], and a pair it installs is announced to the web
    /// process with `tls.renew`. Refused with `not_authorized` on the web connection, without
    /// a registry, for a peer the administrator calls' admission refuses
    /// ([`crate::admin::Admin::admit_peer`]) or for an account that is not the
    /// administrator; `unavailable` before the administrator service is installed and on an
    /// instance whose listener is not `https`.
    fn set_tls(&self, request: &Request, connection: &Arc<Connection>, received: Received) -> bool {
        use org_host_control::ops::TlsSet;
        if !request.is::<TlsSet>() {
            return false;
        }
        let (None, Some(authentication)) = (connection.gateway_listen(), &self.authentication)
        else {
            connection.send_response(not_authorized(request.id));
            return true;
        };
        connection.send_response(answer::<TlsSet>(request, |params| {
            let admin = self.admin.get().ok_or_else(|| {
                ApiError::new(
                    ErrorCode::Unavailable,
                    "administrator calls are unavailable on this host",
                )
            })?;
            admin.admit_peer(connection, received)?;
            let account = authentication.authorize(connection, &params.session)?;
            if !account.admin() {
                return Err(ApiError::new(
                    ErrorCode::NotAuthorized,
                    "only the instance's administrator sets its TLS pair",
                ));
            }
            let tls = self.web_tls.as_ref().ok_or_else(|| {
                ApiError::new(
                    ErrorCode::Unavailable,
                    "this instance's listener is not https; it has no TLS pair",
                )
            })?;
            let now = crate::web_tls::now()
                .ok_or_else(|| ApiError::new(ErrorCode::Unavailable, "clock unavailable"))?;
            let (result, installed) = tls.set(&params, account.account_id(), now)?;
            if installed {
                self.announce_tls_renewal();
            }
            Ok(result)
        }));
        true
    }
    /// `tls.fetch` (the web connection only): the listener's snapshot and, in `https`, the
    /// stored pair, re-checked for the snapshot's origin. A pair that does not cover it is
    /// `unavailable` with `tls_name_mismatch`; one that cannot be read (its key is gone) is
    /// `storage_error`. An answer with a key goes out on the secret-line path.
    fn fetch_tls(&self, request: &Request, connection: &Arc<Connection>) {
        use crate::web_tls::Unservable;
        use org_host_control::listen::ListenMode;
        use org_host_control::ops::{TlsFetch, TlsFetchResult};
        if request.params::<TlsFetch>().is_err() {
            connection.send_response(params_dont_fit(request.id));
            return;
        }
        let Some(snapshot) = connection.gateway_listen() else {
            connection.send_response(Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::NotAuthorized,
                    "private gateway connection required",
                ),
            ));
            return;
        };
        // The two plain HTTP modes carry no pair. Which of them a listener has was judged
        // against the instance's service mode when the host started (`registered`), before
        // the listener was bound.
        let (origin, hsts) = (snapshot.origin().clone(), snapshot.hsts());
        let answer = match snapshot.scheme() {
            ListenMode::HttpLoopback => TlsFetchResult::HttpLoopback { origin, hsts },
            ListenMode::HttpPublished => TlsFetchResult::HttpPublished { origin, hsts },
            ListenMode::Https => match self.web_tls.as_deref().map(|tls| tls.fetch_for(&origin)) {
                Some(Ok((cert, key))) => TlsFetchResult::Https {
                    cert,
                    key,
                    origin,
                    hsts,
                },
                Some(Err(Unservable::Failure(failure))) => {
                    connection.send_response(ok::<TlsFetch>(
                        request.id,
                        &TlsFetchResult::Unavailable(failure),
                    ));
                    return;
                }
                Some(Err(Unservable::Missing | Unservable::NotValid | Unservable::Storage))
                | None => {
                    connection.send_response(Response::error(
                        request.id,
                        ApiError::new(ErrorCode::StorageError, "the TLS pair cannot be read"),
                    ));
                    return;
                }
            },
        };
        connection.note_listener_fetched();
        if matches!(answer, TlsFetchResult::Https { .. }) {
            secret_response(connection, request.id, &answer);
        } else {
            connection.send_response(ok::<TlsFetch>(request.id, &answer));
        }
    }
    fn administer(
        &self,
        request: &Request,
        connection: &Arc<Connection>,
        received: Received,
    ) -> bool {
        use org_host_control::admin::{AdminLinkInitial, AdminReset};
        let initial = request.is::<AdminLinkInitial>();
        if !initial && !request.is::<AdminReset>() {
            return false;
        }
        let Some(admin) = self.admin.get() else {
            connection.send_response(Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::Unavailable,
                    "administrator links are unavailable on this host",
                ),
            ));
            return true;
        };
        let result = if initial {
            request
                .params::<AdminLinkInitial>()
                .map_err(|_| {
                    ApiError::new(
                        ErrorCode::InvalidRequest,
                        "the params don't fit this method",
                    )
                })
                .and_then(|params| admin.initial(connection, received, params))
        } else {
            request
                .params::<AdminReset>()
                .map_err(|_| {
                    ApiError::new(
                        ErrorCode::InvalidRequest,
                        "the params don't fit this method",
                    )
                })
                .and_then(|_| admin.reset(connection, received))
        };
        match result {
            Ok(result) => secret_response(connection, request.id, &result),
            Err(error) => connection.send_response(Response::error(request.id, error)),
        }
        true
    }
    /// Whether the registry holds an administrator; a host without a registry has none.
    fn administrator(&self) -> Result<Administrator, ApiError> {
        let Some(authentication) = &self.authentication else {
            return Ok(Administrator::None);
        };
        match authentication
            .provisioning_storage()
            .0
            .administrator_exists()
        {
            Ok(true) => Ok(Administrator::Present),
            Ok(false) => Ok(Administrator::None),
            Err(_) => Err(ApiError::new(
                ErrorCode::Unavailable,
                "the host registry is unavailable",
            )),
        }
    }
    fn recover(&self, request: &Request, connection: &Arc<Connection>, received: Received) -> bool {
        use org_host_control::{
            ops::{RecoveryAcknowledge, RecoveryRelease},
            recovery::{Package, Status},
        };
        if !crate::recovery::handles(request) {
            return false;
        }
        if request.is::<RecoveryAcknowledge>() && request.params::<RecoveryAcknowledge>().is_err() {
            connection.clear_recovery_permission();
        }
        let Some(recovery) = &self.recovery else {
            connection.send_response(Response::error(
                request.id,
                ApiError::new(ErrorCode::Unavailable, "recovery unavailable"),
            ));
            return true;
        };
        if request.is::<RecoveryRelease>() {
            connection.clear_recovery_permission();
            let result = request
                .params::<RecoveryRelease>()
                .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "invalid recovery request"))
                .and_then(|p| recovery.release(connection, received, p));
            match result {
                Ok(result) => secret_response(connection, request.id, &result),
                Err(e) => connection.send_response(Response::error(request.id, e)),
            }
        } else if request.is::<RecoveryAcknowledge>() {
            connection.send_response(answer::<RecoveryAcknowledge>(request, |p| {
                recovery.acknowledge(connection, received, p)
            }));
        } else if request.is::<Status>() {
            connection.send_response(answer::<Status>(request, |p| {
                recovery.status(connection, received, p)
            }));
        } else {
            connection.send_response(answer::<Package>(request, |p| {
                recovery.package(connection, received, p)
            }));
        }
        true
    }
    pub(crate) fn with_gateway_state(mut self, state: Arc<std::sync::Mutex<WebState>>) -> Self {
        self.gateway = state;
        self
    }
    /// Whether the host may start its web process: the instance is registered and its
    /// host authentication is present. No organization or office is required: the person
    /// registers from setup's link and creates the organization in the browser, so routes
    /// that need one answer their own "not yet" state. Persisted listener configuration
    /// alone never qualifies.
    pub(crate) fn web_eligible(&self) -> bool {
        web_start_eligible(self.authentication.is_some(), self.registered())
    }
    /// The start-eligibility predicate the host wires into its web process supervisor:
    /// [`Service::web_eligible`], evaluated live on each supervisor poll.
    pub(crate) fn web_eligibility(self: &Arc<Self>) -> Arc<dyn Fn() -> bool + Send + Sync> {
        let service = Arc::clone(self);
        Arc::new(move || service.web_eligible())
    }
    /// Whether browsers can be served: eligible, and an admitted web process is serving
    /// (the supervisor publishes `Running` only between admission and exit/EOF).
    pub(crate) fn web_ready(&self) -> bool {
        web_serving(
            self.authentication.is_some(),
            self.registered(),
            *crate::sync::lock(&self.gateway),
        )
    }
    fn registered(&self) -> bool {
        self.instance_id.is_some()
    }
    #[cfg(test)]
    pub(crate) fn recovery_for_test(&self) -> Arc<crate::recovery::Recovery> {
        Arc::clone(self.recovery.as_ref().expect("selected test host"))
    }
    /// Browser-session ops (A3 B4, B2 Q8), only on the admitted web process's control
    /// connection. Each `session.ended` push is written before the reply on the
    /// connection's one ordered writer, after the record's delete.
    fn browser_op(&self, request: &Request, connection: &Arc<Connection>) -> bool {
        use org_host_control::browser::{BrowserOpenChannel, BrowserSessionGrant, InstanceView};
        if !is_browser_op(request) || connection.gateway_listen().is_none() {
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
        } else {
            browser_session_op(authentication, request, connection);
        }
        true
    }
    fn open_browser_channel(
        &self,
        authentication: &Authentication,
        request: &Request,
        connection: &Arc<Connection>,
    ) {
        use org_host_control::browser::{BrowserOpenChannel, SessionValidateParams};
        let mut ended = Vec::new();
        let result = browser_now_ms().and_then(|now| {
            let p = request
                .params::<BrowserOpenChannel>()
                .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "invalid channel request"))?;
            let account = authentication.browser_account(
                &SessionValidateParams {
                    cookie_digest: p.auth.cookie_digest,
                    proof_digest: p.auth.proof_digest,
                    origin: p.auth.origin.clone(),
                },
                now,
                &mut ended,
            )?;
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
    fn authenticate(&self, request: &Request, connection: &Arc<Connection>) -> bool {
        use org_host_control::ops::{
            AccountVerify, FdsReceived, OfficeOpenChannel, SessionEnd, SessionGrant,
            SessionInspect, SessionRenew,
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
        } else if request.is::<OfficeOpenChannel>() {
            self.open_authenticated_channel(authentication, request, connection);
        } else if request.is::<SessionGrant>() {
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
                let offices = self.offices.offices();
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
                    .open_channel(connection, &p.session, endpoint, &office.organization_id)
                    .map(|(id, socket, witness)| (id, socket, witness, p.office_id))
            });
        match result {
            Ok((id, socket, witness, office)) => {
                deliver_channel(connection, request.id, id, socket, &office, witness);
            }
            Err(error) => connection.send_response(Response::error(request.id, error)),
        }
    }
}

fn is_browser_op(request: &Request) -> bool {
    use org_host_control::browser::{
        BrowserOpenChannel, BrowserSessionEnd, BrowserSessionGrant, InstanceView, LinkRegister,
        SessionSignIn, SessionValidate,
    };
    request.is::<LinkRegister>()
        || request.is::<SessionSignIn>()
        || request.is::<SessionValidate>()
        || request.is::<BrowserSessionEnd>()
        || request.is::<InstanceView>()
        || request.is::<BrowserOpenChannel>()
        || request.is::<BrowserSessionGrant>()
}

fn browser_session_op(authentication: &Authentication, request: &Request, connection: &Connection) {
    use org_host_control::browser::{
        BrowserSessionEnd, LinkRegister, SessionSignIn, SessionValidate,
    };
    let invalid = |_| ApiError::new(ErrorCode::InvalidRequest, "invalid browser session request");
    let mut ended = Vec::new();
    let now = browser_now_ms();
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
    connection.send_response(reply.unwrap_or_else(|e| Response::error(request.id, e)));
}

fn browser_now_ms() -> Result<i64, ApiError> {
    authentication::wall_ms().and_then(|ms| {
        i64::try_from(ms).map_err(|_| ApiError::new(ErrorCode::Unavailable, "clock unavailable"))
    })
}

fn finish_ended_browser_sessions(
    authentication: &Authentication,
    connection: &Connection,
    ended: Vec<crate::sessions::Ended>,
) {
    use org_host_control::browser::{SessionEnded, SESSION_ENDED};
    authentication.revoke_browser_sessions(&ended);
    for session in ended {
        let event = SessionEnded {
            session_id: session.id,
        };
        if let Ok(data) = serde_json::to_value(&event) {
            connection.send_event(SESSION_ENDED, data);
        }
    }
}

/// Deliver an already authorized channel. A definite local reservation refusal
/// answers this request; uncertain transport still terminates the connection.
fn deliver_channel(
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
fn web_reports_serving(request: &Request, connection: &Arc<Connection>) {
    use crate::service::listener::ServingReport;
    use org_host_control::ops::WebServing;
    let refused = |code, message| Response::error(request.id, ApiError::new(code, message));
    if connection.gateway_listen().is_none() {
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

fn session_auth_method(request: &Request) -> bool {
    use org_host_control::ops::{
        AccountVerify, OfficeOpenChannel, SessionEnd, SessionGrant, SessionInspect, SessionRenew,
    };
    request.is::<AccountVerify>()
        || request.is::<SessionRenew>()
        || request.is::<SessionEnd>()
        || request.is::<OfficeOpenChannel>()
        || request.is::<SessionGrant>()
        || request.is::<SessionInspect>()
}

fn acknowledge_descriptors(request: &Request, connection: &Arc<Connection>) {
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

impl Handler for Service {
    fn disconnected(&self, connection: &Arc<Connection>) {
        connection.clear_recovery_permission();
        if let Some(authentication) = &self.authentication {
            authentication.disconnect(connection);
        }
    }
    /// Decide handshake refusals before a later pipelined request can change the state.
    fn inline(&self, request: &Request, connection: &Connection) -> bool {
        !connection.hello_completed()
            || request.is::<Hello>()
            || request.is::<org_host_control::ops::FdsReceived>()
            || crate::recovery::handles(request)
            // Waits for the presence dialog; it must see EOF without consuming a later request.
            || request.is::<org_host_control::admin::AdminReset>()
    }

    fn handle(&self, request: Request, connection: &Arc<Connection>, received: Received) {
        let Some(instance_id) = self.instance_id.as_deref() else {
            connection.send_response(Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::NotAuthorized,
                    "this host has no registered instance; run setup",
                ),
            ));
            return;
        };
        if request.is::<org_host_control::ops::WebServing>() {
            web_reports_serving(&request, connection);
            return;
        }
        if !connection.hello_completed() && !request.is::<Hello>() {
            connection.send_response(Response::error(
                request.id,
                ApiError::new(ErrorCode::InvalidRequest, "hello must come first"),
            ));
            return;
        }
        if request.is::<org_host_control::ops::TlsFetch>() {
            self.fetch_tls(&request, connection);
            return;
        }
        if self.set_tls(&request, connection, received) {
            return;
        }
        if self.administer(&request, connection, received) {
            return;
        }
        if self.recover(&request, connection, received) {
            return;
        }
        if let Some(reply) = organizations::dispatch(
            &request,
            connection.gateway_listen().is_some(),
            self.authentication.as_deref(),
            self.provisioning.as_ref(),
            &mut |name, data| connection.send_event(name, data),
        ) {
            #[cfg(test)]
            if connection.gateway_listen().is_some() {
                self.organization_ops
                    .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            }
            connection.send_response(reply);
            return;
        }
        if self.browser_op(&request, connection) {
            return;
        }
        if self.authenticate(&request, connection) {
            return;
        }
        let Some(operation) = OpenOperation::from_method(&request.method) else {
            connection.send_response(not_authorized(request.id));
            return;
        };
        match operation {
            OpenOperation::Hello => {
                let Ok(params) = request.params::<Hello>() else {
                    connection.send_response(params_dont_fit(request.id));
                    return;
                };
                // Host control speaks only version1; every connection is fixed at that version.
                if !params.api_range().contains(org_host_control::API_VERSION) {
                    connection.refuse_and_finish(Response::error(
                        request.id,
                        ApiError::new(
                            ErrorCode::UnsupportedVersion,
                            "no common host control version",
                        ),
                    ));
                    return;
                }
                let mut result = hello(instance_id);
                result.api_version = org_host_control::API_VERSION;
                connection.complete_hello();
                // `account.changed` goes to the admitted web connection and to no other.
                // The hello answer is queued under the same lock that adds the connection,
                // so no event is written ahead of it.
                if let (true, Some(provisioning)) = (
                    connection.gateway_listen().is_some(),
                    self.provisioning.as_ref(),
                ) {
                    let subscriber: Arc<dyn crate::broadcast::Subscriber> = connection.clone();
                    provisioning
                        .web()
                        .subscribe(Arc::downgrade(&subscriber), || {
                            connection.send_response(ok::<Hello>(request.id, &result));
                            true
                        });
                    return;
                }
                connection.send_response(ok::<Hello>(request.id, &result));
            }
            OpenOperation::InstanceStatus => {
                self.broadcast.snapshot(|| {
                    connection.send_response(answer::<InstanceStatus>(&request, |_| {
                        let administrator = self.administrator()?;
                        let offices = self.offices.offices();
                        Ok(InstanceStatusResult {
                            instance_id: instance_id.to_owned(),
                            mode: InstanceMode::Service,
                            gateway: WebStatus {
                                state: *crate::sync::lock(&self.gateway),
                            },
                            offices: OfficeCounts {
                                total: offices.len() as u64,
                                active: offices
                                    .iter()
                                    .filter(|o| o.status == org_proto::OfficeStatus::Active)
                                    .count() as u64,
                                failed: offices
                                    .iter()
                                    .filter(|o| o.status.requires_operator_action())
                                    .count() as u64,
                            },
                            administrator,
                        })
                    }))
                });
            }
        }
    }
}

pub(crate) fn secret_response(connection: &Connection, id: u64, result: &impl serde::Serialize) {
    #[derive(serde::Serialize)]
    struct SecretResponse<'a, T: ?Sized> {
        id: u64,
        result: &'a T,
    }
    struct Count(usize);
    impl std::io::Write for Count {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .filter(|n| *n < org_proto::MAX_FRAME_BYTES)
                .ok_or_else(|| std::io::Error::other("secret response limit"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let value = SecretResponse { id, result };
    let mut count = Count(0);
    if serde_json::to_writer(&mut count, &value).is_ok() {
        // Measure first so encoding cannot leave a secret in a grown buffer.
        let mut bytes = Vec::with_capacity(count.0 + 1);
        if serde_json::to_writer(&mut bytes, &value).is_ok() {
            bytes.push(b'\n');
            connection.send_secret_line(listener::SecretLine::new(bytes));
            return;
        }
        crate::secrets::zero(&mut bytes);
    }
    connection.send_response(Response::error(
        id,
        ApiError::new(ErrorCode::StorageError, "secret result unavailable"),
    ));
}

#[derive(Debug, PartialEq, Eq)]
enum OpenOperation {
    Hello,
    InstanceStatus,
}

impl OpenOperation {
    fn from_method(method: &str) -> Option<Self> {
        match method {
            Hello::METHOD => Some(Self::Hello),
            InstanceStatus::METHOD => Some(Self::InstanceStatus),
            _ => None,
        }
    }
}

fn not_authorized(id: u64) -> Response {
    Response::error(
        id,
        ApiError::new(
            ErrorCode::NotAuthorized,
            "this request is not authorized on host.sock",
        ),
    )
}

fn params_dont_fit(id: u64) -> Response {
    Response::error(
        id,
        ApiError::new(
            ErrorCode::InvalidRequest,
            "the params don't fit this method",
        ),
    )
}

/// Host control names its immutable registered instance and advertises no retired calls.
fn hello(instance_id: &str) -> HelloResult {
    HelloResult::host(env!("CARGO_PKG_VERSION"), THIS_MAC).with_instance_id(instance_id)
}

/// Runs a typed handler: params that don't decode are `invalid_request`.
fn answer<C: Call>(
    request: &Request,
    handler: impl FnOnce(C::Params) -> Result<C::Result, ApiError>,
) -> Response {
    let params = match request.params::<C>() {
        Ok(params) => params,
        Err(_) => {
            return Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::InvalidRequest,
                    "the params don't fit this method",
                ),
            );
        }
    };
    match handler(params) {
        Ok(result) => ok::<C>(request.id, &result),
        Err(error) => Response::error(request.id, error),
    }
}

/// A successful answer to request `id` (or `storage_error` if it can't be encoded).
pub(crate) fn ok<C: Call>(id: u64, result: &C::Result) -> Response {
    Response::ok::<C>(id, result).unwrap_or_else(|_| {
        Response::error(
            id,
            ApiError::new(ErrorCode::StorageError, "the answer couldn't be encoded"),
        )
    })
}

/// The start-eligibility rule: the instance is registered and host authentication is
/// present. HQ being Active is not required. Persisted listener configuration alone never
/// qualifies.
pub(crate) fn web_start_eligible(authenticated: bool, registered: bool) -> bool {
    authenticated && registered
}

/// The readiness rule: all three conditions, never persisted state alone.
pub(crate) fn web_serving(authenticated: bool, registered: bool, state: WebState) -> bool {
    web_start_eligible(authenticated, registered) && state == WebState::Running
}

#[cfg(test)]
pub(crate) mod tests {
    #[test]
    fn web_start_needs_authentication_and_a_registered_instance() {
        use super::web_start_eligible;
        assert!(web_start_eligible(true, true));
        assert!(!web_start_eligible(false, true), "no authentication");
        assert!(!web_start_eligible(true, false), "not registered");
        assert!(!web_start_eligible(false, false));
    }

    /// The service fixture, and the predicate the host wires into its web process
    /// supervisor, refuse to start the web process without authentication or without a
    /// registered instance, and allow it once both hold, before any organization or
    /// office exists (lead ruling: the person registers and creates the organization
    /// in the browser).
    #[test]
    fn the_wired_web_eligibility_needs_authentication_and_registration_but_no_office() {
        let home = tempfile::tempdir().unwrap();
        let bare = Arc::new(service(home.path()));
        assert!(!bare.web_eligible(), "no authentication");
        assert!(!(bare.web_eligibility())(), "wired: no authentication");
        let (_root, authentication, _connection) = crate::service::authentication::tests::fixture();
        let authentication = Arc::new(authentication);
        let unregistered =
            Arc::new(service(home.path()).with_authentication(Some(Arc::clone(&authentication))));
        assert!(unregistered.instance_id.is_none());
        assert!(!unregistered.web_eligible(), "not registered");
        assert!(!(unregistered.web_eligibility())(), "wired: not registered");
        let mut registered = service(home.path()).with_authentication(Some(authentication));
        registered.instance_id = Some(crate::ids::uuid_v7().unwrap());
        let registered = Arc::new(registered);
        assert!(registered.offices.offices().is_empty(), "no office at all");
        assert!(registered.web_eligible(), "registered, no organization yet");
        assert!((registered.web_eligibility())(), "wired: registered");
        assert!(!registered.web_ready(), "eligible is not serving");
        bare.offices.shutdown();
        unregistered.offices.shutdown();
        registered.offices.shutdown();
    }

    #[test]
    fn web_ready_needs_authentication_registration_and_a_serving_web() {
        use super::web_serving;
        assert!(web_serving(true, true, WebState::Running));
        for state in [
            WebState::NotConfigured,
            WebState::Starting,
            WebState::Failed,
            WebState::Unknown,
        ] {
            assert!(!web_serving(true, true, state), "{state:?}");
        }
        assert!(!web_serving(false, true, WebState::Running));
        assert!(!web_serving(true, false, WebState::Running));
    }

    use super::*;
    use std::path::Path;

    use org_proto::{Empty, HelloParams, OfficeList, OfficeRestart, OfficeRestartParams};

    use crate::catalog::Catalog;
    use crate::keys::memory::MemoryKeys;
    use crate::supervisor::{Supervisor, SupervisorConfig};
    use crate::timings::Timings;

    #[test]
    fn only_hello_and_instance_status_have_dispatch_variants() {
        assert_eq!(
            OpenOperation::from_method("hello"),
            Some(OpenOperation::Hello)
        );
        assert_eq!(
            OpenOperation::from_method("instance.status"),
            Some(OpenOperation::InstanceStatus)
        );
        for method in [
            "organization.create",
            "organization.list",
            "office.list",
            "subscribe",
            "office.app_token",
            "office.restart",
            "account.verify",
            "bootstrap",
            "exit",
            "session.grant",
            "recovery.release",
            "tls.fetch",
            "fds.received",
            "future.method",
            "",
            "Hello",
            "instance.status.extra",
        ] {
            assert_eq!(OpenOperation::from_method(method), None, "{method}");
        }
    }

    #[test]
    fn status_reports_whether_an_administrator_exists() {
        let home = tempfile::tempdir().unwrap();
        let unregistered = service(home.path());
        assert_eq!(unregistered.administrator().unwrap(), Administrator::None);
        unregistered.offices.shutdown();
        let (_root, auth, _connection) = authentication::tests::fixture();
        let registered = service(home.path()).with_authentication(Some(Arc::new(auth)));
        // The fixture seeds an administrator account.
        assert_eq!(registered.administrator().unwrap(), Administrator::Present);
        registered.offices.shutdown();
    }

    pub(crate) fn service(home: &Path) -> Service {
        let supervisor = Supervisor::new(
            SupervisorConfig {
                program: home.join("no-such-server"),
                home: home.to_path_buf(),
                runtime_dir: None,
                logs_dir: home.join("logs"),
                log_max_bytes: 1024,
                timings: Timings::default(),
                keys: Arc::new(MemoryKeys::default()),
                server_check: Arc::new(crate::server_check::ServerCheck::Skip),
                sweep_orphans: false,
            },
            "01JD5Z00000000000000000NST",
        );
        let broadcast = Arc::new(Broadcast::default());
        let offices = Offices::start(
            home,
            Catalog::default(),
            Vec::new(),
            supervisor,
            Arc::clone(&broadcast),
            &Timings::default(),
        );
        Service::new(offices, broadcast, None)
    }

    /// A service fixture with an instance id, as an admitted web process's handler, and
    /// its offices for shutdown.
    pub(crate) fn web_handler(home: &Path) -> (Arc<Service>, Arc<Offices>) {
        let mut service = service(home);
        service.instance_id = Some(crate::ids::uuid_v7().unwrap());
        let offices = service.offices.clone();
        (Arc::new(service), offices)
    }

    /// The real handler on an admitted web process control connection, plus its peer end.
    fn web_peer(
        home: &Path,
    ) -> (
        listener::WebConnection,
        std::io::BufReader<std::os::unix::net::UnixStream>,
        Arc<Offices>,
    ) {
        web_peer_on(
            home,
            r#"{"address":"127.0.0.1","port":48123,"scheme":"http_loopback","origin":"http://127.0.0.1:48123","hsts":"off"}"#,
        )
    }

    /// [`web_peer`] whose web process listens on `listen` (JSON).
    fn web_peer_on(
        home: &Path,
        listen: &str,
    ) -> (
        listener::WebConnection,
        std::io::BufReader<std::os::unix::net::UnixStream>,
        Arc<Offices>,
    ) {
        web_peer_for(service(home), listen)
    }

    /// [`web_peer_on`] over a service the caller prepared.
    fn web_peer_for(
        mut service: Service,
        listen: &str,
    ) -> (
        listener::WebConnection,
        std::io::BufReader<std::os::unix::net::UnixStream>,
        Arc<Offices>,
    ) {
        service.instance_id = Some(crate::ids::uuid_v7().unwrap());
        let offices = service.offices.clone();
        let (host, peer) = std::os::unix::net::UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        let (transfer, _child) = std::os::unix::net::UnixDatagram::pair().unwrap();
        let listen = serde_json::from_str(listen).unwrap();
        let web =
            listener::WebConnection::start(host, transfer, listen, Arc::new(service)).unwrap();
        (web, std::io::BufReader::new(peer), offices)
    }

    fn exchange<C: Call>(
        peer: &mut std::io::BufReader<std::os::unix::net::UnixStream>,
        id: u64,
        params: &C::Params,
    ) -> Option<org_proto::Response> {
        org_proto::frame::write_message(peer.get_mut(), &Request::new::<C>(id, params).unwrap())
            .unwrap();
        org_proto::frame::read_message(peer).ok().flatten()
    }

    fn refused(response: Option<org_proto::Response>, code: ErrorCode) -> bool {
        matches!(response.map(|r| r.outcome),
            Some(org_proto::Outcome::Error(error)) if error.code == code)
    }

    fn wait_closed(web: &listener::WebConnection) {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while web.live() && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!web.live(), "protocol error closes the control connection");
    }

    #[test]
    fn web_serving_is_accepted_once_after_hello_and_tls_fetch() {
        use org_host_control::ops::{TlsFetch, WebServing};
        let home = tempfile::tempdir().unwrap();
        let (web, mut peer, offices) = web_peer(home.path());
        exchange::<Hello>(&mut peer, 1, &Default::default())
            .unwrap()
            .into_result::<Hello>()
            .unwrap();
        exchange::<TlsFetch>(&mut peer, 2, &Default::default())
            .unwrap()
            .into_result::<TlsFetch>()
            .unwrap();
        assert!(!web.reported_serving());
        exchange::<WebServing>(&mut peer, 3, &Default::default())
            .unwrap()
            .into_result::<WebServing>()
            .unwrap();
        assert!(web.reported_serving());
        assert!(web.live());
        assert!(!web.serving_refused());
        // A duplicate is refused and closes the control connection.
        assert!(refused(
            exchange::<WebServing>(&mut peer, 4, &Default::default()),
            ErrorCode::InvalidRequest
        ));
        wait_closed(&web);
        // The supervisor logs the refusal, not only the EOF that follows it.
        assert!(web.serving_refused());
        drop(web);
        offices.shutdown();
    }

    #[test]
    fn web_serving_with_params_that_do_not_fit_is_a_protocol_error() {
        use org_host_control::ops::TlsFetch;
        let home = tempfile::tempdir().unwrap();
        let (web, mut peer, offices) = web_peer(home.path());
        exchange::<Hello>(&mut peer, 1, &Default::default())
            .unwrap()
            .into_result::<Hello>()
            .unwrap();
        exchange::<TlsFetch>(&mut peer, 2, &Default::default())
            .unwrap()
            .into_result::<TlsFetch>()
            .unwrap();
        // In order, but its params don't fit `web.serving`.
        let request: Request = serde_json::from_value(serde_json::json!({
            "id": 3,
            "method": "web.serving",
            "params": {"ready": true},
        }))
        .unwrap();
        org_proto::frame::write_message(peer.get_mut(), &request).unwrap();
        let response = org_proto::frame::read_message(&mut peer).ok().flatten();
        assert!(refused(response, ErrorCode::InvalidRequest));
        wait_closed(&web);
        assert!(!web.reported_serving());
        assert!(web.serving_refused());
        drop(web);
        offices.shutdown();
    }

    /// An `https` listener with its stored pair: `tls.fetch` answers the chain and the key
    /// for the snapshot's origin, and the web process may then report that it serves. A pair
    /// that does not name the origin is `unavailable` with `tls_name_mismatch`, and one whose
    /// key is gone `storage_error`.
    #[test]
    fn tls_fetch_on_an_https_listener_answers_the_stored_pair_and_enables_serving() {
        use org_host_control::listen::ListenFailure;
        use org_host_control::ops::{TlsFetch, TlsFetchResult, WebServing};
        let listen = r#"{"address":"127.0.0.1","port":8443,"scheme":"https","origin":"https://127.0.0.1:8443","hsts":"on"}"#;
        let home = tempfile::tempdir().unwrap();
        let (kept, tls) = crate::web_tls::tests::started("https://127.0.0.1:8443");
        let (web, mut peer, offices) = web_peer_for(
            service(home.path()).with_web_tls(Some(Arc::clone(&tls))),
            listen,
        );
        exchange::<Hello>(&mut peer, 1, &Default::default())
            .unwrap()
            .into_result::<Hello>()
            .unwrap();
        let fetched = exchange::<TlsFetch>(&mut peer, 2, &Default::default())
            .unwrap()
            .into_result::<TlsFetch>()
            .unwrap();
        let TlsFetchResult::Https {
            cert,
            key,
            origin,
            hsts,
        } = fetched
        else {
            panic!("not the https mode: {fetched:?}");
        };
        assert_eq!(origin.as_str(), "https://127.0.0.1:8443");
        assert_eq!(hsts, org_host_control::listen::Hsts::On);
        let (chain, stored) = tls.fetch().unwrap();
        assert_eq!(cert, chain);
        assert_eq!(key.expose(), stored.expose());
        exchange::<WebServing>(&mut peer, 3, &Default::default())
            .unwrap()
            .into_result::<WebServing>()
            .unwrap();
        assert!(web.reported_serving());
        drop(web);
        offices.shutdown();
        // Another host's pair, then no key.
        crate::web_tls::tests::store_pair_of(&kept, &tls, "https://elsewhere.example");
        let home = tempfile::tempdir().unwrap();
        let (web, mut peer, offices) = web_peer_for(
            service(home.path()).with_web_tls(Some(Arc::clone(&tls))),
            listen,
        );
        exchange::<Hello>(&mut peer, 1, &Default::default())
            .unwrap()
            .into_result::<Hello>()
            .unwrap();
        let fetched = exchange::<TlsFetch>(&mut peer, 2, &Default::default())
            .unwrap()
            .into_result::<TlsFetch>()
            .unwrap();
        let TlsFetchResult::Unavailable(ListenFailure::TlsNameMismatch { uncovered }) = fetched
        else {
            panic!("not a name mismatch: {fetched:?}");
        };
        assert_eq!(uncovered.origins()[0].as_str(), "https://127.0.0.1:8443");
        crate::web_tls::tests::lose_keys(&kept);
        assert!(refused(
            exchange::<TlsFetch>(&mut peer, 3, &Default::default()),
            ErrorCode::StorageError
        ));
        drop(web);
        offices.shutdown();
    }

    /// A published plain-HTTP listener is answered as that mode, with its origin and no
    /// pair, and the web process may then report that it serves.
    #[test]
    fn tls_fetch_on_a_published_listener_answers_http_published_and_enables_serving() {
        use org_host_control::ops::{TlsFetch, TlsFetchResult, WebServing};
        let home = tempfile::tempdir().unwrap();
        let (web, mut peer, offices) = web_peer_on(
            home.path(),
            r#"{"address":"0.0.0.0","port":8443,"scheme":"http_published","origin":"http://127.0.0.1:8443","hsts":"off"}"#,
        );
        exchange::<Hello>(&mut peer, 1, &Default::default())
            .unwrap()
            .into_result::<Hello>()
            .unwrap();
        let fetched = exchange::<TlsFetch>(&mut peer, 2, &Default::default())
            .unwrap()
            .into_result::<TlsFetch>()
            .unwrap();
        let TlsFetchResult::HttpPublished { origin, hsts } = fetched else {
            panic!("not the published mode: {fetched:?}");
        };
        assert_eq!(origin.as_str(), "http://127.0.0.1:8443");
        assert_eq!(hsts, org_host_control::listen::Hsts::Off);
        exchange::<WebServing>(&mut peer, 3, &Default::default())
            .unwrap()
            .into_result::<WebServing>()
            .unwrap();
        assert!(web.reported_serving());
        drop(web);
        offices.shutdown();
    }

    /// An `https` listener on a host without its pair (no registry): `tls.fetch` is the plain
    /// error `storage_error`, and the web process may not report that it serves.
    #[test]
    fn tls_fetch_on_an_https_listener_without_a_pair_is_refused_and_never_enables_serving() {
        use org_host_control::ops::{TlsFetch, WebServing};
        let home = tempfile::tempdir().unwrap();
        let (web, mut peer, offices) = web_peer_on(
            home.path(),
            r#"{"address":"127.0.0.1","port":8443,"scheme":"https","origin":"https://127.0.0.1:8443","hsts":"off"}"#,
        );
        exchange::<Hello>(&mut peer, 1, &Default::default())
            .unwrap()
            .into_result::<Hello>()
            .unwrap();
        assert!(refused(
            exchange::<TlsFetch>(&mut peer, 2, &Default::default()),
            ErrorCode::StorageError
        ));
        assert!(refused(
            exchange::<WebServing>(&mut peer, 3, &Default::default()),
            ErrorCode::InvalidRequest
        ));
        wait_closed(&web);
        assert!(!web.reported_serving());
        drop(web);
        offices.shutdown();
    }

    #[test]
    fn web_serving_before_tls_fetch_is_a_protocol_error() {
        use org_host_control::ops::WebServing;
        let home = tempfile::tempdir().unwrap();
        let (web, mut peer, offices) = web_peer(home.path());
        exchange::<Hello>(&mut peer, 1, &Default::default())
            .unwrap()
            .into_result::<Hello>()
            .unwrap();
        assert!(refused(
            exchange::<WebServing>(&mut peer, 2, &Default::default()),
            ErrorCode::InvalidRequest
        ));
        wait_closed(&web);
        assert!(!web.reported_serving());
        assert!(web.serving_refused());
        drop(web);
        offices.shutdown();
    }

    #[test]
    fn web_serving_from_a_local_connection_is_not_authorized() {
        use org_host_control::ops::WebServing;
        let home = tempfile::tempdir().unwrap();
        let mut service = service(home.path());
        service.instance_id = Some(crate::ids::uuid_v7().unwrap());
        let offices = service.offices.clone();
        let (host, peer) = std::os::unix::net::UnixStream::pair().unwrap();
        // Every host.sock connection is `Local`.
        let local = listener::LocalConnection::start(host, Arc::new(service)).unwrap();
        peer.set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        let mut peer = std::io::BufReader::new(peer);
        exchange::<Hello>(&mut peer, 1, &Default::default())
            .unwrap()
            .into_result::<Hello>()
            .unwrap();
        assert!(refused(
            exchange::<WebServing>(&mut peer, 2, &Default::default()),
            ErrorCode::NotAuthorized
        ));
        assert!(local.live());
        drop(local);
        offices.shutdown();
    }

    fn browser_sign_in(seed: u8) -> org_host_control::browser::SessionSignInParams {
        use org_host_control::browser::{BrowserOrigin, Digest32, SessionSignInParams};
        SessionSignInParams {
            login: "owner".into(),
            password: org_host_control::Password::new("synthetic test password".into()).unwrap(),
            client_address: "192.0.2.1".parse().unwrap(),
            cookie_digest: Digest32([seed; 32]),
            proof_digest: Digest32([seed ^ 0xff; 32]),
            origin: BrowserOrigin {
                scheme: "https".into(),
                host: "org.example".into(),
                port: 443,
            },
        }
    }

    /// `session.end` deletes the record, then writes `session.ended` before its
    /// reply on the web connection's one ordered writer; a later validate fails.
    #[test]
    fn browser_session_end_pushes_session_ended_before_its_reply_on_the_web_connection() {
        use org_host_control::browser::{
            BrowserSessionEnd, BrowserSessionEndParams, SessionEnded, SessionSignIn,
            SessionValidate, SessionValidateParams, SESSION_ENDED,
        };
        let home = tempfile::tempdir().unwrap();
        let (_root, authentication, _c) = crate::service::authentication::tests::fixture();
        let service = service(home.path()).with_authentication(Some(Arc::new(authentication)));
        let (web, mut peer, offices) = web_peer_for(
            service,
            r#"{"address":"127.0.0.1","port":48123,"scheme":"http_loopback","origin":"http://127.0.0.1:48123","hsts":"off"}"#,
        );
        exchange::<Hello>(&mut peer, 1, &Default::default())
            .unwrap()
            .into_result::<Hello>()
            .unwrap();
        let signed_in = browser_sign_in(1);
        let session = exchange::<SessionSignIn>(&mut peer, 2, &signed_in)
            .unwrap()
            .into_result::<SessionSignIn>()
            .unwrap();
        org_proto::frame::write_message(
            peer.get_mut(),
            &Request::new::<BrowserSessionEnd>(
                3,
                &BrowserSessionEndParams {
                    session_id: session.session_id.clone(),
                },
            )
            .unwrap(),
        )
        .unwrap();
        let event: org_proto::Event = org_proto::frame::read_message(&mut peer).unwrap().unwrap();
        assert_eq!(event.event, SESSION_ENDED);
        let ended: SessionEnded = serde_json::from_value(event.data).unwrap();
        assert_eq!(ended.session_id, session.session_id);
        let reply: org_proto::Response =
            org_proto::frame::read_message(&mut peer).unwrap().unwrap();
        assert_eq!(reply.id, 3);
        reply.into_result::<BrowserSessionEnd>().unwrap();
        let validate = SessionValidateParams {
            cookie_digest: signed_in.cookie_digest,
            proof_digest: signed_in.proof_digest,
            origin: signed_in.origin,
        };
        assert!(refused(
            exchange::<SessionValidate>(&mut peer, 4, &validate),
            ErrorCode::NotAuthorized
        ));
        drop(web);
        offices.shutdown();
    }

    /// `instance.view` on the web connection: `unavailable` until the registered start
    /// installs the name, then the name, mode, web state and administrator presence.
    /// On host.sock it is `not_authorized`. Every accepted op counts once.
    #[test]
    fn instance_view_is_served_only_on_the_web_connection_after_the_name_is_installed() {
        use org_host_control::browser::{InstanceView, InstanceViewResult};
        use org_host_control::{Administrator, InstanceMode, WebState};
        let home = tempfile::tempdir().unwrap();
        let (_root, authentication, _c) = crate::service::authentication::tests::fixture();
        let authentication = Arc::new(authentication);
        let mut service = service(home.path()).with_authentication(Some(authentication.clone()));
        service.instance_id = Some(crate::ids::uuid_v7().unwrap());
        let service = Arc::new(service);
        let (host, peer) = std::os::unix::net::UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        let (transfer, _child) = std::os::unix::net::UnixDatagram::pair().unwrap();
        let listen = serde_json::from_str(
            r#"{"address":"127.0.0.1","port":48123,"scheme":"http_loopback","origin":"http://127.0.0.1:48123","hsts":"off"}"#,
        )
        .unwrap();
        let web = listener::WebConnection::start(
            host,
            transfer,
            listen,
            service.clone() as Arc<dyn Handler>,
        )
        .unwrap();
        let mut peer = std::io::BufReader::new(peer);
        exchange::<Hello>(&mut peer, 1, &Default::default())
            .unwrap()
            .into_result::<Hello>()
            .unwrap();
        assert!(refused(
            exchange::<InstanceView>(&mut peer, 2, &Default::default()),
            ErrorCode::Unavailable
        ));
        assert!(service.install_instance_name("studio"));
        assert!(!service.install_instance_name("other"), "installed once");
        let view = exchange::<InstanceView>(&mut peer, 3, &Default::default())
            .unwrap()
            .into_result::<InstanceView>()
            .unwrap();
        assert_eq!(
            view,
            InstanceViewResult {
                name: "studio".into(),
                status: InstanceMode::Service,
                web_status: WebState::NotConfigured,
                administrator: Administrator::Present,
                tls: None,
            }
        );
        assert_eq!(service.browser_ops(), 2);
        // Unknown params are refused, not ignored.
        org_proto::frame::write_message(
            peer.get_mut(),
            &serde_json::json!({"id": 4, "method": InstanceView::METHOD, "params": {"path": "/"}}),
        )
        .unwrap();
        let response: org_proto::Response =
            org_proto::frame::read_message(&mut peer).unwrap().unwrap();
        assert!(matches!(
            response.outcome,
            org_proto::Outcome::Error(ref e) if e.code == ErrorCode::InvalidRequest
        ));
        drop(web);

        let (host, peer) = std::os::unix::net::UnixStream::pair().unwrap();
        let local =
            listener::LocalConnection::start(host, service.clone() as Arc<dyn Handler>).unwrap();
        peer.set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        let mut peer = std::io::BufReader::new(peer);
        exchange::<Hello>(&mut peer, 1, &Default::default())
            .unwrap()
            .into_result::<Hello>()
            .unwrap();
        assert!(refused(
            exchange::<InstanceView>(&mut peer, 2, &Default::default()),
            ErrorCode::NotAuthorized
        ));
        assert_eq!(service.browser_ops(), 3, "host.sock reaches no browser op");
        drop(local);
        service.offices.shutdown();
    }

    /// A local (host.sock) connection cannot create or validate browser sessions:
    /// every browser op is `not_authorized` there and writes no session or throttle state.
    #[test]
    fn browser_session_ops_off_the_web_connection_are_refused() {
        use org_host_control::browser::{
            LinkRegister, SessionSignIn, SessionValidate, SessionValidateParams,
        };
        let home = tempfile::tempdir().unwrap();
        let (_root, authentication, _c) = crate::service::authentication::tests::fixture();
        let authentication = Arc::new(authentication);
        let mut service = service(home.path()).with_authentication(Some(authentication.clone()));
        service.instance_id = Some(crate::ids::uuid_v7().unwrap());
        let offices = service.offices.clone();
        let (host, peer) = std::os::unix::net::UnixStream::pair().unwrap();
        let local = listener::LocalConnection::start(host, Arc::new(service)).unwrap();
        peer.set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        let mut peer = std::io::BufReader::new(peer);
        exchange::<Hello>(&mut peer, 1, &Default::default())
            .unwrap()
            .into_result::<Hello>()
            .unwrap();
        // The right password: had it been served, a session would exist.
        let signed_in = browser_sign_in(2);
        assert!(refused(
            exchange::<SessionSignIn>(&mut peer, 2, &signed_in),
            ErrorCode::NotAuthorized
        ));
        // Six wrong passwords: had they counted, the login would now be locked.
        let mut wrong = browser_sign_in(3);
        wrong.password = org_host_control::Password::new("not the password".into()).unwrap();
        for id in 3..9 {
            assert!(refused(
                exchange::<SessionSignIn>(&mut peer, id, &wrong),
                ErrorCode::NotAuthorized
            ));
        }
        let validate = SessionValidateParams {
            cookie_digest: signed_in.cookie_digest,
            proof_digest: signed_in.proof_digest,
            origin: signed_in.origin.clone(),
        };
        assert!(refused(
            exchange::<SessionValidate>(&mut peer, 9, &validate),
            ErrorCode::NotAuthorized
        ));
        let digest = "ab".repeat(32);
        org_proto::frame::write_message(
            peer.get_mut(),
            &serde_json::json!({"id": 10, "method": LinkRegister::METHOD, "params": {
                "link": "A".repeat(43), "login": "ada", "password": "long enough password",
                "client_address": "192.0.2.1", "cookie_digest": digest, "proof_digest": digest,
                "origin": {"scheme": "https", "host": "org.example", "port": 443}}}),
        )
        .unwrap();
        let response: org_proto::Response =
            org_proto::frame::read_message(&mut peer).unwrap().unwrap();
        assert!(matches!(
            response.outcome,
            org_proto::Outcome::Error(ref e) if e.code == ErrorCode::NotAuthorized
        ));
        drop(local);
        offices.shutdown();
        // Nothing was stored: no session for the served digests, and the login
        // and address still have their whole budget.
        let now = i64::try_from(authentication::wall_ms().unwrap()).unwrap();
        let mut ended = Vec::new();
        assert_eq!(
            authentication
                .validate_browser(&validate, now, &mut ended)
                .unwrap_err()
                .code,
            ErrorCode::NotAuthorized
        );
        authentication
            .sign_in_browser(&browser_sign_in(4), now, &mut ended)
            .unwrap();
        assert!(ended.is_empty());
    }

    /// `tls.set` is the administrator's, on `host.sock` only: a bad session, an account that
    /// is not the administrator and the web connection are `not_authorized`; an instance
    /// without an `https` pair is `unavailable`; the administrator's pair is installed.
    #[test]
    fn tls_set_needs_the_administrators_session_on_host_sock() {
        use org_host_control::ops::{AccountVerify, AccountVerifyParams, TlsSet, TlsSetParams};
        use org_host_control::{HostOpResult, Password, SessionHandle, TlsPrivateKey};
        let (_root, authentication, _c) = crate::service::authentication::tests::fixture();
        let authentication = Arc::new(authentication);
        let verifier = authentication::hash_password(
            &Password::new("synthetic test password".into()).unwrap(),
        )
        .unwrap();
        authentication
            .provisioning_storage()
            .0
            .seed_account(
                &crate::ids::generate().unwrap(),
                "Member",
                &verifier,
                false,
                1,
            )
            .unwrap();
        let (_kept, tls) = crate::web_tls::tests::started("https://127.0.0.1:8443");
        let now = crate::web_tls::now().unwrap();
        let (cert, key) =
            crate::web_tls::tests::pair_for(&["127.0.0.1"], now - 60, now + 30 * 86_400);
        let params = |session: SessionHandle| TlsSetParams {
            session,
            operation_id: crate::ids::generate().unwrap(),
            cert: cert.clone(),
            key: TlsPrivateKey::new(key.clone()).unwrap(),
        };
        // The administrator calls' admission: one that admits this test's own executable as
        // the development `agentic-org` peer, or one that admits no unsigned peer.
        let admission = |admits: bool| {
            let policy = crate::app_auth::AppPolicy::new(crate::HOST_SIGNING_ID, None).unwrap();
            let policy = if admits {
                policy
                    .with_dev_app(&std::env::current_exe().unwrap())
                    .unwrap()
            } else {
                policy
            };
            Arc::new(crate::admin::Admin::new(
                authentication.provisioning_storage().0,
                &crate::ids::uuid_v7().unwrap(),
                org_host_control::listen::ListenOrigin::parse("https://127.0.0.1:8443").unwrap(),
                policy,
                Arc::new(crate::admin::NativePresence),
            ))
        };
        let local = |web_tls: Option<Arc<crate::web_tls::WebTls>>, admits: Option<bool>| {
            let home = tempfile::tempdir().unwrap();
            let mut service = service(home.path())
                .with_authentication(Some(Arc::clone(&authentication)))
                .with_web_tls(web_tls);
            if let Some(admits) = admits {
                assert!(service.install_admin(admission(admits)));
            }
            service.instance_id = Some(crate::ids::uuid_v7().unwrap());
            let offices = service.offices.clone();
            let (host, peer) = std::os::unix::net::UnixStream::pair().unwrap();
            let local = listener::LocalConnection::start(host, Arc::new(service)).unwrap();
            peer.set_read_timeout(Some(std::time::Duration::from_secs(10)))
                .unwrap();
            let mut peer = std::io::BufReader::new(peer);
            exchange::<Hello>(&mut peer, 1, &Default::default())
                .unwrap()
                .into_result::<Hello>()
                .unwrap();
            (home, local, peer, offices)
        };
        let sign_in = |peer: &mut std::io::BufReader<std::os::unix::net::UnixStream>,
                       login: &str| {
            exchange::<AccountVerify>(
                peer,
                2,
                &AccountVerifyParams {
                    login: login.into(),
                    password: Password::new("synthetic test password".into()).unwrap(),
                },
            )
            .unwrap()
            .into_result::<AccountVerify>()
            .unwrap()
            .session
        };
        // Before the administrator service is installed, and for a peer it does not admit
        // (an unsigned process of the same user, where the system names a peer's code), even
        // with the administrator's session. Where it names a peer by its user alone (Linux),
        // the user is the admission, as for every administrator call.
        let by_code = matches!(
            crate::test_programs::image_identity(),
            org_platform_host::admission::ImageIdentity::CodeSignature
        );
        let before = tls.fetch().unwrap().0;
        for admits in [None, Some(false)] {
            let (_home, connection, mut peer, offices) = local(Some(Arc::clone(&tls)), admits);
            let owner = sign_in(&mut peer, "Owner");
            let answer = exchange::<TlsSet>(&mut peer, 3, &params(owner));
            match admits {
                None => assert!(refused(answer, ErrorCode::Unavailable)),
                Some(_) if by_code => assert!(refused(answer, ErrorCode::NotAuthorized)),
                Some(_) => assert!(answer
                    .unwrap()
                    .into_result::<TlsSet>()
                    .is_ok_and(|r| matches!(r, HostOpResult::Completed { .. }))),
            }
            drop(connection);
            offices.shutdown();
        }
        let (_home, connection, mut peer, offices) = local(Some(Arc::clone(&tls)), Some(true));
        let bogus = SessionHandle::new("A".repeat(43)).unwrap();
        assert!(refused(
            exchange::<TlsSet>(&mut peer, 3, &params(bogus)),
            ErrorCode::NotAuthorized
        ));
        let member = sign_in(&mut peer, "Member");
        assert!(refused(
            exchange::<TlsSet>(&mut peer, 4, &params(member)),
            ErrorCode::NotAuthorized
        ));
        let owner = sign_in(&mut peer, "Owner");
        let answered = exchange::<TlsSet>(&mut peer, 5, &params(owner))
            .unwrap()
            .into_result::<TlsSet>()
            .unwrap();
        assert_eq!(
            answered,
            HostOpResult::Completed {
                result: org_proto::Empty {}
            }
        );
        let after = tls.fetch().unwrap().0;
        assert_ne!(after, before);
        drop(connection);
        offices.shutdown();
        // Without an https pair.
        let (_home, connection, mut peer, offices) = local(None, Some(true));
        let owner = sign_in(&mut peer, "Owner");
        assert!(refused(
            exchange::<TlsSet>(&mut peer, 3, &params(owner)),
            ErrorCode::Unavailable
        ));
        drop(connection);
        offices.shutdown();
        // On the web connection: refused before its params are read.
        let home = tempfile::tempdir().unwrap();
        let (web, mut peer, offices) = web_peer_for(
            service(home.path())
                .with_authentication(Some(Arc::clone(&authentication)))
                .with_web_tls(Some(Arc::clone(&tls))),
            r#"{"address":"127.0.0.1","port":8443,"scheme":"https","origin":"https://127.0.0.1:8443","hsts":"off"}"#,
        );
        exchange::<Hello>(&mut peer, 1, &Default::default())
            .unwrap()
            .into_result::<Hello>()
            .unwrap();
        assert!(refused(
            exchange::<TlsSet>(
                &mut peer,
                2,
                &params(SessionHandle::new("A".repeat(43)).unwrap())
            ),
            ErrorCode::NotAuthorized
        ));
        assert_eq!(tls.fetch().unwrap().0, after);
        drop(web);
        offices.shutdown();
    }

    /// Before the registered start installs the administrator service, both admin calls
    /// answer `unavailable` on the local channel rather than reaching any registry.
    #[test]
    fn admin_calls_are_unavailable_before_install_admin() {
        use org_host_control::admin::{AdminLinkInitial, AdminLinkInitialParams, AdminReset};
        let home = tempfile::tempdir().unwrap();
        let mut service = service(home.path());
        service.instance_id = Some(crate::ids::uuid_v7().unwrap());
        assert!(service.admin().is_none());
        let offices = service.offices.clone();
        let (host, peer) = std::os::unix::net::UnixStream::pair().unwrap();
        let local = listener::LocalConnection::start(host, Arc::new(service)).unwrap();
        peer.set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        let mut peer = std::io::BufReader::new(peer);
        exchange::<Hello>(&mut peer, 1, &Default::default())
            .unwrap()
            .into_result::<Hello>()
            .unwrap();
        let params = AdminLinkInitialParams {
            setup_operation: crate::admin::tests::OPERATION.into(),
        };
        assert!(refused(
            exchange::<AdminLinkInitial>(&mut peer, 2, &params),
            ErrorCode::Unavailable
        ));
        assert!(refused(
            exchange::<AdminReset>(&mut peer, 3, &Default::default()),
            ErrorCode::Unavailable
        ));
        assert!(local.live());
        drop(local);
        offices.shutdown();
    }

    /// An installed administrator service refuses both calls off a non-local channel (here
    /// the web process's) before any registry read, and issues no link.
    #[test]
    fn admin_calls_off_a_non_local_channel_are_refused_and_issue_nothing() {
        use crate::links::{LinkKind, OneTimeLinks, Subject};
        use org_host_control::admin::{AdminLinkInitial, AdminLinkInitialParams, AdminReset};
        let home = tempfile::tempdir().unwrap();
        let (_registry, admin) = crate::admin::tests::admin();
        let admin = Arc::new(admin);
        let service = service(home.path());
        assert!(service.install_admin(admin.clone()));
        assert!(!service.install_admin(admin.clone()), "installed once");
        let (web, mut peer, offices) = web_peer_for(
            service,
            r#"{"address":"127.0.0.1","port":48123,"scheme":"http_loopback","origin":"http://127.0.0.1:48123","hsts":"off"}"#,
        );
        exchange::<Hello>(&mut peer, 1, &Default::default())
            .unwrap()
            .into_result::<Hello>()
            .unwrap();
        let params = AdminLinkInitialParams {
            setup_operation: crate::admin::tests::OPERATION.into(),
        };
        assert!(refused(
            exchange::<AdminLinkInitial>(&mut peer, 2, &params),
            ErrorCode::NotAuthorized
        ));
        assert!(refused(
            exchange::<AdminReset>(&mut peer, 3, &Default::default()),
            ErrorCode::NotAuthorized
        ));
        // Nothing was issued: the first link for the registration subject is still available.
        assert!(crate::admin::tests::registry(&admin)
            .pending(
                LinkKind::Registration,
                &Subject::new("administrator").unwrap(),
                1
            )
            .unwrap()
            .is_none());
        drop(web);
        offices.shutdown();
    }

    /// Hello and pre-hello requests run inline (see the listener's
    /// `an_inline_call_runs_before_anything_more_is_read`).
    #[test]
    fn hello_and_pre_hello_requests_run_inline() {
        let dir = tempfile::tempdir().unwrap();
        let service = service(dir.path());
        let id = "01AAAAAAAAAAAAAAAAAAAAAAAA".to_owned();
        let inline = [
            Request::new::<Hello>(1, &HelloParams::default()).unwrap(),
            // The retired `office.app_token`: refused inline, like any call the host doesn't serve.
            Request {
                id: 1,
                method: "office.app_token".to_owned(),
                params: serde_json::json!({"office_id": id.clone()}),
                principal: None,
            },
            Request::new::<OfficeRestart>(1, &OfficeRestartParams { office_id: id }).unwrap(),
        ];
        let threaded = [
            Request::new::<InstanceStatus>(1, &Default::default()).unwrap(),
            Request::new::<OfficeList>(1, &Empty {}).unwrap(),
            // The retired name-only create: no typed call exists for it any more.
            Request {
                id: 1,
                method: "organization.create".to_owned(),
                params: serde_json::json!({"name": "Acme"}),
                principal: None,
            },
        ];
        let connection = Connection::for_test();
        for request in inline.iter().chain(&threaded) {
            assert!(service.inline(request, &connection), "{}", request.method);
        }
        connection.complete_hello();
        for request in inline.iter().chain(&threaded) {
            assert_eq!(
                service.inline(request, &connection),
                request.is::<Hello>(),
                "{}",
                request.method
            );
        }
        service.offices.shutdown();
    }
}
