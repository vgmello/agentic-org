use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use org_store::{
    testkit::{id, identity, FakeCustody, Faults, FixedClock, MemoryEngine, Probe},
    Actor, AnchorFacts, Opening, Outcome, Reply, StoreError,
};

use super::office_store::memory_engine;
use super::*;
use crate::office_local::{testkit::ca_material, CaMaterial};
use crate::organization::{
    features::CreateTeam,
    ports::RecordRef,
    records::{Employee, Organization, Team, TeamAllocation, TeamCounter},
    testkit::{creation_command, CREATION_AT},
};

struct Place {
    root: PathBuf,
    custody: Arc<FakeCustody>,
    faults: Arc<Faults>,
    probe: Probe,
    _dir: Option<tempfile::TempDir>,
}

impl Place {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let mut place = Self::at(dir.path());
        place._dir = Some(dir);
        place
    }

    fn at(root: &Path) -> Self {
        Self {
            root: root.canonicalize().expect("a real path"),
            custody: Arc::new(FakeCustody::new()),
            faults: Faults::new(),
            probe: Probe::new(&identity().office_id),
            _dir: None,
        }
    }

    fn durable(&self) -> PathBuf {
        self.root.join("durable")
    }

    fn runtime(&self) -> PathBuf {
        self.root.join("runtime")
    }

    fn config(&self, engine: EngineChoice, opening: Opening) -> StoreConfig {
        let custody = Arc::clone(&self.custody) as Arc<dyn WriterAuthority>;
        let clock = Arc::new(FixedClock(CREATION_AT));
        let (durable, runtime) = (self.durable(), self.runtime());
        let mut config = StoreConfig::new(engine, durable, runtime, identity(), custody, clock);
        config.opening = opening;
        config.testing.faults = Some(Arc::clone(&self.faults));
        config.testing.probe = Some(self.probe.clone());
        config.flush_interval = std::time::Duration::from_secs(3_600);
        config
    }

    fn create(&self, engine: EngineChoice) -> OfficeStore {
        let config = self.config(engine, Opening::Create(AnchorFacts::default()));
        open_store(&config).expect("a new store")
    }

    fn reopen(&self, engine: EngineChoice) -> OfficeStore {
        open_store(&self.config(engine, Opening::Existing)).expect("the store again")
    }
}

#[derive(PartialEq, Debug)]
struct Directory {
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

struct ReadDirectory;

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

fn directory(store: &OfficeStore) -> Directory {
    store.read(ReadDirectory).expect("a read").0
}

const CA: u8 = 1;
const CREATOR: u64 = 0x12;
const TOP_TEAM: u64 = 0x11;

fn create_organization(store: &OfficeStore, op: u64) -> Result<Reply, StoreError> {
    create_organization_with(store, op, ca_material(&identity().organization_id, CA))
}

fn create_organization_with(
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

fn create_team(store: &OfficeStore, op: u64, team: u64) -> Result<Reply, StoreError> {
    store.run(CreateTeamOp {
        operation_id: id(op),
        here: identity(),
        owner: id(CREATOR),
        by: Actor::Employee(id(CREATOR)),
        command: CreateTeam {
            id: id(team),
            name: format!("Team {team:x}"),
            parent: Some(id(TOP_TEAM)),
        },
    })
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
fn sqlite_engine_fails_closed_when_no_backend_is_installed() {
    let place = Place::new();
    let config = place.config(
        EngineChoice::Sqlite,
        Opening::Create(AnchorFacts::default()),
    );
    let refused = open_store(&config);
    assert!(
        matches!(refused, Err(StoreError::Invalid(_))),
        "expected fail-closed StoreError::Invalid when no SQLite backend is installed"
    );
}

#[test]
fn memory_engine_creates_reads_retries_and_reopens_through_the_one_seam() {
    let place = Place::new();
    let store = place.create(EngineChoice::Memory);
    assert_eq!(store.read(DirectoryGeneration).unwrap().0, 0);
    let created = create_organization(&store, 1).unwrap();
    assert_eq!(created.outcome, Outcome::Completed);
    let team = create_team(&store, 2, 0x21).unwrap();
    assert_eq!(team.outcome, Outcome::Completed);
    assert_eq!(create_team(&store, 2, 0x21).as_ref(), Ok(&team));
    assert_eq!(store.read(DirectoryGeneration).unwrap().0, 2);
    assert_eq!(store.status().stopped, None);
    let before = directory(&store);
    assert_eq!(before.teams.len(), 2);
    assert_eq!(before.counter.as_ref().map(|c| c.last_number), Some(2));
    store.close().unwrap();

    let store = place.reopen(EngineChoice::Memory);
    assert_eq!(directory(&store), before);
    assert_eq!(create_team(&store, 2, 0x21), Ok(team));
    store.close().unwrap();
    let again = place.config(
        EngineChoice::Memory,
        Opening::Create(AnchorFacts::default()),
    );
    assert!(open_store(&again).is_err());
}
