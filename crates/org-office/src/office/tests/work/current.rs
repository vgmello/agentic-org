use super::*;
use org_proto::directory::{StatusSetParams, WhoParams};
use org_proto::work::{ItemKind, ItemStatus, Resolution};
use org_work::StoredItem;

fn task(office: &Office) {
    seed(office);
    let mut params = create_params();
    params["assignee_id"] = json!(AGENT);
    assert!(matches!(
        call(office, Caller::agent(AGENT), "task.create", params),
        TxnOutput::Committed { .. }
    ));
}

fn mutate(office: &Office, change: impl FnOnce(&mut StoredItem)) {
    let mut item = office
        .local_store()
        .read(|db| {
            let tx = db.unchecked_transaction().map_err(OfficeError::storage)?;
            let item = crate::store::work::get_in(&tx, ITEM)?.unwrap();
            tx.rollback().map_err(OfficeError::storage)?;
            Ok(item)
        })
        .unwrap();
    change(&mut item);
    let data = serde_json::to_string(&item).unwrap();
    office
        .local_store()
        .write(move |db| {
            db.execute("UPDATE work_items SET data=?1 WHERE id=?2", (&data, ITEM))
                .map_err(OfficeError::storage)?;
            Ok(((), None))
        })
        .unwrap();
}

type StatusRow = (
    String,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
);

fn rows(office: &Office) -> Vec<StatusRow> {
    office.local_store().read(|db| {
        db.prepare("SELECT member_id,status_text,current_task_id,updated_at,expires_at FROM member_status ORDER BY member_id").map_err(OfficeError::storage)?
            .query_map([],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).map_err(OfficeError::storage)?
            .collect::<rusqlite::Result<_>>().map_err(OfficeError::storage)
    }).unwrap()
}

fn current(office: &Office) -> Option<String> {
    office
        .who_with_status(&WhoParams::default())
        .unwrap()
        .unwrap()
        .members
        .into_iter()
        .find(|member| member.id == AGENT)
        .unwrap()
        .current_task_id
}

#[test]
fn current_task_set_restart_done_and_clear_keep_the_status_halves_independent() {
    let (dir, key, office) = made();
    task(&office);
    let event = office
        .set_current(AGENT, true, Some(ITEM))
        .unwrap()
        .unwrap();
    assert_eq!(event.current_task_id.as_deref(), Some(ITEM));
    assert_eq!(event.status, None);
    assert_eq!(current(&office).as_deref(), Some(ITEM));
    let set = office
        .set_status(
            AGENT,
            &StatusSetParams {
                text: Some("Working".into()),
                ttl_ms: None,
            },
        )
        .unwrap()
        .unwrap();
    let event = crate::presence_after_status(AGENT, &set.0, set.1);
    assert_eq!(event.status.as_deref(), Some("Working"));
    assert_eq!(event.current_task_id.as_deref(), Some(ITEM));
    let folder = folder_of(&dir, ID);
    drop(office);
    let office = Office::open(&folder, key).unwrap();
    assert_eq!(current(&office).as_deref(), Some(ITEM));
    for resolution in [Resolution::Completed, Resolution::Cancelled] {
        mutate(&office, |item| {
            item.status = ItemStatus::Done;
            item.resolution = Some(resolution);
        });
        assert_eq!(
            office
                .set_current(AGENT, true, Some(ITEM))
                .unwrap()
                .unwrap()
                .current_task_id
                .as_deref(),
            Some(ITEM)
        );
        assert_eq!(current(&office).as_deref(), Some(ITEM));
    }
    let event = office.set_current(AGENT, true, None).unwrap().unwrap();
    assert_eq!(event.current_task_id, None);
    assert_eq!(event.status.as_deref(), Some("Working"));
    assert_eq!(rows(&office).len(), 1);
    office
        .set_current(AGENT, true, Some(ITEM))
        .unwrap()
        .unwrap();
    let set = office
        .set_status(AGENT, &StatusSetParams::default())
        .unwrap()
        .unwrap();
    assert_eq!(set.0.status, None);
    assert_eq!(set.1.as_deref(), Some(ITEM));
    assert_eq!(rows(&office).len(), 1);
    office.set_current(AGENT, true, None).unwrap().unwrap();
    assert!(rows(&office).is_empty());
    assert_eq!(current(&office), None);
}

