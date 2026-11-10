use super::*;
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use tokio::sync::oneshot;
use tokio_rustls::rustls;

const GUARD: Duration = Duration::from_secs(10);
const REQUEST: &[u8] = b"GET / HTTP/1.1\r\nHost: gateway.test\r\nConnection: close\r\n\r\n";

struct Running {
    address: SocketAddr,
    stop: Option<oneshot::Sender<()>>,
    done: mpsc::Receiver<io::Result<()>>,
    thread: Option<thread::JoinHandle<()>>,
    calls: Arc<AtomicUsize>,
}

impl Running {
    fn start(mode: ListenMode, limits: HttpLimits) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, stopped) = oneshot::channel();
        let (ready, initialized) = mpsc::channel();
        let (done, completed) = mpsc::channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&calls);
        let thread = thread::spawn(move || {
            let result = run_http(listener, mode, limits, move || {
                ready.send(()).unwrap();
                let handler = move |_request: Request<Incoming>| {
                    observed.fetch_add(1, Ordering::AcqRel);
                    async {
                        Ok(Response::builder()
                            .header("Content-Type", "application/json")
                            .body(Full::new(Bytes::from_static(b"{}")))
                            .unwrap())
                    }
                };
                (handler, async {
                    let _ = stopped.await;
                })
            });
            let _ = done.send(result);
        });
        initialized.recv_timeout(GUARD).unwrap();
        Self {
            address,
            stop: Some(stop),
            done: completed,
            thread: Some(thread),
            calls,
        }
    }

    fn connect(&self) -> TcpStream {
        let stream = TcpStream::connect(self.address).unwrap();
        stream.set_read_timeout(Some(GUARD)).unwrap();
        stream.set_write_timeout(Some(GUARD)).unwrap();
        stream
    }

    fn request(&self, bytes: &[u8]) -> Vec<u8> {
        self.exchange(bytes, false)
    }

    fn protocol_refusal(&self, bytes: &[u8]) -> Vec<u8> {
        self.exchange(bytes, true)
    }

    fn exchange(&self, bytes: &[u8], refusal_may_reset: bool) -> Vec<u8> {
        let mut stream = self.connect();
        stream.write_all(bytes).unwrap();
        let mut response = Vec::new();
        if let Err(error) = stream.read_to_end(&mut response) {
            // A parser refusal closes with unread input still in the socket.
            // TCP may report reset after delivering the complete error reply.
            // The test below still requires that 4xx and its empty body.
            assert!(
                refusal_may_reset && error.kind() == io::ErrorKind::ConnectionReset,
                "{error}"
            );
        }
        response
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let result = self
            .done
            .recv_timeout(GUARD)
            .expect("HTTP service did not stop");
        self.thread.take().unwrap().join().unwrap();
        result.unwrap();
    }
}

fn body(response: &[u8]) -> &[u8] {
    let end = response
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .unwrap();
    &response[end + 4..]
}

#[test]
fn factory_runs_inside_the_runtime_and_an_existing_listener_serves_http1() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let (observed, initialized) = mpsc::channel();
    run_http(
        listener,
        ListenMode::HttpLoopback,
        HttpLimits::default(),
        move || {
            assert!(tokio::runtime::Handle::try_current().is_ok());
            observed.send(()).unwrap();
            (
                |_request| async { Ok(Response::new(Full::new(Bytes::new()))) },
                async {},
            )
        },
    )
    .unwrap();
    initialized.recv_timeout(GUARD).unwrap();

    let server = Running::start(ListenMode::HttpLoopback, HttpLimits::default());
    let response = server.request(REQUEST);
    assert!(response.starts_with(b"HTTP/1.1 200"));
    assert_eq!(body(&response), b"{}");
    assert_eq!(server.calls.load(Ordering::Acquire), 1);
}

