//! Zero-Unix-socket BFF tests for `/api/v1/organizations/{org}/tess/*` HTTP & SSE routes
//! (`TASK-80.4` Part B, `TASK-232` `SessionApi::from_ports`).

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use http_body_util::{BodyExt, Full};
use hyper::{body::Bytes, header, Method, Request, StatusCode};
use org_host_control::browser::{BrowserAuth, BrowserOrigin};
use org_host_control::listen::{ListenMode, ListenOrigin};
use org_proto::tess::{
    SuggestionApplyReceipt, SuggestionApplyResult, SuggestionAttribution, SuggestionListResult,
    SuggestionStatus, SuggestionView, TessAskResult, TessAskStatus, TessSessionResetResult,
    TessToolState, TessTurnCancelResult, TessTurnEvent, TessTurnEventKind, TessTurnEventsResult,
    TessTurnOutcome, UiBlock,
};
use org_proto::{ApiError, ErrorCode};
use serde_json::{json, Map, Value};
use tokio::sync::mpsc;
use zeroize::Zeroizing;

use crate::api::SessionApi;
use crate::assets::BuildFiles;
use crate::event_hub::{EventBus, EventRecvError, EventSubscription, HubEvent};
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
const SESSION_ID: &str = "sess-tess-1";
const PEER: &str = "192.0.2.50:51000";

struct FakeControlPlane {
    instance: String,
    generation: u64,
}

impl FakeControlPlane {
    fn new() -> Self {
        Self {
            instance: INSTANCE.to_owned(),
            generation: 1,
        }
    }

    fn handle(&self, method: &str, _params: &Value) -> Result<Value, Failure> {
        match method {
            "session.sign_in" | "session.validate" => Ok(json!({
                "session_id": SESSION_ID,
                "account": {
                    "id": "acc-1",
                    "login": "owner",
                    "time_zone": "UTC",
                    "last_org_id": ORG_ID
                },
                "admin": true,
                "orgs": [{
                    "org_id": ORG_ID,
                    "employee_id": "owner-1",
                    "office_ids": [OFFICE_ID]
                }],
                "server_time_ms": 1_794_312_000_000_u64,
                "idle_expires_at_ms": u64::MAX / 2
            })),
            "organization.list" => Ok(json!({
                "organizations": [{
                    "id": ORG_ID,
                    "name": "Acme",
                    "slug": "acme",
                    "created_at": "2026-11-27T00:00:00.000Z",
                    "employee_id": "owner-1",
                    "offices": [{
                        "id": OFFICE_ID,
                        "organization_id": ORG_ID,
                        "name": "HQ",
                        "role": "hq",
                        "host": "This Mac",
                        "status": "active"
                    }],
                    "recovery": "acknowledged"
                }],
                "creating": []
            })),
            _ => Err(Failure::NotFound),
        }
    }
}

impl ControlPlaneTransport for FakeControlPlane {
    fn instance(&self) -> &str {
        &self.instance
    }

    fn generation(&self) -> u64 {
        self.generation
    }

    fn live(&self) -> bool {
        true
    }

    fn close(&self) {}

    fn call_raw(
        &self,
        method: &'static str,
        params_json: Zeroizing<Vec<u8>>,
        hook: Option<RawReplyHook>,
    ) -> Result<ControlPayload, Failure> {
        let params: Value =
            serde_json::from_slice(&params_json).map_err(|_| Failure::Unavailable)?;
        match self.handle(method, &params) {
            Ok(val) => {
                let bytes =
                    Zeroizing::new(serde_json::to_vec(&val).map_err(|_| Failure::Unavailable)?);
                if let Some(h) = hook {
                    h(&Ok(ControlPayloadRef::Result(&bytes)));
                }
                Ok(ControlPayload::Result(bytes))
            }
            Err(err) => {
                if let Some(h) = hook {
                    h(&Err(err.clone()));
                }
                Err(err)
            }
        }
    }

