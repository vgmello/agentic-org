use super::*;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};

use http_body_util::BodyExt;
use hyper::body::Frame;
use hyper::header::{HOST, ORIGIN};

const ORIGIN_TEXT: &str = "https://gateway.test:8443";
const HOST_TEXT: &str = "gateway.test:8443";

// Admission and route authorization must finish without polling an untrusted body.
struct UnreadBody;

impl Body for UnreadBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        panic!("body polled before admission/authorization");
    }
}

struct ProofBackend(Arc<AtomicUsize>);

impl ApiBackend for ProofBackend {
    type Route = Infallible;
    type Call = Infallible;

    fn route(&self, _: &Parts, _: &RequestPath) -> Result<Infallible, ApiReply> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(ApiReply::error(
            StatusCode::FORBIDDEN,
            GatewayErrorCode::NotAuthorized,
            "Route proof required.",
        ))
    }

    fn prepare(
        &self,
        route: Infallible,
        _: BusinessBody,
    ) -> Result<PreparedCall<Infallible>, ApiReply> {
        match route {}
    }

    fn execute(&self, call: Infallible) -> ApiReply {
        match call {}
    }
}

fn service() -> (Gateway<ProofBackend>, Arc<AtomicUsize>) {
    let origin = ListenOrigin::parse(ORIGIN_TEXT).unwrap();
    let files =
        BuildFiles::from_files([("index.html".into(), b"<script>0</script>".to_vec())]).unwrap();
    let routed = Arc::new(AtomicUsize::new(0));
    let gateway = Gateway::new(
        StaticUi::new(files, origin.clone(), Hsts::Off),
        ProofBackend(Arc::clone(&routed)),
        ApiLimits::default(),
        RequestAdmission::new(origin),
    )
    .unwrap();
    (gateway, routed)
}

fn request(method: Method, target: &str, hosts: &[&str], origins: &[&str]) -> Request<UnreadBody> {
    let mut builder = Request::builder().method(method).uri(target);
    for host in hosts {
        builder = builder.header(HOST, *host);
    }
    for origin in origins {
        builder = builder.header(ORIGIN, *origin);
    }
    builder.body(UnreadBody).unwrap()
}

async fn error(
    response: Response<Full<Bytes>>,
    status: StatusCode,
    code: &str,
    head: bool,
) -> Value {
    assert_eq!(response.status(), status);
    for (name, value) in [
        ("content-type", "application/json; charset=utf-8"),
        ("cache-control", "no-store"),
        ("x-content-type-options", "nosniff"),
        ("referrer-policy", "same-origin"),
        ("x-frame-options", "DENY"),
        ("cross-origin-opener-policy", "same-origin"),
        ("cross-origin-resource-policy", "same-origin"),
        ("permissions-policy", "camera=(), microphone=(), geolocation=(), payment=(), usb=(), serial=(), hid=(), bluetooth=()"),
        ("content-security-policy", "default-src 'none'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'"),
    ] {
        assert_eq!(response.headers()[name], value, "{name}");
    }
    assert!(
        response.headers()["content-length"]
            .to_str()
            .unwrap()
            .parse::<usize>()
            .unwrap()
            > 0
    );
    let body = response.into_body().collect().await.unwrap().to_bytes();
    if head {
        assert!(body.is_empty());
        Value::Null
    } else {
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["error"]["code"], code);
        value
    }
}

