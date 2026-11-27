//! `TASK-224`: a paged snapshot carrying HQ's `MoveTeam` to a satellite that holds the
//! organization from before the move, against a real HQ and an enrolled satellite, applied in
//! one commit and in several batches (the org-core in one batch, then in the oversized
//! cycle-breaking passes).
//!
//! - the snapshot applies: the moved team, the sub-team below it and the profile homed in it
//!   stand at their new paths, and no batch is refused for a missing team or employee;
//! - nothing is left at the old paths: no `team.yaml`, profile or task file;
//! - every HQ task file is at its new path with HQ's bytes, and the satellite's own task made
//!   offline under the moved subtree (still in its outbox) moves with it, intact;
//! - the satellite's projection deleted and rebuilt from its files, on the memory engine and
//!   on SQLite, is the one the snapshot left.

use std::collections::BTreeMap;
use std::sync::Arc;

use org_office::rules::OrgCommand;
use org_office::tasks::ports::TasksRead;
use org_office::{open_store, EngineChoice, Office, ReadOp, SnapshotPathClass, StoreConfig};
use org_proto::org::{MemberAddPersonParams, TeamCreateParams, TeamMoveParams, TeamSetLeadParams};
use org_proto::tasks::{TasksCommentParams, TasksCreateParams};
use org_store::testkit::FakeCustody;
use org_store::{DurablePath, Files, Id, StoreError, SystemClock, WriterAuthority};

use super::a2a_messaging::sat_custody;
use super::org_event_bursts::{fixture, Fixture};
use crate::federation::SatelliteSyncSession;
use crate::snapshot_transfer::sync_initial_snapshot_in_batches;

/// HQ tasks in each of the moved team and the sub-team below it.
const TASKS_PER_TEAM: usize = 12;

/// The teams a move involves: B and C below it are moved under D; the top team stays.
struct Teams {
    top: String,
    b: String,
    c: String,
    d: String,
}

impl Teams {
    fn folder(&self, chain: &[&String]) -> String {
        let mut folder = String::from("state");
        for team in chain {
            folder.push_str("/teams/");
            folder.push_str(team);
        }
        folder
    }

    fn old_b(&self) -> String {
        self.folder(&[&self.top, &self.b])
    }

    fn old_c(&self) -> String {
        self.folder(&[&self.top, &self.b, &self.c])
    }

    fn new_b(&self) -> String {
        self.folder(&[&self.top, &self.d, &self.b])
    }

    fn new_c(&self) -> String {
        self.folder(&[&self.top, &self.d, &self.b, &self.c])
    }
}

fn org(office: &Office, owner: &str, command: OrgCommand) {
    office
        .submit_org(owner, command)
        .unwrap()
        .unwrap_or_else(|refusal| panic!("refused: {refusal:?}"));
}

fn create_team(office: &Office, owner: &str, name: &str, parent: &str) -> String {
    let team_id = office.next_uuid_v7();
    let params = TeamCreateParams {
        command_id: office.next_uuid_v7(),
        team_id: team_id.clone(),
        name: name.into(),
        owner_team_id: Some(parent.into()),
    };
    org(office, owner, OrgCommand::TeamCreate(params));
    team_id
}

fn team_version(office: &Office, team: &str) -> u64 {
    let teams = office.org_snapshot().unwrap().teams;
    teams.into_iter().find(|t| t.id == team).unwrap().version
}

/// A task in `team` with a body large enough that the tasks outweigh the organization core.
fn create_task(office: &Office, actor: &str, team: &str, title: &str) -> String {
    let task_id = office.next_uuid_v7();
    let params: TasksCreateParams = serde_json::from_value(serde_json::json!({
        "command_id": office.next_uuid_v7(), "task_id": task_id, "team_id": team,
        "title": title, "body_markdown": "A moved team's task. ".repeat(60),
    }))
    .unwrap();
    office.task_create(actor, params).unwrap().unwrap();
    task_id
}

fn comment(office: &Office, actor: &str, task: &str) {
    let params: TasksCommentParams = serde_json::from_value(serde_json::json!({
        "command_id": office.next_uuid_v7(), "task": task,
        "comment_id": office.next_uuid_v7(), "body_markdown": "Before the move.",
    }))
    .unwrap();
    office.task_comment(actor, params).unwrap().unwrap();
}

