//! `TASK-87`: replicated task batches on a satellite (`doc-77`, `doc-79`), on both engines.
//! Two stores in the same state stand for HQ and a satellite that has seen all of it; HQ's
//! journal entries are applied to the satellite as replication carries them.
//!
//! - an echo of a child-only change (no `task.md`) after a later local one never rewrites
//!   `checklist.yaml` back;
//! - a snapshot part holding a task's checklist with older history than the satellite's is
//!   not stale when the whole snapshot brings the task further;
//! - a snapshot part holding only a task's child files (no `task.md`, no event) is judged by
//!   the snapshot's revision for the task, and never reverts a later local write;
//! - HQ's `MoveTeam` moves every local projectless task of the moved subtree, those HQ never
//!   named included, whether the move arrives whole or in pieces.

use std::collections::BTreeMap;

use org_proto::tasks::TasksChecklistAddParams;
use serde_json::json;

use super::task_storage::{
    apply_batch, batches_after, keep_entries_from_here, move_b_under_d, params, team_folder, Bench,
    TEAM_B, TEAM_C, TEAM_D,
};
use super::*;
use crate::replication::state_files::ApplyReplicatedFiles;
use crate::store::open::task_ops::TaskChecklistAddOp;
use crate::store::open::ApplyReplicatedFilesOp;
use crate::tasks::ops::TaskWriteAuthority;
use crate::tasks::records::{self, ChecklistStepRecord, Location};

type Batch = Vec<(String, Option<Vec<u8>>)>;

struct ReadSteps(Id);

impl ReadOp for ReadSteps {
    type Out = Vec<ChecklistStepRecord>;
    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        Ok(snapshot.checklist_steps(&self.0)?)
    }
}

fn steps(b: &Bench, task: &str) -> Vec<Id> {
    let (steps, _) = b.store().read(ReadSteps(Id::parse(task).unwrap())).unwrap();
    steps.into_iter().map(|s| s.id).collect()
}

/// Adds a checklist step to `task`; returns its ID.
fn add_step(b: &mut Bench, task: &str, text: &str) -> Id {
    let (command, step) = (b.fresh(), b.fresh());
    let out = b.store().run(TaskChecklistAddOp {
        actor: b.actor(),
        params: params::<TasksChecklistAddParams>(json!({
            "command_id": command.as_str(), "task": task,
            "step_id": step.as_str(), "text": text, "required": false,
        })),
        authority: TaskWriteAuthority::Unrestricted,
    });
    out.expect("stored").expect("added");
    step
}

fn apply_part(b: &Bench, op: u64, files: Batch, revisions: BTreeMap<Id, u64>) {
    let reply = b.store().run(ApplyReplicatedFilesOp {
        operation_id: id(op),
        actor: Actor::System,
        command: ApplyReplicatedFiles {
            files,
            rehome_tasks: false,
            task_revisions: revisions,
        },
    });
    assert_eq!(reply.expect("applied").outcome, Outcome::Completed);
}

fn task_dir(chain: &[u64], task: &str) -> String {
    format!("{}/work/tasks/{task}", team_folder(chain))
}

fn checklist_on_disk(b: &Bench, chain: &[u64], task: &str) -> Vec<Id> {
    let bytes = b
        .file(&format!("{}/checklist.yaml", task_dir(chain, task)))
        .expect("a checklist file");
    records::decode_checklist(&bytes)
        .unwrap()
        .into_iter()
        .map(|s| s.id)
        .collect()
}

#[test]
fn an_echo_of_a_child_only_change_never_reverts_a_later_local_one() {
    for engine in ENGINES {
        let mut hq = Bench::new(engine);
        let mut sat = Bench::new(engine);
        let task = hq.create(Some(TEAM_B), None);
        assert_eq!(sat.create(Some(TEAM_B), None).id, task.id);
        // The satellite's first step (revision 2) reached HQ, which echoes it back...
        let mark = keep_entries_from_here(hq.store());
        let first = add_step(&mut hq, &task.id, "Step one");
        assert_eq!(add_step(&mut sat, &task.id, "Step one"), first);
        let echo = batches_after(hq.store(), mark);
        // ...after the satellite's second step (revision 3).
        let second = add_step(&mut sat, &task.id, "Step two");
        assert_eq!(sat.task(&task.id).unwrap().meta.revision, 3);
        assert!(
            echo.iter().flatten().all(|(p, _)| !p.ends_with("/task.md")),
            "{engine:?}: a child-only change carries no task.md"
        );
        for (n, batch) in echo.into_iter().enumerate() {
            apply_batch(sat.store(), 0x70 + n as u64, batch, false);
        }
        sat.store().sync().unwrap();

        let both = vec![first, second];
        assert_eq!(steps(&sat, &task.id), both, "{engine:?}: projection");
        assert_eq!(
            checklist_on_disk(&sat, &[TOP_TEAM, TEAM_B], &task.id),
            both,
            "{engine:?}: checklist.yaml"
        );
        assert_eq!(sat.task(&task.id).unwrap().meta.revision, 3);
        let dump = sat.store().dump();
        sat.rebuild();
        assert_eq!(sat.store().dump(), dump, "{engine:?}: rebuild parity");
        assert_eq!(steps(&sat, &task.id), both);
    }
}

