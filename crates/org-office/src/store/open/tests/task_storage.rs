//! `TASK-87`: task storage on the `doc-77`/`doc-79`/`doc-80` layout, on both engines.
//!
//! - project and projectless task folders (`task.md`, `comments/<id>.md`, `checklist.yaml`,
//!   `operations.yaml`), `canonical-json/2` history under `state/work-history/`, and nothing
//!   under the flat pre-v1 `state/work/`;
//! - the office-local `task-counter.yaml` and its `ADR-0019` block reservation: `committed`
//!   inside a block, `durable` for a new one, a jump past it after a stop that was not clean,
//!   and survival of a `runtime/` wipe;
//! - child-only changes that never rewrite `task.md` while the effective revision advances,
//!   kept exactly by a rebuild with optimistic concurrency intact;
//! - a rebuild refusing a tampered event, a history gap, or `task.md` ahead of its history;
//! - `MoveTeam` moving every projectless task folder of the subtree in one journal entry.

use std::path::PathBuf;

use org_proto::tasks::{
    TaskView, TasksChecklistAddParams, TasksChecklistCheckParams, TasksCommentParams,
    TasksCreateParams, TasksUpdateParams, TasksWatchParams,
};
use org_proto::ErrorCode;
use org_store::{Ack, Id, OfficeIdentity};
use serde_json::json;

use crate::organization::features::MoveTeam;
use crate::projects::features::{CreateProject, OwnerTeamChoice};
use crate::store::open::task_ops::{
    TaskChecklistAddOp, TaskChecklistCheckOp, TaskCommentOp, TaskCreateOp, TaskUpdateOp,
    TaskWatchOp,
};
use crate::store::open::{ApplyReplicatedFilesOp, CreateProjectOp, MoveTeamOp};
use crate::tasks::features::TASK_NUMBER_BLOCK;
use crate::tasks::records::{OfficeTaskCounterRecord, TaskRecord};

use super::*;

const TEAM_B: u64 = 0x31;
const TEAM_C: u64 = 0x32;
const TEAM_D: u64 = 0x33;
const PROJECT: u64 = 0x41;

/// A UUIDv7 for the `n`th command, task, comment or step of a test.
fn uid(n: u64) -> Id {
    id(0x7000_0000 + n)
}

fn params<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> T {
    serde_json::from_value(value).expect("valid params")
}

struct Bench {
    place: Place,
    engine: EngineChoice,
    store: Option<OfficeStore>,
    here: OfficeIdentity,
    next: u64,
}

impl Bench {
    /// An organization (top team), team B under it, team C under B and team D under the
    /// top team, and a project owned by the top team.
    fn new(engine: EngineChoice) -> Self {
        let place = Place::new();
        let store = place.create(engine);
        assert_eq!(
            create_organization(&store, 1).unwrap().outcome,
            Outcome::Completed
        );
        let bench = Self {
            place,
            engine,
            store: Some(store),
            here: identity(),
            next: 1,
        };
        for (op, team, parent) in [
            (2, TEAM_B, TOP_TEAM),
            (3, TEAM_C, TEAM_B),
            (4, TEAM_D, TOP_TEAM),
        ] {
            let reply = bench.store().run(CreateTeamOp {
                operation_id: id(op),
                here: identity(),
                owner: id(CREATOR),
                by: Actor::Employee(id(CREATOR)),
                command: team_command(team, parent),
            });
            assert_eq!(reply.unwrap().outcome, Outcome::Completed, "team {team:x}");
        }
        let project = bench.store().run(CreateProjectOp {
            operation_id: id(5),
            here: identity(),
            owner: id(CREATOR),
            by: Actor::Employee(id(CREATOR)),
            command: CreateProject {
                id: id(PROJECT),
                name: "Storage".into(),
                slug: None,
                owner_team: OwnerTeamChoice::Existing(id(TOP_TEAM)),
                contributing_team_ids: Vec::new(),
            },
        });
        assert_eq!(project.unwrap().outcome, Outcome::Completed);
        bench
    }

    fn store(&self) -> &OfficeStore {
        self.store.as_ref().expect("an open store")
    }

    fn fresh(&mut self) -> Id {
        self.next += 1;
        uid(self.next)
    }

