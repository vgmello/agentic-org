//! Actual web image plus actual private host service/SQLCipher account registry.
//! The static HTML and empty office catalog are component fixtures, not setup,
//! selected adoption, production UI or release-signing acceptance.
use super::*;
use std::net::{SocketAddr, TcpStream};

fn config(port: u16) -> ListenConfig {
    serde_json::from_value(serde_json::json!({"address":"127.0.0.1","port":port,
        "scheme":"http_loopback","origin":format!("http://127.0.0.1:{port}"),"hsts":"off"}))
    .unwrap()
}
fn http(
    address: SocketAddr,
    method: &str,
    path: &str,
    headers: &str,
    body: &str,
) -> (String, serde_json::Value) {
    let mut stream = TcpStream::connect(address).unwrap();
    stream.set_read_timeout(Some(WAIT)).unwrap();
    write!(stream,"{method} {path} HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{headers}Connection: close\r\n\r\n{body}",body.len()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let (headers, body) = response.split_once("\r\n\r\n").expect("HTTP response");
    (
        headers.into(),
        serde_json::from_str(body).expect("JSON response"),
    )
}

#[test]
#[cfg(target_os = "macos")]
#[ignore = "root runs with ORG_TEST_GATEWAY_BIN pointing to the built web executable; child cannot bind"]
fn actual_gateway_uses_protected_host_sessions_rotation_logout_and_eof() {
    let binary = std::env::var_os("ORG_TEST_GATEWAY_BIN").expect("explicit disposable test binary");
    let root = tempfile::tempdir_in("/private/tmp").unwrap();
    let root = root.path().canonicalize().unwrap();
    let program = root.join("agentic-org-web");
    fs::copy(binary, &program).unwrap();
    fs::create_dir(root.join("ui")).unwrap();
    fs::write(
        root.join("ui/index.html"),
        b"<html><body>public component fixture<script>globalThis.componentFixture=true</script></body></html>",
    )
    .unwrap();
    let runtime = root.join("run");
    fs::create_dir(&runtime).unwrap();
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).unwrap();
    let reserved = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reserved.local_addr().unwrap();
    drop(reserved);
    let (account_root, auth, _unused) =
        crate::service::authentication::tests::fixture_for("01ARZ3NDEKTSV4RRFFQ69G5FAV");
    let supervisor = crate::supervisor::Supervisor::new(crate::supervisor::SupervisorConfig {
        program: root.join("absent-worker"),
        home: root.clone(),
        runtime_dir: Some(runtime.clone()),
        logs_dir: root.join("logs"),
        log_max_bytes: 1024,
        timings: crate::Timings::default(),
        keys: Arc::new(crate::keys::memory::MemoryKeys::default()),
        server_check: Arc::new(ServerCheck::Skip),
        sweep_orphans: false,
    });
    let broadcast = Arc::new(crate::broadcast::Broadcast::default());
    let offices = crate::offices::Offices::start(
        &root,
        crate::catalog::Catalog::default(),
        vec![],
        supervisor,
        Arc::clone(&broadcast),
        &crate::Timings::default(),
    );
    let service = Arc::new(
        crate::service::Service::new(
            Arc::clone(&offices),
            broadcast,
            Some("01ARZ3NDEKTSV4RRFFQ69G5FAV".into()),
        )
        .with_authentication(Some(Arc::new(auth))),
    );
    let mut child = Running::start(
        &Configuration {
            release: None,
            program,
            instance: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
            runtime,
            listen: config(address.port()),
            check: ServerCheck::Skip,
            critical_paths: vec![account_root.path().to_owned(), root.clone()],
        },
        service,
    )
    .expect("actual authenticated gateway admission");
    let until = Instant::now() + WAIT;
    // The listener is transferred before its runtime is initialized. Wait for
    // actual HTTP readiness without treating connection creation as success.
    loop {
        if !child.control.live() || child.process.try_wait().unwrap() {
            panic!("gateway exited");
        }
        let mut stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        write!(
            stream,
            "GET /api/v1/version HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut first = [0; 12];
        if stream.read_exact(&mut first).is_ok() && &first == b"HTTP/1.1 200" {
            break;
        }
        assert!(Instant::now() < until, "HTTP readiness deadline");
    }
    let (denied, _) = http(address, "GET", "/api/v1/session", "", "");
    assert!(denied.starts_with("HTTP/1.1 401"));
    let (signed, result) = http(
        address,
        "POST",
        "/api/v1/session/sign-in",
        "",
        r#"{"login":"OWNER","password":"synthetic test password"}"#,
    );
    assert!(signed.starts_with("HTTP/1.1 200"));
    assert!(signed
        .to_ascii_lowercase()
        .contains("cache-control: no-store"));
    let cookie = signed
        .lines()
        .find_map(|line| {
            line.strip_prefix("set-cookie: ")
                .or_else(|| line.strip_prefix("Set-Cookie: "))
        })
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    assert!(signed.contains("HttpOnly") && signed.contains("SameSite=Strict"));
    let proof = result["result"]["proof"].as_str().expect("proof");
    let headers = format!("Cookie: {cookie}\r\nX-Org-Session-Proof: {proof}\r\n");
    let (view, public) = http(address, "GET", "/api/v1/session", &headers, "");
    assert!(view.starts_with("HTTP/1.1 200"));
    // Actual administrator with no optional employee or organization profile.
    assert_eq!(public["result"]["admin"], true);
    let (refreshed, next) = http(address, "POST", "/api/v1/session/refresh", &headers, "{}");
    assert!(refreshed.starts_with("HTTP/1.1 200"));
    let (old, _) = http(address, "GET", "/api/v1/session", &headers, "");
    assert!(old.starts_with("HTTP/1.1 401"));
    let cookie = refreshed
        .lines()
        .find_map(|line| {
            line.strip_prefix("set-cookie: ")
                .or_else(|| line.strip_prefix("Set-Cookie: "))
        })
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let proof = next["result"]["proof"].as_str().expect("rotated proof");
    let headers = format!("Cookie: {cookie}\r\nX-Org-Session-Proof: {proof}\r\n");
    let (logout, _) = http(address, "POST", "/api/v1/session/sign-out", &headers, "{}");
    assert!(logout.starts_with("HTTP/1.1 200"));
    let (old, _) = http(address, "GET", "/api/v1/session", &headers, "");
    assert!(old.starts_with("HTTP/1.1 401"));
    let Running {
        control,
        mut process,
        _stdin,
    } = child;
    drop(control);
    let until = Instant::now() + WAIT;
    while !process.try_wait().unwrap() {
        assert!(Instant::now() < until, "gateway EOF exit");
        thread::sleep(Duration::from_millis(10));
    }
    drop(process);
    offices.shutdown();
}
