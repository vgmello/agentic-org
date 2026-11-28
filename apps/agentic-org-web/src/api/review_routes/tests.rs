//! Unit and full-gateway HTTP integration tests for `/api/v1/organizations/{org}/reviews*`
//! (`TASK-92.2` AC #2).
//!
//! Runs strictly against in-memory [`ControlPlaneTransport`] and [`OfficeTransport`] doubles
//! (zero Unix sockets, zero disk writes).

use std::future::Future;
use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use http_body_util::{BodyExt, Full};
use hyper::{body::Bytes, header, Method, Request, StatusCode};
use org_host_control::listen::{ListenMode, ListenOrigin};
use org_host_control::organization::BrowserAuth;
use org_proto::reviews::{
    FindingAnchor, FindingSeverity, FindingStatus, ReviewDiffFile, ReviewDiffFileStatus,
    ReviewDiffResult, ReviewFindingOp, ReviewFindingResult, ReviewFindingView, ReviewListResult,
    ReviewMutationResult, ReviewReviewerView, ReviewRoundResult, ReviewRoundView,
    ReviewRouteResult, ReviewSessionMode, ReviewStatus, ReviewTargetKind, ReviewVerdict,
    ReviewVerdictResult, ReviewVerdictView, ReviewView, RoutingMode,
};
use org_proto::{ApiError, ErrorCode};
use serde_json::{json, Value};
use zeroize::Zeroizing;

use super::bodies::{
    decode_close_body, decode_finding_body, decode_request_body, decode_resubmit_body,
    decode_route_body, decode_verdict_body,
};
use super::query::{
    filter_diff_by_line_range, filter_inspect_round, parse_diff_query, parse_inspect_query,
    parse_list_query, valid_diff_path, valid_ref,
};
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
const USER_ID: &str = "0193a5c0-0000-7000-8000-000000000006";
const REVIEWER_ID: &str = "0193a5c0-0000-7000-8000-000000000007";
const REVIEW_ID: &str = "0193a5c0-0000-7000-8000-000000000010";
const FINDING_ID: &str = "0193a5c0-0000-7000-8000-000000000011";
const REPLY_ID: &str = "0193a5c0-0000-7000-8000-000000000012";
const CMD_ID: &str = "0193a5c0-0000-7000-8000-000000000099";
const PEER: &str = "192.0.2.42:51000";

