//! Zero-Unix-socket conformance tests for [`ControlPlaneTransport`], [`OfficeTransport`],
//! and [`EventBus`] (`TASK-232` AC #3).
//!
//! Every test in this module runs strictly against custom in-memory port doubles — no
//! `UnixStream`, `UnixDatagram`, or `tokio::sync::broadcast::Sender` is created anywhere here.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use http_body_util::{BodyExt, Full};
use hyper::{body::Bytes, header, Method, Request, StatusCode};
use org_host_control::listen::{ListenMode, ListenOrigin};
use org_host_control::organization::BrowserAuth;
use org_proto::costs::{CostSubscriptionUsageCollect, CostSubscriptionUsageCollectParams};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use zeroize::Zeroizing;

use crate::api::SessionApi;
use crate::assets::BuildFiles;
use crate::event_hub::{EventBus, EventRecvError, EventSubscription, HubEvent};
use crate::ingress::RequestAdmission;
use crate::ports::{
    BoxFuture, ControlPayload, ControlPayloadRef, ControlPlaneTransport, Failure,
    OfficeCallOptions, OfficeFailure, OfficeTransport, RawReplyHook, MAX_CONTROL_BYTES,
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
const OP_ID: &str = "01a1c554-5200-7000-8000-000000000009";
const PEER: &str = "192.0.2.42:51000";

type ControlHandler = dyn Fn(&str, &Value) -> Result<Value, Failure> + Send + Sync;

/// Pure in-memory [`ControlPlaneTransport`] double — zero sockets, zero threads.
struct InMemoryControlPlane {
    instance: String,
    generation: u64,
    live: AtomicBool,
    calls: Mutex<Vec<(String, Value)>>,
    handler: Arc<ControlHandler>,
}

impl InMemoryControlPlane {
    fn new(
        handler: impl Fn(&str, &Value) -> Result<Value, Failure> + Send + Sync + 'static,
    ) -> Self {
        Self {
            instance: INSTANCE.to_owned(),
            generation: 7,
            live: AtomicBool::new(true),
            calls: Mutex::new(Vec::new()),
            handler: Arc::new(handler),
        }
    }

    fn methods(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .map(|(method, _)| method.clone())
            .collect()
    }

    fn dispatch(
        &self,
        method: &'static str,
        params_json: Zeroizing<Vec<u8>>,
        hook: Option<RawReplyHook>,
    ) -> Result<ControlPayload, Failure> {
        if !self.live.load(Ordering::SeqCst) {
            return Err(Failure::Unavailable);
        }
        let params: Value =
            serde_json::from_slice(&params_json).map_err(|_| Failure::Unavailable)?;
        self.calls
            .lock()
            .unwrap()
            .push((method.to_owned(), params.clone()));
        match (self.handler)(method, &params) {
            Ok(result) => {
                let bytes =
                    Zeroizing::new(serde_json::to_vec(&result).map_err(|_| Failure::Unavailable)?);
                if let Some(hook) = hook {
                    hook(&Ok(ControlPayloadRef::Result(&bytes)));
                }
                Ok(ControlPayload::Result(bytes))
            }
            Err(error) => {
                if let Some(hook) = hook {
                    hook(&Err(error.clone()));
                }
                Err(error)
            }
        }
    }
}

impl ControlPlaneTransport for InMemoryControlPlane {
    fn instance(&self) -> &str {
        &self.instance
    }

    fn generation(&self) -> u64 {
        self.generation
    }

    fn live(&self) -> bool {
        self.live.load(Ordering::SeqCst)
    }

    fn close(&self) {
        self.live.store(false, Ordering::SeqCst);
    }

    fn call_raw(
        &self,
        method: &'static str,
        params_json: Zeroizing<Vec<u8>>,
        hook: Option<RawReplyHook>,
    ) -> Result<ControlPayload, Failure> {
        self.dispatch(method, params_json, hook)
    }

    fn call_raw_async(
        &self,
        method: &'static str,
        params_json: Zeroizing<Vec<u8>>,
        hook: Option<RawReplyHook>,
    ) -> BoxFuture<'_, Result<ControlPayload, Failure>> {
        let outcome = self.dispatch(method, params_json, hook);
        Box::pin(async move { outcome })
    }
}