/// Every file `office` holds under `folder`, with its bytes.
fn files_under(office: &Office, folder: &str) -> BTreeMap<String, Vec<u8>> {
    let files = Files::open(office.durable_root()).unwrap();
    let mut out = BTreeMap::new();
    for path in files.walk(&DurablePath::parse(folder).unwrap()).unwrap() {
        let bytes = files.read(&path).unwrap().expect("a walked file");
        out.insert(path.as_str().to_owned(), bytes.to_vec());
    }
    out
}

/// Whether `office` holds a file at `path`.
fn holds(office: &Office, path: &str) -> bool {
    let files = Files::open(office.durable_root()).unwrap();
    files
        .read(&DurablePath::parse(path).unwrap())
        .unwrap()
        .is_some()
}

/// What a projection says of the organization's structure and the tasks: each team's
/// parent, each employee's home team, and each task's team and effective revision.
#[derive(Debug, PartialEq, Eq)]
struct Structure {
    teams: BTreeMap<String, Option<String>>,
    homes: BTreeMap<String, String>,
    tasks: BTreeMap<String, (String, u64)>,
}

struct ReadStructure(Vec<String>);

impl ReadOp for ReadStructure {
    type Out = Structure;

    fn read<S: TasksRead + org_office::attachments::ports::AttachmentsRead>(
        self,
        snapshot: &S,
    ) -> Result<Structure, StoreError> {
        let teams = snapshot.teams()?.into_iter();
        let homes = snapshot.employees()?.into_iter();
        let mut tasks = BTreeMap::new();
        for id in self.0 {
            let task = snapshot.task(&Id::parse(&id).unwrap())?.expect("a task");
            tasks.insert(id, (task.team_id.to_string(), task.meta.revision));
        }
        Ok(Structure {
            teams: teams
                .map(|t| (t.id.to_string(), t.parent_team_id.map(|p| p.to_string())))
                .collect(),
            homes: homes
                .map(|e| (e.id.to_string(), e.home_team_id.to_string()))
                .collect(),
            tasks,
        })
    }
}

/// What the live office says of the same.
fn live_structure(office: &Office, tasks: &[String]) -> Structure {
    let teams = office.org_snapshot().unwrap().teams.into_iter();
    let homes = office.list_employees().unwrap().into_iter();
    let tasks = tasks.iter().map(|id| {
        let task = office.task_inspect(id).unwrap().unwrap().task;
        (id.clone(), (task.team_id, task.revision))
    });
    Structure {
        teams: teams.map(|t| (t.id, t.owner_team_id)).collect(),
        homes: homes
            .map(|e| (e.id.to_string(), e.home_team_id.to_string()))
            .collect(),
        tasks: tasks.collect(),
    }
}

/// The bytes HQ's snapshot holds of the organization core and in all, as staging counts them.
fn snapshot_bytes(hq: &Office) -> (usize, usize) {
    let (mut core, mut total) = (0usize, 0usize);
    for (path, bytes) in hq.collect_authoritative_state_snapshot().unwrap() {
        let entry = path.len() + bytes.len() + 64;
        total += entry;
        let class = org_office::classify_replicated_snapshot_path(&path);
        if matches!(
            class,
            SnapshotPathClass::OrgCoreStructure | SnapshotPathClass::OrgCoreEmployee
        ) {
            core += entry;
        }
    }
    (core, total)
}

/// How the test sizes the satellite's native batches against HQ's snapshot.
#[derive(Clone, Copy, Debug)]
enum Batches {
    /// The whole snapshot in one commit.
    One,
    /// The organization core in one batch, the rest in several.
    CoreWhole,
    /// The organization core over the limit: the cycle-breaking passes, in several batches.
    CoreSplit,
}

