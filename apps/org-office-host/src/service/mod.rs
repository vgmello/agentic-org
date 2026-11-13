//! host.sock's private control boundary: registered-instance hello and count-only status.
//! Only the two open operations are dispatched; session capabilities arrive in 2b.3.

pub(crate) mod listener;
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
        }
    }
}

impl Handler for Service {
    /// Decide handshake refusals before a later pipelined request can change the state.
    fn inline(&self, request: &Request, connection: &Connection) -> bool {
        !connection.hello_completed() || request.is::<Hello>()
    }

    fn handle(&self, request: Request, connection: &Arc<Connection>, _received: Received) {
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
                                state: GatewayState::NotConfigured,
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
fn ok<C: Call>(id: u64, result: &C::Result) -> Response {
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

    fn service(home: &Path) -> Service {
        let supervisor = Supervisor::new(SupervisorConfig {
            program: home.join("no-such-server"),
            home: home.to_path_buf(),
            runtime_dir: None,
            instance_id: "01JINSTANCE000000000000000".into(),
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