#[test]
fn a_snapshot_part_with_older_history_applies_when_the_whole_snapshot_is_newer() {
    for engine in ENGINES {
        let mut hq = Bench::new(engine);
        let mut sat = Bench::new(engine);
        let task = hq.create(Some(TEAM_B), None);
        assert_eq!(sat.create(Some(TEAM_B), None).id, task.id);
        let first = add_step(&mut hq, &task.id, "Step one");
        assert_eq!(add_step(&mut sat, &task.id, "Step one"), first);
        let second = add_step(&mut hq, &task.id, "Step two");
        hq.store().sync().unwrap();

        // HQ's snapshot of the task in two parts, as a large snapshot's batches split it:
        // its checklist (both steps) with history through revision 1, then the rest.
        let tid = Id::parse(&task.id).unwrap();
        let location = Location::Team(vec![id(TOP_TEAM), id(TEAM_B)]);
        let file = |path: &str| (path.to_owned(), Some(hq.file(path).expect(path)));
        let checklist = records::checklist_path(&location, &tid).unwrap();
        let event = |n| records::event_path(&tid, n);
        let part_a = vec![file(checklist.as_str()), file(event(1).as_str())];
        let part_b = vec![file(event(2).as_str()), file(event(3).as_str())];

        // Judged alone, the first part is older than the satellite (revision 2): skipped.
        apply_part(&sat, 0x80, part_a.clone(), BTreeMap::new());
        assert_eq!(
            steps(&sat, &task.id),
            std::slice::from_ref(&first),
            "{engine:?}"
        );
        // As part of a snapshot that brings the task to revision 3, it applies.
        let whole = BTreeMap::from([(tid.clone(), 3)]);
        apply_part(&sat, 0x81, part_a, whole.clone());
        apply_part(&sat, 0x82, part_b, whole);
        sat.store().sync().unwrap();

        let both = vec![first, second];
        assert_eq!(steps(&sat, &task.id), both, "{engine:?}");
        assert_eq!(sat.task(&task.id).unwrap().meta.revision, 3);
        let dump = sat.store().dump();
        sat.rebuild();
        assert_eq!(sat.store().dump(), dump, "{engine:?}: rebuild parity");
    }
}

#[test]
fn a_child_only_snapshot_part_older_than_the_local_task_is_skipped() {
    for engine in ENGINES {
        let mut hq = Bench::new(engine);
        let mut sat = Bench::new(engine);
        let task = hq.create(Some(TEAM_B), None);
        assert_eq!(sat.create(Some(TEAM_B), None).id, task.id);
        let first = add_step(&mut hq, &task.id, "Step one");
        assert_eq!(add_step(&mut sat, &task.id, "Step one"), first);
        hq.store().sync().unwrap();
        // A local write lands while HQ's snapshot (revision 2) is in flight...
        let second = add_step(&mut sat, &task.id, "Step two");
        assert_eq!(sat.task(&task.id).unwrap().meta.revision, 3);

        // ...then a snapshot part holding only the task's checklist: no task.md, no event.
        let tid = Id::parse(&task.id).unwrap();
        let location = Location::Team(vec![id(TOP_TEAM), id(TEAM_B)]);
        let checklist = records::checklist_path(&location, &tid).unwrap();
        let part = vec![(
            checklist.as_str().to_owned(),
            Some(hq.file(checklist.as_str()).expect("HQ's checklist")),
        )];
        apply_part(&sat, 0x83, part, BTreeMap::from([(tid, 2)]));
        sat.store().sync().unwrap();

        let both = vec![first, second];
        assert_eq!(steps(&sat, &task.id), both, "{engine:?}: projection");
        assert_eq!(
            checklist_on_disk(&sat, &[TOP_TEAM, TEAM_B], &task.id),
            both,
            "{engine:?}: checklist.yaml"
        );
        assert_eq!(sat.task(&task.id).unwrap().meta.revision, 3);
        let dump = sat.store().dump();
        sat.rebuild();
        assert_eq!(sat.store().dump(), dump, "{engine:?}: rebuild parity");
        assert_eq!(steps(&sat, &task.id), both, "{engine:?}: after rebuild");
    }
}

