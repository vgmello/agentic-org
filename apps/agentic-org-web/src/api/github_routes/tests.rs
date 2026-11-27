//! Full-gateway HTTP integration tests for GitHub App webhook ingress, owner-only config/bind
//! routes, and project-scoped PR/CI routes (`TASK-272`).

use std::future::Future;
use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use http_body_util::{BodyExt, Full};
use hyper::{body::Bytes, header, Method, Request, StatusCode};
use org_host_control::listen::{ListenMode, ListenOrigin};
use org_host_control::organization::BrowserAuth;
use org_proto::github::{
    GitHubAppStatusView, GitHubCiOverallStatus, GitHubPrChecksResult, GitHubPrListResult,
    GitHubWebhookIngestResult,
};
use org_proto::{ApiError, ErrorCode};
use serde_json::{json, Value};
use zeroize::Zeroizing;

use crate::api::SessionApi;
use crate::assets::BuildFiles;
use crate::event_hub::{EventBus, EventSubscription, HubEvent};
use crate::ingress::RequestAdmission;
use crate::ports::{
    BoxFuture, ControlPayload, ControlPayloadRef, ControlPlaneTransport, Failure,
    OfficeCallOptions, OfficeFailure, OfficeTransport, RawReplyHook,
};
use crate::service::{ApiLimits, WebService};
use crate::session::{cookie::PROOF_HEADER, SessionCache};
use crate::transport::PeerAddress;
use crate::{Hsts, StaticUi};

const ORIGIN: &str = "http://127.0.0.1:8080";
const HOST: &str = "127.0.0.1:8080";
const INSTANCE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const ORG_ID: &str = "0193a5c0-0000-7000-8000-000000000001";
const OFFICE_ID: &str = "0193a5c0-0000-7000-8000-000000000002";
const PEER: &str = "192.0.2.42:51000";

struct FakeHost;

impl ControlPlaneTransport for FakeHost {
    fn instance(&self) -> &str {
        INSTANCE
    }
    fn generation(&self) -> u64 {
        1
    }
    fn live(&self) -> bool {
        true
    }
    fn close(&self) {}
    fn call_raw(
        &self,
        method: &'static str,
        _params: Zeroizing<Vec<u8>>,
        hook: Option<RawReplyHook>,
    ) -> Result<ControlPayload, Failure> {
        let res = host_reply(method)?;
        let bytes = Zeroizing::new(serde_json::to_vec(&res).unwrap());
        if let Some(hook) = hook {
            hook(&Ok(ControlPayloadRef::Result(&bytes)));
        }
        Ok(ControlPayload::Result(bytes))
    }
    fn call_raw_async(
        &self,
        method: &'static str,
        params: Zeroizing<Vec<u8>>,
        hook: Option<RawReplyHook>,
    ) -> BoxFuture<'_, Result<ControlPayload, Failure>> {
        let out = self.call_raw(method, params, hook);
        Box::pin(async move { out })
    }
}

fn host_reply(method: &str) -> Result<Value, Failure> {
    match method {
        "session.validate" => Ok(json!({
            "session_id": "0193a5c0-0000-7000-8000-000000000010",
            "account": { "id": "acc-1", "login": "owner" },
            "admin": true,
            "orgs": [{
                "org_id": ORG_ID,
                "name": "Acme",
                "employee_id": "0193a5c0-0000-7000-8000-000000000006",
                "office_ids": [OFFICE_ID]
            }],
            "server_time_ms": 1_794_312_000_000u64,
            "idle_expires_at_ms": u64::MAX / 2
        })),
        "organization.list" => Ok(json!({
            "organizations": [{
                "id": ORG_ID,
                "name": "Acme",
                "slug": "acme",
                "created_at": "2026-11-10T12:00:00.000Z",
                "employee_id": "0193a5c0-0000-7000-8000-000000000006",
                "recovery": "pending",
                "offices": [{
                    "id": OFFICE_ID,
                    "organization_id": ORG_ID,
                    "name": "HQ",
                    "role": "hq",
                    "host": "This Host",
                    "status": "active",
                    "endpoint": "/private/office.sock"
                }]
            }],
            "creating": []
        })),
        _ => Err(Failure::Unavailable),
    }
}

