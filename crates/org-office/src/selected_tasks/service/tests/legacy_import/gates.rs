use super::*;
use org_work::{Caller, TxnOutput, WorkCall};
fn work(
    store: Arc<Store>,
    reporter: &str,
) -> org_work::WorkService<crate::store::work::assembly::OfficeWorkStore> {
    let mut org = org_work::testkit::MemOrg::new("owner-member")
        .with_person(reporter)
        .with_project("project", "tst", &["team"]);
    org.position = org_proto::Position { epoch: 1, seq: 9 };
    let native = crate::store::work::assembly::OfficeWorkStore::new(
        store,
        Arc::new(org),
        Arc::new(crate::time::now_ms),
    )
    .unwrap();
    let clock = native.clock();
    org_work::WorkService::new(native, clock, "office")
}
fn assign_legacy(
    service: &org_work::WorkService<crate::store::work::assembly::OfficeWorkStore>,
    id: &str,
) -> Result<TxnOutput, org_work::StoreError> {
    service.call(Caller::app("owner-member"),WorkCall::TaskAssign(serde_json::from_value(json!({"change_id":crate::ulid::generate().unwrap(),"item_id":id,"expected_version":1})).unwrap()))
}
#[test]
fn adopted_legacy_mutators_and_raw_writes_are_fenced_without_origin_changes() {
    let legacy = Legacy::new("task");
    let (dir, store, _) = setup(&legacy);
    let store = Arc::new(store);
    let source = legacy.source();
    let action = stage(
        &store,
        &dir.path().join("staging"),
        &legacy,
        source.clone(),
        legacy.template(&source, legacy.mapping()),
    );
    assert!(matches!(
        run(&store, dir.path().join("staging"), authority(), action).unwrap(),
        Outcome::Completed { .. }
    ));
    let before = retained(&store, &legacy.item.id);
    let service = work(store.clone(), &legacy.item.created_by);
    assert!(assign_legacy(&service, &legacy.item.id).is_err());
    for action in [
        "update",
        "status",
        "watch",
        "comment",
        "checklist",
        "delete",
        "undo",
    ] {
        let mut p = json!({"change_id":crate::ulid::generate().unwrap(),"item_id":legacy.item.id,"expected_version":1});
        let call = match action {
            "update" => {
                p["title"] = json!("Changed");
                WorkCall::TaskUpdate(serde_json::from_value(p).unwrap())
            }
            "status" => {
                p["status"] = json!("in_progress");
                WorkCall::TaskSetStatus(serde_json::from_value(p).unwrap())
            }
            "watch" => {
                p["watching"] = json!(true);
                WorkCall::Watch(serde_json::from_value(p).unwrap())
            }
            "comment" => {
                p["comment_id"] = json!(crate::ulid::generate().unwrap());
                p["body"] = json!("Comment");
                WorkCall::Comment(serde_json::from_value(p).unwrap())
            }
            "checklist" => {
                p["steps"] = json!([{"step_id":crate::ulid::generate().unwrap(),"text":"Step","required":false}]);
                WorkCall::ChecklistAdd(serde_json::from_value(p).unwrap())
            }
            "delete" => WorkCall::Delete(serde_json::from_value(p).unwrap()),
            "undo" => {
                p["target_id"] = json!(legacy.item.id);
                p["event_version"] = json!(1);
                WorkCall::Undo(serde_json::from_value(p).unwrap())
            }
            _ => unreachable!(),
        };
        let result = service.call(Caller::app("owner-member"), call);
        if action == "undo" {
            assert!(matches!(result.unwrap(), TxnOutput::Refused(_)));
        }
        // Created is not automatic.
        else {
            assert!(result.is_err(), "{action}: {result:?}");
        }
        assert_eq!(retained(&store, &legacy.item.id), before, "{action}");
    }
    for sql in [
        "UPDATE work_items SET data=data WHERE id=?1",
        "DELETE FROM work_items WHERE id=?1",
        "INSERT OR REPLACE INTO work_items SELECT * FROM work_items WHERE id=?1",
        "UPDATE work_ids SET kind=kind WHERE item_id=?1",
        "DELETE FROM work_ids WHERE item_id=?1",
        "INSERT OR REPLACE INTO work_ids SELECT * FROM work_ids WHERE item_id=?1",
        "UPDATE work_events SET data=data WHERE target_id=?1",
        "DELETE FROM work_events WHERE target_id=?1",
        "INSERT INTO work_events SELECT target_id,version+1,feed_seq,json_set(data,'$.version',version+1) FROM work_events WHERE target_id=?1",
    ] {
        let task=legacy.item.id.clone();
        assert!(store.write(move|db| {db.execute(sql,[&task]).map_err(storage)?;Ok(((),None))}).is_err(),"{sql}");
        assert_eq!(retained(&store,&legacy.item.id),before);
    }
}
#[test]
fn source_version_directory_position_and_number_races_stay_staged_with_unadopted_control() {
    for race in ["source", "directory", "position", "number"] {
        let legacy = Legacy::new("task");
        let (dir, store, _) = setup(&legacy);
        let store = Arc::new(store);
        let source = legacy.source();
        let root = dir.path().join("staging");
        let action = stage(
            &store,
            &root,
            &legacy,
            source.clone(),
            legacy.template(&source, legacy.mapping()),
        );
        match race {
            "source" => {
                assert!(matches!(
                    assign_legacy(
                        &work(store.clone(), &legacy.item.created_by),
                        &legacy.item.id
                    )
                    .unwrap(),
                    TxnOutput::Committed { .. }
                ));
            }
            "directory" => {
                super::super::super::super::directory::admit(
                    &store,
                    authority(),
                    Some(1),
                    directory_source(true),
                )
                .unwrap();
            }
            "position" => {}
            "number" => {
                store.selected_write(|db| {put_artifact(db,"public","identity/allocations/project/project/task/43.yaml",b"scope_kind: project\nscope_id: project\nentity_kind: task\nnumber: 43\nentity_id: another-task\nallocated_at: 2026-01-01T00:00:00Z\n",true)}).unwrap();
            }
            _ => unreachable!(),
        }
        let result = if race == "position" {
            let mut org =
                org_work::testkit::MemOrg::new("owner-member").with_person(&legacy.item.created_by);
            org.position = org_proto::Position { epoch: 1, seq: 10 };
            adoption::run_with_org(&store, root, authority(), action, Some(Arc::new(org)))
        } else {
            run(&store, root, authority(), action)
        }
        .unwrap();
        assert!(matches!(result, Outcome::Staged { .. }), "{race}");
    }
}