    fn actor(&self) -> String {
        id(CREATOR).as_str().to_owned()
    }

    fn create(&mut self, team: Option<u64>, project: Option<u64>) -> TaskView {
        let (command, task) = (self.fresh(), self.fresh());
        let out = self.store().run(TaskCreateOp {
            actor: self.actor(),
            here: self.here.clone(),
            params: params::<TasksCreateParams>(json!({
                "command_id": command.as_str(),
                "task_id": task.as_str(),
                "team_id": team.map(|t| id(t).as_str().to_owned()),
                "project_id": project.map(|p| id(p).as_str().to_owned()),
                "title": format!("Task {task}"),
                "body_markdown": "The body.\n",
                "labels": ["storage"],
            })),
            counter_unsettled: self.store().task_counter_unsettled(),
        });
        out.expect("stored").expect("created").task
    }

    fn update(&mut self, task: &TaskView, expected: u64) -> Result<TaskView, ErrorCode> {
        let command = self.fresh();
        let out = self.store().run(TaskUpdateOp {
            actor: self.actor(),
            params: params::<TasksUpdateParams>(json!({
                "command_id": command.as_str(),
                "task": task.id,
                "expected_revision": expected,
                "title": format!("Renamed at {expected}"),
            })),
        });
        out.expect("stored").map(|o| o.task).map_err(|e| e.code)
    }

    /// A comment, a watch, a checklist step and its check: the child-only changes.
    /// Returns their command IDs in that order.
    fn child_changes(&mut self, task: &TaskView) -> [Id; 4] {
        let ids = [self.fresh(), self.fresh(), self.fresh(), self.fresh()];
        let (comment, step) = (self.fresh(), self.fresh());
        let actor = self.actor();
        let store = self.store();
        store
            .run(TaskCommentOp {
                actor: actor.clone(),
                params: params::<TasksCommentParams>(json!({
                    "command_id": ids[0].as_str(), "task": task.id,
                    "comment_id": comment.as_str(), "body_markdown": "A comment.",
                })),
            })
            .unwrap()
            .unwrap();
        store
            .run(TaskWatchOp {
                actor: actor.clone(),
                params: params::<TasksWatchParams>(
                    json!({"command_id": ids[1].as_str(), "task": task.id}),
                ),
            })
            .unwrap()
            .unwrap();
        store
            .run(TaskChecklistAddOp {
                actor: actor.clone(),
                params: params::<TasksChecklistAddParams>(json!({
                    "command_id": ids[2].as_str(), "task": task.id,
                    "step_id": step.as_str(), "text": "Check it", "required": true,
                })),
            })
            .unwrap()
            .unwrap();
        store
            .run(TaskChecklistCheckOp {
                actor,
                params: params::<TasksChecklistCheckParams>(json!({
                    "command_id": ids[3].as_str(), "task": task.id,
                    "step_id": step.as_str(), "done": true,
                })),
            })
            .unwrap()
            .unwrap();
        ids
    }

    fn task(&self, task: &str) -> Option<TaskRecord> {
        let id = Id::parse(task).unwrap();
        self.store().read(ReadTask(id)).expect("a read").0
    }

    fn ack(&self, operation: &Id) -> Option<Ack> {
        self.store()
            .read(ReadAck(operation.clone()))
            .expect("a read")
            .0
    }

    fn counter(&self) -> Vec<OfficeTaskCounterRecord> {
        self.store().read(ReadCounters).expect("a read").0
    }

    fn durable(&self, path: &str) -> PathBuf {
        self.place.durable().join(path)
    }

    fn file(&self, path: &str) -> Option<Vec<u8>> {
        std::fs::read(self.durable(path)).ok()
    }

    /// An orderly stop and a new opening.
    fn reopen_clean(&mut self) {
        self.store.take().expect("open").close().expect("closed");
        self.store = Some(self.place.reopen(self.engine));
    }

    /// A kill: no close, no clean-stop marker.
    fn reopen_after_crash(&mut self) {
        self.store().sync().expect("synced");
        drop(self.store.take());
        self.store = Some(self.place.reopen(self.engine));
    }

