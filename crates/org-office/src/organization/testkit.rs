//! The organization family on the memory engine: both ports over plain maps. Never part
//! of a production build.

use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet},
};

use org_store::{
    testkit::{FamilyTable, MemorySnapshot, MemoryTx},
    EngineError, Id,
};

use super::{
    ports::{OrganizationRead, OrganizationWrite, RecordRef},
    records::{
        Employee, Office, OfficeCode, Organization, Repository, Role, Team, TeamAllocation,
        TeamCounter,
    },
};

/// The family's tables in the memory engine. It has no constraint of any kind.
#[derive(Clone, Default, Debug)]
pub struct OrganizationTables {
    organization: Option<Organization>,
    repositories: Vec<Repository>,
    offices: BTreeMap<Id, Office>,
    office_codes: Vec<OfficeCode>,
    roles: BTreeMap<Id, Role>,
    teams: BTreeMap<Id, Team>,
    employees: BTreeMap<Id, Employee>,
    team_counter: Option<TeamCounter>,
    team_allocations: BTreeMap<u64, TeamAllocation>,
    record_refs: BTreeSet<RecordRef>,
}

fn record_ref(kind: &str, id: &Id) -> RecordRef {
    RecordRef {
        kind: kind.to_owned(),
        id: id.clone(),
        is_deleted: false,
    }
}

impl FamilyTable for OrganizationTables {
    fn name(&self) -> &'static str {
        "organization"
    }

    fn clone_box(&self) -> Box<dyn FamilyTable> {
        Box::new(self.clone())
    }

    fn dump(&self) -> Vec<String> {
        fn rows<'a, T: std::fmt::Debug + 'a>(
            table: &'a str,
            rows: impl IntoIterator<Item = &'a T> + 'a,
        ) -> impl Iterator<Item = String> + 'a {
            let rows = rows.into_iter().enumerate();
            rows.map(move |(i, row)| format!("{table} {i:06} {row:?}"))
        }
        let mut lines: Vec<String> = Vec::new();
        lines.extend(rows("organizations", &self.organization));
        lines.extend(rows("repositories", &self.repositories));
        lines.extend(rows("offices", self.offices.values()));
        lines.extend(rows("office_codes", &self.office_codes));
        lines.extend(rows("roles", self.roles.values()));
        lines.extend(rows("teams", self.teams.values()));
        lines.extend(rows("employees", self.employees.values()));
        lines.extend(rows("reference_counters", &self.team_counter));
        let allocations = self.team_allocations.values();
        lines.extend(rows("reference_allocations", allocations));
        lines.extend(rows("record_refs", &self.record_refs));
        lines.sort();
        lines
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// A read of the tables; a family that has written nothing reads as empty.
fn read<R: Default>(
    tables: Option<&OrganizationTables>,
    query: impl FnOnce(&OrganizationTables) -> R,
) -> Result<R, EngineError> {
    Ok(tables.map(query).unwrap_or_default())
}

macro_rules! memory_read {
    ($($view:ty),+) => {$(
        impl OrganizationRead for $view {
            fn organization(&self) -> Result<Option<Organization>, EngineError> {
                read(self.table(), |t| t.organization.clone())
            }

            fn repositories(&self) -> Result<Vec<Repository>, EngineError> {
                read(self.table(), |t| t.repositories.clone())
            }

            fn office(&self, id: &Id) -> Result<Option<Office>, EngineError> {
                read(self.table(), |t| t.offices.get(id).cloned())
            }

            fn offices(&self) -> Result<Vec<Office>, EngineError> {
                read(self.table(), |t| t.offices.values().cloned().collect())
            }

            fn office_codes(&self) -> Result<Vec<OfficeCode>, EngineError> {
                read(self.table(), |t| t.office_codes.clone())
            }

            fn role(&self, id: &Id) -> Result<Option<Role>, EngineError> {
                read(self.table(), |t| t.roles.get(id).cloned())
            }

            fn roles(&self) -> Result<Vec<Role>, EngineError> {
                read(self.table(), |t| t.roles.values().cloned().collect())
            }

            fn team(&self, id: &Id) -> Result<Option<Team>, EngineError> {
                read(self.table(), |t| t.teams.get(id).cloned())
            }

            fn team_by_number(&self, number: u64) -> Result<Option<Team>, EngineError> {
                read(self.table(), |t| {
                    t.teams.values().find(|team| team.number == number).cloned()
                })
            }

            fn teams(&self) -> Result<Vec<Team>, EngineError> {
                read(self.table(), |t| t.teams.values().cloned().collect())
            }

            fn employee(&self, id: &Id) -> Result<Option<Employee>, EngineError> {
                read(self.table(), |t| t.employees.get(id).cloned())
            }

            fn employees(&self) -> Result<Vec<Employee>, EngineError> {
                read(self.table(), |t| t.employees.values().cloned().collect())
            }

            fn handle_in_use(&self, handle: &str) -> Result<bool, EngineError> {
                read(self.table(), |t| {
                    t.employees.values().any(|employee| employee.handle == handle)
                })
            }

            fn team_counter(&self) -> Result<Option<TeamCounter>, EngineError> {
                read(self.table(), |t| t.team_counter.clone())
            }

            fn team_allocations(&self) -> Result<Vec<TeamAllocation>, EngineError> {
                read(self.table(), |t| t.team_allocations.values().cloned().collect())
            }

            fn record_refs(&self) -> Result<Vec<RecordRef>, EngineError> {
                read(self.table(), |t| t.record_refs.iter().cloned().collect())
            }
        }
    )+};
}

memory_read!(MemoryTx, MemorySnapshot);

impl OrganizationWrite for MemoryTx {
    fn put_organization(&mut self, record: &Organization) -> Result<(), EngineError> {
        let tables = self.table_mut::<OrganizationTables>()?;
        tables.record_refs.retain(|r| r.kind != "organization");
        tables
            .record_refs
            .insert(record_ref("organization", &record.id));
        tables.organization = Some(record.clone());
        Ok(())
    }

    fn remove_organization(&mut self) -> Result<(), EngineError> {
        let tables = self.table_mut::<OrganizationTables>()?;
        tables.record_refs.retain(|r| r.kind != "organization");
        tables.organization = None;
        Ok(())
    }

    fn put_repositories(&mut self, records: &[Repository]) -> Result<(), EngineError> {
        self.table_mut::<OrganizationTables>()?.repositories = records.to_vec();
        Ok(())
    }

    fn put_office(&mut self, record: &Office) -> Result<(), EngineError> {
        let tables = self.table_mut::<OrganizationTables>()?;
        tables.offices.insert(record.id.clone(), record.clone());
        Ok(())
    }