/// HQ with teams B and C below it under the top team, D beside B, a person homed in B who
/// leads it, and tasks with comments in B and C; the satellite holds all of it, and a task of
/// its own made offline in C, still in its outbox. Returns the teams, the person, the HQ
/// tasks and the offline task.
fn before_the_move(
    f: &Fixture,
    session: &mut SatelliteSyncSession,
) -> (Teams, String, Vec<String>, String) {
    let (hq, owner) = (&f.hq, f.owner.as_str());
    let top = f.hq_team.clone();
    let b = create_team(hq, owner, "Moved", &top);
    let c = create_team(hq, owner, "Below moved", &b);
    let d = create_team(hq, owner, "Destination", &top);
    let teams = Teams { top, b, c, d };
    let person = hq.next_uuid_v7();
    let add = MemberAddPersonParams {
        command_id: hq.next_uuid_v7(),
        member_id: person.clone(),
        name: "Moved Lead".into(),
        handle: Some("moved-lead".into()),
        home_team_id: Some(teams.b.clone()),
        contact: None,
    };
    org(hq, owner, OrgCommand::MemberAddPerson(add));
    let lead = TeamSetLeadParams {
        command_id: hq.next_uuid_v7(),
        team_id: teams.b.clone(),
        expected_version: team_version(hq, &teams.b),
        lead_member_id: Some(person.clone()),
    };
    org(hq, owner, OrgCommand::TeamSetLead(lead));
    let mut tasks = Vec::new();
    for team in [&teams.b, &teams.c] {
        for n in 0..TASKS_PER_TEAM {
            let task = create_task(hq, owner, team, &format!("Task {n}"));
            if n % 3 == 0 {
                comment(hq, owner, &task);
            }
            tasks.push(task);
        }
    }
    hq.sync_store().unwrap();
    session.pull_org_events().unwrap();
    let sat = &f.sat;
    assert!(holds(sat, &format!("{}/team.yaml", teams.old_b())));
    assert!(holds(sat, &format!("{}/team.yaml", teams.old_c())));
    for task in &tasks {
        sat.task_inspect(task)
            .unwrap()
            .expect("the satellite holds HQ's task");
    }
    let offline = create_task(sat, owner, &teams.c, "Made offline");
    comment(sat, owner, &offline);
    sat.sync_store().unwrap();
    sat.sync_satellite_work_outbox().unwrap();
    (teams, person, tasks, offline)
}

fn move_b_under_d(hq: &Office, owner: &str, teams: &Teams) {
    let params = TeamMoveParams {
        command_id: hq.next_uuid_v7(),
        team_id: teams.b.clone(),
        expected_version: team_version(hq, &teams.b),
        owner_team_id: Some(teams.d.clone()),
    };
    org(hq, owner, OrgCommand::TeamMove(params));
    hq.sync_store().unwrap();
}

fn batch_limit(hq: &Office, batches: Batches) -> usize {
    let (core, total) = snapshot_bytes(hq);
    assert!(
        total > 2 * core + 8 * 1024,
        "the tasks outweigh the organization core: {core} of {total} bytes"
    );
    match batches {
        Batches::One => total + 64 * 1024,
        Batches::CoreWhole => core + 4 * 1024,
        Batches::CoreSplit => core / 4,
    }
}

fn assert_moved(f: &Fixture, teams: &Teams, person: &str, tasks: &[String], offline: &str) {
    let (hq, sat) = (&f.hq, &f.sat);
    let label = sat.info().office_name.clone();
    // Nothing is left at the old paths: no team.yaml, profile or task file.
    let left = files_under(sat, &teams.old_b());
    assert!(left.is_empty(), "{label}: left behind: {:?}", left.keys());
    for folder in [teams.new_b(), teams.new_c()] {
        assert!(holds(sat, &format!("{folder}/team.yaml")), "{label}");
    }
    let profiles: Vec<String> = files_under(sat, &teams.new_b())
        .into_keys()
        .filter(|p| p.contains(person) && p.ends_with("/profile.md"))
        .collect();
    assert_eq!(profiles.len(), 1, "{label}: the lead's profile moved");
    // HQ's files of the moved subtree are here with HQ's bytes; the only others are the
    // offline task's, moved with its team.
    let at_hq = files_under(hq, &teams.new_b());
    let here = files_under(sat, &teams.new_b());
    for (path, bytes) in &at_hq {
        assert_eq!(here.get(path), Some(bytes), "{label}: {path}");
    }
    let extra: Vec<&String> = here.keys().filter(|p| !at_hq.contains_key(*p)).collect();
    assert!(!extra.is_empty(), "{label}: the offline task moved");
    assert!(
        extra.iter().all(|p| p.contains(offline)),
        "{label}: {extra:?}"
    );
    let offline_md = format!("{}/work/tasks/{offline}/task.md", teams.new_c());
    assert!(holds(sat, &offline_md), "{label}: {offline_md}");

    let snapshot = sat.org_snapshot().unwrap();
    let parent = |team: &String| {
        let found = snapshot.teams.iter().find(|t| t.id == *team).unwrap();
        found.owner_team_id.clone()
    };
    assert_eq!(parent(&teams.b), Some(teams.d.clone()), "{label}");
    assert_eq!(parent(&teams.c), Some(teams.b.clone()), "{label}");
    let lead = snapshot.teams.iter().find(|t| t.id == teams.b).unwrap();
    assert_eq!(lead.lead_member_id.as_deref(), Some(person), "{label}");
    for task in tasks {
        let (theirs, ours) = (hq.task_inspect(task), sat.task_inspect(task));
        let (theirs, ours) = (theirs.unwrap().unwrap(), ours.unwrap().unwrap());
        assert_eq!(ours.task.revision, theirs.task.revision, "{label}: {task}");
        assert_eq!(
            ours.comments.len(),
            theirs.comments.len(),
            "{label}: {task}"
        );
    }
    let mine = sat.task_inspect(offline).unwrap().unwrap();
    assert_eq!(
        mine.comments.len(),
        1,
        "{label}: the offline task keeps its comment"
    );
}

