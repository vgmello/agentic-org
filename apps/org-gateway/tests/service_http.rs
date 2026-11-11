use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use hyper::http::request::Parts;
use hyper::{Method, StatusCode};
use org_gateway::assets::BuildFiles;
use org_gateway::body::BusinessBody;
use org_gateway::path::RequestPath;
use org_gateway::service::{
    run_gateway, ApiBackend, ApiLimits, ApiReply, GatewayConfig, PreparedCall,
};
use org_gateway::transport::{HttpLimits, ListenMode};
use org_gateway::{ConfiguredOrigin, Hsts};
use org_proto::gateway::{GatewayErrorCode, OperationId};
use serde_json::{json, Value};
use tokio::sync::oneshot;

const GUARD: Duration = Duration::from_secs(10);
const OPERATION: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const CSP: &str =
    "default-src 'none'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'";

#[derive(Clone, Copy)]
enum Route {
    Read,
    Mutation,
}

struct Call {
    block: bool,
}

struct TestBackend {
    release: Mutex<mpsc::Receiver<()>>,
    started: mpsc::Sender<()>,
    executed: Arc<AtomicUsize>,
    routed: Arc<AtomicUsize>,
    prepared: Arc<AtomicUsize>,
}

impl ApiBackend for TestBackend {
    type Route = Route;
    type Call = Call;

    fn route(&self, request: &Parts, path: &RequestPath) -> Result<Route, ApiReply> {
        self.routed.fetch_add(1, Ordering::AcqRel);
        match (
            request.method.clone(),
            path.segments()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .as_slice(),
        ) {
            (Method::POST, ["api", "v1", "test", "read"]) => Ok(Route::Read),
            (Method::POST, ["api", "v1", "test", "mutate"]) => Ok(Route::Mutation),
            _ => Err(ApiReply::error(
                StatusCode::NOT_FOUND,
                GatewayErrorCode::NotFound,
                "Not found.",
            )),
        }
    }

    fn prepare(&self, route: Route, body: BusinessBody) -> Result<PreparedCall<Call>, ApiReply> {
        self.prepared.fetch_add(1, Ordering::AcqRel);
        let value: Value = serde_json::from_slice(body.as_bytes()).map_err(|_| {
            ApiReply::error(
                StatusCode::BAD_REQUEST,
                GatewayErrorCode::InvalidRequest,
                "Invalid JSON body.",
            )
        })?;
        let call = Call {
            block: value.get("block").and_then(Value::as_bool).unwrap_or(false),
        };
        Ok(match route {
            Route::Read => PreparedCall::read(call, 0),
            Route::Mutation => PreparedCall::mutation(
                call,
                OperationId::parse(OPERATION.into()).unwrap(),
                OPERATION.len(),
            ),
        })
    }

    fn execute(&self, call: Call) -> ApiReply {
        self.executed.fetch_add(1, Ordering::AcqRel);
        if call.block {
            self.started.send(()).unwrap();
            self.release.lock().unwrap().recv_timeout(GUARD).unwrap();
        }
        ApiReply::json(StatusCode::OK, json!({"result": {}}))
    }
}

