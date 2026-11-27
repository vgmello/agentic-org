//! Zero-Unix-socket BFF tests for:
//! - `GET  /api/v1/organizations/{org}/copilot/features`
//! - `POST /api/v1/organizations/{org}/copilot/draft`
//! - `GET  /api/v1/account/preferences`
//! - `PATCH /api/v1/account/preferences` and `POST /api/v1/account/preferences`

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use http_body_util::{BodyExt, Full};
use hyper::{body::Bytes, header, Method, Request, StatusCode};
use org_host_control::listen::{ListenMode, ListenOrigin};
use org_host_control::organization::BrowserAuth;
use org_proto::{ApiError, ErrorCode};
use serde_json::{json, Value};
use zeroize::Zeroizing;

use super::COPILOT_DRAFT_DEADLINE;
use crate::api::SessionApi;
use crate::assets::BuildFiles;
use crate::event_hub::{EventBus, InMemoryEventBus};
use crate::ingress::RequestAdmission;
use crate::ports::{
    BoxFuture, ControlPayload, ControlPlaneTransport, Failure, OfficeCallOptions, OfficeFailure,
    OfficeTransport, RawReplyHook,
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
const SESSION_ID: &str = "sess-copilot-1";
const PEER: &str = "192.0.2.50:51000";

struct FakeControlPlane {
    prefs: Mutex<(Option<String>, Option<String>)>,
}

impl FakeControlPlane {
    fn new() -> Self {
        Self {
            prefs: Mutex::new((Some("UTC".to_owned()), Some(ORG_ID.to_owned()))),
        }
    }

    fn handle(&self, method: &str, params: &Value) -> Result<Value, Failure> {
        match method {
            "session.sign_in" | "session.validate" => {
                let (tz, org) = self.prefs.lock().unwrap().clone();
                Ok(json!({
                    "session_id": SESSION_ID,
                    "account": {
                        "id": "acc-1",
                        "login": "owner",
                        "time_zone": tz,
                        "last_org_id": org
                    },
                    "admin": true,
                    "orgs": [{
                        "org_id": ORG_ID,
                        "employee_id": "owner-1",
                        "office_ids": [OFFICE_ID]
                    }],
                    "server_time_ms": 1_794_312_000_000_u64,
                    "idle_expires_at_ms": u64::MAX / 2
                }))
            }
            "browser.account.preferences.set" => {
                let mut prefs = self.prefs.lock().unwrap();
                if params["clear_time_zone"].as_bool() == Some(true) {
                    prefs.0 = None;
                } else if let Some(tz) = params["time_zone"].as_str() {
                    prefs.0 = Some(tz.to_owned());
                }
                if params["clear_last_org_id"].as_bool() == Some(true) {
                    prefs.1 = None;
                } else if let Some(org) = params["last_org_id"].as_str() {
                    if org != ORG_ID {
                        return Err(Failure::Denied);
                    }
                    prefs.1 = Some(org.to_owned());
                }
                Ok(json!({
                    "time_zone": prefs.0,
                    "last_org_id": prefs.1,
                }))
            }
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
            other => panic!("unexpected host method `{other}`"),
        }
    }
}

impl ControlPlaneTransport for FakeControlPlane {
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
        params_json: Zeroizing<Vec<u8>>,
        hook: Option<RawReplyHook>,
    ) -> Result<ControlPayload, Failure> {
        let params: Value = serde_json::from_slice(&params_json).unwrap_or(Value::Null);
        let res = self.handle(method, &params);
        let payload = res.and_then(|v| ControlPayload::from_result(&v));
        if let Some(hook) = hook {
            match &payload {
                Ok(ControlPayload::Result(bytes)) => {
                    hook(&Ok(crate::ports::ControlPayloadRef::Result(bytes)));
                }
                Ok(ControlPayload::Frame { id, bytes }) => {
                    hook(&Ok(crate::ports::ControlPayloadRef::Frame {
                        id: *id,
                        bytes,
                    }));
                }
                Err(err) => hook(&Err(err.clone())),
            }
        }
        payload
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

struct RecordedCall {
    method: &'static str,
    params: Value,
    options: OfficeCallOptions,
}

struct FakeOffice {
    replies: Mutex<VecDeque<Result<Value, OfficeFailure>>>,
    calls: Mutex<Vec<RecordedCall>>,
}

impl FakeOffice {
    fn new(replies: Vec<Result<Value, OfficeFailure>>) -> Self {
        Self {
            replies: Mutex::new(replies.into()),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn take_calls(&self) -> Vec<RecordedCall> {
        std::mem::take(&mut *self.calls.lock().unwrap())
    }
}

impl OfficeTransport for FakeOffice {
    fn call_raw<'a>(
        &'a self,
        _auth: &'a BrowserAuth,
        _office_id: &'a str,
        method: &'static str,
        params: Value,
        options: OfficeCallOptions,
    ) -> BoxFuture<'a, Result<Value, OfficeFailure>> {
        self.calls.lock().unwrap().push(RecordedCall {
            method,
            params,
            options,
        });
        let reply = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Err(OfficeFailure::Transport));
        Box::pin(async move { reply })
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
}

fn build_service(office: Arc<FakeOffice>) -> (WebService<SessionApi>, Arc<FakeControlPlane>) {
    let origin = ListenOrigin::parse(ORIGIN).unwrap();
    let host = Arc::new(FakeControlPlane::new());
    let events: Arc<dyn EventBus> = Arc::new(InMemoryEventBus::default());
    let cache = Arc::new(SessionCache::default());
    let api = SessionApi::from_ports(
        Arc::clone(&host) as Arc<dyn ControlPlaneTransport>,
        office as Arc<dyn OfficeTransport>,
        events,
        cache,
        &origin,
        ListenMode::HttpLoopback,
        Arc::new(AtomicBool::new(true)),
    );
    let files =
        BuildFiles::from_files([("index.html".into(), b"<script>0</script>".to_vec())]).unwrap();
    let ui = StaticUi::new(files, origin.clone(), Hsts::Off);
    let admission = RequestAdmission::new(origin);
    let svc = WebService::new(ui, api, ApiLimits::default(), admission).unwrap();
    (svc, host)
}

struct Reply {
    status: StatusCode,
    body: Value,
    cookie: Option<String>,
}

async fn send(
    svc: &WebService<SessionApi>,
    method: Method,
    path: &str,
    body: &str,
    auth: Option<(&str, &str)>,
) -> Reply {
    let mut builder = Request::builder()
        .method(method.clone())
        .uri(path)
        .header(header::HOST, HOST);
    if matches!(method, Method::POST | Method::PATCH) {
        builder = builder
            .header(header::ORIGIN, ORIGIN)
            .header(header::CONTENT_TYPE, "application/json");
    }
    if let Some((cookie, proof)) = auth {
        builder = builder
            .header(header::COOKIE, cookie)
            .header(PROOF_HEADER, proof);
    }
    let mut req = builder
        .body(Full::new(Bytes::from(body.to_owned())))
        .unwrap();
    req.extensions_mut()
        .insert(PeerAddress(PEER.parse::<SocketAddr>().unwrap()));
    let resp = svc.respond(req).await;
    let status = resp.status();
    let cookie = resp
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(';').next().unwrap_or(s).to_owned());
    let raw = resp.into_body().collect().await.unwrap().to_bytes();
    let parsed = serde_json::from_slice(&raw).unwrap_or(Value::Null);
    Reply {
        status,
        body: parsed,
        cookie,
    }
}

async fn sign_in(svc: &WebService<SessionApi>) -> (String, String) {
    let r = send(
        svc,
        Method::POST,
        "/api/v1/session/sign-in",
        r#"{"login":"owner","password":"correct-horse-battery-staple"}"#,
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let cookie = r.cookie.expect("set-cookie");
    let proof = r.body["result"]["proof"]
        .as_str()
        .expect("proof")
        .to_owned();
    (cookie, proof)
}

#[test]
fn copilot_routes_enforce_auth_and_input_bounds() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let office = Arc::new(FakeOffice::new(vec![]));
        let (svc, _) = build_service(Arc::clone(&office));
        let features_url = format!("/api/v1/organizations/{ORG_ID}/copilot/features");
        let draft_url = format!("/api/v1/organizations/{ORG_ID}/copilot/draft");

        // Unauthenticated requests fail closed with 401 before touching office.
        let unauth_f = send(&svc, Method::GET, &features_url, "", None).await;
        assert_eq!(unauth_f.status, StatusCode::UNAUTHORIZED);
        let unauth_d = send(
            &svc,
            Method::POST,
            &draft_url,
            r#"{"feature":"text.improve","input":{"mode":"improve","field":"Body","text":"hi"}}"#,
            None,
        )
        .await;
        assert_eq!(unauth_d.status, StatusCode::UNAUTHORIZED);
        assert!(office.take_calls().is_empty());

        let (cookie, proof) = sign_in(&svc).await;
        let auth = Some((cookie.as_str(), proof.as_str()));

        // Malformed bodies, unknown fields, and >8 context items fail before reaching office.
        for bad_body in [
            r#"{"feature":"","input":{}}"#,
            r#"{"feature":"text.improve","input":"not-an-object"}"#,
            r#"{"feature":"text.improve","input":{},"extra":1}"#,
            r#"{"feature":"BAD FEATURE!","input":{}}"#,
        ] {
            let bad = send(&svc, Method::POST, &draft_url, bad_body, auth).await;
            assert_eq!(bad.status, StatusCode::BAD_REQUEST, "{bad_body}");
        }
        let too_many_ctx = json!({
            "feature": "bug.from_sentence",
            "input": { "sentence": "crash" },
            "context": (0..9)
                .map(|i| json!({"kind":"area","label":format!("A{i}"),"text":"a"}))
                .collect::<Vec<_>>()
        })
        .to_string();
        let ctx_err = send(&svc, Method::POST, &draft_url, &too_many_ctx, auth).await;
        assert_eq!(ctx_err.status, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(ctx_err.body["error"]["code"], "too_large");
        assert!(office.take_calls().is_empty());
    });
}