    /// An orderly stop, the projection deleted, and a rebuild from the files and journal.
    fn rebuild(&mut self) {
        self.store.take().expect("open").close().expect("closed");
        let _ = std::fs::remove_dir_all(self.place.runtime());
        self.store = Some(self.place.reopen(self.engine));
    }

    /// An orderly stop and the projection deleted, the store not reopened.
    fn close_and_wipe_runtime(&mut self) {
        self.store.take().expect("open").close().expect("closed");
        let _ = std::fs::remove_dir_all(self.place.runtime());
    }
}

struct ReadTask(Id);

impl ReadOp for ReadTask {
    type Out = Option<TaskRecord>;
    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        Ok(snapshot.task(&self.0)?)
    }
}

struct ReadAck(Id);

impl ReadOp for ReadAck {
    type Out = Option<Ack>;
    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        Ok(snapshot.receipt(&self.0)?.map(|r| r.ack))
    }
}

struct ReadCounters;

impl ReadOp for ReadCounters {
    type Out = Vec<OfficeTaskCounterRecord>;
    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        Ok(snapshot.office_task_counters()?)
    }
}

fn team_folder(chain: &[u64]) -> String {
    let mut folder = String::from("state");
    for team in chain {
        folder.push_str(&format!("/teams/{}", id(*team)));
    }
    folder
}

#[test]
fn task_files_follow_the_data_contract_layout_and_nothing_is_written_under_state_work() {
    for engine in ENGINES {
        let mut b = Bench::new(engine);
        let deep = b.create(Some(TEAM_C), None);
        let project = b.create(None, Some(PROJECT));
        b.child_changes(&deep);
        b.store().sync().unwrap();

        let folder = format!(
            "{}/work/tasks/{}",
            team_folder(&[TOP_TEAM, TEAM_B, TEAM_C]),
            deep.id
        );
        let md = String::from_utf8(b.file(&format!("{folder}/task.md")).unwrap()).unwrap();
        assert!(md.starts_with("---\n"), "{engine:?}: {md}");
        assert!(md.ends_with("---\nThe body.\n"), "{engine:?}: {md}");
        assert!(md.contains(&format!("team_id: \"{}\"", id(TEAM_C))), "{md}");
        let files = tree(&b.durable(&folder));
        let names: Vec<&str> = files.keys().map(String::as_str).collect();
        assert_eq!(names.len(), 4, "{engine:?}: {names:?}");
        assert!(names.contains(&"task.md"));
        assert!(names.contains(&"checklist.yaml"));
        assert!(names.contains(&"operations.yaml"));
        assert!(names
            .iter()
            .any(|n| n.starts_with("comments/") && n.ends_with(".md")));
        let history = tree(&b.durable(&format!("state/work-history/tasks/{}", deep.id)));
        let revisions: Vec<&str> = history.keys().map(String::as_str).collect();
        assert_eq!(
            revisions,
            ["1.json", "2.json", "3.json", "4.json", "5.json"]
        );

        let project_md = format!(
            "state/projects/{}/work/tasks/{}/task.md",
            id(PROJECT),
            project.id
        );
        assert!(b.file(&project_md).is_some(), "{engine:?}");
        let counter = format!("local/offices/{}/task-counter.yaml", identity().office_id);
        assert!(b.file(&counter).is_some(), "{engine:?}");
        assert!(!b.durable("state/work").exists(), "{engine:?}");
        assert!(tree(&b.durable("state"))
            .keys()
            .all(|p| !p.ends_with("task.json") && !p.contains("task-counter")));
    }
}

