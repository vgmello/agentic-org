//! Session routes against a scripted host socket: what reaches the host, what
//! the browser gets back, and when the cache spares a host call.
use super::*;
use crate::transport::PeerAddress;
use crate::{
    assets::BuildFiles,
    ingress::RequestAdmission,
    service::{ApiLimits, WebService},
    session::cookie::PROOF_HEADER,
    Hsts, StaticUi,
};
use http_body_util::{BodyExt, Full};
use hyper::Method;
use hyper::{body::Bytes, header, Request};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::{
    io,
    net::SocketAddr,
    os::unix::net::{UnixDatagram, UnixStream},
    sync::Mutex,
};

const ORIGIN: &str = "http://127.0.0.1:8080";
const HOST: &str = "127.0.0.1:8080";
const INSTANCE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const PEER: &str = "192.0.2.7:51000";

type Script = dyn Fn(&str, &Value) -> Vec<Value> + Send + Sync;

/// Every non-hello call the host received, as `(method, params)`.
#[derive(Clone, Default)]
struct Calls(Arc<Mutex<Vec<(String, Value)>>>);
impl Calls {
    fn methods(&self) -> Vec<String> {
        self.0.lock().unwrap().iter().map(|c| c.0.clone()).collect()
    }
    fn last(&self) -> Value {
        self.0.lock().unwrap().last().unwrap().1.clone()
    }
}

fn record(n: u64) -> Value {
    json!({
        "session_id": format!("S{n}"),
        "account": {"id": "A1", "login": "alex"},
        "admin": true,
        "orgs": [],
        "server_time_ms": 1_794_312_000_000u64,
        "idle_expires_at_ms": u64::MAX / 2,
    })
}

/// A host that ends sessions on `session.end` (event first, then the reply),
/// validates only sessions it created, and answers `script` for the rest.
fn default_script() -> Arc<Script> {
    let live: Arc<Mutex<Vec<(Value, Value)>>> = Arc::default();
    Arc::new(move |method, params| {
        let mut live = live.lock().unwrap();
        match method {
            "session.sign_in" | "link.register" => {
                live.push((
                    params["cookie_digest"].clone(),
                    params["proof_digest"].clone(),
                ));
                vec![json!({"result": record(live.len() as u64)})]
            }
            "session.validate" => {
                let key = (
                    params["cookie_digest"].clone(),
                    params["proof_digest"].clone(),
                );
                match live.iter().position(|k| *k == key) {
                    Some(i) => vec![json!({"result": record(i as u64 + 1)})],
                    None => vec![json!({"error":{"code":"not_authorized","message":"no"}})],
                }
            }
            "session.end" => {
                let n: usize = params["session_id"].as_str().unwrap()[1..].parse().unwrap();
                live[n - 1] = (Value::Null, Value::Null);
                vec![
                    json!({"event":"session.ended","seq":1,"data":{"session_id":params["session_id"]}}),
                    json!({"result":{}}),
                ]
            }
            "organization.list" => {
                vec![json!({"result": {"organizations": [], "creating": []}})]
            }
            "instance.view" => vec![json!({"result": {
                "name": "studio", "status": "service", "web_status": "running",
                "administrator": "present"
            }})],
            _ => vec![json!({"error":{"code":"unavailable","message":"no"}})],
        }
    })
}

fn gateway(script: Arc<Script>) -> (WebService<SessionApi>, Calls) {
    gateway_in(script, org_host_control::listen::ListenMode::HttpLoopback)
}

/// [`gateway`] for a listener of `mode` (the instance view's `listen_scheme`).
fn gateway_in(
    script: Arc<Script>,
    mode: org_host_control::listen::ListenMode,
) -> (WebService<SessionApi>, Calls) {
    let (web, peer) = UnixStream::pair().unwrap();
    let calls = Calls::default();
    let seen = calls.clone();
    std::thread::spawn(move || {
        let mut reader = io::BufReader::new(peer);
        while let Ok(Some(request)) = org_proto::frame::read_message::<Value>(&mut reader) {
            let method = request["method"].as_str().unwrap_or_default().to_owned();
            let frames = if method == "hello" {
                let result =
                    org_proto::HelloResult::host("test", "disposable").with_instance_id(INSTANCE);
                vec![json!({"result": result})]
            } else {
                seen.0
                    .lock()
                    .unwrap()
                    .push((method.clone(), request["params"].clone()));
                script(&method, &request["params"])
            };
            for mut frame in frames {
                if frame.get("event").is_none() {
                    frame["id"] = request["id"].clone();
                }
                if org_proto::frame::write_message(reader.get_mut(), &frame).is_err() {
                    return;
                }
            }
        }
    });
    let cache = Arc::new(SessionCache::default());
    let sink_cache = Arc::clone(&cache);
    let generation = Arc::new(std::sync::OnceLock::<u64>::new());
    let sink_generation = Arc::clone(&generation);
    let host = HostControl::from_handoff(
        web.into(),
        UnixDatagram::pair().unwrap().0.into(),
        INSTANCE.into(),
        Arc::new(|| panic!("unexpected ancillary failure")),
        // The sink the web process installs, not a copy of it.
        crate::process::session_events(sink_cache, sink_generation),
    )
    .unwrap();
    generation.set(host.generation()).unwrap();
    let origin = ListenOrigin::parse(ORIGIN).unwrap();
    let files =
        BuildFiles::from_files([("index.html".into(), b"<script>0</script>".to_vec())]).unwrap();
    let api = SessionApi::new(host, cache, &origin, mode);
    let gateway = WebService::new(
        StaticUi::new(files, origin.clone(), Hsts::Off),
        api,
        ApiLimits::default(),
        RequestAdmission::new(origin),
    )
    .unwrap();
    (gateway, calls)
}

/// The browser's cookie and proof after a sign-in or registration.
#[derive(Clone)]
struct Pair {
    cookie: String,
    proof: String,
}

struct Answer {
    status: StatusCode,
    headers: header::HeaderMap,
    body: Value,
}
impl Answer {
    fn pair(&self) -> Pair {
        let set = self.headers[header::SET_COOKIE].to_str().unwrap();
        Pair {
            cookie: set.split(';').next().unwrap().to_owned(),
            proof: self.body["result"]["proof"].as_str().unwrap().to_owned(),
        }
    }
}

fn send(
    gateway: &WebService<SessionApi>,
    method: Method,
    target: &str,
    body: &str,
    pair: Option<&Pair>,
    extra: &[(&str, &str)],
) -> Answer {
    let mut builder = Request::builder()
        .method(method.clone())
        .uri(target)
        .header("host", HOST);
    if method != Method::GET {
        builder = builder
            .header("origin", ORIGIN)
            .header("content-type", "application/json");
    }
    if let Some(pair) = pair {
        builder = builder
            .header(header::COOKIE, &pair.cookie)
            .header(PROOF_HEADER, &pair.proof);
    }
    for (name, value) in extra {
        builder = builder.header(*name, *value);
    }
    let mut request = builder
        .body(Full::new(Bytes::copy_from_slice(body.as_bytes())))
        .unwrap();
    request
        .extensions_mut()
        .insert(PeerAddress(PEER.parse::<SocketAddr>().unwrap()));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let response = runtime.block_on(gateway.respond(request));
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = runtime
        .block_on(response.into_body().collect())
        .unwrap()
        .to_bytes();
    Answer {
        status,
        headers,
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    }
}

fn sign_in(gateway: &WebService<SessionApi>) -> Answer {
    send(
        gateway,
        Method::POST,
        "/api/v1/session/sign-in",
        r#"{"login":"alex","password":"generated unit credential"}"#,
        None,
        &[("x-forwarded-for", "203.0.113.9")],
    )
}

#[test]
fn version_is_the_public_constant() {
    let (gateway, calls) = gateway(default_script());
    let answer = send(&gateway, Method::GET, "/api/v1/version", "", None, &[]);
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(
        answer.body,
        json!({"result":{"api_version":1,"contract_revision":org_proto::web::CONTRACT_REVISION}})
    );
    assert_eq!(answer.headers[header::CACHE_CONTROL], "no-store");
    assert!(answer.headers.get(header::SET_COOKIE).is_none());
    assert!(calls.methods().is_empty());
}

#[test]
fn known_routes_refuse_other_methods_queries_and_bodies_without_a_host_call() {
    let (gateway, calls) = gateway(default_script());
    for (method, target, body) in [
        (Method::POST, "/api/v1/version", "{}"),
        (Method::GET, "/api/v1/version?x=1", ""),
        (Method::GET, "/api/v1/session/sign-in", ""),
        (Method::POST, "/api/v1/session/sign-in?x=1", "{}"),
        (Method::POST, "/api/v1/session/sign-in", "{\"login\":\"a\"}"),
        (
            Method::POST,
            "/api/v1/session/sign-in",
            r#"{"login":"a","password":"generated unit credential","extra":1}"#,
        ),
        (Method::POST, "/api/v1/links/register", r#"{"login":"a"}"#),
        (Method::POST, "/api/v1/session", "{}"),
        (Method::POST, "/api/v1/instance", "{}"),
        (Method::GET, "/api/v1/instance?x=1", ""),
    ] {
        let answer = send(&gateway, method, target, body, None, &[]);
        assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{target} {body}");
        assert_eq!(answer.body["error"]["code"], "invalid_request", "{target}");
        assert!(answer.headers.get(header::SET_COOKIE).is_none(), "{target}");
    }
    assert!(calls.methods().is_empty());
}

#[test]
fn unsupported_api_routes_are_json_404() {
    let (gateway, calls) = gateway(default_script());
    for (method, target) in [
        (Method::POST, "/api/v1/session/refresh"),
        (Method::GET, "/api/v1/capabilities"),
        (Method::GET, "/api/v1/instances"),
        (Method::GET, "/api/v1/tasks"),
    ] {
        let answer = send(&gateway, method, target, "{}", None, &[]);
        assert_eq!(answer.status, StatusCode::NOT_FOUND, "{target}");
        assert_eq!(answer.body["error"]["code"], "not_found", "{target}");
        assert_eq!(answer.headers[header::CACHE_CONTROL], "no-store");
    }
    assert!(calls.methods().is_empty());
}

/// Sign-in mints the pair, sends only digests and the socket peer address (never a
/// forwarding header), returns the view with the proof and sets the cookie; the
/// session it created is cached, so later reads make no host call (SES-5).
#[test]
fn sign_in_sets_the_pair_and_caches_the_session() {
    let (gateway, calls) = gateway(default_script());
    let answer = sign_in(&gateway);
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    let sent = calls.last();
    assert_eq!(sent["client_address"], "192.0.2.7");
    assert_eq!(
        sent["origin"],
        json!({"scheme":"http","host":"127.0.0.1","port":8080})
    );
    let pair = answer.pair();
    let set = answer.headers[header::SET_COOKIE].to_str().unwrap();
    assert!(set.starts_with("org_session_01arz3ndektsv4rrffq69g5fav="));
    assert!(set.ends_with("; Path=/api/v1; HttpOnly; SameSite=Strict"));
    let wire = sent.to_string();
    assert!(!wire.contains(&pair.proof) && !wire.contains(pair.cookie.split_once('=').unwrap().1));
    let result = &answer.body["result"];
    assert_eq!(result["instance_id"], INSTANCE);
    assert_eq!(result["account"], json!({"id":"A1","login":"alex"}));
    assert_fresh(&result["server_time_ms"]);
    assert!(result["idle_expires_at"].as_str().unwrap().ends_with('Z'));
    assert!(result.get("session_id").is_none());

    for _ in 0..3 {
        let view = send(
            &gateway,
            Method::GET,
            "/api/v1/session",
            "",
            Some(&pair),
            &[],
        );
        assert_eq!(view.status, StatusCode::OK);
        assert!(view.body["result"].get("proof").is_none());
        assert_eq!(view.body["result"]["account"]["login"], "alex");
    }
    let orgs = send(
        &gateway,
        Method::GET,
        "/api/v1/organizations",
        "",
        Some(&pair),
        &[],
    );
    assert_eq!(orgs.status, StatusCode::OK);
    assert_eq!(
        orgs.body,
        json!({"result":{"organizations":[],"creating":[]}})
    );
    // The session came from the cache; the list is always the host's answer.
    assert_eq!(calls.methods(), ["session.sign_in", "organization.list"]);
}

/// `GET /instance` validates the pair (here from the cache), then asks the host's
/// `instance.view` and answers the public view: no host status internals, no TLS
/// field until TLS lands, the listen scheme from the configured origin.
#[test]
fn instance_view_needs_a_session_and_reports_the_public_view() {
    let (gateway, calls) = gateway(default_script());
    let pair = sign_in(&gateway).pair();
    let answer = send(
        &gateway,
        Method::GET,
        "/api/v1/instance",
        "",
        Some(&pair),
        &[],
    );
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    let result = &answer.body["result"];
    assert_eq!(result["instance_id"], INSTANCE);
    assert_eq!(result["name"], "studio");
    assert_eq!(result["web_status"], "ok");
    assert_eq!(result["listen_scheme"], "http_loopback");
    assert_eq!(result["server_version"], env!("CARGO_PKG_VERSION"));
    assert!(result["server_time_ms"].as_u64().unwrap() > 0);
    let mut keys: Vec<_> = result.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "instance_id",
            "listen_scheme",
            "name",
            "server_time_ms",
            "server_version",
            "web_status"
        ]
    );
    assert!(answer.headers.get(header::SET_COOKIE).is_none());
    assert_eq!(calls.methods(), ["session.sign_in", "instance.view"]);
    // Any host web state other than running is degraded; a host refusal is 503.
    let degraded: Arc<Script> = Arc::new(|method, _| match method {
        "session.validate" => vec![json!({"result": record(1)})],
        "instance.view" => vec![json!({"result": {
            "name": "studio", "status": "service", "web_status": "starting",
            "administrator": "present"
        }})],
        _ => vec![json!({"error":{"code":"unavailable","message":"no"}})],
    });
    let (other, _) = self::gateway(degraded);
    let answer = send(
        &other,
        Method::GET,
        "/api/v1/instance",
        "",
        Some(&pair),
        &[],
    );
    assert_eq!(
        answer.body["result"]["web_status"], "degraded",
        "{}",
        answer.body
    );
    let refusing: Arc<Script> = Arc::new(|method, _| match method {
        "session.validate" => vec![json!({"result": record(1)})],
        _ => vec![json!({"error":{"code":"unavailable","message":"no"}})],
    });
    let (other, _) = self::gateway(refusing);
    let answer = send(
        &other,
        Method::GET,
        "/api/v1/instance",
        "",
        Some(&pair),
        &[],
    );
    assert_eq!(answer.status, StatusCode::SERVICE_UNAVAILABLE);
}