type OfficeCallHandler = dyn Fn(&BrowserAuth, &str, &'static str, &Value, OfficeCallOptions) -> Result<Value, OfficeFailure>
    + Send
    + Sync;
type OfficeMutationHandler =
    dyn Fn(&BrowserAuth, &str, &str, &Value) -> Result<Option<Value>, OfficeFailure> + Send + Sync;

/// Pure in-memory [`OfficeTransport`] double — zero Unix channels, zero host fd transfer.
struct InMemoryOfficeTransport {
    recorded_calls: Mutex<Vec<(String, String, OfficeCallOptions)>>,
    recorded_mutations: Mutex<Vec<(String, String, Value)>>,
    call_handler: Arc<OfficeCallHandler>,
    mutation_handler: Arc<OfficeMutationHandler>,
}

impl InMemoryOfficeTransport {
    fn new(
        call_handler: impl Fn(
                &BrowserAuth,
                &str,
                &'static str,
                &Value,
                OfficeCallOptions,
            ) -> Result<Value, OfficeFailure>
            + Send
            + Sync
            + 'static,
        mutation_handler: impl Fn(&BrowserAuth, &str, &str, &Value) -> Result<Option<Value>, OfficeFailure>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        Self {
            recorded_calls: Mutex::new(Vec::new()),
            recorded_mutations: Mutex::new(Vec::new()),
            call_handler: Arc::new(call_handler),
            mutation_handler: Arc::new(mutation_handler),
        }
    }
}

impl OfficeTransport for InMemoryOfficeTransport {
    fn call_raw<'a>(
        &'a self,
        auth: &'a BrowserAuth,
        office_id: &'a str,
        method: &'static str,
        params: Value,
        options: OfficeCallOptions,
    ) -> BoxFuture<'a, Result<Value, OfficeFailure>> {
        self.recorded_calls.lock().unwrap().push((
            office_id.to_owned(),
            method.to_owned(),
            options,
        ));
        let outcome = (self.call_handler)(auth, office_id, method, &params, options);
        Box::pin(async move { outcome })
    }

    fn dispatch_mutation_raw<'a>(
        &'a self,
        auth: &'a BrowserAuth,
        office_id: &'a str,
        method: &'a str,
        params: Value,
    ) -> BoxFuture<'a, Result<Option<Value>, OfficeFailure>> {
        self.recorded_mutations.lock().unwrap().push((
            office_id.to_owned(),
            method.to_owned(),
            params.clone(),
        ));
        let outcome = (self.mutation_handler)(auth, office_id, method, &params);
        Box::pin(async move { outcome })
    }
}

/// Custom [`EventBus`] backed purely by `mpsc` channels (never touches `broadcast::Sender`).
/// Supports deterministic injection of [`EventRecvError::Lagged`] and records published events.
#[derive(Default)]
struct ChannelEventBus {
    subscribers: Mutex<Vec<mpsc::Sender<Result<HubEvent, EventRecvError>>>>,
    published: Mutex<Vec<HubEvent>>,
}

impl ChannelEventBus {
    fn inject_lag(&self, skipped: u64) {
        let mut subs = self.subscribers.lock().unwrap();
        subs.retain(|tx| tx.try_send(Err(EventRecvError::Lagged(skipped))).is_ok());
    }

    fn published_count(&self) -> usize {
        self.published.lock().unwrap().len()
    }
}

impl EventBus for ChannelEventBus {
    fn publish(&self, event: HubEvent) {
        self.published.lock().unwrap().push(event.clone());
        let mut subs = self.subscribers.lock().unwrap();
        subs.retain(|tx| tx.try_send(Ok(event.clone())).is_ok());
    }

    fn subscribe(&self) -> EventSubscription {
        let (tx, rx) = mpsc::channel(64);
        self.subscribers.lock().unwrap().push(tx);
        EventSubscription::from_channel(rx)
    }
}

