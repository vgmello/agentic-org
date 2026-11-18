use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use org_store::{
    testkit::{id, identity, FakeCustody, FamilyScript, Faults, FixedClock, MemoryEngine, Rng},
    Actor, AnchorFacts, Opening, Outcome, Reply, Store, StoreError,
};

use super::*;
use crate::organization::{
    features::CreateTeam,
    ports::RecordRef,
    records::{Employee, Organization, Team, TeamAllocation, TeamCounter},
    testkit::{creation_command, OrganizationScript, Step, CREATION_AT},
};

pub(super) const ENGINES: [EngineChoice; 2] = [EngineChoice::Sqlite, EngineChoice::Memory];

/// One office's directories, custody and disk, which outlive any one opening.
pub(super) struct Place {
    root: PathBuf,
    pub(super) custody: Arc<FakeCustody>,
    pub(super) faults: Arc<Faults>,
    _dir: tempfile::TempDir,
}

impl Place {
    pub(super) fn new() -> Self {
        let dir = tempfile::tempdir().expect("a temporary directory");
        Self {
            root: dir.path().canonicalize().expect("a real path"),
            custody: Arc::new(FakeCustody::new()),
            faults: Faults::new(),
            _dir: dir,
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

    fn read<S: OrganizationRead>(self, snapshot: &S) -> Result<Directory, StoreError> {
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

pub(super) fn create_organization(store: &OfficeStore, op: u64) -> Result<Reply, StoreError> {
    store.run(CreateOrganizationOp {
        operation_id: id(op),
        here: identity(),
        command: creation_command(),
    })
}

/// The creator of the suite's organization, and its top-level team.
pub(super) const CREATOR: u64 = 0x12;
pub(super) const TOP_TEAM: u64 = 0x11;

pub(super) fn team_command(team: u64, parent: u64) -> CreateTeam {
    CreateTeam {
        id: id(team),
        name: format!("Team {team:x}"),
        parent: id(parent),
    }
}

pub(super) fn create_team(store: &OfficeStore, op: u64, team: u64) -> Result<Reply, StoreError> {
    store.run(CreateTeamOp {
        operation_id: id(op),
        here: identity(),
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