/// The view names the listener's own mode, `http_published` included, and passes on the
/// host's word on an `https` certificate (`tls`); a plain HTTP listener's view has none.
#[test]
fn instance_view_reports_the_listen_mode_and_the_certificate_status() {
    use org_host_control::listen::ListenMode;
    let (first, _) = gateway(default_script());
    let pair = sign_in(&first).pair();
    for (mode, scheme, tls) in [
        (ListenMode::HttpPublished, "http_published", None),
        (ListenMode::Https, "https", Some("expiring")),
    ] {
        let script: Arc<Script> = Arc::new(move |method, _| match method {
            "session.validate" => vec![json!({"result": record(1)})],
            "instance.view" => {
                let mut view = json!({
                    "name": "studio", "status": "service", "web_status": "running",
                    "administrator": "present"
                });
                if let Some(tls) = tls {
                    view["tls"] = tls.into();
                }
                vec![json!({ "result": view })]
            }
            _ => vec![json!({"error":{"code":"unavailable","message":"no"}})],
        });
        let (gateway, _) = gateway_in(script, mode);
        let answer = send(
            &gateway,
            Method::GET,
            "/api/v1/instance",
            "",
            Some(&pair),
            &[],
        );
        assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
        let result = &answer.body["result"];
        assert_eq!(result["listen_scheme"], scheme);
        match tls {
            Some(tls) => assert_eq!(result["tls"], tls),
            None => assert!(result.get("tls").is_none(), "{result}"),
        }
    }
}

/// A pair this host doesn't know is 401 after one validate and is not cached.
#[test]
fn an_unknown_pair_is_401_after_one_validate() {
    let (first, _) = gateway(default_script());
    let pair = sign_in(&first).pair();
    let (gateway, calls) = gateway(default_script());
    for _ in 0..2 {
        let refused = send(
            &gateway,
            Method::GET,
            "/api/v1/session",
            "",
            Some(&pair),
            &[],
        );
        assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
        assert_eq!(refused.body["error"]["code"], "not_authorized");
        assert!(refused.headers.get(header::SET_COOKIE).is_none());
    }
    assert_eq!(calls.methods(), ["session.validate", "session.validate"]);
}

/// A pair the cache doesn't hold (a restarted web process, SES-1) is validated
/// once, then served from the cache on this connection's generation (SES-5).
#[test]
fn a_validated_pair_is_cached_for_later_requests() {
    let (first, _) = gateway(default_script());
    let pair = sign_in(&first).pair();
    let (gateway, calls) = gateway(Arc::new(|method, _| match method {
        "session.validate" => vec![json!({ "result": record(1) })],
        "instance.view" => vec![json!({"result": {
            "name": "studio", "status": "service", "web_status": "running",
            "administrator": "present"
        }})],
        _ => vec![json!({"error":{"code":"unavailable","message":"no"}})],
    }));
    for target in ["/api/v1/session", "/api/v1/instance", "/api/v1/session"] {
        let answer = send(&gateway, Method::GET, target, "", Some(&pair), &[]);
        assert_eq!(answer.status, StatusCode::OK, "{target}");
    }
    assert_eq!(calls.methods(), ["session.validate", "instance.view"]);
    let sent = calls.0.lock().unwrap()[0].1.clone();
    assert_eq!(
        sent["origin"],
        json!({"scheme":"http","host":"127.0.0.1","port":8080})
    );
}

/// HEAD, advertised in `Allow`, answers a GET route's status and headers with no
/// body, through the same pair check; a POST-only route stays `invalid_request`.
#[test]
fn head_answers_get_routes_without_a_body() {
    let (first, _) = gateway(default_script());
    let pair = sign_in(&first).pair();
    let (gateway, calls) = gateway(Arc::new(|method, _| match method {
        "session.validate" => vec![json!({ "result": record(1) })],
        _ => vec![json!({"error":{"code":"unavailable","message":"no"}})],
    }));
    let version = send(&gateway, Method::HEAD, "/api/v1/version", "", None, &[]);
    assert_eq!(version.status, StatusCode::OK);
    assert_eq!(version.body, Value::Null);
    let unpaired = send(&gateway, Method::HEAD, "/api/v1/session", "", None, &[]);
    assert_eq!(unpaired.status, StatusCode::UNAUTHORIZED);
    assert_eq!(unpaired.body, Value::Null);
    let session = send(
        &gateway,
        Method::HEAD,
        "/api/v1/session",
        "",
        Some(&pair),
        &[],
    );
    assert_eq!(session.status, StatusCode::OK);
    assert_eq!(session.body, Value::Null);
    assert!(session.headers.get(header::SET_COOKIE).is_none());
    let post_only = send(
        &gateway,
        Method::HEAD,
        "/api/v1/session/sign-in",
        "",
        None,
        &[],
    );
    assert_eq!(post_only.status, StatusCode::BAD_REQUEST);
    assert_eq!(calls.methods(), ["session.validate"]);
}

/// Registration redeems the link with the socket peer address and signs in.
#[test]
fn register_redeems_the_link_and_sets_the_pair() {
    let (gateway, calls) = gateway(default_script());
    let registered = send(
        &gateway,
        Method::POST,
        "/api/v1/links/register",
        r#"{"link":"generated-unit-link","login":"alex","password":"generated unit credential"}"#,
        None,
        &[("x-forwarded-for", "203.0.113.9")],
    );
    assert_eq!(registered.status, StatusCode::OK, "{}", registered.body);
    assert_eq!(calls.methods(), ["link.register"]);
    let sent = calls.last();
    assert_eq!(sent["client_address"], "192.0.2.7");
    assert_eq!(sent["link"], "generated-unit-link");
    let pair = registered.pair();
    let view = send(
        &gateway,
        Method::GET,
        "/api/v1/session",
        "",
        Some(&pair),
        &[],
    );
    assert_eq!(view.status, StatusCode::OK);
    assert_eq!(calls.methods(), ["link.register"]);
}

#[test]
fn protected_routes_without_the_full_pair_are_401_with_no_host_call() {
    let (gateway, calls) = gateway(default_script());
    let pair = sign_in(&gateway).pair();
    let cookie_only = [(header::COOKIE.as_str(), pair.cookie.as_str())];
    let proof_only = [(PROOF_HEADER, pair.proof.as_str())];
    let two_proofs = [
        (header::COOKIE.as_str(), pair.cookie.as_str()),
        (PROOF_HEADER, pair.proof.as_str()),
        (PROOF_HEADER, pair.proof.as_str()),
    ];
    for extra in [&cookie_only[..], &proof_only[..], &two_proofs[..], &[]] {
        for (method, target, body) in [
            (Method::GET, "/api/v1/session", ""),
            (Method::GET, "/api/v1/organizations", ""),
            (Method::GET, "/api/v1/instance", ""),
            (Method::POST, "/api/v1/session/sign-out", "{}"),
        ] {
            let answer = send(&gateway, method, target, body, None, extra);
            assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{target}");
            assert_eq!(answer.body["error"]["code"], "not_authorized");
        }
    }
    assert_eq!(calls.methods(), ["session.sign_in"]);
}

/// Sign-out ends the host session and clears the cookie; the host's
/// `session.ended` push drops the warm cache entry, so the pair is 401 (SES-3).
#[test]
fn sign_out_ends_the_session_and_drops_the_warm_cache_entry() {
    let (gateway, calls) = gateway(default_script());
    let pair = sign_in(&gateway).pair();
    assert_eq!(
        send(
            &gateway,
            Method::GET,
            "/api/v1/session",
            "",
            Some(&pair),
            &[]
        )
        .status,
        StatusCode::OK
    );
    let out = send(
        &gateway,
        Method::POST,
        "/api/v1/session/sign-out",
        "{}",
        Some(&pair),
        &[],
    );
    assert_eq!(out.status, StatusCode::OK);
    assert_eq!(out.body, json!({"result":{}}));
    let cleared = out.headers[header::SET_COOKIE].to_str().unwrap();
    assert!(cleared.starts_with("org_session_01arz3ndektsv4rrffq69g5fav=;"));
    assert!(cleared.contains("Max-Age=0"));
    let after = send(
        &gateway,
        Method::GET,
        "/api/v1/session",
        "",
        Some(&pair),
        &[],
    );
    assert_eq!(after.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        calls.methods(),
        ["session.sign_in", "session.end", "session.validate"]
    );
}

/// A reply's `server_time_ms` is this process's clock at reply time, not a stored
/// record's (the scripted record says 1_794_312_000_000, days before any test run).
fn assert_fresh(time: &Value) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let time = time.as_u64().unwrap();
    assert!(time <= now && now - time < 60_000, "{time} vs {now}");
}