/// The satellite before HQ's `MoveTeam(B -> under D)`: a task both offices hold (`shared`,
/// newer here when `newer_here`), and a projectless task made here offline under C, below
/// B, that HQ has never seen. Returns `(hq, sat, shared, offline)` and HQ's move batches.
fn before_the_move(
    engine: EngineChoice,
    newer_here: bool,
) -> (Bench, Bench, String, String, Batch) {
    let mut hq = Bench::new(engine);
    let mut sat = Bench::new(engine);
    let shared = hq.create(Some(TEAM_B), None);
    assert_eq!(sat.create(Some(TEAM_B), None).id, shared.id);
    if newer_here {
        sat.child_changes(&shared);
    }
    let offline = sat.create(Some(TEAM_C), None);
    sat.child_changes(&offline);
    let mark = keep_entries_from_here(hq.store());
    move_b_under_d(hq.store());
    let mut batches = batches_after(hq.store(), mark);
    assert_eq!(batches.len(), 1, "{engine:?}: the move is one entry");
    let batch = batches.remove(0);
    assert!(
        batch.iter().all(|(p, _)| !p.contains(&offline.id)),
        "{engine:?}: HQ never names the offline task"
    );
    (hq, sat, shared.id, offline.id, batch)
}

fn assert_moved(sat: &mut Bench, shared: &str, offline: &str, revisions: [u64; 2]) {
    let engine = sat.engine;
    sat.store().sync().unwrap();
    for (task, old, new) in [
        (
            shared,
            vec![TOP_TEAM, TEAM_B],
            vec![TOP_TEAM, TEAM_D, TEAM_B],
        ),
        (
            offline,
            vec![TOP_TEAM, TEAM_B, TEAM_C],
            vec![TOP_TEAM, TEAM_D, TEAM_B, TEAM_C],
        ),
    ] {
        assert!(
            tree(&sat.durable(&task_dir(&old, task))).is_empty(),
            "{engine:?}: {task} left in its old folder"
        );
        assert!(
            sat.file(&format!("{}/task.md", task_dir(&new, task)))
                .is_some(),
            "{engine:?}: {task} in its new folder"
        );
    }
    let offline_files = tree(&sat.durable(&task_dir(&[TOP_TEAM, TEAM_D, TEAM_B, TEAM_C], offline)));
    assert_eq!(
        offline_files.len(),
        4,
        "{engine:?}: {:?}",
        offline_files.keys()
    );
    for (task, revision) in [shared, offline].into_iter().zip(revisions) {
        assert_eq!(
            sat.task(task).unwrap().meta.revision,
            revision,
            "{engine:?}"
        );
    }
    // The full rebuild accepts every task in the folder its chain names.
    let dump = sat.store().dump();
    sat.rebuild();
    assert_eq!(sat.store().dump(), dump, "{engine:?}: rebuild parity");
}

#[test]
fn a_satellite_applying_a_move_relocates_its_offline_tasks_hq_never_named() {
    for engine in ENGINES {
        let (_hq, mut sat, shared, offline, batch) = before_the_move(engine, false);
        let revisions = [
            sat.task(&shared).unwrap().meta.revision,
            sat.task(&offline).unwrap().meta.revision,
        ];
        apply_batch(sat.store(), 0x90, batch, false);
        assert_moved(&mut sat, &shared, &offline, revisions);
    }
}

#[test]
fn a_move_arriving_in_pieces_moves_every_task_with_its_team() {
    for engine in ENGINES {
        let (hq, mut sat, shared, offline, batch) = before_the_move(engine, true);
        let revisions = [
            sat.task(&shared).unwrap().meta.revision,
            sat.task(&offline).unwrap().meta.revision,
        ];
        assert!(revisions[0] > hq.task(&shared).unwrap().meta.revision);
        // HQ's entry removes every old file, then writes the team's new file and the moved
        // files, task folders last. Split as the chunks of a large move are: the first piece
        // removes the shared task's old `task.md` and moves the team, the second brings the
        // moved task folders.
        let split = batch
            .iter()
            .position(|(path, bytes)| bytes.is_some() && path.contains("/work/tasks/"))
            .expect("the move writes task folders");
        let (first, second) = batch.split_at(split);
        assert!(first
            .iter()
            .any(|(p, bytes)| bytes.is_none() && p.contains(&shared) && p.ends_with("/task.md")));
        assert!(second
            .iter()
            .any(|(p, bytes)| bytes.is_some() && p.contains(&shared) && p.ends_with("/task.md")));
        apply_batch(sat.store(), 0x91, first.to_vec(), false);
        sat.store().sync().unwrap();
        // Every task of the moved subtree moved with `team.yaml`, its newer local files
        // included: the removal never took the task the move relocated.
        let new = task_dir(&[TOP_TEAM, TEAM_D, TEAM_B], &shared);
        assert!(sat.file(&format!("{new}/task.md")).is_some(), "{engine:?}");
        assert_eq!(sat.task(&shared).unwrap().meta.revision, revisions[0]);
        assert_eq!(sat.task(&offline).unwrap().meta.revision, revisions[1]);

        apply_batch(sat.store(), 0x92, second.to_vec(), false);
        assert_moved(&mut sat, &shared, &offline, revisions);
    }
}