struct Running {
    address: SocketAddr,
    release: mpsc::Sender<()>,
    started: mpsc::Receiver<()>,
    executed: Arc<AtomicUsize>,
    routed: Arc<AtomicUsize>,
    prepared: Arc<AtomicUsize>,
    stop: Option<oneshot::Sender<()>>,
    done: mpsc::Receiver<std::io::Result<()>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Running {
    fn start(limits: ApiLimits) -> Self {
        Self::start_with_tls(limits, false)
    }

    fn start_with_tls(limits: ApiLimits, tls: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let origin = if tls {
            ConfiguredOrigin::parse(&format!("https://gateway.test:{}", address.port())).unwrap()
        } else {
            ConfiguredOrigin::parse(&format!("http://{address}")).unwrap()
        };
        let mode = if tls {
            use org_gateway::tls::TlsCertificates;
            use tokio_rustls::rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
            let cert = CertificateDer::from(include_bytes!("fixtures/test-cert-a.der").to_vec());
            let key =
                PrivatePkcs8KeyDer::from(include_bytes!("fixtures/test-key-a.der").to_vec()).into();
            ListenMode::Https(Arc::new(TlsCertificates::new(vec![cert], key).unwrap()))
        } else {
            ListenMode::HttpLoopback
        };
        let files = BuildFiles::from_files([(
            "index.html".into(),
            b"<!doctype html><script>0</script>".to_vec(),
        )])
        .unwrap();
        let (release, gate) = mpsc::channel();
        let (started, observed) = mpsc::channel();
        let executed = Arc::new(AtomicUsize::new(0));
        let routed = Arc::new(AtomicUsize::new(0));
        let prepared = Arc::new(AtomicUsize::new(0));
        let backend = TestBackend {
            release: Mutex::new(gate),
            started,
            executed: Arc::clone(&executed),
            routed: Arc::clone(&routed),
            prepared: Arc::clone(&prepared),
        };
        let (stop, stopped) = oneshot::channel();
        let (ready, initialized) = mpsc::channel();
        let (done, completed) = mpsc::channel();
        let thread = thread::spawn(move || {
            let result = run_gateway(
                listener,
                GatewayConfig {
                    mode,
                    origin,
                    hsts: Hsts::On,
                    http: HttpLimits::default(),
                    api: limits,
                },
                files,
                move || {
                    ready.send(()).unwrap();
                    Ok((backend, async {
                        let _ = stopped.await;
                    }))
                },
            );
            let _ = done.send(result);
        });
        initialized.recv_timeout(GUARD).unwrap();
        Self {
            address,
            release,
            started: observed,
            executed,
            routed,
            prepared,
            stop: Some(stop),
            done: completed,
            thread: Some(thread),
        }
    }

    fn request(&self, path: &str, body: &[u8]) -> Wire {
        exchange(self.address, path, body)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        // Unblock all admitted calls even if an assertion fails.
        let _ = self.release.send(());
        let _ = self.release.send(());
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let result = self.done.recv_timeout(GUARD).expect("gateway did not stop");
        self.thread.take().unwrap().join().unwrap();
        result.unwrap();
    }
}

struct Wire {
    status: String,
    headers: BTreeMap<String, String>,
    body: Value,
}

impl Wire {
    fn read(mut stream: TcpStream) -> Self {
        let mut response = String::new();
        if let Err(error) = stream.read_to_string(&mut response) {
            assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset);
        }
        let (head, body) = response
            .split_once("\r\n\r\n")
            .expect("application response");
        let mut lines = head.split("\r\n");
        let status = lines.next().unwrap().to_owned();
        let mut headers = BTreeMap::new();
        for line in lines {
            let (key, value) = line.split_once(':').unwrap();
            assert!(headers
                .insert(key.to_ascii_lowercase(), value.trim().to_owned())
                .is_none());
        }
        Self {
            status,
            headers,
            body: serde_json::from_str(body).unwrap(),
        }
    }

    fn assert_error_headers(&self, status: u16) {
        self.assert_error_headers_version(status, "HTTP/1.1");
    }

    fn assert_error_headers_version(&self, status: u16, version: &str) {
        assert!(
            self.status.starts_with(&format!("{version} {status} ")),
            "{}",
            self.status
        );
        let mut headers = self.headers.clone();
        for (name, value) in [
            ("content-security-policy", CSP), ("cache-control", "no-store"),
            ("content-type", "application/json; charset=utf-8"),
            ("x-content-type-options", "nosniff"), ("referrer-policy", "same-origin"),
            ("x-frame-options", "DENY"), ("cross-origin-opener-policy", "same-origin"),
            ("cross-origin-resource-policy", "same-origin"),
            ("permissions-policy", "camera=(), microphone=(), geolocation=(), payment=(), usb=(), serial=(), hid=(), bluetooth=()"),
        ] {
            assert_eq!(headers.remove(name).as_deref(), Some(value), "{name}");
        }
        assert!(
            headers
                .remove("content-length")
                .unwrap()
                .parse::<usize>()
                .unwrap()
                > 0
        );
        assert_eq!(headers.remove("connection").as_deref(), Some("close"));
        headers.remove("date");
        assert!(headers.is_empty(), "unexpected headers {headers:?}");
    }
}