fn copilot_scripted_replies() -> Vec<Result<Value, OfficeFailure>> {
    vec![
        Ok(json!({
            "features": [
                { "id": "text.improve", "version": 1, "title": "Improve text" },
                { "id": "bug.from_sentence", "version": 2, "title": "Draft bug report" }
            ]
        })),
        Ok(json!({
            "draft": { "text": "Sharper description." },
            "notes": ["shortened"]
        })),
        Ok(json!({
            "draft": {
                "title": "Crash on save",
                "what_happened": "App closes when clicking Save.",
                "severity": "major"
            },
            "notes": []
        })),
        Err(OfficeFailure::Refused(ApiError::new(
            ErrorCode::CopilotUnavailable,
            "no copilot runtime is set up",
        ))),
        Err(OfficeFailure::Refused(ApiError::new(
            ErrorCode::CopilotBusy,
            "a run of this feature is still going",
        ))),
        Err(OfficeFailure::Refused(ApiError::new(
            ErrorCode::CopilotFailed,
            "the answer looked like it held a secret",
        ))),
        Err(OfficeFailure::Refused(ApiError::new(
            ErrorCode::Timeout,
            "the copilot took too long",
        ))),
        Err(OfficeFailure::Refused(ApiError::new(
            ErrorCode::TooLarge,
            "the input is too large",
        ))),
    ]
}

