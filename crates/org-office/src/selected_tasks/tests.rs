use super::*;
use org_work::selected_assignment::{
    plan_assignment, AssignmentCommand, DesiredTargetFacts, EmployeeFact, TeamFact,
};

const SOURCE: &str = "---\nschema_version: 1\nid: opaque task 🦀\nnumber: 42\ntype: request\nteam_id: accountable\nproject_id: null\ntitle: Preserve the original public bytes\nstatus: todo\nrevision: 1 # latest revision\nassigned_team_id: null # assigned team\nassignee_id: null # employee\ncreated_at: null\ncreated_by_kind: unknown\ncreated_by_id: null\nupdated_at: null\nupdated_by_kind: unknown\nupdated_by_id: null\nextension:\n  exact: -0.000000000000123456789E+012\n  revision: 91\n---\nOriginal body: café 🦀\nrevision: 777\n";

#[test]
fn all_sixteen_pairs_are_materialized_with_only_the_six_scalar_edits() {
    let team = TeamFact {
        organization_id: "org".into(),
        id: "doing-team".into(),
        active: true,
    };
    let employee = EmployeeFact {
        organization_id: "org".into(),
        id: "employee".into(),
        active: true,
    };
    let pairs = [
        (None, None),
        (Some("doing-team"), None),
        (None, Some("employee")),
        (Some("doing-team"), Some("employee")),
    ];
    for (from_team, from_employee) in pairs {
        let source = SOURCE
            .replace(
                "assigned_team_id: null",
                &format!("assigned_team_id: {}", from_team.unwrap_or("null")),
            )
            .replace(
                "assignee_id: null",
                &format!("assignee_id: {}", from_employee.unwrap_or("null")),
            );
        let source = format!("\u{feff}{}", source.replace('\n', "\r\n"));
        for (to_team, to_employee) in pairs {
            let snapshot = TaskSnapshot::parse("org", &source).unwrap();
            let command = AssignmentCommand {
                expected_revision: 1,
                assigned_team_id: to_team.map(str::to_owned),
                assignee_id: to_employee.map(str::to_owned),
            };
            let plan = plan_assignment(
                &snapshot.assignment_state(),
                &command,
                DesiredTargetFacts {
                    team: to_team.map(|_| &team),
                    employee: to_employee.map(|_| &employee),
                },
            )
            .unwrap();
            let output = snapshot
                .rewrite_assignment(&plan, "2026-11-15T14:00:00.000Z", Updater::Owner)
                .unwrap();
            let expected = source
                .replace("revision: 1 #", "revision: 2 #")
                .replace(
                    &format!("assigned_team_id: {} #", from_team.unwrap_or("null")),
                    &format!(
                        "assigned_team_id: {} #",
                        to_team.map(|s| format!("\"{s}\"")).unwrap_or("null".into())
                    ),
                )
                .replace(
                    &format!("assignee_id: {} #", from_employee.unwrap_or("null")),
                    &format!(
                        "assignee_id: {} #",
                        to_employee
                            .map(|s| format!("\"{s}\""))
                            .unwrap_or("null".into())
                    ),
                )
                .replace(
                    "updated_at: null",
                    "updated_at: \"2026-11-15T14:00:00.000Z\"",
                )
                .replace("updated_by_kind: unknown", "updated_by_kind: \"owner\"");
            assert_eq!(output.as_bytes(), expected.as_bytes());
            let rewritten = TaskSnapshot::parse("org", &output).unwrap();
            assert_eq!(rewritten.immutable(), snapshot.immutable());
            assert_eq!(rewritten.assignment(), plan.change().to());
        }
    }
}

#[test]
fn plan_is_bound_to_original_identity_revision_and_assignment() {
    let snapshot = TaskSnapshot::parse("org", SOURCE).unwrap();
    let command = AssignmentCommand {
        expected_revision: 1,
        assigned_team_id: None,
        assignee_id: None,
    };
    let plan = plan_assignment(
        &snapshot.assignment_state(),
        &command,
        DesiredTargetFacts::default(),
    )
    .unwrap();
    let other_org = TaskSnapshot::parse("other", SOURCE).unwrap();
    assert_eq!(
        other_org.rewrite_assignment(&plan, "2026-11-15T14:00:00.000Z", Updater::Owner),
        Err(SnapshotError::PlanMismatch)
    );
    let source = SOURCE.replace("revision: 1", "revision: 2");
    let later = TaskSnapshot::parse("org", &source).unwrap();
    assert_eq!(
        later.rewrite_assignment(&plan, "2026-11-15T14:00:00.000Z", Updater::Owner),
        Err(SnapshotError::PlanMismatch)
    );
    assert_eq!(
        snapshot.rewrite_assignment(&plan, "untrusted time", Updater::Owner),
        Err(SnapshotError::PlanMismatch)
    );
    let output = snapshot
        .rewrite_assignment(
            &plan,
            "2026-11-15T14:00:00.000Z",
            Updater::Employee("opaque employee"),
        )
        .unwrap();
    assert!(output.contains("created_by_kind: unknown\ncreated_by_id: null"));
    assert!(output.contains("updated_by_kind: \"employee\"\nupdated_by_id: \"opaque employee\""));
}

#[test]
fn missing_selected_facts_and_unsupported_edits_remain_unadmitted() {
    for source in [
        SOURCE.replace("team_id: accountable\n", ""),
        SOURCE.replace("schema_version: 1", "schema_version: 2"),
        SOURCE
            .replace("created_by_kind: unknown", "created_by_kind: owner")
            .replace("created_by_id: null", "created_by_id: fake-employee"),
        SOURCE.replace("assigned_team_id: null", "assigned_team_id: []"),
    ] {
        assert!(TaskSnapshot::parse("org", &source).is_err());
    }
    let source = SOURCE.replace("assignee_id: null # employee", "assignee_id: # empty");
    let snapshot = TaskSnapshot::parse("org", &source).unwrap();
    let command = AssignmentCommand {
        expected_revision: 1,
        assigned_team_id: None,
        assignee_id: None,
    };
    let plan = plan_assignment(
        &snapshot.assignment_state(),
        &command,
        DesiredTargetFacts::default(),
    )
    .unwrap();
    assert!(matches!(
        snapshot.rewrite_assignment(&plan, "2026-11-15T14:00:00.000Z", Updater::Owner),
        Err(SnapshotError::Source(
            SourceError::UnsupportedRepresentation
        ))
    ));
}