fn connect(address: SocketAddr) -> TcpStream {
    let stream = TcpStream::connect_timeout(&address, GUARD).unwrap();
    stream.set_read_timeout(Some(GUARD)).unwrap();
    stream.set_write_timeout(Some(GUARD)).unwrap();
    stream
}

fn exchange(address: SocketAddr, path: &str, body: &[u8]) -> Wire {
    let mut stream = connect(address);
    write!(stream, "POST {path} HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n", body.len()).unwrap();
    stream.write_all(body).unwrap();
    Wire::read(stream)
}

#[test]
fn b105_http_1_1_only_refuses_http_1_0_before_native_admission() {
    let server = Running::start(ApiLimits::default());
    for connection in ["close", "keep-alive"] {
        let mut stream = connect(server.address);
        write!(stream, "POST /api/v1/test/read HTTP/1.0\r\nHost: {}\r\nConnection: {connection}\r\nContent-Length: 2\r\n\r\n{{}}", server.address).unwrap();
        let response = Wire::read(stream);
        response.assert_error_headers_version(400, "HTTP/1.0");
        assert_eq!(server.executed.load(Ordering::Acquire), 0);
    }
}

#[test]
fn rowan_b105_literal_fragment_uses_the_same_effective_api_target() {
    let server = Running::start(ApiLimits::default());
    let mut normal = server.request("/api/v1/test/read", b"{}");
    let mut raw = server.request("/api/v1/test/read#/%2f/%zz/..", b"{}");
    assert_eq!(raw.status, "HTTP/1.1 200 OK");
    assert_eq!(raw.status, normal.status);
    assert_eq!(raw.body, normal.body);
    normal.headers.remove("date");
    raw.headers.remove("date");
    assert_eq!(raw.headers, normal.headers);
    assert_eq!(server.executed.load(Ordering::Acquire), 2);
    // The retained prefix still undergoes the existing raw-path validation.
    server
        .request("/api/v1/test/%zz#ignored", b"{}")
        .assert_error_headers(400);
    assert_eq!(server.executed.load(Ordering::Acquire), 2);
}

#[test]
fn rowan_b105_hashes_in_chunked_business_data_remain_ordinary_data() {
    let server = Running::start(ApiLimits::default());
    let mut stream = connect(server.address);
    write!(stream, "POST /api/v1/test/read HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nConnection: close\r\nTransfer-Encoding: chunked\r\n\r\n", address = server.address).unwrap();
    // A body chunk can look exactly like a request line. It is not a request target.
    for chunk in ["{\"value\":\"", "GET /# HTTP/1.1", "\"}"] {
        write!(stream, "{:x}\r\n{chunk}\r\n", chunk.len()).unwrap();
    }
    stream.write_all(b"0\r\n\r\n").unwrap();
    let response = Wire::read(stream);
    assert_eq!(response.status, "HTTP/1.1 200 OK");
    assert_eq!(server.executed.load(Ordering::Acquire), 1);
}

#[test]
fn rowan_b105_next_keepalive_target_uses_the_same_effective_path() {
    let server = Running::start(ApiLimits::default());
    let mut stream = connect(server.address);
    write!(stream, "POST /api/v1/test/read HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nContent-Length: 2\r\n\r\n{{}}POST /api/v1/test/read#fragment HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nConnection: close\r\nContent-Length: 2\r\n\r\n{{}}", address = server.address).unwrap();
    let mut response = String::new();
    if let Err(error) = stream.read_to_string(&mut response) {
        assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset);
    }
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert_eq!(response.matches("HTTP/1.1 200 OK\r\n").count(), 2);
    assert_eq!(response.matches("{\"result\":{}}").count(), 2);
    assert!(!response.contains("text/html"));
    assert_eq!(server.executed.load(Ordering::Acquire), 2);
}