#[test]
fn current_task_refusals_follow_missing_agent_kind_deleted_assignee_order_and_never_sweep() {
    let (_dir, _key, office) = made();
    task(&office);
    office.local_store().run_sql_for_test("INSERT INTO member_status VALUES ('expired','keep',NULL,'2000-01-01T00:00:00.000Z','2000-01-02T00:00:00.000Z');");
    let before = rows(&office);
    for agent in [false, true] {
        let error = office
            .set_current(AGENT, agent, Some(PROJECT))
            .unwrap()
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::NotFound);
    }
    assert_eq!(
        office
            .set_current(AGENT, false, Some(ITEM))
            .unwrap()
            .unwrap_err()
            .message,
        "the caller isn't an agent"
    );
    assert_eq!(
        office
            .set_current(AGENT, false, None)
            .unwrap()
            .unwrap_err()
            .message,
        "the caller isn't an agent"
    );
    mutate(&office, |item| {
        item.kind = ItemKind::Bug;
        item.deleted = true;
        item.assignee_id = None;
    });
    assert_eq!(
        office
            .set_current(AGENT, false, Some(ITEM))
            .unwrap()
            .unwrap_err()
            .message,
        "the caller isn't an agent"
    );
    assert_eq!(
        office
            .set_current(AGENT, true, Some(ITEM))
            .unwrap()
            .unwrap_err()
            .message,
        "this call is for a task, and the item is a bug"
    );
    mutate(&office, |item| item.kind = ItemKind::Task);
    assert_eq!(
        office
            .set_current(AGENT, true, Some(ITEM))
            .unwrap()
            .unwrap_err()
            .message,
        "the item is deleted"
    );
    mutate(&office, |item| item.deleted = false);
    for assignee in [None, Some(PROJECT.to_owned())] {
        mutate(&office, |item| item.assignee_id = assignee);
        assert_eq!(
            office
                .set_current(AGENT, true, Some(ITEM))
                .unwrap()
                .unwrap_err()
                .message,
            "the task isn't assigned to you"
        );
    }
    assert_eq!(rows(&office), before, "even expired rows survive a refusal");
    mutate(&office, |item| item.assignee_id = Some(AGENT.into()));
    office
        .set_current(AGENT, true, Some(ITEM))
        .unwrap()
        .unwrap();
    assert_eq!(rows(&office).len(), 1, "a successful write sweeps");
}

#[test]
fn current_task_reads_validate_missing_deleted_kind_and_assignee_without_rewriting_rows() {
    let (_dir, _key, office) = made();
    task(&office);
    office
        .set_current(AGENT, true, Some(ITEM))
        .unwrap()
        .unwrap();
    let before = rows(&office);
    for condition in 0..4 {
        mutate(&office, |item| {
            item.deleted = condition == 0;
            item.kind = if condition == 1 {
                ItemKind::Bug
            } else {
                ItemKind::Task
            };
            item.assignee_id = if condition == 2 {
                None
            } else if condition == 3 {
                Some(PROJECT.into())
            } else {
                Some(AGENT.into())
            };
        });
        assert_eq!(current(&office), None, "condition {condition}");
        let set = office
            .set_status(AGENT, &StatusSetParams::default())
            .unwrap()
            .unwrap();
        assert_eq!(set.1, None, "status event validates too");
    }
    assert_eq!(rows(&office)[0].2, before[0].2, "invalid ids stay stored");
    mutate(&office, |item| item.assignee_id = Some(AGENT.into()));
    assert_eq!(current(&office).as_deref(), Some(ITEM));
    office
        .local_store()
        .run_sql_for_test("UPDATE member_status SET current_task_id='01ARZ3NDEKTSV4RRFFQ69G5FAY';");
    assert_eq!(current(&office), None);
}

#[test]
fn current_task_decode_failure_rolls_back_a_status_write_and_does_not_poison_the_reader() {
    let (_dir, _key, office) = made();
    task(&office);
    office
        .set_current(AGENT, true, Some(ITEM))
        .unwrap()
        .unwrap();
    let before = rows(&office);
    office
        .local_store()
        .run_sql_for_test("UPDATE work_items SET data=json_set(data,'$.title',42);");
    assert!(office
        .set_status(
            AGENT,
            &StatusSetParams {
                text: Some("rolled back".into()),
                ttl_ms: None
            }
        )
        .is_err());
    assert_eq!(rows(&office), before);
    assert!(office.who_with_status(&WhoParams::default()).is_err());
    office
        .local_store()
        .run_sql_for_test("UPDATE work_items SET data=json_set(data,'$.title','First title');");
    assert_eq!(current(&office).as_deref(), Some(ITEM));
}

#[test]
fn current_task_status_projection_selects_each_members_own_row() {
    let (_dir, _key, office) = made();
    task(&office);
    let owner = office
        .org_snapshot()
        .unwrap()
        .members
        .into_iter()
        .find(|m| m.owner)
        .unwrap()
        .id;
    let text = |text: &str| StatusSetParams {
        text: Some(text.into()),
        ttl_ms: None,
    };
    assert_eq!(
        office
            .set_status(&owner, &text("Owner line"))
            .unwrap()
            .unwrap()
            .1,
        None
    );
    let event = office
        .set_current(AGENT, true, Some(ITEM))
        .unwrap()
        .unwrap();
    assert_eq!(event.status, None);
    assert_eq!(event.current_task_id.as_deref(), Some(ITEM));
    let set = office
        .set_status(AGENT, &text("Agent line"))
        .unwrap()
        .unwrap();
    assert_eq!(set.0.status.as_deref(), Some("Agent line"));
    assert_eq!(set.1.as_deref(), Some(ITEM));
    let other = office
        .set_status(&owner, &text("Owner changed"))
        .unwrap()
        .unwrap();
    assert_eq!(other.0.status.as_deref(), Some("Owner changed"));
    assert_eq!(other.1, None);
    let event = office
        .set_current(AGENT, true, Some(ITEM))
        .unwrap()
        .unwrap();
    assert_eq!(event.status.as_deref(), Some("Agent line"));
    assert_eq!(event.current_task_id.as_deref(), Some(ITEM));
    let cards = office
        .who_with_status(&WhoParams::default())
        .unwrap()
        .unwrap();
    let other = cards.members.iter().find(|m| m.id == owner).unwrap();
    assert_eq!(other.status.as_deref(), Some("Owner changed"));
    assert_eq!(other.current_task_id, None);
    assert_eq!(current(&office).as_deref(), Some(ITEM));
}
