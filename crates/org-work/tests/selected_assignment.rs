use org_work::selected_assignment::{
    plan_assignment, AssignmentCommand, AssignmentError, AssignmentPair, AssignmentTarget,
    DesiredTargetFacts, EmployeeFact, TargetProblem, TaskAssignmentSnapshot, TeamFact,
};
use serde_json::{json, Value};

fn pair(team: Option<&str>, employee: Option<&str>) -> AssignmentPair {
    AssignmentPair::new(team.map(str::to_owned), employee.map(str::to_owned))
}

fn states(team: &str, employee: &str) -> [AssignmentPair; 4] {
    [
        pair(None, None),
        pair(Some(team), None),
        pair(None, Some(employee)),
        pair(Some(team), Some(employee)),
    ]
}

fn snapshot(assignment: AssignmentPair) -> TaskAssignmentSnapshot {
    TaskAssignmentSnapshot {
        organization_id: "org".into(),
        task_id: "legacy-task-id".into(),
        revision: 7,
        assignment,
    }
}

fn command(desired: &AssignmentPair) -> AssignmentCommand {
    AssignmentCommand {
        expected_revision: 7,
        assigned_team_id: desired.assigned_team_id().map(str::to_owned),
        assignee_id: desired.assignee_id().map(str::to_owned),
    }
}

fn team() -> TeamFact {
    TeamFact {
        id: "team".into(),
        organization_id: "org".into(),
        active: true,
    }
}

fn employee() -> EmployeeFact {
    EmployeeFact {
        id: "employee".into(),
        organization_id: "org".into(),
        active: true,
    }
}

fn invalid_target(target: AssignmentTarget, problem: TargetProblem) -> AssignmentError {
    AssignmentError::InvalidTarget { target, problem }
}

fn assert_refusal(
    current: &TaskAssignmentSnapshot,
    desired: &AssignmentCommand,
    targets: DesiredTargetFacts<'_>,
    expected: AssignmentError,
) {
    let before = (
        current.clone(),
        desired.clone(),
        targets.team.cloned(),
        targets.employee.cloned(),
    );
    assert_eq!(plan_assignment(current, desired, targets), Err(expected));
    assert_eq!(
        (
            current.clone(),
            desired.clone(),
            targets.team.cloned(),
            targets.employee.cloned(),
        ),
        before,
        "a refusal must leave every supplied input unchanged"
    );
}

#[test]
fn all_sixteen_pair_transitions_plan_exact_change_payloads() {
    let team = team();
    let employee = employee();
    let targets = DesiredTargetFacts {
        team: Some(&team),
        employee: Some(&employee),
    };
    for from in states("old-team", "old-employee") {
        for to in states("team", "employee") {
            let current = snapshot(from.clone());
            let desired = command(&to);
            let before = (
                current.clone(),
                desired.clone(),
                team.clone(),
                employee.clone(),
            );
            let plan = plan_assignment(&current, &desired, targets).unwrap();

            assert_eq!(plan.organization_id(), "org");
            assert_eq!(plan.task_id(), "legacy-task-id");
            assert_eq!(plan.next_revision(), 8);
            assert_eq!(plan.change().from(), &from);
            assert_eq!(plan.change().to(), &to);
            assert_eq!(
                serde_json::to_value(plan.change()).unwrap(),
                json!({
                    "type": "assignment_changed",
                    "from": {
                        "assigned_team_id": from.assigned_team_id(),
                        "assignee_id": from.assignee_id()
                    },
                    "to": {
                        "assigned_team_id": to.assigned_team_id(),
                        "assignee_id": to.assignee_id()
                    }
                })
            );
            assert_eq!((current, desired, team.clone(), employee.clone()), before);
        }
    }
}

#[test]
fn clearing_one_assignment_preserves_the_explicit_other_assignment() {
    let current = snapshot(pair(Some("team"), Some("employee")));
    let team = team();
    let employee = employee();
    for to in [pair(Some("team"), None), pair(None, Some("employee"))] {
        // A cleared field needs no lookup, while the explicitly retained target does.
        let targets = DesiredTargetFacts {
            team: to.assigned_team_id().map(|_| &team),
            employee: to.assignee_id().map(|_| &employee),
        };
        let plan = plan_assignment(&current, &command(&to), targets).unwrap();
        assert_eq!(plan.change().to(), &to);
        assert_eq!(plan.change().from(), &current.assignment);
    }
}

