//! X7's production-path proof: a real host, started for a registered instance whose listener
//! is `https`, makes its own certificate at its first start, logs its fingerprint, and starts
//! the real built `agentic-org-web`, which serves the browser over TLS with that certificate
//! (checked here by a rustls client that trusts exactly the certificate the host stored). A
//! session cookie is `Secure`. `tls.set` from the administrator on `host.sock` installs a
//! supplied pair, and the running web process serves it after `tls.renew` without a restart.
//! A restart of the host keeps the pair, so its fingerprint is stable.
//!
//! It needs the built `agentic-org-web` and `agentic-org-office` beside this crate's tests,
//! where `cargo test --workspace` puts them. Only disposable roots are used.
use std::fs;
use std::io::{BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use org_host_control::ops::{AccountVerify, AccountVerifyParams, TlsSet, TlsSetParams};
use org_host_control::{HostOpResult, Password, TlsPrivateKey};
use org_proto::{frame, ApiError, Call, HelloParams, Request};
use rustls::pki_types::{pem::PemObject, CertificateDer, ServerName};
use sha2::Digest;

use crate::hostdb::{HostDb, HostDbInit};
use crate::service::authentication::hash_password;
use crate::web_process_tests::{fixture_config, host_at};
use crate::Host;

const WAIT: Duration = Duration::from_secs(60);
const PASSWORD: &str = "synthetic first password";

fn built_program(variable: &str, name: &str) -> PathBuf {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::test_programs::sibling_program(name))
}

/// A disposable root with copies of the real worker and web images and a UI fixture, and a
/// free loopback port.
fn root() -> (tempfile::TempDir, PathBuf, SocketAddr) {
    let temp =
        tempfile::tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap()).unwrap();
    let base = temp.path().canonicalize().unwrap();
    for name in ["temp", "keys", "bin"] {
        fs::create_dir(base.join(name)).unwrap();
        fs::set_permissions(base.join(name), fs::Permissions::from_mode(0o700)).unwrap();
    }
    fs::copy(
        built_program("ORG_TEST_OFFICE_BIN", "agentic-org-office"),
        base.join("bin/agentic-org-office"),
    )
    .unwrap();
    fs::copy(
        built_program("ORG_TEST_WEB_BIN", "agentic-org-web"),
        base.join("bin/agentic-org-web"),
    )
    .unwrap();
    fs::create_dir(base.join("bin/ui")).unwrap();
    fs::write(
        base.join("bin/ui/index.html"),
        b"<html><body>https fixture<script>/* fixture bootstrap */</script></body></html>",
    )
    .unwrap();
    let reserved = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reserved.local_addr().unwrap();
    drop(reserved);
    (temp, base, address)
}

fn listen(address: SocketAddr) -> org_host_control::listen::ListenConfig {
    serde_json::from_value(serde_json::json!({
        "address": "127.0.0.1", "port": address.port(), "scheme": "https",
        "origin": format!("https://{address}"), "hsts": "off"
    }))
    .unwrap()
}

/// The registered `https` host under `base`; the first start creates the registry with one
/// administrator. Returns the host, its `host.sock` and what it logged while it started.
fn start(base: &Path, address: SocketAddr, first: bool) -> (Host, PathBuf, Vec<String>) {
    let listen = listen(address);
    let ((host, socket), log) = crate::host_log::capture(|| {
        host_at(fixture_config(base), base, |instance, home, custody| {
            if !first {
                return;
            }
            instance.set_browser(&listen).unwrap();
            let now = super::web_tls::now().unwrap() * 1000;
            let db = HostDb::create(
                home,
                custody,
                &HostDbInit {
                    instance_id: instance.id().to_owned(),
                    clock_high_ms: now,
                },
            )
            .unwrap();
            let verifier = hash_password(&Password::new(PASSWORD.into()).unwrap()).unwrap();
            db.seed_account(
                &crate::ids::generate().unwrap(),
                "Owner",
                &verifier,
                true,
                now,
            )
            .unwrap();
        })
    });
    // As the registered start installs it: the administrator calls' admission (this test's
    // executable is the development `agentic-org` peer of `fixture_config`).
    let policy = crate::startup_app_policy(&fixture_config(base), true).unwrap();
    let db = host
        .service
        .authentication
        .as_ref()
        .expect("a registry")
        .provisioning_storage()
        .0;
    assert!(host
        .service
        .install_admin(Arc::new(crate::admin::Admin::new(
            db,
            &crate::ids::uuid_v7().unwrap(),
            listen.origin().clone(),
            policy,
            Arc::new(crate::admin::NativePresence),
        ))));
    (host, socket, log)
}