#[test]
fn the_counter_reserves_blocks_durably_and_jumps_past_them_after_an_unclean_stop() {
    for engine in ENGINES {
        let mut b = Bench::new(engine);
        let first = b.create(Some(TEAM_B), None);
        let first_op = uid(b.next - 1);
        let second = b.create(Some(TEAM_B), None);
        let second_op = uid(b.next - 1);
        assert_eq!((first.number, second.number), (1, 2), "{engine:?}");
        // The first number needs a reservation (durable); the next is inside it (committed).
        assert_eq!(b.ack(&first_op), Some(Ack::Durable), "{engine:?}");
        assert_eq!(b.ack(&second_op), Some(Ack::Committed), "{engine:?}");
        let counter = b.counter();
        assert_eq!(counter.len(), 1);
        assert_eq!(counter[0].last_number, 2);
        assert_eq!(counter[0].reserved_through, TASK_NUMBER_BLOCK);

        // An orderly stop: numbering carries on inside the block.
        b.reopen_clean();
        let third = b.create(Some(TEAM_B), None);
        assert_eq!(third.number, 3, "{engine:?}");
        assert_eq!(b.ack(&uid(b.next - 1)), Some(Ack::Committed));

        // A kill: numbers up to the reservation may have been answered and lost, so the
        // next is past it, reserved anew and durable. None is ever issued twice.
        b.reopen_after_crash();
        let after_crash = b.create(Some(TEAM_B), None);
        assert_eq!(after_crash.number, TASK_NUMBER_BLOCK + 1, "{engine:?}");
        assert_eq!(b.ack(&uid(b.next - 1)), Some(Ack::Durable));
        let next = b.create(Some(TEAM_B), None);
        assert_eq!(next.number, TASK_NUMBER_BLOCK + 2);
        assert_eq!(b.ack(&uid(b.next - 1)), Some(Ack::Committed));

        // The counter is office-local durable state: a runtime wipe rebuilds it.
        b.rebuild();
        let counter = b.counter();
        assert_eq!(counter[0].last_number, TASK_NUMBER_BLOCK + 2, "{engine:?}");
        assert_eq!(counter[0].reserved_through, 2 * TASK_NUMBER_BLOCK);
        let after_rebuild = b.create(Some(TEAM_B), None);
        assert_eq!(after_rebuild.number, TASK_NUMBER_BLOCK + 3, "{engine:?}");
    }
}

#[test]
fn child_only_changes_never_rewrite_task_md_and_a_rebuild_keeps_the_effective_revision() {
    for engine in ENGINES {
        let mut b = Bench::new(engine);
        let task = b.create(Some(TEAM_B), None);
        let task = b.update(&task, 1).unwrap();
        assert_eq!(task.revision, 2);
        b.store().sync().unwrap();
        let md_path = format!(
            "{}/work/tasks/{}/task.md",
            team_folder(&[TOP_TEAM, TEAM_B]),
            task.id
        );
        let md = b.file(&md_path).unwrap();

        let ops = b.child_changes(&task);
        b.store().sync().unwrap();
        assert_eq!(
            b.file(&md_path).unwrap(),
            md,
            "{engine:?}: task.md rewritten"
        );
        assert_eq!(b.task(&task.id).unwrap().meta.revision, 6, "{engine:?}");
        let acks: Vec<_> = ops.iter().map(|op| b.ack(op)).collect();
        assert_eq!(
            acks,
            [
                Some(Ack::Committed),
                Some(Ack::Committed),
                Some(Ack::Durable),
                Some(Ack::Committed)
            ],
            "{engine:?}: comment, watch, checklist add, checklist check"
        );

        b.rebuild();
        assert_eq!(b.task(&task.id).unwrap().meta.revision, 6, "{engine:?}");
        // Optimistic concurrency holds across the rebuild: a stale revision conflicts.
        assert_eq!(b.update(&task, 5), Err(ErrorCode::Conflict), "{engine:?}");
        let updated = b.update(&task, 6).unwrap();
        assert_eq!(updated.revision, 7, "{engine:?}");
        assert_eq!(updated.comment_count, 1);
        assert_eq!(updated.watcher_count, 1);
        assert_eq!((updated.checklist_total, updated.checklist_done), (1, 1));
    }
}

