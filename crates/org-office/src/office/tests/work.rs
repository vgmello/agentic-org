use super::*;
use org_proto::org::{AgentProfile, AgentRuntime, AgentStatus, MemberAddAgentParams};
use org_proto::{ErrorCode, Request};
use org_work::{Caller, TxnOutput, WorkCall};
use serde_json::{json, Value};
use std::cell::Cell;
use std::rc::Rc;

const AGENT: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAX";
const PROJECT: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAY";
const ITEM: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAZ";
const INSTANCE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FA1";

fn request(method: &str, params: Value) -> Request {
    Request {
        id: 1,
        method: method.into(),
        params,
        principal: None,
    }
}

fn call(office: &Office, caller: Caller, method: &str, params: Value) -> TxnOutput {
    office
        .work_call(
            caller,
            WorkCall::parse(&request(method, params)).unwrap().unwrap(),
        )
        .unwrap()
}

fn seed(office: &Office) -> String {
    let owner = office
        .org_snapshot()
        .unwrap()
        .members
        .into_iter()
        .find(|m| m.owner)
        .unwrap()
        .id;
    let hire = MemberAddAgentParams {
        command_id: crate::ulid::generate().unwrap(),
        member_id: AGENT.into(),
        name: "Work agent".into(),
        office_id: ID.into(),
        profile: AgentProfile {
            role: "engineer".into(),
            title: "Engineer".into(),
            focus: None,
            runtime: AgentRuntime::ClaudeCode,
            model: None,
            run_mode: None,
            schedule: None,
            home: None,
            messaging_name: "work-agent".into(),
            status: AgentStatus::Active,
            offboarded_at: None,
        },
    };
    let command = OrgCommand::parse(&request(
        "member.add_agent",
        serde_json::to_value(hire).unwrap(),
    ))
    .unwrap()
    .unwrap();
    office.submit_org(command).unwrap().unwrap();
    office.org_store().run_sql_for_test(r#"INSERT INTO projects VALUES ('01ARZ3NDEKTSV4RRFFQ69G5FAY','{"id":"01ARZ3NDEKTSV4RRFFQ69G5FAY","key":"c1","name":"C1","version":1}');"#);
    owner
}

fn create_params() -> Value {
    json!({"change_id":crate::ulid::generate().unwrap(),"item_id":ITEM,"project_id":PROJECT,"title":"First title"})
}

#[test]
fn c1_work_commit_replay_reopen_conflict_and_owner_delete_use_real_stores() {
    for instance_bound in [false, true] {
        let dir = TempDir::new().unwrap();
        let folder = folder_of(&dir, ID);
        let key = test_key();
        let office = if instance_bound {
            Office::create_for_instance(&folder, &new_office(ID), key.clone(), INSTANCE)
        } else {
            Office::create(&folder, &new_office(ID), key.clone())
        }
        .unwrap();
        let owner = seed(&office);
        let params = create_params();
        let created = call(&office, Caller::agent(AGENT), "task.create", params.clone());
        assert!(
            matches!(created, TxnOutput::Committed { .. }),
            "{created:?}"
        );
        let expected = created.answer().unwrap().clone();
        assert_eq!(expected.items.len(), 1);
        assert_eq!(expected.items[0].id, ITEM);
        assert_eq!(expected.items[0].version, 1);
        let replay = call(&office, Caller::agent(AGENT), "task.create", params.clone());
        assert!(matches!(replay, TxnOutput::Replayed(_)));
        assert_eq!(replay.answer().unwrap(), &expected);
        drop(office);
        let office = if instance_bound {
            Office::open_for_instance(&folder, key.clone(), INSTANCE)
        } else {
            Office::open(&folder, key.clone())
        }
        .unwrap();
        let replay = call(&office, Caller::agent(AGENT), "task.create", params.clone());
        assert!(matches!(replay, TxnOutput::Replayed(_)));
        assert_eq!(replay.answer().unwrap(), &expected);
        let mut changed = params.clone();
        changed["title"] = json!("Other params");
        assert_eq!(
            call(&office, Caller::agent(AGENT), "task.create", changed)
                .answer()
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        let update = |title: &str| json!({"change_id":crate::ulid::generate().unwrap(),"item_id":ITEM,"expected_version":1,"title":title});
        let updated = call(
            &office,
            Caller::agent(AGENT),
            "task.update",
            update("Second title"),
        );
        assert!(matches!(updated, TxnOutput::Committed { .. }));
        assert_eq!(updated.answer().unwrap().items[0].version, 2);
        assert_eq!(
            call(
                &office,
                Caller::agent(AGENT),
                "task.update",
                update("Stale title")
            )
            .answer()
            .unwrap_err()
            .code,
            ErrorCode::Conflict
        );
        let deleted = call(
            &office,
            Caller::app(owner),
            "work.delete",
            json!({"change_id":crate::ulid::generate().unwrap(),"item_id":ITEM,"expected_version":2}),
        );
        assert!(
            matches!(deleted, TxnOutput::Committed { .. }),
            "{deleted:?}"
        );
    }
}

#[test]
fn c1_work_stores_close_before_the_office_lock_is_released() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<Office>();
    let (_dir, _key, office) = made();
    seed(&office);
    let written = call(
        &office,
        Caller::agent(AGENT),
        "task.create",
        create_params(),
    );
    assert!(matches!(written, TxnOutput::Committed { .. }));
    let org = Arc::downgrade(&office.stores.org);
    let local = Arc::downgrade(&office.stores.local);
    let observed = Rc::new(Cell::new(None));
    let record = Rc::clone(&observed);
    BEFORE_UNLOCK.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move || {
            record.set(Some((org.upgrade().is_some(), local.upgrade().is_some())));
        }))
    });
    drop(office);
    assert_eq!(
        observed.get(),
        Some((false, false)),
        "all store owners and writer jobs must close before flock release"
    );
}

