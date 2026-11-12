use super::*;
use org_proto::work::{TaskCreate, TaskCreateParams};
use org_proto::Outcome;
use serde_json::{json, Value};

type Seen = Arc<Mutex<Vec<(Caller, WorkCall)>>>;
type Reply = Result<Result<WorkAccepted, ApiError>, String>;

fn harness(reply: Reply) -> (Shared, Seen) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&seen);
    let mut ports = ports(Box::new(|| Err(SnapshotFailure::Storage)));
    ports.work = Box::new(move |caller, call| {
        recorded.lock().unwrap().push((caller, call));
        reply.clone()
    });
    (
        Shared::new(&test_info(), Path::new("/unused-c1-socket"), ports, token()),
        seen,
    )
}

fn request(method: &str, valid: bool) -> Request {
    Request {
        id: 73,
        method: method.into(),
        principal: None,
        params: if valid {
            json!({"change_id":"change", "item_id":"item", "project_id":"project", "title":"Title", "expected_version":1})
        } else {
            Value::Null
        },
    }
}

fn member(app: bool) -> Binding {
    Binding::Member {
        id: "member-id".into(),
        with_app_token: app,
    }
}

fn accepted() -> WorkAccepted {
    serde_json::from_value(
        json!({"status":"committed", "position":{"incarnation":2,"seq":5},"items":[]}),
    )
    .unwrap()
}

fn assert_error(response: Response, expected: ErrorCode, message: Option<&str>) {
    assert_eq!(response.id, 73);
    match response.outcome {
        Outcome::Error(error) => {
            assert_eq!(error.code, expected);
            if let Some(message) = message {
                assert_eq!(error.message, message);
            }
        }
        other => panic!("expected refusal, got {other:?}"),
    }
}

#[test]
fn c1_work_guard_matrix_checks_binding_and_app_only_before_params() {
    for binding in [
        Binding::Anonymous,
        Binding::App,
        member(false),
        member(true),
    ] {
        for method in ["task.create", "work.delete"] {
            for valid in [false, true] {
                let (shared, seen) = harness(Ok(Ok(accepted())));
                let req = request(method, valid);
                let response = answer(&req, &shared, &mut binding.clone());
                let expected = match &binding {
                    Binding::Anonymous | Binding::App => Some(ErrorCode::NotBound),
                    Binding::Member {
                        with_app_token: false,
                        ..
                    } if method == "work.delete" => Some(ErrorCode::NotAuthorized),
                    _ if !valid => Some(ErrorCode::InvalidRequest),
                    _ => None,
                };
                if let Some(code) = expected {
                    let message = match code {
                        ErrorCode::NotBound => Some("bind as a member to change work"),
                        ErrorCode::NotAuthorized => Some("`work.delete` is for the app"),
                        _ => None,
                    };
                    assert_error(response, code, message);
                    assert!(
                        seen.lock().unwrap().is_empty(),
                        "{binding:?}/{method}/{valid}"
                    );
                } else {
                    assert_eq!(
                        response.outcome,
                        Outcome::Result(serde_json::to_value(accepted()).unwrap())
                    );
                    let calls = seen.lock().unwrap();
                    let Binding::Member { with_app_token, .. } = binding else {
                        unreachable!()
                    };
                    assert_eq!(
                        calls.as_slice(),
                        &[(
                            if with_app_token {
                                Caller::app("member-id")
                            } else {
                                Caller::agent("member-id")
                            },
                            WorkCall::parse(&req).unwrap().unwrap()
                        )]
                    );
                }
            }
        }
    }
}

#[test]
fn c1_all_25_work_methods_check_binding_then_their_typed_params() {
    for method in [
        "task.create",
        "task.update",
        "task.assign",
        "task.set_status",
        "work.comment",
        "work.watch",
        "work.link",
        "work.unlink",
        "work.undo",
        "work.delete",
        "checklist.add",
        "checklist.edit",
        "checklist.check",
        "checklist.move",
        "checklist.remove",
        "checklist.make_task",
        "checklist.unlink_task",
        "bug.report",
        "bug.update",
        "bug.triage",
        "bug.assign",
        "bug.mark_fixed",
        "bug.verify",
        "bug.close",
        "bug.reopen",
    ] {
        let (shared, seen) = harness(Err("must not be called".into()));
        let req = request(method, false);
        assert_error(
            answer(&req, &shared, &mut Binding::Anonymous),
            ErrorCode::NotBound,
            None,
        );
        assert_error(
            answer(&req, &shared, &mut Binding::App),
            ErrorCode::NotBound,
            None,
        );
        assert_error(
            answer(&req, &shared, &mut member(true)),
            ErrorCode::InvalidRequest,
            None,
        );
        assert!(seen.lock().unwrap().is_empty(), "{method}");
    }
}

#[test]
fn c1_work_port_maps_success_refusal_and_storage_without_leaking_details() {
    for reply in [
        Ok(Ok(accepted())),
        Ok(Err(ApiError::new(ErrorCode::Conflict, "native conflict"))),
        Err("private storage detail".into()),
    ] {
        let (shared, seen) = harness(reply.clone());
        let req = request("task.create", true);
        let response = answer(&req, &shared, &mut member(false));
        assert_eq!(seen.lock().unwrap().len(), 1);
        match reply {
            Ok(Ok(value)) => assert_eq!(
                response.outcome,
                Outcome::Result(serde_json::to_value(value).unwrap())
            ),
            Ok(Err(error)) => assert_error(response, error.code, Some(&error.message)),
            Err(_) => assert_error(
                response,
                ErrorCode::StorageError,
                Some("the office could not answer"),
            ),
        }
    }
}

#[test]
fn c1_leaves_later_work_calls_unknown_and_org_dispatch_unchanged() {
    let (shared, seen) = harness(Err("must not be called".into()));
    for method in [
        "work.set_current",
        "work.get",
        "work.list",
        "work.snapshot",
        "work.activity",
        "work.feed_after",
    ] {
        assert_error(
            answer(&request(method, false), &shared, &mut member(true)),
            ErrorCode::UnknownMethod,
            None,
        );
    }
    assert_error(
        answer(
            &request("team.create", false),
            &shared,
            &mut Binding::Anonymous,
        ),
        ErrorCode::NotAuthorized,
        Some("organization commands are for the app"),
    );
    assert_error(
        answer(&request("team.create", false), &shared, &mut member(true)),
        ErrorCode::InvalidRequest,
        None,
    );
    assert!(seen.lock().unwrap().is_empty());
}

#[test]
fn c1_app_only_request_classifier_ignores_bad_params_and_shares_call_policy() {
    for (method, app_only) in [
        ("work.delete", true),
        ("task.create", false),
        ("work.get", false),
    ] {
        assert_eq!(WorkCall::is_app_only(&request(method, false)), app_only);
        if let Some(call) = WorkCall::parse(&request(method, true)).unwrap() {
            assert_eq!(call.app_only(), app_only);
        }
    }
    let params: TaskCreateParams = request("task.create", true).params::<TaskCreate>().unwrap();
    assert_eq!(params.title, "Title");
}