fn session_record(id: &str) -> Value {
    json!({
        "session_id": id,
        "account": {"id": "A1", "login": "alex"},
        "admin": true,
        "orgs": [{
            "org_id": ORG_ID,
            "employee_id": "0193a5c0-0000-7000-8000-000000000006",
            "office_ids": [OFFICE_ID]
        }],
        "server_time_ms": 1_794_312_000_000u64,
        "idle_expires_at_ms": u64::MAX / 2
    })
}

fn organization_list_result() -> Value {
    json!({
        "creating": [],
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
        }]
    })
}

fn build_gateway(
    host: Arc<InMemoryControlPlane>,
    offices: Arc<InMemoryOfficeTransport>,
    events: Arc<ChannelEventBus>,
) -> WebService<SessionApi> {
    let origin = ListenOrigin::parse(ORIGIN).unwrap();
    let files =
        BuildFiles::from_files([("index.html".into(), b"<script>0</script>".to_vec())]).unwrap();
    let api = SessionApi::from_ports(
        host,
        offices,
        events,
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

fn http_request(
    method: Method,
    uri: &str,
    cookie_and_proof: Option<(&str, &str)>,
    body: Option<Value>,
) -> Request<Full<Bytes>> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", HOST)
        .header("origin", ORIGIN)
        .header("sec-fetch-site", "same-origin");
    if let Some((cookie, proof)) = cookie_and_proof {
        builder = builder
            .header(header::COOKIE, cookie)
            .header(PROOF_HEADER, proof);
    }
    let payload = match body {
        Some(value) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Bytes::from(serde_json::to_vec(&value).unwrap())
        }
        None => Bytes::new(),
    };
    let mut req = builder.body(Full::new(payload)).unwrap();
    req.extensions_mut()
        .insert(PeerAddress(PEER.parse::<SocketAddr>().unwrap()));
    req
}

async fn read_json(
    response: hyper::Response<crate::sse::WebBody>,
) -> (StatusCode, Value, Option<String>) {
    let status = response.status();
    let set_cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .map(str::to_owned);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap();
    (status, json, set_cookie)
}

