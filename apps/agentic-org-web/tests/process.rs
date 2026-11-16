//! Actual executable/confinement/HTTP component tests with a scripted native
//! peer. These are not protected setup/adoption or full product browser evidence.
#![cfg(target_os = "macos")]
use org_host_control::{
    gateway::{GatewayFirstLine, GatewayHandoff, GatewayReadyForSecrets},
    listen::{Hsts, ListenOrigin},
    ops::{TlsFetchResult, HOST_SOCK_MARKER},
};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    os::{
        fd::AsFd,
        unix::net::{UnixDatagram, UnixListener, UnixStream},
    },
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
const INSTANCE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const WAIT: Duration = Duration::from_secs(10);
struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn receive<T: serde::de::DeserializeOwned>(reader: &mut impl BufRead) -> T {
    org_proto::frame::read_message(reader)
        .unwrap()
        .expect("native frame")
}
fn spawn(root: &Path, rendezvous: &Path) -> Running {
    fs::copy(
        env!("CARGO_BIN_EXE_agentic-org-web"),
        root.join("agentic-org-web"),
    )
    .unwrap();
    fs::create_dir(root.join("ui")).unwrap();
    // Bounded immutable HTML fixture: production UI acceptance uses the actual
    // Svelte build and protected host, separately from this process-boundary test.
    fs::write(
        root.join("ui/index.html"),
        b"<html><body>fixture<script>globalThis.fixture=true</script></body></html>",
    )
    .unwrap();
    let mut child = Running(
        Command::new(root.join("agentic-org-web"))
            .arg("--serve")
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    org_proto::frame::write_message(
        child.0.stdin.as_mut().unwrap(),
        &GatewayFirstLine {
            rendezvous: rendezvous.to_str().unwrap().into(),
            instance_id: INSTANCE.into(),
            release: None,
        },
    )
    .unwrap();
    child
}
fn accept(listener: &UnixListener, child: &mut Running) -> UnixStream {
    listener.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + WAIT;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                // macOS accepts inherit the listener's O_NONBLOCK. This peer
                // uses bounded blocking framing, unlike the gateway recvmsg side.
                stream.set_nonblocking(false).unwrap();
                stream.set_read_timeout(Some(WAIT)).unwrap();
                stream.set_write_timeout(Some(WAIT)).unwrap();
                return stream;
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                assert!(
                    child.0.try_wait().unwrap().is_none(),
                    "gateway exited before rendezvous"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("rendezvous: {error}"),
        }
    }
}
fn request(address: std::net::SocketAddr, path: &str) -> String {
    let mut stream = TcpStream::connect(address).unwrap();
    stream.set_read_timeout(Some(WAIT)).unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}
fn finish(child: &mut Running, success: bool) {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert_eq!(status.success(), success);
            return;
        }
        assert!(Instant::now() < deadline, "gateway did not terminate");
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn actual_gateway_confines_before_handoff_serves_inherited_http_and_ends_on_host_eof() {
    let temp = tempfile::tempdir_in("/private/tmp").unwrap();
    let root = temp.path();
    let path = root.join("r\"\\\né");
    let listener = UnixListener::bind(&path).unwrap();
    let browser = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = browser.local_addr().unwrap();
    let mut child = spawn(root, &path);
    let stream = accept(&listener, &mut child);
    let mut reader = BufReader::new(stream);
    let ready: GatewayReadyForSecrets = receive(&mut reader);
    assert_eq!(ready.landlock, None);
    assert!(reader.buffer().is_empty());
    org_socket::confinement::verify_gateway(
        child.0.id(),
        &[
            &root.join("private-host-data"),
            &root.join("private-office-data"),
        ],
    )
    .unwrap();
    let (transfer, gateway_transfer) = UnixDatagram::pair().unwrap();
    gateway_transfer.shutdown(Shutdown::Write).unwrap();
    transfer.shutdown(Shutdown::Read).unwrap();
    org_proto::frame::write_message(reader.get_mut(), &GatewayHandoff).unwrap();
    org_socket::rights::send(
        reader.get_ref().as_fd(),
        &[HOST_SOCK_MARKER],
        &[gateway_transfer.as_fd(), browser.as_fd()],
        Instant::now() + WAIT,
    )
    .unwrap();
    let ack: serde_json::Value = receive(&mut reader);
    assert_eq!(ack["event"], "fds.received");
    assert_eq!(ack["of"], "handoff");
    drop(gateway_transfer);
    let hello: serde_json::Value = receive(&mut reader);
    assert_eq!(hello["method"], "hello");
    let hello_result = org_proto::HelloResult::host("test", "temporary").with_instance_id(INSTANCE);
    org_proto::frame::write_message(
        reader.get_mut(),
        &serde_json::json!({"id":hello["id"],"result":hello_result}),
    )
    .unwrap();
    let fetch: serde_json::Value = receive(&mut reader);
    assert_eq!(fetch["method"], "tls.fetch");
    let config = TlsFetchResult::HttpLoopback {
        origin: ListenOrigin::parse(&format!("http://{address}")).unwrap(),
        hsts: Hsts::Off,
    };
    org_proto::frame::write_message(
        reader.get_mut(),
        &serde_json::json!({"id":fetch["id"],"result":config}),
    )
    .unwrap();
    let response = request(address, "/api/v1/version");
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "version response: {response}"
    );
    assert!(response.contains("contract_revision"));
    let response = request(address, "/");
    assert!(response.starts_with("HTTP/1.1 200"));
    assert!(response
        .to_ascii_lowercase()
        .contains("content-security-policy:"));
    let policy = response
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(name, _)| name.eq_ignore_ascii_case("content-security-policy"))
                .map(|(_, value)| value)
        })
        .expect("CSP header");
    let scripts = policy
        .split(';')
        .map(str::trim)
        .find(|directive| directive.starts_with("script-src "))
        .expect("script-src");
    // The maintained policy deliberately permits inline styles for Svelte.
    // Scripts must retain the exact startup hash, without inline/eval allowances.
    assert!(scripts.contains("'sha256-"));
    assert!(!scripts.contains("unsafe-inline"));
    assert!(!scripts.contains("unsafe-eval"));
    let response = request(address, "/api/v1/organizations/ORG/tasks?office_id=HQ");
    assert!(
        response.starts_with("HTTP/1.1 401"),
        "unauthenticated read: {response}"
    );
    reader.get_ref().shutdown(Shutdown::Both).unwrap();
    drop(reader);
    drop(transfer);
    finish(&mut child, true);
}

#[test]
fn actual_gateway_rejects_early_rights_without_ack_or_http_readiness() {
    let temp = tempfile::tempdir_in("/private/tmp").unwrap();
    let path = temp.path().join("r");
    let listener = UnixListener::bind(&path).unwrap();
    let mut child = spawn(temp.path(), &path);
    let stream = accept(&listener, &mut child);
    let mut reader = BufReader::new(stream);
    let _: GatewayReadyForSecrets = receive(&mut reader);
    let (extra, _peer) = UnixDatagram::pair().unwrap();
    org_socket::rights::send(
        reader.get_ref().as_fd(),
        b"{",
        &[extra.as_fd()],
        Instant::now() + WAIT,
    )
    .unwrap();
    finish(&mut child, false);
    let mut byte = [0];
    assert_eq!(reader.read(&mut byte).unwrap(), 0);
}