#[test]
fn path_json_and_body_refusals_use_the_shared_response_policy_before_dispatch() {
    let server = Running::start(ApiLimits {
        body_bytes: 16,
        body_timeout: Duration::from_millis(50),
        ..ApiLimits::default()
    });
    for path in ["/bad%zz", "/bad//path", "/bad/%2e"] {
        server.request(path, b"{}").assert_error_headers(400);
    }
    server
        .request("/api/v1/test/read", b"{broken}")
        .assert_error_headers(400);
    server
        .request("/api/v1/test/read", &[b'x'; 17])
        .assert_error_headers(413);
    let mut malformed = connect(server.address);
    write!(malformed, "POST /api/v1/test/read HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nConnection: close\r\nTransfer-Encoding: chunked\r\n\r\nZ\r\ninvalid\r\n", address = server.address).unwrap();
    Wire::read(malformed).assert_error_headers(400);
    let mut stream = connect(server.address);
    write!(stream, "POST /api/v1/test/read HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nConnection: close\r\nContent-Length: 10\r\n\r\n{{", address = server.address).unwrap();
    Wire::read(stream).assert_error_headers(503);
    assert_eq!(server.executed.load(Ordering::Acquire), 0);
    let valid = server.request("/api/v1/test/read", b"{}");
    assert_eq!(valid.body, json!({"result": {}}));
    assert_eq!(server.executed.load(Ordering::Acquire), 1);
}