#[test]
fn actual_bug_cascade_is_fenced_after_adoption_and_unadopted_control_still_commits() {
    for adopted in [false, true] {
        let legacy = Legacy::new("bug");
        let (dir, store, _) = setup(&legacy);
        let store = Arc::new(store);
        if adopted {
            let source = legacy.source();
            let action = stage(
                &store,
                &dir.path().join("staging"),
                &legacy,
                source.clone(),
                legacy.template(&source, legacy.mapping()),
            );
            assert!(matches!(
                run(&store, dir.path().join("staging"), authority(), action).unwrap(),
                Outcome::Completed { .. }
            ));
        }
        let service = work(store.clone(), &legacy.item.created_by);
        // This source now has no team lead, so the triager becomes owner and
        // real reconcile_bugs tries to start a waiting span on the legacy bug.
        let before = retained(&store, &legacy.item.id);
        let result = service.reconcile_bugs();
        if adopted {
            assert!(result.is_err());
            assert_eq!(retained(&store, &legacy.item.id), before);
        } else {
            assert!(
                matches!(result.unwrap(),TxnOutput::Committed{ref changed,..} if !changed.is_empty())
            );
        }
    }
}

#[test]
fn legacy_generation_and_origin_incarnation_are_preserved_for_following_selected_events() {
    let mut legacy = Legacy::new("task");
    legacy.item.generation = 2;
    legacy.snapshot = legacy
        .snapshot
        .replace("\"generation\":1", "\"generation\":2");
    let (dir, store, _) = setup(&legacy);
    let source = legacy.source();
    let action = stage(
        &store,
        &dir.path().join("staging"),
        &legacy,
        source.clone(),
        legacy.template(&source, legacy.mapping()),
    );
    assert!(matches!(
        run(&store, dir.path().join("staging"), authority(), action).unwrap(),
        Outcome::Completed { .. }
    ));
    let mut change = request(1, None, None);
    change.task_id = legacy.item.id.clone();
    assert!(matches!(
        assign(&store, authority(), change).unwrap(),
        AssignmentOutcome::Completed { revision: 2, .. }
    ));
    let task = legacy.item.id;
    store.selected_read(move|db,_| {
        let entries:Vec<(i64,i64,String)>=db.prepare("SELECT generation,version,origin_incarnation FROM work_origin_log WHERE target_id=?1 ORDER BY origin_seq").map_err(storage)?.query_map([&task],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(storage)?.collect::<rusqlite::Result<_>>().map_err(storage)?;
        assert_eq!(entries.len(),2);assert_eq!((entries[0].0,entries[0].1),(2,1));assert_eq!((entries[1].0,entries[1].1),(2,2));assert_eq!(entries[0].2,entries[1].2);Ok(())
    }).unwrap();
}

#[test]
fn published_legacy_adoption_reopens_with_frozen_mapping_without_rerunning_source_decisions() {
    let legacy = Legacy::new("request");
    let (dir, store, config) = setup(&legacy);
    let source = legacy.source();
    let root = dir.path().join("staging");
    let action = stage(
        &store,
        &root,
        &legacy,
        source.clone(),
        legacy.template(&source, legacy.mapping()),
    );
    let target = config.public_root.join(format!(
        "teams/accountable/work/tasks/{}.md",
        legacy.item.id
    ));
    for path in [
        "teams",
        "teams/accountable",
        "teams/accountable/work",
        "teams/accountable/work/tasks",
    ] {
        let path = config.public_root.join(path);
        std::fs::create_dir_all(&path).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    std::fs::write(&target, b"outside writer").unwrap();
    let before = store
        .selected_read(|db, _| {
            db.query_row("SELECT ordinal FROM selected_journal_cursor", [], |r| {
                r.get::<_, i64>(0)
            })
            .map_err(storage)
        })
        .unwrap();
    assert!(run(&store, root.clone(), authority(), action.clone()).is_err());
    assert!(config
        .protected_root
        .join(format!(
            "offices/office/journals/work-acceptance/journal/transactions/{}.json",
            before + 1
        ))
        .is_file());
    drop(store);
    std::fs::remove_file(&target).unwrap();
    let store = Store::open(
        &dir.path().join("office.db"),
        &Key::from_bytes([31; 32]),
        Opening::Existing,
        &OFFICE_SCHEMA,
    )
    .unwrap();
    store.install_acceptance(config).unwrap();
    struct NoDecision;
    impl crate::store::work::assembly::OrgSource for NoDecision {
        fn snapshot(&self) -> Result<Box<dyn org_work::OrgView + '_>, org_work::StoreError> {
            panic!("durable retry must not rerun migration mappings")
        }
    }
    assert!(matches!(
        adoption::run_with_org(
            &store,
            root,
            authority(),
            action,
            Some(Arc::new(NoDecision))
        )
        .unwrap(),
        Outcome::Completed { revision: 1, .. }
    ));
    assert_eq!(std::fs::read(target).unwrap(), source);
    assert_eq!(retained(&store, &legacy.item.id).0, legacy.snapshot);
}
