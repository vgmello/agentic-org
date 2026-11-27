//! Task route parsing and refusal mapping (`TASK-77`, `TASK-105`, `TASK-88`).

use super::*;

#[test]
fn task_list_query_parses_multi_labels_sort_and_rejects_unknown_params() {
    let p = parse_list_query(Some(
        "type=bug&status=in_progress&labels=backend,security&label=p0&label_match=any&sort=order&q=auth+token&limit=25",
    ))
    .ok()
    .expect("valid query");
    assert_eq!(p.task_type.as_deref(), Some("bug"));
    assert_eq!(p.status, Some(TaskStatus::InProgress));
    assert_eq!(p.labels, vec!["backend", "security", "p0"]);
    assert_eq!(p.label_match, LabelMatchMode::Any);
    assert_eq!(p.sort, TaskSortMode::Order);
    assert_eq!(p.query.as_deref(), Some("auth token"));
    assert_eq!(p.limit, Some(25));

    assert!(parse_list_query(Some("unknown=1")).is_err());
    assert!(parse_list_query(Some("status=bogus")).is_err());
    assert!(parse_list_query(Some("sort=random")).is_err());

    // `TASK-105`: the lifecycle scope and the done-since instant pass through.
    let board = parse_list_query(Some(
        "status_scope=open&done_since=2026-11-20T00%3A00%3A00Z&project_id=p",
    ))
    .ok()
    .expect("valid board query");
    assert_eq!(board.status_scope, Some(TaskStatusScope::Open));
    assert_eq!(board.done_since.as_deref(), Some("2026-11-20T00:00:00Z"));
    assert_eq!(board.project_id.as_deref(), Some("p"));
    assert_eq!(parse_list_query(None).ok().unwrap().status_scope, None);
    assert!(parse_list_query(Some("status_scope=recent")).is_err());
    assert!(parse_list_query(Some("status_scope=Open")).is_err());

    let lp = parse_labels_query(Some("q=sec"))
        .ok()
        .expect("valid labels query");
    assert_eq!(lp.query.as_deref(), Some("sec"));
    assert!(parse_labels_query(Some("bad=1")).is_err());

    // UTF-8 percent-decoding + malformed escape rejection + reserved action guard.
    let utf8 = parse_list_query(Some("q=caf%C3%A9"))
        .ok()
        .expect("valid utf8 query");
    assert_eq!(utf8.query.as_deref(), Some("café"));
    assert!(parse_list_query(Some("q=%ZZ")).is_err());
    assert!(parse_list_query(Some("q=%2")).is_err());
    assert!(is_reserved_task_action("update"));
    assert!(is_reserved_task_action("notes"));
    assert!(!is_reserved_task_action("T-A1"));
}

fn org() -> String {
    "0193a5c0-0000-7000-8000-000000000001".to_owned()
}

fn status_of(reply: &ApiReply) -> StatusCode {
    reply.status()
}

#[test]
fn checklist_incomplete_is_a_conflict_that_keeps_its_code_and_message() {
    let refused = org_proto::ApiError::new(
        org_proto::ErrorCode::ChecklistIncomplete,
        "required checklist steps are not done: [dod] Tests pass",
    );
    let reply = map_office_failure(OfficeFailure::Refused(refused));
    assert_eq!(status_of(&reply), StatusCode::CONFLICT);
    let body: Value = serde_json::from_slice(reply.body_bytes()).unwrap();
    assert_eq!(body["error"]["code"], "checklist_incomplete");
    assert!(body["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Tests pass"));
    // Its neighbours keep their classes.
    let stale = map_office_failure(OfficeFailure::Refused(org_proto::ApiError::new(
        org_proto::ErrorCode::Conflict,
        "stale",
    )));
    assert_eq!(status_of(&stale), StatusCode::CONFLICT);
    let denied = map_office_failure(OfficeFailure::Refused(org_proto::ApiError::new(
        org_proto::ErrorCode::NotAuthorized,
        "only the owner",
    )));
    assert_eq!(status_of(&denied), StatusCode::FORBIDDEN);
}

#[test]
fn final_summary_and_default_dod_payloads_decode_strictly() {
    let auth = CallerAuth::LocalAuthless;
    let summary = br#"{"command_id":"0193a5c0-0000-7000-8000-0000000000aa","task":"T-A1","expected_revision":3,"final_summary":"Shipped"}"#;
    assert!(prepare(TaskRoute::FinalSummary(auth, org()), summary).is_ok());
    // The summary is required, and the body is JSON.
    let missing = br#"{"command_id":"0193a5c0-0000-7000-8000-0000000000aa","task":"T-A1","expected_revision":3}"#;
    assert!(prepare(TaskRoute::FinalSummary(auth, org()), missing).is_err());
    assert!(prepare(TaskRoute::FinalSummary(auth, org()), b"not json").is_err());

    // Reading the organization's default takes no body.
    assert!(prepare(TaskRoute::DefaultDod(auth, org()), b"").is_ok());
    assert!(prepare(TaskRoute::DefaultDod(auth, org()), b"{}").is_err());

    let set = br#"{"command_id":"0193a5c0-0000-7000-8000-0000000000ab","expected_revision":1,"default_dod":["Tests pass"]}"#;
    assert!(prepare(TaskRoute::SetDefaultDod(auth, org()), set).is_ok());
    let clear = br#"{"command_id":"0193a5c0-0000-7000-8000-0000000000ab","expected_revision":1}"#;
    assert!(prepare(TaskRoute::SetDefaultDod(auth, org()), clear).is_ok());
    assert!(prepare(TaskRoute::SetDefaultDod(auth, org()), b"[]").is_err());

    let project = br#"{"command_id":"0193a5c0-0000-7000-8000-0000000000ac","project":"P-1","expected_revision":2,"default_dod":[]}"#;
    assert!(prepare(TaskRoute::SetProjectDefaultDod(auth, org()), project).is_ok());
    let no_project =
        br#"{"command_id":"0193a5c0-0000-7000-8000-0000000000ac","expected_revision":2}"#;
    assert!(prepare(TaskRoute::SetProjectDefaultDod(auth, org()), no_project).is_err());

    // The new action name is reserved, so it never reads as a task reference.
    assert!(is_reserved_task_action("final-summary"));
}
