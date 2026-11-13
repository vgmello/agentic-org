//! Temporary in-process lifecycle fixtures. Delete when 2b.3 can seed through sessions.
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc, Arc, Mutex,
};
use std::time::Duration;

use org_proto::{
    ApiError, Call, Empty, ErrorCode, Event, ListOfficesResult, ListOrganizationsResult,
    OfficeList, OrganizationCreate, OrganizationList, Request, Response, ServerMessage, Subscribe,
};
use serde_json::Value;

use crate::{broadcast::Subscriber, sync::lock, Host};

/// Native lifecycle operations and observation for this crate's integration tests only.
/// No listener or socket exposes these operations.
pub struct Fixture {
    offices: Arc<crate::offices::Offices>,
    broadcast: Arc<crate::broadcast::Broadcast>,
    tx: mpsc::Sender<ServerMessage>,
    rx: Mutex<mpsc::Receiver<ServerMessage>>,
    seq: AtomicU64,
}
impl Fixture {
    pub fn new(host: &Host) -> Arc<Self> {
        let (tx, rx) = mpsc::channel();
        Arc::new(Self {
            offices: Arc::clone(&host.offices),
            broadcast: host.offices.broadcast_for_test(),
            tx,
            rx: Mutex::new(rx),
            seq: AtomicU64::new(0),
        })
    }
    pub fn submit(self: &Arc<Self>, request: Request) {
        if request.is::<OrganizationCreate>() {
            match request.params::<OrganizationCreate>() {
                Ok(params) => {
                    if let Err(error) = self.offices.create_organization(&params.name, |org| {
                        self.answer::<OrganizationCreate>(request.id, Ok(org.clone()))
                    }) {
                        self.answer::<OrganizationCreate>(request.id, Err(error));
                    }
                }
                Err(_) => self.invalid(request.id),
            }
        } else if request.is::<OrganizationList>() {
            self.broadcast.snapshot(|| {
                self.answer::<OrganizationList>(
                    request.id,
                    Ok(ListOrganizationsResult {
                        organizations: self.offices.organizations(),
                    }),
                )
            });
        } else if request.is::<OfficeList>() {
            self.broadcast.snapshot(|| {
                self.answer::<OfficeList>(
                    request.id,
                    Ok(ListOfficesResult {
                        offices: self.offices.offices(),
                    }),
                )
            });
        } else if request.is::<Subscribe>() {
            let subscriber: Arc<dyn Subscriber> = self.clone();
            self.broadcast.subscribe(Arc::downgrade(&subscriber), || {
                self.answer::<Subscribe>(request.id, Ok(Empty {}));
                true
            });
        } else {
            panic!("this fixture has no operation {}", request.method);
        }
    }
    fn answer<C: Call>(&self, id: u64, result: Result<C::Result, ApiError>) {
        let response = match result {
            Ok(result) => Response::ok::<C>(id, &result).unwrap(),
            Err(error) => Response::error(id, error),
        };
        self.tx.send(ServerMessage::Response(response)).unwrap();
    }
    fn invalid(&self, id: u64) {
        self.tx
            .send(ServerMessage::Response(Response::error(
                id,
                ApiError::new(ErrorCode::InvalidRequest, "invalid fixture params"),
            )))
            .unwrap();
    }
    pub fn recv(&self, timeout: Duration) -> Option<ServerMessage> {
        lock(&self.rx).recv_timeout(timeout).ok()
    }
    pub fn offices(&self) -> Vec<org_proto::OfficeInfo> {
        self.offices.offices()
    }
    pub fn restart(&self, id: &str) -> Result<(), ApiError> {
        self.offices.restart_office(id)
    }
}
impl Subscriber for Fixture {
    fn send_event(&self, name: &str, data: Value) {
        let event = Event {
            seq: self.seq.fetch_add(1, Ordering::SeqCst) + 1,
            event: name.to_owned(),
            data,
        };
        self.tx.send(ServerMessage::Event(event)).unwrap();
    }
}
