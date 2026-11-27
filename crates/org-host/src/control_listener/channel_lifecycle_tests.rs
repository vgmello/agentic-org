use std::io::{Read, Write};
use std::os::unix::net::{UnixDatagram, UnixStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use org_host_control::{ops, Password};
use org_proto::{frame, ErrorCode, Outcome, Request, Response};
use serde_json::Value;

use crate::cloexec::SPAWN_LOCK;
use crate::peer::{Accepted, Received};
use crate::supervisor::channels::{
    tests::{await_closed, worker_fixture},
    ChannelSlot,
};
use crate::sync::lock;

use super::connection::{Connection, DescriptorError, Gateway};
use super::outbound::write_loop;
use super::serve::serve;
use super::*;

fn response(id: u64) -> Response {
    Response {
        id,
        outcome: Outcome::Result(serde_json::json!({})),
        waiting_messages: None,
    }
}
fn queued_descriptor(
    connection: &Connection,
    id: &str,
    witness: ChannelSlot,
) -> Result<(), DescriptorError> {
    let (descriptor, _peer) = UnixStream::pair().unwrap();
    connection.send_descriptor(response(1), id.into(), descriptor.into(), "office", witness)
}
#[test]
fn concurrent_pending_reservations_count_with_live_acknowledged_records_and_capacity_is_definite() {
    let (host, _peer) = UnixStream::pair().unwrap();
    let (connection, queue) = Connection::new(host.try_clone().unwrap(), Accepted::sample(&host));
    for n in 0..496 {
        lock(&connection.received_channels).insert(format!("ack-{n}"), ChannelSlot::for_test());
    }
    let barrier = Arc::new(std::sync::Barrier::new(33));
    let threads: Vec<_> = (0..32)
        .map(|n| {
            let connection = Arc::clone(&connection);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                let id = format!("pending-{n}");
                let result = queued_descriptor(&connection, &id, ChannelSlot::for_test());
                (id, result)
            })
        })
        .collect();
    barrier.wait();
    let mut admitted = Vec::new();
    for thread in threads {
        let (id, result) = thread.join().unwrap();
        match result {
            Ok(()) => admitted.push(id),
            Err(error) => assert_eq!(error, DescriptorError::Capacity),
        }
    }
    assert_eq!(admitted.len(), 16);
    assert_eq!(lock(&connection.descriptor_receipts).len(), 16);
    assert_eq!(lock(&connection.received_channels).len(), 496);
    assert!(!connection.closed.load(Ordering::SeqCst));
    let closed = lock(&connection.descriptor_receipts)[&admitted[0]]
        .1
        .clone();
    closed.close_for_test();
    assert_eq!(
        queued_descriptor(&connection, "cannot-prune-pending", ChannelSlot::for_test()),
        Err(DescriptorError::Capacity)
    );
    assert_eq!(
        queued_descriptor(&connection, &admitted[0], ChannelSlot::for_test()),
        Err(DescriptorError::Transport)
    );
    assert!(
        !Connection::for_test().descriptor_received(&admitted[0]),
        "foreign connection ACK"
    );
    // A late, exact ACK still resolves ownership even after physical EOF.
    assert!(connection.descriptor_received(&admitted[0]));
    assert!(!connection.channel_received(&admitted[0]));
    assert!(!connection.descriptor_received(&admitted[0]));
    assert!(queued_descriptor(&connection, "replacement", ChannelSlot::for_test()).is_ok());
    for id in &admitted[1..] {
        assert!(connection.descriptor_received(id));
    }
    assert_eq!(lock(&connection.received_channels).len(), 511);
    assert_eq!(lock(&connection.descriptor_receipts).len(), 1);
    assert_eq!(
        queued_descriptor(&connection, "combined-513", ChannelSlot::for_test()),
        Err(DescriptorError::Capacity)
    );
    // Losing the actual queued ownership waiter cannot turn a late ACK into authority.
    drop(queue);
    assert!(!connection.descriptor_received("replacement"));
    assert!(!connection.channel_received("replacement"));
}