type HostHandler = dyn Fn(&str, &Value) -> Result<Value, Failure> + Send + Sync;
type OfficeHandler = dyn Fn(&BrowserAuth, &str, &'static str, &Value, OfficeCallOptions) -> Result<Value, OfficeFailure>
    + Send
    + Sync;

struct FakeHost {
    handler: Arc<HostHandler>,
}

impl FakeHost {
    fn new(
        handler: impl Fn(&str, &Value) -> Result<Value, Failure> + Send + Sync + 'static,
    ) -> Self {
        Self {
            handler: Arc::new(handler),
        }
    }

    fn dispatch(
        &self,
        method: &'static str,
        params_json: Zeroizing<Vec<u8>>,
        hook: Option<RawReplyHook>,
    ) -> Result<ControlPayload, Failure> {
        let params: Value =
            serde_json::from_slice(&params_json).map_err(|_| Failure::Unavailable)?;
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
                        "employee_id": USER_ID,
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
                            "employee_id": USER_ID,
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
                    "employee_id": USER_ID,
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
        .method(method)
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

fn sample_review_view() -> ReviewView {
    ReviewView {
        id: REVIEW_ID.into(),
        key: "RV-A1".into(),
        office_code: "A".into(),
        number: 1,
        organization_id: ORG_ID.into(),
        project_id: None,
        task_id: None,
        target_kind: ReviewTargetKind::Code,
        repository_id: Some("repo-1".into()),
        branch: Some("feat/reviews".into()),
        target_ref: None,
        kind_slug: "code-review".into(),
        category_slug: None,
        title: "Implement review BFF".into(),
        body_markdown: "Please verify security guards.".into(),
        independent: false,
        status: ReviewStatus::Open,
        current_round: 2,
        pin_stale: false,
        guidance_refs: vec![],
        criteria: vec!["All 9 routes work".into()],
        open_findings: 1,
        closed_reason: None,
        revision: 3,
        created_by: USER_ID.into(),
        created_at: "2026-11-27T10:00:00.000Z".into(),
        updated_at: "2026-11-27T11:00:00.000Z".into(),
    }
}

fn sample_round(round_number: u32) -> ReviewRoundView {
    ReviewRoundView {
        round_number,
        base_oid: Some("1111111111111111111111111111111111111111".into()),
        head_oid: Some("2222222222222222222222222222222222222222".into()),
        session_mode: ReviewSessionMode::Warm,
        submitted_by: USER_ID.into(),
        submitted_at: "2026-11-27T10:00:00.000Z".into(),
        note_markdown: String::new(),
        verdicts: vec![],
    }
}

fn sample_finding(round_number: u32, status: FindingStatus) -> ReviewFindingView {
    ReviewFindingView {
        id: FINDING_ID.into(),
        review_id: REVIEW_ID.into(),
        number: 1,
        round_number,
        severity: FindingSeverity::Major,
        status,
        anchor: FindingAnchor::file_lines("src/lib.rs", 10, 12),
        anchor_stale: false,
        body_markdown: "Check bounds".into(),
        author_id: REVIEWER_ID.into(),
        attachment_ids: vec![],
        resolved_by: None,
        resolved_at: None,
        resolved_in_round: None,
        replies: vec![],
        revision: 1,
        created_at: "2026-11-27T10:30:00.000Z".into(),
        updated_at: "2026-11-27T10:30:00.000Z".into(),
    }
}

fn sample_diff_result() -> ReviewDiffResult {
    let unified = concat!(
        "diff --git a/src/lib.rs b/src/lib.rs\n",
        "--- a/src/lib.rs\n",
        "+++ b/src/lib.rs\n",
        "@@ -1,3 +1,4 @@\n",
        " fn a() {}\n",
        "+fn b() {}\n",
        "@@ -50,2 +51,3 @@\n",
        " fn far() {}\n",
        "+fn very_far() {}\n",
    );
    ReviewDiffResult {
        review_id: REVIEW_ID.into(),
        round_number: 2,
        since_round: Some(1),
        base_oid: "1111111111111111111111111111111111111111".into(),
        head_oid: "2222222222222222222222222222222222222222".into(),
        files: vec![ReviewDiffFile {
            path: "src/lib.rs".into(),
            status: ReviewDiffFileStatus::Modified,
            added: 2,
            deleted: 0,
            binary: false,
        }],
        total_files: 1,
        files_truncated: false,
        unified_diff: unified.into(),
        truncated: false,
    }
}

fn standard_review_office() -> Arc<FakeOffice> {
    Arc::new(FakeOffice::new(|_, office_id, method, params, _| {
        assert_eq!(office_id, OFFICE_ID);
        match method {
            "review.list" => Ok(serde_json::to_value(ReviewListResult {
                reviews: vec![sample_review_view()],
                total: 1,
            })
            .unwrap()),
            "review.request" | "review.resubmit" => Ok(serde_json::to_value(ReviewRoundResult {
                review: sample_review_view(),
                round: sample_round(2),
            })
            .unwrap()),
            "review.inspect" => {
                if params.get("fields").is_some() {
                    Ok(json!({
                        "review": sample_review_view(),
                        "rounds": [sample_round(1), sample_round(2)],
                        "findings": [
                            sample_finding(1, FindingStatus::Resolved),
                            sample_finding(2, FindingStatus::Open),
                        ]
                    }))
                } else {
                    Ok(json!({
                        "review": sample_review_view(),
                        "rounds": [sample_round(1), sample_round(2)],
                        "reviewers": [ReviewReviewerView {
                            reviewer_id: REVIEWER_ID.into(),
                            routing: RoutingMode::Explicit,
                            team_id: None,
                            session_mode: ReviewSessionMode::Warm,
                            assigned_by: USER_ID.into(),
                            assigned_at: "2026-11-27T10:00:00.000Z".into(),
                            last_verdict: None,
                            last_verdict_round: None,
                            removed: false,
                        }],
                        "findings": [
                            sample_finding(1, FindingStatus::Resolved),
                            sample_finding(2, FindingStatus::Open),
                        ]
                    }))
                }
            }
            "review.diff" => Ok(serde_json::to_value(sample_diff_result()).unwrap()),
            "review.route" => Ok(serde_json::to_value(ReviewRouteResult {
                review: sample_review_view(),
                reviewers: vec![],
            })
            .unwrap()),
            "review.finding" => Ok(serde_json::to_value(ReviewFindingResult {
                review: sample_review_view(),
                finding: sample_finding(2, FindingStatus::Open),
            })
            .unwrap()),
            "review.verdict" => Ok(serde_json::to_value(ReviewVerdictResult {
                review: sample_review_view(),
                verdict: ReviewVerdictView {
                    reviewer_id: REVIEWER_ID.into(),
                    round_number: 2,
                    verdict: ReviewVerdict::Approve,
                    summary_markdown: "Looks great".into(),
                    finding_ids: vec![],
                    attachment_ids: vec![],
                    submitted_at: "2026-11-27T11:05:00.000Z".into(),
                },
                findings: vec![],
            })
            .unwrap()),
            "review.close" => Ok(serde_json::to_value(ReviewMutationResult {
                review: sample_review_view(),
            })
            .unwrap()),
            _ => Err(OfficeFailure::Transport),
        }
    }))
}

#[test]
fn unit_validators_reject_blank_oversized_traversal_and_control_chars() {
    assert!(valid_ref("RV-A1").is_ok());
    assert!(valid_ref(REVIEW_ID).is_ok());
    assert!(valid_ref("").is_err());
    assert!(valid_ref("   ").is_err());
    assert!(valid_ref(&"a".repeat(129)).is_err());
    assert!(valid_ref("RV-A1\n").is_err());
    assert!(valid_ref("RV-A1\u{0085}").is_err());
    assert!(valid_ref("RV-A1\0").is_err());
    assert!(valid_ref("..").is_err());
    assert!(valid_ref("a/b").is_err());
    assert!(valid_ref("a\\b").is_err());

    assert!(valid_diff_path("src/api/review_routes.rs").is_ok());
    assert!(valid_diff_path("").is_err());
    assert!(valid_diff_path("/etc/passwd").is_err());
    assert!(valid_diff_path("-foo").is_err());
    assert!(valid_diff_path(":foo").is_err());
    assert!(valid_diff_path("../secret").is_err());
    assert!(valid_diff_path("src/.GIT/config").is_err());
    assert!(valid_diff_path("src\\win.rs").is_err());
    assert!(valid_diff_path("src/a\u{0007}.rs").is_err());
}

#[test]
fn unit_query_parsers_and_filters_enforce_bounds_and_aliases() {
    let list = parse_list_query(Some(&format!(
        "status=open&project_id=PRJ-1&task_id=TSK-A1&reviewer_id={REVIEWER_ID}&author_id={USER_ID}&kind=code&limit=25&cursor=10"
    )))
    .ok()
    .unwrap();
    assert_eq!(list.status, Some(ReviewStatus::Open));
    assert_eq!(list.project_id.as_deref(), Some("PRJ-1"));
    assert_eq!(list.task_id.as_deref(), Some("TSK-A1"));
    assert_eq!(list.reviewer_id.as_deref(), Some(REVIEWER_ID));
    assert_eq!(list.created_by.as_deref(), Some(USER_ID));
    assert_eq!(list.target_kind, Some(ReviewTargetKind::Code));
    assert_eq!(list.limit, Some(25));
    assert_eq!(list.offset, Some(10));

    // Duplicate & conflicting aliases rejected.
    assert!(parse_list_query(Some("limit=0")).is_err());
    assert!(parse_list_query(Some("limit=201")).is_err());
    assert!(parse_list_query(Some("cursor=1&offset=2")).is_err());
    assert!(parse_list_query(Some("kind=code&target_kind=doc")).is_err());
    assert!(parse_list_query(Some("unknown=1")).is_err());

    let inspect = parse_inspect_query("RV-A1".into(), Some("fields=rounds,findings&round=2"))
        .ok()
        .unwrap();
    assert_eq!(
        inspect.params.fields,
        Some(vec!["rounds".to_owned(), "findings".to_owned()])
    );
    assert_eq!(inspect.round, Some(2));
    assert!(parse_inspect_query("RV-A1".into(), Some("fields=bogus")).is_err());
    assert!(parse_inspect_query("RV-A1".into(), Some("round=0")).is_err());

    let diff = parse_diff_query(
        "RV-A1".into(),
        Some("round=3&compare_round=1&path=src%2Flib.rs&start_line=1&end_line=10&context_lines=5"),
    )
    .ok()
    .unwrap();
    assert_eq!(diff.params.round, Some(3));
    assert_eq!(diff.params.since_round, Some(1));
    assert_eq!(diff.params.path.as_deref(), Some("src/lib.rs"));
    assert_eq!(diff.start_line, Some(1));
    assert_eq!(diff.end_line, Some(10));
    assert_eq!(diff.params.context_lines, Some(5));
    assert!(parse_diff_query("RV-A1".into(), Some("round=2&compare_round=2")).is_err());
    assert!(parse_diff_query("RV-A1".into(), Some("start_line=10&end_line=5")).is_err());
    assert!(parse_diff_query("RV-A1".into(), Some("stat_only=true&start_line=1")).is_err());

    // Round filtering keeps only matching round & findings, and returns 404 on unknown round.
    let mut inspect_val = json!({
        "review": {"current_round": 2},
        "rounds": [{"round_number": 1}, {"round_number": 2}],
        "findings": [{"round_number": 1, "id": "f1"}, {"round_number": 2, "id": "f2"}],
    });
    assert!(filter_inspect_round(&mut inspect_val, 2).is_ok());
    assert_eq!(inspect_val["rounds"].as_array().unwrap().len(), 1);
    assert_eq!(inspect_val["findings"].as_array().unwrap().len(), 1);
    assert_eq!(inspect_val["findings"][0]["id"], "f2");
    assert!(filter_inspect_round(&mut inspect_val, 9).is_err());

    // Line-range diff filtering keeps only overlapping hunks.
    let mut diff_res = sample_diff_result();
    filter_diff_by_line_range(&mut diff_res, Some(50), Some(55));
    assert!(diff_res.unified_diff.contains("diff --git a/src/lib.rs"));
    assert!(diff_res.unified_diff.contains("very_far"));
    assert!(!diff_res.unified_diff.contains("fn b()"));
}

#[test]
fn unit_body_decoders_enforce_deny_unknown_fields_and_finding_actions() {
    let req_unknown = format!(
        r#"{{"command_id":"{CMD_ID}","review_id":"{REVIEW_ID}","target_kind":"doc","title":"Spec","kind_slug":"doc-review","extra":1}}"#
    );
    assert!(decode_request_body(req_unknown.as_bytes()).is_err());

    let route_unknown = format!(
        r#"{{"command_id":"{CMD_ID}","expected_revision":1,"routing":{{"mode":"explicit","reviewer_ids":["{REVIEWER_ID}"],"bogus":true}}}}"#
    );
    assert!(decode_route_body("RV-A1".into(), route_unknown.as_bytes()).is_err());

    // Flat finding actions map cleanly to ReviewFindingOp::Add / Reply with set_status.
    let resolve_json = format!(
        r#"{{"command_id":"{CMD_ID}","action":"resolve","finding_id":"{FINDING_ID}","reply_id":"{REPLY_ID}","body_markdown":"Fixed in r2"}}"#
    );
    let resolved = decode_finding_body("RV-A1".into(), resolve_json.as_bytes())
        .ok()
        .unwrap();
    assert!(matches!(
        resolved.op,
        ReviewFindingOp::Reply {
            set_status: Some(FindingStatus::Resolved),
            ..
        }
    ));

    let wont_fix_json = format!(
        r#"{{"command_id":"{CMD_ID}","op":{{"action":"wont_fix","finding_id":"{FINDING_ID}","reply_id":"{REPLY_ID}"}}}}"#
    );
    let wont_fix = decode_finding_body("RV-A1".into(), wont_fix_json.as_bytes())
        .ok()
        .unwrap();
    assert!(matches!(
        wont_fix.op,
        ReviewFindingOp::Reply {
            set_status: Some(FindingStatus::WontFix),
            ..
        }
    ));

    let reopen_json = format!(
        r#"{{"command_id":"{CMD_ID}","action":"reopen","finding_id":"{FINDING_ID}","reply_id":"{REPLY_ID}"}}"#
    );
    let reopened = decode_finding_body("RV-A1".into(), reopen_json.as_bytes())
        .ok()
        .unwrap();
    assert!(matches!(
        reopened.op,
        ReviewFindingOp::Reply {
            set_status: Some(FindingStatus::Open),
            ..
        }
    ));

    // Unknown fields on finding, verdict, resubmit, close rejected.
    let bad_finding = format!(
        r#"{{"command_id":"{CMD_ID}","action":"resolve","finding_id":"{FINDING_ID}","reply_id":"{REPLY_ID}","sneaky":1}}"#
    );
    assert!(decode_finding_body("RV-A1".into(), bad_finding.as_bytes()).is_err());
    let bad_verdict =
        format!(r#"{{"command_id":"{CMD_ID}","round_number":1,"verdict":"approve","extra":1}}"#);
    assert!(decode_verdict_body("RV-A1".into(), bad_verdict.as_bytes()).is_err());
    let bad_resubmit = format!(
        r#"{{"command_id":"{CMD_ID}","expected_revision":1,"fresh":true,"session_mode":"warm"}}"#
    );
    assert!(decode_resubmit_body("RV-A1".into(), bad_resubmit.as_bytes()).is_err());
    let bad_close =
        format!(r#"{{"command_id":"{CMD_ID}","expected_revision":1,"reason":"ok","extra":1}}"#);
    assert!(decode_close_body("RV-A1".into(), bad_close.as_bytes()).is_err());

    // Duplicate JSON keys (flat, nested, unicode-escaped, nested anchor) and unknown "review" rejected.
    let dup_flat_cmd = format!(
        r#"{{"command_id":"{CMD_ID}","command_id":"{CMD_ID}","action":"resolve","finding_id":"{FINDING_ID}","reply_id":"{REPLY_ID}"}}"#
    );
    assert!(decode_finding_body("RV-A1".into(), dup_flat_cmd.as_bytes()).is_err());
    let dup_nested_cmd = format!(
        r#"{{"command_id":"{CMD_ID}","command_id":"{CMD_ID}","op":{{"action":"resolve","finding_id":"{FINDING_ID}","reply_id":"{REPLY_ID}"}}}}"#
    );
    assert!(decode_finding_body("RV-A1".into(), dup_nested_cmd.as_bytes()).is_err());
    let dup_flat_action = format!(
        r#"{{"command_id":"{CMD_ID}","action":"resolve","action":"reopen","finding_id":"{FINDING_ID}","reply_id":"{REPLY_ID}"}}"#
    );
    assert!(decode_finding_body("RV-A1".into(), dup_flat_action.as_bytes()).is_err());
    let dup_nested_action = format!(
        r#"{{"command_id":"{CMD_ID}","op":{{"action":"resolve","action":"reopen","finding_id":"{FINDING_ID}","reply_id":"{REPLY_ID}"}}}}"#
    );
    assert!(decode_finding_body("RV-A1".into(), dup_nested_action.as_bytes()).is_err());
    let dup_escaped_cmd = format!(
        r#"{{"command_id":"{CMD_ID}","command\u005fid":"{CMD_ID}","action":"resolve","finding_id":"{FINDING_ID}","reply_id":"{REPLY_ID}"}}"#
    );
    assert!(decode_finding_body("RV-A1".into(), dup_escaped_cmd.as_bytes()).is_err());
    let dup_anchor_key = format!(
        r#"{{"command_id":"{CMD_ID}","action":"add","finding_id":"{FINDING_ID}","severity":"major","anchor":{{"kind":"general","kind":"general"}},"body_markdown":"Check"}}"#
    );
    assert!(decode_finding_body("RV-A1".into(), dup_anchor_key.as_bytes()).is_err());
    let flat_with_review = format!(
        r#"{{"command_id":"{CMD_ID}","review":"RV-OTHER","action":"resolve","finding_id":"{FINDING_ID}","reply_id":"{REPLY_ID}"}}"#
    );
    assert!(decode_finding_body("RV-A1".into(), flat_with_review.as_bytes()).is_err());
    let nested_with_review = format!(
        r#"{{"command_id":"{CMD_ID}","review":"RV-OTHER","op":{{"action":"resolve","finding_id":"{FINDING_ID}","reply_id":"{REPLY_ID}"}}}}"#
    );
    assert!(decode_finding_body("RV-A1".into(), nested_with_review.as_bytes()).is_err());

    // Valid body with explicit JSON `null` optional field still decodes cleanly.
    let reply_with_null = format!(
        r#"{{"command_id":"{CMD_ID}","action":"reply","finding_id":"{FINDING_ID}","reply_id":"{REPLY_ID}","body_markdown":"Clarified","set_status":null}}"#
    );
    assert!(decode_finding_body("RV-A1".into(), reply_with_null.as_bytes()).is_ok());
}

#[test]
fn unit_filter_diff_by_line_range_prunes_non_overlapping_files_metadata() {
    let unified = concat!(
        "diff --git a/src/lib.rs b/src/lib.rs\n",
        "--- a/src/lib.rs\n",
        "+++ b/src/lib.rs\n",
        "@@ -1,3 +1,4 @@\n",
        " fn a() {}\n",
        "+fn b() {}\n",
        "@@ -50,2 +51,3 @@\n",
        " fn far() {}\n",
        "+fn very_far() {}\n",
        "diff --git a/src/other.rs b/src/other.rs\n",
        "--- a/src/other.rs\n",
        "+++ b/src/other.rs\n",
        "@@ -10,2 +10,3 @@\n",
        " fn middle() {}\n",
        "+fn middle_two() {}\n",
        "diff --git a/src/spaced file.rs b/src/spaced file.rs\n",
        "--- a/src/spaced file.rs\n",
        "+++ b/src/spaced file.rs\n",
        "@@ -52,1 +52,2 @@\n",
        " fn spaced() {}\n",
        "+fn spaced_two() {}\n",
    );
    let make_diff = || ReviewDiffResult {
        review_id: REVIEW_ID.into(),
        round_number: 2,
        since_round: Some(1),
        base_oid: "1111111111111111111111111111111111111111".into(),
        head_oid: "2222222222222222222222222222222222222222".into(),
        files: vec![
            ReviewDiffFile {
                path: "src/lib.rs".into(),
                status: ReviewDiffFileStatus::Modified,
                added: 2,
                deleted: 0,
                binary: false,
            },
            ReviewDiffFile {
                path: "src/other.rs".into(),
                status: ReviewDiffFileStatus::Modified,
                added: 1,
                deleted: 0,
                binary: false,
            },
            ReviewDiffFile {
                path: "src/spaced file.rs".into(),
                status: ReviewDiffFileStatus::Added,
                added: 1,
                deleted: 0,
                binary: false,
            },
        ],
        total_files: 3,
        files_truncated: false,
        unified_diff: unified.into(),
        truncated: false,
    };

    // Range [50, 55] matches `src/lib.rs` (hunk 2) and `src/spaced file.rs`, omitting `src/other.rs`.
    let mut filtered = make_diff();
    filter_diff_by_line_range(&mut filtered, Some(50), Some(55));
    let paths: Vec<&str> = filtered.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, vec!["src/lib.rs", "src/spaced file.rs"]);
    assert_eq!(filtered.total_files, 2);
    assert!(!filtered.files_truncated);
    assert!(filtered.unified_diff.contains("very_far"));
    assert!(filtered.unified_diff.contains("spaced_two"));
    assert!(!filtered.unified_diff.contains("src/other.rs"));
    assert!(!filtered.unified_diff.contains("fn b()"));

    // Range [10, 12] matches ONLY `src/other.rs`.
    let mut only_other = make_diff();
    filter_diff_by_line_range(&mut only_other, Some(10), Some(12));
    let paths: Vec<&str> = only_other.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, vec!["src/other.rs"]);
    assert_eq!(only_other.total_files, 1);

    // Non-overlapping range [500, 510] empties both unified_diff and files.
    let mut none = make_diff();
    filter_diff_by_line_range(&mut none, Some(500), Some(510));
    assert!(none.unified_diff.is_empty());
    assert!(none.files.is_empty());
    assert_eq!(none.total_files, 0);

    // Header fallback without `+++` line still extracts path from `diff --git`.
    let mut git_header_only = ReviewDiffResult {
        files: vec![
            ReviewDiffFile {
                path: "src/header_only.rs".into(),
                status: ReviewDiffFileStatus::Modified,
                added: 1,
                deleted: 0,
                binary: false,
            },
            ReviewDiffFile {
                path: "src/unmatched.rs".into(),
                status: ReviewDiffFileStatus::Modified,
                added: 1,
                deleted: 0,
                binary: false,
            },
        ],
        total_files: 2,
        unified_diff: "diff --git a/src/header_only.rs b/src/header_only.rs\n@@ -1,1 +1,2 @@\n+x\n"
            .into(),
        ..make_diff()
    };
    filter_diff_by_line_range(&mut git_header_only, Some(1), Some(2));
    assert_eq!(git_header_only.files.len(), 1);
    assert_eq!(git_header_only.files[0].path, "src/header_only.rs");
    assert_eq!(git_header_only.total_files, 1);
}

#[test]
fn unauthenticated_requests_refused_with_401_on_all_nine_routes_without_touching_office() {
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
                format!("/api/v1/organizations/{ORG_ID}/reviews"),
                None,
            ),
            (
                Method::POST,
                format!("/api/v1/organizations/{ORG_ID}/reviews"),
                Some("{}"),
            ),
            (
                Method::GET,
                format!("/api/v1/organizations/{ORG_ID}/reviews/RV-A1"),
                None,
            ),
            (
                Method::GET,
                format!("/api/v1/organizations/{ORG_ID}/reviews/RV-A1/diff"),
                None,
            ),
            (
                Method::POST,
                format!("/api/v1/organizations/{ORG_ID}/reviews/RV-A1/route"),
                Some("{}"),
            ),
            (
                Method::POST,
                format!("/api/v1/organizations/{ORG_ID}/reviews/RV-A1/findings"),
                Some("{}"),
            ),
            (
                Method::POST,
                format!("/api/v1/organizations/{ORG_ID}/reviews/RV-A1/verdict"),
                Some("{}"),
            ),
            (
                Method::POST,
                format!("/api/v1/organizations/{ORG_ID}/reviews/RV-A1/resubmit"),
                Some("{}"),
            ),
            (
                Method::POST,
                format!("/api/v1/organizations/{ORG_ID}/reviews/RV-A1/close"),
                Some("{}"),
            ),
        ];

        for (method, path, body) in &endpoints {
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

            // Cookie without proof header -> 401.
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
        }
        assert!(office.recorded().is_empty());
    });
}

