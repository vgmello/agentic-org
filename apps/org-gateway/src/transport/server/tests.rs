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

type TestHandler =
    fn(Request<Incoming>) -> std::future::Ready<Result<Response<Full<Bytes>>, Infallible>>;

fn fail_initialize() -> (TestHandler, std::future::Ready<()>) {
    panic!("invalid service must not initialize")
}

struct Running {
    address: SocketAddr,
    stop: Option<oneshot::Sender<()>>,
    done: mpsc::Receiver<io::Result<()>>,
    thread: Option<thread::JoinHandle<()>>,
    calls: Arc<AtomicUsize>,
}

impl Running {
    fn start(mode: ListenMode, limits: HttpLimits) -> Self {
        Self::start_on(TcpListener::bind("127.0.0.1:0").unwrap(), mode, limits)
    }

    fn start_on(listener: TcpListener, mode: ListenMode, limits: HttpLimits) -> Self {
        let address = listener.local_addr().unwrap();
        let scheme = if matches!(&mode, ListenMode::Https(_)) {
            "https"
        } else {
            "http"
        };
        let origin = ConfiguredOrigin::parse(&format!("{scheme}://{address}")).unwrap();
        Self::start_serving(listener, mode, origin, limits, |_| {
            Response::builder()
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from_static(b"{}")))
                .unwrap()
        })
    }

    fn start_serving(
        listener: TcpListener,
        mode: ListenMode,
        origin: ConfiguredOrigin,
        limits: HttpLimits,
        serve: impl Fn(Request<Incoming>) -> Response<Full<Bytes>> + Send + Sync + 'static,
    ) -> Self {
        let address = listener.local_addr().unwrap();
        let (stop, stopped) = oneshot::channel();
        let (ready, initialized) = mpsc::channel();
        let (done, completed) = mpsc::channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&calls);
        let thread = thread::spawn(move || {
            let result = run_http(listener, mode, origin, limits, move || {
                ready.send(()).unwrap();
                let handler = move |request: Request<Incoming>| {
                    observed.fetch_add(1, Ordering::AcqRel);
                    std::future::ready(Ok(serve(request)))
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

#[test]
fn plaintext_listener_accepts_its_exact_ipv6_loopback_origin() {
    let listener = TcpListener::bind("[::1]:0").unwrap();
    let server = Running::start_on(listener, ListenMode::HttpLoopback, HttpLimits::default());
    let mut stream = server.connect();
    stream.write_all(REQUEST).unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    assert!(response.starts_with(b"HTTP/1.1 200"));
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
    let origin =
        ConfiguredOrigin::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
    let (observed, initialized) = mpsc::channel();
    run_http(
        listener,
        ListenMode::HttpLoopback,
        origin,
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
fn b105_header_field_count_is_enforced_independently_of_header_bytes() {
    let server = Running::start(
        ListenMode::HttpLoopback,
        HttpLimits {
            header_count: 2,
            ..HttpLimits::default()
        },
    );
    assert!(server.request(REQUEST).starts_with(b"HTTP/1.1 200"));
    let response = server.protocol_refusal(
        b"GET / HTTP/1.1\r\nHost: gateway.test\r\nConnection: close\r\nX-Third: x\r\n\r\n",
    );
    assert!(response.starts_with(b"HTTP/1.1 431"));
    assert!(body(&response).is_empty());
    let head = String::from_utf8(response).unwrap().to_ascii_lowercase();
    assert!(!head.contains("content-security-policy"));
    assert!(!head.contains("content-type"));
    assert_eq!(server.calls.load(Ordering::Acquire), 1);
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

fn resuming_client() -> tokio_rustls::TlsConnector {
    resuming_client_for(&[&rustls::version::TLS13, &rustls::version::TLS12])
}

fn resuming_client_for(
    versions: &[&'static rustls::SupportedProtocolVersion],
) -> tokio_rustls::TlsConnector {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert(b'a').remove(0)).unwrap();
    roots.add(cert(b'b').remove(0)).unwrap();
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(versions)
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    tokio_rustls::TlsConnector::from(Arc::new(config))
}

#[test]
fn x02_s6_1_new_tls_handshakes_cannot_resume_a_replaced_identity() {
    for version in [&rustls::version::TLS13, &rustls::version::TLS12] {
        let certificates = Arc::new(TlsCertificates::new(cert(b'a'), key(b'a')).unwrap());
        let server = Running::start(
            ListenMode::Https(certificates.clone()),
            HttpLimits::default(),
        );
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let client = resuming_client_for(&[version]);
                for expected in *b"ab" {
                    let tcp = tokio::net::TcpStream::connect(server.address)
                        .await
                        .unwrap();
                    let mut tls = time::timeout(
                        GUARD,
                        client.connect(ServerName::try_from("gateway.test").unwrap(), tcp),
                    )
                    .await
                    .unwrap()
                    .unwrap();
                    assert_eq!(tls.get_ref().1.peer_certificates().unwrap(), cert(expected));
                    assert_eq!(
                        tls.get_ref().1.handshake_kind(),
                        Some(rustls::HandshakeKind::Full)
                    );
                    tls.write_all(REQUEST).await.unwrap();
                    let mut response = Vec::new();
                    time::timeout(GUARD, tls.read_to_end(&mut response))
                        .await
                        .unwrap()
                        .unwrap();
                    assert!(response.starts_with(b"HTTP/1.1 200"));
                    if expected == b'a' {
                        certificates.replace(cert(b'b'), key(b'b')).unwrap();
                    }
                }
            });
    }
}

#[test]
fn b105_positive_connection_budget_precedes_tls_handshakes_and_releases_capacity() {
    let certificates = Arc::new(TlsCertificates::new(cert(b'a'), key(b'a')).unwrap());
    let server = Running::start(
        ListenMode::Https(certificates),
        HttpLimits {
            connections: 1,
            ..HttpLimits::default()
        },
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let client = resuming_client();
            let first = time::timeout(
                GUARD,
                client.connect(
                    ServerName::try_from("gateway.test").unwrap(),
                    tokio::net::TcpStream::connect(server.address)
                        .await
                        .unwrap(),
                ),
            )
            .await
            .unwrap()
            .unwrap();
            // A completed first handshake proves its connection owns the only permit.
            let second_tcp = tokio::net::TcpStream::connect(server.address)
                .await
                .unwrap();
            let second = client.connect(ServerName::try_from("gateway.test").unwrap(), second_tcp);
            let mut second = pin!(second);
            assert!(time::timeout(Duration::from_millis(100), &mut second)
                .await
                .is_err());
            assert_eq!(server.calls.load(Ordering::Acquire), 0);
            drop(first);
            // The same waiting handshake must complete once the permit is released.
            let mut second = time::timeout(GUARD, second).await.unwrap().unwrap();
            second.write_all(REQUEST).await.unwrap();
            let mut response = Vec::new();
            time::timeout(GUARD, second.read_to_end(&mut response))
                .await
                .unwrap()
                .unwrap();
            assert!(response.starts_with(b"HTTP/1.1 200"));
            assert_eq!(server.calls.load(Ordering::Acquire), 1);
        });
}

#[test]
fn real_http_listener_releases_the_permit_of_a_stalled_writer() {
    real_listener_releases_stalled_writer(false);
}

#[test]
fn real_https_listener_releases_the_permit_of_a_stalled_writer() {
    real_listener_releases_stalled_writer(true);
}

fn real_listener_releases_stalled_writer(encrypted: bool) {
    // B1-05 transport contract: a non-reading peer must not retain a connection
    // forever. Exercise run_http's real TCP wrapper, also below the TLS layer.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let scheme = if encrypted { "https" } else { "http" };
    let origin =
        ConfiguredOrigin::parse(&format!("{scheme}://{}", listener.local_addr().unwrap())).unwrap();
    let mode = if encrypted {
        ListenMode::Https(Arc::new(
            TlsCertificates::new(cert(b'a'), key(b'a')).unwrap(),
        ))
    } else {
        ListenMode::HttpLoopback
    };
    let (entered, waiting) = mpsc::channel();
    let large = Bytes::from(vec![b'x'; 32 * 1024 * 1024]);
    let server = Running::start_serving(
        listener,
        mode,
        origin,
        HttpLimits {
            connections: 1,
            write_stall: Duration::from_millis(200),
            ..HttpLimits::default()
        },
        move |request| {
            let body = if request.uri().path() == "/stalled" {
                entered.send(()).unwrap();
                large.clone()
            } else {
                Bytes::from_static(b"second client served")
            };
            Response::new(Full::new(body))
        },
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            trait TestIo: AsyncRead + AsyncWrite + Unpin {}
            impl<T: AsyncRead + AsyncWrite + Unpin> TestIo for T {}
            let connector = resuming_client();
            let connect = || async {
                let tcp = tokio::net::TcpStream::connect(server.address)
                    .await
                    .unwrap();
                if encrypted {
                    let tls = connector
                        .connect(ServerName::try_from("gateway.test").unwrap(), tcp)
                        .await
                        .unwrap();
                    Box::new(tls) as Box<dyn TestIo>
                } else {
                    Box::new(tcp) as Box<dyn TestIo>
                }
            };
            let mut first = time::timeout(GUARD, connect()).await.unwrap();
            first
                .write_all(b"GET /stalled HTTP/1.1\r\nHost: gateway.test\r\n\r\n")
                .await
                .unwrap();
            waiting.recv_timeout(GUARD).unwrap();
            // Keep A open and never read it. B's handshake/response needs the
            // sole permit, and must finish before any unrelated 10s deadline.
            let response = time::timeout(Duration::from_secs(3), async {
                let mut second = connect().await;
                second.write_all(REQUEST).await.unwrap();
                let mut response = Vec::new();
                second.read_to_end(&mut response).await.unwrap();
                response
            })
            .await
            .expect("a non-reading first peer retained the connection permit");
            assert!(response.starts_with(b"HTTP/1.1 200"));
            assert_eq!(body(&response), b"second client served");
            assert_eq!(server.calls.load(Ordering::Acquire), 2);
            drop(first);
        });
}

fn tls_request(address: SocketAddr, expected: u8) {
    let response = tls_exchange(address, expected, REQUEST);
    assert!(response.starts_with(b"HTTP/1.1 200"));
    assert_eq!(body(&response), b"{}");
}

fn tls_exchange(address: SocketAddr, expected: u8, request: &[u8]) -> Vec<u8> {
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
            stream.write_all(request).await.unwrap();
            let mut response = Vec::new();
            time::timeout(GUARD, stream.read_to_end(&mut response))
                .await
                .unwrap()
                .unwrap();
            response
        })
}

#[test]
fn https_static_html_sends_the_full_policy_and_hsts_only_for_its_authority() {
    use crate::{assets::BuildFiles, Hsts, StaticUi};
    use std::collections::BTreeMap;

    const CSP: &str = "default-src 'none'; script-src 'self' 'sha256-X+zrZv/IbzjZUnhsbWlsecLbwjndTpG0ZynXOif7V+k='; style-src 'self' 'unsafe-inline'; img-src 'self'; font-src 'self'; connect-src 'self'; manifest-src 'self'; worker-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; object-src 'none'; upgrade-insecure-requests";
    let html = b"<!doctype html><script>0</script>";
    let length = html.len().to_string();
    let certificates = Arc::new(TlsCertificates::new(cert(b'a'), key(b'a')).unwrap());
    for (hsts, expected) in [
        (Hsts::Off, None),
        (Hsts::On, Some("max-age=31536000")),
        (Hsts::Clear, Some("max-age=0")),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let authority = format!("gateway.test:{}", listener.local_addr().unwrap().port());
        let origin = ConfiguredOrigin::parse(&format!("https://{authority}")).unwrap();
        let files = BuildFiles::from_files([("index.html".into(), html.to_vec())]).unwrap();
        let ui = StaticUi::new(files, origin.clone(), hsts);
        let server = Running::start_serving(
            listener,
            ListenMode::Https(Arc::clone(&certificates)),
            origin,
            HttpLimits::default(),
            move |request| ui.serve(&request),
        );

        for (host, expected) in [(authority.as_str(), expected), ("gateway.test", None)] {
            let request = format!("GET / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
            let response = tls_exchange(server.address, b'a', request.as_bytes());
            assert_eq!(body(&response), html);
            let (head, _) = std::str::from_utf8(&response)
                .unwrap()
                .split_once("\r\n\r\n")
                .unwrap();
            let mut lines = head.split("\r\n");
            assert_eq!(lines.next(), Some("HTTP/1.1 200 OK"));
            let mut headers = BTreeMap::new();
            for line in lines {
                let (name, value) = line.split_once(':').unwrap();
                assert!(headers
                    .insert(name.to_ascii_lowercase(), value.trim())
                    .is_none());
            }
            for (name, value) in [
                ("content-security-policy", CSP), ("cache-control", "no-cache"),
                ("content-type", "text/html; charset=utf-8"), ("content-length", length.as_str()),
                ("x-content-type-options", "nosniff"), ("referrer-policy", "same-origin"),
                ("x-frame-options", "DENY"), ("cross-origin-opener-policy", "same-origin"),
                ("cross-origin-resource-policy", "same-origin"),
                ("permissions-policy", "camera=(), microphone=(), geolocation=(), payment=(), usb=(), serial=(), hid=(), bluetooth=()"),
            ] {
                assert_eq!(headers.remove(name), Some(value), "{name}");
            }
            assert_eq!(headers.remove("strict-transport-security"), expected);
            assert_eq!(headers.remove("connection"), Some("close"));
            headers.remove("date");
            assert!(headers.is_empty(), "unexpected headers: {headers:?}");
        }
    }
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
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin =
        ConfiguredOrigin::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
    let limits = HttpLimits {
        connections: 0,
        ..HttpLimits::default()
    };
    assert!(
        matches!(run_http(listener, ListenMode::HttpLoopback, origin, limits, fail_initialize), Err(error) if error.kind() == io::ErrorKind::InvalidInput)
    );
    let listener = TcpListener::bind("0.0.0.0:0").unwrap();
    let origin =
        ConfiguredOrigin::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
    assert!(
        matches!(run_http(listener, ListenMode::HttpLoopback, origin, HttpLimits::default(), fail_initialize), Err(error) if error.kind() == io::ErrorKind::PermissionDenied)
    );
}

#[test]
fn loopback_origin_aliases_or_a_different_port_are_refused_before_initialization() {
    for host in ["localhost", "x.localhost", "[::1]", "127.0.0.2"] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = ConfiguredOrigin::parse(&format!(
            "http://{host}:{}",
            listener.local_addr().unwrap().port()
        ))
        .unwrap();
        assert!(
            matches!(run_http(listener, ListenMode::HttpLoopback, origin, HttpLimits::default(), fail_initialize), Err(error) if error.kind() == io::ErrorKind::PermissionDenied),
            "{host}"
        );
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let different = if port == 65535 { port - 1 } else { port + 1 };
    let origin = ConfiguredOrigin::parse(&format!("http://127.0.0.1:{different}")).unwrap();
    assert!(
        matches!(run_http(listener, ListenMode::HttpLoopback, origin, HttpLimits::default(), fail_initialize), Err(error) if error.kind() == io::ErrorKind::PermissionDenied)
    );
}

#[test]
fn https_origin_on_a_different_port_is_refused_before_initialization() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = ConfiguredOrigin::parse("https://gateway.test").unwrap();
    assert_ne!(listener.local_addr().unwrap().port(), 443);
    let certificates = Arc::new(TlsCertificates::new(cert(b'a'), key(b'a')).unwrap());
    assert!(
        matches!(run_http(listener, ListenMode::Https(certificates), origin,
        HttpLimits::default(), fail_initialize), Err(error) if error.kind() == io::ErrorKind::InvalidInput)
    );
}

#[test]
fn http_origin_on_an_https_listener_is_refused_before_initialization() {
    // B1-05 headers spec, "Other headers", interim origin fit rules:
    // "the scheme equals the mode". Refuse before application initialization.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = ConfiguredOrigin::parse(&format!(
        "http://gateway.test:{}",
        listener.local_addr().unwrap().port()
    ))
    .unwrap();
    let certificates = Arc::new(TlsCertificates::new(cert(b'a'), key(b'a')).unwrap());
    assert!(
        matches!(run_http(listener, ListenMode::Https(certificates), origin,
        HttpLimits::default(), fail_initialize), Err(error) if error.kind() == io::ErrorKind::InvalidInput)
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
