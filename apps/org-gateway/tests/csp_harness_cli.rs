use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const GUARD: Duration = Duration::from_secs(30);
const CSP: &str = "default-src 'none'; script-src 'self' 'sha256-X+zrZv/IbzjZUnhsbWlsecLbwjndTpG0ZynXOif7V+k='; style-src 'self' 'unsafe-inline'; img-src 'self'; font-src 'self'; connect-src 'self'; manifest-src 'self'; worker-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; object-src 'none'";

struct Harness(Child);

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn build_harness() -> PathBuf {
    // The outer cargo test holds its build directory lock while this test runs.
    // Keep a distinct cache within the active profile, including custom target roots.
    let test_binary = std::env::current_exe().unwrap();
    let build_dir = test_binary
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("csp-harness-build");
    let build = Command::new(env!("CARGO"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env_remove("CARGO_MAKEFLAGS")
        .env_remove("MAKEFLAGS")
        .env_remove("MFLAGS")
        .args([
            "build",
            "--locked",
            "-p",
            "org-gateway",
            "--example",
            "csp_harness",
            "--message-format=json",
        ])
        .arg("--target-dir")
        .arg(build_dir)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    String::from_utf8(build.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find_map(|message| {
            if message["reason"] == "compiler-artifact"
                && message["target"]["name"] == "csp_harness"
            {
                message["executable"].as_str().map(PathBuf::from)
            } else {
                None
            }
        })
        .expect("Cargo reported no csp_harness example executable")
}

#[test]
fn example_cli_serves_the_reviewed_html_headers_on_its_configured_loopback_port() {
    let build = tempfile::tempdir().unwrap();
    let html = b"<!doctype html><script>0</script>";
    std::fs::write(build.path().join("index.html"), html).unwrap();
    let harness = build_harness();
    let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reservation.local_addr().unwrap();
    drop(reservation);
    let mut process = Harness(
        Command::new(&harness)
            .arg("--ui-build")
            .arg(build.path())
            .arg("--port")
            .arg(address.port().to_string())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap_or_else(|error| panic!("failed to start built csp_harness example: {error}")),
    );
    let stdout = process.0.stdout.take().unwrap();
    let (ready, published) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(stdout).read_line(&mut line).map(|_| line);
        let _ = ready.send(result);
    });
    assert_eq!(published.recv_timeout(GUARD).unwrap().unwrap(), "ready\n");
    let html_wire = get(address, "/");
    assert_response(
        &html_wire,
        "200 OK",
        CSP,
        "no-cache",
        "text/html; charset=utf-8",
        std::str::from_utf8(html).unwrap(),
    );
    let api_wire = get(address, "/api/v1/capabilities");
    assert_response(
        &api_wire,
        "503 Service Unavailable",
        "default-src 'none'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'",
        "no-store",
        "application/json; charset=utf-8",
        r#"{"error":{"code":"unavailable","message":"Service unavailable."}}"#,
    );
    // Rowan's B1-05 ruling: http truncates literal fragments before dispatch.
    // Pin the complete API refusal; the discarded bytes cannot select a static file.
    let raw_wire = get(address, "/api/v1/capabilities#/%2f/%zz/..");
    assert_response(
        &raw_wire,
        "503 Service Unavailable",
        "default-src 'none'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'",
        "no-store",
        "application/json; charset=utf-8",
        r#"{"error":{"code":"unavailable","message":"Service unavailable."}}"#,
    );
    assert_eq!(
        normalized_response(&raw_wire),
        normalized_response(&api_wire)
    );
}

fn normalized_response(wire: &str) -> (String, BTreeMap<String, String>, String) {
    let (head, body) = wire.split_once("\r\n\r\n").unwrap();
    let mut lines = head.split("\r\n");
    let status = lines.next().unwrap().to_owned();
    let mut headers = BTreeMap::new();
    for line in lines {
        let (key, value) = line.split_once(':').unwrap();
        headers.insert(key.to_ascii_lowercase(), value.trim().to_owned());
    }
    // Date measures each response's time; every policy and representation header stays.
    assert!(headers.remove("date").is_some());
    (status, headers, body.to_owned())
}

fn get(address: std::net::SocketAddr, path: &str) -> String {
    let mut peer = TcpStream::connect_timeout(&address, GUARD).unwrap();
    peer.set_read_timeout(Some(GUARD)).unwrap();
    write!(peer, "GET {path} HTTP/1.1\r\nHost: {address}\r\nSec-Fetch-Mode: navigate\r\nConnection: close\r\n\r\n").unwrap();
    let mut wire = String::new();
    peer.read_to_string(&mut wire).unwrap();
    wire
}

fn assert_response(
    wire: &str,
    status: &str,
    csp: &str,
    cache: &str,
    content_type: &str,
    expected_body: &str,
) {
    let (head, body) = wire.split_once("\r\n\r\n").unwrap();
    let mut lines = head.split("\r\n");
    assert_eq!(lines.next().unwrap(), format!("HTTP/1.1 {status}"));
    let mut headers = BTreeMap::new();
    for line in lines {
        let (name, value) = line.split_once(':').unwrap();
        assert!(headers
            .insert(name.to_ascii_lowercase(), value.trim())
            .is_none());
    }
    let length = expected_body.len().to_string();
    for (name, value) in [
        ("content-security-policy", csp), ("cache-control", cache),
        ("content-type", content_type), ("content-length", length.as_str()),
        ("x-content-type-options", "nosniff"), ("referrer-policy", "same-origin"),
        ("x-frame-options", "DENY"), ("cross-origin-opener-policy", "same-origin"),
        ("cross-origin-resource-policy", "same-origin"),
        ("permissions-policy", "camera=(), microphone=(), geolocation=(), payment=(), usb=(), serial=(), hid=(), bluetooth=()"),
    ] {
        assert_eq!(headers.remove(name), Some(value), "{name}");
    }
    headers.remove("date");
    assert_eq!(headers.remove("connection"), Some("close"));
    assert!(headers.is_empty(), "unexpected headers: {headers:?}");
    assert_eq!(body, expected_body);
}