#[test]
fn host_refusals_precede_static_api_origin_version_path_and_body() {
    run(async {
        let (gateway, routed) = service();
        for path in ["/", "/api", "/api/v1/read", "/api/v9/read", "/api/%zz"] {
            for hosts in [
                vec![],
                vec![""],
                vec!["attacker.test"],
                vec![HOST_TEXT, HOST_TEXT],
                vec![HOST_TEXT, "attacker.test"],
            ] {
                for version in [hyper::Version::HTTP_11, hyper::Version::HTTP_10] {
                    let mut req = request(Method::POST, path, &hosts, &["null"]);
                    *req.version_mut() = version;
                    let value = error(
                        gateway.respond(req).await,
                        StatusCode::BAD_REQUEST,
                        "invalid_request",
                        false,
                    )
                    .await;
                    assert_eq!(
                        value["error"]["message"],
                        "Invalid request authority or target."
                    );
                    assert_eq!(routed.load(Ordering::SeqCst), 0);
                }
            }
        }
        // HEAD refuses with the same headers and an empty body.
        error(
            gateway
                .respond(request(Method::HEAD, "/", &["attacker.test"], &[]))
                .await,
            StatusCode::BAD_REQUEST,
            "invalid_request",
            true,
        )
        .await;
    });
}

#[test]
fn target_forms_are_refused_before_route_and_body() {
    run(async {
        let (gateway, routed) = service();
        for (method, target) in [
            (Method::GET, "https://gateway.test:8443/api/v1/read"),
            (Method::GET, "https://attacker.test/api/v1/read"),
            (Method::CONNECT, "gateway.test:8443"),
            (Method::CONNECT, "/api/v1/read"),
            (Method::OPTIONS, "*"),
        ] {
            error(
                gateway
                    .respond(request(method, target, &[HOST_TEXT], &[ORIGIN_TEXT]))
                    .await,
                StatusCode::BAD_REQUEST,
                "invalid_request",
                false,
            )
            .await;
            assert_eq!(routed.load(Ordering::SeqCst), 0);
        }
    });
}

#[test]
fn origin_gate_covers_every_api_namespace_and_method_but_no_static_namespace() {
    run(async {
        let (gateway, routed) = service();
        for path in [
            "/api",
            "/api/v1/read",
            "/api/v9/read",
            "/%61pi/v1/read",
            "/apiary",
            "/",
        ] {
            for method in [
                Method::GET,
                Method::HEAD,
                Method::POST,
                Method::PUT,
                Method::PATCH,
                Method::DELETE,
                Method::OPTIONS,
            ] {
                for origins in [
                    vec![],
                    vec![ORIGIN_TEXT],
                    vec!["null"],
                    vec!["https://gateway.test:8443/"],
                    vec!["https://attacker.test"],
                    vec![ORIGIN_TEXT, ORIGIN_TEXT],
                ] {
                    routed.store(0, Ordering::SeqCst);
                    let api = path != "/apiary" && path != "/";
                    let accepted = origins == [ORIGIN_TEXT]
                        || (origins.is_empty() && matches!(method, Method::GET | Method::HEAD));
                    let response = gateway
                        .respond(request(method.clone(), path, &[HOST_TEXT], &origins))
                        .await;
                    if api {
                        let expected_route = usize::from(accepted && path.ends_with("/v1/read"));
                        assert_eq!(
                            routed.load(Ordering::SeqCst),
                            expected_route,
                            "{path} {method} {origins:?}"
                        );
                        let status = if accepted && expected_route == 0 {
                            StatusCode::NOT_FOUND
                        } else {
                            StatusCode::FORBIDDEN
                        };
                        let code = if status == StatusCode::NOT_FOUND {
                            "not_found"
                        } else {
                            "not_authorized"
                        };
                        let value = error(response, status, code, method == Method::HEAD).await;
                        if !accepted && method != Method::HEAD {
                            assert_eq!(
                                value["error"]["message"],
                                "Request origin is not authorized."
                            );
                        }
                    } else {
                        let status = if path == "/" && matches!(method, Method::GET | Method::HEAD)
                        {
                            StatusCode::OK
                        } else {
                            StatusCode::NOT_FOUND
                        };
                        assert_eq!(response.status(), status, "{path} {method} {origins:?}");
                        assert_eq!(routed.load(Ordering::SeqCst), 0);
                    }
                }
            }
        }
    });
}

fn run(future: impl Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future);
}