#[test]
fn a_rebuild_refuses_a_tampered_event_a_history_gap_or_task_md_ahead_of_its_history() {
    for engine in ENGINES {
        for damage in ["tamper", "gap", "ahead"] {
            let mut b = Bench::new(engine);
            let task = b.create(Some(TEAM_B), None);
            let task = b.update(&task, 1).unwrap();
            b.update(&task, 2).unwrap();
            b.close_and_wipe_runtime();
            let history = format!("state/work-history/tasks/{}", task.id);
            let md = format!(
                "{}/work/tasks/{}/task.md",
                team_folder(&[TOP_TEAM, TEAM_B]),
                task.id
            );
            let (path, damaged) = match damage {
                "tamper" => {
                    let path = format!("{history}/2.json");
                    let text = String::from_utf8(b.file(&path).unwrap()).unwrap();
                    let damaged = text.replace("task_updated", "task_hidden");
                    assert_ne!(text, damaged);
                    (path, Some(damaged))
                }
                "gap" => (format!("{history}/2.json"), None),
                _ => {
                    let text = String::from_utf8(b.file(&md).unwrap()).unwrap();
                    (md.clone(), Some(text.replace("revision: 3", "revision: 9")))
                }
            };
            match damaged {
                Some(text) => std::fs::write(b.durable(&path), text).unwrap(),
                None => std::fs::remove_file(b.durable(&path)).unwrap(),
            }
            let opened = open_store(&b.place.config(engine, Opening::Existing));
            let error = opened.err().map(|e| e.to_string());
            assert!(
                error.is_some(),
                "{engine:?} {damage}: the damaged store opened"
            );
            let error = error.unwrap();
            let named = match damage {
                "gap" => format!("{history}/2.json"),
                _ => path,
            };
            assert!(
                error.contains(&named) || error.contains("history"),
                "{engine:?} {damage}: {error}"
            );
        }
    }
}

#[test]
fn move_team_moves_every_projectless_task_folder_of_the_subtree_in_one_entry() {
    for engine in ENGINES {
        let mut b = Bench::new(engine);
        let on_b = b.create(Some(TEAM_B), None);
        let on_c = b.create(Some(TEAM_C), None);
        let on_d = b.create(Some(TEAM_D), None);
        let in_project = b.create(None, Some(PROJECT));
        b.child_changes(&on_b);
        b.child_changes(&on_c);
        b.store().sync().unwrap();
        let old_b = format!(
            "{}/work/tasks/{}",
            team_folder(&[TOP_TEAM, TEAM_B]),
            on_b.id
        );
        let old_c = format!(
            "{}/work/tasks/{}",
            team_folder(&[TOP_TEAM, TEAM_B, TEAM_C]),
            on_c.id
        );
        let before_b = tree(&b.durable(&old_b));
        let before_c = tree(&b.durable(&old_c));
        let revisions = [on_b.id.clone(), on_c.id.clone()].map(|t| b.task(&t).unwrap().meta);

        // A consumer keeps the journal's entries from here for the check below.
        let mark = b.store().register_consumer("task-storage-test").unwrap();
        let move_op = id(0x60);
        let moved = b.store().run(MoveTeamOp {
            operation_id: move_op.clone(),
            here: identity(),
            owner: id(CREATOR),
            by: Actor::Employee(id(CREATOR)),
            command: MoveTeam {
                id: id(TEAM_B),
                expected_revision: 1,
                parent: id(TEAM_D),
            },
        });
        assert_eq!(moved.unwrap().outcome, Outcome::Completed, "{engine:?}");
        b.store().sync().unwrap();

        let new_b = format!(
            "{}/work/tasks/{}",
            team_folder(&[TOP_TEAM, TEAM_D, TEAM_B]),
            on_b.id
        );
        let new_c = format!(
            "{}/work/tasks/{}",
            team_folder(&[TOP_TEAM, TEAM_D, TEAM_B, TEAM_C]),
            on_c.id
        );
        // Nothing is left at the old paths (the store may leave empty folders).
        assert!(tree(&b.durable(&old_b)).is_empty(), "{engine:?}");
        assert!(tree(&b.durable(&old_c)).is_empty(), "{engine:?}");
        let after_b = tree(&b.durable(&new_b));
        let after_c = tree(&b.durable(&new_c));
        assert_eq!(
            after_b.keys().collect::<Vec<_>>(),
            before_b.keys().collect::<Vec<_>>()
        );
        assert_eq!(
            after_c.keys().collect::<Vec<_>>(),
            before_c.keys().collect::<Vec<_>>()
        );
        // Child files keep their bytes; `task.md` keeps every field.
        for (name, bytes) in &before_b {
            if name != "task.md" {
                assert_eq!(&after_b[name], bytes, "{engine:?} {name}");
            }
        }
        // Tasks of other teams and of projects stay where they are.
        let stay_d = format!(
            "{}/work/tasks/{}/task.md",
            team_folder(&[TOP_TEAM, TEAM_D]),
            on_d.id
        );
        assert!(b.file(&stay_d).is_some());
        let stay_p = format!(
            "state/projects/{}/work/tasks/{}/task.md",
            id(PROJECT),
            in_project.id
        );
        assert!(b.file(&stay_p).is_some());

        // One entry moved the team, its subteam and both task folders.
        let entries = b.store().entries_after(mark);
        let entry = entries
            .iter()
            .find(|e| {
                e.header
                    .operation
                    .as_ref()
                    .is_some_and(|op| op.id == move_op)
            })
            .expect("the move's entry");
        let paths: Vec<String> = entry
            .versions()
            .map(|v| v.path.as_str().to_owned())
            .collect();
        for folder in [&old_b, &old_c, &new_b, &new_c] {
            for name in before_b
                .keys()
                .filter(|_| folder == &old_b || folder == &new_b)
            {
                assert!(
                    paths.contains(&format!("{folder}/{name}")),
                    "{folder}/{name}"
                );
            }
            for name in before_c
                .keys()
                .filter(|_| folder == &old_c || folder == &new_c)
            {
                assert!(
                    paths.contains(&format!("{folder}/{name}")),
                    "{folder}/{name}"
                );
            }
        }

        // The moved tasks read the same, and a rebuild agrees with the live projection.
        let dump = b.store().dump();
        for (task, meta) in [&on_b, &on_c].into_iter().zip(&revisions) {
            assert_eq!(&b.task(&task.id).unwrap().meta, meta, "{engine:?}");
        }
        b.rebuild();
        assert_eq!(
            b.store().dump(),
            dump,
            "{engine:?}: rebuild parity after the move"
        );
        for (task, meta) in [&on_b, &on_c].into_iter().zip(&revisions) {
            assert_eq!(&b.task(&task.id).unwrap().meta, meta, "{engine:?}");
        }
        let updated = b.update(&on_c, revisions[1].revision).unwrap();
        assert_eq!(updated.revision, revisions[1].revision + 1);
        b.store().sync().unwrap();
        assert!(b.file(&format!("{new_c}/task.md")).is_some());
        assert!(tree(&b.durable(&old_c)).is_empty());
    }
}