/// The chain the host stores and serves, as `tls.fetch` reads it.
fn stored_chain(host: &Host) -> String {
    host.service
        .web_tls_for_test()
        .expect("an https host has a pair")
        .fetch()
        .map(|(chain, _)| chain)
        .unwrap_or_else(|_| panic!("the stored pair is servable"))
}

fn leaf_of(chain: &str) -> CertificateDer<'static> {
    CertificateDer::pem_slice_iter(chain.as_bytes())
        .next()
        .unwrap()
        .unwrap()
}

fn fingerprint(der: &[u8]) -> String {
    super::web_tls::Fingerprint(sha2::Sha256::digest(der).into()).to_string()
}

/// One HTTPS request to `address`, trusting only `trusted` (a certificate, as a trust
/// anchor) and checking the name `127.0.0.1`. Returns the head and body, or the TLS or I/O
/// error.
fn https(
    address: SocketAddr,
    trusted: &CertificateDer<'static>,
    request: &str,
) -> std::io::Result<(String, String)> {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(trusted.clone()).map_err(std::io::Error::other)?;
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(std::io::Error::other)?
    .with_root_certificates(roots)
    .with_no_client_auth();
    let name = ServerName::try_from(address.ip().to_string()).unwrap();
    let mut connection =
        rustls::ClientConnection::new(Arc::new(config), name).map_err(std::io::Error::other)?;
    let mut socket = TcpStream::connect(address)?;
    socket.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut stream = rustls::Stream::new(&mut connection, &mut socket);
    stream.write_all(request.as_bytes())?;
    let mut response = Vec::new();
    match stream.read_to_end(&mut response) {
        Ok(_) => {}
        // A server that closes without close_notify after `Connection: close`.
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof && !response.is_empty() => {}
        Err(e) => return Err(e),
    }
    let response = String::from_utf8(response).map_err(std::io::Error::other)?;
    let (head, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| std::io::Error::other("no HTTP response"))?;
    Ok((head.to_owned(), body.to_owned()))
}

fn get(
    address: SocketAddr,
    trusted: &CertificateDer<'static>,
    path: &str,
) -> std::io::Result<(String, String)> {
    https(
        address,
        trusted,
        &format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"),
    )
}

/// The live processes executing exactly `program` (a unique temp path).
fn pids_of(program: &Path) -> Vec<String> {
    let out = std::process::Command::new("/usr/bin/pgrep")
        .arg("-f")
        .arg(program)
        .output()
        .unwrap();
    String::from_utf8(out.stdout)
        .unwrap()
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let until = Instant::now() + WAIT;
    while !done() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(50));
    }
}

fn answer<C: Call>(
    wire: &mut BufReader<UnixStream>,
    id: u64,
    params: &C::Params,
) -> Result<C::Result, ApiError> {
    frame::write_message(wire.get_mut(), &Request::new::<C>(id, params).unwrap()).unwrap();
    let response: org_proto::Response = frame::read_message(wire).unwrap().unwrap();
    assert_eq!(response.id, id);
    response.into_result::<C>().map_err(|error| match error {
        org_proto::CallError::Api(error) => error,
        _ => panic!("malformed typed response"),
    })
}

/// A pair for `127.0.0.1` made outside the host, as an operator's.
fn operator_pair() -> (String, String) {
    let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
    let mut params = rcgen::CertificateParams::new(vec!["127.0.0.1".to_owned()]).unwrap();
    let now = super::web_tls::now().unwrap();
    params.not_before = time::OffsetDateTime::from_unix_timestamp(now - 3600).unwrap();
    params.not_after = time::OffsetDateTime::from_unix_timestamp(now + 30 * 86_400).unwrap();
    let cert = params.self_signed(&key).unwrap();
    (cert.pem(), key.serialize_pem())
}