#[test]
fn malformed_and_oversize_headers_close_with_bodyless_errors_before_a_handler() {
    let limits = HttpLimits {
        header_bytes: 8192,
        ..HttpLimits::default()
    };
    let server = Running::start(ListenMode::HttpLoopback, limits);
    let response = server.protocol_refusal(b"NOT A REQUEST\r\n\r\n");
    assert!(response.starts_with(b"HTTP/1.1 400"));
    assert!(body(&response).is_empty());
    assert!(!String::from_utf8_lossy(&response).contains("text/html"));
    let mut request = b"GET / HTTP/1.1\r\nHost: gateway.test\r\nX-Large: ".to_vec();
    request.extend(vec![b'x'; 9000]);
    request.extend(b"\r\n\r\n");
    let response = server.protocol_refusal(&request);
    assert!(response.starts_with(b"HTTP/1.1 431"));
    assert!(body(&response).is_empty());
    assert!(!String::from_utf8_lossy(&response).contains("text/html"));
    assert_eq!(server.calls.load(Ordering::Acquire), 0);
}

#[test]
fn initial_and_keep_alive_idleness_close_at_the_injected_header_deadline() {
    let limits = HttpLimits {
        header_timeout: Duration::from_millis(100),
        ..HttpLimits::default()
    };
    let server = Running::start(ListenMode::HttpLoopback, limits);
    let mut idle = server.connect();
    let mut response = Vec::new();
    idle.read_to_end(&mut response).unwrap();
    if !response.is_empty() {
        assert!(body(&response).is_empty());
    }
    assert_eq!(server.calls.load(Ordering::Acquire), 0);
    let response =
        server.request(b"GET / HTTP/1.1\r\nHost: gateway.test\r\nConnection: keep-alive\r\n\r\n");
    assert!(response.starts_with(b"HTTP/1.1 200"));
    assert_eq!(body(&response), b"{}");
    assert_eq!(server.calls.load(Ordering::Acquire), 1);
}

fn cert(which: u8) -> Vec<CertificateDer<'static>> {
    let bytes: &[u8] = if which == b'a' {
        include_bytes!("../../../tests/fixtures/test-cert-a.der")
    } else {
        include_bytes!("../../../tests/fixtures/test-cert-b.der")
    };
    vec![CertificateDer::from(bytes.to_vec())]
}

fn key(which: u8) -> rustls::pki_types::PrivateKeyDer<'static> {
    let bytes: &[u8] = if which == b'a' {
        include_bytes!("../../../tests/fixtures/test-key-a.der")
    } else {
        include_bytes!("../../../tests/fixtures/test-key-b.der")
    };
    PrivatePkcs8KeyDer::from(bytes.to_vec()).into()
}

fn tls_request(address: SocketAddr, expected: u8) {
    tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut roots = rustls::RootCertStore::empty();
            roots.add(cert(b'a').remove(0)).unwrap();
            roots.add(cert(b'b').remove(0)).unwrap();
            let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth();
            config.alpn_protocols = vec![b"http/1.1".to_vec()];
            config.resumption = rustls::client::Resumption::disabled();
            let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
            let stream = tokio::net::TcpStream::connect(address).await.unwrap();
            let mut stream = time::timeout(
                GUARD,
                connector.connect(ServerName::try_from("gateway.test").unwrap(), stream),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(
                stream.get_ref().1.peer_certificates().unwrap(),
                cert(expected)
            );
            assert_eq!(
                stream.get_ref().1.alpn_protocol(),
                Some(b"http/1.1".as_slice())
            );
            stream.write_all(REQUEST).await.unwrap();
            let mut response = Vec::new();
            time::timeout(GUARD, stream.read_to_end(&mut response))
                .await
                .unwrap()
                .unwrap();
            assert!(response.starts_with(b"HTTP/1.1 200"));
            assert_eq!(body(&response), b"{}");
        });
}

#[test]
fn real_https_handshakes_observe_replacement_without_restarting_the_service() {
    let certificates = Arc::new(TlsCertificates::new(cert(b'a'), key(b'a')).unwrap());
    let server = Running::start(
        ListenMode::Https(Arc::clone(&certificates)),
        HttpLimits::default(),
    );
    tls_request(server.address, b'a');
    certificates.replace(cert(b'b'), key(b'b')).unwrap();
    tls_request(server.address, b'b');
    assert_eq!(server.calls.load(Ordering::Acquire), 2);
}

