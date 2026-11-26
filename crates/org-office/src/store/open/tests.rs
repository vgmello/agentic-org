use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use org_store::{
    testkit::{
        id, identity, FakeCustody, FamilyScript, Faults, FixedClock, MemoryEngine, Probe, Rng,
    },
    Actor, AnchorFacts, Id, Opening, Outcome, Reply, Store, StoreError,
};

use super::office_store::{memory_engine, sqlite_engine};
use super::*;
use crate::office_local::{testkit::ca_material, CaMaterial};
use crate::organization::{
    features::CreateTeam,
    ports::RecordRef,
    records::{Employee, Organization, Team, TeamAllocation, TeamCounter},
    testkit::{creation_command, OrganizationScript, Step, CREATION_AT},
};
use crate::store::sqlite::SqliteEngine;

mod ack;
mod companions;
mod crashes;
mod local;
mod messaging;
mod tasks;

pub(super) const ENGINES: [EngineChoice; 2] = [EngineChoice::Sqlite, EngineChoice::Memory];

/// One office's directories, custody and disk, which outlive any one opening.
pub(super) struct Place {
    root: PathBuf,
    pub(super) custody: Arc<FakeCustody>,
    pub(super) faults: Arc<Faults>,
    pub(super) probe: Probe,
    _dir: Option<tempfile::TempDir>,
}

impl Place {
    pub(super) fn new() -> Self {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let mut place = Self::at(dir.path());
        place._dir = Some(dir);
        place
    }

    /// The office under `root`, as a new process finds it: nothing kept in memory.
    pub(super) fn at(root: &Path) -> Self {
        Self {
            root: root.canonicalize().expect("a real path"),
            custody: Arc::new(FakeCustody::new()),
            faults: Faults::new(),
            probe: Probe::new(&identity().office_id),
            _dir: None,
        }
    }

    pub(super) fn durable(&self) -> PathBuf {
        self.root.join("durable")
    }

    pub(super) fn runtime(&self) -> PathBuf {
        self.root.join("runtime")
    }

    pub(super) fn config(&self, engine: EngineChoice, opening: Opening) -> StoreConfig {
        let custody = Arc::clone(&self.custody) as Arc<dyn WriterAuthority>;
        let clock = Arc::new(FixedClock(CREATION_AT));
        let (durable, runtime) = (self.durable(), self.runtime());
        let mut config = StoreConfig::new(engine, durable, runtime, identity(), custody, clock);
        config.opening = opening;
        // The disk every opening of this office is on: it fails and dies when told to.
        config.testing.faults = Some(Arc::clone(&self.faults));
        config.testing.probe = Some(self.probe.clone());
        // Flushes come from durable requests, back-pressure, syncing and closing only.
        config.flush_interval = std::time::Duration::from_secs(3_600);
        config
    }

    pub(super) fn create(&self, engine: EngineChoice) -> OfficeStore {
        let config = self.config(engine, Opening::Create(AnchorFacts::default()));
        open_store(&config).expect("a new store")
    }

    pub(super) fn reopen(&self, engine: EngineChoice) -> OfficeStore {
        open_store(&self.config(engine, Opening::Existing)).expect("the store again")
    }
}

/// Every file under `root`, by its path under it.
pub(super) fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut folders = vec![root.to_owned()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                folders.push(path);
                continue;
            }
            let name = path.strip_prefix(root).expect("under the root");
            let name = name.to_str().expect("a UTF-8 path").to_owned();
            files.insert(name, std::fs::read(&path).expect("a file"));
        }
    }
    files
}

/// Everything the organization family projects, as a read sees it.
#[derive(PartialEq, Debug)]
pub(super) struct Directory {
    generation: u64,
    organization: Option<Organization>,
    teams: Vec<Team>,
    employees: Vec<Employee>,
    counter: Option<TeamCounter>,
    allocations: Vec<TeamAllocation>,
    refs: Vec<RecordRef>,
    repositories: usize,
    offices: usize,
    codes: usize,
}

pub(super) struct ReadDirectory;