    fn remove_office(&mut self, id: &Id) -> Result<(), EngineError> {
        self.table_mut::<OrganizationTables>()?.offices.remove(id);
        Ok(())
    }

    fn put_office_codes(&mut self, records: &[OfficeCode]) -> Result<(), EngineError> {
        self.table_mut::<OrganizationTables>()?.office_codes = records.to_vec();
        Ok(())
    }

    fn put_role(&mut self, record: &Role) -> Result<(), EngineError> {
        let tables = self.table_mut::<OrganizationTables>()?;
        tables.roles.insert(record.id.clone(), record.clone());
        Ok(())
    }

    fn remove_role(&mut self, id: &Id) -> Result<(), EngineError> {
        self.table_mut::<OrganizationTables>()?.roles.remove(id);
        Ok(())
    }

    fn put_team(&mut self, record: &Team) -> Result<(), EngineError> {
        let tables = self.table_mut::<OrganizationTables>()?;
        tables.record_refs.insert(record_ref("team", &record.id));
        tables.teams.insert(record.id.clone(), record.clone());
        Ok(())
    }

    fn remove_team(&mut self, id: &Id) -> Result<(), EngineError> {
        let tables = self.table_mut::<OrganizationTables>()?;
        tables.record_refs.remove(&record_ref("team", id));
        tables.teams.remove(id);
        Ok(())
    }

    fn put_employee(&mut self, record: &Employee) -> Result<(), EngineError> {
        let tables = self.table_mut::<OrganizationTables>()?;
        tables.employees.insert(record.id.clone(), record.clone());
        Ok(())
    }

    fn remove_employee(&mut self, id: &Id) -> Result<(), EngineError> {
        self.table_mut::<OrganizationTables>()?.employees.remove(id);
        Ok(())
    }

    fn put_team_counter(&mut self, record: &TeamCounter) -> Result<(), EngineError> {
        self.table_mut::<OrganizationTables>()?.team_counter = Some(record.clone());
        Ok(())
    }

    fn remove_team_counter(&mut self) -> Result<(), EngineError> {
        self.table_mut::<OrganizationTables>()?.team_counter = None;
        Ok(())
    }

    fn put_team_allocation(&mut self, record: &TeamAllocation) -> Result<(), EngineError> {
        let tables = self.table_mut::<OrganizationTables>()?;
        tables
            .team_allocations
            .insert(record.number, record.clone());
        Ok(())
    }

    fn remove_team_allocation(&mut self, number: u64) -> Result<(), EngineError> {
        let tables = self.table_mut::<OrganizationTables>()?;
        tables.team_allocations.remove(&number);
        Ok(())
    }
}

// ---- the conformance suite: one office of one organization, on any engine ----

pub use suite::*;

mod suite {
    use std::{
        collections::BTreeMap,
        path::{Path, PathBuf},
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
    };

    use org_store::{
        testkit::{
            id, identity, rebuild_equivalence, Command, Dump, FakeCustody, FamilyScript,
            FixedClock, Rng,
        },
        Actor, AnchorFacts, Change, CoreRead, Engine, EngineError, Id, OfficeIdentity, Opening,
        Operation, Outcome, Projector, Refusal, Reply, Sha256, Store, StoreError, StoreParts,
        Stream, Timestamp, UnitOfWork, WriterAuthority,
    };

    use crate::organization::{
        features::{
            create_organization, ArchiveTeam, Changed, CreateOrganization, CreateTeam, Creator,
            Feature, MoveTeam, Op, OrgRejection, OrganizationCreated, RejectCode, TeamCreated,
            NOT_AUTHORIZED,
        },
        ports::{OrganizationEngine, OrganizationRead},
        projector::OrganizationFiles,
        records::{self, EmployeeKind, EmployeeStatus, OfficeRole, TeamCounter, TeamStatus},
    };

    /// The instant of every suite operation, and of the golden creation files.
    pub const CREATION_AT: Timestamp = Timestamp(1_700_000_000_000);

    const REPOSITORY: u64 = 0x10;
    const TEAM: u64 = 0x11;
    const CREATOR: u64 = 0x12;
    const HR: u64 = 0x13;

    /// The creation the golden files are the result of: the organization and office of
    /// `org_store::testkit::identity`, fixed IDs for the rest, at [`CREATION_AT`].
    pub fn creation_command() -> CreateOrganization {
        let here = identity();
        CreateOrganization {
            organization_id: here.organization_id,
            office_id: here.office_id,
            repository_id: id(REPOSITORY),
            team_id: id(TEAM),
            creator: Creator {
                employee_id: id(CREATOR),
                name: "Ana".into(),
                handle: "ana".into(),
            },
            hr_employee_id: id(HR),
            name: "Example".into(),
            slug: "example".into(),
            repository_remote_url: "https://example.test/org.git".into(),
            state_branch: "org/state".into(),
            office_name: "London HQ".into(),
        }
    }

    /// The golden creation files: the exact bytes [`creation_command`] leaves under
    /// `durable/`, by path. Every way of creating an organization is tested against them.
    pub fn creation_files() -> BTreeMap<String, Vec<u8>> {
        macro_rules! golden {
            ($($path:literal),+ $(,)?) => {
                [$((
                    concat!("state/", $path).to_owned(),
                    include_bytes!(concat!("testdata/creation/", $path)).to_vec(),
                )),+]
            };
        }
        BTreeMap::from(golden![
            "organization.yaml",
            "repositories.yaml",
            "offices/01900000-0000-7000-8000-0000000000a3.yaml",
            "identity/office-codes.yaml",
            "identity/counters/organization/01900000-0000-7000-8000-0000000000a2/team.yaml",
            "identity/allocations/organization/01900000-0000-7000-8000-0000000000a2/team/1.yaml",
            "teams/01900000-0000-7000-8000-000000000011/team.yaml",
            "teams/01900000-0000-7000-8000-000000000011/people/01900000-0000-7000-8000-000000000012/profile.md",
            "teams/01900000-0000-7000-8000-000000000011/agents/01900000-0000-7000-8000-000000000013/profile.md",
        ])
    }