/// An escaped password reaches the host as the text the browser typed.
#[test]
fn an_escaped_password_is_sent_as_typed() {
    let (gateway, calls) = gateway(default_script());
    let body = r#"{"login":"alex","password":"q\"uote\\ \u00e9"}"#;
    let answer = send(
        &gateway,
        Method::POST,
        "/api/v1/session/sign-in",
        body,
        None,
        &[],
    );
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    assert_eq!(calls.last()["password"], "q\"uote\\ é");
}

/// A cache hit answers with the time of the reply, not the validation it reuses.
#[test]
fn a_cached_session_reports_the_current_server_time() {
    let (gateway, calls) = gateway(default_script());
    let pair = sign_in(&gateway).pair();
    let view = send(
        &gateway,
        Method::GET,
        "/api/v1/session",
        "",
        Some(&pair),
        &[],
    );
    assert_eq!(view.status, StatusCode::OK);
    assert_eq!(calls.methods(), ["session.sign_in"]);
    assert_fresh(&view.body["result"]["server_time_ms"]);
}

/// Host refusals keep their web class and only `code`, `field`, `rule` and
/// `retry_after_ms`; the message is the web process's own, never the host's.
#[test]
fn host_refusals_map_to_their_http_class_and_set_no_cookie() {
    for (error, status, expected) in [
        (
            json!({"code":"invalid_request","message":"host rule text","field":"login","rule":"taken"}),
            StatusCode::BAD_REQUEST,
            json!({"code":"invalid_request","message":"This doesn't meet the sign-in rules.","field":"login","rule":"taken"}),
        ),
        (
            json!({"code":"rate_limited","message":"host rate text","retry_after_ms":1500}),
            StatusCode::TOO_MANY_REQUESTS,
            json!({"code":"rate_limited","message":"Too many attempts. Try again later.","retry_after_ms":1500}),
        ),
        // A pair this build can't name stays a rule refusal (`unknown`, forward
        // compatible) and still never carries the host's names or message.
        (
            json!({"code":"invalid_request","message":"invalid origin"}),
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"code":"unavailable","message":"Service unavailable."}),
        ),
        (
            json!({"code":"invalid_request","message":"the params don't fit this method","field":"origin","rule":"shape"}),
            StatusCode::BAD_REQUEST,
            json!({"code":"invalid_request","message":"This doesn't meet the sign-in rules.","field":"unknown","rule":"unknown"}),
        ),
        // A web-to-host contract fault: `invalid_request` without a rule pair is 503.
        (
            json!({"code":"invalid_request","message":"the params don't fit this method","field":"login"}),
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"code":"unavailable","message":"Service unavailable."}),
        ),
        (
            json!({"code":"not_authorized","message":"no"}),
            StatusCode::UNAUTHORIZED,
            json!({"code":"not_authorized"}),
        ),
        (
            json!({"code":"not_found","message":"no link"}),
            StatusCode::NOT_FOUND,
            json!({"code":"not_found"}),
        ),
        (
            json!({"code":"storage_error","message":"/private/var/secret path"}),
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"code":"unavailable"}),
        ),
    ] {
        let reply = error.clone();
        let (gateway, _) = gateway(Arc::new(move |_, _| vec![json!({ "error": reply })]));
        for target in ["/api/v1/session/sign-in", "/api/v1/links/register"] {
            let body = if target.ends_with("sign-in") {
                r#"{"login":"alex","password":"generated unit credential"}"#
            } else {
                r#"{"link":"generated-unit-link","login":"alex","password":"generated unit credential"}"#
            };
            let answer = send(&gateway, Method::POST, target, body, None, &[]);
            assert_eq!(answer.status, status, "{error} {target}");
            for (key, value) in expected.as_object().unwrap() {
                assert_eq!(&answer.body["error"][key], value, "{error} {target}");
            }
            let text = answer.body.to_string();
            for host_text in ["/private", "host rule", "host rate", "origin", "params"] {
                assert!(!text.contains(host_text), "{text}");
            }
            let fields = answer.body["error"].as_object().unwrap();
            assert!(
                fields
                    .keys()
                    .all(|k| expected.get(k).is_some() || k == "message"),
                "{text}"
            );
            assert!(answer.headers.get(header::SET_COOKIE).is_none());
        }
    }
}

#[test]
fn sign_in_without_a_socket_peer_fails_closed() {
    let (gateway, calls) = gateway(default_script());
    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/session/sign-in")
        .header("host", HOST)
        .header("origin", ORIGIN)
        .header("content-type", "application/json")
        .header("x-forwarded-for", "203.0.113.9")
        .body(Full::new(Bytes::from_static(
            br#"{"login":"alex","password":"generated unit credential"}"#,
        )))
        .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let response = runtime.block_on(gateway.respond(request));
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(calls.methods().is_empty());
}

#[test]
fn the_web_process_sends_only_browser_session_ops() {
    // Source-level guard: the host connection carries hello, tls.fetch, the
    // serving notification and the browser-session ops; no office channel.
    for (name, source) in [
        ("api.rs", include_str!("../api.rs")),
        ("host.rs", include_str!("../host.rs")),
        ("ports.rs", include_str!("../ports.rs")),
        ("process.rs", include_str!("../process.rs")),
        ("local_unix.rs", include_str!("../transport/local_unix.rs")),
    ] {
        for forbidden in [
            ["office", ".open_channel"].concat(),
            ["Office", "OpenChannel"].concat(),
            ["selected", "_tasks"].concat(),
            ["org", "_client"].concat(),
            ["account", ".verify"].concat(),
            ["session", ".renew"].concat(),
            ["X-Forwarded", "-For"].concat(),
            ["x-forwarded", "-for"].concat(),
        ] {
            assert!(!source.contains(&forbidden), "{name} contains {forbidden}");
        }
    }
}

const OPERATION: &str = "01a1c554-5200-7000-8000-000000000006";
const ORG: &str = "0193a5c0-0000-7000-8000-000000000001";

fn create_body(operation: &str, repository: Value) -> String {
    json!({
        "operation_id": operation,
        "scope": {"kind": "instance", "instance_id": INSTANCE},
        "params": {"name": "Acme", "slug": "acme", "organization_repository": repository,
            "state_branch": "org/state"},
    })
    .to_string()
}
fn remote() -> Value {
    json!({"kind": "remote", "remote_url": "https://example.test/acme.git"})
}
fn host_organization() -> Value {
    json!({
        "id": ORG, "name": "Acme", "slug": "acme", "created_at": "2026-11-10T12:00:00.000Z",
        "employee_id": "0193a5c0-0000-7000-8000-000000000006",
        "offices": [{"id": "0193a5c0-0000-7000-8000-000000000002", "organization_id": ORG,
            "name": "HQ", "role": "hq", "host": "This Mac", "status": "active",
            "endpoint": "/private/office.sock"}],
        "recovery": "pending",
    })
}
/// The default host, with `answers` deciding the organization calls.
fn organization_script(
    answers: impl Fn(&str, &Value) -> Option<Vec<Value>> + Send + Sync + 'static,
) -> Arc<Script> {
    let base = default_script();
    Arc::new(move |method, params| answers(method, params).unwrap_or_else(|| base(method, params)))
}
fn create(gateway: &WebService<SessionApi>, pair: &Pair, body: &str) -> Answer {
    send(
        gateway,
        Method::POST,
        "/api/v1/organizations",
        body,
        Some(pair),
        &[],
    )
}

#[test]
fn create_is_accepted_then_completed_and_sends_the_plan_with_the_pair_digests() {
    let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = Arc::clone(&asked);
    let (gateway, calls) = gateway(organization_script(move |method, _| {
        (method == "organization.create").then(|| {
            if count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                vec![json!({"result": {"status": "accepted"}})]
            } else {
                vec![json!({"result": {"status": "completed", "result": host_organization()}})]
            }
        })
    }));
    let pair = sign_in(&gateway).pair();
    let body = create_body(OPERATION, remote());
    let accepted = create(&gateway, &pair, &body);
    assert_eq!(accepted.status, StatusCode::ACCEPTED);
    assert_eq!(
        accepted.body,
        json!({"outcome": "accepted", "operation_id": OPERATION})
    );
    let sent = calls.last();
    assert_eq!(sent["operation_id"], OPERATION);
    assert_eq!(
        sent["plan"],
        json!({"version": 1, "name": "Acme", "slug": "acme",
            "repository_remote_url": "https://example.test/acme.git", "state_branch": "org/state"})
    );
    assert_eq!(sent["auth"]["origin"]["port"], 8080);
    assert!(sent["auth"]["cookie_digest"].is_string() && sent["auth"]["proof_digest"].is_string());
    assert!(
        !sent.to_string().contains(&pair.proof),
        "only digests leave"
    );
    // The same request again: the lost-response replay.
    let completed = create(&gateway, &pair, &body);
    assert_eq!(completed.status, StatusCode::OK);
    assert_eq!(completed.body["outcome"], "completed");
    assert_eq!(completed.body["result"]["slug"], "acme");
    assert_eq!(completed.body["result"]["recovery"], "pending");
    assert_eq!(completed.body["result"]["offices"][0]["role"], "hq");
    assert!(!completed.body.to_string().contains("endpoint"));
}