type RpcHandler = dyn Fn(&'static str, &Value) -> Result<Value, OfficeFailure> + Send + Sync;

struct FakeOffice {
    rpc: Arc<RpcHandler>,
    webhook: Arc<RpcHandler>,
    seen_webhooks: Mutex<Vec<(String, Value)>>,
}

impl OfficeTransport for FakeOffice {
    fn call_raw<'a>(
        &'a self,
        _auth: &'a BrowserAuth,
        _office_id: &'a str,
        method: &'static str,
        params: Value,
        _options: OfficeCallOptions,
    ) -> BoxFuture<'a, Result<Value, OfficeFailure>> {
        let out = (self.rpc)(method, &params);
        Box::pin(async move { out })
    }

    fn dispatch_mutation_raw<'a>(
        &'a self,
        _auth: &'a BrowserAuth,
        _office_id: &'a str,
        _method: &'a str,
        _params: Value,
    ) -> BoxFuture<'a, Result<Option<Value>, OfficeFailure>> {
        Box::pin(async { Err(OfficeFailure::Transport) })
    }

    fn call_webhook_raw<'a>(
        &'a self,
        org_id: &'a str,
        method: &'static str,
        params: Value,
    ) -> BoxFuture<'a, Result<Value, OfficeFailure>> {
        self.seen_webhooks
            .lock()
            .unwrap()
            .push((org_id.to_owned(), params.clone()));
        let out = (self.webhook)(method, &params);
        Box::pin(async move { out })
    }
}

struct NullEventBus;
impl EventBus for NullEventBus {
    fn publish(&self, _event: HubEvent) {}
    fn subscribe(&self) -> EventSubscription {
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        EventSubscription::from_channel(rx)
    }
}

fn run_async(fut: impl Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(fut);
}

fn build_service(office: Arc<FakeOffice>) -> WebService<SessionApi> {
    let origin = ListenOrigin::parse(ORIGIN).unwrap();
    let files =
        BuildFiles::from_files([("index.html".into(), b"<script>0</script>".to_vec())]).unwrap();
    let api = SessionApi::from_ports(
        Arc::new(FakeHost),
        office,
        Arc::new(NullEventBus),
        Arc::new(SessionCache::default()),
        &origin,
        ListenMode::HttpLoopback,
        Arc::new(AtomicBool::new(true)),
    );
    WebService::new(
        StaticUi::new(files, origin.clone(), Hsts::Off),
        api,
        ApiLimits::default(),
        RequestAdmission::new(origin),
    )
    .unwrap()
}

async fn send(
    service: &WebService<SessionApi>,
    mut req: Request<Full<Bytes>>,
) -> (StatusCode, Value) {
    let peer: SocketAddr = PEER.parse().unwrap();
    req.extensions_mut().insert(PeerAddress(peer));
    let resp = service.respond(req).await;
    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    (status, json)
}

fn make_fake_office() -> Arc<FakeOffice> {
    Arc::new(FakeOffice {
        rpc: Arc::new(|method, _params| match method {
            "github.app.status" => Ok(serde_json::to_value(GitHubAppStatusView {
                office_id: OFFICE_ID.into(),
                configured: true,
                app_id: Some(77),
                installation_id: Some(88),
                api_base_url: Some("https://api.github.com".into()),
                webhook_configured: true,
                account_hint: Some("acme".into()),
                updated_at: Some("2026-11-27T12:00:00Z".into()),
                bindings: vec![],
            })
            .unwrap()),
            "github.pr.list" => Ok(serde_json::to_value(GitHubPrListResult {
                project_id: "proj-1".into(),
                pull_requests: vec![],
            })
            .unwrap()),
            "github.pr.checks" => Ok(serde_json::to_value(GitHubPrChecksResult {
                project_id: "proj-1".into(),
                repository_id: "repo-1".into(),
                pr_number: 42,
                head_sha: "deadbeef".into(),
                overall_status: GitHubCiOverallStatus::Passing,
                linked_task_ids: vec![],
                linked_review_ids: vec![],
                linked_review_keys: vec![],
                check_runs: vec![],
                failure_summaries: vec![],
            })
            .unwrap()),
            _ => Err(OfficeFailure::Refused(ApiError::new(
                ErrorCode::NotAuthorized,
                "owner only",
            ))),
        }),
        webhook: Arc::new(|method, params| {
            assert_eq!(method, "github.webhook.ingest");
            let sig = params["signature_256"].as_str().unwrap_or("");
            if sig != "sha256=valid" {
                return Err(OfficeFailure::Refused(ApiError::new(
                    ErrorCode::NotAuthorized,
                    "invalid HMAC",
                )));
            }
            Ok(serde_json::to_value(GitHubWebhookIngestResult {
                delivery_id: params["delivery_id"].as_str().unwrap().into(),
                event: params["event"].as_str().unwrap().into(),
                accepted: true,
                duplicate: false,
                ignored: false,
                repository_id: Some("repo-1".into()),
                pr_number: Some(42),
                linked_task_ids: vec![],
                linked_review_ids: vec![],
            })
            .unwrap())
        }),
        seen_webhooks: Mutex::new(Vec::new()),
    })
}