#[test]
fn in_memory_ports_drive_auth_snapshot_revalidation_and_org_mutation_without_unix_sockets() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let host = Arc::new(InMemoryControlPlane::new(|method, _params| match method {
            "session.sign_in" | "session.validate" => Ok(session_record("S-inmem")),
            "instance.view" => Ok(json!({
                "name": "studio",
                "status": "service",
                "web_status": "running",
                "administrator": "present",
                "auth_enabled": true
            })),
            "organization.list" => Ok(organization_list_result()),
            _ => Err(Failure::Unavailable),
        }));

        let snapshot_attempts = Arc::new(AtomicUsize::new(0));
        let attempts = Arc::clone(&snapshot_attempts);
        let offices = Arc::new(InMemoryOfficeTransport::new(
            move |_auth, _office_id, method, _params, _opts| match method {
                "org.snapshot" => {
                    // First attempt simulates a stale office grant -> triggers session revalidation.
                    if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                        return Err(OfficeFailure::Refused(org_proto::ApiError::new(
                            org_proto::ErrorCode::NotAuthorized,
                            "stale grant",
                        )));
                    }
                    Ok(json!({
                        "organization": {
                            "id": ORG_ID,
                            "name": "Acme",
                            "slug": "acme",
                            "created_at": "2026-11-10T12:00:00.000Z",
                            "version": 1
                        },
                        "teams": [],
                        "members": [],
                        "position": { "epoch": 1, "seq": 4 }
                    }))
                }
                _ => Err(OfficeFailure::Transport),
            },
            |_auth, _office_id, method, _params| {
                assert_eq!(method, "team.create");
                Ok(Some(json!({
                    "id": 9,
                    "result": {
                        "operation_id": OP_ID,
                        "status": "committed",
                        "position": { "epoch": 1, "seq": 5 },
                        "result": {
                            "team": {
                                "id": "0193a5c0-0000-7000-8000-000000000099",
                                "name": "Core",
                                "parent_team_id": null,
                                "lead_member_id": null,
                                "version": 1
                            }
                        }
                    }
                })))
            },
        ));
        let events = Arc::new(ChannelEventBus::default());
        let gateway = build_gateway(Arc::clone(&host), Arc::clone(&offices), Arc::clone(&events));

        // 1. Sign in: exercises ControlPlaneTransport::call_raw + reader-thread caching hook.
        let (status, body, cookie) = read_json(
            gateway
                .respond(http_request(
                    Method::POST,
                    "/api/v1/session/sign-in",
                    None,
                    Some(json!({"login": "alex", "password": "secret-password-value"})),
                ))
                .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let cookie = cookie.expect("sign-in sets session cookie");
        let proof = body["result"]["proof"].as_str().unwrap().to_owned();

        // 2. GET /api/v1/session hits the in-memory session cache seeded by the sign-in hook!
        let (status, body, _) = read_json(
            gateway
                .respond(http_request(
                    Method::GET,
                    "/api/v1/session",
                    Some((&cookie, &proof)),
                    None,
                ))
                .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["result"]["account"]["login"], "alex");
        assert_eq!(host.methods(), vec!["session.sign_in"]);

        // 3. GET /api/v1/organizations/{org}/snapshot: stale grant triggers revalidate + retry.
        let (status, body, _) = read_json(
            gateway
                .respond(http_request(
                    Method::GET,
                    &format!("/api/v1/organizations/{ORG_ID}/snapshot"),
                    Some((&cookie, &proof)),
                    None,
                ))
                .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["result"]["resume_cursor"], format!("org:{ORG_ID}:1:4"));
        assert_eq!(snapshot_attempts.load(Ordering::SeqCst), 2);
        assert!(
            host.methods().iter().any(|m| m == "session.validate"),
            "stale office grant must trigger host session revalidation: {:?}",
            host.methods()
        );

        // 4. POST /api/v1/organizations/{org}/teams dispatches through OfficeTransport and
        //    publishes HubEvent::OrganizationChanged onto the custom EventBus.
        let (status, body, _) = read_json(
            gateway
                .respond(http_request(
                    Method::POST,
                    &format!("/api/v1/organizations/{ORG_ID}/teams"),
                    Some((&cookie, &proof)),
                    Some(json!({
                        "operation_id": OP_ID,
                        "scope": { "kind": "organization", "instance_id": INSTANCE, "org_id": ORG_ID },
                        "params": {
                            "team_id": "0193a5c0-0000-7000-8000-000000000099",
                            "name": "Core"
                        }
                    })),
                ))
                .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["outcome"], "completed");
        assert_eq!(events.published_count(), 1);
        assert_eq!(offices.recorded_mutations.lock().unwrap().len(), 1);
    });
}