#[test]
fn a_create_that_breaks_a_rule_is_400_with_its_field_before_any_host_call() {
    let (gateway, calls) = gateway(default_script());
    let pair = sign_in(&gateway).pair();
    let before = calls.methods();
    for (body, field, rule) in [
        (
            create_body("01M72N8MG00000000000000007", remote()),
            "operation_id",
            "format",
        ),
        (
            create_body(OPERATION, json!({"kind": "local"})),
            "organization_repository",
            "remote_required",
        ),
        (
            create_body(
                OPERATION,
                json!({"kind": "remote", "remote_url": "file:///tmp/acme.git"}),
            ),
            "organization_repository",
            "remote_required",
        ),
        (
            create_body(
                OPERATION,
                json!({"kind": "remote", "remote_url": "https://example.test/a.git",
                    "credential_ref": {"id": "deploy"}}),
            ),
            "organization_repository",
            "unsupported",
        ),
    ] {
        let refused = create(&gateway, &pair, &body);
        assert_eq!(refused.status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(refused.body["error"]["code"], "invalid_request");
        assert_eq!(refused.body["error"]["field"], field, "{body}");
        assert_eq!(refused.body["error"]["rule"], rule, "{body}");
    }
    // Another instance's scope, a team field, and no pair at all.
    let mut other: Value = serde_json::from_str(&create_body(OPERATION, remote())).unwrap();
    other["scope"]["instance_id"] = json!("01ARZ3NDEKTSV4RRFFQ69G5FAW");
    assert_eq!(
        create(&gateway, &pair, &other.to_string()).status,
        StatusCode::BAD_REQUEST
    );
    let mut team: Value = serde_json::from_str(&create_body(OPERATION, remote())).unwrap();
    team["params"]["home_team_name"] = json!("Core");
    assert_eq!(
        create(&gateway, &pair, &team.to_string()).status,
        StatusCode::BAD_REQUEST
    );
    let mut unnamed: Value = serde_json::from_str(&create_body(OPERATION, remote())).unwrap();
    unnamed.as_object_mut().unwrap().remove("operation_id");
    let unnamed = create(&gateway, &pair, &unnamed.to_string());
    assert_eq!(unnamed.status, StatusCode::BAD_REQUEST);
    assert_eq!(unnamed.body["error"]["field"], "operation_id");
    let signed_out = send(
        &gateway,
        Method::POST,
        "/api/v1/organizations",
        &create_body(OPERATION, remote()),
        None,
        &[],
    );
    assert_eq!(signed_out.status, StatusCode::UNAUTHORIZED);
    assert_eq!(calls.methods(), before, "no host call for any of them");
}

#[test]
fn host_refusals_of_a_create_keep_their_class_and_a_lost_answer_is_unknown() {
    for (error, status, outcome, code) in [
        (
            json!({"code": "already_exists", "message": "host words", "field": "name", "rule": "taken"}),
            StatusCode::CONFLICT,
            "rejected",
            "already_exists",
        ),
        (
            json!({"code": "operation_reused", "message": "host words"}),
            StatusCode::CONFLICT,
            "rejected",
            "operation_reused",
        ),
        (
            json!({"code": "not_authorized", "message": "host words"}),
            StatusCode::FORBIDDEN,
            "rejected",
            "not_authorized",
        ),
        (
            json!({"code": "unavailable", "message": "host words"}),
            StatusCode::GATEWAY_TIMEOUT,
            "unknown",
            "",
        ),
        (
            json!({"code": "rate_limited", "message": "host words", "retry_after_ms": 5}),
            StatusCode::GATEWAY_TIMEOUT,
            "unknown",
            "",
        ),
    ] {
        let answer = error.clone();
        let (gateway, _) = gateway(organization_script(move |method, _| {
            (method == "organization.create").then(|| vec![json!({"error": answer})])
        }));
        let pair = sign_in(&gateway).pair();
        let reply = create(&gateway, &pair, &create_body(OPERATION, remote()));
        assert_eq!(reply.status, status, "{error}");
        assert_eq!(reply.body["outcome"], outcome, "{error}");
        assert_eq!(reply.body["operation_id"], OPERATION);
        if !code.is_empty() {
            assert_eq!(reply.body["error"]["code"], code);
        }
        if code == "already_exists" {
            assert_eq!(reply.body["error"]["field"], "name");
            assert_eq!(reply.body["error"]["rule"], "taken");
        }
        assert!(!reply.body.to_string().contains("host words"));
    }
}

/// The host says `expired` only about an id it has never seen, so the answer is a plain
/// 410 error: there is no operation for an outcome to be about.
#[test]
fn a_create_whose_new_id_is_too_old_is_a_plain_410_and_never_an_outcome() {
    let (gateway, _) = gateway(organization_script(|method, _| {
        (method == "organization.create")
            .then(|| vec![json!({"error": {"code": "expired", "message": "host words"}})])
    }));
    let pair = sign_in(&gateway).pair();
    let reply = create(&gateway, &pair, &create_body(OPERATION, remote()));
    assert_eq!(reply.status, StatusCode::GONE);
    assert_eq!(reply.body["error"]["code"], "expired");
    assert!(reply.body.get("outcome").is_none(), "{}", reply.body);
    assert!(reply.body.get("operation_id").is_none(), "{}", reply.body);
    assert!(!reply.body.to_string().contains("host words"));
}

#[test]
fn an_account_that_is_not_the_administrator_is_403_from_host_authority() {
    let (gateway, calls) = gateway(organization_script(|method, _| match method {
        "session.sign_in" | "session.validate" => {
            let mut session = record(1);
            session["admin"] = json!(false);
            Some(vec![json!({"result": session})])
        }
        "organization.create" => Some(vec![json!({
            "error": {"code": "not_authorized", "message": "administrator required"}
        })]),
        _ => None,
    }));
    let pair = sign_in(&gateway).pair();
    let reply = create(&gateway, &pair, &create_body(OPERATION, remote()));
    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    assert_eq!(reply.body["outcome"], "rejected");
    assert_eq!(reply.body["error"]["code"], "not_authorized");
    assert!(calls.methods().contains(&"organization.create".to_owned()));
}

/// A `session.validate` answer the host wrote before `account.changed` is applied before
/// it: the reader caches the answer and then the event drops it, so a session that
/// predates the link is never served from the cache afterwards.
#[test]
fn a_session_validated_before_account_changed_is_not_cached_after_it() {
    let (lists, validates) = (
        Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    );
    let changed = || json!({"event": "account.changed", "seq": 1, "data": {"account_id": "A1"}});
    let listed = || json!({"result": {"organizations": [], "creating": []}});
    let base = default_script();
    let (gateway, calls) = gateway(Arc::new(move |method: &str, params: &Value| {
        use std::sync::atomic::Ordering::SeqCst;
        match method {
            // The first list drops the session the sign-in cached; later ones say nothing.
            "organization.list" if lists.fetch_add(1, SeqCst) == 0 => vec![changed(), listed()],
            "organization.list" => vec![listed()],
            // The first validation is answered, then the account changes.
            "session.validate" if validates.fetch_add(1, SeqCst) == 0 => {
                let mut frames = base(method, params);
                frames.push(changed());
                frames
            }
            _ => base(method, params),
        }
    }));
    let pair = sign_in(&gateway).pair();
    let get = |target: &str| send(&gateway, Method::GET, target, "", Some(&pair), &[]);
    let validations = || {
        calls
            .methods()
            .iter()
            .filter(|m| *m == "session.validate")
            .count()
    };
    assert_eq!(get("/api/v1/organizations").status, StatusCode::OK);
    assert_eq!(validations(), 0, "the sign-in was cached");
    assert_eq!(get("/api/v1/session").status, StatusCode::OK);
    assert_eq!(validations(), 1, "validated: its answer precedes the event");
    // A host answer written after the event and asked for without this pair: once it is
    // read, the reader has applied the event.
    assert_eq!(sign_in(&gateway).status, StatusCode::OK);
    assert_eq!(get("/api/v1/session").status, StatusCode::OK);
    assert_eq!(
        validations(),
        2,
        "the stale session was served from the cache"
    );
    // That validation had no event after it: it is current and stays cached.
    assert_eq!(get("/api/v1/session").status, StatusCode::OK);
    assert_eq!(validations(), 2);
}

#[test]
fn the_list_is_the_hosts_answer_and_account_changed_drops_the_cached_session() {
    let (gateway, calls) = gateway(organization_script(|method, _| {
        (method == "organization.list").then(|| {
            vec![
                json!({"event": "account.changed", "seq": 1, "data": {"account_id": "A1"}}),
                json!({"result": {"organizations": [host_organization()],
                    "creating": [{"operation_id": OPERATION, "name": "Other", "slug": "other"}]}}),
            ]
        })
    }));
    let pair = sign_in(&gateway).pair();
    let validations = |calls: &Calls| {
        calls
            .methods()
            .iter()
            .filter(|m| *m == "session.validate")
            .count()
    };
    let session = |gateway: &WebService<SessionApi>| {
        send(
            gateway,
            Method::GET,
            "/api/v1/session",
            "",
            Some(&pair),
            &[],
        )
    };
    assert_eq!(session(&gateway).status, StatusCode::OK);
    assert_eq!(validations(&calls), 0, "the sign-in was cached");
    let listed = send(
        &gateway,
        Method::GET,
        "/api/v1/organizations",
        "",
        Some(&pair),
        &[],
    );
    assert_eq!(listed.status, StatusCode::OK);
    let result = &listed.body["result"];
    assert_eq!(result["organizations"][0]["id"], ORG);
    assert_eq!(result["organizations"][0]["offices"][0]["name"], "HQ");
    assert_eq!(result["creating"][0]["operation_id"], OPERATION);
    assert!(!listed.body.to_string().contains("endpoint"));
    assert!(calls.last()["auth"]["cookie_digest"].is_string());
    // The event came before the list's answer: the next request validates again.
    assert_eq!(session(&gateway).status, StatusCode::OK);
    assert_eq!(validations(&calls), 1, "the cached session was dropped");
    assert_eq!(session(&gateway).status, StatusCode::OK);
    assert_eq!(validations(&calls), 1, "and the fresh one is cached");
}

#[test]
fn a_host_operation_lookup_checks_its_id_and_scope_before_asking_the_host() {
    let (gateway, calls) = gateway(organization_script(|method, params| {
        (method == "operation.lookup").then(|| {
            if params["operation_id"] == OPERATION {
                vec![json!({"result": {"status": "accepted"}})]
            } else if params["operation_id"] == "01a1c554-5200-7000-8000-000000000008" {
                vec![json!({"result": {"status": "completed", "result": host_organization()}})]
            } else if params["operation_id"] == "01a1c554-5200-7000-8000-000000000009" {
                vec![json!({"error": {"code": "expired", "message": "host words"}})]
            } else {
                vec![json!({"error": {"code": "not_found", "message": "no"}})]
            }
        })
    }));
    let pair = sign_in(&gateway).pair();
    let get = |target: &str| send(&gateway, Method::GET, target, "", Some(&pair), &[]);
    let scoped = format!("/api/v1/operations/host/{OPERATION}?instance_id={INSTANCE}");
    let found = get(&scoped);
    assert_eq!(found.status, StatusCode::OK);
    assert_eq!(
        found.body["result"],
        json!({"operation_id": OPERATION, "kind": "host", "state": "reserved",
            "outcome": "accepted"})
    );
    assert_eq!(calls.last()["operation_id"], OPERATION);
    // HEAD reads the same operation and carries no body.
    let asked = calls.methods().len();
    let head = send(&gateway, Method::HEAD, &scoped, "", Some(&pair), &[]);
    assert_eq!(head.status, StatusCode::OK);
    assert_eq!(head.body, Value::Null);
    assert_eq!(calls.methods().len(), asked + 1, "HEAD asked the host");
    assert_eq!(calls.last()["operation_id"], OPERATION);
    let before = calls.methods();
    for target in [
        format!("/api/v1/operations/host/{OPERATION}"),
        format!("/api/v1/operations/host/{OPERATION}?instance_id=01ARZ3NDEKTSV4RRFFQ69G5FAW"),
        format!("/api/v1/operations/host/{OPERATION}?instance_id={INSTANCE}&org_id={ORG}"),
        format!("/api/v1/operations/host/{OPERATION}?instance_id=%30"),
    ] {
        assert_eq!(get(&target).status, StatusCode::BAD_REQUEST, "{target}");
    }
    let unshaped = get(&format!(
        "/api/v1/operations/host/01M72N8MG00000000000000006?instance_id={INSTANCE}"
    ));
    assert_eq!(unshaped.status, StatusCode::BAD_REQUEST);
    assert_eq!(unshaped.body["error"]["field"], "operation_id");
    assert_eq!(
        get(&format!(
            "/api/v1/operations/org/{OPERATION}?instance_id={INSTANCE}"
        ))
        .status,
        StatusCode::BAD_REQUEST
    );
    for bad_org_lookup in [
        format!("/api/v1/operations/org/{OPERATION}?instance_id={INSTANCE}&org_id={ORG}&office_id=01M72N8MG00000000000000005"),
        format!("/api/v1/operations/org/{OPERATION}?instance_id=01ARZ3NDEKTSV4RRFFQ69G5FAW&org_id={ORG}"),
        format!("/api/v1/operations/org/01M72N8MG00000000000000006?instance_id={INSTANCE}&org_id={ORG}"),
    ] {
        assert_eq!(get(&bad_org_lookup).status, StatusCode::BAD_REQUEST, "{bad_org_lookup}");
    }
    assert_eq!(
        get(&format!(
            "/api/v1/operations/work/{OPERATION}?instance_id={INSTANCE}"
        ))
        .status,
        StatusCode::NOT_FOUND
    );
    // Live project mutation routes validate envelopes before touching the host (B3).
    for (project_route, body) in [
        (format!("/api/v1/organizations/{ORG}/projects"), "{}"),
        (format!("/api/v1/organizations/{ORG}/projects"), "[]"),
        (
            format!("/api/v1/organizations/{ORG}/projects/add-team"),
            "{}",
        ),
        (
            format!("/api/v1/organizations/{ORG}/projects/remove-team"),
            "{}",
        ),
    ] {
        let reply = send(
            &gateway,
            Method::POST,
            &project_route,
            body,
            Some(&pair),
            &[],
        );
        assert_eq!(
            reply.status,
            StatusCode::BAD_REQUEST,
            "{project_route} {body}"
        );
        assert!(
            reply.body["error"].get("field").is_none(),
            "malformed envelope must not leak field rule: {}",
            reply.body
        );
    }
    // Project GET routes reject unexpected query strings and reserved action segments with 400.
    for bad_project_get in [
        format!("/api/v1/organizations/{ORG}/projects?foo=bar"),
        format!("/api/v1/organizations/{ORG}/projects/P-1?foo=bar"),
        format!("/api/v1/organizations/{ORG}/projects/add-team"),
        format!("/api/v1/organizations/{ORG}/projects/remove-team"),
    ] {
        assert_eq!(
            get(&bad_project_get).status,
            StatusCode::BAD_REQUEST,
            "{bad_project_get}"
        );
    }
    assert_eq!(calls.methods(), before, "none of them reached the host");
    let other = "01a1c554-5200-7000-8000-000000000007";
    let missing = get(&format!(
        "/api/v1/operations/host/{other}?instance_id={INSTANCE}"
    ));
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    let linked = get(&format!(
        "/api/v1/operations/host/01a1c554-5200-7000-8000-000000000008?instance_id={INSTANCE}"
    ));
    assert_eq!(linked.status, StatusCode::OK);
    assert_eq!(linked.body["result"]["state"], "linked");
    assert_eq!(linked.body["result"]["outcome"], "completed");
    assert_eq!(linked.body["result"]["result"]["slug"], "acme");
    // A refused lookup is an error, never a `rejected` create.
    let expired = get(&format!(
        "/api/v1/operations/host/01a1c554-5200-7000-8000-000000000009?instance_id={INSTANCE}"
    ));
    assert_eq!(expired.status, StatusCode::GONE);
    assert_eq!(expired.body["error"]["code"], "expired");
    assert!(expired.body.get("result").is_none());
    assert!(!expired.body.to_string().contains("host words"));
}

#[test]
fn authenticated_org_mutation_not_authorized_is_403_and_pair_checked_before_org_id() {
    let op_id = OperationId::parse(OPERATION.into()).unwrap();
    // Critical regression check: authenticated org mutation permission refusal MUST be 403,
    // never 401 (which would trigger browser sign-out).
    let forbidden = super::mutation(
        MutationOutcome::<org_proto::Empty>::Rejected {
            operation_id: op_id.clone(),
            error: WebError::new(
                WebErrorCode::NotAuthorized,
                "You are not authorized to perform this action.",
            ),
        },
        false,
    );
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
    // `accepted_allowed = false` must refuse `Accepted` (202) with 503.
    let disallowed_accepted = super::mutation(
        MutationOutcome::<org_proto::Empty>::Accepted {
            operation_id: op_id,
        },
        false,
    );
    assert_eq!(
        disallowed_accepted.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );

    // Unauthenticated requests to `/api/v1/organizations/{org}/*` return 401 before org ID shape.
    let (gateway, calls) = gateway(organization_script(|_, _| None));
    let before = calls.methods();
    for (method, path) in [
        (Method::GET, "/api/v1/organizations/bad!org/snapshot"),
        (Method::POST, "/api/v1/organizations/bad!org/teams/create"),
        (
            Method::POST,
            "/api/v1/organizations/bad!org/projects/add-team",
        ),
    ] {
        let reply = send(&gateway, method, path, "{}", None, &[]);
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{path}");
    }
    assert_eq!(
        calls.methods(),
        before,
        "unauthenticated calls never hit host"
    );
}

#[test]
fn org_offices_list_and_restart_enforce_scope_auth_and_error_mapping() {
    const OFFICE: &str = "0193a5c0-0000-7000-8000-000000000002";
    const OTHER_OFFICE: &str = "0193a5c0-0000-7000-8000-00000000000b";
    const FOREIGN_ORG: &str = "0193a5c0-0000-7000-8000-0000000000ff";

    let restart_mode = std::sync::Arc::new(std::sync::Mutex::new("completed"));
    let session_valid = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let mode_for_script = restart_mode.clone();
    let valid_for_script = session_valid.clone();

    let (gateway, calls) = gateway(organization_script(move |method, params| match method {
        "session.validate" if !valid_for_script.load(std::sync::atomic::Ordering::Acquire) => {
            Some(vec![
                json!({"error": {"code": "not_authorized", "message": "revoked"}}),
            ])
        }
        "organization.list" => Some(vec![json!({
            "result": {
                "organizations": [host_organization()],
                "creating": []
            }
        })]),
        "browser.office_restart" => {
            assert_eq!(params["org_id"], ORG);
            let reply = match *mode_for_script.lock().unwrap() {
                "completed" => json!({"result": {"status": "completed", "result": {}}}),
                "conflict" => json!({"error": {"code": "conflict", "message": "active"}}),
                "not_ready" => json!({"error": {"code": "not_ready", "message": "starting"}}),
                "reused" => json!({"error": {"code": "operation_reused", "message": "reused"}}),
                "expired" => json!({"error": {"code": "expired", "message": "old"}}),
                "not_found" => json!({"error": {"code": "not_found", "message": "missing"}}),
                "denied" => json!({"error": {"code": "not_authorized", "message": "not owner"}}),
                _ => json!({"error": {"code": "unavailable", "message": "down"}}),
            };
            Some(vec![reply])
        }
        _ => None,
    }));

    // Unauthenticated requests fail 401 before touching the host.
    let offices_path = format!("/api/v1/organizations/{ORG}/offices");
    let restart_path = format!("/api/v1/organizations/{ORG}/offices/{OFFICE}/restart");
    assert_eq!(
        send(&gateway, Method::GET, &offices_path, "", None, &[]).status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(&gateway, Method::POST, &restart_path, "{}", None, &[]).status,
        StatusCode::UNAUTHORIZED
    );
    assert!(calls.methods().is_empty());

    let pair = sign_in(&gateway).pair();

    // GET /api/v1/organizations/{org}/offices returns public OfficeView list (no endpoint).
    let listed = send(&gateway, Method::GET, &offices_path, "", Some(&pair), &[]);
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    let offices = listed.body["result"]["offices"].as_array().unwrap();
    assert_eq!(offices.len(), 1);
    assert_eq!(offices[0]["id"], OFFICE);
    assert_eq!(offices[0]["status"], "active");
    assert!(offices[0].get("endpoint").is_none());

    // Foreign organization returns 404.
    let missing_org = send(
        &gateway,
        Method::GET,
        &format!("/api/v1/organizations/{FOREIGN_ORG}/offices"),
        "",
        Some(&pair),
        &[],
    );
    assert_eq!(missing_org.status, StatusCode::NOT_FOUND);

    // Invalid operation_id or mismatched scope fails 400 before host call.
    let calls_before = calls.methods().len();
    let bad_op = send(
        &gateway,
        Method::POST,
        &restart_path,
        &json!({
            "operation_id": "01M72N8MG00000000000000006",
            "scope": {"kind": "office", "instance_id": INSTANCE, "org_id": ORG, "office_id": OFFICE},
            "params": {}
        })
        .to_string(),
        Some(&pair),
        &[],
    );
    assert_eq!(bad_op.status, StatusCode::BAD_REQUEST);
    assert_eq!(bad_op.body["error"]["field"], "operation_id");

    let bad_scope = send(
        &gateway,
        Method::POST,
        &restart_path,
        &json!({
            "operation_id": OPERATION,
            "scope": {"kind": "office", "instance_id": INSTANCE, "org_id": ORG, "office_id": OTHER_OFFICE},
            "params": {}
        })
        .to_string(),
        Some(&pair),
        &[],
    );
    assert_eq!(bad_scope.status, StatusCode::BAD_REQUEST);
    assert_eq!(calls.methods().len(), calls_before);

    let restart_body = json!({
        "operation_id": OPERATION,
        "scope": {"kind": "office", "instance_id": INSTANCE, "org_id": ORG, "office_id": OFFICE},
        "params": {}
    })
    .to_string();

    // Completed restart -> 200.
    let ok = send(
        &gateway,
        Method::POST,
        &restart_path,
        &restart_body,
        Some(&pair),
        &[],
    );
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.body);
    assert_eq!(ok.body["outcome"], "completed");
    assert_eq!(ok.body["operation_id"], OPERATION);

    // Conflict (already active) -> 409 conflict.
    *restart_mode.lock().unwrap() = "conflict";
    let conflict = send(
        &gateway,
        Method::POST,
        &restart_path,
        &restart_body,
        Some(&pair),
        &[],
    );
    assert_eq!(conflict.status, StatusCode::CONFLICT, "{}", conflict.body);
    assert_eq!(conflict.body["outcome"], "rejected");
    assert_eq!(conflict.body["error"]["code"], "conflict");

    // NotReady -> 503 not_ready.
    *restart_mode.lock().unwrap() = "not_ready";
    let not_ready = send(
        &gateway,
        Method::POST,
        &restart_path,
        &restart_body,
        Some(&pair),
        &[],
    );
    assert_eq!(
        not_ready.status,
        StatusCode::SERVICE_UNAVAILABLE,
        "{}",
        not_ready.body
    );
    assert_eq!(not_ready.body["error"]["code"], "not_ready");

    // OperationReused -> 409 operation_reused.
    *restart_mode.lock().unwrap() = "reused";
    let reused = send(
        &gateway,
        Method::POST,
        &restart_path,
        &restart_body,
        Some(&pair),
        &[],
    );
    assert_eq!(reused.status, StatusCode::CONFLICT, "{}", reused.body);
    assert_eq!(reused.body["error"]["code"], "operation_reused");

    // Expired -> 410 expired (plain error envelope, not rejected outcome).
    *restart_mode.lock().unwrap() = "expired";
    let expired = send(
        &gateway,
        Method::POST,
        &restart_path,
        &restart_body,
        Some(&pair),
        &[],
    );
    assert_eq!(expired.status, StatusCode::GONE, "{}", expired.body);
    assert_eq!(expired.body["error"]["code"], "expired");
    assert!(expired.body.get("outcome").is_none());

    // Missing office -> 404 not_found.
    *restart_mode.lock().unwrap() = "not_found";
    let missing = send(
        &gateway,
        Method::POST,
        &restart_path,
        &restart_body,
        Some(&pair),
        &[],
    );
    assert_eq!(missing.status, StatusCode::NOT_FOUND, "{}", missing.body);
    assert_eq!(missing.body["error"]["code"], "not_found");

    // Non-owner with valid session -> 403 not_authorized (NOT 401!).
    *restart_mode.lock().unwrap() = "denied";
    let forbidden = send(
        &gateway,
        Method::POST,
        &restart_path,
        &restart_body,
        Some(&pair),
        &[],
    );
    assert_eq!(
        forbidden.status,
        StatusCode::FORBIDDEN,
        "{}",
        forbidden.body
    );
    assert_eq!(forbidden.body["outcome"], "rejected");
    assert_eq!(forbidden.body["error"]["code"], "not_authorized");

    // Revoked session -> revalidation fails -> 401 signed_out.
    session_valid.store(false, std::sync::atomic::Ordering::Release);
    let revoked = send(
        &gateway,
        Method::POST,
        &restart_path,
        &restart_body,
        Some(&pair),
        &[],
    );
    assert_eq!(revoked.status, StatusCode::UNAUTHORIZED, "{}", revoked.body);
}