#[test]
fn unchanged_pairs_still_plan_one_change_at_the_next_revision() {
    let team = team();
    let employee = employee();
    for assignment in states("team", "employee") {
        let current = snapshot(assignment.clone());
        let plan = plan_assignment(
            &current,
            &command(&assignment),
            DesiredTargetFacts {
                team: Some(&team),
                employee: Some(&employee),
            },
        )
        .unwrap();
        assert_eq!(plan.next_revision(), 8);
        assert_eq!(plan.change().from(), plan.change().to());
        assert_eq!(
            serde_json::to_value(plan.change()).unwrap()["type"],
            "assignment_changed"
        );
    }
}

#[test]
fn every_invalid_team_fact_is_rejected_including_for_unchanged_pairs() {
    let employee = employee();
    let cases = [
        (None, TargetProblem::MissingFact),
        (
            Some(TeamFact {
                id: "wrong-team".into(),
                ..team()
            }),
            TargetProblem::WrongId,
        ),
        (
            Some(TeamFact {
                organization_id: "other-org".into(),
                ..team()
            }),
            TargetProblem::WrongOrganization,
        ),
        (
            Some(TeamFact {
                active: false,
                ..team()
            }),
            TargetProblem::Inactive,
        ),
    ];
    let desired = command(&pair(Some("team"), Some("employee")));
    for current_pair in [
        pair(Some("old-team"), Some("old-employee")),
        pair(Some("team"), Some("employee")),
    ] {
        for (fact, problem) in &cases {
            assert_refusal(
                &snapshot(current_pair.clone()),
                &desired,
                DesiredTargetFacts {
                    team: fact.as_ref(),
                    employee: Some(&employee),
                },
                invalid_target(AssignmentTarget::Team, *problem),
            );
        }
    }
}

#[test]
fn every_invalid_employee_fact_is_rejected_including_for_unchanged_pairs() {
    let team = team();
    let cases = [
        (None, TargetProblem::MissingFact),
        (
            Some(EmployeeFact {
                id: "wrong-employee".into(),
                ..employee()
            }),
            TargetProblem::WrongId,
        ),
        (
            Some(EmployeeFact {
                organization_id: "other-org".into(),
                ..employee()
            }),
            TargetProblem::WrongOrganization,
        ),
        (
            Some(EmployeeFact {
                active: false,
                ..employee()
            }),
            TargetProblem::Inactive,
        ),
    ];
    let desired = command(&pair(Some("team"), Some("employee")));
    for current_pair in [
        pair(Some("old-team"), Some("old-employee")),
        pair(Some("team"), Some("employee")),
    ] {
        for (fact, problem) in &cases {
            assert_refusal(
                &snapshot(current_pair.clone()),
                &desired,
                DesiredTargetFacts {
                    team: Some(&team),
                    employee: fact.as_ref(),
                },
                invalid_target(AssignmentTarget::Employee, *problem),
            );
        }
    }
}

#[test]
fn empty_desired_ids_are_not_clearing_aliases_even_with_matching_facts() {
    let current = snapshot(pair(None, None));
    let empty_team = TeamFact {
        id: String::new(),
        ..team()
    };
    let empty_employee = EmployeeFact {
        id: String::new(),
        ..employee()
    };
    for (desired, target) in [
        (command(&pair(Some(""), None)), AssignmentTarget::Team),
        (command(&pair(None, Some(""))), AssignmentTarget::Employee),
    ] {
        assert_refusal(
            &current,
            &desired,
            DesiredTargetFacts {
                team: Some(&empty_team),
                employee: Some(&empty_employee),
            },
            invalid_target(target, TargetProblem::EmptyDesiredId),
        );
    }
}

#[test]
fn historical_references_need_no_facts_and_surplus_facts_cannot_assign_targets() {
    let current = snapshot(pair(
        Some("archived/legacy-team"),
        Some("inactive-employee"),
    ));
    let archived_team = TeamFact {
        id: "archived/legacy-team".into(),
        organization_id: "old-org".into(),
        active: false,
    };
    let inactive_employee = EmployeeFact {
        id: "inactive-employee".into(),
        organization_id: "old-org".into(),
        active: false,
    };
    let desired = command(&pair(None, None));
    let without_facts = plan_assignment(&current, &desired, DesiredTargetFacts::default()).unwrap();
    let with_surplus = plan_assignment(
        &current,
        &desired,
        DesiredTargetFacts {
            team: Some(&archived_team),
            employee: Some(&inactive_employee),
        },
    )
    .unwrap();
    assert_eq!(without_facts, with_surplus);
    assert_eq!(with_surplus.change().from(), &current.assignment);
    assert_eq!(with_surplus.change().to(), &pair(None, None));

    // Even arbitrary empty historical IDs are preserved, not treated as new desired targets.
    let historical = snapshot(pair(Some(""), Some("unresolvable")));
    let replacement =
        plan_assignment(&historical, &desired, DesiredTargetFacts::default()).unwrap();
    assert_eq!(replacement.change().from(), &historical.assignment);
}