#[test]
fn admitted_deadlines_keep_the_operation_id_and_queue_refusals_never_execute() {
    let server = Running::start(ApiLimits {
        backend_timeout: Duration::from_millis(75),
        workers: 1,
        waiting_jobs: 1,
        ..ApiLimits::default()
    });
    let address = server.address;
    let first =
        thread::spawn(move || exchange(address, "/api/v1/test/mutate", br#"{"block":true}"#));
    server.started.recv_timeout(GUARD).unwrap();
    let first = first.join().unwrap();
    first.assert_error_headers(504);
    assert_eq!(
        first.body,
        json!({"outcome":"unknown", "operation_id": OPERATION})
    );
    let queued = server.request("/api/v1/test/mutate", br#"{"block":true}"#);
    queued.assert_error_headers(504);
    assert_eq!(queued.body, first.body);
    let refused = server.request("/api/v1/test/mutate", br#"{"block":true}"#);
    refused.assert_error_headers(503);
    assert_eq!(refused.body["outcome"], "rejected");
    assert_eq!(refused.body["operation_id"], OPERATION);
    assert_eq!(server.executed.load(Ordering::Acquire), 1);
    server.release.send(()).unwrap();
    server.started.recv_timeout(GUARD).unwrap();
    assert_eq!(server.executed.load(Ordering::Acquire), 2);
    server.release.send(()).unwrap();
}

#[test]
fn prepared_input_bytes_participate_in_admission_before_execution() {
    let server = Running::start(ApiLimits {
        waiting_bytes: OPERATION.len() - 1,
        ..ApiLimits::default()
    });
    let response = server.request("/api/v1/test/mutate", b"{}");
    response.assert_error_headers(503);
    assert_eq!(response.body["outcome"], "rejected");
    assert_eq!(response.body["operation_id"], OPERATION);
    assert_eq!(server.executed.load(Ordering::Acquire), 0);
}

#[test]
fn invalid_api_limits_fail_before_application_initialization() {
    use org_gateway::service::UnavailableApi;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin =
        ConfiguredOrigin::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
    let files =
        BuildFiles::from_files([("index.html".into(), b"<script>0</script>".to_vec())]).unwrap();
    let error = run_gateway::<UnavailableApi, std::future::Ready<()>>(
        listener,
        GatewayConfig {
            mode: ListenMode::HttpLoopback,
            origin,
            hsts: Hsts::Off,
            http: HttpLimits::default(),
            api: ApiLimits {
                workers: 0,
                ..ApiLimits::default()
            },
        },
        files,
        || panic!("an invalid service must not initialize the application"),
    )
    .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn the_composed_https_service_sends_html_hash_upgrade_and_authority_hsts() {
    use rustls::pki_types::{CertificateDer, ServerName};
    use tokio_rustls::rustls;
    let server = Running::start_with_tls(ApiLimits::default(), true);
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(CertificateDer::from(
            include_bytes!("fixtures/test-cert-a.der").to_vec(),
        ))
        .unwrap();
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    let connection = rustls::ClientConnection::new(
        Arc::new(config),
        ServerName::try_from("gateway.test").unwrap(),
    )
    .unwrap();
    let mut stream = rustls::StreamOwned::new(connection, connect(server.address));
    write!(
        stream,
        "GET / HTTP/1.1\r\nHost: gateway.test:{}\r\nConnection: close\r\n\r\n",
        server.address.port()
    )
    .unwrap();
    let mut wire = String::new();
    stream.read_to_string(&mut wire).unwrap();
    let (head, body) = wire.split_once("\r\n\r\n").unwrap();
    assert!(head.starts_with("HTTP/1.1 200 OK\r\n"));
    assert_eq!(body, "<!doctype html><script>0</script>");
    let mut headers = BTreeMap::new();
    for line in head.split("\r\n").skip(1) {
        let (name, value) = line.split_once(':').unwrap();
        assert!(headers
            .insert(name.to_ascii_lowercase(), value.trim())
            .is_none());
    }
    let expected = "default-src 'none'; script-src 'self' 'sha256-X+zrZv/IbzjZUnhsbWlsecLbwjndTpG0ZynXOif7V+k='; style-src 'self' 'unsafe-inline'; img-src 'self'; font-src 'self'; connect-src 'self'; manifest-src 'self'; worker-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; object-src 'none'; upgrade-insecure-requests";
    for (name, value) in [
        ("content-security-policy", expected), ("cache-control", "no-cache"),
        ("content-type", "text/html; charset=utf-8"), ("content-length", "33"),
        ("strict-transport-security", "max-age=31536000"),
        ("x-content-type-options", "nosniff"), ("referrer-policy", "same-origin"),
        ("x-frame-options", "DENY"), ("cross-origin-opener-policy", "same-origin"),
        ("cross-origin-resource-policy", "same-origin"),
        ("permissions-policy", "camera=(), microphone=(), geolocation=(), payment=(), usb=(), serial=(), hid=(), bluetooth=()"),
        ("connection", "close"),
    ] { assert_eq!(headers.remove(name), Some(value), "{name}"); }
    headers.remove("date");
    assert!(headers.is_empty(), "unexpected headers {headers:?}");
}

#[test]
fn an_incomplete_keepalive_body_is_closed_after_its_ingress_deadline() {
    let server = Running::start(ApiLimits {
        body_timeout: Duration::from_millis(50),
        ..ApiLimits::default()
    });
    let mut stream = connect(server.address);
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write!(
        stream,
        "POST /api/v1/test/read HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nContent-Length: 10\r\n\r\n{{",
        address = server.address
    )
    .unwrap();
    let response = Wire::read(stream);
    assert!(response.status.starts_with("HTTP/1.1 503 "));
    assert_eq!(server.executed.load(Ordering::Acquire), 0);
}

fn ingress_exchange(
    server: &Running,
    method: &str,
    target: &str,
    headers: &str,
    body: &[u8],
) -> Wire {
    let mut stream = connect(server.address);
    write!(
        stream,
        "{method} {target} HTTP/1.1\r\n{headers}Connection: close\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .unwrap();
    stream.write_all(body).unwrap();
    Wire::read(stream)
}

#[test]
fn b106_duplicate_json_never_reaches_prepare() {
    let bodies = [
        r#"{"operation_id":"first","operation_id":"second"}"#,
        r#"{"scope":{"instance_id":"a","instance_id":"b"}}"#,
        r#"{"kind":"org","kind":"work"}"#,
        r#"{"params":{"name":"first","name":"second"}}"#,
        r#"{"params":{"profile":{"role":"first","role":"second"}}}"#,
        r#"{"params":[{"a":1,"\u0061":2}]}"#,
        r#"{"params":[{"nested":[{"secret":"PRIVATE_INPUT","secret":"OTHER"}]}]}"#,
    ];
    let server = Running::start(ApiLimits::default());
    let headers = format!(
        "Host: {}\r\nOrigin: http://{}\r\n",
        server.address, server.address
    );
    for body in bodies {
        let response = ingress_exchange(
            &server,
            "POST",
            "/api/v1/test/read",
            &headers,
            body.as_bytes(),
        );
        response.assert_error_headers(400);
        assert_eq!(response.body["error"]["code"], "invalid_request");
        assert!(!response.body.to_string().contains("PRIVATE_INPUT"));
        assert_eq!(server.prepared.load(Ordering::Acquire), 0);
        assert_eq!(server.executed.load(Ordering::Acquire), 0);
    }
    // Identical names in separate sibling objects are independent scopes.
    let valid = ingress_exchange(
        &server,
        "POST",
        "/api/v1/test/read",
        &headers,
        br#"{"params":[{"a":1},{"a":2}],"value":"a:a"}"#,
    );
    assert_eq!(valid.status, "HTTP/1.1 200 OK");
    assert_eq!(server.prepared.load(Ordering::Acquire), 1);
    assert_eq!(server.executed.load(Ordering::Acquire), 1);
}

#[test]
fn b106_wrong_host_or_origin_never_reaches_route() {
    let server = Running::start(ApiLimits::default());
    for (headers, status) in [
        (
            format!(
                "Host: attacker.test\r\nOrigin: http://{}\r\n",
                server.address
            ),
            400,
        ),
        (
            format!(
                "Host: {}\r\nOrigin: https://attacker.test\r\n",
                server.address
            ),
            403,
        ),
        (format!("Host: {}\r\nOrigin: null\r\n", server.address), 403),
        (format!("Host: {}\r\n", server.address), 403),
    ] {
        let response = ingress_exchange(&server, "POST", "/api/v1/test/read", &headers, b"{}");
        response.assert_error_headers(status);
        assert_eq!(server.routed.load(Ordering::Acquire), 0);
        assert_eq!(server.prepared.load(Ordering::Acquire), 0);
        assert_eq!(server.executed.load(Ordering::Acquire), 0);
    }
}

#[test]
fn b106_nesting_above_64_is_invalid_before_prepare() {
    let server = Running::start(ApiLimits::default());
    let headers = format!(
        "Host: {}\r\nOrigin: http://{}\r\n",
        server.address, server.address
    );
    let body = format!("{}0{}", "[".repeat(65), "]".repeat(65));
    let response = ingress_exchange(
        &server,
        "POST",
        "/api/v1/test/read",
        &headers,
        body.as_bytes(),
    );
    response.assert_error_headers(400);
    assert_eq!(server.prepared.load(Ordering::Acquire), 0);
    assert_eq!(server.executed.load(Ordering::Acquire), 0);
    let body = format!("{}0{}", "[".repeat(64), "]".repeat(64));
    let valid = ingress_exchange(
        &server,
        "POST",
        "/api/v1/test/read",
        &headers,
        body.as_bytes(),
    );
    assert_eq!(valid.status, "HTTP/1.1 200 OK");
    assert_eq!(server.executed.load(Ordering::Acquire), 1);
}