#[test]
fn cli_approval_decide_requires_browser_session_and_maps_host_decisions() {
    let mode = std::sync::Arc::new(std::sync::Mutex::new("approved"));
    let mode_for_script = mode.clone();
    let (gateway, calls) = gateway(organization_script(move |method, params| {
        (method == "browser.cli_approval_decide").then(|| {
            assert!(params["auth"]["cookie_digest"].is_string());
            assert!(params["link"].is_string());
            match *mode_for_script.lock().unwrap() {
                "approved" => vec![json!({"result": {"status": "approved"}})],
                "declined" => vec![json!({"result": {"status": "declined"}})],
                "not_found" => vec![json!({"error": {"code": "not_found", "message": "gone"}})],
                _ => vec![json!({"error": {"code": "not_authorized", "message": "expired"}})],
            }
        })
    }));

    // Unauthenticated fails 401 before host call.
    let unauth = send(
        &gateway,
        Method::POST,
        "/api/v1/cli-approvals/decide",
        r#"{"link":"tok","approve":true}"#,
        None,
        &[],
    );
    assert_eq!(unauth.status, StatusCode::UNAUTHORIZED);
    assert!(calls.methods().is_empty());

    let pair = sign_in(&gateway).pair();

    // Malformed body or empty link fails 400 before host call.
    let before = calls.methods().len();
    for bad in [
        "{}",
        r#"{"link":"","approve":true}"#,
        r#"{"link":"tok","approve":true,"extra":1}"#,
    ] {
        let reply = send(
            &gateway,
            Method::POST,
            "/api/v1/cli-approvals/decide",
            bad,
            Some(&pair),
            &[],
        );
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{bad}");
    }
    assert_eq!(calls.methods().len(), before);

    // Approved -> 200 { result: { status: "approved" } }.
    let approved = send(
        &gateway,
        Method::POST,
        "/api/v1/cli-approvals/decide",
        r#"{"link":"tok-1","approve":true}"#,
        Some(&pair),
        &[],
    );
    assert_eq!(approved.status, StatusCode::OK, "{}", approved.body);
    assert_eq!(approved.body["result"]["status"], "approved");

    // Declined -> 200 { result: { status: "declined" } }.
    *mode.lock().unwrap() = "declined";
    let declined = send(
        &gateway,
        Method::POST,
        "/api/v1/cli-approvals/decide",
        r#"{"link":"tok-2","approve":false}"#,
        Some(&pair),
        &[],
    );
    assert_eq!(declined.status, StatusCode::OK, "{}", declined.body);
    assert_eq!(declined.body["result"]["status"], "declined");

    // Missing / already used / expired -> 404.
    *mode.lock().unwrap() = "not_found";
    let missing = send(
        &gateway,
        Method::POST,
        "/api/v1/cli-approvals/decide",
        r#"{"link":"tok-3","approve":true}"#,
        Some(&pair),
        &[],
    );
    assert_eq!(missing.status, StatusCode::NOT_FOUND, "{}", missing.body);
    assert_eq!(missing.body["error"]["code"], "not_found");
}

