//! `attachment.*` reach the office's attachment port through the real dispatch, for a person
//! on a host channel and an agent on the office socket alike; an unbound connection is
//! refused, and an externally bound agent may call them (`TASK-98.1`).

use super::*;
use org_office::TestOffice;
use org_proto::attachments::{AttachmentList, AttachmentListParams, AttachmentScope};
use org_proto::Outcome;

fn harness() -> (Shared, String) {
    let office = Arc::new(org_office::Office::for_test(TestOffice::default()));
    let owner = office.owner_member_id().to_owned();
    let mut ports = ports(Box::new(|| Err(SnapshotFailure::Storage)));
    ports.attachments = Some(Box::new(move |member, request| {
        crate::attachment_calls::dispatch_attachment_rpc(&office, member, request)
    }));
    (
        Shared::new(
            &test_info(),
            Path::new("/unused-attachments"),
            ports,
            launch_key(),
        ),
        owner,
    )
}

fn private_list() -> Request {
    Request::new::<AttachmentList>(
        41,
        &AttachmentListParams {
            target_kind: None,
            target_id: None,
            scope: Some(AttachmentScope::UserPrivate),
            limit: None,
            cursor: None,
        },
    )
    .unwrap()
}

#[test]
fn attachment_calls_route_to_the_port_for_grant_bound_people_and_bound_agents() {
    let (shared, owner) = harness();
    for via_grant in [true, false] {
        let response = shared.answer_for_test(&private_list(), &owner, via_grant);
        assert!(
            matches!(response.outcome, Outcome::Result(_)),
            "{response:?}"
        );
    }
    let mut anonymous = Binding::Anonymous;
    let response = answer(&private_list(), &shared, &mut anonymous);
    assert!(
        matches!(response.outcome, Outcome::Error(ref e) if e.code == ErrorCode::NotBound),
        "{response:?}"
    );
}

#[test]
fn without_a_port_attachment_calls_are_unknown_and_external_agents_may_call_them() {
    let shared = Shared::new(
        &test_info(),
        Path::new("/unused-attachments"),
        ports(Box::new(|| Err(SnapshotFailure::Storage))),
        launch_key(),
    );
    let response = shared.answer_for_test(&private_list(), OWNER_ID, false);
    assert!(
        matches!(response.outcome, Outcome::Error(ref e) if e.code == ErrorCode::UnknownMethod),
        "{response:?}"
    );
    for method in org_proto::attachments::ATTACHMENT_METHODS {
        assert!(
            super::super::external::external_may_call(method),
            "{method}"
        );
    }
}
