//! Unit and full-gateway HTTP integration tests for BYOA external-auth BFF routes (`TASK-249`).
//!
//! Runs strictly against in-memory [`ControlPlaneTransport`] and [`OfficeTransport`] test doubles
//! (zero Unix sockets, zero disk writes).

use std::future::Future;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use http_body_util::{BodyExt, Full};
use hyper::{body::Bytes, header, Method, Request, StatusCode};
use org_host_control::listen::{ListenMode, ListenOrigin};
use org_host_control::organization::BrowserAuth;
use org_proto::external_auth::{
    ExternalHistoryEntry, ExternalSessionView, MemberExternalPendingResult,
    MemberExternalRevokeResult, MemberExternalSessionsResult, PendingExternalAuthView,
    WorkspaceFamiliarity, MAX_LEASE_TTL_SECS, MIN_LEASE_TTL_SECS,
};
use org_proto::{ApiError, ErrorCode};
use serde_json::{json, Value};
use zeroize::Zeroizing;

use super::calls::mask_code_prefix;
use super::*;
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
const MEMBER_ID: &str = "0193a5c0-0000-7000-8000-000000000050";
const CMD_ID: &str = "0193a5c0-0000-7000-8000-000000000099";
const PEER: &str = "192.0.2.42:51000";

type HostHandler = dyn Fn(&str, &Value) -> Result<Value, Failure> + Send + Sync;
type OfficeHandler = dyn Fn(&BrowserAuth, &str, &'static str, &Value, OfficeCallOptions) -> Result<Value, OfficeFailure>
    + Send
    + Sync;

struct FakeHost {
    calls: Mutex<Vec<(String, Value)>>,
    handler: Arc<HostHandler>,
}

impl FakeHost {
    fn new(
        handler: impl Fn(&str, &Value) -> Result<Value, Failure> + Send + Sync + 'static,
    ) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            handler: Arc::new(handler),
        }
    }

    fn methods(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .map(|(m, _)| m.clone())
            .collect()
    }

    fn dispatch(
        &self,
        method: &'static str,
        params_json: Zeroizing<Vec<u8>>,
        hook: Option<RawReplyHook>,
    ) -> Result<ControlPayload, Failure> {
        let params: Value =
            serde_json::from_slice(&params_json).map_err(|_| Failure::Unavailable)?;
        self.calls
            .lock()
            .unwrap()
            .push((method.to_owned(), params.clone()));
        match (self.handler)(method, &params) {
            Ok(res) => {
                let bytes =
                    Zeroizing::new(serde_json::to_vec(&res).map_err(|_| Failure::Unavailable)?);
                if let Some(hook) = hook {
                    hook(&Ok(ControlPayloadRef::Result(&bytes)));
                }
                Ok(ControlPayload::Result(bytes))
            }
            Err(err) => {
                if let Some(hook) = hook {
                    hook(&Err(err.clone()));
                }
                Err(err)
            }
        }
    }
}

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
        let out = self.dispatch(method, params_json, hook);
        Box::pin(async move { out })
    }
}

struct FakeOffice {
    calls: Mutex<Vec<(String, Value, OfficeCallOptions)>>,
    handler: Arc<OfficeHandler>,
}

impl FakeOffice {
    fn new(
        handler: impl Fn(
                &BrowserAuth,
                &str,
                &'static str,
                &Value,
                OfficeCallOptions,
            ) -> Result<Value, OfficeFailure>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            handler: Arc::new(handler),
        }
    }

    fn recorded(&self) -> Vec<(String, Value, OfficeCallOptions)> {
        self.calls.lock().unwrap().clone()
    }
}

impl OfficeTransport for FakeOffice {
    fn call_raw<'a>(
        &'a self,
        auth: &'a BrowserAuth,
        office_id: &'a str,
        method: &'static str,
        params: Value,
        options: OfficeCallOptions,
    ) -> BoxFuture<'a, Result<Value, OfficeFailure>> {
        self.calls
            .lock()
            .unwrap()
            .push((method.to_owned(), params.clone(), options));
        let out = (self.handler)(auth, office_id, method, &params, options);
        Box::pin(async move { out })
    }

    fn dispatch_mutation_raw<'a>(
        &'a self,
        _auth: &'a BrowserAuth,
        _office_id: &'a str,
        _method: &'a str,
        _params: Value,
    ) -> BoxFuture<'a, Result<Option<Value>, OfficeFailure>> {
        Box::pin(async { Ok(None) })
    }
}

#[derive(Default)]
struct NoopBus;