#[test]
fn copilot_routes_wire_features_draft_and_error_mapping() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let office = Arc::new(FakeOffice::new(copilot_scripted_replies()));
        let (svc, _) = build_service(Arc::clone(&office));
        let features_url = format!("/api/v1/organizations/{ORG_ID}/copilot/features");
        let draft_url = format!("/api/v1/organizations/{ORG_ID}/copilot/draft");
        let (cookie, proof) = sign_in(&svc).await;
        let auth = Some((cookie.as_str(), proof.as_str()));

        // `GET copilot/features` returns live features.
        let feats = send(&svc, Method::GET, &features_url, "", auth).await;
        assert_eq!(feats.status, StatusCode::OK, "{}", feats.body);
        assert_eq!(
            feats.body["result"]["features"].as_array().unwrap().len(),
            2
        );

        // `POST copilot/draft` for `text.improve` and `bug.from_sentence` uses 65s deadline.
        let text_resp = send(
            &svc,
            Method::POST,
            &draft_url,
            r#"{"feature":"text.improve","input":{"mode":"improve","field":"Summary","text":"make better"}}"#,
            auth,
        )
        .await;
        assert_eq!(text_resp.status, StatusCode::OK, "{}", text_resp.body);
        assert_eq!(text_resp.body["result"]["draft"]["text"], "Sharper description.");

        let bug_resp = send(
            &svc,
            Method::POST,
            &draft_url,
            r#"{"feature":"bug.from_sentence","input":{"sentence":"app crashes on save"},"context":[{"kind":"area","label":"Files","text":"area-files"}]}"#,
            auth,
        )
        .await;
        assert_eq!(bug_resp.status, StatusCode::OK, "{}", bug_resp.body);
        assert_eq!(bug_resp.body["result"]["draft"]["title"], "Crash on save");

        let recorded = office.take_calls();
        assert_eq!(recorded.len(), 3);
        assert_eq!(recorded[0].method, "copilot.features");
        assert_eq!(recorded[1].method, "copilot.draft");
        assert_eq!(recorded[1].options.deadline, COPILOT_DRAFT_DEADLINE);
        assert!(!recorded[1].options.retry_on_transport);
        assert_eq!(recorded[2].method, "copilot.draft");
        assert_eq!(recorded[2].params["feature"], "bug.from_sentence");

        // Error mapping preserves exact copilot error codes and HTTP status classes.
        for (status, code) in [
            (StatusCode::SERVICE_UNAVAILABLE, "copilot_unavailable"),
            (StatusCode::TOO_MANY_REQUESTS, "copilot_busy"),
            (StatusCode::SERVICE_UNAVAILABLE, "copilot_failed"),
            (StatusCode::SERVICE_UNAVAILABLE, "timeout"),
            (StatusCode::PAYLOAD_TOO_LARGE, "too_large"),
        ] {
            let err = send(
                &svc,
                Method::POST,
                &draft_url,
                r#"{"feature":"text.improve","input":{"mode":"improve","field":"Body","text":"x"}}"#,
                auth,
            )
            .await;
            assert_eq!(err.status, status, "expected {status} for {code}: {}", err.body);
            assert_eq!(err.body["error"]["code"], code, "{}", err.body);
        }
    });
}