impl ReadOp for ReadDirectory {
    type Out = Directory;

    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Directory, StoreError> {
        Ok(Directory {
            generation: snapshot.generation()?,
            organization: snapshot.organization()?,
            teams: snapshot.teams()?,
            employees: snapshot.employees()?,
            counter: snapshot.team_counter()?,
            allocations: snapshot.team_allocations()?,
            refs: snapshot.record_refs()?,
            repositories: snapshot.repositories()?.len(),
            offices: snapshot.offices()?.len(),
            codes: snapshot.office_codes()?.len(),
        })
    }
}

pub(super) fn directory(store: &OfficeStore) -> Directory {
    store.read(ReadDirectory).expect("a read").0
}

/// The seed of the suite's CA material.
pub(super) const CA: u8 = 1;

pub(super) fn create_organization(store: &OfficeStore, op: u64) -> Result<Reply, StoreError> {
    create_organization_with(store, op, ca_material(&identity().organization_id, CA))
}

pub(super) fn create_organization_with(
    store: &OfficeStore,
    op: u64,
    ca: CaMaterial,
) -> Result<Reply, StoreError> {
    store.run(CreateOrganizationOp {
        operation_id: id(op),
        here: identity(),
        command: creation_command(),
        ca,
    })
}

/// The creator of the suite's organization, and its top-level team.
pub(super) const CREATOR: u64 = 0x12;
pub(super) const TOP_TEAM: u64 = 0x11;

pub(super) fn team_command(team: u64, parent: u64) -> CreateTeam {
    CreateTeam {
        id: id(team),
        name: format!("Team {team:x}"),
        parent: Some(id(parent)),
    }
}

pub(super) fn create_team(store: &OfficeStore, op: u64, team: u64) -> Result<Reply, StoreError> {
    store.run(CreateTeamOp {
        operation_id: id(op),
        here: identity(),
        owner: id(CREATOR),
        by: Actor::Employee(id(CREATOR)),
        command: team_command(team, TOP_TEAM),
    })
}

// The family's suite on each engine, made exactly as `open_store` makes it.
mod sqlite_suite {
    use super::*;

    fn open() -> (Arc<SqliteEngine>, Place) {
        let place = Place::new();
        let config = place.config(EngineChoice::Sqlite, Opening::Existing);
        let engine = sqlite_engine(&config).expect("a new projection");
        (Arc::new(engine), place)
    }

    crate::organization_conformance_tests!(open);
}

mod memory_suite {
    use super::*;

    fn open() -> (Arc<MemoryEngine>, ()) {
        (Arc::new(memory_engine()), ())
    }

    crate::organization_conformance_tests!(open);
}

#[test]
fn only_an_engine_that_is_built_in_is_chosen() {
    assert_eq!(EngineChoice::named("sqlite"), Ok(EngineChoice::Sqlite));
    assert_eq!(EngineChoice::named("memory"), Ok(EngineChoice::Memory));
    for name in ["postgres", "SQLite", "", "sqlite3"] {
        let refused = EngineChoice::named(name);
        assert!(matches!(refused, Err(StoreError::Invalid(_))), "{name:?}");
    }
}

#[test]
fn both_engines_create_read_retry_and_reopen_through_the_one_seam() {
    for engine in ENGINES {
        let place = Place::new();
        let store = place.create(engine);
        assert_eq!(store.read(DirectoryGeneration).unwrap().0, 0, "{engine:?}");
        let created = create_organization(&store, 1).unwrap();
        assert_eq!(created.outcome, Outcome::Completed, "{engine:?}");
        let team = create_team(&store, 2, 0x21).unwrap();
        assert_eq!(team.outcome, Outcome::Completed, "{engine:?}");
        // A retry answers what the first submission answered.
        assert_eq!(create_team(&store, 2, 0x21).as_ref(), Ok(&team));
        assert_eq!(store.read(DirectoryGeneration).unwrap().0, 2, "{engine:?}");
        assert_eq!(store.status().stopped, None);
        let before = directory(&store);
        assert_eq!(before.teams.len(), 2, "{engine:?}");
        assert_eq!(before.counter.as_ref().map(|c| c.last_number), Some(2));
        store.close().unwrap();

        let store = place.reopen(engine);
        assert_eq!(directory(&store), before, "{engine:?}");
        assert_eq!(create_team(&store, 2, 0x21), Ok(team), "{engine:?}");
        store.close().unwrap();
        // A second creation of the same journal is refused.
        let again = place.config(engine, Opening::Create(AnchorFacts::default()));
        assert!(open_store(&again).is_err(), "{engine:?}");
    }
}