    fn call_raw_async(
        &self,
        method: &'static str,
        params_json: Zeroizing<Vec<u8>>,
        hook: Option<RawReplyHook>,
    ) -> BoxFuture<'_, Result<ControlPayload, Failure>> {
        let out = self.call_raw(method, params_json, hook);
        Box::pin(async move { out })
    }
}

type OfficeHandler =
    dyn Fn(&str, &Value, OfficeCallOptions) -> Result<Value, OfficeFailure> + Send + Sync;

struct FakeOfficeTransport {
    recorded: Mutex<Vec<(String, Value)>>,
    handler: Arc<OfficeHandler>,
}

impl FakeOfficeTransport {
    fn new(
        handler: impl Fn(&str, &Value, OfficeCallOptions) -> Result<Value, OfficeFailure>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        Self {
            recorded: Mutex::new(Vec::new()),
            handler: Arc::new(handler),
        }
    }

    fn calls(&self) -> Vec<(String, Value)> {
        self.recorded.lock().unwrap().clone()
    }
}

impl OfficeTransport for FakeOfficeTransport {
    fn call_raw<'a>(
        &'a self,
        _auth: &'a BrowserAuth,
        _office_id: &'a str,
        method: &'static str,
        params: Value,
        options: OfficeCallOptions,
    ) -> BoxFuture<'a, Result<Value, OfficeFailure>> {
        self.recorded
            .lock()
            .unwrap()
            .push((method.to_owned(), params.clone()));
        let wait_ms = params.get("wait_ms").and_then(Value::as_u64).unwrap_or(0);
        let res = (self.handler)(method, &params, options);
        let delay = method == "tess.turn_events"
            && wait_ms > 0
            && res
                .as_ref()
                .ok()
                .and_then(|v| v.get("events"))
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty);
        Box::pin(async move {
            if delay {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            res
        })
    }

    fn dispatch_mutation_raw<'a>(
        &'a self,
        _auth: &'a BrowserAuth,
        _office_id: &'a str,
        _method: &'a str,
        _params: Value,
    ) -> BoxFuture<'a, Result<Option<Value>, OfficeFailure>> {
        Box::pin(async move { Ok(None) })
    }
}

#[derive(Default)]
struct ChannelBus {
    subs: Mutex<Vec<mpsc::Sender<Result<HubEvent, EventRecvError>>>>,
}

impl EventBus for ChannelBus {
    fn publish(&self, event: HubEvent) {
        let mut subs = self.subs.lock().unwrap();
        subs.retain(|tx| tx.try_send(Ok(event.clone())).is_ok());
    }

    fn subscribe(&self) -> EventSubscription {
        let (tx, rx) = mpsc::channel(32);
        self.subs.lock().unwrap().push(tx);
        EventSubscription::from_channel(rx)
    }
}

fn build_service(office: Arc<FakeOfficeTransport>, bus: Arc<ChannelBus>) -> WebService<SessionApi> {
    let origin = ListenOrigin::parse(ORIGIN).unwrap();
    let host: Arc<dyn ControlPlaneTransport> = Arc::new(FakeControlPlane::new());
    let offices: Arc<dyn OfficeTransport> = office;
    let events: Arc<dyn EventBus> = bus;
    let api = SessionApi::from_ports(
        host,
        offices,
        events,
        Arc::new(SessionCache::default()),
        &origin,
        ListenMode::HttpLoopback,
        Arc::new(AtomicBool::new(true)),
    );
    let files =
        BuildFiles::from_files([("index.html".into(), b"<script>0</script>".to_vec())]).unwrap();
    let ui = StaticUi::new(files, origin.clone(), Hsts::Off);
    let admission = RequestAdmission::new(origin);
    WebService::new(ui, api, ApiLimits::default(), admission).unwrap()
}

fn run_async(fut: impl std::future::Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(fut);
}

