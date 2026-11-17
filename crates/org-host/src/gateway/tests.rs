//! Supervisor and handoff unit tests. The real-process test (the host spawning the
//! real `agentic-org-web`) needs the built web binary and is added separately.
use super::*;
use crate::service::listener::Connection;
use std::os::fd::OwnedFd;

struct NoOps;
impl Handler for NoOps {
    fn handle(&self, _: org_proto::Request, _: &Arc<Connection>, _: crate::peer::Received) {}
}

fn loopback() -> ListenConfig {
    serde_json::from_str(
        r#"{"address":"127.0.0.1","port":48123,"scheme":"http_loopback","origin":"http://127.0.0.1:48123","hsts":"off"}"#,
    )
    .unwrap()
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn refused_gateway_start_never_becomes_ready() {
    let dir = tempfile::tempdir().unwrap();
    let state = Arc::new(Mutex::new(GatewayState::NotConfigured));
    let started = Supervisor::start(
        Configuration {
            program: dir.path().join("agentic-org-web"),
            instance: "instance".into(),
            runtime: dir.path().to_owned(),
            listen: loopback(),
            check: ServerCheck::gateway_for_build(false, None).unwrap(),
            critical_paths: vec![],
            release: None,
        },
        Arc::new(NoOps),
        Arc::clone(&state),
        Arc::new(|| true),
    );
    assert!(
        started.is_err(),
        "a release build without a team never spawns"
    );
    let observed = *lock(&state);
    assert_ne!(observed, GatewayState::Running);
    assert!(!crate::service::gateway_serving(true, true, observed));
}

struct Fake(Arc<AtomicBool>);
impl Serving for Fake {
    fn serving(&mut self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[test]
fn readiness_turns_false_when_the_gateway_exits_or_its_control_reaches_eof() {
    let stopping = Arc::new(AtomicBool::new(false));
    let state = Arc::new(Mutex::new(GatewayState::NotConfigured));
    let live = Arc::new(AtomicBool::new(true));
    let starts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let supervisor = {
        let (stopping, state, live, starts) = (
            Arc::clone(&stopping),
            Arc::clone(&state),
            Arc::clone(&live),
            Arc::clone(&starts),
        );
        thread::spawn(move || {
            supervise(&stopping, &state, &|| true, || {
                starts.fetch_add(1, Ordering::SeqCst);
                Ok(Fake(Arc::clone(&live)))
            })
        })
    };
    wait_for("running", || *lock(&state) == GatewayState::Running);
    assert!(crate::service::gateway_serving(true, true, *lock(&state)));
    live.store(false, Ordering::SeqCst); // child exit or control EOF
    wait_for("failed", || *lock(&state) == GatewayState::Failed);
    assert!(!crate::service::gateway_serving(true, true, *lock(&state)));
    stopping.store(true, Ordering::SeqCst);
    supervisor.thread().unpark();
    supervisor.join().unwrap();
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert_eq!(*lock(&state), GatewayState::Failed);
}

#[test]
fn an_ineligible_host_never_starts_the_gateway() {
    let stopping = Arc::new(AtomicBool::new(false));
    let state = Arc::new(Mutex::new(GatewayState::NotConfigured));
    let supervisor = {
        let (stopping, state) = (Arc::clone(&stopping), Arc::clone(&state));
        thread::spawn(move || {
            supervise(&stopping, &state, &|| false, || -> io::Result<Fake> {
                panic!("started without HQ/authentication")
            })
        })
    };
    wait_for("starting", || *lock(&state) == GatewayState::Starting);
    thread::sleep(Duration::from_millis(120));
    assert_eq!(*lock(&state), GatewayState::Starting);
    stopping.store(true, Ordering::SeqCst);
    supervisor.thread().unpark();
    supervisor.join().unwrap();
}

#[test]
fn transfer_end_handed_to_the_gateway_cannot_send() {
    let (transfer, child) = transfer_pair().unwrap();
    let error = child.send(b"x").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe, "{error}");
    transfer.send(b"record").unwrap();
    let mut bytes = [0u8; 16];
    assert_eq!(child.recv(&mut bytes).unwrap(), 6);
}

#[test]
fn handoff_sends_exactly_the_transfer_end_then_the_listener() {
    let (mut host, gateway) = UnixStream::pair().unwrap();
    let (transfer, child) = transfer_pair().unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    send_handoff(
        &mut host,
        &child,
        &listener,
        Instant::now() + Duration::from_secs(5),
    )
    .unwrap();
    drop(child);
    drop(listener);

    let mut receiver = org_socket::rights::StreamReceiver::new(gateway);
    let mut header = Vec::new();
    loop {
        // Every header byte carries no descriptor (an early descriptor is refused).
        let (byte, fds) = receiver.receive_byte(0, &SPAWN_LOCK).unwrap();
        assert!(fds.is_empty());
        if byte == b'\n' {
            break;
        }
        header.push(byte);
    }
    let named: GatewayHandoff = serde_json::from_slice(&header).unwrap();
    assert_eq!(named, GatewayHandoff);
    let header: serde_json::Value = serde_json::from_slice(&header).unwrap();
    assert_eq!(header["fds"], serde_json::json!(["transfer", "listener"]));

    // Exactly two descriptors ride the marker; any other count is refused.
    let (marker, fds) = receiver
        .receive_byte(GATEWAY_HANDOFF_FDS.len(), &SPAWN_LOCK)
        .unwrap();
    assert_eq!(marker, org_host_control::ops::HOST_SOCK_MARKER);
    let [first, second]: [OwnedFd; 2] = fds.try_into().unwrap();

    // First: the gateway's receive-only transfer end, paired with the host's end.
    let received = UnixDatagram::from(first);
    assert_eq!(
        received.send(b"x").unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
    transfer.send(b"record").unwrap();
    let mut bytes = [0u8; 16];
    assert_eq!(received.recv(&mut bytes).unwrap(), 6);

    // Second: the bound browser listener.
    let listening = TcpListener::from(second);
    assert_eq!(listening.local_addr().unwrap().port(), port);
}