#[test]
fn the_sqlite_projection_is_in_the_offices_runtime_folder_and_deleting_it_rebuilds_it() {
    let place = Place::new();
    let store = place.create(EngineChoice::Sqlite);
    create_organization(&store, 1).unwrap();
    create_team(&store, 2, 0x21).unwrap();
    store.sync().unwrap();
    let (dump, read) = (store.dump(), directory(&store));
    store.close().unwrap();
    let folder = place.runtime().join(identity().office_id.as_str());
    assert!(folder.join("office.db").is_file());
    let durable = tree(&place.durable());

    std::fs::remove_dir_all(&folder).unwrap();
    let store = place.reopen(EngineChoice::Sqlite);
    assert_eq!(store.dump(), dump);
    assert_eq!(directory(&store), read);
    store.close().unwrap();
    assert!(folder.join("office.db").is_file());
    // The rebuild wrote nothing durable.
    assert_eq!(tree(&place.durable()), durable);
}

/// One scripted command, on whichever engine.
struct Scripted<'a> {
    script: &'a OrganizationScript,
    step: &'a Step,
}

impl StoreOp for Scripted<'_> {
    type Out = Result<Reply, StoreError>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let command = FamilyScript::<E>::command(self.script, self.step);
        store.submit(command.op, false, command.decide)
    }
}

#[test]
fn memory_and_sqlite_agree_on_every_scripted_command() {
    for seed in 0..4u64 {
        let script = OrganizationScript::new();
        let (sqlite, memory) = (Place::new(), Place::new());
        let on_sqlite = sqlite.create(EngineChoice::Sqlite);
        let on_memory = memory.create(EngineChoice::Memory);
        let mut rng = Rng::new(seed);
        let mut steps: Vec<Step> = Vec::new();
        for n in 0..40 {
            // Now and then an earlier command again: a retry, or a conflict.
            let step = if !steps.is_empty() && rng.chance(5) {
                rng.pick(&steps).clone()
            } else {
                FamilyScript::<MemoryEngine>::generate(&script, &mut rng, n)
            };
            let (script, step) = (&script, &step);
            let a = on_sqlite.run(Scripted { script, step });
            let b = on_memory.run(Scripted { script, step });
            assert_eq!(a, b, "seed {seed}, step {n}: {step:?}");
            steps.push(step.clone());
        }
        assert_eq!(
            directory(&on_sqlite),
            directory(&on_memory),
            "seed {seed}: the reads differ"
        );
        on_sqlite.sync().unwrap();
        on_memory.sync().unwrap();
        let state = |place: &Place| tree(&place.durable().join("state"));
        assert_eq!(
            state(&sqlite),
            state(&memory),
            "seed {seed}: the files differ"
        );
    }
}

/// How long the SQLite projection's log is, in bytes.
fn log_bytes(place: &Place) -> u64 {
    let config = place.config(EngineChoice::Sqlite, Opening::Existing);
    let mut log = config.projection_path().into_os_string();
    log.push("-wal");
    std::fs::metadata(log).expect("the log").len()
}

/// A read that stays open until it is told to end.
struct HeldRead {
    began: std::sync::mpsc::Sender<()>,
    end: std::sync::mpsc::Receiver<()>,
}

impl ReadOp for HeldRead {
    type Out = u64;

    fn read<S: TasksRead>(self, snapshot: &S) -> Result<u64, StoreError> {
        let generation = snapshot.generation()?;
        self.began.send(()).expect("the test is waiting");
        self.end.recv().expect("the test ends the read");
        Ok(generation)
    }
}

