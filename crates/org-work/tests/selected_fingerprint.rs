use org_work::selected_assignment::{assignment_fingerprint, AssignmentCommand, AssignmentScope};

fn scope() -> AssignmentScope<'static> {
    AssignmentScope {
        instance_id: "instance",
        organization_id: "org",
        office_id: "office",
        task_id: "opaque task 🦀",
    }
}

#[test]
fn typed_fingerprint_is_independent_of_input_escaping_and_revision_transport() {
    let first: AssignmentCommand = serde_json::from_str(r#"{"expected_revision":9223372036854775807,"assigned_team_id":null,"assignee_id":"employee 🦀"}"#).unwrap();
    let second: AssignmentCommand = serde_json::from_str(r#"{"assignee_id":"employ\u0065e \ud83e\udd80","assigned_team_id":null,"expected_revision":9223372036854775807}"#).unwrap();
    let hash = assignment_fingerprint(scope(), &first).unwrap();
    assert_eq!(hash, assignment_fingerprint(scope(), &second).unwrap());
    let native = org_proto::selected_tasks::decode_http_assignment(br#"{"operation_id":"01J00000000000000000000000","scope":{"kind":"office","instance_id":"instance","org_id":"org","office_id":"office"},"params":{"expected_revision":"9223372036854775807","assigned_team_id":null,"assignee_id":"employee \ud83e\udd80"}}"#).unwrap().params.into_native();
    let adapted = AssignmentCommand {
        expected_revision: native.expected_revision.get(),
        assigned_team_id: native.assigned_team_id.0,
        assignee_id: native.assignee_id.0,
    };
    assert_eq!(hash, assignment_fingerprint(scope(), &adapted).unwrap());
}

#[test]
fn every_typed_intent_and_scope_component_is_bound_without_normalizing_ids() {
    let command = AssignmentCommand {
        expected_revision: 4,
        assigned_team_id: None,
        assignee_id: None,
    };
    let expected = assignment_fingerprint(scope(), &command).unwrap();
    for modified in [
        AssignmentCommand {
            expected_revision: 5,
            ..command.clone()
        },
        AssignmentCommand {
            assigned_team_id: Some("team".into()),
            ..command.clone()
        },
        AssignmentCommand {
            assignee_id: Some("employee".into()),
            ..command.clone()
        },
    ] {
        assert_ne!(
            expected,
            assignment_fingerprint(scope(), &modified).unwrap()
        );
    }
    for changed in [
        AssignmentScope {
            instance_id: "other",
            ..scope()
        },
        AssignmentScope {
            organization_id: "other",
            ..scope()
        },
        AssignmentScope {
            office_id: "other",
            ..scope()
        },
        AssignmentScope {
            task_id: "other",
            ..scope()
        },
    ] {
        assert_ne!(expected, assignment_fingerprint(changed, &command).unwrap());
    }
    let composed = AssignmentCommand {
        assigned_team_id: Some("é".into()),
        ..command.clone()
    };
    let decomposed = AssignmentCommand {
        assigned_team_id: Some("e\u{301}".into()),
        ..command
    };
    assert_ne!(
        assignment_fingerprint(scope(), &composed).unwrap(),
        assignment_fingerprint(scope(), &decomposed).unwrap()
    );
}