#[test]
fn missing_native_ownership_ack_times_out_and_late_exact_ack_cannot_grant() {
    let (host, peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(8))).unwrap();
    let (connection, queue) = Connection::new(host.try_clone().unwrap(), Accepted::sample(&host));
    let closed = Arc::clone(&connection.closed);
    let writer = thread::spawn(move || write_loop(&host, queue, &closed));
    let witness = ChannelSlot::for_test();
    let (descriptor, _worker) = UnixStream::pair().unwrap();
    connection
        .send_descriptor(
            response(1),
            "no-ownership-ack".into(),
            descriptor.into(),
            "office",
            witness,
        )
        .unwrap();
    let mut wire = Wire {
        writer: peer.try_clone().unwrap(),
        reader: org_socket::rights::StreamReceiver::new(peer),
        gateway: None,
        id: 1,
    };
    assert_eq!(wire.response().id, 1);
    let descriptor = wire.descriptor("no-ownership-ack");
    let until = Instant::now() + Duration::from_secs(8);
    while !connection.closed.load(Ordering::SeqCst) {
        assert!(
            Instant::now() < until,
            "writer did not enforce ownership deadline"
        );
        thread::sleep(Duration::from_millis(10));
    }
    writer.join().unwrap();
    assert!(!connection.descriptor_received("no-ownership-ack"));
    assert!(!connection.channel_received("no-ownership-ack"));
    drop(descriptor);
}