#[test]
fn in_memory_event_bus_and_office_transport_drive_sse_push_lag_resync_and_revocation() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let host = Arc::new(InMemoryControlPlane::new(|method, _params| match method {
            "session.sign_in" | "session.validate" => Ok(session_record("S-sse-1")),
            "organization.list" => Ok(organization_list_result()),
            _ => Err(Failure::Unavailable),
        }));

        // Queue one immediate bootstrap poll carrying a live message state change + agent activity,
        // then deny gracefully on subsequent polls while EventBus drives lag + revocation.
        let poll_batch = org_proto::chat::ChatEventsPollResult {
            org_head_seq: 2,
            live_head_seq: 3,
            presence_head_seq: 5,
            snapshot_required: false,
            live_lost: false,
            org_changes: Vec::new(),
            messages_received: Vec::new(),
            message_states: vec![org_proto::msg::MessageStateChangedData {
                id: "m-1".into(),
                thread_id: "t-1".into(),
                to_member_id: "e-2".into(),
                state: org_proto::msg::MessageLifecycleState::Read,
                triage_gen: 1,
                snoozed_until: None,
                snooze_after_task: false,
                reason: None,
                expired_waiting_person: false,
                updated_at: "2026-11-27T10:00:00Z".into(),
            }],
            presence_changes: Vec::new(),
            activity_changes: vec![org_proto::sessions::MemberActivityChangedData {
                member_id: "ada".into(),
                phase: Some(org_proto::sessions::ActivityPhase::RunningTool),
                active_tool_name: Some("Bash".into()),
                active_tool_started_at: None,
                supports_steer: true,
                changed_at: "2026-11-27T10:00:00Z".into(),
            }],
            inbox_changes: Vec::new(),
        };
        let polls = Arc::new(Mutex::new(VecDeque::from([
            Ok(serde_json::to_value(poll_batch).unwrap()),
            Err(OfficeFailure::Refused(org_proto::ApiError::new(
                org_proto::ErrorCode::NotAuthorized,
                "stop polling after bootstrap batch",
            ))),
        ])));
        let seen_wait_ms = Arc::new(Mutex::new(Vec::<Option<u64>>::new()));
        let poll_queue = Arc::clone(&polls);
        let wait_log = Arc::clone(&seen_wait_ms);
        let offices = Arc::new(InMemoryOfficeTransport::new(
            move |_auth, _office_id, method, params, _opts| {
                assert_eq!(method, "chat.events_poll");
                wait_log
                    .lock()
                    .unwrap()
                    .push(params.get("wait_ms").and_then(Value::as_u64));
                poll_queue
                    .lock()
                    .unwrap()
                    .pop_front()
                    .unwrap_or(Err(OfficeFailure::Transport))
            },
            |_auth, _office_id, _method, _params| Ok(None),
        ));
        let events = Arc::new(ChannelEventBus::default());
        let gateway = build_gateway(Arc::clone(&host), Arc::clone(&offices), Arc::clone(&events));

        let (status, body, cookie) = read_json(
            gateway
                .respond(http_request(
                    Method::POST,
                    "/api/v1/session/sign-in",
                    None,
                    Some(json!({"login": "alex", "password": "secret-password-value"})),
                ))
                .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let cookie = cookie.unwrap();
        let proof = body["result"]["proof"].as_str().unwrap().to_owned();

        let response = gateway
            .respond(http_request(
                Method::POST,
                "/api/v1/events/stream",
                Some((&cookie, &proof)),
                Some(json!({
                    "subscriptions": [
                        {
                            "scope": { "kind": "organization", "instance_id": INSTANCE, "org_id": ORG_ID },
                            "feed": "organization"
                        },
                        {
                            "scope": { "kind": "organization", "instance_id": INSTANCE, "org_id": ORG_ID },
                            "feed": "messaging"
                        },
                        {
                            "scope": { "kind": "organization", "instance_id": INSTANCE, "org_id": ORG_ID },
                            "feed": "presence"
                        }
                    ]
                })),
            ))
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let mut stream = response.into_body();

        async fn next_frame(body: &mut crate::sse::WebBody) -> String {
            let chunk = tokio::time::timeout(Duration::from_secs(3), body.frame())
                .await
                .expect("frame timeout")
                .expect("stream ended unexpectedly")
                .unwrap()
                .into_data()
                .unwrap();
            String::from_utf8(chunk.to_vec()).unwrap()
        }

        let ready = next_frame(&mut stream).await;
        assert!(ready.contains("\"type\":\"ready\""), "{ready}");

        // Next two frames arrive from the in-memory OfficeTransport's chat.events_poll push loop.
        let msg = next_frame(&mut stream).await;
        assert!(msg.contains("\"message.state_changed\""), "{msg}");
        let act = next_frame(&mut stream).await;
        assert!(act.contains("\"member.activity_changed\""), "{act}");

        // Verify TASK-140 push-poll cadence: first poll immediate (None), second long-wait (10_000).
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(*seen_wait_ms.lock().unwrap(), vec![None, Some(10_000)]);

        // Publish a live org change on the custom EventBus (zero broadcast::Sender involved).
        events.publish(HubEvent::OrganizationChanged {
            org_id: ORG_ID.to_owned(),
            cursor: format!("org:{ORG_ID}:1:9"),
            event: "team.created".to_owned(),
            data: json!({ "operation_id": OP_ID }),
        });
        let pushed = next_frame(&mut stream).await;
        assert!(pushed.contains("\"team.created\""), "{pushed}");

        // Deterministically trigger slow-subscriber lag -> resync(slow_consumer) for all 3 feeds.
        events.inject_lag(12);
        let mut resync_feeds = Vec::new();
        for _ in 0..3 {
            let frame = next_frame(&mut stream).await;
            assert!(frame.contains("\"type\":\"resync\""), "{frame}");
            assert!(frame.contains("\"slow_consumer\""), "{frame}");
            resync_feeds.push(frame);
        }
        assert!(resync_feeds.iter().any(|f| f.contains("\"organization\"")));
        assert!(resync_feeds.iter().any(|f| f.contains("\"messaging\"")));
        assert!(resync_feeds.iter().any(|f| f.contains("\"presence\"")));

        // Session revocation emits session_ended and closes the stream gracefully.
        events.publish(HubEvent::SessionEnded {
            session_id: "S-sse-1".to_owned(),
        });
        let ended = next_frame(&mut stream).await;
        assert!(ended.contains("\"type\":\"session_ended\""), "{ended}");
        assert!(
            tokio::time::timeout(Duration::from_secs(2), stream.frame())
                .await
                .unwrap()
                .is_none(),
            "revoked SSE stream must close cleanly"
        );
    });
}