/// The log of a store that stays open does not grow with what is written: each round of
/// the flusher folds it into the file, and the writer starts it again from its beginning.
/// A read that stays open keeps the log from being started again only until it ends.
#[test]
fn the_sqlite_log_stops_growing_while_the_store_stays_open_and_a_long_read_only_delays_it() {
    use std::sync::mpsc::channel;

    let place = Place::new();
    let store = place.create(EngineChoice::Sqlite);
    create_organization(&store, 1).unwrap();
    store.sync().unwrap();
    // One committed write, then a round of the flusher.
    let round = |n: u64| {
        let team = create_team(&store, 0x1000 + n, 0x2000 + n).unwrap();
        assert_eq!(team.outcome, Outcome::Completed);
        store.sync().unwrap();
        log_bytes(&place)
    };
    let settled = round(0);
    let sizes: Vec<u64> = (1..12).map(round).collect();
    assert!(sizes.iter().all(|size| *size == settled), "{sizes:?}");

    // A read open across many rounds: the log cannot be started again under it.
    let (began, has_begun) = channel();
    let (end, ended) = channel();
    let held = std::thread::scope(|scope| {
        let read = scope.spawn(|| store.read(HeldRead { began, end: ended }));
        has_begun.recv().expect("the read began");
        let held = (12..24).map(round).last().expect("rounds");
        end.send(()).expect("the read is waiting");
        assert_eq!(read.join().unwrap().unwrap().0, 13);
        held
    });
    assert!(held > settled, "{held} after {settled}");
    // It ended: the next rounds fold everything in, and the log is no longer added to.
    round(24);
    let after = round(25);
    let sizes: Vec<u64> = (26..36).map(round).collect();
    assert!(sizes.iter().all(|size| *size == after), "{sizes:?}");
    assert_eq!(directory(&store).teams.len(), 37);
    store.close().unwrap();
}