impl EventBus for NoopBus {
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

fn standard_host() -> Arc<FakeHost> {
    let valid_sessions: Arc<Mutex<Vec<(Value, Value)>>> = Arc::default();
    Arc::new(FakeHost::new(move |method, params| {
        let mut sessions = valid_sessions.lock().unwrap();
        match method {
            "session.sign_in" => {
                sessions.push((
                    params["cookie_digest"].clone(),
                    params["proof_digest"].clone(),
                ));
                Ok(json!({
                    "session_id": "S-human-1",
                    "account": {"id": "A1", "login": "alex"},
                    "admin": true,
                    "orgs": [{
                        "org_id": ORG_ID,
                        "employee_id": "0193a5c0-0000-7000-8000-000000000006",
                        "office_ids": [OFFICE_ID]
                    }],
                    "server_time_ms": 1_794_312_000_000u64,
                    "idle_expires_at_ms": u64::MAX / 2
                }))
            }
            "session.validate" => {
                let key = (
                    params["cookie_digest"].clone(),
                    params["proof_digest"].clone(),
                );
                if sessions.contains(&key) {
                    Ok(json!({
                        "session_id": "S-human-1",
                        "account": {"id": "A1", "login": "alex"},
                        "admin": true,
                        "orgs": [{
                            "org_id": ORG_ID,
                            "employee_id": "0193a5c0-0000-7000-8000-000000000006",
                            "office_ids": [OFFICE_ID]
                        }],
                        "server_time_ms": 1_794_312_000_000u64,
                        "idle_expires_at_ms": u64::MAX / 2
                    }))
                } else {
                    Err(Failure::Denied)
                }
            }
            "organization.list" => Ok(json!({
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
            })),
            _ => Err(Failure::Unavailable),
        }
    }))
}

fn build_gateway(host: Arc<FakeHost>, office: Arc<FakeOffice>) -> WebService<SessionApi> {
    let origin = ListenOrigin::parse(ORIGIN).unwrap();
    let files =
        BuildFiles::from_files([("index.html".into(), b"<script>0</script>".to_vec())]).unwrap();
    let api = SessionApi::from_ports(
        host,
        office,
        Arc::new(NoopBus),
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

fn make_request(
    method: Method,
    uri: &str,
    creds: Option<(&str, &str)>,
    body: Option<&str>,
    origin: Option<&str>,
    extra: &[(&str, &str)],
) -> Request<Full<Bytes>> {
    let mut builder = Request::builder()
        .method(method.clone())
        .uri(uri)
        .header(header::HOST, HOST);
    if let Some(origin) = origin {
        builder = builder.header(header::ORIGIN, origin);
    }
    if let Some((cookie, proof)) = creds {
        builder = builder
            .header(header::COOKIE, cookie)
            .header(PROOF_HEADER, proof);
    }
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    for (name, value) in extra {
        builder = builder.header(*name, *value);
    }
    let payload = body
        .map(|s| Bytes::copy_from_slice(s.as_bytes()))
        .unwrap_or_default();
    let mut req = builder.body(Full::new(payload)).unwrap();
    req.extensions_mut()
        .insert(PeerAddress(PEER.parse::<SocketAddr>().unwrap()));
    req
}

async fn sign_in(gateway: &WebService<SessionApi>) -> (String, String) {
    let req = make_request(
        Method::POST,
        "/api/v1/session/sign-in",
        None,
        Some(r#"{"login":"alex","password":"generated unit credential"}"#),
        Some(ORIGIN),
        &[("sec-fetch-site", "same-origin")],
    );
    let resp = gateway.respond(req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let cookie = resp
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let proof = body["result"]["proof"].as_str().unwrap().to_owned();
    (cookie, proof)
}

async fn collect_json(
    resp: hyper::Response<crate::sse::WebBody>,
) -> (StatusCode, header::HeaderMap, Value) {
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, headers, body)
}

fn sample_pending_view(code_prefix: &str) -> PendingExternalAuthView {
    PendingExternalAuthView {
        request_id: "0193a5c0-0000-7000-8000-000000000701".into(),
        handle: Some("@ada".into()),
        employee_id: MEMBER_ID.into(),
        office_id: OFFICE_ID.into(),
        office_name: "HQ".into(),
        hostname: Some("devbox.local".into()),
        workspace_path: Some("/home/ada/src/app".into()),
        workspace_label: "devbox:~/src/app".into(),
        pubkey_fingerprint: "0123456789abcdef".into(),
        managed_turn_active: true,
        requested_at: "2026-11-27T10:00:00.000Z".into(),
        expires_at: "2026-11-27T10:10:00.000Z".into(),
        code_prefix: code_prefix.into(),
        workspace_familiarity: WorkspaceFamiliarity::ReturningKnownKey,
        last_approved_by: Some("0193a5c0-0000-7000-8000-000000000006".into()),
        last_approved_at: Some("2026-11-26T08:30:00.000Z".into()),
    }
}

fn sample_session_view(took_over_managed: bool) -> ExternalSessionView {
    ExternalSessionView {
        session_id: "ext-0193a5c0-0000-7000-8000-000000000801".into(),
        employee_id: MEMBER_ID.into(),
        handle: Some("ada".into()),
        workspace_label: "devbox:~/src/app".into(),
        pubkey_fingerprint: "0123456789abcdef".into(),
        approved_by: "0193a5c0-0000-7000-8000-000000000006".into(),
        approved_at: "2026-11-27T10:01:00.000Z".into(),
        expires_at: "2026-12-04T10:01:00.000Z".into(),
        revoked_at: None,
        active: true,
        superseded_session_id: None,
        took_over_managed,
    }
}

fn sample_history_entry() -> ExternalHistoryEntry {
    ExternalHistoryEntry {
        at: "2026-11-26T08:30:00.000Z".into(),
        event: "approved".into(),
        session_id: Some("ext-0193a5c0-0000-7000-8000-000000000700".into()),
        actor_id: Some("0193a5c0-0000-7000-8000-000000000006".into()),
        detail: Some("devbox:~/src/app".into()),
        hostname: Some("devbox.local".into()),
        workspace_path: Some("/home/ada/src/app".into()),
        workspace_label: Some("devbox:~/src/app".into()),
        pubkey_fingerprint: Some("0123456789abcdef".into()),
    }
}

#[test]
fn unauthenticated_requests_fail_with_401_on_all_five_endpoints_without_touching_office() {
    run_async(async {
        let host = standard_host();
        let office = Arc::new(FakeOffice::new(|_, _, _, _, _| {
            Err(OfficeFailure::Transport)
        }));
        let gw = build_gateway(Arc::clone(&host), Arc::clone(&office));
        let (cookie, _proof) = sign_in(&gw).await;

        let endpoints = [
            (
                Method::GET,
                format!("/api/v1/organizations/{ORG_ID}/external-auth/pending"),
                None,
            ),
            (
                Method::GET,
                format!("/api/v1/organizations/{ORG_ID}/members/{MEMBER_ID}/external-sessions"),
                None,
            ),
            (
                Method::POST,
                format!("/api/v1/organizations/{ORG_ID}/external-auth/approve"),
                Some(r#"{"code":"K7M9-P4Q2"}"#),
            ),
            (
                Method::POST,
                format!(
                    "/api/v1/organizations/{ORG_ID}/members/{MEMBER_ID}/external-sessions/renew"
                ),
                Some(r#"{}"#),
            ),
            (
                Method::POST,
                format!(
                    "/api/v1/organizations/{ORG_ID}/members/{MEMBER_ID}/external-sessions/revoke"
                ),
                Some(r#"{}"#),
            ),
        ];

        for (method, path, body) in &endpoints {
            // No credentials at all -> 401.
            let resp = gw
                .respond(make_request(
                    method.clone(),
                    path,
                    None,
                    *body,
                    Some(ORIGIN),
                    &[("sec-fetch-site", "same-origin")],
                ))
                .await;
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "{method} {path}");

            // Cookie present without X-Org-Session-Proof -> 401.
            let resp = gw
                .respond(make_request(
                    method.clone(),
                    path,
                    None,
                    *body,
                    Some(ORIGIN),
                    &[
                        ("sec-fetch-site", "same-origin"),
                        ("cookie", cookie.as_str()),
                    ],
                ))
                .await;
            assert_eq!(
                resp.status(),
                StatusCode::UNAUTHORIZED,
                "cookie-only {method} {path}"
            );

            // Cookie with wrong 64-hex proof -> 401.
            let bad_proof = "0".repeat(64);
            let resp = gw
                .respond(make_request(
                    method.clone(),
                    path,
                    Some((cookie.as_str(), bad_proof.as_str())),
                    *body,
                    Some(ORIGIN),
                    &[("sec-fetch-site", "same-origin")],
                ))
                .await;
            assert_eq!(
                resp.status(),
                StatusCode::UNAUTHORIZED,
                "wrong proof {method} {path}"
            );
        }

        assert!(
            office.recorded().is_empty(),
            "unauthenticated requests must never reach the office"
        );
    });
}

#[test]
fn post_endpoints_enforce_csrf_origin_sec_fetch_site_and_json_media_type() {
    run_async(async {
        let host = standard_host();
        let office = Arc::new(FakeOffice::new(|_, _, _, _, _| {
            Err(OfficeFailure::Transport)
        }));
        let gw = build_gateway(Arc::clone(&host), Arc::clone(&office));
        let (cookie, proof) = sign_in(&gw).await;
        let creds = Some((cookie.as_str(), proof.as_str()));

        let posts = [
            (
                format!("/api/v1/organizations/{ORG_ID}/external-auth/approve"),
                r#"{"code":"K7M9-P4Q2"}"#,
            ),
            (
                format!(
                    "/api/v1/organizations/{ORG_ID}/members/{MEMBER_ID}/external-sessions/renew"
                ),
                r#"{}"#,
            ),
            (
                format!(
                    "/api/v1/organizations/{ORG_ID}/members/{MEMBER_ID}/external-sessions/revoke"
                ),
                r#"{}"#,
            ),
        ];

        for (path, body) in &posts {
            // Missing Origin -> 403 Forbidden.
            let resp = gw
                .respond(make_request(
                    Method::POST,
                    path,
                    creds,
                    Some(*body),
                    None,
                    &[("sec-fetch-site", "same-origin")],
                ))
                .await;
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "missing origin {path}"
            );

            // Mismatched Origin -> 403 Forbidden.
            let resp = gw
                .respond(make_request(
                    Method::POST,
                    path,
                    creds,
                    Some(*body),
                    Some("https://evil.example"),
                    &[("sec-fetch-site", "same-origin")],
                ))
                .await;
            assert_eq!(resp.status(), StatusCode::FORBIDDEN, "evil origin {path}");

            // Cross-site Sec-Fetch-Site even with valid Origin -> 403 Forbidden.
            let resp = gw
                .respond(make_request(
                    Method::POST,
                    path,
                    creds,
                    Some(*body),
                    Some(ORIGIN),
                    &[("sec-fetch-site", "cross-site")],
                ))
                .await;
            assert_eq!(resp.status(), StatusCode::FORBIDDEN, "cross-site {path}");

            // Form-encoded Content-Type -> 415 Unsupported Media Type.
            let mut form_req = Request::builder()
                .method(Method::POST)
                .uri(path.as_str())
                .header(header::HOST, HOST)
                .header(header::ORIGIN, ORIGIN)
                .header("sec-fetch-site", "same-origin")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header(header::COOKIE, cookie.as_str())
                .header(PROOF_HEADER, proof.as_str())
                .body(Full::new(Bytes::from("code=K7M9-P4Q2")))
                .unwrap();
            form_req
                .extensions_mut()
                .insert(PeerAddress(PEER.parse::<SocketAddr>().unwrap()));
            assert_eq!(
                gw.respond(form_req).await.status(),
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "form content-type {path}"
            );
        }

        assert!(office.recorded().is_empty());
    });
}

#[test]
fn invalid_paths_queries_methods_and_bodies_fail_with_400_before_touching_office() {
    run_async(async {
        let host = standard_host();
        let office = Arc::new(FakeOffice::new(|_, _, _, _, _| {
            Err(OfficeFailure::Transport)
        }));
        let gw = build_gateway(Arc::clone(&host), Arc::clone(&office));
        let (cookie, proof) = sign_in(&gw).await;
        let creds = Some((cookie.as_str(), proof.as_str()));

        let bad_requests: Vec<(Method, String, Option<String>)> = vec![
            // Path traversal & non-UUIDv7 org/member segments.
            (
                Method::GET,
                "/api/v1/organizations/../external-auth/pending".into(),
                None,
            ),
            (
                Method::GET,
                "/api/v1/organizations/not-a-uuid/external-auth/pending".into(),
                None,
            ),
            (
                Method::GET,
                format!("/api/v1/organizations/{ORG_ID}/members/not-a-uuid/external-sessions"),
                None,
            ),
            (
                Method::GET,
                format!(
                    "/api/v1/organizations/{ORG_ID}/members/0193A5C0-0000-7000-8000-000000000050/external-sessions"
                ),
                None,
            ),
            (
                Method::GET,
                format!(
                    "/api/v1/organizations/{ORG_ID}/members/0193a5c0-0000-4000-8000-000000000050/external-sessions"
                ),
                None,
            ),
            // Bad queries on pending & member sessions.
            (
                Method::GET,
                format!("/api/v1/organizations/{ORG_ID}/external-auth/pending?member_id=not-a-uuid"),
                None,
            ),
            (
                Method::GET,
                format!(
                    "/api/v1/organizations/{ORG_ID}/external-auth/pending?member_id={MEMBER_ID}&member_id={MEMBER_ID}"
                ),
                None,
            ),
            (
                Method::GET,
                format!("/api/v1/organizations/{ORG_ID}/external-auth/pending?extra=1"),
                None,
            ),
            (
                Method::GET,
                format!(
                    "/api/v1/organizations/{ORG_ID}/members/{MEMBER_ID}/external-sessions?limit=0"
                ),
                None,
            ),
            (
                Method::GET,
                format!(
                    "/api/v1/organizations/{ORG_ID}/members/{MEMBER_ID}/external-sessions?limit=201"
                ),
                None,
            ),
            // Non-empty body on GET endpoints.
            (
                Method::GET,
                format!("/api/v1/organizations/{ORG_ID}/external-auth/pending"),
                Some("{}".into()),
            ),
            (
                Method::GET,
                format!("/api/v1/organizations/{ORG_ID}/members/{MEMBER_ID}/external-sessions"),
                Some("{}".into()),
            ),
            // Wrong HTTP methods.
            (
                Method::POST,
                format!("/api/v1/organizations/{ORG_ID}/external-auth/pending"),
                Some("{}".into()),
            ),
            (
                Method::GET,
                format!("/api/v1/organizations/{ORG_ID}/external-auth/approve"),
                None,
            ),
            (
                Method::GET,
                format!(
                    "/api/v1/organizations/{ORG_ID}/members/{MEMBER_ID}/external-sessions/renew"
                ),
                None,
            ),
            (
                Method::GET,
                format!(
                    "/api/v1/organizations/{ORG_ID}/members/{MEMBER_ID}/external-sessions/revoke"
                ),
                None,
            ),
            // Unknown fields & malformed/oversized codes/TTLs on approve.
            (
                Method::POST,
                format!("/api/v1/organizations/{ORG_ID}/external-auth/approve"),
                Some(r#"{"code":"K7M9-P4Q2","unknown":true}"#.into()),
            ),
            (
                Method::POST,
                format!("/api/v1/organizations/{ORG_ID}/external-auth/approve"),
                Some(r#"{"code":""}"#.into()),
            ),
            (
                Method::POST,
                format!("/api/v1/organizations/{ORG_ID}/external-auth/approve"),
                Some(r#"{"code":"K7M9-P4QU"}"#.into()), // 'U' is not in Crockford base32
            ),
            (
                Method::POST,
                format!("/api/v1/organizations/{ORG_ID}/external-auth/approve"),
                Some(format!(r#"{{"code":"{}"}}"#, "A".repeat(40))),
            ),
            (
                Method::POST,
                format!("/api/v1/organizations/{ORG_ID}/external-auth/approve"),
                Some(r#"{"code":"K7M9-P4Q2","ttl_seconds":0}"#.into()),
            ),
            (
                Method::POST,
                format!("/api/v1/organizations/{ORG_ID}/external-auth/approve"),
                Some(format!(
                    r#"{{"code":"K7M9-P4Q2","ttl_seconds":{}}}"#,
                    MIN_LEASE_TTL_SECS - 1
                )),
            ),
            (
                Method::POST,
                format!("/api/v1/organizations/{ORG_ID}/external-auth/approve"),
                Some(format!(
                    r#"{{"code":"K7M9-P4Q2","ttl_seconds":{}}}"#,
                    MAX_LEASE_TTL_SECS + 1
                )),
            ),
            // Unknown fields & invalid session_id/TTL on renew & revoke.
            (
                Method::POST,
                format!(
                    "/api/v1/organizations/{ORG_ID}/members/{MEMBER_ID}/external-sessions/renew"
                ),
                Some(r#"{"unknown":1}"#.into()),
            ),
            (
                Method::POST,
                format!(
                    "/api/v1/organizations/{ORG_ID}/members/{MEMBER_ID}/external-sessions/renew"
                ),
                Some(r#"{"ttl_seconds":0}"#.into()),
            ),
            (
                Method::POST,
                format!(
                    "/api/v1/organizations/{ORG_ID}/members/{MEMBER_ID}/external-sessions/renew"
                ),
                Some(r#"{"session_id":"../escape"}"#.into()),
            ),
            (
                Method::POST,
                format!(
                    "/api/v1/organizations/{ORG_ID}/members/{MEMBER_ID}/external-sessions/revoke"
                ),
                Some(r#"{"extra":"nope"}"#.into()),
            ),
            (
                Method::POST,
                format!(
                    "/api/v1/organizations/{ORG_ID}/members/{MEMBER_ID}/external-sessions/revoke"
                ),
                Some(r#"{"command_id":"not-uuid"}"#.into()),
            ),
        ];

        for (method, path, body) in &bad_requests {
            let (status, _, payload) = collect_json(
                gw.respond(make_request(
                    method.clone(),
                    path,
                    creds,
                    body.as_deref(),
                    Some(ORIGIN),
                    &[("sec-fetch-site", "same-origin")],
                ))
                .await,
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{method} {path} {body:?}");
            assert_eq!(payload["error"]["code"], "invalid_request");
            assert!(
                !payload.to_string().contains("P4Q2") && !payload.to_string().contains("P4QU"),
                "validation error must never echo submitted code: {payload}"
            );
        }

        // Unknown subpaths under external-auth / external-sessions -> 404.
        for path in [
            format!("/api/v1/organizations/{ORG_ID}/external-auth/bogus"),
            format!("/api/v1/organizations/{ORG_ID}/members/{MEMBER_ID}/external-sessions/bogus"),
        ] {
            let resp = gw
                .respond(make_request(
                    Method::GET,
                    &path,
                    creds,
                    None,
                    Some(ORIGIN),
                    &[("sec-fetch-site", "same-origin")],
                ))
                .await;
            assert_eq!(resp.status(), StatusCode::NOT_FOUND, "{path}");
        }

        assert!(
            office.recorded().is_empty(),
            "invalid requests must fail before calling office: {:?}",
            office.recorded()
        );
    });
}

#[test]
fn happy_paths_proxy_all_five_endpoints_and_never_leak_approval_code_or_secret_half() {
    run_async(async {
        let host = standard_host();
        let office = Arc::new(FakeOffice::new(|_auth, office_id, method, params, opts| {
            assert_eq!(office_id, OFFICE_ID);
            match method {
                "member.external_pending" => {
                    assert!(opts.retry_on_transport);
                    // Deliberately pass an unmasked secret half to verify BFF defense-in-depth!
                    Ok(serde_json::to_value(MemberExternalPendingResult {
                        pending: vec![sample_pending_view("K7M9-P4Q2")],
                    })
                    .unwrap())
                }
                "member.external_sessions" => {
                    assert!(opts.retry_on_transport);
                    assert_eq!(params["employee_id"], MEMBER_ID);
                    Ok(serde_json::to_value(MemberExternalSessionsResult {
                        employee_id: MEMBER_ID.into(),
                        active: Some(sample_session_view(false)),
                        history: vec![sample_history_entry()],
                    })
                    .unwrap())
                }
                "member.external_approve" => {
                    assert!(!opts.retry_on_transport);
                    assert_eq!(params["code"], "K7M9-P4Q2");
                    assert_eq!(params["ttl_secs"], 7200);
                    assert_eq!(params["takeover"], true);
                    assert_eq!(params["command_id"], CMD_ID);
                    Ok(serde_json::to_value(sample_session_view(true)).unwrap())
                }
                "member.external_renew" => {
                    assert!(!opts.retry_on_transport);
                    assert_eq!(params["employee_id"], MEMBER_ID);
                    Ok(serde_json::to_value(sample_session_view(false)).unwrap())
                }
                "member.external_revoke" => {
                    assert!(!opts.retry_on_transport);
                    assert_eq!(params["employee_id"], MEMBER_ID);
                    Ok(serde_json::to_value(MemberExternalRevokeResult {
                        employee_id: MEMBER_ID.into(),
                        revoked: true,
                        session_id: params["session_id"].as_str().map(str::to_owned),
                    })
                    .unwrap())
                }
                other => panic!("unexpected office method: {other}"),
            }
        }));
        let gw = build_gateway(Arc::clone(&host), Arc::clone(&office));
        let (cookie, proof) = sign_in(&gw).await;
        let creds = Some((cookie.as_str(), proof.as_str()));

        // 1. GET /external-auth/pending (office-wide) masks code_prefix defensively.
        let (status, _, body) = collect_json(
            gw.respond(make_request(
                Method::GET,
                &format!("/api/v1/organizations/{ORG_ID}/external-auth/pending"),
                creds,
                None,
                Some(ORIGIN),
                &[("sec-fetch-site", "same-origin")],
            ))
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["result"]["pending"][0]["handle"], "@ada");
        assert_eq!(body["result"]["pending"][0]["office_name"], "HQ");
        assert_eq!(body["result"]["pending"][0]["code_prefix"], "K7M9-••••");
        assert_eq!(
            body["result"]["pending"][0]["workspace_familiarity"],
            "returning_known_key"
        );
        assert!(
            !body.to_string().contains("P4Q2"),
            "secret half must never appear in pending response: {body}"
        );

        // 2. GET /external-auth/pending?member_id=<uuid> forwards employee_id filter.
        let (status, _, _) = collect_json(
            gw.respond(make_request(
                Method::GET,
                &format!(
                    "/api/v1/organizations/{ORG_ID}/external-auth/pending?member_id={MEMBER_ID}"
                ),
                creds,
                None,
                Some(ORIGIN),
                &[("sec-fetch-site", "same-origin")],
            ))
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let calls = office.recorded();
        assert_eq!(calls[1].0, "member.external_pending");
        assert_eq!(calls[1].1["employee_id"], MEMBER_ID);

        // 3. GET /members/{member_id}/external-sessions?limit=50 returns active, pending & history.
        let (status, _, body) = collect_json(
            gw.respond(make_request(
                Method::GET,
                &format!(
                    "/api/v1/organizations/{ORG_ID}/members/{MEMBER_ID}/external-sessions?limit=50"
                ),
                creds,
                None,
                Some(ORIGIN),
                &[("sec-fetch-site", "same-origin")],
            ))
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["result"]["employee_id"], MEMBER_ID);
        assert_eq!(
            body["result"]["active"]["workspace_label"],
            "devbox:~/src/app"
        );
        assert_eq!(body["result"]["pending"][0]["code_prefix"], "K7M9-••••");
        assert_eq!(body["result"]["history"][0]["event"], "approved");
        assert!(!body.to_string().contains("P4Q2"));

        // 4. POST /external-auth/approve normalizes Crockford code & never leaks it.
        let (status, _, body) = collect_json(
            gw.respond(make_request(
                Method::POST,
                &format!("/api/v1/organizations/{ORG_ID}/external-auth/approve"),
                creds,
                Some(&format!(
                    r#"{{"command_id":"{CMD_ID}","code":" k7m9 p4q2 ","ttl_seconds":7200,"takeover":true}}"#
                )),
                Some(ORIGIN),
                &[("sec-fetch-site", "same-origin")],
            ))
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["result"]["active"], true);
        assert_eq!(body["result"]["took_over_managed"], true);
        assert!(!body.to_string().contains("K7M9-P4Q2"));
        assert!(!body.to_string().contains("P4Q2"));

        // 5a. POST /members/{member_id}/external-sessions/renew with {} defaults to 7d TTL.
        let (status, _, body) = collect_json(
            gw.respond(make_request(
                Method::POST,
                &format!(
                    "/api/v1/organizations/{ORG_ID}/members/{MEMBER_ID}/external-sessions/renew"
                ),
                creds,
                Some(r#"{}"#),
                Some(ORIGIN),
                &[("sec-fetch-site", "same-origin")],
            ))
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let calls = office.recorded();
        let default_renew = calls.last().unwrap();
        assert_eq!(default_renew.0, "member.external_renew");
        assert!(default_renew.1.get("ttl_secs").is_none());
        assert!(default_renew.1.get("session_id").is_none());

        // 5b. POST /members/{member_id}/external-sessions/renew (explicit session_id & TTL).
        let (status, _, body) = collect_json(
            gw.respond(make_request(
                Method::POST,
                &format!(
                    "/api/v1/organizations/{ORG_ID}/members/{MEMBER_ID}/external-sessions/renew"
                ),
                creds,
                Some(r#"{"session_id":"ext-0193a5c0-0000-7000-8000-000000000801","ttl_seconds":86400}"#),
                Some(ORIGIN),
                &[("sec-fetch-site", "same-origin")],
            ))
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let calls = office.recorded();
        let renew_call = calls.last().unwrap();
        assert_eq!(renew_call.0, "member.external_renew");
        assert_eq!(renew_call.1["ttl_secs"], 86400);
        assert_eq!(
            renew_call.1["session_id"],
            "ext-0193a5c0-0000-7000-8000-000000000801"
        );

        // 6a. POST /members/{member_id}/external-sessions/revoke with {} (any active lease).
        let (status, _, body) = collect_json(
            gw.respond(make_request(
                Method::POST,
                &format!(
                    "/api/v1/organizations/{ORG_ID}/members/{MEMBER_ID}/external-sessions/revoke"
                ),
                creds,
                Some(r#"{}"#),
                Some(ORIGIN),
                &[("sec-fetch-site", "same-origin")],
            ))
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["result"]["revoked"], true);

        // 6b. POST /members/{member_id}/external-sessions/revoke (specific session_id).
        let (status, _, body) = collect_json(
            gw.respond(make_request(
                Method::POST,
                &format!(
                    "/api/v1/organizations/{ORG_ID}/members/{MEMBER_ID}/external-sessions/revoke"
                ),
                creds,
                Some(r#"{"session_id":"ext-0193a5c0-0000-7000-8000-000000000801"}"#),
                Some(ORIGIN),
                &[("sec-fetch-site", "same-origin")],
            ))
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["result"]["revoked"], true);
        assert_eq!(
            body["result"]["session_id"],
            "ext-0193a5c0-0000-7000-8000-000000000801"
        );
    });
}

#[test]
fn office_errors_map_cleanly_and_approve_dispatches_once_with_code_scrubbing() {
    run_async(async {
        let approve_attempts = Arc::new(AtomicUsize::new(0));
        let attempts_counter = Arc::clone(&approve_attempts);
        let host = standard_host();
        let office = Arc::new(FakeOffice::new(
            move |_, _, method, params, _| match method {
                "member.external_approve" => {
                    let n = attempts_counter.fetch_add(1, Ordering::SeqCst);
                    match n {
                    // Non-operator / wrong code -> 403, must NOT retry & must scrub echoed code!
                    0 => Err(OfficeFailure::Refused(ApiError::new(
                        ErrorCode::NotAuthorized,
                        format!("wrong approval code {}", params["code"].as_str().unwrap()),
                    ))),
                    // Live managed turn without takeover -> 409 Conflict.
                    1 => Err(OfficeFailure::Refused(ApiError::new(
                        ErrorCode::Conflict,
                        "this agent has a live managed turn; approve with --takeover to park it first",
                    ))),
                    // Rate limited -> 429 + Retry-After.
                    2 => {
                        let mut err = ApiError::new(ErrorCode::RateLimited, "too many attempts");
                        err.retry_after_ms = Some(2500);
                        Err(OfficeFailure::Refused(err))
                    }
                    // NotFound -> 404.
                    3 => Err(OfficeFailure::Refused(ApiError::new(
                        ErrorCode::NotFound,
                        "no sign-in is waiting for that approval code",
                    ))),
                    // Storage error -> 503 with opaque message.
                    _ => Err(OfficeFailure::Refused(ApiError::new(
                        ErrorCode::StorageError,
                        "sqlite disk detail must not leak",
                    ))),
                }
                }
                "member.external_renew" => Err(OfficeFailure::Refused(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "invalid ttl",
                ))),
                "member.external_revoke" => Err(OfficeFailure::Refused(ApiError::new(
                    ErrorCode::NotAuthorized,
                    "only an authorized operator may revoke",
                ))),
                _ => Err(OfficeFailure::Transport),
            },
        ));
        let gw = build_gateway(Arc::clone(&host), Arc::clone(&office));
        let (cookie, proof) = sign_in(&gw).await;
        let creds = Some((cookie.as_str(), proof.as_str()));

        // 1. NotAuthorized on approve: 403, called EXACTLY ONCE, echoed code scrubbed!
        let (status, _, body) = collect_json(
            gw.respond(make_request(
                Method::POST,
                &format!("/api/v1/organizations/{ORG_ID}/external-auth/approve"),
                creds,
                Some(r#"{"code":"K7M9-P4Q2"}"#),
                Some(ORIGIN),
                &[("sec-fetch-site", "same-origin")],
            ))
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(body["error"]["code"], "not_authorized");
        assert_eq!(
            approve_attempts.load(Ordering::SeqCst),
            1,
            "approve must NEVER retry on NotAuthorized and double-spend code attempts"
        );
        assert!(
            !body.to_string().contains("K7M9") && !body.to_string().contains("P4Q2"),
            "office error echoing code must be scrubbed: {body}"
        );

        // 2. Conflict (managed turn active without takeover) -> 409.
        let (status, _, body) = collect_json(
            gw.respond(make_request(
                Method::POST,
                &format!("/api/v1/organizations/{ORG_ID}/external-auth/approve"),
                creds,
                Some(r#"{"code":"K7M9-P4Q2"}"#),
                Some(ORIGIN),
                &[("sec-fetch-site", "same-origin")],
            ))
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(body["error"]["code"], "conflict");

        // 3. RateLimited -> 429 + retry_after_ms + Retry-After header.
        let (status, headers, body) = collect_json(
            gw.respond(make_request(
                Method::POST,
                &format!("/api/v1/organizations/{ORG_ID}/external-auth/approve"),
                creds,
                Some(r#"{"code":"K7M9-P4Q2"}"#),
                Some(ORIGIN),
                &[("sec-fetch-site", "same-origin")],
            ))
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
        assert_eq!(body["error"]["code"], "rate_limited");
        assert_eq!(body["error"]["retry_after_ms"], 2500);
        assert_eq!(headers.get(header::RETRY_AFTER).unwrap(), "3");

        // 4. NotFound -> 404.
        let (status, _, body) = collect_json(
            gw.respond(make_request(
                Method::POST,
                &format!("/api/v1/organizations/{ORG_ID}/external-auth/approve"),
                creds,
                Some(r#"{"code":"K7M9-P4Q2"}"#),
                Some(ORIGIN),
                &[("sec-fetch-site", "same-origin")],
            ))
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        assert_eq!(body["error"]["code"], "not_found");

        // 5. StorageError -> 503 with opaque message (never leaks internal error text).
        let (status, _, body) = collect_json(
            gw.respond(make_request(
                Method::POST,
                &format!("/api/v1/organizations/{ORG_ID}/external-auth/approve"),
                creds,
                Some(r#"{"code":"K7M9-P4Q2"}"#),
                Some(ORIGIN),
                &[("sec-fetch-site", "same-origin")],
            ))
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
        assert!(!body.to_string().contains("sqlite"));

        // 6. InvalidRequest on renew -> 400; NotAuthorized on revoke -> 403.
        let (status, _, body) = collect_json(
            gw.respond(make_request(
                Method::POST,
                &format!(
                    "/api/v1/organizations/{ORG_ID}/members/{MEMBER_ID}/external-sessions/renew"
                ),
                creds,
                Some(r#"{}"#),
                Some(ORIGIN),
                &[("sec-fetch-site", "same-origin")],
            ))
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["error"]["code"], "invalid_request");

        let (status, _, body) = collect_json(
            gw.respond(make_request(
                Method::POST,
                &format!(
                    "/api/v1/organizations/{ORG_ID}/members/{MEMBER_ID}/external-sessions/revoke"
                ),
                creds,
                Some(r#"{}"#),
                Some(ORIGIN),
                &[("sec-fetch-site", "same-origin")],
            ))
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(body["error"]["code"], "not_authorized");
    });
}

#[test]
fn pending_read_revalidates_once_on_stale_grant_and_approval_code_debug_is_redacted() {
    let code = normalize_code_literal("k7mo-p4ql").expect("valid crockford");
    assert_eq!(code.as_str(), "K7M0-P4Q1");
    let debug = format!("{code:?}");
    assert_eq!(debug, "ApprovalCode(<redacted>)");
    assert!(!debug.contains("K7M0") && !debug.contains("P4Q1"));
    assert_eq!(mask_code_prefix("k7m9-p4q2"), "K7M9-••••");
    assert_eq!(mask_code_prefix("bad"), "••••-••••");

    run_async(async {
        let host = standard_host();
        let attempts = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&attempts);
        let office = Arc::new(FakeOffice::new(move |_, _, method, _, _| {
            assert_eq!(method, "member.external_pending");
            if counter.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(OfficeFailure::Refused(ApiError::new(
                    ErrorCode::NotAuthorized,
                    "stale grant",
                )));
            }
            Ok(serde_json::to_value(MemberExternalPendingResult { pending: vec![] }).unwrap())
        }));
        let gw = build_gateway(Arc::clone(&host), Arc::clone(&office));
        let (cookie, proof) = sign_in(&gw).await;

        let (status, _, body) = collect_json(
            gw.respond(make_request(
                Method::GET,
                &format!("/api/v1/organizations/{ORG_ID}/external-auth/pending"),
                Some((cookie.as_str(), proof.as_str())),
                None,
                Some(ORIGIN),
                &[("sec-fetch-site", "same-origin")],
            ))
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        assert!(host.methods().iter().any(|m| m == "session.validate"));
    });
}
