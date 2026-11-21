//! Real-process E2E proofs for ADR-0033 (`TASK-62`: default plain HTTP + opt-in HTTPS)
//! and ADR-0034 (`TASK-76`: default local HTTP authless onboarding + opt-in authentication).
//!
//! 1. `actual_local_hq_starts_http_authless_creates_org_then_enables_auth_and_enforces_401`:
//!    - Fresh local HTTP HQ starts with zero accounts and zero registration links.
//!    - `GET /api/v1/access` reports `auth_enabled: false`, `listen_scheme: "http_loopback"`.
//!    - Unauthenticated browser reads `GET /api/v1/session`, `GET /api/v1/instance`,
//!      creates an organization via `POST /api/v1/organizations`, and reads `GET /snapshot`.
//!    - Opt-in `POST /api/v1/instance/auth/enable` upgrades the local owner account, preserves
//!      the created organization, flips `auth_enabled: true`, and returns a valid browser pair.
//!    - Unauthenticated requests immediately fail `401`; the returned pair still owns the org.
//!    - After host restart, `auth_enabled: true` persists and unauthenticated requests stay `401`.
//!
//! 2. `actual_local_hq_enables_https_opt_in_from_http_and_restarts_serving_https`:
//!    - Local HTTP HQ serves over plain HTTP.
//!    - Opt-in `POST /api/v1/instance/tls` with `{"mode":"self_signed"}` persists a generated
//!      certificate/key and updates the instance's browser listener to `https` on the same port.
//!    - After host restart, the host serves real HTTPS with that exact persisted certificate
//!      (verified by a rustls client pinning the returned SHA-256 fingerprint).

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use rustls::pki_types::{pem::PemObject, CertificateDer, ServerName};
use sha2::Digest;

use super::{
    browser_sessions::{pair, wait_serving},
    create_body, fixture_config, host_at, http, launch, lookup, post_until_available, remote,
    wait_until, web_ready, Launched, Seed, PASSWORD,
};
use crate::web_process_tests::organizations::{restart, stop, wait_offices_active};

const HTTPS_WAIT: Duration = Duration::from_secs(60);

fn wait_org_completed(
    address: SocketAddr,
    headers: &str,
    instance_id: &str,
    operation_id: &str,
) -> serde_json::Value {
    let until = Instant::now() + Duration::from_secs(120);
    loop {
        let reply = lookup(address, headers, instance_id, operation_id);
        assert_eq!(reply.status, 200, "lookup: {}", reply.body);
        let outcome = &reply.body["result"];
        if outcome["outcome"] == "completed" {
            return outcome["result"].clone();
        }
        assert_eq!(
            outcome["outcome"], "accepted",
            "unexpected create outcome: {}",
            reply.body
        );
        assert!(Instant::now() < until, "timed out waiting for org creation");
        thread::sleep(Duration::from_millis(50));
    }
}

fn https_request(
    address: SocketAddr,
    trusted: &CertificateDer<'static>,
    method: &str,
    path: &str,
    body: &str,
) -> Result<(u16, serde_json::Value), String> {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(trusted.clone()).map_err(|e| e.to_string())?;
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let name = ServerName::from(address.ip());
    let conn = rustls::ClientConnection::new(Arc::new(config), name).map_err(|e| e.to_string())?;
    let stream = TcpStream::connect(address).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;
    let mut tls = rustls::StreamOwned::new(conn, stream);
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nOrigin: https://{address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    tls.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    tls.flush().map_err(|e| e.to_string())?;
    let mut response = String::new();
    match tls.read_to_string(&mut response) {
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof && !response.is_empty() => {}
        Err(err) => return Err(err.to_string()),
    }
    let (headers, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| format!("malformed HTTP response: {response}"))?;
    let status = headers
        .split(' ')
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| format!("missing status: {headers}"))?;
    Ok((
        status,
        serde_json::from_str(body).unwrap_or(serde_json::Value::Null),
    ))
}

fn https_get(
    address: SocketAddr,
    trusted: &CertificateDer<'static>,
    path: &str,
) -> Result<(u16, serde_json::Value), String> {
    https_request(address, trusted, "GET", path, "")
}