#[test]
fn account_preferences_supports_get_patch_post_validation_auth_and_cache_invalidation() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let office = Arc::new(FakeOffice::new(vec![]));
        let (svc, _) = build_service(office);

        // 1. Unauthenticated GET and PATCH fail closed with 401.
        assert_eq!(
            send(&svc, Method::GET, "/api/v1/account/preferences", "", None)
                .await
                .status,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            send(
                &svc,
                Method::PATCH,
                "/api/v1/account/preferences",
                r#"{"time_zone":"America/New_York"}"#,
                None
            )
            .await
            .status,
            StatusCode::UNAUTHORIZED
        );

        let (cookie, proof) = sign_in(&svc).await;
        let auth = Some((cookie.as_str(), proof.as_str()));

        // 2. GET returns initial persisted preferences.
        let initial = send(&svc, Method::GET, "/api/v1/account/preferences", "", auth).await;
        assert_eq!(initial.status, StatusCode::OK, "{}", initial.body);
        assert_eq!(initial.body["result"]["time_zone"], "UTC");
        assert_eq!(initial.body["result"]["last_org_id"], ORG_ID);

        // 3. Strict validation refuses invalid TZ, conflicting clear flags, bad org ID, and unknown fields.
        for bad_body in [
            r#"{"time_zone":"Not/A_Zone"}"#,
            r#"{"time_zone":"UTC","clear_time_zone":true}"#,
            r#"{"last_org_id":"0193a5c0-0000-7000-8000-000000000001","clear_last_org_id":true}"#,
            r#"{"last_org_id":"../evil"}"#,
            r#"{"unknown_pref":true}"#,
        ] {
            let bad = send(
                &svc,
                Method::PATCH,
                "/api/v1/account/preferences",
                bad_body,
                auth,
            )
            .await;
            assert_eq!(bad.status, StatusCode::BAD_REQUEST, "{bad_body}");
        }

        // 4. PATCH updates time_zone and invalidates session cache so GET immediately reflects it.
        let patched = send(
            &svc,
            Method::PATCH,
            "/api/v1/account/preferences",
            r#"{"time_zone":"America/New_York"}"#,
            auth,
        )
        .await;
        assert_eq!(patched.status, StatusCode::OK, "{}", patched.body);
        assert_eq!(patched.body["result"]["time_zone"], "America/New_York");

        let after_patch = send(&svc, Method::GET, "/api/v1/account/preferences", "", auth).await;
        assert_eq!(after_patch.status, StatusCode::OK);
        assert_eq!(after_patch.body["result"]["time_zone"], "America/New_York");

        // 5. POST remains supported for backwards compatibility (clearing time_zone).
        let cleared = send(
            &svc,
            Method::POST,
            "/api/v1/account/preferences",
            r#"{"clear_time_zone":true}"#,
            auth,
        )
        .await;
        assert_eq!(cleared.status, StatusCode::OK, "{}", cleared.body);
        assert!(cleared.body["result"]["time_zone"].is_null());

        let after_clear = send(&svc, Method::GET, "/api/v1/account/preferences", "", auth).await;
        assert!(after_clear.body["result"]["time_zone"].is_null());

        // 6. Unauthorized organization preference returns 403 Forbidden.
        let unauth_org = send(
            &svc,
            Method::PATCH,
            "/api/v1/account/preferences",
            r#"{"last_org_id":"0193a5c0-0000-7000-8000-000000000099"}"#,
            auth,
        )
        .await;
        assert_eq!(
            unauth_org.status,
            StatusCode::FORBIDDEN,
            "{}",
            unauth_org.body
        );
    });
}