#[test]
fn ingress_enforces_host_origin_csrf_json_and_body_bounds_on_review_routes() {
    run_async(async {
        let host = standard_host();
        let office = standard_review_office();
        let gw = build_gateway(Arc::clone(&host), Arc::clone(&office));
        let (cookie, proof) = sign_in(&gw).await;
        let creds = Some((cookie.as_str(), proof.as_str()));

        let list_path = format!("/api/v1/organizations/{ORG_ID}/reviews");
        let close_path = format!("/api/v1/organizations/{ORG_ID}/reviews/RV-A1/close");
        let close_body = format!(r#"{{"command_id":"{CMD_ID}","expected_revision":1}}"#);

        // Wrong Host header -> 400.
        let mut wrong_host = Request::builder()
            .method(Method::GET)
            .uri(&list_path)
            .header(header::HOST, "evil.example:8080")
            .header("sec-fetch-site", "same-origin")
            .header(header::COOKIE, cookie.as_str())
            .header(PROOF_HEADER, proof.as_str())
            .body(Full::new(Bytes::new()))
            .unwrap();
        wrong_host
            .extensions_mut()
            .insert(PeerAddress(PEER.parse::<SocketAddr>().unwrap()));
        assert_eq!(
            gw.respond(wrong_host).await.status(),
            StatusCode::BAD_REQUEST
        );

        // Missing Origin on POST -> 403.
        let resp = gw
            .respond(make_request(
                Method::POST,
                &close_path,
                creds,
                Some(&close_body),
                None,
                &[("sec-fetch-site", "same-origin")],
            ))
            .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Wrong Origin on POST -> 403.
        let resp = gw
            .respond(make_request(
                Method::POST,
                &close_path,
                creds,
                Some(&close_body),
                Some("https://evil.example"),
                &[("sec-fetch-site", "same-origin")],
            ))
            .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Cross-site Sec-Fetch-Site -> 403.
        let resp = gw
            .respond(make_request(
                Method::POST,
                &close_path,
                creds,
                Some(&close_body),
                Some(ORIGIN),
                &[("sec-fetch-site", "cross-site")],
            ))
            .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Non-JSON Content-Type -> 415.
        let mut non_json = Request::builder()
            .method(Method::POST)
            .uri(&close_path)
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN)
            .header("sec-fetch-site", "same-origin")
            .header(header::CONTENT_TYPE, "text/plain")
            .header(header::COOKIE, cookie.as_str())
            .header(PROOF_HEADER, proof.as_str())
            .body(Full::new(Bytes::from(close_body.clone())))
            .unwrap();
        non_json
            .extensions_mut()
            .insert(PeerAddress(PEER.parse::<SocketAddr>().unwrap()));
        assert_eq!(
            gw.respond(non_json).await.status(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE
        );

        // Oversized JSON body (> MAX_BUSINESS_BODY_BYTES) -> 413 Payload Too Large.
        let huge = format!(
            r#"{{"command_id":"{CMD_ID}","expected_revision":1,"reason":"{}"}}"#,
            "x".repeat(crate::body::MAX_BUSINESS_BODY_BYTES + 1)
        );
        let resp = gw
            .respond(make_request(
                Method::POST,
                &close_path,
                creds,
                Some(&huge),
                Some(ORIGIN),
                &[("sec-fetch-site", "same-origin")],
            ))
            .await;
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);

        // Control-character review_ref (both ASCII NUL %00 and Unicode C1 NEL %C2%85) -> 400.
        for bad_uri in [
            format!("/api/v1/organizations/{ORG_ID}/reviews/RV-A1%00"),
            format!("/api/v1/organizations/{ORG_ID}/reviews/RV-A1%C2%85"),
            format!("/api/v1/organizations/{ORG_ID}/reviews/{}", "R".repeat(129)),
        ] {
            let resp = gw
                .respond(make_request(
                    Method::GET,
                    &bad_uri,
                    creds,
                    None,
                    Some(ORIGIN),
                    &[("sec-fetch-site", "same-origin")],
                ))
                .await;
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "{bad_uri}");
        }

        // Unknown JSON fields -> 400 before touching office.
        let unknown_close =
            format!(r#"{{"command_id":"{CMD_ID}","expected_revision":1,"evil":1}}"#);
        let resp = gw
            .respond(make_request(
                Method::POST,
                &close_path,
                creds,
                Some(&unknown_close),
                Some(ORIGIN),
                &[("sec-fetch-site", "same-origin")],
            ))
            .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Duplicate JSON keys on /findings -> 400 before touching office.
        let findings_path = format!("/api/v1/organizations/{ORG_ID}/reviews/RV-A1/findings");
        for dup_body in [
            format!(
                r#"{{"command_id":"{CMD_ID}","command_id":"{CMD_ID}","action":"resolve","finding_id":"{FINDING_ID}","reply_id":"{REPLY_ID}"}}"#
            ),
            format!(
                r#"{{"command_id":"{CMD_ID}","action":"resolve","action":"reopen","finding_id":"{FINDING_ID}","reply_id":"{REPLY_ID}"}}"#
            ),
        ] {
            let resp = gw
                .respond(make_request(
                    Method::POST,
                    &findings_path,
                    creds,
                    Some(&dup_body),
                    Some(ORIGIN),
                    &[("sec-fetch-site", "same-origin")],
                ))
                .await;
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        }

        assert!(office.recorded().is_empty());
    });
}