#[test]
fn a_real_host_serves_the_browser_over_tls_with_its_own_certificate_and_a_supplied_one() {
    let (temp, base, address) = root();
    let (host, socket, log) = start(&base, address, true);
    // The certificate the host made at its first start, and its fingerprint in the log.
    let chain = stored_chain(&host);
    let leaf = leaf_of(&chain);
    let shown = fingerprint(&leaf);
    let logged = log.join("\n");
    assert!(
        logged.contains(&format!(
            "made by this instance, SHA-256 fingerprint {shown}"
        )),
        "{logged}"
    );
    assert!(!logged.contains("PRIVATE KEY"), "{logged}");
    wait_until("the web process to serve TLS", || {
        host.web_ready() && get(address, &leaf, "/api/v1/version").is_ok()
    });
    let (head, body) = get(address, &leaf, "/api/v1/version").unwrap();
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert!(body.contains("\"result\""), "{body}");
    // Plain HTTP on the TLS port gets no HTTP answer.
    let mut plain = TcpStream::connect(address).unwrap();
    plain
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let _ = write!(
        plain,
        "GET /api/v1/version HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
    );
    let mut answered = Vec::new();
    let _ = plain.read_to_end(&mut answered);
    assert!(!answered.starts_with(b"HTTP/1.1 200"));
    // Another certificate is not trusted: the client checks the chain, not just TLS.
    let (other, _) = operator_pair();
    assert!(get(address, &leaf_of(&other), "/api/v1/version").is_err());
    // Sign-in over TLS sets a `Secure` session cookie.
    let credentials = serde_json::json!({"login": "Owner", "password": PASSWORD}).to_string();
    let sign_in = |trusted: &CertificateDer<'static>| {
        https(
            address,
            trusted,
            &format!(
                "POST /api/v1/session/sign-in HTTP/1.1\r\nHost: {address}\r\nOrigin: \
                 https://{address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n{credentials}",
                credentials.len()
            ),
        )
    };
    let mut signed = sign_in(&leaf).unwrap();
    for _ in 0..20 {
        if !signed.0.starts_with("HTTP/1.1 503") {
            break;
        }
        thread::sleep(Duration::from_millis(250));
        signed = sign_in(&leaf).unwrap();
    }
    assert!(signed.0.starts_with("HTTP/1.1 200"), "{}", signed.0);
    let cookie = signed
        .0
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("set-cookie:"))
        .expect("a session cookie")
        .to_owned();
    assert!(cookie.contains("; Secure"), "{cookie}");

    // `tls.set` from the administrator: the running web process serves the new pair, without
    // a restart (the same process).
    let web = pids_of(&base.join("bin/agentic-org-web"));
    assert_eq!(web.len(), 1, "one web process: {web:?}");
    let stream = UnixStream::connect(&socket).unwrap();
    stream.set_read_timeout(Some(WAIT)).unwrap();
    let mut wire = BufReader::new(stream);
    answer::<org_host_control::Hello>(&mut wire, 1, &HelloParams::default()).unwrap();
    let session = answer::<AccountVerify>(
        &mut wire,
        2,
        &AccountVerifyParams {
            login: "Owner".into(),
            password: Password::new(PASSWORD.into()).unwrap(),
        },
    )
    .unwrap()
    .session;
    let (cert, key) = operator_pair();
    let set = answer::<TlsSet>(
        &mut wire,
        3,
        &TlsSetParams {
            session,
            operation_id: crate::ids::generate().unwrap(),
            cert: cert.clone(),
            key: TlsPrivateKey::new(key).unwrap(),
        },
    )
    .unwrap();
    assert_eq!(
        set,
        HostOpResult::Completed {
            result: org_proto::Empty {}
        }
    );
    let supplied = leaf_of(&cert);
    wait_until("the web process to serve the supplied pair", || {
        get(address, &supplied, "/api/v1/version").is_ok()
    });
    assert!(get(address, &leaf, "/api/v1/version").is_err());
    assert_eq!(pids_of(&base.join("bin/agentic-org-web")), web);
    assert_eq!(leaf_of(&stored_chain(&host)), supplied);
    drop(wire);

    // A restart keeps the stored pair: its fingerprint is stable.
    drop(host);
    let (host, _, log) = start(&base, address, false);
    assert_eq!(leaf_of(&stored_chain(&host)), supplied);
    let logged = log.join("\n");
    assert!(
        logged.contains(&format!(
            "supplied by the operator, SHA-256 fingerprint {}",
            fingerprint(&supplied)
        )),
        "{logged}"
    );
    wait_until("the restarted web process to serve TLS", || {
        get(address, &supplied, "/api/v1/version").is_ok()
    });
    drop(host);
    drop(temp);
}

