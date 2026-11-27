//! Test fixtures and shared engine-independent storage conformance runners.
#[cfg(all(feature = "test-support", debug_assertions))]
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc, Arc, Mutex,
};
#[cfg(all(feature = "test-support", debug_assertions))]
use std::time::Duration;

#[cfg(all(feature = "test-support", debug_assertions))]
use org_proto::{
    ApiError, Call, Empty, Event, ListOfficesResult, ListOrganizationsResult, OfficeList,
    OrganizationList, Request, Response, ServerMessage, Subscribe,
};
#[cfg(all(feature = "test-support", debug_assertions))]
use serde_json::Value;

#[cfg(all(feature = "test-support", debug_assertions))]
use crate::{broadcast::Subscriber, sync::lock, Host};

pub mod conformance;
#[cfg(all(feature = "test-support", debug_assertions))]
pub mod process;

/// Native lifecycle operations and observation for this crate's integration tests only.
/// No listener or socket exposes these operations.
#[cfg(all(feature = "test-support", debug_assertions))]
pub struct Fixture {
    offices: Arc<crate::offices::Offices>,
    broadcast: Arc<crate::broadcast::Broadcast>,
    tx: mpsc::Sender<ServerMessage>,
    rx: Mutex<mpsc::Receiver<ServerMessage>>,
    seq: AtomicU64,
}
#[cfg(all(feature = "test-support", debug_assertions))]
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
        if request.is::<OrganizationList>() {
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
#[cfg(all(feature = "test-support", debug_assertions))]
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