#[test]
fn an_employee_can_be_assigned_independently_of_the_assigned_team() {
    let current = snapshot(pair(None, None));
    let employee = employee();
    // The employee projection has no home-team dependency. The same employee can be selected
    // alone or with either unrelated active team without changing any employee fact.
    for team_id in [None, Some("platform"), Some("payments")] {
        let team = team_id.map(|id| TeamFact {
            id: id.into(),
            ..team()
        });
        let desired = command(&pair(team_id, Some("employee")));
        let plan = plan_assignment(
            &current,
            &desired,
            DesiredTargetFacts {
                team: team.as_ref(),
                employee: Some(&employee),
            },
        )
        .unwrap();
        assert_eq!(plan.change().to().assigned_team_id(), team_id);
        assert_eq!(plan.change().to().assignee_id(), Some("employee"));
    }
}

#[test]
fn nonempty_opaque_ids_preserve_bytes_without_normalization_or_path_rules() {
    for (org_id, task_id, team_id, employee_id) in [
        (
            "  org/../e\u{301}\0 ",
            "old/B-7/任务",
            " team/../é ",
            "@Ana/../agent\0",
        ),
        (" ", "\t", "\n", "\r"),
    ] {
        let current = TaskAssignmentSnapshot {
            organization_id: org_id.into(),
            task_id: task_id.into(),
            ..snapshot(pair(None, None))
        };
        let team = TeamFact {
            id: team_id.into(),
            organization_id: org_id.into(),
            active: true,
        };
        let employee = EmployeeFact {
            id: employee_id.into(),
            organization_id: org_id.into(),
            active: true,
        };
        let desired = command(&pair(Some(team_id), Some(employee_id)));
        let plan = plan_assignment(
            &current,
            &desired,
            DesiredTargetFacts {
                team: Some(&team),
                employee: Some(&employee),
            },
        )
        .unwrap();
        assert_eq!(plan.organization_id().as_bytes(), org_id.as_bytes());
        assert_eq!(plan.task_id().as_bytes(), task_id.as_bytes());
        assert_eq!(
            plan.change().to().assigned_team_id().unwrap().as_bytes(),
            team_id.as_bytes()
        );
        assert_eq!(
            plan.change().to().assignee_id().unwrap().as_bytes(),
            employee_id.as_bytes()
        );
        assert_eq!(
            serde_json::from_value::<AssignmentCommand>(serde_json::to_value(&desired).unwrap())
                .unwrap(),
            desired
        );
    }
}

#[test]
fn positive_revisions_can_advance_to_the_signed_sqlite_maximum() {
    for revision in [1, 7, i64::MAX - 1] {
        let current = TaskAssignmentSnapshot {
            revision,
            ..snapshot(pair(None, None))
        };
        let desired = AssignmentCommand {
            expected_revision: revision,
            ..command(&pair(None, None))
        };
        assert_eq!(
            plan_assignment(&current, &desired, DesiredTargetFacts::default())
                .unwrap()
                .next_revision(),
            revision + 1
        );
    }
}

#[test]
fn invalid_stale_and_exhausted_revisions_refuse_without_mutating_inputs() {
    let current = snapshot(pair(Some("historical-team"), Some("historical-employee")));
    let desired = command(&pair(None, None));
    for revision in [i64::MIN, -1, 0] {
        assert_refusal(
            &TaskAssignmentSnapshot {
                revision,
                ..current.clone()
            },
            &desired,
            DesiredTargetFacts::default(),
            AssignmentError::InvalidCurrentRevision,
        );
        assert_refusal(
            &current,
            &AssignmentCommand {
                expected_revision: revision,
                ..desired.clone()
            },
            DesiredTargetFacts::default(),
            AssignmentError::InvalidExpectedRevision,
        );
    }
    for expected_revision in [1, 6, 8, i64::MAX] {
        assert_refusal(
            &current,
            &AssignmentCommand {
                expected_revision,
                ..desired.clone()
            },
            DesiredTargetFacts::default(),
            AssignmentError::RevisionConflict,
        );
    }
    assert_refusal(
        &TaskAssignmentSnapshot {
            revision: i64::MAX,
            ..current
        },
        &AssignmentCommand {
            expected_revision: i64::MAX,
            ..desired
        },
        DesiredTargetFacts::default(),
        AssignmentError::RevisionOverflow,
    );
}