#[test]
fn c1_work_shared_constructor_builds_once_and_instance_mismatch_refuses_first() {
    let dir = TempDir::new().unwrap();
    let folder = folder_of(&dir, ID);
    let key = test_key();
    let before = STORES_BUILT.with(Cell::get);
    let office =
        Office::create_for_instance(&folder, &new_office(ID), key.clone(), INSTANCE).unwrap();
    assert_eq!(STORES_BUILT.with(Cell::get), before + 1);
    drop(office);
    let office = Office::open_for_instance(&folder, key.clone(), INSTANCE).unwrap();
    assert_eq!(STORES_BUILT.with(Cell::get), before + 2);
    drop(office);
    assert!(Office::open_for_instance(&folder, key.clone(), "01ARZ3NDEKTSV4RRFFQ69G5FA2").is_err());
    assert_eq!(STORES_BUILT.with(Cell::get), before + 2);
    let office = Office::open(&folder, key).unwrap();
    assert_eq!(STORES_BUILT.with(Cell::get), before + 3);
    drop(office);
}

#[test]
fn c1_work_construction_is_between_workspace_and_genesis() {
    let dir = TempDir::new().unwrap();
    let key = test_key();
    let before = STORES_BUILT.with(Cell::get);
    let seen_key = key.clone();
    let seen = Rc::new(Cell::new(0));
    let record = Rc::clone(&seen);
    ON_STEP.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move |folder, step| {
            if !matches!(step, "workspace" | "work" | "organization") {
                return;
            }
            assert!(folder.join(WORKSPACE).is_dir());
            assert_eq!(
                STORES_BUILT.with(Cell::get),
                before + usize::from(step != "workspace")
            );
            let committed = Store::inspect(&folder.join(ORG_DB), &seen_key, |db| {
                db.query_row(
                    "SELECT EXISTS (SELECT 1 FROM meta WHERE key = 'organization_id')",
                    (),
                    |row| row.get::<_, bool>(0),
                )
                .map_err(OfficeError::storage)
            })
            .unwrap();
            assert_eq!(committed, step == "organization", "{step}");
            record.set(record.get() + 1);
        }))
    });
    let result = Office::create(folder_of(&dir, ID), &new_office(ID), key);
    ON_STEP.with(|hook| *hook.borrow_mut() = None);
    result.unwrap();
    assert_eq!(seen.get(), 3);
}
