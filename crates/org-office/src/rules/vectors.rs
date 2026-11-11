//! Shared browser cases exercised against the canonical office rules, without the Tauri shell.

use org_proto::org::{OrgChange, OrgSnapshot};
use org_proto::{ApiError, Request};
use serde_json::{json, Value};

use super::memory::MemState;
use super::{apply, decide, OrgCommand};

const NOW: &str = "2026-11-08T12:00:00.000Z";

fn seed() -> MemState {
    let snapshot: OrgSnapshot =
        serde_json::from_str(include_str!("../../test-vectors/organization.json")).unwrap();
    MemState {
        teams: snapshot
            .teams
            .into_iter()
            .map(|t| (t.id.clone(), t))
            .collect(),
        members: snapshot
            .members
            .into_iter()
            .map(|m| (m.id.clone(), m))
            .collect(),
        memberships: snapshot
            .memberships
            .into_iter()
            .map(|m| (m.id.clone(), m))
            .collect(),
        project_teams: snapshot
            .project_teams
            .into_iter()
            .map(|p| (p.id.clone(), p))
            .collect(),
        projects: snapshot.projects.into_iter().map(|p| p.id).collect(),
        offices: ["office-1".to_owned()].into(),
        ..MemState::default()
    }
}

fn command(step: &Value, n: usize) -> Option<OrgCommand> {
    let mut params = step["params"].clone();
    params["command_id"] = json!(format!("01ARZ3NDEKTSV4RRFFQ69G5F{n:02}"));
    OrgCommand::parse(&Request {
        id: n as u64 + 1,
        method: step["method"].as_str().unwrap().into(),
        params,
        principal: None,
    })
    .unwrap()
}

fn run(state: &mut MemState, command: &OrgCommand) -> Result<(), ApiError> {
    let changes = decide(state, NOW, command)?;
    for change in changes {
        apply(state, &change).unwrap();
    }
    Ok(())
}

fn state_json(state: &MemState) -> Value {
    json!({
        "teams": state.teams.values().collect::<Vec<_>>(),
        "members": state.members.values().collect::<Vec<_>>(),
        "memberships": state.memberships.values().collect::<Vec<_>>(),
        "project_teams": state.project_teams.values().collect::<Vec<_>>(),
    })
}

#[test]
fn shared_organization_cases_use_the_office_rules() {
    let vectors: Value =
        serde_json::from_str(include_str!("../../test-vectors/org-commands.json")).unwrap();
    let cases = vectors["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 252, "review coverage when shared cases change");
    let manifest: Value = serde_json::from_str(include_str!(
        "../../test-vectors/deferred-org-commands.json"
    ))
    .unwrap();
    let deferred = manifest["cases"].as_array().unwrap();
    assert_eq!(deferred.len(), 104);
    let mut names = std::collections::BTreeSet::new();
    for gap in deferred {
        let name = gap["name"].as_str().unwrap();
        assert!(names.insert(name), "duplicate deferred case {name}");
        assert_eq!(
            cases.iter().filter(|c| c["name"] == name).count(),
            1,
            "unknown deferred case {name}"
        );
    }
    let mut failures = vec![];
    let mut exercised = 0;
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let setup: Vec<_> = case["setup"].as_array().into_iter().flatten().collect();
        if let Some(gap) = deferred.iter().find(|gap| gap["name"] == name) {
            let methods = gap["unavailable_methods"].as_array().unwrap();
            if methods.is_empty() {
                assert_eq!(
                    gap["reason"],
                    "B1-10: area state/reconciliation not implemented"
                );
                // Area changes are intentionally unsupported by the canonical write port.
                // A newly implemented area model must replace these deferrals with assertions.
                let snapshot: OrgSnapshot =
                    serde_json::from_str(include_str!("../../test-vectors/organization.json"))
                        .unwrap();
                let mut state = seed();
                let before = state.clone();
                assert!(
                    apply(
                        &mut state,
                        &OrgChange::AreaUpserted {
                            area: snapshot.areas[0].clone()
                        }
                    )
                    .is_err(),
                    "{name}: area model is now available; enable its cases"
                );
                assert_eq!(state, before);
            } else {
                for method in methods {
                    let step = setup
                        .iter()
                        .copied()
                        .chain([&case["command"]])
                        .find(|step| step["method"] == *method)
                        .unwrap();
                    assert!(
                        command(step, 99).is_none(),
                        "{name}: {method} is now supported; enable its cases"
                    );
                }
            }
            continue;
        }
        exercised += 1;
        let mut state = seed();
        for (n, step) in setup.iter().enumerate() {
            run(&mut state, &command(step, n).unwrap())
                .unwrap_or_else(|e| panic!("setup of {name}: {e}"));
        }
        let before = state.clone();
        let result = run(&mut state, &command(&case["command"], 99).unwrap());
        let got = result.as_ref().err().map_or(json!("ok"), |e| json!(e.code));
        if got != case["expect"] {
            failures.push(format!("{name}: expected {}, got {got}", case["expect"]));
            continue;
        }
        if result.is_err() {
            assert_eq!(state, before, "refused case {name} mutated state");
        } else {
            let value = state_json(&state);
            for check in case["after"].as_array().into_iter().flatten() {
                let list = check["in"].as_str().unwrap();
                let Some(entities) = value[list].as_array() else {
                    failures.push(format!("{name}: after-check list {list} is not modeled"));
                    continue;
                };
                let found = entities.iter().find(|x| x["id"] == check["id"]);
                if check["absent"] == true {
                    assert!(found.is_none(), "{name}: {check}");
                } else if let Some(found) = found {
                    for (key, want) in check["has"].as_object().into_iter().flatten() {
                        if found[key] != *want {
                            failures.push(format!(
                                "{name}: {} {key}: expected {want}, got {}",
                                check["id"], found[key]
                            ));
                        }
                    }
                    for key in check["lacks"].as_array().into_iter().flatten() {
                        assert!(
                            found.get(key.as_str().unwrap()).is_none(),
                            "{name}: {check}"
                        );
                    }
                } else {
                    failures.push(format!("{name}: missing {check}"));
                }
            }
        }
    }
    assert_eq!(exercised, 148);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