#[test]
fn identity_revision_and_target_errors_have_a_documented_first_error_order() {
    let mut current = TaskAssignmentSnapshot {
        organization_id: String::new(),
        task_id: String::new(),
        revision: 0,
        assignment: pair(None, None),
    };
    let mut desired = AssignmentCommand {
        expected_revision: 0,
        assigned_team_id: Some(String::new()),
        assignee_id: Some(String::new()),
    };
    let no_facts = DesiredTargetFacts::default();
    assert_refusal(
        &current,
        &desired,
        no_facts,
        AssignmentError::EmptyOrganizationId,
    );
    current.organization_id = "org".into();
    assert_refusal(&current, &desired, no_facts, AssignmentError::EmptyTaskId);
    current.task_id = "task".into();
    assert_refusal(
        &current,
        &desired,
        no_facts,
        AssignmentError::InvalidCurrentRevision,
    );
    current.revision = 1;
    assert_refusal(
        &current,
        &desired,
        no_facts,
        AssignmentError::InvalidExpectedRevision,
    );
    desired.expected_revision = 2;
    assert_refusal(
        &current,
        &desired,
        no_facts,
        AssignmentError::RevisionConflict,
    );
    current.revision = i64::MAX;
    desired.expected_revision = i64::MAX;
    assert_refusal(
        &current,
        &desired,
        no_facts,
        AssignmentError::RevisionOverflow,
    );
    current.revision = 1;
    desired.expected_revision = 1;
    assert_refusal(
        &current,
        &desired,
        no_facts,
        invalid_target(AssignmentTarget::Team, TargetProblem::EmptyDesiredId),
    );
    desired.assigned_team_id = None;
    assert_refusal(
        &current,
        &desired,
        no_facts,
        invalid_target(AssignmentTarget::Employee, TargetProblem::EmptyDesiredId),
    );
}

#[test]
fn target_errors_prefer_team_then_missing_wrong_id_wrong_org_and_inactive() {
    let current = snapshot(pair(None, None));
    let desired = command(&pair(Some("team"), Some("employee")));
    let mut team = TeamFact {
        id: "wrong".into(),
        organization_id: "wrong".into(),
        active: false,
    };
    let mut employee = EmployeeFact {
        id: "wrong".into(),
        organization_id: "wrong".into(),
        active: false,
    };
    assert_refusal(
        &current,
        &desired,
        DesiredTargetFacts::default(),
        invalid_target(AssignmentTarget::Team, TargetProblem::MissingFact),
    );
    for problem in [
        TargetProblem::WrongId,
        TargetProblem::WrongOrganization,
        TargetProblem::Inactive,
    ] {
        assert_refusal(
            &current,
            &desired,
            DesiredTargetFacts {
                team: Some(&team),
                employee: Some(&employee),
            },
            invalid_target(AssignmentTarget::Team, problem),
        );
        match problem {
            TargetProblem::WrongId => team.id = "team".into(),
            TargetProblem::WrongOrganization => team.organization_id = "org".into(),
            TargetProblem::Inactive => team.active = true,
            _ => unreachable!(),
        }
    }
    assert_refusal(
        &current,
        &desired,
        DesiredTargetFacts {
            team: Some(&team),
            employee: None,
        },
        invalid_target(AssignmentTarget::Employee, TargetProblem::MissingFact),
    );
    for problem in [
        TargetProblem::WrongId,
        TargetProblem::WrongOrganization,
        TargetProblem::Inactive,
    ] {
        assert_refusal(
            &current,
            &desired,
            DesiredTargetFacts {
                team: Some(&team),
                employee: Some(&employee),
            },
            invalid_target(AssignmentTarget::Employee, problem),
        );
        match problem {
            TargetProblem::WrongId => employee.id = "employee".into(),
            TargetProblem::WrongOrganization => employee.organization_id = "org".into(),
            TargetProblem::Inactive => employee.active = true,
            _ => unreachable!(),
        }
    }
    assert!(plan_assignment(
        &current,
        &desired,
        DesiredTargetFacts {
            team: Some(&team),
            employee: Some(&employee)
        }
    )
    .is_ok());
}

