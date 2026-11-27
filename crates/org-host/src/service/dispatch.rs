//! Routing each `host.sock` request to its handler (`Handler for Service`).

use std::sync::Arc;

use org_host_control::{
    Hello, HelloResult, InstanceMode, InstanceStatus, InstanceStatusResult, OfficeCounts, WebStatus,
};
use org_proto::{ApiError, Call, ErrorCode, Request, Response};

use crate::offices::THIS_MAC;
use crate::peer::Received;

use super::browser::web_reports_serving;
use super::organizations;
use super::reply::{answer, not_authorized, ok, params_dont_fit};
use super::Service;
use crate::control_listener::{Connection, Handler};

impl Handler for Service {
    fn disconnected(&self, connection: &Arc<Connection>) {
        connection.clear_recovery_permission();
        self.cli_approvals.disconnect(connection);
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
        if let Some(reply) = organizations::dispatch_with_listen(
            &request,
            connection.web_listen().map(|c| c.scheme()),
            self.authentication.as_deref(),
            self.provisioning.as_ref(),
            &mut |name, data| connection.send_event(name, data),
        ) {
            #[cfg(any(test, all(feature = "test-support", debug_assertions)))]
            if connection.web_listen().is_some() {
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
                    connection.web_listen().is_some(),
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

#[derive(Debug, PartialEq, Eq)]
pub(super) enum OpenOperation {
    Hello,
    InstanceStatus,
}

impl OpenOperation {
    pub(super) fn from_method(method: &str) -> Option<Self> {
        match method {
            Hello::METHOD => Some(Self::Hello),
            InstanceStatus::METHOD => Some(Self::InstanceStatus),
            _ => None,
        }
    }
}

/// Host control names its immutable registered instance and advertises no retired calls.
fn hello(instance_id: &str) -> HelloResult {
    HelloResult::host(env!("CARGO_PKG_VERSION"), THIS_MAC).with_instance_id(instance_id)
}