struct LiveHandler {
    auth: Arc<crate::authentication::Authentication>,
    endpoint: Arc<crate::supervisor::channels::Endpoint>,
}
impl Handler for LiveHandler {
    fn inline(&self, _: &Request, _: &Connection) -> bool {
        true
    }
    fn disconnected(&self, connection: &Arc<Connection>) {
        self.auth.disconnect(connection);
    }
    fn handle(&self, request: Request, connection: &Arc<Connection>, _: Received) {
        if request.is::<ops::FdsReceived>() {
            crate::service::acknowledge_descriptors(&request, connection);
        } else if request.is::<ops::OfficeOpenChannel>() {
            let p = request.params::<ops::OfficeOpenChannel>().unwrap();
            match self
                .auth
                .open_channel(connection, &p.session, Arc::clone(&self.endpoint), "org")
            {
                Ok((id, socket, witness)) => crate::service::deliver_channel(
                    connection, request.id, id, socket, "office", witness,
                ),
                Err(error) => connection.send_response(Response::error(request.id, error)),
            }
        } else if request.is::<ops::SessionGrant>() {
            let p = request.params::<ops::SessionGrant>().unwrap();
            match self
                .auth
                .grant(connection, &p.session, &p.office_id, &p.channel_id)
            {
                Ok(result) => crate::service::secret_response(connection, request.id, &result),
                Err(error) => connection.send_response(Response::error(request.id, error)),
            }
        } else {
            let p = request.params::<ops::SessionRenew>().unwrap();
            let response = match self.auth.authorize(connection, &p.session) {
                Ok(_) => response(request.id),
                Err(error) => Response::error(request.id, error),
            };
            connection.send_response(response);
        }
    }
}
struct Wire {
    writer: UnixStream,
    reader: org_socket::rights::StreamReceiver,
    gateway: Option<org_socket::rights::DatagramReceiver>,
    id: u64,
}
impl Wire {
    fn request(&mut self, method: &str, params: Value) {
        self.id += 1;
        frame::write_message(
            &mut self.writer,
            &serde_json::json!({"id":self.id,"method":method,"params":params}),
        )
        .unwrap();
    }
    fn byte(&mut self, count: usize) -> (u8, Vec<std::os::fd::OwnedFd>) {
        let until = Instant::now() + Duration::from_secs(3);
        loop {
            match self.reader.receive_byte(count, &SPAWN_LOCK) {
                Ok(result) => return result,
                Err(org_socket::rights::ReceiveError::WouldBlock) if Instant::now() < until => {
                    thread::yield_now()
                }
                Err(error) => panic!("{error}"),
            }
        }
    }
    fn response(&mut self) -> Response {
        let mut line = Vec::new();
        loop {
            let (b, fds) = self.byte(0);
            assert!(fds.is_empty());
            if b == b'\n' {
                break;
            }
            line.push(b);
        }
        let response: Response = serde_json::from_slice(&line).unwrap();
        assert_eq!(response.id, self.id);
        response
    }
    fn descriptor(&mut self, id: &str) -> UnixStream {
        let mut fds = if let Some(gateway) = &mut self.gateway {
            let mut buffer = [0; org_office_launch::TRANSFER_HEADER_MAX_BYTES];
            let until = Instant::now() + Duration::from_secs(3);
            let received = loop {
                match gateway.receive(&mut buffer, 1, &SPAWN_LOCK) {
                    Ok(received) => break received,
                    Err(org_socket::rights::ReceiveError::WouldBlock) if Instant::now() < until => {
                        thread::yield_now()
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            assert!(
                matches!(org_office_launch::decode_transfer_record(&buffer[..received.bytes]).unwrap(),
                org_office_launch::TransferRecord::Channel { kind: org_office_launch::ChannelKind::Web, office_id, channel_id } if office_id == "office" && channel_id == id)
            );
            received.descriptors
        } else {
            let (marker, fds) = self.byte(1);
            assert_eq!(marker, ops::HOST_SOCK_MARKER);
            fds
        };
        assert_eq!(fds.len(), 1);
        let stream = org_socket::rights::connected_unix_stream(fds.pop().unwrap()).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream
    }
}
struct Control {
    _root: tempfile::TempDir,
    worker: crate::supervisor::channels::tests::WorkerFixture,
    connection: Arc<Connection>,
    wire: Wire,
    session: org_host_control::SessionHandle,
    thread: Option<JoinHandle<()>>,
}
impl Control {
    fn new(gateway: bool) -> Self {
        let worker = worker_fixture("office");
        let (_root, auth, _) = crate::authentication::tests::fixture();
        let auth = Arc::new(auth);
        let (host, peer) = UnixStream::pair().unwrap();
        let (mut connection, queue) =
            Connection::new(host.try_clone().unwrap(), Accepted::sample(&host));
        let datagrams = if gateway {
            let (transfer, peer) = UnixDatagram::pair().unwrap();
            Arc::get_mut(&mut connection).unwrap().gateway = Some(Gateway {
            transfer: Arc::new(transfer),
            listen: serde_json::from_str(r#"{"address":"127.0.0.1","port":48123,"scheme":"http_loopback","origin":"http://127.0.0.1:48123","hsts":"off"}"#).unwrap(),
            fetched: AtomicBool::new(false),
            serving: AtomicBool::new(false),
            serving_refused: AtomicBool::new(false),
        });
            Some(org_socket::rights::DatagramReceiver::new(peer))
        } else {
            None
        };
        let signed_in = auth
            .verify(
                &connection,
                "OWNER",
                &Password::new("synthetic test password".into()).unwrap(),
            )
            .unwrap();
        crate::authentication::tests::link_owner(&auth, &connection, &signed_in.session);
        let serving = Arc::clone(&connection);
        let handler = Arc::new(LiveHandler {
            auth: Arc::clone(&auth),
            endpoint: Arc::clone(&worker.endpoint),
        });
        let thread = thread::spawn(move || serve(host, serving, queue, handler, 4));
        let wire = Wire {
            writer: peer.try_clone().unwrap(),
            reader: org_socket::rights::StreamReceiver::new(peer),
            gateway: datagrams,
            id: 0,
        };
        Self {
            _root,
            worker,
            connection,
            wire,
            session: signed_in.session,
            thread: Some(thread),
        }
    }
    fn closed_cycle(&mut self, cycle: usize) -> String {
        self.wire.request(
            "office.open_channel",
            serde_json::json!({"session":self.session,"office_id":"office"}),
        );
        let result = self
            .wire
            .response()
            .into_result::<ops::OfficeOpenChannel>()
            .unwrap();
        let mut channel = self.wire.descriptor(&result.channel_id);
        let witness = lock(&self.connection.descriptor_receipts)[&result.channel_id]
            .1
            .clone();
        assert!(!self.connection.channel_received(&result.channel_id));
        if cycle.is_multiple_of(2) {
            // A worker-side protocol close is observable before the listener drops its
            // retained sender FD on client ACK. Merely dropping our copy cannot be.
            channel.write_all(b"not-json\n").unwrap();
            drop(channel);
            await_closed(&witness); // Late client ACK after authenticated worker close.
        } else {
            channel.write_all(b"actual transferred RPC").unwrap();
            let mut bytes = [0; 22];
            channel.read_exact(&mut bytes).unwrap();
            assert_eq!(&bytes, b"actual transferred RPC");
            self.wire
                .request("fds.received", serde_json::json!({"of":result.channel_id}));
            assert!(self
                .wire
                .response()
                .into_result::<ops::FdsReceived>()
                .is_ok());
            self.wire.request("session.grant", serde_json::json!({"session":self.session,"office_id":"office","channel_id":result.channel_id}));
            assert!(self
                .wire
                .response()
                .into_result::<ops::SessionGrant>()
                .is_ok());
            drop(channel);
            await_closed(&witness);
        }
        if cycle.is_multiple_of(2) {
            self.wire
                .request("fds.received", serde_json::json!({"of":result.channel_id}));
            assert!(self
                .wire
                .response()
                .into_result::<ops::FdsReceived>()
                .is_ok());
        }
        assert!(!self.connection.channel_received(&result.channel_id));
        self.wire.request("session.grant", serde_json::json!({"session":self.session,"office_id":"office","channel_id":result.channel_id}));
        assert!(self
            .wire
            .response()
            .into_result::<ops::SessionGrant>()
            .is_err());
        assert!(lock(&self.connection.received_channels).len() <= 1);
        result.channel_id
    }
    fn capacity_preserves_channel(&mut self) {
        // The same control connection survives genuine listener capacity refusal.
        lock(&self.connection.received_channels).clear();
        self.wire.request(
            "office.open_channel",
            serde_json::json!({"session":self.session,"office_id":"office"}),
        );
        let held = self
            .wire
            .response()
            .into_result::<ops::OfficeOpenChannel>()
            .unwrap();
        let mut held_channel = self.wire.descriptor(&held.channel_id);
        self.wire
            .request("fds.received", serde_json::json!({"of":held.channel_id}));
        assert!(self
            .wire
            .response()
            .into_result::<ops::FdsReceived>()
            .is_ok());
        for n in 0..511 {
            lock(&self.connection.received_channels)
                .insert(format!("live-budget-{n}"), ChannelSlot::for_test());
        }
        self.wire.request(
            "office.open_channel",
            serde_json::json!({"session":self.session,"office_id":"office"}),
        );
        let error = self
            .wire
            .response()
            .into_result::<ops::OfficeOpenChannel>()
            .unwrap_err();
        assert!(
            matches!(error, org_proto::CallError::Api(error) if error.code == ErrorCode::Unavailable)
        );
        self.wire
            .request("session.renew", serde_json::json!({"session":self.session}));
        assert!(self
            .wire
            .response()
            .into_result::<ops::SessionRenew>()
            .is_ok());
        assert!(!self.connection.closed.load(Ordering::SeqCst));
        held_channel.write_all(b"still live").unwrap();
        let mut bytes = [0; 10];
        held_channel.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"still live");
        self.wire.request("session.grant", serde_json::json!({"session":self.session,"office_id":"office","channel_id":held.channel_id}));
        assert!(self
            .wire
            .response()
            .into_result::<ops::SessionGrant>()
            .is_ok());
    }
    fn bad_receipt(&mut self, receipt: &str) {
        // Duplicates and foreign receipts still terminate the actual control reader.
        self.wire
            .request("fds.received", serde_json::json!({"of":receipt}));
        assert!(self
            .wire
            .response()
            .into_result::<ops::FdsReceived>()
            .is_err());
        assert!(self.connection.closed.load(Ordering::SeqCst));
    }
}
impl Drop for Control {
    fn drop(&mut self) {
        self.connection.close();
        self.thread.take().unwrap().join().unwrap();
    }
}
fn live_control_cycles(gateway: bool) {
    let mut control = Control::new(gateway);
    let mut last_channel = String::new();
    for cycle in 0..520 {
        last_channel = control.closed_cycle(cycle);
    }
    assert_eq!(control.worker.transferred.load(Ordering::SeqCst), 520);
    control.capacity_preserves_channel();
    control.bad_receipt(if gateway { &last_channel } else { "foreign" });
}
#[test]
fn native_live_control_exceeds_512_closed_channels_without_losing_receipt_or_authority_checks() {
    live_control_cycles(false);
}
#[test]
fn gateway_live_control_exceeds_512_closed_channels_without_losing_receipt_or_authority_checks() {
    live_control_cycles(true);
}