#[test]
fn control_plane_and_office_ports_enforce_bounds_desync_close_and_isolated_options() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        // 1. Mismatched wire reply id closes the ControlPlaneTransport.
        struct DesyncHost(AtomicBool);
        impl ControlPlaneTransport for DesyncHost {
            fn instance(&self) -> &str {
                INSTANCE
            }
            fn generation(&self) -> u64 {
                1
            }
            fn live(&self) -> bool {
                self.0.load(Ordering::SeqCst)
            }
            fn close(&self) {
                self.0.store(false, Ordering::SeqCst);
            }
            fn call_raw(
                &self,
                _method: &'static str,
                _params_json: Zeroizing<Vec<u8>>,
                _hook: Option<RawReplyHook>,
            ) -> Result<ControlPayload, Failure> {
                Ok(ControlPayload::Frame {
                    id: 1,
                    bytes: Zeroizing::new(br#"{"id":99,"result":{}}"#.to_vec()),
                })
            }
            fn call_raw_async(
                &self,
                method: &'static str,
                params_json: Zeroizing<Vec<u8>>,
                hook: Option<RawReplyHook>,
            ) -> BoxFuture<'_, Result<ControlPayload, Failure>> {
                let res = self.call_raw(method, params_json, hook);
                Box::pin(async move { res })
            }
        }
        let desync: Arc<dyn ControlPlaneTransport> = Arc::new(DesyncHost(AtomicBool::new(true)));
        assert!(desync.live());
        assert_eq!(
            desync.request::<org_host_control::ops::WebServing>(&Default::default()),
            Err(Failure::Unavailable)
        );
        assert!(
            !desync.live(),
            "reply id mismatch must close the ControlPlaneTransport"
        );

        // 2. Oversized control params are rejected before touching the transport.
        let oversized = "x".repeat(MAX_CONTROL_BYTES);
        assert_eq!(
            super::encode_control_params(&oversized).err(),
            Some(Failure::Unavailable)
        );

        // 3. OfficeTransport::call_typed_isolated forwards OfficeCallOptions::isolated().
        let office = Arc::new(InMemoryOfficeTransport::new(
            |_auth, _office_id, method, _params, opts| {
                assert_eq!(method, "cost.subscription_usage.collect");
                assert!(opts.isolated);
                assert!(!opts.retry_on_transport);
                Ok(json!({
                    "snapshots": [],
                    "collected_at": "2026-11-27T10:00:00Z"
                }))
            },
            |_auth, _office_id, _method, _params| Ok(None),
        ));
        let port: Arc<dyn OfficeTransport> = office.clone();
        let auth = BrowserAuth::local_authless(org_host_control::browser::BrowserOrigin {
            scheme: "http".into(),
            host: "127.0.0.1".into(),
            port: 8080,
        });
        let result = port
            .call_typed_isolated::<CostSubscriptionUsageCollect>(
                &auth,
                OFFICE_ID,
                &CostSubscriptionUsageCollectParams::default(),
            )
            .await
            .unwrap();
        assert!(result.snapshots.is_empty());
        assert_eq!(office.recorded_calls.lock().unwrap().len(), 1);
    });
}
