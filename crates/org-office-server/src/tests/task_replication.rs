//! `TASK-87`: task history across federation, against a real HQ and an enrolled satellite
//! (`doc-79`). A task's effective revision is its latest history event's, and child-only
//! changes (a checklist step, a comment, a watcher) write no `task.md`:
//!
//! - a paged snapshot brings an HQ task with child-only changes past the satellite's copy,
//!   never taking it for older than the satellite's;
//! - HQ's echo of a satellite's child-only change never rewrites a later local one.

use std::sync::Arc;

use org_office::Office;
use org_proto::tasks::{TaskInspectResult, TasksChecklistAddParams, TasksCommentParams};
use org_store::{DurablePath, Files};

use super::org_event_bursts::{create_task, files_of, fixture, hq_events_after, sat_position};
use crate::federation::SatelliteSyncSession;

fn add_step(office: &Office, actor: &str, task: &str, text: &str) -> String {
    let step = office.next_uuid_v7();
    let params: TasksChecklistAddParams = serde_json::from_value(serde_json::json!({
        "command_id": office.next_uuid_v7(), "task": task,
        "step_id": step, "text": text, "required": false,
    }))
    .unwrap();
    office.task_checklist_add(actor, params).unwrap().unwrap();
    step
}

fn comment(office: &Office, actor: &str, task: &str, text: &str) -> String {
    let comment = office.next_uuid_v7();
    let params: TasksCommentParams = serde_json::from_value(serde_json::json!({
        "command_id": office.next_uuid_v7(), "task": task,
        "comment_id": comment, "body_markdown": text,
    }))
    .unwrap();
    office.task_comment(actor, params).unwrap().unwrap();
    comment
}

fn inspect(office: &Office, task: &str) -> TaskInspectResult {
    office.task_inspect(task).unwrap().unwrap()
}

/// The ids of the task's steps, comments and history revisions.
fn children(office: &Office, task: &str) -> (Vec<String>, Vec<String>, Vec<u64>) {
    let seen = inspect(office, task);
    let steps = seen.checklist.into_iter().map(|s| s.id).collect();
    let comments = seen.comments.into_iter().map(|c| c.id).collect();
    let mut revisions: Vec<u64> = seen.history.into_iter().map(|e| e.revision).collect();
    revisions.sort_unstable();
    (steps, comments, revisions)
}

/// What `office`'s own files hold at `path`.
fn file_at(office: &Office, path: &str) -> Vec<u8> {
    let files = Files::open(office.durable_root()).unwrap();
    let held = files.read(&DurablePath::parse(path).unwrap()).unwrap();
    held.expect("a file").to_vec()
}

#[test]
fn a_paged_snapshot_brings_an_hq_tasks_child_only_history_past_the_satellites() {
    let f = fixture("Snapshot History", 51);
    let task = create_task(&f.hq, &f.owner, &f.hq_team, "HQ task");
    // Child-only changes before the satellite's baseline: it holds the task at revision 3,
    // past its `task.md` (revision 1).
    let before = [
        add_step(&f.hq, &f.owner, &task, "Before"),
        comment(&f.hq, &f.owner, &task, "Before"),
    ];
    // Committed changes are published once flushed.
    f.hq.sync_store().unwrap();
    let mut session = SatelliteSyncSession::connect(Arc::clone(&f.sat)).unwrap();
    session.pull_org_events().unwrap();
    assert_eq!(inspect(&f.sat, &task).task.revision, 3);

    // And after it: HQ's task moves on by child-only changes alone (revisions 4 and 5).
    let after = [
        add_step(&f.hq, &f.owner, &task, "After"),
        comment(&f.hq, &f.owner, &task, "After"),
    ];
    f.hq.sync_store().unwrap();
    session
        .sync_initial_snapshot(&f.sat.next_uuid_v7())
        .expect("the snapshot applies");

    assert_eq!(inspect(&f.sat, &task).task.revision, 5);
    let (steps, comments, revisions) = children(&f.sat, &task);
    assert_eq!(steps, [before[0].clone(), after[0].clone()]);
    let mut expected_comments = vec![before[1].clone(), after[1].clone()];
    expected_comments.sort();
    let mut comments = comments;
    comments.sort();
    assert_eq!(comments, expected_comments);
    assert_eq!(
        revisions,
        [1, 2, 3, 4, 5],
        "no gap in the satellite's history"
    );
    assert_eq!(children(&f.sat, &task), children(&f.hq, &task));
    f.server.stop();
}

#[test]
fn an_hq_echo_of_a_child_only_change_never_rewrites_a_later_local_one() {
    let f = fixture("Child Echo", 52);
    let sat_team = f.sat.org_snapshot().unwrap().teams[0].id.clone();
    // The satellite's own task, at HQ and echoed back.
    let task = create_task(&f.sat, &f.owner, &sat_team, "Satellite task");
    let mut session = SatelliteSyncSession::connect(Arc::clone(&f.sat)).unwrap();
    assert_eq!(session.flush_work_outbox().unwrap(), 1);
    session.pull_org_events().unwrap();
    let start = sat_position(&f.sat);

    // Step one (revision 2) reaches HQ; step two (revision 3) is still only here.
    let first = add_step(&f.sat, &f.owner, &task, "Step one");
    f.sat.sync_store().unwrap();
    f.sat.sync_satellite_work_outbox().unwrap();
    assert_eq!(session.flush_work_outbox().unwrap(), 1);
    let second = add_step(&f.sat, &f.owner, &task, "Step two");
    assert_eq!(inspect(&f.sat, &task).task.revision, 3);

    // HQ's echo of revision 2: its checklist and event, no `task.md`.
    let echo = hq_events_after(&f.hq, start);
    let files: Vec<_> = echo.iter().flat_map(files_of).collect();
    assert!(files.iter().all(|(p, _)| !p.ends_with("/task.md")));
    let checklist = files
        .iter()
        .map(|(p, _)| p.clone())
        .find(|p| p.ends_with("/checklist.yaml"))
        .unwrap();
    let kept = f.sat.filter_self_echoed_org_event_files(files).unwrap();
    assert!(
        kept.iter().all(|(p, _)| !p.contains(&task)),
        "the echo of the satellite's own older revision is dropped whole: {kept:?}"
    );

    session.pull_org_events().unwrap();
    let both = vec![first.clone(), second.clone()];
    assert_eq!(children(&f.sat, &task).0, both, "the projection keeps both");
    f.sat.sync_store().unwrap();
    let on_disk = String::from_utf8(file_at(&f.sat, &checklist)).unwrap();
    assert!(
        on_disk.contains(&first) && on_disk.contains(&second),
        "{on_disk}"
    );
    assert_eq!(inspect(&f.sat, &task).task.revision, 3);

    // Step two then reaches HQ, and both offices agree.
    f.sat.sync_store().unwrap();
    f.sat.sync_satellite_work_outbox().unwrap();
    assert_eq!(session.flush_work_outbox().unwrap(), 1);
    session.pull_org_events().unwrap();
    assert_eq!(children(&f.hq, &task).0, both);
    assert_eq!(inspect(&f.hq, &task).task.revision, 3);
    f.server.stop();
}
