//! The host's `org-proto` service: `hello`, `organization.create`, `organization.list`,
//! `office.list` and `subscribe` (which receives `organization.created`).

pub(crate) mod listener;

use std::sync::{Arc, Mutex, Weak};

use org_proto::{
    ApiError, Call, Empty, ErrorCode, EventKind, Hello, HelloResult, ListOfficesResult,
    ListOrganizationsResult, OfficeList, OrganizationCreate, OrganizationCreated, OrganizationList,
    Request, Response, Subscribe, API_VERSION,
};

use crate::offices::{Offices, THIS_MAC};
use crate::sync::lock;
use listener::{Connection, Handler};

pub(crate) struct Service {
    offices: Arc<Offices>,
    subscribers: Mutex<Vec<Weak<Connection>>>,
}

impl Service {
    pub(crate) fn new(offices: Arc<Offices>) -> Self {
        Self {
            offices,
            subscribers: Mutex::new(Vec::new()),
        }
    }

    /// Pushes event `E` to every subscribed connection. The subscriber list is only locked
    /// to copy it; queueing never blocks (a client that doesn't read is disconnected).
    fn publish<E: EventKind>(&self, data: &E::Data) {
        let Ok(data) = serde_json::to_value(data) else {
            return;
        };
        let live: Vec<Arc<Connection>> = {
            let mut subscribers = lock(&self.subscribers);
            subscribers.retain(|s| s.strong_count() > 0);
            subscribers.iter().filter_map(Weak::upgrade).collect()
        };
        for connection in live {
            connection.send_event(E::NAME, data.clone());
        }
    }

    fn subscribe(&self, request: &Request, connection: &Arc<Connection>) {
        // The response goes out before the connection is added, so no event can overtake it.
        let response = answer::<Subscribe>(request, |_from_position_is_ignored| Ok(Empty {}));
        let ok = matches!(response.outcome, org_proto::Outcome::Result(_));
        connection.send_response(response);
        if ok {
            // One subscription per connection: subscribing again changes nothing.
            let this = Arc::downgrade(connection);
            let mut subscribers = lock(&self.subscribers);
            if !subscribers.iter().any(|s| Weak::ptr_eq(s, &this)) {
                subscribers.push(this);
            }
        }
    }

    fn create(&self, request: &Request, connection: &Arc<Connection>) {
        let mut created = None;
        let response = answer::<OrganizationCreate>(request, |params| {
            let organization = self.offices.create_organization(&params.name)?;
            created = Some(organization.clone());
            Ok(organization)
        });
        connection.send_response(response);
        if let Some(organization) = created {
            self.publish::<OrganizationCreated>(&organization);
        }
    }
}

impl Handler for Service {
    fn handle(&self, request: Request, connection: &Arc<Connection>) {
        if request.is::<Hello>() {
            connection.send_response(answer::<Hello>(&request, |_| Ok(hello())));
        } else if request.is::<OrganizationCreate>() {
            self.create(&request, connection);
        } else if request.is::<OrganizationList>() {
            connection.send_response(answer::<OrganizationList>(&request, |_| {
                Ok(ListOrganizationsResult {
                    organizations: self.offices.organizations(),
                })
            }));
        } else if request.is::<OfficeList>() {
            connection.send_response(answer::<OfficeList>(&request, |_| {
                Ok(ListOfficesResult {
                    offices: self.offices.offices(),
                })
            }));
        } else if request.is::<Subscribe>() {
            self.subscribe(&request, connection);
        } else {
            connection.send_response(Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::UnknownMethod,
                    "this host doesn't know that method",
                ),
            ));
        }
    }
}

/// `hello` from this Mac's host: no office.
fn hello() -> HelloResult {
    HelloResult {
        api_version: API_VERSION,
        server_version: env!("CARGO_PKG_VERSION").to_owned(),
        host: THIS_MAC.to_owned(),
        office: None,
    }
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
        Ok(result) => Response::ok::<C>(request.id, &result).unwrap_or_else(|_| {
            Response::error(
                request.id,
                ApiError::new(ErrorCode::StorageError, "the answer couldn't be encoded"),
            )
        }),
        Err(error) => Response::error(request.id, error),
    }
}