/// Registers a consumer so the journal keeps its entries, and returns the head: the mark the
/// entries after it are read from.
fn keep_entries_from_here(store: &OfficeStore) -> u64 {
    store.register_consumer("task-storage-test").unwrap();
    store.sync().unwrap();
    store.status().flushed
}

/// The `state/` files of each entry after `mark`, one batch per entry, as replication carries
/// them.
fn batches_after(store: &OfficeStore, mark: u64) -> Vec<Vec<(String, Option<Vec<u8>>)>> {
    // `committed` entries are read once they are flushed.
    store.sync().unwrap();
    store
        .entries_after(mark)
        .iter()
        .map(|entry| {
            entry
                .versions()
                .map(|v| (v.path.as_str().to_owned(), v.bytes.map(<[u8]>::to_vec)))
                .filter(|(path, _)| path.starts_with("state/"))
                .collect::<Vec<_>>()
        })
        .filter(|batch| !batch.is_empty())
        .collect()
}

fn apply_batch(store: &OfficeStore, op: u64, files: Vec<(String, Option<Vec<u8>>)>, rehome: bool) {
    let reply = store.run(ApplyReplicatedFilesOp {
        operation_id: id(op),
        actor: Actor::System,
        command: crate::replication::state_files::ApplyReplicatedFiles {
            files,
            rehome_tasks: rehome,
        },
    });
    assert_eq!(reply.expect("applied").outcome, Outcome::Completed);
}

fn move_b_under_d(store: &OfficeStore) {
    let moved = store.run(MoveTeamOp {
        operation_id: id(0x60),
        here: identity(),
        owner: id(CREATOR),
        by: Actor::Employee(id(CREATOR)),
        command: MoveTeam {
            id: id(TEAM_B),
            expected_revision: 1,
            parent: id(TEAM_D),
        },
    });
    assert_eq!(moved.unwrap().outcome, Outcome::Completed);
}