fn gateway_with_auth_enabled(
    script: Arc<Script>,
    mode: org_host_control::listen::ListenMode,
    initial_auth_enabled: bool,
) -> (WebService<SessionApi>, Calls) {
    let (web, peer) = UnixStream::pair().unwrap();
    let calls = Calls::default();
    let seen = calls.clone();
    std::thread::spawn(move || {
        let mut reader = io::BufReader::new(peer);
        while let Ok(Some(request)) = org_proto::frame::read_message::<Value>(&mut reader) {
            let method = request["method"].as_str().unwrap_or_default().to_owned();
            let frames = if method == "hello" {
                let result =
                    org_proto::HelloResult::host("test", "disposable").with_instance_id(INSTANCE);
                vec![json!({"result": result})]
            } else {
                seen.0
                    .lock()
                    .unwrap()
                    .push((method.clone(), request["params"].clone()));
                script(&method, &request["params"])
            };
            for mut frame in frames {
                if frame.get("event").is_none() {
                    frame["id"] = request["id"].clone();
                }
                if org_proto::frame::write_message(reader.get_mut(), &frame).is_err() {
                    return;
                }
            }
        }
    });
    let cache = Arc::new(SessionCache::default());
    let sink_cache = Arc::clone(&cache);
    let generation = Arc::new(std::sync::OnceLock::<u64>::new());
    let sink_generation = Arc::clone(&generation);
    let auth_enabled = Arc::new(AtomicBool::new(initial_auth_enabled));
    let sink_auth = Arc::clone(&auth_enabled);
    let host = HostControl::from_handoff(
        web.into(),
        UnixDatagram::pair().unwrap().0.into(),
        INSTANCE.into(),
        Arc::new(|| panic!("unexpected ancillary failure")),
        crate::process::session_events_with_access(sink_cache, sink_generation, sink_auth),
    )
    .unwrap();
    generation.set(host.generation()).unwrap();
    let origin = ListenOrigin::parse(ORIGIN).unwrap();
    let files =
        BuildFiles::from_files([("index.html".into(), b"<script>0</script>".to_vec())]).unwrap();
    let api = SessionApi::with_access(
        host,
        cache,
        &origin,
        mode,
        EventHub::default(),
        auth_enabled,
    );
    let gateway = WebService::new(
        StaticUi::new(files, origin.clone(), Hsts::Off),
        api,
        ApiLimits::default(),
        RequestAdmission::new(origin),
    )
    .unwrap();
    (gateway, calls)
}

#[test]
fn local_authless_access_and_opt_in_auth_enable_flow() {
    let enabled = Arc::new(AtomicBool::new(false));
    let enabled_script = Arc::clone(&enabled);
    let base = default_script();
    let script: Arc<Script> = Arc::new(move |method, params| match method {
        "browser.local_session" => {
            if enabled_script.load(Ordering::Acquire) {
                vec![json!({"error": {"code": "not_authorized", "message": "auth enabled"}})]
            } else {
                vec![json!({"result": record(1)})]
            }
        }
        "browser.auth_enable" => {
            if enabled_script.swap(true, Ordering::AcqRel) {
                vec![json!({"error": {"code": "conflict", "message": "already enabled"}})]
            } else {
                assert!(params["cookie_digest"].is_string());
                assert!(params["proof_digest"].is_string());
                vec![
                    json!({"event": "auth.mode_changed", "seq": 1, "data": {"auth_enabled": true}}),
                    json!({"result": record(2)}),
                ]
            }
        }
        "organization.list" => {
            if !enabled_script.load(Ordering::Acquire) {
                assert_eq!(params["auth"]["local_authless"], true);
            }
            vec![json!({"result": {"organizations": [], "creating": []}})]
        }
        _ => base(method, params),
    });

    let (gateway, _calls) = gateway_with_auth_enabled(
        script,
        org_host_control::listen::ListenMode::HttpLoopback,
        false,
    );

    // Public GET /api/v1/access reports auth_enabled: false.
    let access = send(&gateway, Method::GET, "/api/v1/access", "", None, &[]);
    assert_eq!(access.status, StatusCode::OK, "{}", access.body);
    assert_eq!(access.body["result"]["auth_enabled"], false);
    assert_eq!(access.body["result"]["listen_scheme"], "http_loopback");

    // Unauthenticated GET /api/v1/session and GET /api/v1/organizations succeed in authless mode.
    let session = send(&gateway, Method::GET, "/api/v1/session", "", None, &[]);
    assert_eq!(session.status, StatusCode::OK, "{}", session.body);
    assert_eq!(session.body["result"]["admin"], true);
    let orgs = send(
        &gateway,
        Method::GET,
        "/api/v1/organizations",
        "",
        None,
        &[],
    );
    assert_eq!(orgs.status, StatusCode::OK, "{}", orgs.body);

    // Opt-in enable auth establishes a browser pair and flips auth_enabled to true.
    let enabled_reply = send(
        &gateway,
        Method::POST,
        "/api/v1/instance/auth/enable",
        r#"{"login":"Owner","password":"long-valid-password-123"}"#,
        None,
        &[],
    );
    assert_eq!(
        enabled_reply.status,
        StatusCode::OK,
        "{}",
        enabled_reply.body
    );
    let pair = enabled_reply.pair();

    let access_after = send(&gateway, Method::GET, "/api/v1/access", "", None, &[]);
    assert_eq!(access_after.status, StatusCode::OK);
    assert_eq!(access_after.body["result"]["auth_enabled"], true);

    // Unauthenticated requests immediately fail 401; paired requests stay authorized.
    let unauth_after = send(&gateway, Method::GET, "/api/v1/session", "", None, &[]);
    assert_eq!(unauth_after.status, StatusCode::UNAUTHORIZED);
    let authed_after = send(
        &gateway,
        Method::GET,
        "/api/v1/session",
        "",
        Some(&pair),
        &[],
    );
    assert_eq!(authed_after.status, StatusCode::OK);

    // Calling auth/enable a second time when authenticated returns 409 Conflict.
    let second = send(
        &gateway,
        Method::POST,
        "/api/v1/instance/auth/enable",
        r#"{"login":"Owner2","password":"long-valid-password-123"}"#,
        Some(&pair),
        &[],
    );
    assert_eq!(second.status, StatusCode::CONFLICT, "{}", second.body);

    // Forged or revoked cookie+proof once auth is enabled MUST fail 401 before BrowserAuthEnable.
    let forged = Pair {
        cookie: format!("org_session_{INSTANCE}=forged-cookie-value"),
        proof: "forged-proof-value".into(),
    };
    let forged_reply = send(
        &gateway,
        Method::POST,
        "/api/v1/instance/auth/enable",
        r#"{"login":"x","password":"short"}"#,
        Some(&forged),
        &[],
    );
    assert_eq!(
        forged_reply.status,
        StatusCode::UNAUTHORIZED,
        "{}",
        forged_reply.body
    );
}

#[test]
fn opt_in_tls_enable_forwards_and_sanitizes_errors() {
    let tls_mode = Arc::new(Mutex::new("ok"));
    let tls_mode_script = Arc::clone(&tls_mode);
    let (gateway, calls) = gateway(organization_script(move |method, params| {
        (method == "browser.tls_enable").then(|| match *tls_mode_script.lock().unwrap() {
            "ok" => {
                assert_eq!(params["mode"]["mode"], "self_signed");
                vec![json!({"result": {
                    "scheme": "https",
                    "origin": "https://127.0.0.1:8080",
                    "fingerprint": "AA:BB",
                    "source": "made",
                    "restart_required": true
                }})]
            }
            "supplied" => {
                assert_eq!(params["mode"]["mode"], "supplied");
                assert_eq!(
                    params["mode"]["cert"],
                    "-----BEGIN CERTIFICATE-----\nCERT\n-----END CERTIFICATE-----"
                );
                assert_eq!(
                    params["mode"]["key"],
                    "-----BEGIN PRIVATE KEY-----\nKEY\n-----END PRIVATE KEY-----"
                );
                vec![json!({"result": {
                    "scheme": "https",
                    "origin": "https://127.0.0.1:8080",
                    "fingerprint": "CC:DD",
                    "source": "supplied",
                    "restart_required": false
                }})]
            }
            "bad_cert" => vec![json!({"error": {
                "code": "invalid_request",
                "message": "internal-host-secret-detail-must-not-leak"
            }})],
            _ => vec![json!({"error": {
                "code": "not_authorized",
                "message": "administrator required"
            }})],
        })
    }));

    // Unauthenticated fails 401 before host call.
    let unauth = send(
        &gateway,
        Method::POST,
        "/api/v1/instance/tls",
        r#"{"mode":"self_signed"}"#,
        None,
        &[],
    );
    assert_eq!(unauth.status, StatusCode::UNAUTHORIZED);
    assert!(calls.methods().is_empty());

    let pair = sign_in(&gateway).pair();

    // Unknown mode, unknown fields, or mismatched cert/key fields fail 400 before host call.
    let before = calls.methods().len();
    for bad in [
        "{}",
        r#"{"mode":"bogus"}"#,
        r#"{"mode":"self_signed","extra":1}"#,
        r#"{"mode":"self_signed","cert":"abc"}"#,
        r#"{"mode":"supplied","cert":"abc"}"#,
        r#"{"mode":"supplied","cert":"abc","key":null}"#,
    ] {
        let reply = send(
            &gateway,
            Method::POST,
            "/api/v1/instance/tls",
            bad,
            Some(&pair),
            &[],
        );
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{bad}");
    }
    assert_eq!(calls.methods().len(), before);

    // Self-signed succeeds with TlsEnableResult.
    let ok_reply = send(
        &gateway,
        Method::POST,
        "/api/v1/instance/tls",
        r#"{"mode":"self_signed"}"#,
        Some(&pair),
        &[],
    );
    assert_eq!(ok_reply.status, StatusCode::OK, "{}", ok_reply.body);
    assert_eq!(ok_reply.body["result"]["scheme"], "https");
    assert_eq!(ok_reply.body["result"]["restart_required"], true);

    // Supplied PEM with escaped `\n` newlines decodes via zeroizing raw deserializer and succeeds.
    *tls_mode.lock().unwrap() = "supplied";
    let supplied_reply = send(
        &gateway,
        Method::POST,
        "/api/v1/instance/tls",
        r#"{"mode":"supplied","cert":"-----BEGIN CERTIFICATE-----\nCERT\n-----END CERTIFICATE-----","key":"-----BEGIN PRIVATE KEY-----\nKEY\n-----END PRIVATE KEY-----"}"#,
        Some(&pair),
        &[],
    );
    assert_eq!(
        supplied_reply.status,
        StatusCode::OK,
        "{}",
        supplied_reply.body
    );
    assert_eq!(supplied_reply.body["result"]["source"], "supplied");

    // Host invalid_request never leaks internal host error text.
    *tls_mode.lock().unwrap() = "bad_cert";
    let bad_cert = send(
        &gateway,
        Method::POST,
        "/api/v1/instance/tls",
        r#"{"mode":"self_signed"}"#,
        Some(&pair),
        &[],
    );
    assert_eq!(
        bad_cert.status,
        StatusCode::BAD_REQUEST,
        "{}",
        bad_cert.body
    );
    assert!(
        !bad_cert
            .body
            .to_string()
            .contains("internal-host-secret-detail"),
        "host wording leaked: {}",
        bad_cert.body
    );

    // Non-admin valid session -> 403 Forbidden.
    *tls_mode.lock().unwrap() = "forbidden";
    let forbidden = send(
        &gateway,
        Method::POST,
        "/api/v1/instance/tls",
        r#"{"mode":"self_signed"}"#,
        Some(&pair),
        &[],
    );
    assert_eq!(
        forbidden.status,
        StatusCode::FORBIDDEN,
        "{}",
        forbidden.body
    );
}