#[test]
#[ignore = "root runs with ORG_TEST_OFFICE_BIN and ORG_TEST_WEB_BIN; actual web/worker listeners require permitted local bind"]
fn actual_local_hq_starts_http_authless_creates_org_then_enables_auth_and_enforces_401() {
    let Launched {
        temp: _temp,
        address,
        program,
        host,
        socket: _,
        link: _,
        base,
    } = launch(Seed::LocalAuthless);
    let instance_id = host.instance.as_ref().unwrap().id().to_owned();
    wait_serving(&host, address);

    // 1. Public access bootstrap reports auth_enabled: false on plain loopback HTTP.
    let access = http(address, "GET", "/api/v1/access", "", "");
    assert_eq!(access.status, 200, "{}", access.body);
    assert_eq!(access.body["result"]["auth_enabled"], false);
    assert_eq!(access.body["result"]["listen_scheme"], "http_loopback");

    // 2. Unauthenticated browser immediately accesses session, instance, and creates an org.
    let session = http(address, "GET", "/api/v1/session", "", "");
    assert_eq!(session.status, 200, "{}", session.body);
    assert_eq!(session.body["result"]["admin"], true);

    let instance = http(address, "GET", "/api/v1/instance", "", "");
    assert_eq!(instance.status, 200, "{}", instance.body);
    assert_eq!(instance.body["result"]["auth_enabled"], false);
    assert_eq!(instance.body["result"]["listen_scheme"], "http_loopback");

    let op_id = crate::ids::uuid_v7().unwrap();
    let body = create_body(
        &instance_id,
        &op_id,
        "Local Studio",
        "local-studio",
        remote("local-studio"),
    );
    let created = http(address, "POST", "/api/v1/organizations", "", &body);
    assert!(
        matches!(created.status, 200 | 202),
        "authless create: {} {}",
        created.status,
        created.body
    );
    let org = wait_org_completed(address, "", &instance_id, &op_id);
    let org_id = org["id"].as_str().expect("org id").to_owned();
    wait_offices_active(&host, 1);

    // Unauthenticated browser reads live organization snapshot directly.
    let snap = http(
        address,
        "GET",
        &format!("/api/v1/organizations/{org_id}/snapshot"),
        "",
        "",
    );
    assert_eq!(snap.status, 200, "authless snapshot: {}", snap.body);
    assert!(
        snap.body["result"]["resume_cursor"]
            .as_str()
            .unwrap_or_default()
            .starts_with(&format!("org:{org_id}:")),
        "resume cursor: {}",
        snap.body
    );
    assert!(
        !snap.body["result"]["snapshot"]["teams"]
            .as_array()
            .unwrap()
            .is_empty(),
        "teams populated"
    );

    // 3. Opt-in enable authentication upgrades the local owner account and issues a browser pair.
    let enable_body = serde_json::json!({
        "login": "Owner",
        "password": PASSWORD
    })
    .to_string();
    let enabled = post_until_available(address, "/api/v1/instance/auth/enable", &enable_body);
    let browser_pair = pair(&enabled, "enable auth");
    assert_eq!(enabled.body["result"]["account"]["login"], "Owner");
    assert_eq!(enabled.body["result"]["admin"], true);

    // 4. Access immediately flips to auth_enabled: true; unauthenticated requests fail 401.
    let access_after = http(address, "GET", "/api/v1/access", "", "");
    assert_eq!(access_after.status, 200, "{}", access_after.body);
    assert_eq!(access_after.body["result"]["auth_enabled"], true);

    for path in [
        "/api/v1/session",
        "/api/v1/organizations",
        &format!("/api/v1/organizations/{org_id}/snapshot"),
    ] {
        let denied = http(address, "GET", path, "", "");
        assert_eq!(denied.status, 401, "{path}: {}", denied.body);
    }

    // 5. Returned session pair still owns the organization created before auth was enabled.
    let orgs = http(
        address,
        "GET",
        "/api/v1/organizations",
        &browser_pair.headers(),
        "",
    );
    assert_eq!(orgs.status, 200, "{}", orgs.body);
    assert_eq!(
        orgs.body["result"]["organizations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(orgs.body["result"]["organizations"][0]["id"], org_id);

    // 6. Restart host: auth_enabled: true persists and unauthenticated requests stay 401.
    stop(host, &program);
    let restarted = restart(&base, address);
    let access_restarted = http(address, "GET", "/api/v1/access", "", "");
    assert_eq!(access_restarted.status, 200);
    assert_eq!(access_restarted.body["result"]["auth_enabled"], true);
    assert_eq!(http(address, "GET", "/api/v1/session", "", "").status, 401);
    assert_eq!(
        http(
            address,
            "GET",
            "/api/v1/organizations",
            &browser_pair.headers(),
            ""
        )
        .status,
        200
    );
    stop(restarted, &program);
}

#[test]
#[ignore = "root runs with ORG_TEST_OFFICE_BIN and ORG_TEST_WEB_BIN; actual web/worker listeners require permitted local bind"]
fn actual_local_hq_enables_https_opt_in_from_http_and_restarts_serving_https() {
    let Launched {
        temp: _temp,
        address,
        program,
        host,
        socket: _,
        link: _,
        base,
    } = launch(Seed::LocalAuthless);
    wait_serving(&host, address);

    // 1. Starts over plain HTTP by default (ADR-0033).
    let access_before = http(address, "GET", "/api/v1/access", "", "");
    assert_eq!(access_before.status, 200, "{}", access_before.body);
    assert_eq!(
        access_before.body["result"]["listen_scheme"],
        "http_loopback"
    );

    // 2. Switching to HTTPS while authless is refused (409 Conflict) to prevent lockout on restart.
    let refused_authless = http(
        address,
        "POST",
        "/api/v1/instance/tls",
        "",
        r#"{"mode":"self_signed"}"#,
    );
    assert_eq!(
        refused_authless.status, 409,
        "must require auth before HTTPS: {}",
        refused_authless.body
    );

    // 3. Enable authentication first, then opt in to HTTPS with the authenticated admin session.
    let enable_body = serde_json::json!({
        "login": "Owner",
        "password": PASSWORD
    })
    .to_string();
    let enabled_auth = post_until_available(address, "/api/v1/instance/auth/enable", &enable_body);
    let admin_pair = pair(&enabled_auth, "enable auth before tls");

    let enabled = http(
        address,
        "POST",
        "/api/v1/instance/tls",
        &admin_pair.headers(),
        r#"{"mode":"self_signed"}"#,
    );
    assert_eq!(enabled.status, 200, "enable tls: {}", enabled.body);
    assert_eq!(enabled.body["result"]["scheme"], "https");
    assert_eq!(
        enabled.body["result"]["origin"],
        format!("https://{address}")
    );
    assert_eq!(enabled.body["result"]["restart_required"], true);
    let returned_fp = enabled.body["result"]["fingerprint"]
        .as_str()
        .expect("fingerprint")
        .to_owned();
    assert!(!returned_fp.is_empty());

    // 3. Stop and restart host: listener now binds and serves real HTTPS with that exact cert.
    stop(host, &program);
    let (restarted, _socket) = host_at(fixture_config(&base), &base, |_, _, _| {});
    wait_until("web_ready on https", || web_ready(&restarted));

    let (chain, _) = restarted
        .service
        .web_tls_for_test()
        .expect("restarted host has WebTls")
        .fetch()
        .expect("persisted TLS pair servable");
    let leaf = CertificateDer::pem_slice_iter(chain.as_bytes())
        .next()
        .unwrap()
        .unwrap();
    let stored_fp =
        crate::web_tls::Fingerprint(sha2::Sha256::digest(leaf.as_ref()).into()).to_string();
    assert_eq!(stored_fp, returned_fp);

    let deadline = Instant::now() + HTTPS_WAIT;
    let (status, body) = loop {
        match https_get(address, &leaf, "/api/v1/access") {
            Ok(reply) => break reply,
            Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            Err(err) => panic!("HTTPS GET /api/v1/access failed: {err}"),
        }
    };
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["result"]["listen_scheme"], "https");
    assert_eq!(body["result"]["auth_enabled"], true);

    let (unauth_status, _) = https_get(address, &leaf, "/api/v1/session").unwrap();
    assert_eq!(unauth_status, 401, "HTTPS never serves authless sessions");

    // 5. Administrator signs in over HTTPS using the credentials established before switching.
    let (sign_in_status, sign_in_body) = https_request(
        address,
        &leaf,
        "POST",
        "/api/v1/session/sign-in",
        &enable_body,
    )
    .unwrap();
    assert_eq!(sign_in_status, 200, "HTTPS sign-in: {sign_in_body}");
    assert_eq!(sign_in_body["result"]["account"]["login"], "Owner");

    stop(restarted, &program);
}
