//! The retired phase-1 work calls (`task.*`, `bug.*`, `checklist.*`, `work.*`) fail closed on
//! the office socket: whatever the binding and the params, each is `unknown_method`, exactly
//! like a method the office never knew, and no office port is touched on the way.
use super::*;
use org_proto::Outcome;
use serde_json::{json, Value};

/// Every method of the retired phase-1 work contract: its 25 writes (the grant-only
/// `work.delete` among them), `work.set_current` and its 5 reads. Host-channel tests share it.
pub(crate) const RETIRED: &[&str] = &[
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
    "work.set_current",
    "work.get",
    "work.list",
    "work.snapshot",
    "work.times",
    "work.activity",
];

fn shared() -> Shared {
    let mut ports = ports(Box::new(|| {
        panic!("a retired work call read the organization")
    }));
    ports.submit = Box::new(|_, _, _| panic!("a retired work call submitted an org command"));
    ports.operation_get = Box::new(|_, _| panic!("a retired work call looked up an operation"));
    ports.who = Box::new(|_| panic!("a retired work call read the directory"));
    ports.status = Box::new(|_, _| panic!("a retired work call set a status line"));
    Shared::new(
        &test_info(),
        Path::new("/unused-retired-work-socket"),
        ports,
        launch_key(),
    )
}

fn request(method: &str, params: Value) -> Request {
    Request {
        id: 73,
        method: method.into(),
        principal: None,
        params,
    }
}

fn member(via_grant: bool) -> Binding {
    Binding::Member {
        id: "member-id".into(),
        via_grant,
    }
}

fn assert_unknown(response: &Response, method: &str) {
    assert_eq!(response.id, 73, "{method}");
    match &response.outcome {
        Outcome::Error(error) => {
            assert_eq!(error.code, ErrorCode::UnknownMethod, "{method}");
            assert_eq!(
                error.message,
                format!("this office does not know `{method}`"),
                "{method}"
            );
        }
        other => panic!("{method}: expected unknown_method, got {other:?}"),
    }
}

#[test]
fn retired_work_methods_are_unknown_for_every_binding_and_params_shape() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let shared = shared();
    // The params the retired writes took, garbage, and none at all: none of them revives a call.
    let shapes = [
        json!({"change_id": "01ARZ3NDEKTSV4RRFFQ69G5FAV", "item_id": "item",
            "project_id": "project", "title": "Title", "expected_version": 1}),
        json!({"command_id": "01ARZ3NDEKTSV4RRFFQ69G5FAV"}),
        json!({}),
        Value::Null,
    ];
    for method in RETIRED {
        for params in &shapes {
            for mut binding in [Binding::Anonymous, member(false), member(true)] {
                let response = answer(&request(method, params.clone()), &shared, &mut binding);
                assert_unknown(&response, method);
            }
        }
    }
}
