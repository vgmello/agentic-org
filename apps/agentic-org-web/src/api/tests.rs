//! Session routes against a scripted host socket: what reaches the host, what
//! the browser gets back, and when the cache spares a host call.
use super::*;
use crate::{
    assets::BuildFiles,
    ingress::RequestAdmission,
    service::{ApiLimits, Gateway},
    session::cookie::PROOF_HEADER,
    Hsts, StaticUi,
};
use http_body_util::{BodyExt, Full};
use hyper::{body::Bytes, header, Request};
use serde_json::{json, Value};
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
            "instance.view" => vec![json!({"result": {
                "name": "studio", "status": "service", "web_status": "running",
                "administrator": "present"
            }})],
            _ => vec![json!({"error":{"code":"unavailable","message":"no"}})],
        }
    })
}

fn gateway(script: Arc<Script>) -> (Gateway<SessionApi>, Calls) {
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
        Arc::new(move |name: &str, data: &Value| {
            if name == "session.ended" {
                sink_cache.ended(
                    data["session_id"].as_str().unwrap(),
                    *sink_generation.get().unwrap(),
                );
            }
        }),
    )
    .unwrap();
    generation.set(host.generation()).unwrap();
    let origin = ListenOrigin::parse(ORIGIN).unwrap();
    let files =
        BuildFiles::from_files([("index.html".into(), b"<script>0</script>".to_vec())]).unwrap();
    let api = SessionApi::new(host, cache, &origin);
    let gateway = Gateway::new(
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
    gateway: &Gateway<SessionApi>,
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

fn sign_in(gateway: &Gateway<SessionApi>) -> Answer {
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
        json!({"result":{"api_version":1,"contract_revision":org_proto::gateway::CONTRACT_REVISION}})
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
        (Method::POST, "/api/v1/links/set-password"),
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
    assert_eq!(orgs.body, json!({"result":{"organizations":[]}}));
    assert_eq!(calls.methods(), ["session.sign_in"]);
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
        _ => vec![json!({"error":{"code":"unavailable","message":"no"}})],
    }));
    for target in [
        "/api/v1/session",
        "/api/v1/organizations",
        "/api/v1/session",
    ] {
        let answer = send(&gateway, Method::GET, target, "", Some(&pair), &[]);
        assert_eq!(answer.status, StatusCode::OK, "{target}");
    }
    assert_eq!(calls.methods(), ["session.validate"]);
    let sent = calls.last();
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
        ("process.rs", include_str!("../process.rs")),
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