#[test]
fn all_nine_review_routes_round_trip_with_no_store_and_exact_rpc_params() {
    run_async(async {
        let host = standard_host();
        let office = standard_review_office();
        let gw = build_gateway(Arc::clone(&host), Arc::clone(&office));
        let (cookie, proof) = sign_in(&gw).await;
        let creds = Some((cookie.as_str(), proof.as_str()));

        // 1. GET /reviews (list with filters)
        let (status, headers, body) = collect_json(
            gw.respond(make_request(
                Method::GET,
                &format!(
                    "/api/v1/organizations/{ORG_ID}/reviews?status=open&kind=code&reviewer_id={REVIEWER_ID}&author_id={USER_ID}&limit=10&cursor=2"
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
        assert_eq!(headers.get(header::CACHE_CONTROL).unwrap(), "no-store");
        assert!(headers.get("access-control-allow-origin").is_none());
        assert_eq!(body["result"]["total"], 1);

        // 2. POST /reviews (review.request)
        let req_body = format!(
            r#"{{"command_id":"{CMD_ID}","review_id":"{REVIEW_ID}","target_kind":"doc","title":"Design spec","kind_slug":"doc-review","target_ref":"docs/spec.md","routing":{{"mode":"explicit","reviewer_ids":["{REVIEWER_ID}"]}}}}"#
        );
        let (status, headers, body) = collect_json(
            gw.respond(make_request(
                Method::POST,
                &format!("/api/v1/organizations/{ORG_ID}/reviews"),
                creds,
                Some(&req_body),
                Some(ORIGIN),
                &[("sec-fetch-site", "same-origin")],
            ))
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.get(header::CACHE_CONTROL).unwrap(), "no-store");
        assert_eq!(body["result"]["review"]["key"], "RV-A1");

        // 3. GET /reviews/{review_ref} (inspect with fields & round filter)
        let (status, _, body) = collect_json(
            gw.respond(make_request(
                Method::GET,
                &format!(
                    "/api/v1/organizations/{ORG_ID}/reviews/RV-A1?fields=rounds,findings&round=2"
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
        assert_eq!(body["result"]["rounds"].as_array().unwrap().len(), 1);
        assert_eq!(body["result"]["rounds"][0]["round_number"], 2);
        assert_eq!(body["result"]["findings"].as_array().unwrap().len(), 1);
        assert_eq!(body["result"]["findings"][0]["round_number"], 2);

        // 4. GET /reviews/{review_ref}/diff (round, compare_round, path, start_line, end_line)
        let (status, _, body) = collect_json(
            gw.respond(make_request(
                Method::GET,
                &format!("/api/v1/organizations/{ORG_ID}/reviews/RV-A1/diff?round=2&compare_round=1&path=src%2Flib.rs&start_line=1&end_line=10"),
                creds,
                None,
                Some(ORIGIN),
                &[("sec-fetch-site", "same-origin")],
            ))
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let diff_text = body["result"]["unified_diff"].as_str().unwrap();
        assert!(diff_text.contains("fn b()"));
        assert!(!diff_text.contains("very_far"));

        // 5. POST /reviews/{review_ref}/route
        let route_body = format!(
            r#"{{"command_id":"{CMD_ID}","expected_revision":3,"routing":{{"mode":"pool","team_id":"team-core"}}}}"#
        );
        let (status, _, _) = collect_json(
            gw.respond(make_request(
                Method::POST,
                &format!("/api/v1/organizations/{ORG_ID}/reviews/RV-A1/route"),
                creds,
                Some(&route_body),
                Some(ORIGIN),
                &[("sec-fetch-site", "same-origin")],
            ))
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // 6. POST /reviews/{review_ref}/findings (add + resolve)
        let add_finding = format!(
            r#"{{"command_id":"{CMD_ID}","action":"add","finding_id":"{FINDING_ID}","severity":"major","anchor":{{"kind":"file_line","path":"src/lib.rs","start_line":10,"end_line":12}},"body_markdown":"Check bounds"}}"#
        );
        let (status, _, _) = collect_json(
            gw.respond(make_request(
                Method::POST,
                &format!("/api/v1/organizations/{ORG_ID}/reviews/RV-A1/findings"),
                creds,
                Some(&add_finding),
                Some(ORIGIN),
                &[("sec-fetch-site", "same-origin")],
            ))
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // 7. POST /reviews/{review_ref}/verdict
        let verdict_body = format!(
            r#"{{"command_id":"{CMD_ID}","round_number":2,"verdict":"approve","summary_markdown":"LGTM"}}"#
        );
        let (status, _, _) = collect_json(
            gw.respond(make_request(
                Method::POST,
                &format!("/api/v1/organizations/{ORG_ID}/reviews/RV-A1/verdict"),
                creds,
                Some(&verdict_body),
                Some(ORIGIN),
                &[("sec-fetch-site", "same-origin")],
            ))
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // 8. POST /reviews/{review_ref}/resubmit
        let resubmit_body = format!(
            r#"{{"command_id":"{CMD_ID}","expected_revision":3,"fresh":true,"note_markdown":"Addressed feedback"}}"#
        );
        let (status, _, _) = collect_json(
            gw.respond(make_request(
                Method::POST,
                &format!("/api/v1/organizations/{ORG_ID}/reviews/RV-A1/resubmit"),
                creds,
                Some(&resubmit_body),
                Some(ORIGIN),
                &[("sec-fetch-site", "same-origin")],
            ))
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // 9. POST /reviews/{review_ref}/close
        let close_body =
            format!(r#"{{"command_id":"{CMD_ID}","expected_revision":3,"reason":"Merged"}}"#);
        let (status, _, _) = collect_json(
            gw.respond(make_request(
                Method::POST,
                &format!("/api/v1/organizations/{ORG_ID}/reviews/RV-A1/close"),
                creds,
                Some(&close_body),
                Some(ORIGIN),
                &[("sec-fetch-site", "same-origin")],
            ))
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let recorded = office.recorded();
        let methods: Vec<&str> = recorded.iter().map(|(m, _, _)| m.as_str()).collect();
        assert_eq!(
            methods,
            vec![
                "review.list",
                "review.request",
                "review.inspect",
                "review.diff",
                "review.route",
                "review.finding",
                "review.verdict",
                "review.resubmit",
                "review.close",
            ]
        );
        assert_eq!(recorded[0].1["status"], "open");
        assert_eq!(recorded[0].1["target_kind"], "code");
        assert_eq!(recorded[0].1["created_by"], USER_ID);
        assert_eq!(recorded[0].1["offset"], 2);
        assert_eq!(recorded[2].1["review"], "RV-A1");
        assert_eq!(recorded[2].1["fields"], json!(["rounds", "findings"]));
        assert_eq!(recorded[3].1["since_round"], 1);
        assert_eq!(recorded[3].1["path"], "src/lib.rs");
        assert_eq!(recorded[4].1["review"], "RV-A1");
        assert_eq!(recorded[5].1["op"]["action"], "add");
        assert_eq!(recorded[6].1["verdict"], "approve");
        assert_eq!(recorded[7].1["fresh"], true);
        assert_eq!(recorded[8].1["reason"], "Merged");
    });
}

#[test]
fn internal_storage_errors_never_leak_to_browser_clients() {
    run_async(async {
        let host = standard_host();
        let office = Arc::new(FakeOffice::new(|_, _, _, _, _| {
            Err(OfficeFailure::Refused(ApiError::new(
                ErrorCode::StorageError,
                "sqlite table reviews corrupted at /private/org/db.sqlite",
            )))
        }));
        let gw = build_gateway(Arc::clone(&host), Arc::clone(&office));
        let (cookie, proof) = sign_in(&gw).await;

        let (status, _, body) = collect_json(
            gw.respond(make_request(
                Method::GET,
                &format!("/api/v1/organizations/{ORG_ID}/reviews"),
                Some((cookie.as_str(), proof.as_str())),
                None,
                Some(ORIGIN),
                &[("sec-fetch-site", "same-origin")],
            ))
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        let serialized = body.to_string();
        assert!(!serialized.contains("sqlite"));
        assert!(!serialized.contains("/private/org"));
        assert_eq!(body["error"]["code"], "unavailable");
    });
}