/// Engine-independent B3 projects suite (TASK-36 AC #1): default owning-team creation under
/// TEAM-1, binding an existing active team, HQ `P-<n>` numbering & slug derivation/deduplication,
/// contributing teams (add/remove/revision conflict), `archive_team` active-project guard,
/// dry-run check, idempotent replay, and SQLite runtime projection rebuild after deleting `office.db`.
#[test]
fn projects_suite_passes_on_memory_and_sqlite_and_survives_rebuild() {
    use crate::organization::features::{ArchiveTeam, OrgRejection, RejectCode};
    use crate::projects::features::{
        AddProjectTeam, CreateProject, OwnerTeamChoice, ProjectCreated, RemoveProjectTeam,
    };

    struct RecordRefsRead;
    impl ReadOp for RecordRefsRead {
        type Out = Vec<RecordRef>;
        fn read<S: crate::projects::ports::ProjectsRead>(
            self,
            view: &S,
        ) -> Result<Self::Out, StoreError> {
            Ok(view.record_refs()?)
        }
    }

    let mut engine_outputs = Vec::new();
    for engine in ENGINES {
        let place = Place::new();
        let store = place.create(engine);
        let creation = creation_command();
        let owner = creation.creator.employee_id.clone();
        let team1 = creation.team_id.clone();
        let by = Actor::Employee(owner.clone());

        let created_org = store
            .run(CreateOrganizationOp {
                operation_id: id(1),
                here: identity(),
                ca: ca_material(&creation.organization_id, 1),
                command: creation,
            })
            .unwrap();
        assert_eq!(created_org.outcome, Outcome::Completed);

        // Create an extra standalone team `Design` (TEAM-2) to test existing-team & contributing-team flows.
        let design_team = id(20);
        let team_res = store
            .run(CreateTeamOp {
                operation_id: id(21),
                here: identity(),
                owner: owner.clone(),
                by: by.clone(),
                command: CreateTeam {
                    id: design_team.clone(),
                    name: "Design".into(),
                    parent: Some(team1.clone()),
                },
            })
            .unwrap();
        assert_eq!(team_res.outcome, Outcome::Completed);

        // 1. Dry-run `CreateProject` leaves zero projects and does not advance counters.
        let dry = store
            .run(ProjectCheckOp {
                here: identity(),
                owner: owner.clone(),
                by: by.clone(),
                command: CreateProject {
                    id: id(30),
                    name: "Reservations".into(),
                    slug: None,
                    owner_team: OwnerTeamChoice::CreateDefault {
                        team_id: id(31),
                        parent_team_id: None,
                    },
                    contributing_team_ids: vec![design_team.clone()],
                },
            })
            .unwrap();
        assert_eq!(dry.outcome, Outcome::Completed);
        assert!(
            store
                .read(ProjectsList {
                    include_archived: true
                })
                .unwrap()
                .0
                .is_empty(),
            "{engine:?}: dry-run must not persist projects"
        );

        // 2. Real create with default owning team (under TEAM-1) + derived slug + contributing team.
        let proj1_id = id(30);
        let proj1_team = id(31);
        let create1 = CreateProject {
            id: proj1_id.clone(),
            name: "Reservations".into(),
            slug: None,
            owner_team: OwnerTeamChoice::CreateDefault {
                team_id: proj1_team.clone(),
                parent_team_id: None,
            },
            contributing_team_ids: vec![design_team.clone()],
        };
        let reply1 = store
            .run(CreateProjectOp {
                operation_id: id(32),
                here: identity(),
                owner: owner.clone(),
                by: by.clone(),
                command: create1.clone(),
            })
            .unwrap();
        assert_eq!(reply1.outcome, Outcome::Completed);
        let parsed1: ProjectCreated = serde_json::from_slice(&reply1.bytes).unwrap();
        assert_eq!(parsed1.number, 1);
        assert_eq!(parsed1.key, "P-1");
        assert_eq!(parsed1.slug, "reservations");
        assert_eq!(parsed1.owner_team_id, proj1_team);
        assert_eq!(parsed1.created_team_number, Some(3)); // TEAM-1 (org), TEAM-2 (Design), TEAM-3 (Reservations)

        // Idempotent replay with the same operation_id returns identical bytes.
        let replay1 = store
            .run(CreateProjectOp {
                operation_id: id(32),
                here: identity(),
                owner: owner.clone(),
                by: by.clone(),
                command: create1,
            })
            .unwrap();
        assert_eq!(replay1.bytes, reply1.bytes);

        // Verify the created owning team is under TEAM-1 and named "Reservations".
        let dir = directory(&store);
        let created_team = dir.teams.iter().find(|t| t.id == proj1_team).unwrap();
        assert_eq!(created_team.name, "Reservations");
        assert_eq!(created_team.parent_team_id.as_ref(), Some(&team1));

        // 3. Create second project bound to existing team `Design` with explicit slug.
        let proj2_id = id(40);
        let reply2 = store
            .run(CreateProjectOp {
                operation_id: id(41),
                here: identity(),
                owner: owner.clone(),
                by: by.clone(),
                command: CreateProject {
                    id: proj2_id.clone(),
                    name: "Design System".into(),
                    slug: Some("design-sys".into()),
                    owner_team: OwnerTeamChoice::Existing(design_team.clone()),
                    contributing_team_ids: vec![],
                },
            })
            .unwrap();
        assert_eq!(reply2.outcome, Outcome::Completed);
        let parsed2: ProjectCreated = serde_json::from_slice(&reply2.bytes).unwrap();
        assert_eq!(parsed2.number, 2);
        assert_eq!(parsed2.key, "P-2");
        assert_eq!(parsed2.slug, "design-sys");
        assert_eq!(parsed2.owner_team_id, design_team);
        assert_eq!(parsed2.created_team_number, None);

        // 4. Archiving a team that owns or contributes to an active project is refused (`NotEmpty`).
        let archive_owner = store
            .run(ArchiveTeamOp {
                operation_id: id(50),
                here: identity(),
                owner: owner.clone(),
                by: by.clone(),
                command: ArchiveTeam {
                    id: proj1_team.clone(),
                    expected_revision: 1,
                },
            })
            .unwrap();
        assert_eq!(archive_owner.outcome, Outcome::Rejected);
        let rej: OrgRejection = serde_json::from_slice(&archive_owner.bytes).unwrap();
        assert_eq!(rej.code, RejectCode::NotEmpty);

        // 5. Add and remove a contributing team on P-2, checking revision conflicts.
        let add_contrib = store
            .run(AddProjectTeamOp {
                operation_id: id(51),
                here: identity(),
                owner: owner.clone(),
                by: by.clone(),
                command: AddProjectTeam {
                    project_id: proj2_id.clone(),
                    team_id: proj1_team.clone(),
                },
            })
            .unwrap();
        assert_eq!(add_contrib.outcome, Outcome::Completed);

        let stale_remove = store
            .run(RemoveProjectTeamOp {
                operation_id: id(52),
                here: identity(),
                owner: owner.clone(),
                by: by.clone(),
                command: RemoveProjectTeam {
                    project_id: proj2_id.clone(),
                    team_id: proj1_team.clone(),
                    expected_revision: 1,
                },
            })
            .unwrap();
        assert_eq!(stale_remove.outcome, Outcome::Rejected);
        let rej: OrgRejection = serde_json::from_slice(&stale_remove.bytes).unwrap();
        assert_eq!(rej.code, RejectCode::Conflict);

        let remove_contrib = store
            .run(RemoveProjectTeamOp {
                operation_id: id(53),
                here: identity(),
                owner: owner.clone(),
                by: by.clone(),
                command: RemoveProjectTeam {
                    project_id: proj2_id.clone(),
                    team_id: proj1_team.clone(),
                    expected_revision: 2,
                },
            })
            .unwrap();
        assert_eq!(remove_contrib.outcome, Outcome::Completed);

        // 6. Inspect by UUIDv7, `P-<n>` (case-insensitive), and canonical slug.
        for query in [proj1_id.as_str(), "P-1", "p-1", "reservations"] {
            let found = store
                .read(ProjectInspectByRef(query.into()))
                .unwrap()
                .0
                .expect("project found");
            assert_eq!(found.id, proj1_id, "{engine:?} query={query}");
            assert_eq!(found.contributing_team_ids, vec![design_team.clone()]);
        }

        // 6b. Cross-family UUID collision: creating a team or person with an existing project's
        // UUID must fail with AlreadyExists, and reserved slugs (`p-1`, `add-team`) must fail.
        let collide_team = store
            .run(CreateTeamOp {
                operation_id: id(54),
                here: identity(),
                owner: owner.clone(),
                by: by.clone(),
                command: CreateTeam {
                    id: proj1_id.clone(),
                    name: "Colliding Team".into(),
                    parent: Some(team1.clone()),
                },
            })
            .unwrap();
        assert_eq!(collide_team.outcome, Outcome::Rejected);
        let rej: OrgRejection = serde_json::from_slice(&collide_team.bytes).unwrap();
        assert_eq!(rej.code, RejectCode::AlreadyExists);

        let collide_person = store
            .run(AddPersonOp {
                operation_id: id(55),
                here: identity(),
                owner: owner.clone(),
                by: by.clone(),
                command: crate::organization::features::AddPerson::new(
                    proj1_id.clone(),
                    "colliding",
                    "Colliding Person",
                    team1.clone(),
                ),
            })
            .unwrap();
        assert_eq!(collide_person.outcome, Outcome::Rejected);
        let rej: OrgRejection = serde_json::from_slice(&collide_person.bytes).unwrap();
        assert_eq!(rej.code, RejectCode::AlreadyExists);

        for reserved in ["p-1", "add-team", "remove-team"] {
            let bad_slug = store
                .run(CreateProjectOp {
                    operation_id: id(56 + reserved.len() as u64),
                    here: identity(),
                    owner: owner.clone(),
                    by: by.clone(),
                    command: CreateProject {
                        id: id(0x299),
                        name: "Reserved Slug Test".into(),
                        slug: Some(reserved.into()),
                        owner_team: OwnerTeamChoice::Existing(design_team.clone()),
                        contributing_team_ids: vec![],
                    },
                })
                .unwrap();
            assert_eq!(bad_slug.outcome, Outcome::Rejected, "slug={reserved}");
            let rej: OrgRejection = serde_json::from_slice(&bad_slug.bytes).unwrap();
            assert_eq!(rej.code, RejectCode::InvalidName, "slug={reserved}");
        }

        // 6c. Unsorted contributing teams are stored and returned in canonical sorted order.
        let team_lo = id(0x110);
        let team_hi = id(0x111);
        for (op_n, tid, tname) in [(70, &team_lo, "Alpha"), (71, &team_hi, "Beta")] {
            let res = store
                .run(CreateTeamOp {
                    operation_id: id(op_n),
                    here: identity(),
                    owner: owner.clone(),
                    by: by.clone(),
                    command: CreateTeam {
                        id: tid.clone(),
                        name: tname.into(),
                        parent: Some(team1.clone()),
                    },
                })
                .unwrap();
            assert_eq!(res.outcome, Outcome::Completed);
        }
        let proj3_id = id(0x203);
        let create_p3 = store
            .run(CreateProjectOp {
                operation_id: id(72),
                here: identity(),
                owner: owner.clone(),
                by: by.clone(),
                command: CreateProject {
                    id: proj3_id.clone(),
                    name: "Sorted Contrib".into(),
                    slug: None,
                    owner_team: OwnerTeamChoice::Existing(design_team.clone()),
                    // Deliberately pass high-ID before low-ID to verify canonical sorting parity.
                    contributing_team_ids: vec![team_hi.clone(), team_lo.clone()],
                },
            })
            .unwrap();
        assert_eq!(create_p3.outcome, Outcome::Completed);
        let p3 = store
            .read(ProjectInspectByRef("P-3".into()))
            .unwrap()
            .0
            .unwrap();
        assert_eq!(
            p3.contributing_team_ids,
            vec![team_lo.clone(), team_hi.clone()]
        );

        let before_list = store
            .read(ProjectsList {
                include_archived: false,
            })
            .unwrap()
            .0;
        assert_eq!(before_list.len(), 3);
        let before_snap = store
            .read(DirectorySnapshot {
                owner: owner.clone(),
            })
            .unwrap()
            .0;
        assert_eq!(before_snap.projects.len(), 3);
        let before_refs = store.read(RecordRefsRead).unwrap().0;
        assert!(
            before_refs
                .iter()
                .any(|r| r.kind == "project" && r.id == proj1_id && !r.is_deleted),
            "{engine:?}: project record_ref missing: {before_refs:?}"
        );

        // 7. Close, delete SQLite projection file (`runtime/<office>/office.db`), reopen, and verify rebuild!
        store.close().unwrap();
        if engine == EngineChoice::Sqlite {
            let db_path = place.config(engine, Opening::Existing).projection_path();
            assert!(db_path.is_file());
            std::fs::remove_file(&db_path).unwrap();
        }
        let reopened = place.reopen(engine);
        let after_list = reopened
            .read(ProjectsList {
                include_archived: false,
            })
            .unwrap()
            .0;
        assert_eq!(after_list, before_list, "{engine:?}: rebuild parity");
        let after_snap = reopened.read(DirectorySnapshot { owner }).unwrap().0;
        assert_eq!(after_snap.projects, before_snap.projects);
        assert_eq!(after_snap.project_teams, before_snap.project_teams);
        let after_refs = reopened.read(RecordRefsRead).unwrap().0;
        assert_eq!(
            after_refs, before_refs,
            "{engine:?}: record_refs rebuild parity"
        );
        reopened.close().unwrap();
        engine_outputs.push((after_list, after_snap, after_refs));
    }
    assert_eq!(
        engine_outputs[0], engine_outputs[1],
        "SQLite and Memory projections must be identical across projects, snapshot, and record_refs"
    );
}
