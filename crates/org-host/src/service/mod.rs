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
    #[cfg(test)]
    pub(crate) fn recovery_for_test(&self) -> Arc<crate::recovery::Recovery> {
        Arc::clone(self.recovery.as_ref().expect("selected test host"))
    }
    pub(crate) fn gateway_ready(&self) -> bool {
        self.authentication.is_some()
            && self.offices.offices().iter().any(|office| {
                office.role == org_proto::OfficeRole::Hq
                    && office.status == org_proto::OfficeStatus::Active
            })
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
            connection.send_response(Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::Unavailable,
                    "protected account registry unavailable",
                ),
            ));
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
                    .map(|(id, socket)| (id, socket, p.office_id))
            });
        match result {
            Ok((id, socket, office)) => {
                let response = ok::<OfficeOpenChannel>(
                    request.id,
                    &org_host_control::ops::OfficeOpenChannelResult {
                        channel_id: id.clone(),
                    },
                );
                if connection
                    .send_descriptor(response, id, socket.into(), &office)
                    .is_err()
                {
                    connection.close();
                }
            }
            Err(error) => connection.send_response(Response::error(request.id, error)),
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
            )
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

#[cfg(test)]
mod tests {
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

    pub(super) fn service(home: &Path) -> Service {
        let supervisor = Supervisor::new(SupervisorConfig {
            program: home.join("no-such-server"),
            home: home.to_path_buf(),
            runtime_dir: None,
            logs_dir: home.join("logs"),
            log_max_bytes: 1024,
            timings: Timings::default(),
            keys: Arc::new(MemoryKeys::default()),
            server_check: Arc::new(crate::server_check::ServerCheck::Skip),
            sweep_orphans: false,
        });
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
