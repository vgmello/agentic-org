//! host.sock's private control boundary: registered-instance hello and count-only status.
//! Registered hosts additionally dispatch live account sign-in, renewal and logout.

pub(crate) mod authentication;
pub(crate) mod listener;
use authentication::Authentication;
mod request;

use std::sync::Arc;

use org_host_control::{
    GatewayState, GatewayStatus, Hello, HelloResult, InstanceMode, InstanceStatus,
    InstanceStatusResult, OfficeCounts,
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
    gateway: Arc<std::sync::Mutex<GatewayState>>,
    provisioning: Option<Arc<crate::provisioning::Provisioning>>,
    recovery: Option<Arc<crate::recovery::Recovery>>,
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
            gateway: Arc::new(std::sync::Mutex::new(GatewayState::NotConfigured)),
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
    pub(crate) fn with_gateway_state(mut self, state: Arc<std::sync::Mutex<GatewayState>>) -> Self {
        self.gateway = state;
        self
    }
    /// Whether the host may start its web process: host authentication is
    /// present and HQ is Active. Persisted listener configuration alone never qualifies.
    pub(crate) fn web_eligible(&self) -> bool {
        web_start_eligible(self.authentication.is_some(), self.hq_active())
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
            self.hq_active(),
            *crate::sync::lock(&self.gateway),
        )
    }
    fn hq_active(&self) -> bool {
        self.offices.offices().iter().any(|office| {
            office.role == org_proto::OfficeRole::Hq
                && office.status == org_proto::OfficeStatus::Active
        })
    }
    #[cfg(test)]
    pub(crate) fn recovery_for_test(&self) -> Arc<crate::recovery::Recovery> {
        Arc::clone(self.recovery.as_ref().expect("selected test host"))
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
            connection.send_response(answer::<org_host_control::ops::TlsFetch>(&request, |_| {
                let snapshot = connection.gateway_listen().ok_or_else(|| {
                    ApiError::new(
                        ErrorCode::NotAuthorized,
                        "private gateway connection required",
                    )
                })?;
                if snapshot.scheme() != org_host_control::listen::ListenMode::HttpLoopback {
                    return Err(ApiError::new(
                        ErrorCode::Unavailable,
                        "verified TLS pair unavailable",
                    ));
                }
                connection.note_listener_fetched();
                Ok(org_host_control::ops::TlsFetchResult::HttpLoopback {
                    origin: snapshot.origin().clone(),
                    hsts: snapshot.hsts(),
                })
            }));
            return;
        }
        if request.is::<org_host_control::provision::OrganizationProvision>() {
            connection.send_response(
                answer::<org_host_control::provision::OrganizationProvision>(&request, |params| {
                    let authentication = self.authentication.as_ref().ok_or_else(|| {
                        ApiError::new(ErrorCode::Unavailable, "protected registry unavailable")
                    })?;
                    let account = authentication.authorize(connection, &params.session)?;
                    let provisioning = self.provisioning.as_ref().ok_or_else(|| {
                        ApiError::new(ErrorCode::Unavailable, "selected provisioning unavailable")
                    })?;
                    provisioning.run(&account, &params, &self.offices)
                }),
            );
            return;
        }
        if self.recover(&request, connection, received) {
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
                connection.send_response(ok::<Hello>(request.id, &result));
            }
            OpenOperation::InstanceStatus => {
                self.broadcast.snapshot(|| {
                    connection.send_response(answer::<InstanceStatus>(&request, |_| {
                        let offices = self.offices.offices();
                        Ok(InstanceStatusResult {
                            instance_id: instance_id.to_owned(),
                            mode: InstanceMode::Service,
                            gateway: GatewayStatus {
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

/// The start-eligibility rule: host authentication is present and HQ is Active.
/// Persisted listener configuration alone never qualifies.
pub(crate) fn web_start_eligible(authenticated: bool, hq_active: bool) -> bool {
    authenticated && hq_active
}

/// The readiness rule: all three conditions, never persisted state alone.
pub(crate) fn web_serving(authenticated: bool, hq_active: bool, state: GatewayState) -> bool {
    web_start_eligible(authenticated, hq_active) && state == GatewayState::Running
}

#[cfg(test)]
pub(crate) mod tests {
    #[test]
    fn web_start_needs_authentication_and_an_active_hq() {
        use super::web_start_eligible;
        assert!(web_start_eligible(true, true));
        assert!(!web_start_eligible(false, true), "no authentication");
        assert!(!web_start_eligible(true, false), "no Active HQ");
        assert!(!web_start_eligible(false, false));
    }

    /// The service fixture, and the predicate the host wires into its web process
    /// supervisor, refuse to start the web process without authentication and without an
    /// Active HQ. An Active HQ needs a real office worker: the ignored real-process
    /// test covers the eligible case and the authenticated, not-yet-Active window.
    #[test]
    fn the_wired_web_eligibility_refuses_without_authentication_or_an_active_hq() {
        let home = tempfile::tempdir().unwrap();
        let bare = Arc::new(service(home.path()));
        assert!(!bare.web_eligible(), "no authentication");
        assert!(!(bare.web_eligibility())(), "wired: no authentication");
        let (_root, authentication, _connection) = crate::service::authentication::tests::fixture();
        let authenticated =
            Arc::new(service(home.path()).with_authentication(Some(Arc::new(authentication))));
        assert!(authenticated.offices.offices().is_empty());
        assert!(!authenticated.web_eligible(), "no Active HQ");
        assert!(!(authenticated.web_eligibility())(), "wired: no Active HQ");
        bare.offices.shutdown();
        authenticated.offices.shutdown();
    }

    #[test]
    fn web_ready_needs_authentication_active_hq_and_a_serving_web() {
        use super::web_serving;
        assert!(web_serving(true, true, GatewayState::Running));
        for state in [
            GatewayState::NotConfigured,
            GatewayState::Starting,
            GatewayState::Failed,
            GatewayState::Unknown,
        ] {
            assert!(!web_serving(true, true, state), "{state:?}");
        }
        assert!(!web_serving(false, true, GatewayState::Running));
        assert!(!web_serving(true, false, GatewayState::Running));
    }

    use super::*;
    use std::path::Path;

    use org_proto::{
        Empty, HelloParams, OfficeAppToken, OfficeAppTokenParams, OfficeList, OfficeRestart,
        OfficeRestartParams, OrganizationCreate, OrganizationCreateParams,
    };

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
        service.instance_id = Some(crate::ids::generate().unwrap());
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
        let mut service = service(home);
        service.instance_id = Some(crate::ids::generate().unwrap());
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

    #[test]
    fn tls_fetch_off_an_http_loopback_listener_is_unavailable_and_never_enables_serving() {
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
            ErrorCode::Unavailable
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
        service.instance_id = Some(crate::ids::generate().unwrap());
        let offices = service.offices.clone();
        let (host, peer) = std::os::unix::net::UnixStream::pair().unwrap();
        // A setup connection and a host.sock connection are both `Local`.
        let local = listener::SetupConnection::start(host, Arc::new(service)).unwrap();
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

    /// Hello and pre-hello requests run inline (see the listener's
    /// `an_inline_call_runs_before_anything_more_is_read`).
    #[test]
    fn hello_and_pre_hello_requests_run_inline() {
        let dir = tempfile::tempdir().unwrap();
        let service = service(dir.path());
        let id = "01AAAAAAAAAAAAAAAAAAAAAAAA".to_owned();
        let inline = [
            Request::new::<Hello>(1, &HelloParams::default()).unwrap(),
            Request::new::<OfficeAppToken>(
                1,
                &OfficeAppTokenParams {
                    office_id: id.clone(),
                },
            )
            .unwrap(),
            Request::new::<OfficeRestart>(1, &OfficeRestartParams { office_id: id }).unwrap(),
        ];
        let threaded = [
            Request::new::<InstanceStatus>(1, &Default::default()).unwrap(),
            Request::new::<OfficeList>(1, &Empty {}).unwrap(),
            Request::new::<OrganizationCreate>(
                1,
                &OrganizationCreateParams {
                    name: "Acme".into(),
                },
            )
            .unwrap(),
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