#[test]
fn open_existing_organization_routes_validate_auth_body_and_errors() {
    let mode = Arc::new(Mutex::new("ok"));
    let script_mode = Arc::clone(&mode);
    let base = default_script();
    let script: Arc<Script> = Arc::new(move |method, params| match method {
        "organization.open_preview" => match *script_mode.lock().unwrap() {
            "invalid_file" => vec![json!({"error": {
                "code": "invalid_request",
                "message": "state/teams/t1/team.yaml: unsupported schema_version",
                "field": "state/teams/t1/team.yaml",
                "rule": "invalid_file"
            }})],
            "forbidden" => vec![json!({"error": {
                "code": "not_authorized",
                "message": "instance administrator required"
            }})],
            _ => vec![json!({"result": {
                "organization_id": "01945c00-0000-7000-8000-000000000010",
                "name": "Solaris",
                "slug": "solaris",
                "repository_remote_url": "https://example.com/solaris.git",
                "canonical_remote_url": "https://example.com/solaris.git",
                "state_branch": "org/state",
                "teams_count": 2,
                "projects_count": 1,
                "agents_count": 1,
                "people": [{
                    "id": "01945c00-0000-7000-8000-000000000020",
                    "name": "Sam",
                    "handle": "sam",
                    "home_team_id": "01945c00-0000-7000-8000-000000000030"
                }],
                "local_only_not_restored": ["Chat conversations"]
            }})],
        },
        "organization.open" => vec![json!({"result": {
            "organization": {
                "id": "01945c00-0000-7000-8000-000000000010",
                "name": "Solaris",
                "slug": "solaris",
                "created_at": "2026-11-21T00:00:00Z",
                "employee_id": "01945c00-0000-7000-8000-000000000020",
                "offices": [],
                "recovery": "pending"
            },
            "local_only_not_restored": ["Chat conversations"]
        }})],
        _ => base(method, params),
    });
    let (gateway, calls) = gateway(script);

    // Unauthenticated requests fail 401 before reaching the host.
    let unauth = send(
        &gateway,
        Method::POST,
        "/api/v1/organizations/open/preview",
        r#"{"repository_remote_url":"https://example.com/solaris.git","state_branch":"org/state"}"#,
        None,
        &[],
    );
    assert_eq!(unauth.status, StatusCode::UNAUTHORIZED);
    assert!(calls.methods().is_empty());

    let pair = sign_in(&gateway).pair();

    // Unknown fields or missing operation_id fail 400 before host call.
    let before = calls.methods().len();
    for (path, body) in [
        (
            "/api/v1/organizations/open/preview",
            r#"{"repository_remote_url":"https://example.com/solaris.git","state_branch":"org/state","extra":1}"#,
        ),
        (
            "/api/v1/organizations/open",
            r#"{"operation_id":"not-a-uuid","repository_remote_url":"https://example.com/solaris.git","state_branch":"org/state","owner_member_id":"01945c00-0000-7000-8000-000000000020"}"#,
        ),
    ] {
        let reply = send(&gateway, Method::POST, path, body, Some(&pair), &[]);
        assert_eq!(
            reply.status,
            StatusCode::BAD_REQUEST,
            "{path}: {}",
            reply.body
        );
    }
    assert_eq!(calls.methods().len(), before);

    // Valid preview and open succeed.
    let preview = send(
        &gateway,
        Method::POST,
        "/api/v1/organizations/open/preview",
        r#"{"repository_remote_url":"https://example.com/solaris.git","state_branch":"org/state"}"#,
        Some(&pair),
        &[],
    );
    assert_eq!(preview.status, StatusCode::OK, "{}", preview.body);
    assert_eq!(preview.body["result"]["slug"], "solaris");

    let opened = send(
        &gateway,
        Method::POST,
        "/api/v1/organizations/open",
        r#"{"operation_id":"01945c00-0000-7000-8000-000000000099","repository_remote_url":"https://example.com/solaris.git","state_branch":"org/state","owner_member_id":"01945c00-0000-7000-8000-000000000020"}"#,
        Some(&pair),
        &[],
    );
    assert_eq!(opened.status, StatusCode::OK, "{}", opened.body);
    assert_eq!(opened.body["result"]["organization"]["slug"], "solaris");

    // Invalid file surfaces field + message as 400.
    *mode.lock().unwrap() = "invalid_file";
    let bad = send(
        &gateway,
        Method::POST,
        "/api/v1/organizations/open/preview",
        r#"{"repository_remote_url":"https://example.com/solaris.git","state_branch":"org/state"}"#,
        Some(&pair),
        &[],
    );
    assert_eq!(bad.status, StatusCode::BAD_REQUEST, "{}", bad.body);
    assert_eq!(bad.body["error"]["field"], "state/teams/t1/team.yaml");

    // Non-admin with live session returns 403.
    *mode.lock().unwrap() = "forbidden";
    let denied = send(
        &gateway,
        Method::POST,
        "/api/v1/organizations/open/preview",
        r#"{"repository_remote_url":"https://example.com/solaris.git","state_branch":"org/state"}"#,
        Some(&pair),
        &[],
    );
    assert_eq!(denied.status, StatusCode::FORBIDDEN, "{}", denied.body);

    // Expired/revoked cached session re-validates via session.validate and returns 401.
    let signed_out = send(
        &gateway,
        Method::POST,
        "/api/v1/session/sign-out",
        "{}",
        Some(&pair),
        &[],
    );
    assert_eq!(signed_out.status, StatusCode::OK);
    let expired = send(
        &gateway,
        Method::POST,
        "/api/v1/organizations/open/preview",
        r#"{"repository_remote_url":"https://example.com/solaris.git","state_branch":"org/state"}"#,
        Some(&pair),
        &[],
    );
    assert_eq!(expired.status, StatusCode::UNAUTHORIZED, "{}", expired.body);
}

