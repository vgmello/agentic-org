//! The host's `org-proto` service: `hello`, `organization.create`, `organization.list`,
//! `office.list`, `subscribe` (which receives `organization.created` and
//! `office.status_changed`), and the app-only `office.app_token` and `office.restart`.
//!
//! Events and list answers go through the [`Broadcast`], so an event is never older than a list
//! answer written before it on the same connection.

pub(crate) mod listener;

use std::sync::{Arc, Weak};

use org_proto::{
    capability, ApiError, AppToken, Call, Empty, ErrorCode, Hello, HelloResult, ListOfficesResult,
    ListOrganizationsResult, OfficeAppToken, OfficeList, OfficeRestart, Organization,
    OrganizationCreate, OrganizationList, Request, Response, Subscribe, API_VERSION,
};
use serde::Serialize;

use crate::app_auth::AppPolicy;
use crate::broadcast::{Broadcast, Subscriber};
use crate::offices::{Offices, THIS_MAC};
use crate::peer::Received;
use listener::{Connection, Handler, SecretLine};

pub(crate) struct Service {
    offices: Arc<Offices>,
    broadcast: Arc<Broadcast>,
    /// Who may use the app-only calls.
    app: AppPolicy,
}

impl Service {
    pub(crate) fn new(offices: Arc<Offices>, broadcast: Arc<Broadcast>, app: AppPolicy) -> Self {
        Self {
            offices,
            broadcast,
            app,
        }
    }

    fn subscribe(&self, request: &Request, connection: &Arc<Connection>) {
        let subscriber: Weak<dyn Subscriber> = Arc::downgrade(connection) as Weak<Connection>;
        self.broadcast.subscribe(subscriber, || {
            let response = answer::<Subscribe>(request, |_from_position_is_ignored| Ok(Empty {}));
            let ok = matches!(response.outcome, org_proto::Outcome::Result(_));
            connection.send_response(response);
            ok
        });
    }

    /// `organization.create`. On success the answer is queued by `Offices`, in the same
    /// critical section that adds the organization and publishes `organization.created`.
    fn create(&self, request: &Request, connection: &Arc<Connection>) {
        let Ok(params) = request.params::<OrganizationCreate>() else {
            connection.send_response(params_dont_fit(request.id));
            return;
        };
        let respond = |organization: &Organization| {
            connection.send_response(ok::<OrganizationCreate>(request.id, organization));
        };
        if let Err(error) = self.offices.create_organization(&params.name, respond) {
            connection.send_response(Response::error(request.id, error));
        }
    }

    /// Whether the caller on `connection` is the desktop app.
    /// `received`: where the request's line was in the connection's stream.
    fn is_app(&self, connection: &Connection, received: Received) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.app.allows(&crate::app_auth::SocketPeer {
                stream: connection.socket(),
                accepted: connection.accepted(),
                received,
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (connection, received);
            false
        }
    }

    /// `office.app_token`: the caller's signature first, then the office's token, written
    /// straight into the response line (never a `serde_json::Value` or a `String`).
    fn app_token(&self, request: &Request, connection: &Arc<Connection>, received: Received) {
        if !self.is_app(connection, received) {
            connection.send_response(not_authorized(request.id));
            return;
        }
        let params = match request.params::<OfficeAppToken>() {
            Ok(params) => params,
            Err(_) => {
                connection.send_response(params_dont_fit(request.id));
                return;
            }
        };
        let id = request.id;
        match self
            .offices
            .app_token(&params.office_id, |token| token_line(id, token))
        {
            Ok(Some(line)) => connection.send_secret_line(line),
            Ok(None) => connection.send_response(Response::error(
                id,
                ApiError::new(ErrorCode::StorageError, "the answer couldn't be encoded"),
            )),
            Err(error) => connection.send_response(Response::error(id, error)),
        }
    }

    /// `office.restart`: the caller's signature first, then the restart.
    fn restart(&self, request: &Request, connection: &Arc<Connection>, received: Received) {
        if !self.is_app(connection, received) {
            connection.send_response(not_authorized(request.id));
            return;
        }
        connection.send_response(answer::<OfficeRestart>(request, |params| {
            self.offices.restart_office(&params.office_id)?;
            Ok(Empty {})
        }));
    }
}

impl Handler for Service {
    /// The app-only calls are handled on the reader thread: their check needs to know that
    /// nothing arrived after the request (see `app_auth`).
    fn inline(&self, request: &Request) -> bool {
        request.is::<OfficeAppToken>() || request.is::<OfficeRestart>()
    }

    fn handle(&self, request: Request, connection: &Arc<Connection>, received: Received) {
        if request.is::<Hello>() {
            connection.send_response(answer::<Hello>(&request, |_| Ok(hello())));
        } else if request.is::<OrganizationCreate>() {
            self.create(&request, connection);
        } else if request.is::<OrganizationList>() {
            self.broadcast.snapshot(|| {
                connection.send_response(answer::<OrganizationList>(&request, |_| {
                    Ok(ListOrganizationsResult {
                        organizations: self.offices.organizations(),
                    })
                }));
            });
        } else if request.is::<OfficeList>() {
            self.broadcast.snapshot(|| {
                connection.send_response(answer::<OfficeList>(&request, |_| {
                    Ok(ListOfficesResult {
                        offices: self.offices.offices(),
                    })
                }));
            });
        } else if request.is::<Subscribe>() {
            self.subscribe(&request, connection);
        } else if request.is::<OfficeAppToken>() {
            self.app_token(&request, connection, received);
        } else if request.is::<OfficeRestart>() {
            self.restart(&request, connection, received);
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

/// The `office.app_token` response line for request `id`, encoded straight from the token into
/// a buffer big enough never to reallocate (no copy left behind there). `None` if it couldn't be
/// encoded.
fn token_line(id: u64, token: &AppToken) -> Option<SecretLine> {
    #[derive(Serialize)]
    struct Line<'a> {
        id: u64,
        result: TokenResult<'a>,
    }
    #[derive(Serialize)]
    struct TokenResult<'a> {
        app_token: &'a AppToken,
    }
    let mut bytes = Vec::with_capacity(256);
    let encoded = serde_json::to_writer(
        &mut bytes,
        &Line {
            id,
            result: TokenResult { app_token: token },
        },
    );
    bytes.push(b'\n');
    // Wrapped at once, so the bytes are zeroed even when encoding failed.
    let line = SecretLine::new(bytes);
    encoded.ok().map(|()| line)
}

fn not_authorized(id: u64) -> Response {
    Response::error(
        id,
        ApiError::new(
            ErrorCode::NotAuthorized,
            "only the desktop app may use this call",
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

/// `hello` from this Mac's host: no office, and the optional calls and events it implements.
fn hello() -> HelloResult {
    HelloResult {
        api_version: API_VERSION,
        server_version: env!("CARGO_PKG_VERSION").to_owned(),
        host: THIS_MAC.to_owned(),
        office: None,
        capabilities: [
            capability::OFFICE_APP_TOKEN,
            capability::OFFICE_RESTART,
            // The event, and `status_reason` / `next_restart_at` on it and in `office.list`.
            capability::OFFICE_STATUS_CHANGED,
        ]
        .map(str::to_owned)
        .to_vec(),
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