async fn sign_in(svc: &WebService<SessionApi>) -> (String, String) {
    let mut req = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/session/sign-in")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, ORIGIN)
        .header("sec-fetch-site", "same-origin")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Full::new(Bytes::from(
            r#"{"login":"owner","password":"secret-password-value"}"#,
        )))
        .unwrap();
    req.extensions_mut()
        .insert(PeerAddress(PEER.parse::<SocketAddr>().unwrap()));
    let resp = svc.respond(req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let cookie = resp
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .expect("sign-in sets session cookie")
        .to_owned();
    let body: Value =
        serde_json::from_slice(&resp.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let proof = body["result"]["proof"].as_str().unwrap().to_owned();
    (cookie, proof)
}

fn authed_request(
    creds: &(String, String),
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> Request<Full<Bytes>> {
    let mut b = Request::builder()
        .method(method.clone())
        .uri(uri)
        .header(header::HOST, HOST)
        .header(header::COOKIE, creds.0.as_str())
        .header(PROOF_HEADER, creds.1.as_str());
    if method == Method::POST {
        b = b
            .header(header::ORIGIN, ORIGIN)
            .header("sec-fetch-site", "same-origin")
            .header(header::CONTENT_TYPE, "application/json");
    }
    let bytes = body
        .map(|v| Bytes::from(serde_json::to_vec(&v).unwrap()))
        .unwrap_or_default();
    let mut req = b.body(Full::new(bytes)).unwrap();
    req.extensions_mut()
        .insert(PeerAddress(PEER.parse::<SocketAddr>().unwrap()));
    req
}

fn sample_suggestion(id: &str, status: SuggestionStatus) -> SuggestionView {
    SuggestionView {
        id: id.to_owned(),
        status,
        command: "task update".into(),
        call: "tasks.update".into(),
        input: Map::new(),
        summary: "Update task title".into(),
        rationale: None,
        expected_revision: Some(2),
        created_by_member_id: "tess-1".into(),
        for_caller_id: "owner-1".into(),
        preflight_ok: true,
        preflight_note: Some("verified".into()),
        created_at: "2026-11-27T00:00:00.000Z".into(),
        expires_at: "2026-11-27T00:15:00.000Z".into(),
        resolved_at: (status != SuggestionStatus::Pending)
            .then(|| "2026-11-27T00:01:00.000Z".into()),
        close_reason: None,
    }
}

fn standard_tess_office_transport() -> Arc<FakeOfficeTransport> {
    Arc::new(FakeOfficeTransport::new(
        |method, params, _opts| match method {
            "tess.ask" => {
                let prompt = params.get("prompt").and_then(Value::as_str).unwrap_or("");
                let result = if prompt.contains("offline") {
                    TessAskResult {
                        status: TessAskStatus::Fallback,
                        turn_id: None,
                        session_id: None,
                        partition: "tess:ask:owner-1".into(),
                        unavailable_reason: Some("No provider bound.".into()),
                        fallback_answer: None,
                    }
                } else {
                    TessAskResult {
                        status: TessAskStatus::Streaming,
                        turn_id: Some("turn-1".into()),
                        session_id: Some("sess-1".into()),
                        partition: "tess:ask:owner-1".into(),
                        unavailable_reason: None,
                        fallback_answer: None,
                    }
                };
                Ok(serde_json::to_value(result).unwrap())
            }
            "tess.turn_cancel" => Ok(serde_json::to_value(TessTurnCancelResult {
                turn_id: params["turn_id"].as_str().unwrap().to_owned(),
                cancelled: true,
                partition: "tess:ask:owner-1".into(),
            })
            .unwrap()),
            "suggestion.list" => Ok(serde_json::to_value(SuggestionListResult {
                suggestions: vec![sample_suggestion("sug-1", SuggestionStatus::Pending)],
            })
            .unwrap()),
            "suggestion.apply" => {
                if params["suggestion_id"] == "sug-stale" {
                    return Err(OfficeFailure::Refused(ApiError::new(
                        ErrorCode::Conflict,
                        "target task is at revision 3, not expected revision 2",
                    )));
                }
                Ok(serde_json::to_value(SuggestionApplyResult {
                    suggestion: sample_suggestion("sug-1", SuggestionStatus::Applied),
                    applied_result: json!({"key": "T-A1", "revision": 3}),
                    attribution: SuggestionAttribution {
                        actor_member_id: "owner-1".into(),
                        proposed_by_member_id: "tess-1".into(),
                    },
                    receipt: SuggestionApplyReceipt {
                        summary: "Update task title".into(),
                        entity_href: Some("/work/T-A1".into()),
                        entity_label: Some("T-A1".into()),
                        applied_at: "2026-11-27T00:01:00.000Z".into(),
                    },
                })
                .unwrap())
            }
            "suggestion.dismiss" => Ok(json!({
                "suggestion": sample_suggestion("sug-1", SuggestionStatus::Dismissed)
            })),
            "tess.session_reset" => Ok(serde_json::to_value(TessSessionResetResult {
                reset: true,
                stopped_live_session: true,
                partition: "tess:ask:owner-1".into(),
            })
            .unwrap()),
            other => Err(OfficeFailure::Refused(ApiError::new(
                ErrorCode::UnknownMethod,
                format!("unexpected {other}"),
            ))),
        },
    ))
}

async fn json_body(resp: hyper::Response<crate::sse::WebBody>) -> Value {
    serde_json::from_slice(&resp.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

#[test]
fn tess_http_ask_cancel_and_reset_routes_work() {
    run_async(async {
        let svc = build_service(
            standard_tess_office_transport(),
            Arc::new(ChannelBus::default()),
        );
        let creds = sign_in(&svc).await;

        let resp = svc
            .respond(authed_request(
                &creds,
                Method::POST,
                &format!("/api/v1/organizations/{ORG_ID}/tess/ask"),
                Some(json!({
                    "prompt": "Summarize blockers",
                    "viewport_context": {"route": "/work/T-A1", "entity_id": "T-A1"}
                })),
            ))
            .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["result"]["status"], "streaming");
        assert_eq!(
            body["result"]["stream_url"],
            format!("/api/v1/organizations/{ORG_ID}/tess/turns/turn-1/stream")
        );

        let fb = svc
            .respond(authed_request(
                &creds,
                Method::POST,
                &format!("/api/v1/organizations/{ORG_ID}/tess/ask"),
                Some(json!({"prompt": "offline check"})),
            ))
            .await;
        assert_eq!(fb.status(), StatusCode::OK);
        let fb_body = json_body(fb).await;
        assert_eq!(fb_body["result"]["status"], "fallback");
        assert_eq!(
            fb_body["result"]["unavailable_reason"],
            "No provider bound."
        );

        let cancel = svc
            .respond(authed_request(
                &creds,
                Method::POST,
                &format!("/api/v1/organizations/{ORG_ID}/tess/turns/turn-1/cancel"),
                Some(json!({})),
            ))
            .await;
        assert_eq!(cancel.status(), StatusCode::OK);
        assert_eq!(json_body(cancel).await["result"]["cancelled"], true);

        let reset = svc
            .respond(authed_request(
                &creds,
                Method::POST,
                &format!("/api/v1/organizations/{ORG_ID}/tess/session/reset"),
                Some(json!({})),
            ))
            .await;
        assert_eq!(reset.status(), StatusCode::OK);
        assert_eq!(json_body(reset).await["result"]["reset"], true);
    });
}

#[test]
fn tess_http_proposal_routes_and_csrf_guards_work() {
    run_async(async {
        let svc = build_service(
            standard_tess_office_transport(),
            Arc::new(ChannelBus::default()),
        );
        let creds = sign_in(&svc).await;

        let list = svc
            .respond(authed_request(
                &creds,
                Method::GET,
                &format!("/api/v1/organizations/{ORG_ID}/tess/proposals?status=pending&limit=10"),
                None,
            ))
            .await;
        assert_eq!(list.status(), StatusCode::OK);
        assert_eq!(
            json_body(list).await["result"]["suggestions"]
                .as_array()
                .unwrap()
                .len(),
            1
        );

        let bad_q = svc
            .respond(authed_request(
                &creds,
                Method::GET,
                &format!("/api/v1/organizations/{ORG_ID}/tess/proposals?status=bogus"),
                None,
            ))
            .await;
        assert_eq!(bad_q.status(), StatusCode::BAD_REQUEST);

        let apply = svc
            .respond(authed_request(
                &creds,
                Method::POST,
                &format!("/api/v1/organizations/{ORG_ID}/tess/proposals/sug-1/apply"),
                Some(json!({"if_match": "rev-2"})),
            ))
            .await;
        assert_eq!(apply.status(), StatusCode::OK);
        let apply_body = json_body(apply).await;
        assert_eq!(apply_body["result"]["suggestion"]["status"], "applied");
        assert_eq!(
            apply_body["result"]["attribution"]["actor_member_id"],
            "owner-1"
        );

        let conflict = svc
            .respond(authed_request(
                &creds,
                Method::POST,
                &format!("/api/v1/organizations/{ORG_ID}/tess/proposals/sug-stale/apply"),
                Some(json!({})),
            ))
            .await;
        assert_eq!(conflict.status(), StatusCode::CONFLICT);

        let dismiss = svc
            .respond(authed_request(
                &creds,
                Method::POST,
                &format!("/api/v1/organizations/{ORG_ID}/tess/proposals/sug-1/dismiss"),
                Some(json!({"reason": "Not needed"})),
            ))
            .await;
        assert_eq!(dismiss.status(), StatusCode::OK);

        let mut no_proof = Request::builder()
            .method(Method::POST)
            .uri(format!("/api/v1/organizations/{ORG_ID}/tess/ask"))
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN)
            .header("sec-fetch-site", "same-origin")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::COOKIE, creds.0.as_str())
            .body(Full::new(Bytes::from(r#"{"prompt":"hi"}"#)))
            .unwrap();
        no_proof
            .extensions_mut()
            .insert(PeerAddress(PEER.parse::<SocketAddr>().unwrap()));
        assert_eq!(
            svc.respond(no_proof).await.status(),
            StatusCode::UNAUTHORIZED
        );

        let mut bad_origin = Request::builder()
            .method(Method::POST)
            .uri(format!("/api/v1/organizations/{ORG_ID}/tess/ask"))
            .header(header::HOST, HOST)
            .header(header::ORIGIN, "https://evil.example")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::COOKIE, creds.0.as_str())
            .header(PROOF_HEADER, creds.1.as_str())
            .body(Full::new(Bytes::from(r#"{"prompt":"hi"}"#)))
            .unwrap();
        bad_origin
            .extensions_mut()
            .insert(PeerAddress(PEER.parse::<SocketAddr>().unwrap()));
        assert_eq!(
            svc.respond(bad_origin).await.status(),
            StatusCode::FORBIDDEN
        );
    });
}

fn sample_sse_batches(lagged: bool) -> VecDeque<TessTurnEventsResult> {
    VecDeque::from([
        TessTurnEventsResult {
            turn_id: "turn-sse".into(),
            partition: "tess:ask:owner-1".into(),
            events: vec![
                TessTurnEvent {
                    seq: 3,
                    at: "2026-11-27T00:00:01.000Z".into(),
                    event: TessTurnEventKind::ToolProgress {
                        id: "tool-1".into(),
                        label: "tasks.list".into(),
                        state: TessToolState::Running,
                        phase: Some("running_tool".into()),
                        started_at: Some(100),
                        duration_ms: None,
                    },
                },
                TessTurnEvent {
                    seq: 4,
                    at: "2026-11-27T00:00:02.000Z".into(),
                    event: TessTurnEventKind::TextDelta {
                        delta: "Two blockers found.".into(),
                    },
                },
            ],
            head_seq: 4,
            done: false,
            lagged,
        },
        TessTurnEventsResult {
            turn_id: "turn-sse".into(),
            partition: "tess:ask:owner-1".into(),
            events: vec![
                TessTurnEvent {
                    seq: 5,
                    at: "2026-11-27T00:00:03.000Z".into(),
                    event: TessTurnEventKind::UiBlock {
                        ui_block: UiBlock {
                            title: Some("Blockers".into()),
                            ..UiBlock::default()
                        },
                    },
                },
                TessTurnEvent {
                    seq: 6,
                    at: "2026-11-27T00:00:04.000Z".into(),
                    event: TessTurnEventKind::SuggestionCreated {
                        suggestion: Box::new(sample_suggestion("sug-9", SuggestionStatus::Pending)),
                        ui_block: UiBlock {
                            title: Some("Update task title".into()),
                            suggestion_id: Some("sug-9".into()),
                            ..UiBlock::default()
                        },
                    },
                },
                TessTurnEvent {
                    seq: 7,
                    at: "2026-11-27T00:00:05.000Z".into(),
                    event: TessTurnEventKind::TurnDone {
                        turn_id: "turn-sse".into(),
                        outcome: TessTurnOutcome::Completed,
                        follow_ups: vec!["Check budget".into()],
                    },
                },
            ],
            head_seq: 7,
            done: true,
            lagged: false,
        },
    ])
}

#[test]
fn tess_sse_stream_delivers_frames_resumes_last_event_id_and_emits_resync_on_lag() {
    run_async(async {
        let batches = Arc::new(Mutex::new(sample_sse_batches(true)));
        let q = Arc::clone(&batches);
        let office = Arc::new(FakeOfficeTransport::new(move |method, params, _opts| {
            assert_eq!(method, "tess.turn_events");
            if params["turn_id"] == "other-caller-turn" {
                return Err(OfficeFailure::Refused(ApiError::new(
                    ErrorCode::NotFound,
                    "turn not found",
                )));
            }
            Ok(serde_json::to_value(q.lock().unwrap().pop_front().unwrap()).unwrap())
        }));
        let svc = build_service(Arc::clone(&office), Arc::new(ChannelBus::default()));
        let creds = sign_in(&svc).await;

        let missing = svc
            .respond(authed_request(
                &creds,
                Method::GET,
                &format!("/api/v1/organizations/{ORG_ID}/tess/turns/other-caller-turn/stream"),
                None,
            ))
            .await;
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);

        let mut req = authed_request(
            &creds,
            Method::GET,
            &format!("/api/v1/organizations/{ORG_ID}/tess/turns/turn-sse/stream"),
            None,
        );
        req.headers_mut()
            .insert("last-event-id", header::HeaderValue::from_static("2"));
        let resp = svc.respond(req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/event-stream; charset=utf-8"
        );
        let text = String::from_utf8(
            resp.into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
        .unwrap();
        assert!(text.contains("event: resync\n"), "{text}");
        assert!(text.contains("id: 3\nevent: tool.progress\n"), "{text}");
        assert!(text.contains("id: 4\nevent: text.delta\n"), "{text}");
        assert!(text.contains("id: 5\nevent: ui.block\n"), "{text}");
        assert!(
            text.contains("id: 6\nevent: suggestion.created\n"),
            "{text}"
        );
        assert!(text.contains("id: 7\nevent: turn.done\n"), "{text}");
        let calls = office.calls();
        let resumed = calls
            .iter()
            .find(|(m, p)| m == "tess.turn_events" && p["turn_id"] == "turn-sse")
            .unwrap();
        assert_eq!(resumed.1["after_seq"], 2);
    });
}

#[test]
fn tess_sse_stream_closes_promptly_on_session_ended() {
    run_async(async {
        let bus = Arc::new(ChannelBus::default());
        let idle_office = Arc::new(FakeOfficeTransport::new(|_method, _params, _opts| {
            Ok(serde_json::to_value(TessTurnEventsResult {
                turn_id: "turn-open".into(),
                partition: "tess:ask:owner-1".into(),
                events: Vec::new(),
                head_seq: 1,
                done: false,
                lagged: false,
            })
            .unwrap())
        }));
        let svc = build_service(idle_office, Arc::clone(&bus));
        let creds = sign_in(&svc).await;
        let resp = svc
            .respond(authed_request(
                &creds,
                Method::GET,
                &format!("/api/v1/organizations/{ORG_ID}/tess/turns/turn-open/stream"),
                None,
            ))
            .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let bus_publish = Arc::clone(&bus);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            bus_publish.publish(HubEvent::SessionEnded {
                session_id: SESSION_ID.to_owned(),
            });
        });
        let collected = tokio::time::timeout(Duration::from_secs(2), resp.into_body().collect())
            .await
            .expect("stream closes promptly on SessionEnded")
            .unwrap()
            .to_bytes();
        let ended_text = String::from_utf8(collected.to_vec()).unwrap();
        assert!(
            ended_text.contains("event: session.ended\n"),
            "{ended_text}"
        );
    });
}

fn empty_turn_batch(turn_id: &str, head_seq: u64, done: bool) -> TessTurnEventsResult {
    TessTurnEventsResult {
        turn_id: turn_id.to_owned(),
        partition: "tess:ask:owner-1".into(),
        events: Vec::new(),
        head_seq,
        done,
        lagged: false,
    }
}

#[test]
fn tess_sse_keepalive_fires_across_empty_poll_batches_and_closes_on_turn_done() {
    use super::stream::{open_turn_sse, TurnStreamSetup};

    run_async(async {
        let mut queue = VecDeque::new();
        for _ in 0..5 {
            queue.push_back(empty_turn_batch("turn-keepalive", 1, false));
        }
        queue.push_back(TessTurnEventsResult {
            turn_id: "turn-keepalive".into(),
            partition: "tess:ask:owner-1".into(),
            events: vec![TessTurnEvent {
                seq: 2,
                at: "2026-11-27T00:00:02.000Z".into(),
                event: TessTurnEventKind::TurnDone {
                    turn_id: "turn-keepalive".into(),
                    outcome: TessTurnOutcome::Completed,
                    follow_ups: Vec::new(),
                },
            }],
            head_seq: 2,
            done: true,
            lagged: false,
        });
        let batches = Arc::new(Mutex::new(queue));
        let q = Arc::clone(&batches);
        let office = Arc::new(FakeOfficeTransport::new(move |method, _params, _opts| {
            assert_eq!(method, "tess.turn_events");
            let next = q
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| empty_turn_batch("turn-keepalive", 2, true));
            Ok(serde_json::to_value(next).unwrap())
        }));

        let bus = Arc::new(ChannelBus::default());
        let initial = TessTurnEventsResult {
            turn_id: "turn-keepalive".into(),
            partition: "tess:ask:owner-1".into(),
            events: vec![TessTurnEvent {
                seq: 1,
                at: "2026-11-27T00:00:00.000Z".into(),
                event: TessTurnEventKind::TurnStarted {
                    turn_id: "turn-keepalive".into(),
                },
            }],
            head_seq: 1,
            done: false,
            lagged: false,
        };
        let resp = open_turn_sse(
            TurnStreamSetup {
                events: bus.subscribe(),
                offices: Arc::clone(&office) as Arc<dyn OfficeTransport>,
                session_id: SESSION_ID.to_owned(),
                auth: BrowserAuth::local_authless(BrowserOrigin {
                    scheme: "http".into(),
                    host: "127.0.0.1".into(),
                    port: 8080,
                }),
                office_id: OFFICE_ID.to_owned(),
                turn_id: "turn-keepalive".into(),
                initial,
            },
            Duration::from_millis(45),
        );

        let bytes = tokio::time::timeout(Duration::from_secs(2), resp.into_body().collect())
            .await
            .expect("stream shuts down cleanly after turn.done")
            .unwrap()
            .to_bytes();
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        let keepalives = text.matches(": keepalive\n\n").count();
        assert!(
            keepalives >= 2,
            "expected >= 2 keepalive frames across 5 empty 25 ms poll batches (45 ms heartbeat), got {keepalives}; body:\n{text}"
        );
        assert!(text.contains("id: 1\nevent: turn.started\n"), "{text}");
        assert!(text.contains("id: 2\nevent: turn.done\n"), "{text}");
        assert!(
            office.calls().len() >= 6,
            "expected 5 empty polls + 1 terminal poll, got {}",
            office.calls().len()
        );
    });
}

async fn recv_frame(rx: &mut mpsc::Receiver<Bytes>, timeout: Duration, label: &str) -> String {
    let bytes = tokio::time::timeout(timeout, rx.recv())
        .await
        .expect(label)
        .expect("stream open");
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[test]
fn tess_sse_keepalive_resets_only_on_emitted_frames() {
    use super::stream::{write_sse_loop, TurnStreamMsg};

    run_async(async {
        let (body_tx, mut body_rx) = mpsc::channel::<Bytes>(32);
        let (msg_tx, msg_rx) = mpsc::channel::<TurnStreamMsg>(32);
        let loop_task = tokio::spawn(write_sse_loop(
            body_tx,
            msg_rx,
            empty_turn_batch("turn-hb", 1, false),
            Duration::from_millis(60),
        ));

        // Two empty batches at 20 ms and 40 ms must NOT reset `last_heartbeat`.
        for _ in 0..2 {
            tokio::time::sleep(Duration::from_millis(20)).await;
            msg_tx
                .send(TurnStreamMsg::Batch(empty_turn_batch("turn-hb", 1, false)))
                .await
                .unwrap();
        }
        let first = recv_frame(
            &mut body_rx,
            Duration::from_millis(250),
            "keepalive fires across empty batches",
        )
        .await;
        assert_eq!(first, ": keepalive\n\n");

        // Emitting a real `resync` frame resets `last_heartbeat` (no keepalive within 25 ms).
        let mut resync_batch = empty_turn_batch("turn-hb", 5, false);
        resync_batch.lagged = true;
        msg_tx
            .send(TurnStreamMsg::Batch(resync_batch))
            .await
            .unwrap();
        let resync = recv_frame(&mut body_rx, Duration::from_millis(200), "resync frame").await;
        assert!(resync.starts_with("event: resync\n"), "{resync}");
        assert!(
            tokio::time::timeout(Duration::from_millis(25), body_rx.recv())
                .await
                .is_err(),
            "heartbeat timer must reset after emitting resync frame"
        );

        drop(msg_tx);
        loop_task.await.unwrap();
    });
}

#[test]
fn tess_sse_keepalive_closes_cleanly_on_turn_error() {
    use super::stream::{write_sse_loop, TurnStreamMsg};

    run_async(async {
        let (body_tx, mut body_rx) = mpsc::channel::<Bytes>(32);
        let (msg_tx, msg_rx) = mpsc::channel::<TurnStreamMsg>(32);
        let loop_task = tokio::spawn(write_sse_loop(
            body_tx,
            msg_rx,
            empty_turn_batch("turn-err", 1, false),
            Duration::from_millis(40),
        ));

        let keepalive = recv_frame(
            &mut body_rx,
            Duration::from_millis(250),
            "keepalive before turn.error",
        )
        .await;
        assert_eq!(keepalive, ": keepalive\n\n");

        msg_tx
            .send(TurnStreamMsg::Batch(TessTurnEventsResult {
                turn_id: "turn-err".into(),
                partition: "tess:ask:owner-1".into(),
                events: vec![TessTurnEvent {
                    seq: 6,
                    at: "2026-11-27T00:00:06.000Z".into(),
                    event: TessTurnEventKind::TurnError {
                        message: "runtime disconnected".into(),
                    },
                }],
                head_seq: 6,
                done: true,
                lagged: false,
            }))
            .await
            .unwrap();
        let mut saw_error = false;
        while let Some(frame) = tokio::time::timeout(Duration::from_secs(1), body_rx.recv())
            .await
            .expect("stream closes cleanly on turn.error")
        {
            if std::str::from_utf8(&frame)
                .unwrap()
                .contains("event: turn.error\n")
            {
                saw_error = true;
            }
        }
        assert!(saw_error, "expected terminal turn.error frame");
        loop_task.await.unwrap();
    });
}
