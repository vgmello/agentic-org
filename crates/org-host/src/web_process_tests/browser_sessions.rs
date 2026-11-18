//! A3 B5 step 6 production-path proof: the real host and the real built
//! `agentic-org-web` over loopback TCP. A registered instance with no account and
//! one registration link serves its web process before any organization; the
//! browser registers from the link and is signed in; the stored session survives a
//! web restart (SES-1) and a host restart (SES-2); sign-out ends it even while the
//! web process has it cached (SES-3); an unknown pair costs exactly one host call
//! and is then served from memory (SES-5). Every ingress refusal is answered
//! before any host op.
use super::{
    fixture_config, host_at, http, http_ready, launch, post_until_available, send, wait_until,
    web_pid, web_ready, Launched, Reply, Seed, PASSWORD,
};
use crate::Host;
use std::{net::SocketAddr, process::Command};

/// The browser's pair: the `name=value` cookie and the proof header value.
#[derive(Clone)]
pub(super) struct Pair {
    pub(super) cookie: String,
    pub(super) proof: String,
}
impl Pair {
    pub(super) fn headers(&self) -> String {
        format!(
            "Cookie: {}\r\nX-Org-Session-Proof: {}\r\n",
            self.cookie, self.proof
        )
    }
}

pub(super) fn set_cookie(reply: &Reply) -> Option<String> {
    reply.headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case("set-cookie")
            .then(|| value.trim().to_owned())
    })
}

/// The pair a successful sign-in or registration reply carries: its one cookie and the
/// proof in the body.
pub(super) fn pair(reply: &Reply, what: &str) -> Pair {
    assert_eq!(
        reply.status, 200,
        "{what}: {} {}",
        reply.headers, reply.body
    );
    let cookie = set_cookie(reply).expect("a session cookie");
    let cookie = cookie.split(';').next().unwrap().trim().to_owned();
    assert!(cookie.starts_with("org_session_"), "{cookie}");
    Pair {
        cookie,
        proof: reply.body["result"]["proof"]
            .as_str()
            .expect("proof")
            .to_owned(),
    }
}

/// A successful sign-in or registration before any organization: the pair, the account
/// signed in as the administrator.
pub(super) fn established(reply: &Reply, what: &str) -> Pair {
    let pair = pair(reply, what);
    let result = &reply.body["result"];
    assert_eq!(result["account"]["login"], "Owner", "{what}");
    assert_eq!(result["admin"], true, "{what}");
    assert_eq!(result["organizations"], serde_json::json!([]), "{what}");
    pair
}

pub(super) fn credentials() -> String {
    serde_json::json!({ "login": "Owner", "password": PASSWORD }).to_string()
}

fn signed_out(reply: &Reply, what: &str) {
    assert_eq!(reply.status, 401, "{what}: {}", reply.headers);
    assert_eq!(reply.body["error"]["code"], "not_authorized", "{what}");
}

pub(super) fn wait_serving(host: &Host, address: SocketAddr) {
    wait_until("web_ready", || web_ready(host));
    wait_until("HTTP readiness", || http_ready(address));
}

fn get_session(address: SocketAddr, pair: &Pair) -> Reply {
    http(address, "GET", "/api/v1/session", &pair.headers(), "")
}

