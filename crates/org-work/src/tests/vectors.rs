//! Shared browser cases run through the real work service and its transaction/read ports.

use std::sync::Arc;

use org_proto::{work::WorkItem, Request};
use serde_json::{json, Value};

use crate::testkit::{MemOrg, MemStore, TestClock};
use crate::{parse_ms, read_item, Caller, WorkCall, WorkService};

fn subset(have: &Value, want: &Value, path: &str) -> Option<String> {
    match want {
        Value::Object(fields) => fields.iter().find_map(|(key, value)| {
            subset(
                have.get(key).unwrap_or(&Value::Null),
                value,
                &format!("{path}.{key}"),
            )
        }),
        Value::Array(values) => {
            let Some(actual) = have
                .as_array()
                .filter(|actual| actual.len() == values.len())
            else {
                return Some(format!(
                    "{path}: expected an array of {} entries, got {have}",
                    values.len()
                ));
            };
            values
                .iter()
                .zip(actual)
                .enumerate()
                .find_map(|(i, (want, actual))| subset(actual, want, &format!("{path}[{i}]")))
        }
        _ => (have != want).then(|| format!("{path}: expected {want}, got {have}")),
    }
}

fn organization(context: &Value) -> MemOrg {
    let owner = context["actor"].as_str().unwrap();
    let mut org = MemOrg::new(owner);
    for member in context["members"].as_array().unwrap() {
        if member != owner {
            org = org.with_person(member.as_str().unwrap());
        }
    }
    for project in context["projects"].as_array().unwrap() {
        let id = project["id"].as_str().unwrap();
        let teams: Vec<_> = project["teamIds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        org = org.with_project(id, id, &teams);
        org.areas.retain(|_, area| area.project_id != id);
        for area in project["areas"].as_array().into_iter().flatten() {
            let area_id = area["id"].as_str().unwrap();
            org = org.with_area(area_id, id, teams.first().copied(), None);
            org.areas.get_mut(area_id).unwrap().general = area["general"] == true;
        }
        org.set_archived(id, project["archived"] == true);
    }
    org
}

fn canonical_step(legacy: &Value) -> Value {
    let mut step = legacy.clone();
    if let Some(canonical) = legacy.get("canonical") {
        assert!(!canonical["reason"].as_str().unwrap().trim().is_empty());
        for (key, value) in canonical.as_object().unwrap() {
            match key.as_str() {
                "reason" => {}
                "call" => {
                    for (field, fixed) in value.as_object().unwrap() {
                        assert!(
                            matches!(field.as_str(), "change_id" | "link_id" | "fixes" | "expected_version"),
                            "a canonical override must not replace the operation or its business input: {field}"
                        );
                        let original = legacy["call"]
                            .get(field)
                            .unwrap_or_else(|| panic!("override adds absent call field {field}"));
                        assert_ne!(original, fixed, "no-op call override of {field}");
                        if field == "fixes" {
                            let original = step["call"][field].as_array().unwrap();
                            let mapped = fixed.as_array().unwrap();
                            assert_eq!(original.len(), mapped.len());
                            for (original, mapped) in original.iter().zip(mapped) {
                                let (mut original, mut mapped) = (original.clone(), mapped.clone());
                                original.as_object_mut().unwrap().remove("link_id");
                                mapped.as_object_mut().unwrap().remove("link_id");
                                assert_eq!(original, mapped, "only the fix link ID may differ");
                            }
                        }
                        step["call"][field] = fixed.clone();
                    }
                }
                "items" | "expect" | "error" => {
                    let original = legacy
                        .get(key)
                        .unwrap_or_else(|| panic!("override adds absent expectation {key}"));
                    assert_ne!(original, value, "no-op expectation override of {key}");
                    match key.as_str() {
                        "error" => assert!(
                            original.is_string() && value.is_string(),
                            "an error override may change the code, never refusal into acceptance"
                        ),
                        "items" => assert!(original.is_array() && value.is_array()),
                        "expect" => assert!(original.is_object() && value.is_object()),
                        _ => unreachable!(),
                    }
                    step[key] = value.clone();
                }
                "reads" => {
                    // Additional read-model assertions supplement the existing outcome;
                    // they cannot replace it or remove its expected items/refusal.
                    let reads = value.as_array().unwrap();
                    assert!(!reads.is_empty());
                    assert_ne!(legacy.get(key), Some(value), "no-op reads override");
                    for read in reads {
                        assert!(read["id"].is_string() && read["expect"].is_object());
                        assert!(read
                            .as_object()
                            .unwrap()
                            .keys()
                            .all(|key| matches!(key.as_str(), "id" | "expect")));
                    }
                    step[key] = value.clone();
                }
                other => panic!("unknown canonical override {other}"),
            }
        }
    }
    step
}

#[test]
fn shared_work_cases_use_the_canonical_service() {
    let vectors: Value =
        serde_json::from_str(include_str!("../../test-vectors/work-commands.json")).unwrap();
    let cases = vectors["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 29, "review coverage when shared cases change");
    let context = &vectors["context"];
    let base_time = parse_ms(context["now"].as_str().unwrap()).unwrap();
    let mut failures = vec![];
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let org = organization(context);
        let store = Arc::new(MemStore::new(org.clone()));
        let clock = TestClock::at(base_time);
        let service = WorkService::new(Arc::clone(&store), clock.clone(), "office-1");
        let caller = Caller::app(context["actor"].as_str().unwrap());
        for (n, legacy) in case["steps"].as_array().unwrap().iter().enumerate() {
            let step = canonical_step(legacy);
            let now = parse_ms(
                step["now"]
                    .as_str()
                    .unwrap_or(context["now"].as_str().unwrap()),
            )
            .unwrap();
            clock.set(now);
            if let Some(id) = step["get"].as_str() {
                let snapshot = store.snapshot();
                let item = snapshot.items.get(id).unwrap();
                let full: WorkItem = read_item(&org, store.as_ref(), item, now).unwrap();
                let value = serde_json::to_value(full).unwrap();
                if let Some(error) = subset(&value, &step["expect"], "") {
                    failures.push(format!("{name}, step {n}: {error}"));
                }
                continue;
            }
            let mut params = step["call"].clone();
            let method = params["method"].as_str().unwrap().to_owned();
            params.as_object_mut().unwrap().remove("method");
            let call = WorkCall::parse(&Request {
                id: n as u64 + 1,
                method,
                params,
                principal: None,
            })
            .unwrap()
            .unwrap();
            let before = store.snapshot().items;
            let output = service.call(caller.clone(), call).unwrap();
            match output.answer() {
                Err(error) => {
                    assert_eq!(
                        store.snapshot().items,
                        before,
                        "{name}, step {n}: refusal mutated items"
                    );
                    if json!(error.code) != step["error"] {
                        failures.push(format!(
                            "{name}, step {n}: expected {}, got {:?}",
                            step["error"], error.code
                        ));
                        break;
                    }
                }
                Ok(accepted) => {
                    if step["error"].is_string() {
                        failures.push(format!(
                            "{name}, step {n}: expected {} but accepted",
                            step["error"]
                        ));
                        break;
                    }
                    let value = serde_json::to_value(&accepted.items).unwrap();
                    if let Some(error) = subset(&value, &step["items"], "") {
                        failures.push(format!("{name}, step {n}: {error}"));
                    }
                }
            }
            for read in step["reads"].as_array().into_iter().flatten() {
                let snapshot = store.snapshot();
                let item = &snapshot.items[read["id"].as_str().unwrap()];
                let full =
                    serde_json::to_value(read_item(&org, store.as_ref(), item, now).unwrap())
                        .unwrap();
                if let Some(error) = subset(&full, &read["expect"], "") {
                    failures.push(format!("{name}, step {n}, read: {error}"));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