/// Closes the satellite, then deletes its projection and rebuilds it from its files on each
/// engine: each must accept every file and answer what the live projection did.
fn assert_rebuilds(f: Fixture, suffix: u8, live: &Structure) {
    let org_id = f.hq.info().org_id.clone();
    let sat_id = f.sat.info().office_id.clone();
    let label = f.sat.info().office_name.clone();
    let durable = f.sat.durable_root().to_path_buf();
    let sat = Arc::try_unwrap(f.sat).unwrap_or_else(|_| panic!("the satellite is still shared"));
    sat.close().unwrap();
    f.server.stop();
    let tasks: Vec<String> = live.tasks.keys().cloned().collect();
    for engine in [EngineChoice::Memory, EngineChoice::Sqlite] {
        let runtime = tempfile::tempdir().unwrap();
        let config = StoreConfig::new(
            engine,
            durable.clone(),
            runtime.path().canonicalize().unwrap(),
            sat_custody(&org_id, &sat_id, suffix).identity,
            Arc::new(FakeCustody::new()) as Arc<dyn WriterAuthority>,
            Arc::new(SystemClock),
        );
        let store = open_store(&config).unwrap_or_else(|e| panic!("{label} {engine:?}: {e}"));
        assert!(store.status().stopped.is_none(), "{label} {engine:?}");
        let (rebuilt, _) = store.read(ReadStructure(tasks.clone())).unwrap();
        assert_eq!(&rebuilt, live, "{label} {engine:?}: rebuild parity");
        store.close().unwrap();
    }
}

fn a_moved_team_reaches_a_satellite_by_snapshot(batches: Batches, suffix: u8) {
    let f = fixture(&format!("Team Move {batches:?}"), suffix);
    let mut session = SatelliteSyncSession::connect(Arc::clone(&f.sat)).unwrap();
    let (teams, person, tasks, offline) = before_the_move(&f, &mut session);
    move_b_under_d(&f.hq, &f.owner, &teams);
    let limit = batch_limit(&f.hq, batches);

    let op = f.sat.next_uuid_v7();
    sync_initial_snapshot_in_batches(&mut session, &op, limit)
        .unwrap_or_else(|e| panic!("{batches:?}: the snapshot applies: {e}"));
    f.sat.sync_store().unwrap();
    assert_moved(&f, &teams, &person, &tasks, &offline);

    // The offline task (its create and its comment) then reaches HQ, in the folder HQ's own
    // chain names for it.
    assert_eq!(session.flush_work_outbox().unwrap(), 2, "{batches:?}");
    f.hq.sync_store().unwrap();
    let at_hq = format!("{}/work/tasks/{offline}/task.md", teams.new_c());
    assert!(holds(&f.hq, &at_hq), "{batches:?}: {at_hq}");

    let mut all = tasks;
    all.push(offline);
    let live = live_structure(&f.sat, &all);
    drop(session);
    assert_rebuilds(f, suffix, &live);
}

#[test]
fn a_moved_team_applies_from_a_one_commit_snapshot() {
    a_moved_team_reaches_a_satellite_by_snapshot(Batches::One, 71);
}

#[test]
fn a_moved_team_applies_from_a_snapshot_in_batches() {
    a_moved_team_reaches_a_satellite_by_snapshot(Batches::CoreWhole, 72);
}

#[test]
fn a_moved_team_applies_from_a_snapshot_with_an_oversized_organization_core() {
    a_moved_team_reaches_a_satellite_by_snapshot(Batches::CoreSplit, 73);
}