#[test]
#[ignore = "root runs with ORG_TEST_OFFICE_BIN and ORG_TEST_WEB_BIN; actual web/worker listeners require permitted local bind"]
fn actual_browser_registers_from_a_link_and_its_session_survives_web_and_host_restarts() {
    let Launched {
        temp: _temp,
        address,
        program,
        host,
        socket: _,
        link,
        base,
    } = launch(Seed::RegistrationLink);
    let link = link.expect("a registration link");
    // No account and no organization: the web process still serves (A3 B5 ruling).
    assert!(host.offices.offices().is_empty());
    wait_serving(&host, address);
    signed_out(
        &http(address, "GET", "/api/v1/instance", "", ""),
        "instance without a pair",
    );

    // Registration from the link over real TCP signs the browser in.
    let register = |link: &str| {
        let body = serde_json::json!({ "link": link, "login": "Owner", "password": PASSWORD });
        post_until_available(address, "/api/v1/links/register", &body.to_string())
    };
    let pair = established(&register(&link), "register");
    // The link works once only, and a refusal sets no cookie.
    let again = register(&link);
    assert_ne!(again.status, 200, "a used link: {}", again.body);
    assert!(set_cookie(&again).is_none(), "no cookie on a refused link");

    // Registration cached the session: reads are served from memory (SES-5).
    let before = host.service.browser_ops();
    let session = get_session(address, &pair);
    assert_eq!(session.status, 200, "{}", session.body);
    assert_eq!(session.body["result"]["account"]["login"], "Owner");
    assert_eq!(session.body["result"]["admin"], true);
    let organizations = http(address, "GET", "/api/v1/organizations", &pair.headers(), "");
    assert_eq!(organizations.status, 200);
    assert_eq!(
        organizations.body["result"]["organizations"],
        serde_json::json!([])
    );
    assert_eq!(host.service.browser_ops(), before, "cached: no host call");
    // HEAD on a GET route: same status, no body.
    let head = http(address, "HEAD", "/api/v1/session", &pair.headers(), "");
    assert_eq!(head.status, 200, "{}", head.headers);
    assert_eq!(head.body, serde_json::Value::Null);
    let instance = http(address, "GET", "/api/v1/instance", &pair.headers(), "");
    assert_eq!(instance.status, 200, "{}", instance.body);
    assert_eq!(instance.body["result"]["name"], "default");
    assert_eq!(instance.body["result"]["web_status"], "ok");
    assert_eq!(instance.body["result"]["listen_scheme"], "http_loopback");
    assert!(instance.body["result"].get("tls").is_none(), "tls absent");

    // A cookie without its proof, or a proof without its cookie, is 401.
    for (headers, what) in [
        (
            format!("Cookie: {}\r\n", pair.cookie),
            "cookie without proof",
        ),
        (
            format!("X-Org-Session-Proof: {}\r\n", pair.proof),
            "proof without cookie",
        ),
    ] {
        signed_out(&http(address, "GET", "/api/v1/session", &headers, ""), what);
    }

    // SES-1: the web process restarts with an empty cache; the same pair works,
    // validated with exactly one host call and then served from memory (SES-5).
    let killed = web_pid(&program);
    assert!(Command::new("/bin/kill")
        .args(["-KILL", &killed])
        .status()
        .unwrap()
        .success());
    wait_until("web_ready false after EOF", || !web_ready(&host));
    wait_serving(&host, address);
    let before = host.service.browser_ops();
    assert_eq!(get_session(address, &pair).status, 200, "SES-1");
    assert_eq!(host.service.browser_ops(), before + 1, "one validate");
    assert_eq!(get_session(address, &pair).status, 200);
    assert_eq!(host.service.browser_ops(), before + 1, "then memory");

    // SES-2: the host restarts on the same registry; the same pair works.
    drop(host);
    let (host, _socket) = host_at(fixture_config(&base), &base, |_, _, _| {});
    wait_serving(&host, address);
    let restarted = get_session(address, &pair);
    assert_eq!(restarted.status, 200, "SES-2: {}", restarted.body);
    assert_eq!(restarted.body["result"]["account"]["login"], "Owner");

    // Sign-in with the registered credentials yields a second, independent pair.
    let signed_in = established(
        &post_until_available(address, "/api/v1/session/sign-in", &credentials()),
        "sign-in",
    );
    assert_ne!(signed_in.cookie, pair.cookie);
    let wrong = serde_json::json!({ "login": "Owner", "password": "not the password" });
    let refused = http(
        address,
        "POST",
        "/api/v1/session/sign-in",
        "",
        &wrong.to_string(),
    );
    assert_ne!(refused.status, 200);
    assert!(set_cookie(&refused).is_none(), "no cookie on a refusal");

    // SES-3: sign-out ends the cached session; its next request is 401 and the
    // other session is untouched.
    assert_eq!(get_session(address, &pair).status, 200, "cached");
    let out = http(
        address,
        "POST",
        "/api/v1/session/sign-out",
        &pair.headers(),
        "{}",
    );
    assert_eq!(out.status, 200, "{}", out.body);
    let cleared = set_cookie(&out).expect("a clearing cookie");
    assert!(
        cleared.to_ascii_lowercase().contains("max-age=0"),
        "{cleared}"
    );
    wait_until("SES-3 sign-out ends the cached session", || {
        get_session(address, &pair).status == 401
    });
    signed_out(&get_session(address, &pair), "SES-3");
    assert_eq!(
        get_session(address, &signed_in).status,
        200,
        "other session"
    );
}

#[test]
#[ignore = "root runs with ORG_TEST_OFFICE_BIN and ORG_TEST_WEB_BIN; actual web/worker listeners require permitted local bind"]
fn actual_web_ingress_refusals_reach_no_host_op() {
    let Launched {
        temp: _temp,
        address,
        host,
        ..
    } = launch(Seed::Administrator);
    wait_serving(&host, address);
    let body = credentials();
    let post = |extra: &str, content_type: &str, origin: &str, host_header: &str| {
        format!(
            "POST /api/v1/session/sign-in HTTP/1.1\r\nHost: {host_header}\r\n{origin}Content-Type: {content_type}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n{body}",
            body.len()
        )
    };
    let here = address.to_string();
    let origin = format!("Origin: http://{address}\r\n");
    let before = host.service.browser_ops();
    for (request, status, what) in [
        (
            format!("OPTIONS /api/v1/session HTTP/1.1\r\nHost: {address}\r\n{origin}Connection: close\r\n\r\n"),
            405,
            "OPTIONS",
        ),
        (
            post("Sec-Fetch-Site: cross-site\r\n", "application/json", &origin, &here),
            403,
            "cross-site",
        ),
        (
            post("", "application/json", "Origin: http://evil.test\r\n", &here),
            403,
            "foreign Origin",
        ),
        (post("", "application/json", "", &here), 403, "no Origin on POST"),
        (post("", "text/plain", &origin, &here), 415, "text/plain"),
        (
            post("", "application/x-www-form-urlencoded", &origin, &here),
            415,
            "form",
        ),
        (
            post("", "application/json", &origin, "evil.test"),
            400,
            "foreign Host",
        ),
        (
            format!("GET /api/v1/session HTTP/1.1\r\nHost: localhost:{}\r\nConnection: close\r\n\r\n", address.port()),
            400,
            "another name for the same port",
        ),
    ] {
        let reply = send(address, &request);
        assert_eq!(reply.status, status, "{what}: {}", reply.headers);
        let lower = reply.headers.to_ascii_lowercase();
        assert!(!lower.contains("set-cookie"), "{what}: no cookie");
        assert!(!lower.contains("access-control-"), "{what}: no CORS");
    }
    assert_eq!(host.service.browser_ops(), before, "no host op on refusal");
    // The same request, admitted, reaches the host once.
    let admitted = http(
        address,
        "POST",
        "/api/v1/session/sign-in",
        "",
        &credentials(),
    );
    assert_eq!(admitted.status, 200, "{}", admitted.body);
    assert_eq!(host.service.browser_ops(), before + 1);
}