#[test]
fn cost_budget_get_query_routes_and_account_preferences_method_not_allowed() {
    let (gateway, calls) = gateway(organization_script(|method, _| {
        (method == "organization.list").then(|| {
            vec![json!({"result": {"organizations": [host_organization()], "creating": []}})]
        })
    }));
    let pair = sign_in(&gateway).pair();

    // `GET /api/v1/account/preferences` returns 400 InvalidRequest (known POST-only route, not 404).
    let prefs_get = send(
        &gateway,
        Method::GET,
        "/api/v1/account/preferences",
        "",
        Some(&pair),
        &[],
    );
    assert_eq!(prefs_get.status, StatusCode::BAD_REQUEST);

    // `GET /api/v1/organizations/{org}/costs/prices?...` and `/budgets?...` parse query params
    // instead of returning 404 (unknown param returns 400; valid query dispatches to HQ office).
    let bad_prices = send(
        &gateway,
        Method::GET,
        &format!("/api/v1/organizations/{ORG}/costs/prices?unknown=1"),
        "",
        Some(&pair),
        &[],
    );
    assert_eq!(bad_prices.status, StatusCode::BAD_REQUEST);

    let ok_prices = send(
        &gateway,
        Method::GET,
        &format!(
            "/api/v1/organizations/{ORG}/costs/prices?provider=anthropic&model=claude-sonnet-4"
        ),
        "",
        Some(&pair),
        &[],
    );
    assert_eq!(ok_prices.status, StatusCode::SERVICE_UNAVAILABLE);

    let ok_budgets = send(
        &gateway,
        Method::GET,
        &format!("/api/v1/organizations/{ORG}/budgets?scope_kind=agent"),
        "",
        Some(&pair),
        &[],
    );
    assert_eq!(ok_budgets.status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(calls.methods().contains(&"organization.list".to_owned()));
}

#[test]
fn role_inspect_route_requires_canonical_uuidv7_before_reaching_office() {
    let (gateway, calls) = gateway(organization_script(|method, _| {
        (method == "organization.list").then(|| {
            vec![json!({"result": {"organizations": [host_organization()], "creating": []}})]
        })
    }));
    let pair = sign_in(&gateway).pair();

    // Non-UUIDv7 role aliases and reserved segments are rejected at the gateway without calling HQ.
    for bad in [
        "software-engineer",
        "update",
        "0193A5C0-0000-7000-8000-000000000040",
        "550e8400-e29b-41d4-a716-446655440000",
    ] {
        let reply = send(
            &gateway,
            Method::GET,
            &format!("/api/v1/organizations/{ORG}/roles/{bad}"),
            "",
            Some(&pair),
            &[],
        );
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "role={bad}");
    }
    assert!(
        !calls.methods().contains(&"organization.list".to_owned()),
        "invalid role IDs must fail before resolving HQ"
    );

    // Unauthenticated requests still fail with 401 before validating the role ID.
    let unauth = send(
        &gateway,
        Method::GET,
        &format!("/api/v1/organizations/{ORG}/roles/software-engineer"),
        "",
        None,
        &[],
    );
    assert_eq!(unauth.status, StatusCode::UNAUTHORIZED);

    // Canonical lowercase UUIDv7 proceeds to HQ resolution (503 in fixture with no office socket).
    let valid = send(
        &gateway,
        Method::GET,
        &format!("/api/v1/organizations/{ORG}/roles/0193a5c0-0000-7000-8000-000000000040"),
        "",
        Some(&pair),
        &[],
    );
    assert_eq!(valid.status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(calls.methods().contains(&"organization.list".to_owned()));
}

/// `POST /api/v1/organizations/{org}/agents/{id}/{interrupt,steer}` (`TASK-103`): the
/// cookie+proof pair first, then Origin/`Sec-Fetch-Site`, the agent id and the body, all before
/// the office is asked; a valid control goes on to the organization's HQ office.
#[test]
fn turn_control_routes_check_the_pair_origin_id_and_body_before_reaching_the_office() {
    const AGENT: &str = "0193a5c0-0000-7000-8000-000000000050";
    let (gateway, calls) = gateway(organization_script(|method, _| {
        (method == "organization.list").then(|| {
            vec![json!({"result": {"organizations": [host_organization()], "creating": []}})]
        })
    }));
    let interrupt = format!("/api/v1/organizations/{ORG}/agents/{AGENT}/interrupt");
    let steer = format!("/api/v1/organizations/{ORG}/agents/{AGENT}/steer");
    let interrupt_body = json!({"command_id": OPERATION}).to_string();
    let steer_body = json!({"command_id": OPERATION, "text": "Use staging"}).to_string();

    // Without the pair: 401 and no host call.
    let before = calls.methods();
    for (path, body) in [(&interrupt, &interrupt_body), (&steer, &steer_body)] {
        let reply = send(&gateway, Method::POST, path, body, None, &[]);
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{path}");
    }
    assert_eq!(calls.methods(), before);

    let pair = sign_in(&gateway).pair();
    let before = calls.methods();
    // A cross-site or cross-origin browser request is refused before routing.
    for extra in [
        ("sec-fetch-site", "cross-site"),
        ("origin", "http://evil.example"),
    ] {
        for (path, body) in [(&interrupt, &interrupt_body), (&steer, &steer_body)] {
            let reply = send(&gateway, Method::POST, path, body, Some(&pair), &[extra]);
            assert_eq!(reply.status, StatusCode::FORBIDDEN, "{path} {extra:?}");
        }
    }
    // A bad agent id, a wrong method, or a bad body is 400 before HQ is resolved.
    let oversized = "x".repeat(org_proto::sessions::MAX_STEER_TEXT_BYTES + 1);
    for (method, path, body) in [
        (
            Method::POST,
            format!("/api/v1/organizations/{ORG}/agents/not..an-id/interrupt"),
            interrupt_body.clone(),
        ),
        (Method::GET, interrupt.clone(), String::new()),
        (Method::GET, steer.clone(), String::new()),
        (Method::POST, interrupt.clone(), "{}".into()),
        (
            Method::POST,
            interrupt.clone(),
            json!({"command_id": OPERATION, "reason": "x"}).to_string(),
        ),
        (
            Method::POST,
            interrupt.clone(),
            json!({"command_id": "not-an-operation"}).to_string(),
        ),
        (
            Method::POST,
            steer.clone(),
            json!({"command_id": OPERATION}).to_string(),
        ),
        (
            Method::POST,
            steer.clone(),
            json!({"command_id": OPERATION, "text": "  \n "}).to_string(),
        ),
        (
            Method::POST,
            steer.clone(),
            json!({"command_id": OPERATION, "text": oversized}).to_string(),
        ),
        (
            Method::POST,
            steer.clone(),
            json!({"command_id": OPERATION, "text": "t", "employee_id": AGENT}).to_string(),
        ),
    ] {
        let reply = send(&gateway, method.clone(), &path, &body, Some(&pair), &[]);
        assert_eq!(
            reply.status,
            StatusCode::BAD_REQUEST,
            "{method} {path} {body}"
        );
    }
    assert_eq!(calls.methods(), before, "nothing reached the host");

    // A valid control resolves the organization's HQ office (503: no office socket here).
    for (path, body) in [(&interrupt, &interrupt_body), (&steer, &steer_body)] {
        let reply = send(&gateway, Method::POST, path, body, Some(&pair), &[]);
        assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE, "{path}");
    }
    assert!(calls.methods().contains(&"organization.list".to_owned()));
}

#[path = "tests/attachments.rs"]
mod attachments;

#[path = "tests/task_defaults.rs"]
mod task_defaults;

#[path = "tests/search.rs"]
mod search;

#[test]
fn member_decisions_route_checks_session_member_and_query_before_reaching_hq() {
    let (gateway, calls) = gateway(organization_script(|method, _| {
        (method == "organization.list").then(|| {
            vec![json!({"result": {"organizations": [host_organization()], "creating": []}})]
        })
    }));
    let member = "0193a5c0-0000-7000-8000-000000000040";
    let decisions = format!("/api/v1/organizations/{ORG}/members/{member}/decisions");

    // No session: 401, whatever the member or query.
    let unauth = send(&gateway, Method::GET, &decisions, "", None, &[]);
    assert_eq!(unauth.status, StatusCode::UNAUTHORIZED);

    let pair = sign_in(&gateway).pair();
    // A malformed member, another author or kind, or an unknown filter never reaches HQ.
    for target in [
        format!("/api/v1/organizations/{ORG}/members/not-a-uuid/decisions"),
        format!(
            "/api/v1/organizations/{ORG}/members/0193A5C0-0000-7000-8000-000000000040/decisions"
        ),
        format!("{decisions}?kind=note"),
        format!("{decisions}?author_id=0193a5c0-0000-7000-8000-000000000041"),
        format!("{decisions}?bogus=1"),
        format!("{decisions}?fields=secret"),
        format!("{decisions}?limit=ten"),
        // IDs are canonical UUIDv7s, the page size 1 to 500, text filters 1 to 128 bytes, and
        // no key repeats.
        format!("{decisions}?project_id=not-a-uuid"),
        format!("{decisions}?project_id=0193A5C0-0000-7000-8000-000000000050"),
        format!("{decisions}?limit=0"),
        format!("{decisions}?limit=501"),
        format!("{decisions}?tag="),
        format!("{decisions}?since="),
        format!("{decisions}?cursor="),
        format!("{decisions}?tag={}", "a".repeat(129)),
        format!("{decisions}?since={}", "1".repeat(129)),
        format!("{decisions}?cursor={}", "c".repeat(129)),
        format!("{decisions}?tag=a&tag=b"),
        format!("{decisions}?limit=5&limit=6"),
        format!("/api/v1/organizations/{ORG}/tasks/T-A1/notes?author_id=not-a-uuid"),
        format!("/api/v1/organizations/{ORG}/tasks/T-A1/notes?kind=plan&kind=note"),
        format!("/api/v1/organizations/{ORG}/tasks/T-A1/notes?limit=501"),
        format!("/api/v1/organizations/{ORG}/tasks/notes/not-a-uuid"),
        format!("/api/v1/organizations/{ORG}/tasks/notes/0193A5C0-0000-7000-8000-000000000060"),
        format!("/api/v1/organizations/{ORG}/tasks/notes/550e8400-e29b-41d4-a716-446655440000"),
    ] {
        let reply = send(&gateway, Method::GET, &target, "", Some(&pair), &[]);
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{target}");
    }
    let posted = send(&gateway, Method::POST, &decisions, "{}", Some(&pair), &[]);
    assert_eq!(posted.status, StatusCode::BAD_REQUEST);
    let task_notes_with_task_id = send(
        &gateway,
        Method::GET,
        &format!("/api/v1/organizations/{ORG}/tasks/T-A1/notes?task_id=T-A2"),
        "",
        Some(&pair),
        &[],
    );
    assert_eq!(task_notes_with_task_id.status, StatusCode::BAD_REQUEST);
    assert!(
        !calls.methods().contains(&"organization.list".to_owned()),
        "refused requests must fail before resolving HQ"
    );

    // Valid requests go to the organization's HQ office (503 here: the fixture has no office).
    for target in [
        format!("{decisions}?tag=arch&project_id=0193a5c0-0000-7000-8000-000000000050&task_id=T-A1&limit=20&fields=title,tags"),
        format!("/api/v1/organizations/{ORG}/tasks/T-A1/notes?kind=decision&tag=arch&fields=title"),
        format!("/api/v1/organizations/{ORG}/tasks/notes/0193a5c0-0000-7000-8000-000000000060"),
    ] {
        let reply = send(&gateway, Method::GET, &target, "", Some(&pair), &[]);
        assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE, "{target}");
    }
    assert!(calls.methods().contains(&"organization.list".to_owned()));
}

#[test]
fn project_doc_routes_check_session_path_query_and_body_before_reaching_hq() {
    let (gateway, calls) = gateway(organization_script(|method, _| {
        (method == "organization.list").then(|| {
            vec![json!({"result": {"organizations": [host_organization()], "creating": []}})]
        })
    }));
    let base = format!("/api/v1/organizations/{ORG}/projects/checkout/docs");
    let doc = "0193a5c0-0000-7000-8000-000000000040";
    let create = r#"{"command_id":"0193a5c0-0000-7000-8000-000000000041","kind":"guide","title":"Setup","section":"guides"}"#;
    let update = r#"{"command_id":"0193a5c0-0000-7000-8000-000000000042","expected_revision":1,"title":"New"}"#;
    let mv = r#"{"command_id":"0193a5c0-0000-7000-8000-000000000043","expected_revision":2,"new_path":"a.md"}"#;
    let valid = [
        (Method::GET, base.clone(), ""),
        (
            Method::GET,
            format!("{base}?section=guides&status=draft&limit=10"),
            "",
        ),
        (Method::GET, format!("{base}/index"), ""),
        (Method::GET, format!("{base}/{doc}"), ""),
        (Method::POST, base.clone(), create),
        (Method::PATCH, format!("{base}/{doc}"), update),
        (Method::POST, format!("{base}/{doc}/move"), mv),
    ];
    // No session: 401 before the path or the body is looked at.
    for (method, target, body) in &valid {
        let reply = send(&gateway, method.clone(), target, body, None, &[]);
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{method} {target}");
    }
    let pair = sign_in(&gateway).pair();
    // Bad paths, queries and bodies: 400 without resolving the HQ office.
    for (method, target, body) in [
        (Method::GET, format!("{base}/guides-setup"), ""),
        (Method::GET, format!("{base}/index?x=1"), ""),
        (Method::GET, format!("{base}/{doc}?x=1"), ""),
        (Method::GET, format!("{base}?unknown=1"), ""),
        (Method::GET, format!("{base}?status=archived"), ""),
        (Method::GET, format!("{base}?section=a&section=b"), ""),
        (Method::GET, format!("{base}/index"), "{}"),
        (Method::PATCH, format!("{base}/index"), update),
        (Method::PATCH, base.clone(), update),
        (Method::PATCH, format!("{base}/{doc}"), "{}"),
        (Method::POST, format!("{base}/{doc}/move"), "not json"),
        (Method::POST, base.clone(), r#"{"title":1}"#),
        (Method::POST, format!("{base}/index"), create),
        (
            Method::GET,
            format!("/api/v1/organizations/{ORG}/projects/bad.ref/docs"),
            "",
        ),
    ] {
        let reply = send(&gateway, method.clone(), &target, body, Some(&pair), &[]);
        assert_eq!(
            reply.status,
            StatusCode::BAD_REQUEST,
            "{method} {target} {body}"
        );
    }
    let deeper = send(
        &gateway,
        Method::GET,
        &format!("{base}/{doc}/move/x"),
        "",
        Some(&pair),
        &[],
    );
    assert_eq!(deeper.status, StatusCode::NOT_FOUND);
    assert!(
        !calls.methods().contains(&"organization.list".to_owned()),
        "refused requests never resolve the HQ office"
    );
    // Well-formed requests reach the HQ office (503 here: the fixture has no office socket).
    for (method, target, body) in &valid {
        let reply = send(&gateway, method.clone(), target, body, Some(&pair), &[]);
        assert_eq!(
            reply.status,
            StatusCode::SERVICE_UNAVAILABLE,
            "{method} {target}"
        );
    }
    assert!(calls.methods().contains(&"organization.list".to_owned()));
}