#[test]
fn non_tls_clients_on_https_are_bounded_without_reaching_application_code() {
    let certificates = Arc::new(TlsCertificates::new(cert(b'a'), key(b'a')).unwrap());
    let limits = HttpLimits {
        handshake_timeout: Duration::from_millis(100),
        ..HttpLimits::default()
    };
    let server = Running::start(ListenMode::Https(certificates), limits);
    let mut stream = server.connect();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    assert!(response.is_empty());
    assert_eq!(server.calls.load(Ordering::Acquire), 0);
}

#[test]
fn invalid_limits_and_non_loopback_plaintext_are_refused_before_initialization() {
    type TestHandler =
        fn(Request<Incoming>) -> std::future::Ready<Result<Response<Full<Bytes>>, Infallible>>;
    fn fail_initialize() -> (TestHandler, std::future::Ready<()>) {
        panic!("invalid service must not initialize")
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let limits = HttpLimits {
        connections: 0,
        ..HttpLimits::default()
    };
    assert!(
        matches!(run_http(listener, ListenMode::HttpLoopback, limits, fail_initialize), Err(error) if error.kind() == io::ErrorKind::InvalidInput)
    );
    let listener = TcpListener::bind("0.0.0.0:0").unwrap();
    assert!(
        matches!(run_http(listener, ListenMode::HttpLoopback, HttpLimits::default(), fail_initialize), Err(error) if error.kind() == io::ErrorKind::PermissionDenied)
    );
}

fn virtual_time(test: impl Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            time::pause();
            test.await;
        });
}

fn large_response(
    _: Request<Incoming>,
) -> std::future::Ready<Result<Response<Full<Bytes>>, Infallible>> {
    std::future::ready(Ok(Response::new(Full::new(Bytes::from_static(
        &[b'x'; 8192],
    )))))
}

#[test]
fn full_http_stack_times_out_when_a_client_stops_reading() {
    virtual_time(async {
        use std::error::Error as _;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (server, mut client) = tokio::io::duplex(128);
        let server = WriteStall::new(server, Duration::from_secs(30)).unwrap();
        let connection = tokio::spawn(serve_connection(
            server,
            Arc::new(large_response),
            HttpLimits::default(),
        ));
        client.write_all(REQUEST).await.unwrap();
        // Bounded in-memory I/O makes the non-reading condition deterministic:
        // the real Hyper stack cannot fit the response in the 128-byte pipe.
        let error = time::timeout(Duration::from_secs(60), connection)
            .await
            .expect("stalled HTTP connection must finish within its write bound")
            .unwrap()
            .unwrap_err();
        let mut source = error.source();
        let mut timed_out = false;
        while let Some(error) = source {
            timed_out |= error
                .downcast_ref::<io::Error>()
                .is_some_and(|error| error.kind() == io::ErrorKind::TimedOut);
            source = error.source();
        }
        assert!(timed_out, "connection must end on the write-stall error");
        let mut partial = Vec::new();
        client.read_to_end(&mut partial).await.unwrap();
        assert!(partial.starts_with(b"HTTP/1.1 200"));
        assert!(body(&partial).len() < 8192);
    });
}

#[test]
fn full_http_stack_completes_a_slow_but_continuously_read_response() {
    virtual_time(async {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let started = time::Instant::now();
        let (server, mut client) = tokio::io::duplex(128);
        let server = WriteStall::new(server, Duration::from_secs(30)).unwrap();
        let connection = tokio::spawn(serve_connection(
            server,
            Arc::new(large_response),
            HttpLimits::default(),
        ));
        client.write_all(REQUEST).await.unwrap();
        let mut response = Vec::new();
        let mut chunk = [0; 128];
        time::timeout(Duration::from_secs(2000), async {
            loop {
                let read = client.read(&mut chunk).await.unwrap();
                if read == 0 {
                    break;
                }
                response.extend_from_slice(&chunk[..read]);
                time::advance(Duration::from_secs(20)).await;
            }
        })
        .await
        .unwrap();
        connection.await.unwrap().unwrap();
        assert!(started.elapsed() > Duration::from_secs(30));
        assert!(response.starts_with(b"HTTP/1.1 200"));
        assert_eq!(body(&response), &[b'x'; 8192]);
    });
}