fn webhook_request(
    uri: &str,
    sig: &str,
    body: Bytes,
    extra: &[(&str, &str)],
) -> Request<Full<Bytes>> {
    let mut b = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::HOST, HOST)
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-github-event", "pull_request")
        .header("x-github-delivery", "11111111-2222-4333-8444-555555555555")
        .header("x-hub-signature-256", sig);
    for (k, v) in extra {
        b = b.header(*k, *v);
    }
    b.body(Full::new(body)).unwrap()
}

#[test]
fn webhook_route_admits_signed_post_without_cookie_and_enforces_security_guards() {
    run_async(async {
        let service = build_service(make_fake_office());
        let webhook_path = format!("/api/v1/organizations/{ORG_ID}/integrations/github/webhook");

        let (status, body) = send(
            &service,
            webhook_request(
                &webhook_path,
                "sha256=valid",
                Bytes::from_static(br#"{"action":"opened"}"#),
                &[],
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["result"]["pr_number"], json!(42));

        let (status, _) = send(
            &service,
            webhook_request(
                &webhook_path,
                "sha256=bad",
                Bytes::from_static(br#"{}"#),
                &[],
            ),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let (status, _) = send(
            &service,
            webhook_request(
                &webhook_path,
                "sha256=valid",
                Bytes::from(vec![b'a'; 1024 * 1024 + 1]),
                &[],
            ),
        )
        .await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);

        for extra in [
            vec![("origin", "https://evil.example")],
            vec![("sec-fetch-site", "cross-site")],
        ] {
            let (status, _) = send(
                &service,
                webhook_request(
                    &webhook_path,
                    "sha256=valid",
                    Bytes::from_static(br#"{}"#),
                    &extra,
                ),
            )
            .await;
            assert_eq!(status, StatusCode::FORBIDDEN);
        }
    });
}

#[test]
fn management_and_project_pr_routes_require_session_csrf_and_owner_authority() {
    run_async(async {
        let service = build_service(make_fake_office());
        let cfg_path = format!("/api/v1/organizations/{ORG_ID}/integrations/github/configure");

        let req = Request::builder()
            .method(Method::POST)
            .uri(&cfg_path)
            .header(header::HOST, HOST)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Full::new(Bytes::from_static(br#"{}"#)))
            .unwrap();
        assert_eq!(send(&service, req).await.0, StatusCode::FORBIDDEN);

        let req = Request::builder()
            .method(Method::POST)
            .uri(&cfg_path)
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Full::new(Bytes::from_static(br#"{}"#)))
            .unwrap();
        assert_eq!(send(&service, req).await.0, StatusCode::UNAUTHORIZED);

        let cookie_name = crate::session::CookieName::new(INSTANCE, false);
        let cookie = format!("{}={}", cookie_name.as_str(), "A".repeat(43));
        let proof = "a".repeat(64);
        for path in [
            format!("/api/v1/organizations/{ORG_ID}/integrations/github/status"),
            format!("/api/v1/organizations/{ORG_ID}/projects/proj-1/github/prs?state=open&limit=10"),
            format!(
                "/api/v1/organizations/{ORG_ID}/projects/proj-1/github/prs/42/checks?only_failures=true"
            ),
        ] {
            let req = Request::builder()
                .method(Method::GET)
                .uri(&path)
                .header(header::HOST, HOST)
                .header(header::COOKIE, &cookie)
                .header(PROOF_HEADER, &proof)
                .body(Full::new(Bytes::new()))
                .unwrap();
            let (status, body) = send(&service, req).await;
            assert_eq!(status, StatusCode::OK, "{path}: {body}");
        }

        let req = Request::builder()
            .method(Method::POST)
            .uri(&cfg_path)
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN)
            .header(header::COOKIE, &cookie)
            .header(PROOF_HEADER, &proof)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Full::new(Bytes::from(
                json!({
                    "app_id": 123,
                    "installation_id": 456,
                    "private_key_pem": "dummy"
                })
                .to_string(),
            )))
            .unwrap();
        assert_eq!(send(&service, req).await.0, StatusCode::FORBIDDEN);
    });
}