/// `agentic-org admin tls set` against a real host: where the command can identify the host's
/// program it signs the administrator in with the password asked at the terminal (once),
/// replaces the pair in the running web process and prints only the new fingerprint; a
/// pair for another name or a wrong password changes nothing. Where the host is known by its
/// user alone, it refuses before a password is asked or anything is sent.
#[test]
fn the_admin_tls_set_command_replaces_the_pair_where_it_may_send_the_password() {
    use crate::setup_flow::tls_command::{set, Target};
    let (temp, base, address) = root();
    let (host, socket, _) = start(&base, address, true);
    let made = leaf_of(&stored_chain(&host));
    let files = base.join("supplied");
    fs::create_dir(&files).unwrap();
    let write = |name: &str, text: &str| {
        let path = files.join(name);
        fs::write(&path, text).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        path
    };
    let (cert, key) = operator_pair();
    let (cert_path, key_path) = (write("cert.pem", &cert), write("key.pem", &key));
    let id = host.service.instance_id_for_test().unwrap().to_owned();
    let origin = listen(address).origin().clone();
    let target = Target {
        socket: &socket,
        instance_id: &id,
        origin: &origin,
        name: None,
    };
    let asked = std::cell::Cell::new(0);
    let typed = |password: &'static str| {
        let asked = &asked;
        move || {
            asked.set(asked.get() + 1);
            Ok(Password::new(password.into()).unwrap())
        }
    };
    let run = |cert: &Path, password: &'static str| {
        let mut out = Vec::new();
        let result = set(
            &target,
            cert,
            &key_path,
            "Owner",
            &mut typed(password),
            &mut out,
        );
        (result, String::from_utf8(out).unwrap())
    };
    let by_code = matches!(
        crate::test_programs::image_identity(),
        org_platform_host::admission::ImageIdentity::CodeSignature
    );
    if !by_code {
        let (result, out) = run(&cert_path, PASSWORD);
        let refusal = result.unwrap_err();
        assert!(refusal.contains("browser"), "{refusal} {out}");
        assert!(!refusal.contains("not running"), "{refusal}");
        assert_eq!(asked.get(), 0, "no password is asked for");
        assert_eq!(leaf_of(&stored_chain(&host)), made);
        drop(host);
        drop(temp);
        return;
    }
    // A pair for another name: refused from the files, before a password is asked.
    let (other, _) = {
        let key = rcgen::KeyPair::from_pem(&key).unwrap();
        let params = rcgen::CertificateParams::new(vec!["elsewhere.example".to_owned()]).unwrap();
        (params.self_signed(&key).unwrap().pem(), ())
    };
    let other_path = write("other.pem", &other);
    let (result, _) = run(&other_path, PASSWORD);
    assert!(result.unwrap_err().contains("tls_name_mismatch"));
    assert_eq!(asked.get(), 0);
    // A wrong password: refused by the host, nothing changed.
    let (result, _) = run(&cert_path, "not the password at all");
    assert!(result.unwrap_err().contains("not the administrator's"));
    assert_eq!(asked.get(), 1);
    assert_eq!(leaf_of(&stored_chain(&host)), made);
    // The administrator's password: replaced, the fingerprint printed, nothing of the key.
    let (result, out) = run(&cert_path, PASSWORD);
    result.unwrap();
    assert_eq!(asked.get(), 2);
    let supplied = leaf_of(&cert);
    assert!(out.contains(&fingerprint(&supplied)), "{out}");
    assert!(
        !out.contains("PRIVATE KEY") && !out.contains("-----BEGIN"),
        "{out}"
    );
    assert_eq!(leaf_of(&stored_chain(&host)), supplied);
    wait_until("the web process to serve the supplied pair", || {
        get(address, &supplied, "/api/v1/version").is_ok()
    });
    drop(host);
    drop(temp);
}