#[test]
fn a_satellite_applying_a_move_keeps_its_own_newer_files_with_the_moved_task() {
    for engine in ENGINES {
        // Two offices in the same state: HQ and a satellite that has seen all of it.
        let mut hq = Bench::new(engine);
        let mut sat = Bench::new(engine);
        let task = hq.create(Some(TEAM_C), None);
        assert_eq!(sat.create(Some(TEAM_C), None).id, task.id);
        hq.child_changes(&task);
        sat.child_changes(&task);
        // A comment only the satellite has yet: its task is newer there than at HQ.
        let extra = sat.child_changes(&task);
        let local = sat.task(&task.id).unwrap().meta.revision;
        assert!(local > hq.task(&task.id).unwrap().meta.revision);

        let mark = keep_entries_from_here(hq.store());
        move_b_under_d(hq.store());
        let batches = batches_after(hq.store(), mark);
        assert_eq!(batches.len(), 1, "{engine:?}: the move is one entry");
        for batch in batches {
            apply_batch(sat.store(), 0x61, batch, false);
        }
        sat.store().sync().unwrap();

        let old = format!(
            "{}/work/tasks/{}",
            team_folder(&[TOP_TEAM, TEAM_B, TEAM_C]),
            task.id
        );
        let new = format!(
            "{}/work/tasks/{}",
            team_folder(&[TOP_TEAM, TEAM_D, TEAM_B, TEAM_C]),
            task.id
        );
        assert!(tree(&sat.durable(&old)).is_empty(), "{engine:?}");
        let files = tree(&sat.durable(&new));
        let comments = files.keys().filter(|n| n.starts_with("comments/")).count();
        assert_eq!(comments, 2, "{engine:?}: {:?}", files.keys());
        assert!(files.contains_key("task.md") && files.contains_key("checklist.yaml"));
        let moved = sat.task(&task.id).unwrap();
        assert_eq!(
            moved.meta.revision, local,
            "{engine:?}: the newer local task stays"
        );
        assert!(sat.ack(&extra[0]).is_some());

        let dump = sat.store().dump();
        sat.rebuild();
        assert_eq!(sat.store().dump(), dump, "{engine:?}: rebuild parity");
    }
}

#[test]
fn hq_writes_a_satellite_change_made_before_a_move_into_the_moved_folder() {
    for engine in ENGINES {
        let mut hq = Bench::new(engine);
        let mut sat = Bench::new(engine);
        let task = hq.create(Some(TEAM_B), None);
        assert_eq!(sat.create(Some(TEAM_B), None).id, task.id);
        move_b_under_d(hq.store());

        // The satellite has not seen the move: its comment is in the old folder.
        let mark = keep_entries_from_here(sat.store());
        sat.child_changes(&task);
        let work = batches_after(sat.store(), mark);
        let old = format!(
            "{}/work/tasks/{}",
            team_folder(&[TOP_TEAM, TEAM_B]),
            task.id
        );
        assert!(
            work.iter().flatten().any(|(p, _)| p.starts_with(&old)),
            "{engine:?}"
        );
        for (n, batch) in work.into_iter().enumerate() {
            apply_batch(hq.store(), 0x62 + n as u64, batch, true);
        }
        hq.store().sync().unwrap();
        let new = format!(
            "{}/work/tasks/{}",
            team_folder(&[TOP_TEAM, TEAM_D, TEAM_B]),
            task.id
        );
        assert!(tree(&hq.durable(&old)).is_empty(), "{engine:?}");
        let files = tree(&hq.durable(&new));
        let names: Vec<&str> = files.keys().map(String::as_str).collect();
        assert_eq!(names.len(), 4, "{engine:?}: {names:?}");
        for name in ["task.md", "checklist.yaml", "operations.yaml"] {
            assert!(names.contains(&name), "{engine:?}: {name}");
        }
        assert_eq!(
            hq.task(&task.id).unwrap().meta.revision,
            sat.task(&task.id).unwrap().meta.revision
        );
        let dump = hq.store().dump();
        hq.rebuild();
        assert_eq!(hq.store().dump(), dump, "{engine:?}: rebuild parity");
    }
}