#[test]
fn command_json_round_trips_all_explicit_nullable_pairs() {
    for assignment in states("team", "employee") {
        let desired = command(&assignment);
        let encoded = json!({
            "expected_revision": 7,
            "assigned_team_id": assignment.assigned_team_id(),
            "assignee_id": assignment.assignee_id()
        });
        assert_eq!(
            serde_json::from_value::<AssignmentCommand>(encoded.clone()).unwrap(),
            desired
        );
        assert_eq!(serde_json::to_value(&desired).unwrap(), encoded);
    }
    let reordered = r#"{"assignee_id":null,"assigned_team_id":null,"expected_revision":7}"#;
    assert_eq!(
        serde_json::from_str::<AssignmentCommand>(reordered).unwrap(),
        command(&pair(None, None))
    );
}

#[test]
fn command_json_requires_each_key_instead_of_defaulting_absence_to_null() {
    for mask in 0..7 {
        let mut object = serde_json::Map::new();
        for (bit, key, value) in [
            (1, "expected_revision", json!(7)),
            (2, "assigned_team_id", Value::Null),
            (4, "assignee_id", Value::Null),
        ] {
            if mask & bit != 0 {
                object.insert(key.into(), value);
            }
        }
        assert!(
            serde_json::from_value::<AssignmentCommand>(Value::Object(object)).is_err(),
            "missing-key mask {mask}"
        );
    }
}

#[test]
fn command_json_rejects_unknown_and_duplicate_keys_including_after_null() {
    let original = json!({"expected_revision":7,"assigned_team_id":null,"assignee_id":null});
    for unknown in [
        "extra",
        "task_id",
        "organization_id",
        "assigned_team",
        "assignee",
    ] {
        let mut value = original.clone();
        value[unknown] = Value::Null;
        assert!(
            serde_json::from_value::<AssignmentCommand>(value).is_err(),
            "unknown key {unknown}"
        );
    }
    for (key, value) in [
        ("expected_revision", "7"),
        ("assigned_team_id", "null"),
        ("assignee_id", "null"),
    ] {
        let duplicate = format!(
            r#"{{"expected_revision":7,"assigned_team_id":null,"assignee_id":null,"{key}":{value}}}"#
        );
        assert!(
            serde_json::from_str::<AssignmentCommand>(&duplicate).is_err(),
            "duplicate key {key}"
        );
    }
}

#[test]
fn command_json_rejects_nonobjects_and_malformed_assignment_values() {
    for source in ["null", "true", "7", r#""command""#, "[]", "[7,null,null]"] {
        assert!(
            serde_json::from_str::<AssignmentCommand>(source).is_err(),
            "nonobject {source}"
        );
    }
    for key in ["assigned_team_id", "assignee_id"] {
        for malformed in [json!(true), json!(7), json!({"id":"target"}), json!([])] {
            let mut value =
                json!({"expected_revision":7,"assigned_team_id":null,"assignee_id":null});
            value[key] = malformed;
            assert!(
                serde_json::from_value::<AssignmentCommand>(value).is_err(),
                "malformed {key}"
            );
        }
    }
}

#[test]
fn command_json_rejects_noninteger_and_out_of_signed_range_revisions() {
    for revision in [
        "null",
        "true",
        r#""7""#,
        "7.0",
        "1e0",
        "{}",
        "[]",
        "9223372036854775808",
        "-9223372036854775809",
    ] {
        let source = format!(
            r#"{{"expected_revision":{revision},"assigned_team_id":null,"assignee_id":null}}"#
        );
        assert!(
            serde_json::from_str::<AssignmentCommand>(&source).is_err(),
            "revision {revision}"
        );
    }
    for revision in [i64::MIN, -1, 0, 1, i64::MAX] {
        let source =
            json!({"expected_revision":revision,"assigned_team_id":null,"assignee_id":null});
        let decoded: AssignmentCommand = serde_json::from_value(source).unwrap();
        assert_eq!(decoded.expected_revision, revision);
        if revision <= 0 {
            assert_refusal(
                &snapshot(pair(None, None)),
                &decoded,
                DesiredTargetFacts::default(),
                AssignmentError::InvalidExpectedRevision,
            );
        }
    }
}