    type Decision<E> = Box<dyn FnMut(&mut UnitOfWork<'_, E>) -> Result<Change, Refusal> + Send>;

    /// Decisions as a handler composes them, counted so a test can tell a retry that was
    /// answered from its receipt from one that decided again.
    #[derive(Clone)]
    struct Decisions {
        here: OfficeIdentity,
        count: Arc<AtomicUsize>,
    }

    impl Decisions {
        fn new() -> Self {
            Self {
                here: identity(),
                count: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn count(&self) -> usize {
            self.count.load(Ordering::SeqCst)
        }

        fn counted<E: Engine>(
            &self,
            mut decide: impl FnMut(&mut UnitOfWork<'_, E>, &OfficeIdentity) -> Result<Change, Refusal>
                + Send
                + 'static,
        ) -> Decision<E> {
            let (here, count) = (self.here.clone(), Arc::clone(&self.count));
            Box::new(move |uow| {
                count.fetch_add(1, Ordering::SeqCst);
                decide(uow, &here)
            })
        }

        fn create_organization<E: OrganizationEngine>(
            &self,
            command: &CreateOrganization,
        ) -> Decision<E> {
            let command = command.clone();
            self.counted(move |uow, here| create_organization::decide(uow, here, &command))
        }

        /// `command` from `by`, decided as the office decides it: the owner is the creator.
        fn feature<E: OrganizationEngine, C: Feature>(&self, by: &Actor, command: &C) -> Decision<E> {
            let (by, command) = (by.clone(), command.clone());
            self.counted(move |uow, here| command.decide(uow, here, &id(CREATOR), &by))
        }

        /// The owner archives `team` at whatever revision it has when decided, or at 1 when
        /// it does not exist.
        fn archive_team<E: OrganizationEngine>(&self, team: &Id) -> Decision<E> {
            let team = team.clone();
            self.counted(move |uow: &mut UnitOfWork<'_, E>, here| {
                let revision = uow.view().team(&team)?.map_or(1, |t| t.meta.revision);
                let command = ArchiveTeam {
                    id: team.clone(),
                    expected_revision: revision,
                };
                command.decide(uow, here, &id(CREATOR), &Actor::Employee(id(CREATOR)))
            })
        }

        /// The owner moves `team` under `parent` at whatever revision it has when decided.
        fn move_team<E: OrganizationEngine>(&self, team: &Id, parent: &Id) -> Decision<E> {
            let (team, parent) = (team.clone(), parent.clone());
            self.counted(move |uow: &mut UnitOfWork<'_, E>, here| {
                let revision = uow.view().team(&team)?.map_or(1, |t| t.meta.revision);
                let command = MoveTeam {
                    id: team.clone(),
                    expected_revision: revision,
                    parent: parent.clone(),
                };
                command.decide(uow, here, &id(CREATOR), &Actor::Employee(id(CREATOR)))
            })
        }

        /// Any files at all, with a generation taken: what a faulty or foreign writer
        /// could leave behind for a rebuild to find.
        fn write<E: OrganizationEngine>(&self, change: Change) -> Decision<E> {
            self.counted(move |uow: &mut UnitOfWork<'_, E>, _| {
                uow.next(Stream::Organization)?;
                Ok(change.clone())
            })
        }
    }

    /// An operation identity for a decision that is no operation of the family.
    fn test_operation(n: u64, kind: &'static str, subject: &Id) -> Operation {
        Operation {
            id: id(n),
            kind,
            actor: Actor::System,
            fingerprint: Sha256::of(subject.as_str().as_bytes()),
        }
    }

    /// One office's store in a temporary directory, on the engine under test.
    struct Bench<E: Engine, G> {
        store: Store<E>,
        decisions: Decisions,
        root: PathBuf,
        custody: Arc<FakeCustody>,
        _engine: G,
        _dir: tempfile::TempDir,
    }

    fn open_store<E: OrganizationEngine>(
        root: &Path,
        custody: &Arc<FakeCustody>,
        engine: Arc<E>,
        opening: Opening,
    ) -> Result<Store<E>, StoreError> {
        let here = identity();
        let files: Arc<dyn Projector<E>> =
            Arc::new(OrganizationFiles::new(here.organization_id.clone()));
        let mut parts = StoreParts::new(
            root.to_owned(),
            here,
            Arc::clone(custody) as Arc<dyn WriterAuthority>,
            engine,
            vec![files],
            Arc::new(FixedClock(CREATION_AT)),
        );
        parts.opening = opening;
        Store::open(parts)
    }

    impl<E: OrganizationEngine, G> Bench<E, G> {
        /// A new office with an empty journal and no organization.
        fn new(open: &dyn Fn() -> (Arc<E>, G)) -> Self {
            let dir = tempfile::tempdir().expect("a temporary directory");
            let root = dir.path().canonicalize().expect("a real path");
            let root = root.join("durable");
            let custody = Arc::new(FakeCustody::new());
            let (engine, guard) = open();
            let opening = Opening::Create(AnchorFacts::default());
            let store = open_store(&root, &custody, engine, opening).expect("a new store");
            Self {
                store,
                decisions: Decisions::new(),
                root,
                custody,
                _engine: guard,
                _dir: dir,
            }
        }

        /// A new office whose organization is [`creation_command`]'s.
        fn created(open: &dyn Fn() -> (Arc<E>, G)) -> Self {
            let bench = Self::new(open);
            let reply = bench.create_organization(1, &creation_command());
            assert_eq!(reply.map(|reply| reply.outcome), Ok(Outcome::Completed));
            bench
        }

        fn create_organization(
            &self,
            op: u64,
            command: &CreateOrganization,
        ) -> Result<Reply, StoreError> {
            let decide = self.decisions.create_organization(command);
            self.store
                .submit(Some(command.operation(id(op))), false, decide)
        }

        /// `command` from `by` under operation `op`.
        fn submit<C: Feature>(&self, op: u64, by: &Actor, command: &C) -> Result<Reply, StoreError> {
            let decide = self.decisions.feature(by, command);
            let operation = command.operation(id(op), by);
            self.store.submit(Some(operation), false, decide)
        }

        /// `command` from the owner under operation `op`.
        fn owner<C: Feature>(&self, op: u64, command: &C) -> Result<Reply, StoreError> {
            self.submit(op, &Actor::Employee(id(CREATOR)), command)
        }

        /// The reply of a completed change.
        fn done<C: Feature>(&self, op: u64, command: &C) -> Changed {
            let reply = self.owner(op, command).expect("a decided change");
            assert_eq!(reply.outcome, Outcome::Completed, "{:?}", rejection(&reply));
            serde_json::from_slice(&reply.bytes).expect("a change reply")
        }

        /// The recorded rejection of a change, with its receipt.
        fn refused<C: Feature>(&self, op: u64, command: &C) -> OrgRejection {
            let generation = self.generation();
            let reply = self.owner(op, command).expect("a decided change");
            assert_eq!(reply.outcome, Outcome::Rejected);
            assert!(self.has_receipt(op));
            assert_eq!(self.generation(), generation, "a rejection takes no generation");
            rejection(&reply)
        }

        fn create_team(&self, op: u64, team: u64, parent: u64) -> Result<Reply, StoreError> {
            self.owner(op, &team_command(team, parent))
        }

        /// The team's number and generation, from a completed creation.
        fn created_team(&self, op: u64, team: u64, parent: u64) -> TeamCreated {
            let reply = self.create_team(op, team, parent).expect("a created team");
            assert_eq!(reply.outcome, Outcome::Completed);
            serde_json::from_slice(&reply.bytes).expect("a team reply")
        }

        fn archive_team(&self, op: u64, team: u64) {
            let revision = self.team(team).meta.revision;
            let command = ArchiveTeam {
                id: id(team),
                expected_revision: revision,
            };
            self.done(op, &command);
        }

        fn team(&self, team: u64) -> records::Team {
            let found = self.read(|snapshot| snapshot.team(&id(team)));
            found.expect("the team")
        }

        fn employee(&self, employee: u64) -> records::Employee {
            let found = self.read(|snapshot| snapshot.employee(&id(employee)));
            found.expect("the employee")
        }

        fn read<R>(&self, query: impl FnOnce(&E::Snapshot) -> Result<R, EngineError>) -> R {
            let read = self.store.read(|snapshot| Ok(query(snapshot)?));
            read.expect("a read").0
        }

        fn generation(&self) -> u64 {
            self.read(|snapshot| snapshot.generation())
        }

        fn has_receipt(&self, op: u64) -> bool {
            self.read(|snapshot| snapshot.receipt(&id(op))).is_some()
        }

        /// Every file under `durable/state`, by its path under `durable/`.
        fn state_files(&self) -> BTreeMap<String, Vec<u8>> {
            self.store.sync().expect("files written");
            let mut files = BTreeMap::new();
            let mut folders = vec![self.root.join("state")];
            while let Some(folder) = folders.pop() {
                // No operation has written a file yet: no folder.
                let Ok(entries) = std::fs::read_dir(&folder) else {
                    continue;
                };
                for entry in entries {
                    let path = entry.expect("an entry").path();
                    if path.is_dir() {
                        folders.push(path);
                        continue;
                    }
                    let relative = path.strip_prefix(&self.root).expect("under the root");
                    let name = relative.to_str().expect("a UTF-8 path").replace('\\', "/");
                    files.insert(name, std::fs::read(&path).expect("a file"));
                }
            }
            files
        }

        /// Closes the store, deletes its projection (a fresh engine) and opens it again:
        /// a rebuild from the files and the journal.
        fn rebuilt(self, open: &dyn Fn() -> (Arc<E>, G)) -> Result<Self, StoreError> {
            self.store.close().expect("an orderly close");
            let (engine, guard) = open();
            let store = open_store(&self.root, &self.custody, engine, Opening::Existing)?;
            Ok(Self {
                store,
                _engine: guard,
                ..self
            })
        }
    }

    fn team_command(team: u64, parent: u64) -> CreateTeam {
        CreateTeam {
            id: id(team),
            name: format!("Team {team:x}"),
            parent: Some(id(parent)),
        }
    }

    /// The rejection a reply carries.
    fn rejection(reply: &Reply) -> OrgRejection {
        serde_json::from_slice(&reply.bytes).expect("a rejection")
    }

    /// Files as text, so a failed comparison shows the bytes.
    fn text(files: &BTreeMap<String, Vec<u8>>) -> BTreeMap<&str, String> {
        let mut texts = BTreeMap::new();
        for (path, bytes) in files {
            texts.insert(path.as_str(), String::from_utf8_lossy(bytes).into_owned());
        }
        texts
    }

    /// Creation writes its nine state files with the golden bytes and its receipt (ten
    /// files, one entry), at generation 1, with the creator as lead.
    pub fn creation<E: OrganizationEngine, G>(open: &dyn Fn() -> (Arc<E>, G)) {
        let bench = Bench::new(open);
        assert_eq!(bench.generation(), 0);
        let command = creation_command();
        let reply = bench.create_organization(1, &command).expect("created");
        assert_eq!(reply.outcome, Outcome::Completed);
        let created: OrganizationCreated = serde_json::from_slice(&reply.bytes).expect("a reply");
        let expected = OrganizationCreated {
            organization_id: command.organization_id.clone(),
            generation: 1,
            office_code: "A".into(),
            team_id: id(TEAM),
            team_number: 1,
        };
        assert_eq!(created, expected);
        assert_eq!(bench.generation(), 1);

        let golden = creation_files();
        assert_eq!(text(&bench.state_files()), text(&golden));
        assert_eq!(golden.len(), 9);
        let receipts = std::fs::read_dir(bench.root.join("recovery/commands")).expect("receipts");
        let receipts: Vec<_> = receipts
            .map(|entry| entry.expect("an entry").file_name())
            .collect();
        assert_eq!(receipts, [format!("{}.json", id(1)).as_str()]);

        bench.read(|snapshot| {
            let organization = snapshot.organization()?.expect("the organization");
            assert_eq!(organization.id, command.organization_id);
            assert_eq!(organization.organization_repository_id, id(REPOSITORY));
            assert_eq!(snapshot.repositories()?.len(), 1);
            let office = snapshot.office(&command.office_id)?.expect("the office");
            assert_eq!((office.role, office.slug.as_str()), (OfficeRole::Hq, "hq"));
            let code = snapshot.office_code(&command.office_id)?.expect("its code");
            assert_eq!(code.code, "A");

            let team = snapshot.team(&id(TEAM))?.expect("the top-level team");
            assert_eq!(team.name, "Example");
            assert_eq!((team.number, team.parent_team_id.clone()), (1, None));
            assert_eq!(team.lead_employee_id, Some(id(CREATOR)));
            assert_eq!(snapshot.team_by_number(1)?, Some(team));
            assert_eq!(snapshot.team_chain(&id(TEAM))?, Some(vec![id(TEAM)]));

            let creator = snapshot.employee(&id(CREATOR))?.expect("the creator");
            assert_eq!(creator.kind, EmployeeKind::Person);
            assert_eq!(creator.home_team_id, id(TEAM));
            assert!(creator.may_hire);
            let hr = snapshot.employee(&id(HR))?.expect("the HR lead");
            assert_eq!(
                (hr.kind, hr.status),
                (EmployeeKind::Agent, EmployeeStatus::Active)
            );
            assert_eq!(hr.home_team_id, id(TEAM));
            assert!(!hr.may_hire);
            assert!(snapshot.handle_in_use("ana")? && snapshot.handle_in_use("hr")?);
            assert!(!snapshot.handle_in_use("bo")?);

            let counter = snapshot.team_counter()?.expect("the team counter");
            assert_eq!(counter.last_number, 1);
            let allocations = snapshot.team_allocations()?;
            assert_eq!(allocations.len(), 1);
            assert_eq!(
                (allocations[0].number, &allocations[0].team_id),
                (1, &id(TEAM))
            );
            let refs = snapshot.record_refs()?;
            let refs: Vec<_> = refs.iter().map(|r| (r.kind.as_str(), &r.id)).collect();
            let organization_id = &command.organization_id;
            assert_eq!(
                refs,
                [("organization", organization_id), ("team", &id(TEAM))]
            );
            Ok(())
        });
    }

    /// A retry returns the original reply without deciding again; the same ID with other
    /// content conflicts.
    pub fn creation_retry_and_conflict<E: OrganizationEngine, G>(open: &dyn Fn() -> (Arc<E>, G)) {
        let bench = Bench::new(open);
        let command = creation_command();
        let first = bench.create_organization(1, &command).expect("created");
        let decided = bench.decisions.count();
        assert_eq!(bench.create_organization(1, &command), Ok(first.clone()));
        assert_eq!(bench.decisions.count(), decided);

        let mut other = command.clone();
        other.name = "Another".into();
        assert_eq!(
            bench.create_organization(1, &other),
            Err(StoreError::Conflict)
        );
        assert_eq!(bench.decisions.count(), decided);
        assert_eq!(bench.generation(), 1);

        let bench = bench.rebuilt(open).expect("a rebuild");
        assert_eq!(bench.create_organization(1, &command), Ok(first));
        assert_eq!(bench.decisions.count(), decided);
    }

    /// A second creation is refused, and so is one this store cannot hold; nothing is
    /// recorded, so a retry of a failed creation with the same ID decides afresh.
    pub fn creation_refusals<E: OrganizationEngine, G>(open: &dyn Fn() -> (Arc<E>, G)) {
        let bench = Bench::new(open);
        let good = creation_command();
        let refused = |change: &dyn Fn(&mut CreateOrganization)| {
            let mut command = good.clone();
            change(&mut command);
            let reply = bench.create_organization(7, &command);
            assert!(matches!(reply, Err(StoreError::Invalid(_))), "{reply:?}");
            assert!(!bench.has_receipt(7));
            assert_eq!(bench.generation(), 0);
        };
        refused(&|c| c.slug = "Not A Slug".into());
        refused(&|c| c.name = " ".into());
        refused(&|c| c.office_name = "two\nlines".into());
        refused(&|c| c.repository_remote_url = String::new());
        refused(&|c| c.state_branch = "org state".into());
        refused(&|c| c.creator.name = String::new());
        refused(&|c| c.creator.handle = "hr".into());
        refused(&|c| c.creator.handle = "here".into());
        refused(&|c| c.creator.handle = "Ana".into());
        refused(&|c| c.hr_employee_id = c.creator.employee_id.clone());
        refused(&|c| c.team_id = c.organization_id.clone());
        refused(&|c| c.repository_id = Id::parse("not-a-uuid").expect("a safe component"));
        // Another organization's or office's creation is not this store's to accept.
        refused(&|c| c.organization_id = id(0xb2));
        refused(&|c| c.office_id = id(0xb3));
        assert!(bench.state_files().is_empty());

        // The failed creation's ID decides afresh, and now succeeds.
        let decided = bench.decisions.count();
        let reply = bench.create_organization(7, &good).expect("created");
        assert_eq!(reply.outcome, Outcome::Completed);
        assert_eq!(bench.decisions.count(), decided + 1);
        assert_eq!(bench.generation(), 1);

        // A second creation, under any ID and with any content, is refused unrecorded.
        let mut second = good.clone();
        second.team_id = id(0x21);
        for command in [&good, &second] {
            let reply = bench.create_organization(8, command);
            assert!(matches!(reply, Err(StoreError::Invalid(_))), "{reply:?}");
        }
        assert!(!bench.has_receipt(8));
        assert_eq!(bench.generation(), 1);
        assert_eq!(text(&bench.state_files()), text(&creation_files()));
    }

    /// Teams take the numbers 2 and 3, each with the counter and its allocation, and one
    /// generation each.
    pub fn team_numbers<E: OrganizationEngine, G>(open: &dyn Fn() -> (Arc<E>, G)) {
        let bench = Bench::created(open);
        let expected = |team: u64, number: u64| TeamCreated {
            id: id(team),
            number,
            generation: number,
        };
        assert_eq!(bench.created_team(2, 0x21, TEAM), expected(0x21, 2));
        assert_eq!(bench.created_team(3, 0x22, TEAM), expected(0x22, 3));
        assert_eq!(bench.generation(), 3);
        // A retry is the original reply, and takes no number.
        assert_eq!(bench.created_team(2, 0x21, TEAM), expected(0x21, 2));

        let organization = identity().organization_id;
        bench.read(|snapshot| {
            let counter = snapshot.team_counter()?.expect("the counter");
            assert_eq!(counter.last_number, 3);
            let numbered: Vec<_> = snapshot.team_allocations()?;
            let numbered: Vec<_> = numbered
                .iter()
                .map(|a| (a.number, a.team_id.clone()))
                .collect();
            assert_eq!(numbered, [(1, id(TEAM)), (2, id(0x21)), (3, id(0x22))]);
            let team = snapshot.team_by_number(3)?.expect("TEAM-3");
            assert_eq!(team.id, id(0x22));
            assert_eq!(team.parent_team_id, Some(id(TEAM)));
            assert_eq!(team.lead_employee_id, None);
            assert_eq!(team.meta.created_by.employee(), Some(&id(CREATOR)));
            assert_eq!(snapshot.record_refs()?.len(), 4);
            Ok(())
        });
        let files = bench.state_files();
        let counter = TeamCounter {
            organization_id: organization.clone(),
            last_number: 3,
        };
        let counter_path = records::team_counter_path(&organization);
        assert_eq!(
            files[counter_path.as_str()],
            records::encode_team_counter(&counter)
        );
        for (number, team) in [(2, 0x21), (3, 0x22)] {
            let path = records::team_allocation_path(&organization, number);
            let allocation = records::decode_team_allocation(&files[path.as_str()]);
            let allocation = allocation.expect("an allocation");
            assert_eq!((allocation.number, allocation.team_id), (number, id(team)));
            assert_eq!(allocation.allocated_at, records::time_text(CREATION_AT));
        }
        assert_eq!(files.len(), 9 + 2 * 2);
    }

    /// A rule's refusal (an unknown or archived parent, a bad name, a used ID) is a recorded
    /// rejection that takes no generation and no number, and its retry is the same answer;
    /// an actor who is not the owner is refused unrecorded; an operation ID used again with
    /// another command conflicts.
    pub fn team_refusals<E: OrganizationEngine, G>(open: &dyn Fn() -> (Arc<E>, G)) {
        let bench = Bench::created(open);
        assert_eq!(bench.created_team(2, 0x21, TEAM).number, 2);
        bench.archive_team(3, 0x21);
        let generation = bench.generation();
        assert_eq!(generation, 3);
        let files = bench.state_files();

        let code = |op: u64, command: &CreateTeam| bench.refused(op, command).code;
        assert_eq!(code(4, &team_command(0x22, 0x99)), RejectCode::NotFound);
        assert_eq!(code(5, &team_command(0x22, 0x21)), RejectCode::InvalidRequest);
        let unnamed = CreateTeam {
            name: String::new(),
            ..team_command(0x22, TEAM)
        };
        assert_eq!(code(6, &unnamed), RejectCode::InvalidName);
        assert_eq!(code(7, &team_command(0x21, TEAM)), RejectCode::AlreadyExists);
        // An ID that is another record's: the organization, the office, an employee, the
        // repository.
        let here = identity();
        for (op, taken) in [
            (11, here.organization_id),
            (12, here.office_id),
            (13, id(CREATOR)),
            (14, id(HR)),
            (15, id(REPOSITORY)),
        ] {
            let command = CreateTeam {
                id: taken,
                ..team_command(0x22, TEAM)
            };
            assert_eq!(code(op, &command), RejectCode::AlreadyExists);
        }
        // The retry of a rejection is the same answer without deciding, even once the
        // answer would differ.
        let decided = bench.decisions.count();
        let first = bench.create_team(4, 0x22, 0x99).expect("the stored answer");
        assert_eq!(rejection(&first).code, RejectCode::NotFound);
        assert_eq!(bench.decisions.count(), decided);

        // Nobody but the owner creates a team: refused unrecorded, whoever asks.
        let unrecorded = |op: u64, by: Actor| {
            let reply = bench.submit(op, &by, &team_command(0x22, TEAM));
            assert_eq!(reply, Err(StoreError::invalid(NOT_AUTHORIZED)));
            assert!(!bench.has_receipt(op));
            assert_eq!(bench.generation(), generation);
        };
        unrecorded(16, Actor::Employee(id(0x99)));
        unrecorded(17, Actor::Employee(id(HR)));
        unrecorded(18, Actor::System);
        // A malformed ID is refused unrecorded too.
        let malformed = CreateTeam {
            id: Id::parse("team-x").expect("a safe component"),
            ..team_command(0x22, TEAM)
        };
        assert!(matches!(bench.owner(19, &malformed), Err(StoreError::Invalid(_))));
        assert!(!bench.has_receipt(19));

        // The ID of a completed creation with another command conflicts: nothing is
        // decided, and the generation and the original answer stand.
        let decided = bench.decisions.count();
        for (team, parent) in [(0x24, TEAM), (0x21, 0x21)] {
            let other = bench.create_team(2, team, parent);
            assert_eq!(other, Err(StoreError::Conflict), "{team:x} {parent:x}");
        }
        let other_actor = bench.submit(2, &Actor::System, &team_command(0x21, TEAM));
        assert_eq!(other_actor, Err(StoreError::Conflict));
        // So does the ID of a recorded rejection.
        assert_eq!(bench.create_team(4, 0x24, TEAM), Err(StoreError::Conflict));
        assert_eq!(bench.decisions.count(), decided);
        assert_eq!(bench.generation(), generation);
        assert_eq!(bench.created_team(2, 0x21, TEAM).number, 2);
        assert_eq!(bench.decisions.count(), decided);

        assert_eq!(text(&bench.state_files()), text(&files));
        bench.read(|snapshot| {
            assert_eq!(snapshot.team(&id(0x24))?, None);
            assert_eq!(snapshot.team_counter()?.map(|c| c.last_number), Some(2));
            assert_eq!(snapshot.team(&id(0x22))?, None);
            Ok(())
        });
        // The next accepted creation takes the next number and the next generation.
        let next = bench.created_team(9, 0x22, TEAM);
        assert_eq!((next.number, next.generation), (3, generation + 1));
        let team = bench.team(0x22);
        assert_eq!(team.meta.created_by, records::By::Employee(id(CREATOR)));
        assert_eq!(team.max_members, records::DEFAULT_MAX_MEMBERS);
    }

    /// Every row of the family and its generation, to compare across a rebuild.
    fn rows<E: OrganizationEngine, G>(bench: &Bench<E, G>) -> impl PartialEq + std::fmt::Debug {
        bench.read(|snapshot| {
            Ok((
                snapshot.generation()?,
                snapshot.organization()?,
                snapshot.repositories()?,
                snapshot.offices()?,
                snapshot.office_codes()?,
                snapshot.teams()?,
                snapshot.employees()?,
                snapshot.team_counter()?,
                snapshot.team_allocations()?,
                snapshot.record_refs()?,
            ))
        })
    }

    /// Creations submitted at once are numbered one after another: no number and no
    /// generation is given twice or skipped, whatever order the store took them in.
    pub fn concurrent_team_numbers<E: OrganizationEngine, G>(open: &dyn Fn() -> (Arc<E>, G)) {
        const TEAMS: u64 = 8;
        let bench = Bench::created(open);
        let by = Actor::Employee(id(CREATOR));
        let (store, decisions, by) = (&bench.store, &bench.decisions, &by);
        // Every thread is ready before any submits: the submits start together.
        let start = std::sync::Barrier::new(TEAMS as usize);
        let start = &start;
        let created: Vec<TeamCreated> = std::thread::scope(|scope| {
            let submit = |n: u64| {
                move || {
                    let command = team_command(0x20 + n, TEAM);
                    let decide = decisions.feature(by, &command);
                    let operation = command.operation(id(100 + n), by);
                    start.wait();
                    let reply = store.submit(Some(operation), false, decide);
                    let reply = reply.expect("a created team");
                    assert_eq!(reply.outcome, Outcome::Completed);
                    let created: TeamCreated =
                        serde_json::from_slice(&reply.bytes).expect("a reply");
                    assert_eq!(created.id, id(0x20 + n));
                    created
                }
            };
            let threads: Vec<_> = (1..=TEAMS).map(|n| scope.spawn(submit(n))).collect();
            let joined = threads.into_iter().map(|thread| thread.join());
            joined.map(|created| created.expect("no panic")).collect()
        });

        let expected: Vec<u64> = (2..=TEAMS + 1).collect();
        let mut numbers: Vec<u64> = created.iter().map(|c| c.number).collect();
        numbers.sort_unstable();
        assert_eq!(numbers, expected);
        let mut generations: Vec<u64> = created.iter().map(|c| c.generation).collect();
        generations.sort_unstable();
        assert_eq!(generations, expected);
        assert_eq!(bench.generation(), TEAMS + 1);

        // One allocation per number, each to the team that was told the number.
        let mut issued: Vec<(u64, Id)> = created.iter().map(|c| (c.number, c.id.clone())).collect();
        issued.push((1, id(TEAM)));
        issued.sort();
        bench.read(|snapshot| {
            let counter = snapshot.team_counter()?.expect("the counter");
            assert_eq!(counter.last_number, TEAMS + 1);
            let allocations = snapshot.team_allocations()?;
            let allocated: Vec<_> = allocations
                .iter()
                .map(|a| (a.number, a.team_id.clone()))
                .collect();
            assert_eq!(allocated, issued);
            for (number, team) in &issued {
                let numbered = snapshot.team_by_number(*number)?.expect("the team");
                assert_eq!(&numbered.id, team);
                assert_eq!(snapshot.team(team)?.map(|team| team.number), Some(*number));
            }
            assert_eq!(snapshot.teams()?.len(), issued.len());
            Ok(())
        });
        let files = bench.state_files();
        assert_eq!(files.len(), 9 + 2 * TEAMS as usize);

        let before = rows(&bench);
        let bench = bench.rebuilt(open).expect("a rebuild");
        assert!(rows(&bench) == before, "{:?}\n{before:?}", rows(&bench));
        assert_eq!(text(&bench.state_files()), text(&files));
        let next = bench.created_team(200, 0x40, TEAM);
        assert_eq!((next.number, next.generation), (TEAMS + 2, TEAMS + 2));
    }

    /// A team's file is nested in its parent's folder, at every depth, until the tree is
    /// too deep for a path.
    pub fn nested_team_path<E: OrganizationEngine, G>(open: &dyn Fn() -> (Arc<E>, G)) {
        let bench = Bench::created(open);
        bench.created_team(2, 0x21, TEAM);
        bench.created_team(3, 0x22, 0x21);
        let chain = [id(TEAM), id(0x21), id(0x22)];
        let found = bench.read(|snapshot| snapshot.team_chain(&id(0x22)));
        assert_eq!(found, Some(chain.to_vec()));
        let path = format!(
            "state/teams/{}/teams/{}/teams/{}/team.yaml",
            chain[0], chain[1], chain[2]
        );
        let files = bench.state_files();
        let team = records::decode_team(&files[&path]).expect("the nested team");
        assert_eq!(team.parent_team_id, Some(id(0x21)));
        assert_eq!(team.number, 3);

        // Deeper and deeper, until the path would not fit: refused, nothing recorded.
        let mut parent = 0x22;
        let refused = loop {
            let team = parent + 1;
            let reply = bench.create_team(100 + team, team, parent).expect("decided");
            match reply.outcome {
                Outcome::Completed => parent = team,
                Outcome::Rejected => break rejection(&reply),
            }
            assert!(parent < 0x40, "no depth was refused");
        };
        assert_eq!(refused.code, RejectCode::InvalidRequest);
        let depth = bench.read(|snapshot| snapshot.team_chain(&id(parent)));
        let depth = depth.expect("the deepest team").len();
        assert!(
            (8..=crate::organization::ports::MAX_TEAM_DEPTH).contains(&depth),
            "{depth}"
        );
        let bench = bench.rebuilt(open).expect("the deepest tree rebuilds");
        let again = bench.read(|snapshot| snapshot.team_chain(&id(parent)));
        assert_eq!(again.map(|chain| chain.len()), Some(depth));
    }

    /// The generation is the organization stream's high-water: a read returns its
    /// snapshot's, a refusal leaves it alone, and a rebuild from the files and the journal
    /// arrives at the same one with the same rows.
    pub fn generation_and_rebuild<E: OrganizationEngine, G>(open: &dyn Fn() -> (Arc<E>, G)) {
        let bench = Bench::created(open);
        bench.created_team(2, 0x21, TEAM);
        bench.archive_team(3, 0x21);
        let rejected = bench.create_team(4, 0x22, 0x21).expect("a rejection");
        assert_eq!(rejected.outcome, Outcome::Rejected);
        let unknown = bench.create_team(5, 0x22, 0x99).expect("a rejection");
        assert_eq!(unknown.outcome, Outcome::Rejected);
        bench.created_team(6, 0x22, TEAM);
        assert_eq!(bench.generation(), 4);

        let before = rows(&bench);
        let files = bench.state_files();
        let bench = bench.rebuilt(open).expect("a rebuild");
        assert!(rows(&bench) == before, "{:?}\n{before:?}", rows(&bench));
        assert_eq!(bench.generation(), 4);
        assert_eq!(text(&bench.state_files()), text(&files));
        // The rejection is still the answer, and the next creation is generation 5.
        assert_eq!(bench.create_team(4, 0x22, 0x21), Ok(rejected));
        let next = bench.created_team(7, 0x23, 0x22);
        assert_eq!((next.number, next.generation), (4, 5));
    }

    /// A rebuild refuses files that break a rule spanning files: here a missing
    /// organization record, and a team led from outside it. An engine with constraints
    /// of its own does not let such files in at all: its backstop fails the write with
    /// an internal error, never a refusal, and nothing is accepted.
    pub fn rebuild_validation<E: OrganizationEngine, G>(open: &dyn Fn() -> (Arc<E>, G)) {
        let refused = |bench: Bench<E, G>, faulty: Change, reason: &str| {
            let generation = bench.generation();
            let decide = bench.decisions.write(faulty);
            match bench.store.submit(None, false, decide) {
                Err(StoreError::Internal(_)) => {
                    assert_eq!(bench.generation(), generation);
                    return;
                }
                reply => assert_eq!(reply.map(|reply| reply.outcome), Ok(Outcome::Completed)),
            }
            let error = bench.rebuilt(open).err().expect("the rebuild is refused");
            let text = error.to_string();
            assert!(text.contains(reason), "{text}");
        };

        let bench = Bench::created(open);
        let faulty = Change::new(Op::CreateTeam.ack()).remove(records::organization_path());
        refused(bench, faulty, "the organization record is missing");

        // The creator's home is the top-level team; a child team cannot be led by them.
        let bench = Bench::created(open);
        bench.created_team(2, 0x21, TEAM);
        let chain = [id(TEAM), id(0x21)];
        let mut team = bench
            .read(|snapshot| snapshot.team(&id(0x21)))
            .expect("the team");
        team.lead_employee_id = Some(id(CREATOR));
        let path = records::team_path(&chain).expect("a path");
        let faulty = Change::new(Op::CreateTeam.ack()).put(path, records::encode_team(&team));
        refused(bench, faulty, "its lead is not an employee of the team");

        // Without the fault, the same files rebuild.
        let bench = Bench::created(open);
        bench.created_team(2, 0x21, TEAM);
        assert!(bench.rebuilt(open).is_ok());
    }

    // ---- the family's script for the rebuild-equivalence harness ----

    /// Generates the family's commands for `org_store::testkit::rebuild_equivalence`:
    /// creations (valid, invalid and repeated), teams under known, unknown and archived
    /// parents with colliding IDs, archivals and subtree moves.
    pub struct OrganizationScript {
        decisions: Decisions,
    }

    #[derive(Clone, Debug)]
    pub enum Step {
        Organization {
            op: Option<u64>,
            valid: bool,
        },
        Team {
            op: Option<u64>,
            team: u64,
            parent: u64,
        },
        Archive {
            op: Option<u64>,
            team: u64,
        },
        Move {
            op: Option<u64>,
            team: u64,
            parent: u64,
        },
    }

    /// The few teams a script creates, so IDs collide and parents exist.
    const POOL: u64 = 6;

    impl OrganizationScript {
        pub fn new() -> Self {
            Self {
                decisions: Decisions::new(),
            }
        }
    }

    impl Default for OrganizationScript {
        fn default() -> Self {
            Self::new()
        }
    }

    impl<E: OrganizationEngine> FamilyScript<E> for OrganizationScript {
        type Spec = Step;

        fn projectors(&self, identity: &OfficeIdentity) -> Vec<Arc<dyn Projector<E>>> {
            vec![Arc::new(OrganizationFiles::new(
                identity.organization_id.clone(),
            ))]
        }

        fn generate(&self, rng: &mut Rng, step: u64) -> Step {
            let op = (!rng.chance(4)).then_some(step + 1);
            let pooled = |rng: &mut Rng| 0x100 + rng.below(POOL);
            // Most sequences begin with the organization; some never have one.
            if step == 0 && !rng.chance(8) {
                return Step::Organization { op, valid: true };
            }
            match rng.below(12) {
                0 => Step::Organization {
                    op,
                    valid: !rng.chance(3),
                },
                1 => Step::Archive {
                    op,
                    team: pooled(rng),
                },
                2 => Step::Move {
                    op,
                    team: pooled(rng),
                    parent: if rng.chance(3) { TEAM } else { pooled(rng) },
                },
                kind => {
                    let parent = match kind {
                        3 => 0x1ff,
                        4..=6 => TEAM,
                        _ => pooled(rng),
                    };
                    let team = pooled(rng);
                    Step::Team { op, team, parent }
                }
            }
        }

        fn command(&self, spec: &Step) -> Command<E> {
            match spec {
                Step::Organization { op, valid } => {
                    let mut command = creation_command();
                    if !valid {
                        command.slug = "Not A Slug".into();
                    }
                    Command {
                        op: op.map(|n| command.operation(id(n))),
                        decide: self.decisions.create_organization(&command),
                    }
                }
                Step::Team { op, team, parent } => {
                    let command = team_command(*team, *parent);
                    let by = Actor::Employee(id(CREATOR));
                    Command {
                        op: op.map(|n| command.operation(id(n), &by)),
                        decide: self.decisions.feature(&by, &command),
                    }
                }
                Step::Archive { op, team } => Command {
                    op: op.map(|n| test_operation(n, "test.archive_team", &id(*team))),
                    decide: self.decisions.archive_team(&id(*team)),
                },
                Step::Move { op, team, parent } => Command {
                    op: op.map(|n| test_operation(n, "test.move_team", &id(*team))),
                    decide: self.decisions.move_team(&id(*team), &id(*parent)),
                },
            }
        }

        fn decisions(&self) -> usize {
            self.decisions.count()
        }
    }

    /// The projection the write path builds equals the one a rebuild builds from the
    /// files it left, with and without a crash. Every failure prints its seed.
    pub fn rebuild_equivalent<E, G>(open: &dyn Fn() -> (Arc<E>, G))
    where
        E: OrganizationEngine + Dump,
    {
        rebuild_equivalence(open, &OrganizationScript::new(), 0..8);
    }
}

/// Declares one test per case of the organization family's suite. `$open` returns a
/// fresh, empty engine in an `Arc`, and whatever must outlive it.
#[macro_export]
macro_rules! organization_conformance_tests {
    ($open:expr) => {
        #[test]
        fn organization_creation_writes_its_ten_files() {
            $crate::organization_family::testkit::creation(&$open);
        }
        #[test]
        fn organization_creation_retry_and_conflict() {
            $crate::organization_family::testkit::creation_retry_and_conflict(&$open);
        }
        #[test]
        fn organization_creation_refusals() {
            $crate::organization_family::testkit::creation_refusals(&$open);
        }
        #[test]
        fn organization_team_numbers() {
            $crate::organization_family::testkit::team_numbers(&$open);
        }
        #[test]
        fn organization_concurrent_team_numbers() {
            $crate::organization_family::testkit::concurrent_team_numbers(&$open);
        }
        #[test]
        fn organization_team_refusals() {
            $crate::organization_family::testkit::team_refusals(&$open);
        }
        #[test]
        fn organization_nested_team_path() {
            $crate::organization_family::testkit::nested_team_path(&$open);
        }
        #[test]
        fn organization_generation_and_rebuild() {
            $crate::organization_family::testkit::generation_and_rebuild(&$open);
        }
        #[test]
        fn organization_rebuild_validation() {
            $crate::organization_family::testkit::rebuild_validation(&$open);
        }
        #[test]
        fn organization_rebuild_equivalence() {
            $crate::organization_family::testkit::rebuild_equivalent(&$open);
        }
    };
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use org_store::testkit::MemoryEngine;

    crate::organization_conformance_tests!(|| (Arc::new(MemoryEngine::new()), ()));
}
