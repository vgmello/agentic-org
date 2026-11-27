//! The satellite work outbox only enqueues entries that touch this office's own tasks.
use std::collections::BTreeSet;

use org_proto::tasks::{TaskPriority, TaskStatus, TasksCreateParams};
use org_store::{DurablePath, Id};

use super::is_locally_owned_satellite_work_entry;
use crate::office::Office;
use crate::store::open::EngineChoice;
use crate::tasks::records::{
    checklist_path, comment_path, counter_path, encode_task_md, event_path, task_md_path, Location,
    TaskMeta, TaskRecord,
};
use crate::TestOffice;

fn task_json(id: &str, code: &str, number: u64, team_id: &str, actor: &str) -> Vec<u8> {
    let rec = TaskRecord {
        id: Id::parse(id).unwrap(),
        office_code: code.into(),
        number,
        team_id: Id::parse(team_id).unwrap(),
        assigned_team_id: None,
        project_id: None,
        task_type: "task".into(),
        work_type: None,
        title: "Filter test".into(),
        status: TaskStatus::Todo,
        assignee_id: None,
        body_markdown: String::new(),
        feature_id: None,
        milestone_id: None,
        priority: TaskPriority::Normal,
        effort: None,
        severity: None,
        resolution: None,
        blocked_reason: None,
        rank_order: None,
        labels: Vec::new(),
        reference_files: Vec::new(),
        modified_files: Vec::new(),
        repositories: Vec::new(),
        details: None,
        final_summary: None,
        meta: TaskMeta::created(&Id::parse(actor).unwrap(), "2026-11-26T00:00:00.000Z"),
    };
    encode_task_md(&rec, &[], &[]).unwrap()
}

#[test]
fn satellite_work_outbox_filter_accepts_only_valid_local_work_entries() {
    let office = Office::for_test(TestOffice {
        engine: EngineChoice::Sqlite,
    });
    let owner = office.owner_member_id().to_owned();
    let team_id = office.org_snapshot().unwrap().teams[0].id.clone();

    // Create one real task with office_code "A" inside `office` so sidecar lookups resolve it.
    let created = office
        .task_create(
            &owner,
            TasksCreateParams {
                command_id: office.next_uuid_v7(),
                task_id: office.next_uuid_v7(),
                team_id: Some(team_id.clone()),
                assigned_team_id: None,
                project_id: None,
                task_type: Some("task".into()),
                work_type: None,
                title: "Existing task".into(),
                body_markdown: String::new(),
                assignee_id: None,
                priority: None,
                effort: None,
                severity: None,
                rank_order: None,
                labels: Vec::new(),
                reference_files: Vec::new(),
                modified_files: Vec::new(),
                depends_on: Vec::new(),
                details: None,
                dod: None,
                attachment_ids: Vec::new(),
            },
        )
        .unwrap()
        .unwrap();
    let existing_a_id = created.task.id;
    let local_b_id = office.next_uuid_v7();
    let other_id = office.next_uuid_v7();

    let local_b: BTreeSet<String> = ["B".to_owned()].into_iter().collect();
    let local_a: BTreeSet<String> = ["A".to_owned()].into_iter().collect();
    let empty: BTreeSet<String> = BTreeSet::new();

    let team = Location::Team(vec![Id::parse(&team_id).unwrap()]);
    let b = Id::parse(&local_b_id).unwrap();
    let a = Id::parse(&existing_a_id).unwrap();
    // The office's own counter is office-local: never a `state/` work file (`doc-80`).
    let p_counter = counter_path(&office.identity.office_id);
    let p_task_b = task_md_path(&team, &b).unwrap();
    let p_ev_b = event_path(&b, 1);
    let p_cmt_b = comment_path(&team, &b, &Id::parse(&office.next_uuid_v7()).unwrap()).unwrap();
    let p_chk_b = checklist_path(&team, &b).unwrap();
    let p_task_a = task_md_path(&team, &a).unwrap();
    let p_chk_a = checklist_path(&team, &a).unwrap();
    let p_ev_a = event_path(&a, 2);
    let p_legacy = DurablePath::parse(&format!("state/work/tasks/{local_b_id}/task.json")).unwrap();

    let raw_b = task_json(&local_b_id, "B", 1, &team_id, &owner);
    let raw_a = task_json(&existing_a_id, "A", 1, &team_id, &owner);
    let raw_mismatch_id = task_json(&other_id, "B", 2, &team_id, &owner);
    let dummy = b"{}".as_slice();

    // 1. Valid local task + immutable sidecar + mutable sidecar deletion.
    assert!(is_locally_owned_satellite_work_entry(
        &office,
        &local_b,
        &[
            (&p_task_b, Some(&raw_b)),
            (&p_ev_b, Some(dummy)),
            (&p_chk_b, None),
        ],
    )
    .unwrap());

    // 2. Sidecar-only entry for an existing locally owned task succeeds; for remote task fails.
    assert!(
        is_locally_owned_satellite_work_entry(&office, &local_a, &[(&p_chk_a, Some(dummy))])
            .unwrap()
    );
    assert!(
        !is_locally_owned_satellite_work_entry(&office, &local_b, &[(&p_chk_a, Some(dummy))])
            .unwrap()
    );

    // 3. Remote task, remote counter, mixed local+remote entry, and empty local_codes fail.
    assert!(!is_locally_owned_satellite_work_entry(
        &office,
        &local_b,
        &[(&p_task_a, Some(&raw_a))]
    )
    .unwrap());
    assert!(!is_locally_owned_satellite_work_entry(
        &office,
        &local_b,
        &[(&p_counter, Some(dummy))]
    )
    .unwrap());
    assert!(
        !is_locally_owned_satellite_work_entry(&office, &local_b, &[(&p_legacy, Some(dummy))])
            .unwrap()
    );
    assert!(!is_locally_owned_satellite_work_entry(
        &office,
        &local_b,
        &[(&p_task_b, Some(&raw_b)), (&p_ev_a, Some(dummy))],
    )
    .unwrap());
    assert!(
        !is_locally_owned_satellite_work_entry(&office, &empty, &[(&p_task_b, Some(&raw_b))])
            .unwrap()
    );

    // 4. Deletions of Task, Event, Comment, mismatched rec.id, and a malformed task.md fail.
    assert!(
        !is_locally_owned_satellite_work_entry(&office, &local_b, &[(&p_task_b, None)]).unwrap()
    );
    assert!(!is_locally_owned_satellite_work_entry(
        &office,
        &local_b,
        &[(&p_task_b, Some(&raw_b)), (&p_ev_b, None)],
    )
    .unwrap());
    assert!(!is_locally_owned_satellite_work_entry(
        &office,
        &local_b,
        &[(&p_task_b, Some(&raw_b)), (&p_cmt_b, None)],
    )
    .unwrap());
    assert!(!is_locally_owned_satellite_work_entry(
        &office,
        &local_b,
        &[(&p_task_b, Some(&raw_mismatch_id))],
    )
    .unwrap());
    assert!(!is_locally_owned_satellite_work_entry(
        &office,
        &local_b,
        &[(&p_task_b, Some(b"not a task.md"))],
    )
    .unwrap());
}
