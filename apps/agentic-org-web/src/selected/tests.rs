use super::*;
use http_body_util::Full;
use hyper::{body::Bytes, Request as HttpRequest};
use std::sync::atomic::{AtomicUsize, Ordering};
mod lookup;
const INSTANCE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
struct NoNative(AtomicUsize);
impl SessionAuthority for NoNative {
    type Ticket = ();
    fn authenticate(&self, request: &Parts) -> Result<(), ApiReply> {
        self.0.fetch_add(1, Ordering::SeqCst);
        if request
            .headers
            .get("X-Org-Session-Proof")
            .is_some_and(|v| v == "synthetic-proof")
        {
            Ok(())
        } else {
            Err(ApiReply::error(
                StatusCode::UNAUTHORIZED,
                Code::NotAuthorized,
                "Sign in required.",
            ))
        }
    }
    fn office(&self, _: (), _: &OfficeScope, _: Duration) -> Result<OfficeAccess, ApiReply> {
        panic!("decoding must not contact native services")
    }
}
fn backend() -> SelectedApi<NoNative> {
    SelectedApi::new(NoNative(AtomicUsize::new(0)))
}
fn route(api: &SelectedApi<NoNative>, method: Method, path: &str) -> Result<Route<()>, ApiReply> {
    let (parts, _) = HttpRequest::builder()
        .method(method)
        .uri(path)
        .header("X-Org-Session-Proof", "synthetic-proof")
        .body(())
        .unwrap()
        .into_parts();
    api.route(&parts, &RequestPath::parse(parts.uri.path()).unwrap())
}
fn body(bytes: &[u8]) -> BusinessBody {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(crate::body::read_business_body(
            Full::new(Bytes::copy_from_slice(bytes)),
            org_proto::MAX_FRAME_BYTES,
            Duration::from_secs(1),
        ))
        .unwrap()
}
fn assignment() -> String {
    format!(
        r#"{{"operation_id":"{INSTANCE}","scope":{{"kind":"office","instance_id":"{INSTANCE}","org_id":"org","office_id":"office"}},"params":{{"expected_revision":"9223372036854775807","assigned_team_id":null,"assignee_id":null}}}}"#
    )
}
#[test]
fn http_assignment_decodes_original_wrapper_scope_and_complete_pair_before_any_native_call() {
    let api = backend();
    let path = "/api/v1/organizations/org/tasks/opaque%20task/assignment";
    let good = assignment();
    assert!(api
        .prepare(
            route(&api, Method::POST, path).unwrap_or_else(|_| panic!("assignment route rejected")),
            body(good.as_bytes())
        )
        .is_ok());
    for raw in [
        good.replace(
            "\"operation_id\":",
            &format!("\"operation_id\":\"{INSTANCE}\",\"operation_id\":"),
        ),
        good.replace(
            "\"org_id\":\"org\"",
            "\"org_id\":\"other\",\"org_id\":\"org\"",
        ),
        good.replace(
            "\"assigned_team_id\":null",
            "\"assigned_team_id\":null,\"assigned\\u005fteam_id\":null",
        ),
        good.replace(
            "\"assignee_id\":null",
            "\"assignee_id\":null,\"actor\":\"owner\"",
        ),
        good.replace("\"9223372036854775807\"", "9223372036854775807"),
        good.replace("\"9223372036854775807\"", "\"9223372036854775808\""),
        good.replace("\"org_id\":\"org\"", "\"org_id\":\"other\""),
        good.replace(",\"assignee_id\":null", ""),
    ] {
        assert!(api
            .prepare(
                route(&api, Method::POST, path)
                    .unwrap_or_else(|_| panic!("assignment route rejected")),
                body(raw.as_bytes())
            )
            .is_err());
    }
}
#[test]
fn selected_query_decodes_escaped_duplicates_and_all_scope_fields_without_legacy_id_coercion() {
    let api = backend();
    let scope = format!("instance_id={INSTANCE}&office_id=opaque%20office");
    for tail in [
        "kind=status&kind=status",
        "kind=status&%6bind=status",
        "kind=status&office_id=other",
        "kind=status&actor=owner",
        "kind=list&queue=all&target_id=extra&limit=1",
        "kind=list&queue=assigned_team&limit=1",
        "kind=list&queue=all&limit=51",
        "kind=list&queue=all&limit=01",
        "kind=list&queue=all&limit=1.0",
        "kind=list&queue=all&limit=1&after=%ff",
        "kind=status&unused=1",
    ] {
        assert!(
            route(
                &api,
                Method::GET,
                &format!("/api/v1/organizations/org/tasks?{scope}&{tail}")
            )
            .is_err(),
            "{tail}"
        );
    }
    let detail = route(
        &api,
        Method::GET,
        &format!("/api/v1/organizations/org/tasks/opaque%20task?{scope}"),
    )
    .unwrap_or_else(|_| panic!("detail route rejected"));
    assert!(
        matches!(detail.request,Request::Read(Query::Detail {task_id}) if task_id=="opaque task")
    );
    let history=route(&api,Method::GET,&format!("/api/v1/organizations/org/tasks/opaque%20task/history?{scope}&after=9223372036854775807&limit=50")).unwrap_or_else(|_| panic!("selected route rejected"));
    assert!(
        matches!(history.request,Request::Read(Query::History {after:Some(r),..}) if r.get()==i64::MAX)
    );
    assert!(route(&api,Method::GET,&format!("/api/v1/operations/work/{INSTANCE}?instance_id={INSTANCE}&org_id=org&office_id=opaque%20office")).is_ok());
    assert!(route(
        &api,
        Method::GET,
        &format!("/api/v1/operations/work/{INSTANCE}?instance_id={INSTANCE}&office_id=office")
    )
    .is_err());
}
#[test]
fn task_scoped_chunks_are_byte_limited_and_bound_to_exact_lossless_reference() {
    let api = backend();
    let base=format!("/api/v1/organizations/org/tasks/task/content?instance_id={INSTANCE}&office_id=office&revision=9223372036854775807&kind=event&sha256={}&byte_length=16777216&offset=0&limit=262144","a".repeat(64));
    let parsed =
        route(&api, Method::GET, &base).unwrap_or_else(|_| panic!("content route rejected"));
    assert!(
        matches!(parsed.request,Request::Read(Query::Chunk {reference,limit:262144,offset:0,..}) if reference.revision==i64::MAX&&reference.byte_length==16777216&&reference.task_id=="task")
    );
    for changed in [
        base.replace("limit=262144", "limit=262145"),
        base.replace("limit=262144", "limit=0"),
        base.replace("offset=0", "offset=16777217"),
        base.replace("byte_length=16777216", "byte_length=16777217"),
        base.replace("revision=9223372036854775807", "revision=1e0"),
    ] {
        assert!(route(&api, Method::GET, &changed).is_err());
    }
}
#[test]
fn authority_is_checked_before_selected_route_details_and_request_body() {
    let api = backend();
    let (parts, _) = HttpRequest::builder()
        .method(Method::GET)
        .uri("/api/v1/organizations/hidden/tasks?malformed")
        .body(())
        .unwrap()
        .into_parts();
    assert!(api
        .route(&parts, &RequestPath::parse(parts.uri.path()).unwrap())
        .is_err());
    assert_eq!(api.authority.0.load(Ordering::SeqCst), 1);
}

#[test]
fn browser_receipt_keeps_full_revision_original_observed_state_and_flat_lookup_shape() {
    let id = OperationId::parse(INSTANCE.into()).unwrap();
    let reply = mutation(
        &id,
        AssignmentResult::Rejected {
            code: "conflict".into(),
            current: Some(task::CurrentAssignment {
                organization_id: "org".into(),
                office_id: "office".into(),
                task_id: "task".into(),
                revision: i64::MAX,
                assigned_team_id: Some("observed-at-original-barrier".into()),
                assignee_id: None,
            }),
        },
        true,
    );
    let (status, bytes) = reply.inspected();
    assert_eq!(status, StatusCode::OK);
    let value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    assert_eq!(value["result"]["kind"], "work");
    assert_eq!(value["result"]["outcome"], "rejected");
    assert_eq!(value["result"]["operation_id"], INSTANCE);
    assert_eq!(
        value["result"]["error"]["current"]["revision"],
        "9223372036854775807"
    );
    assert_eq!(
        value["result"]["error"]["current"]["assigned_team_id"],
        "observed-at-original-barrier"
    );
}
