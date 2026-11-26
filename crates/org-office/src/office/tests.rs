//! The office on its store, created, opened, killed and refused, in temporary folders.
//! Memory by default (ENG-2); SQLite where the projection's file matters.

use std::{
    cell::Cell,
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use org_proto::{
    directory::WhoParams,
    org::{CommandStatus, TeamCreateParams, TeamRenameParams},
    ErrorCode,
};
use org_store::{
    testkit::{identity, CrashPoint, DiskOp, FakeCustody, Faults, FlushSite, Target},
    Sha256,
};

use super::{
    testkit::{self, OPERATION},
    *,
};
use crate::{
    office_file,
    office_local::{records as local, testkit::ca_material},
    organization::testkit::creation_files,
    store::open::Testing,
};

#[path = "hiring_tests.rs"]
mod hiring_tests;

const ENGINES: [EngineChoice; 2] = [EngineChoice::Sqlite, EngineChoice::Memory];
const CREATOR: &str = "01900000-0000-7000-8000-000000000012";
const HR: &str = "01900000-0000-7000-8000-000000000013";
const TOP_TEAM: &str = "01900000-0000-7000-8000-000000000011";
const TEAM_A: &str = "01900000-0000-7000-8000-000000000021";
const TEAM_B: &str = "01900000-0000-7000-8000-000000000022";

/// One office's folder, data roots, custody and disk, which outlive any one process.
struct Site {
    root: PathBuf,
    folder: PathBuf,
    roots: DataRoots,
    custody: Arc<FakeCustody>,
    faults: Arc<Faults>,
    _dir: tempfile::TempDir,
}

impl Site {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let (folder, roots) = testkit::places(&root);
        Self {
            root,
            folder,
            roots,
            custody: Arc::new(FakeCustody::new()),
            faults: Faults::new(),
            _dir: dir,
        }
    }

    fn options(&self, engine: EngineChoice) -> Options {
        let mut options = testkit::options(engine);
        options.testing = Testing {
            faults: Some(Arc::clone(&self.faults)),
            ..Testing::default()
        };
        options
    }

    fn provision_with(
        &self,
        engine: EngineChoice,
        material: impl FnOnce(Option<LocalRecord>) -> Result<LocalRecord, OfficeError>,
    ) -> Result<Office, OfficeError> {
        let (new, binding) = testkit::request();
        Office::provision_with(
            self.folder.clone(),
            &new,
            Some(Key::from_bytes([7; 32])),
            &binding,
            self.roots.clone(),
            testkit::custody(Arc::clone(&self.custody)),
            material,
            self.options(engine),
        )
    }

    /// The suite's creation: the retained CA when there is one, else seed 1's.
    fn provision(&self, engine: EngineChoice) -> Result<Office, OfficeError> {
        self.provision_with(engine, |prior| {
            prior.map_or_else(|| testkit::record(testkit::CA_SEED), Ok)
        })
    }

    fn open(&self, engine: EngineChoice) -> Result<Office, OfficeError> {
        let instance = identity().instance_id.to_string();
        Office::open_with(
            self.folder.clone(),
            None,
            &instance,
            self.roots.clone(),
            testkit::custody(Arc::clone(&self.custody)),
            self.options(engine),
        )
    }

    /// The process dies with `office` open, and a new one finds the same disk.
    fn killed(&self, office: Office) {
        self.faults.kill();
        drop(office);
        self.faults.revive();
    }

    fn durable(&self, path: &str) -> PathBuf {
        self.roots.durable.join(path)
    }

    fn runtime_folder(&self) -> PathBuf {
        self.roots.runtime.join(identity().office_id.as_str())
    }
}

/// Every file under `root`, by its path under it.
fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut folders = vec![root.to_owned()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path);
                continue;
            }
            let name = path
                .strip_prefix(root)
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned();
            files.insert(name, std::fs::read(&path).unwrap());
        }
    }
    files
}

fn names(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

fn creation_id() -> Id {
    Id::parse(OPERATION).unwrap()
}

fn team_create(n: u64, team: &str, parent: &str) -> OrgCommand {
    OrgCommand::TeamCreate(TeamCreateParams {
        command_id: format!("018bce27-f800-7000-8000-{n:012x}"),
        team_id: team.into(),
        name: format!("Team {n}"),
        owner_team_id: Some(parent.into()),
    })
}

/// STORE-6 on the production path: the creation's files under `durable/` are exactly the
/// golden ones and the two CA files, and nothing an older build made is anywhere.
#[test]
fn creation_writes_exactly_the_golden_files_and_the_ca_records() {
    for engine in ENGINES {
        let site = Site::new();
        let office = site.provision(engine).unwrap();
        let material = ca_material(&identity().organization_id, testkit::CA_SEED);

        let mut expected = creation_files();
        expected.insert(
            "state/security/ca-escrow.v1.bin".into(),
            material.package.clone(),
        );
        let state: BTreeMap<String, Vec<u8>> = tree(&site.durable("state"))
            .into_iter()
            .map(|(path, bytes)| (format!("state/{path}"), bytes))
            .collect();
        assert_eq!(state, expected, "{engine:?}");
        let key = material.record(&identity(), &creation_id());
        let key_path = local::ca_key_path(&identity().office_id);
        let key_path = key_path.as_str().strip_prefix("local/").unwrap().to_owned();
        let expected_local = BTreeMap::from([(key_path, local::encode_ca_key(&key))]);
        let mut local_tree = tree(&site.durable("local"));
        // The Sqlite engine alone also keeps the canonical replication state (TASK-52); the
        // memory engine writes nothing there. Nothing else may appear beside the CA key.
        let replication_path =
            format!("offices/{}/replication/state.v1.json", identity().office_id);
        let replication = local_tree.remove(&replication_path);
        match engine {
            EngineChoice::Sqlite => {
                let bytes = replication.expect("the sqlite creation writes the replication state");
                let state: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(state["version"], 1);
                assert_eq!(state["local_office"], identity().office_id.to_string());
                assert_eq!(state["seq"], 0);
            }
            EngineChoice::Memory => assert!(replication.is_none(), "{engine:?}"),
        }
        assert_eq!(local_tree, expected_local, "{engine:?}");
        // Under durable/ only the files, the journal and the receipt; the Sqlite engine
        // alone adds the companion stores' folders (TASK-52 and the stores before it).
        let top: &[&str] = match engine {
            EngineChoice::Sqlite => &[
                "accounting",
                "authority",
                "budgets",
                "local",
                "private",
                "recovery",
                "state",
            ],
            EngineChoice::Memory => &["local", "recovery", "state"],
        };
        assert_eq!(names(&site.roots.durable), top, "{engine:?}");
        let receipts = tree(&site.durable("recovery/commands"));
        let receipts: Vec<&String> = receipts.keys().collect();
        assert_eq!(receipts, [&format!("{}.json", creation_id())], "{engine:?}");

        // The office folder: the lock, the intent and office.json; no database, no
        // workspace.
        assert_eq!(
            names(&site.folder),
            ["hq-creation.v1.json", "office.json", "office.lock"]
        );
        let frozen = intent::read(&site.folder).unwrap().unwrap();
        assert_eq!(frozen.version, 3);
        assert_eq!(frozen.operation, OPERATION);
        assert_eq!(frozen.roots(), site.roots);

        // The public record is the escrow package, bound by the key record's hash.
        let public = office.hq_creation_public_record().unwrap().unwrap();
        assert_eq!(public.package, material.package);
        assert_eq!(key.package_sha256, Sha256::of(&public.package));
        assert_eq!(public.operation_id, creation_id().as_str());
        let record = office.hq_creation_record().unwrap().unwrap();
        assert_eq!(record.wrapped_ca_key(), material.wrapped_ca_key.as_slice());
        assert!(office.is_hq().unwrap());
        assert_eq!(office.org_snapshot().unwrap().position.seq, 1);
        office.close().unwrap();
    }
}

/// STORE-6, the other half: a foreign file where creation writes stops it, and nothing is
/// replaced and the office is not finished. The organization record is checked before a
/// journal begins, and is permanent: no anchor is written for the new incarnation. Any
/// other file of the creation is found when its entry is written, and its cause names the
/// path.
#[test]
fn a_foreign_file_where_creation_writes_stops_it_and_is_named() {
    for planted in ["state/organization.yaml", "state/repositories.yaml"] {
        let site = Site::new();
        std::fs::create_dir_all(site.durable("state")).unwrap();
        std::fs::write(site.durable(planted), b"planted: true\n").unwrap();
        let error = site.provision(EngineChoice::Sqlite).unwrap_err();
        if planted == "state/organization.yaml" {
            assert!(
                matches!(
                    error,
                    OfficeError::Invalid("the data roots hold an organization before its creation")
                ),
                "{planted}: {error:?}"
            );
            let anchor = org_store::journal_dir(&site.roots.durable, &identity());
            assert!(!anchor.join("anchor.json").exists(), "{planted}");
        } else {
            assert!(
                matches!(error, OfficeError::Storage(_)),
                "{planted}: {error:?}"
            );
            let cause = std::error::Error::source(&error).unwrap().to_string();
            assert!(cause.contains(planted), "{cause}");
        }
        assert!(!error.to_string().contains(".yaml"), "{error}");
        let kept = std::fs::read(site.durable(planted)).unwrap();
        assert_eq!(kept, b"planted: true\n");
        assert!(!site.folder.join("office.json").exists());
    }
}

/// A team, then the projection deleted: the reopened office rebuilds the same rows and
/// reads the same snapshot (SQLite: the memory engine always rebuilds).
#[test]
fn deleting_the_runtime_folder_rebuilds_an_equal_projection() {
    let site = Site::new();
    let office = site.provision(EngineChoice::Sqlite).unwrap();
    let answer = office.submit_org(CREATOR, team_create(1, TEAM_A, TOP_TEAM));
    assert_eq!(answer.unwrap().unwrap().status, CommandStatus::Committed);
    let (dump, snapshot) = (office.dump(), office.org_snapshot().unwrap());
    office.close().unwrap();
    std::fs::remove_dir_all(site.runtime_folder()).unwrap();

    let office = site.open(EngineChoice::Sqlite).unwrap();
    assert_eq!(office.dump(), dump);
    assert_eq!(office.org_snapshot().unwrap(), snapshot);
    assert!(site.runtime_folder().join("office.db").is_file());
    office.close().unwrap();
}

/// A dropped office is a killed one: what it answered is there, and the next opening is
/// told the stop was not clean. An orderly close is.
#[test]
fn an_answered_team_survives_a_kill_and_only_a_close_is_a_clean_stop() {
    for engine in ENGINES {
        let site = Site::new();
        let office = site.provision(engine).unwrap();
        let answered = office.submit_org(CREATOR, team_create(1, TEAM_A, TOP_TEAM));
        let answered = answered.unwrap().unwrap();
        site.killed(office);

        let office = site.open(engine).unwrap();
        assert!(!office.stopped_cleanly(), "{engine:?}");
        let teams = office.org_snapshot().unwrap().teams;
        assert!(teams.iter().any(|team| team.id == TEAM_A), "{engine:?}");
        // The same command again: the same answer.
        let again = office.submit_org(CREATOR, team_create(1, TEAM_A, TOP_TEAM));
        assert_eq!(again.unwrap(), Ok(answered), "{engine:?}");
        office.close().unwrap();

        let office = site.open(engine).unwrap();
        assert!(office.stopped_cleanly(), "{engine:?}");
        office.close().unwrap();
    }
}

/// A creation that dies at any point is completed by the same request: one entry 1, one
/// receipt, and the CA the first attempt committed, handed back to `material`. H1: while
/// it has not completed, there is no `office.json`.
#[test]
fn a_creation_retried_after_a_crash_point_keeps_its_ca_and_its_one_entry() {
    let points = [
        CrashPoint::DuringDecide,
        CrashPoint::AfterAppend,
        CrashPoint::AfterCommit,
        CrashPoint::MaterializeTempFlushed,
        CrashPoint::MaterializeRenamed,
        CrashPoint::BeforeMark,
    ];
    for engine in ENGINES {
        for point in points {
            let context = format!("{engine:?} {point:?}");
            let site = Site::new();
            // Armed once the store is open, so it fires inside the creation itself.
            let failed = site.provision_with(engine, |prior| {
                site.faults.arm(point, 1);
                prior.map_or_else(|| testkit::record(testkit::CA_SEED), Ok)
            });
            assert!(failed.is_err(), "{context}");
            drop(failed);
            site.faults.revive();
            // H1: nobody was told the office exists.
            assert!(!site.folder.join("office.json").exists(), "{context}");

            let prior = Cell::new(None);
            let office = site
                .provision_with(engine, |found| {
                    prior.set(Some(found.is_some()));
                    // Another seed: used only if no CA was committed.
                    found.map_or_else(|| testkit::record(9), Ok)
                })
                .unwrap_or_else(|e| panic!("{context}: {e:?}"));
            let appended = point != CrashPoint::DuringDecide;
            assert_eq!(prior.get(), Some(appended), "{context}");
            let seed = if appended { testkit::CA_SEED } else { 9 };
            let expected = ca_material(&identity().organization_id, seed);
            let public = office.hq_creation_public_record().unwrap().unwrap();
            assert_eq!(public.public_key, expected.public_key, "{context}");
            assert_eq!(office.org_snapshot().unwrap().position.seq, 1, "{context}");
            let receipts = tree(&site.durable("recovery/commands"));
            assert_eq!(receipts.len(), 1, "{context}");
            assert!(site.folder.join("office.json").is_file(), "{context}");
            office.close().unwrap();
        }
    }
}

/// H2: a creation that dies after the journal's folders and before its anchor is
/// completed by a retry, which starts that incarnation afresh; one that dies just after
/// the anchor opens it.
#[test]
fn a_creation_that_dies_around_the_journal_anchor_completes_on_retry() {
    for engine in ENGINES {
        for after in [false, true] {
            let site = Site::new();
            // The first exclusive link of a new office is its journal's anchor.
            site.faults.crash(DiskOp::Link, Target::File, 1, after);
            assert!(site.provision(engine).is_err(), "{engine:?} {after}");
            site.faults.revive();
            let anchor = org_store::journal_dir(&site.roots.durable, &identity());
            assert_eq!(anchor.join("anchor.json").exists(), after, "{engine:?}");
            let office = site.provision(engine).unwrap();
            assert_eq!(office.org_snapshot().unwrap().position.seq, 1);
            office.close().unwrap();
        }
    }
}

type Spoil = Box<dyn Fn(&Site)>;

fn rewrite_key(site: &Site, edit: impl Fn(&mut local::CaKey)) {
    let path = site.durable(local::ca_key_path(&identity().office_id).as_str());
    let mut key = local::decode_ca_key(&std::fs::read(&path).unwrap()).unwrap();
    edit(&mut key);
    std::fs::write(&path, local::encode_ca_key(&key)).unwrap();
}

/// M1: an HQ whose CA records are missing or disagree is not opened, and a creation retry
/// generates nothing in their place.
#[test]
fn opening_refuses_missing_or_mismatched_ca_records() {
    let escrow = "state/security/ca-escrow.v1.bin";
    let cases: Vec<(&str, Spoil)> = vec![
        (
            "no escrow package",
            Box::new(|site| std::fs::remove_file(site.durable(escrow)).unwrap()),
        ),
        (
            "no key record",
            Box::new(|site| {
                let key = local::ca_key_path(&identity().office_id);
                std::fs::remove_file(site.durable(key.as_str())).unwrap();
            }),
        ),
        (
            "another package",
            Box::new(|site| {
                let mut bytes = std::fs::read(site.durable(escrow)).unwrap();
                *bytes.last_mut().unwrap() ^= 1;
                std::fs::write(site.durable(escrow), bytes).unwrap();
            }),
        ),
        (
            "another public key",
            Box::new(|site| {
                let other = ca_material(&identity().organization_id, 5).public_key;
                rewrite_key(site, |key| key.public_key = other);
            }),
        ),
        (
            "another organization",
            Box::new(|site| {
                rewrite_key(site, |key| {
                    key.organization_id = org_store::testkit::id(0xB2);
                });
            }),
        ),
        (
            "another operation",
            Box::new(|site| {
                rewrite_key(site, |key| key.operation_id = org_store::testkit::id(0x99));
            }),
        ),
    ];
    for (case, spoil) in cases {
        let site = Site::new();
        site.provision(EngineChoice::Sqlite)
            .unwrap()
            .close()
            .unwrap();
        spoil(&site);
        let refused = site.open(EngineChoice::Sqlite).unwrap_err();
        assert!(
            matches!(
                refused,
                OfficeError::Invalid("HQ provisioning requires recovery")
            ),
            "{case}: {refused:?}"
        );
        // A creation retry neither generates a CA in their place nor hands records that
        // disagree to `material`.
        let called = Cell::new(false);
        let retried = site.provision_with(EngineChoice::Sqlite, |prior| {
            called.set(true);
            prior.map_or_else(|| testkit::record(9), Ok)
        });
        assert!(retried.is_err(), "{case}");
        assert!(!called.get(), "{case}");
    }
}

/// An office folder the old databases made is refused by both ways in, never migrated.
#[test]
fn an_office_made_by_an_older_build_is_refused() {
    let older = |error: &OfficeError| {
        matches!(
            error,
            OfficeError::Invalid("this office was made by an older build")
        )
    };
    for database in ["org.db", "office.db"] {
        let site = Site::new();
        site.provision(EngineChoice::Memory)
            .unwrap()
            .close()
            .unwrap();
        std::fs::write(site.folder.join(database), b"SQLite format 3\0").unwrap();
        let refused = site.open(EngineChoice::Memory).unwrap_err();
        assert!(older(&refused), "{database}: {refused:?}");
        let refused = site.provision(EngineChoice::Memory).unwrap_err();
        assert!(older(&refused), "{database}: {refused:?}");
    }
    // An intent of the old version too.
    let site = Site::new();
    std::fs::create_dir_all(&site.folder).unwrap();
    std::fs::write(site.folder.join(intent::FILE), br#"{"version":1}"#).unwrap();
    let refused = site.provision(EngineChoice::Memory).unwrap_err();
    assert!(older(&refused), "{refused:?}");
}

/// The intent binds the office: another request for it, or opening it with other roots or
/// another identity, is refused; the same request again completes.
#[test]
fn the_intent_binds_the_request_the_roots_and_the_identity() {
    let site = Site::new();
    site.provision(EngineChoice::Memory)
        .unwrap()
        .close()
        .unwrap();
    let (new, mut binding) = testkit::request();
    binding.operation_id = "019a0000-0000-7000-8000-000000009999".into();
    let other = Office::provision_with(
        site.folder.clone(),
        &new,
        None,
        &binding,
        site.roots.clone(),
        testkit::custody(Arc::clone(&site.custody)),
        |prior| prior.map_or_else(|| testkit::record(1), Ok),
        site.options(EngineChoice::Memory),
    );
    assert!(matches!(other, Err(OfficeError::OfficeExists)), "{other:?}");

    let instance = identity().instance_id.to_string();
    let open = |roots: DataRoots, custody: Custody| {
        Office::open_with(
            site.folder.clone(),
            None,
            &instance,
            roots,
            custody,
            site.options(EngineChoice::Memory),
        )
    };
    let custody = || testkit::custody(Arc::clone(&site.custody));
    let unbound = |refused: Result<Office, OfficeError>| {
        matches!(
            refused,
            Err(OfficeError::Invalid(
                "the office was created with other roots or another identity"
            ))
        )
    };
    // Other roots, that exist.
    let elsewhere = DataRoots {
        durable: site.root.join("elsewhere/durable"),
        runtime: site.roots.runtime.clone(),
    };
    std::fs::create_dir_all(&elsewhere.durable).unwrap();
    assert!(unbound(open(elsewhere, custody())));
    // Another organization in the custody.
    let mut stranger = custody();
    stranger.identity.organization_id = org_store::testkit::id(0xB2);
    assert!(unbound(open(site.roots.clone(), stranger)));
    // Another instance, the custody's too.
    let other_instance = org_store::testkit::id(0xC1);
    let mut moved = custody();
    moved.identity.instance_id = other_instance.clone();
    let refused = Office::open_with(
        site.folder.clone(),
        None,
        other_instance.as_str(),
        site.roots.clone(),
        moved,
        site.options(EngineChoice::Memory),
    );
    assert!(unbound(refused));
    // An office.json edited after the creation.
    let saved = std::fs::read(site.folder.join("office.json")).unwrap();
    let mut edited = office_file::read(&site.folder).unwrap().unwrap();
    edited.office_name = "Renamed HQ".into();
    let bytes = office_file::encode(&edited).unwrap();
    org_store::publish_one(&site.folder, "office.json", &bytes, false).unwrap();
    assert!(unbound(open(site.roots.clone(), custody())));
    std::fs::write(site.folder.join("office.json"), saved).unwrap();
    site.provision(EngineChoice::Memory)
        .unwrap()
        .close()
        .unwrap();
}

/// `fatal` reports an injected flush error, with its path.
#[test]
fn fatal_reports_a_flush_error() {
    for engine in ENGINES {
        let site = Site::new();
        let office = site.provision(engine).unwrap();
        assert_eq!(office.fatal(), None);
        site.faults
            .arm(CrashPoint::FlushError(FlushSite::Segment), 1);
        let refused = office.submit_org(CREATOR, team_create(1, TEAM_A, TOP_TEAM));
        assert!(refused.is_err(), "{engine:?}: {refused:?}");
        let fatal = office.fatal().expect("stopped");
        assert!(fatal.starts_with("the office store stopped: "), "{fatal}");
        assert!(fatal.contains(".log"), "{fatal}");
        assert!(office.close().is_err());
    }
}

/// ENC-5 + TASK-10 (H-06) in process: neither the projection nor any file under `state/`,
/// `runtime/`, or `durable/recovery/` (acceptance journal segments & receipts) holds the
/// wrapped CA key, raw or in hex.
#[test]
fn the_projection_holds_no_form_of_the_ca_key() {
    for engine in ENGINES {
        let site = Site::new();
        let office = site.provision(engine).unwrap();
        let wrapped = ca_material(&identity().organization_id, testkit::CA_SEED).wrapped_ca_key;
        let hex: String = wrapped.iter().map(|b| format!("{b:02x}")).collect();
        let holds = |bytes: &[u8]| {
            bytes
                .windows(wrapped.len())
                .any(|w| w == wrapped.as_slice())
                || bytes.windows(hex.len()).any(|w| w == hex.as_bytes())
        };
        assert!(!holds(office.dump().join("\n").as_bytes()), "{engine:?}");
        office.close().unwrap();
        let runtime = tree(&site.roots.runtime);
        for (path, bytes) in runtime
            .into_iter()
            .chain(tree(&site.durable("state")))
            .chain(tree(&site.durable("recovery")))
        {
            assert!(!holds(&bytes), "{engine:?}: {path}");
        }
    }
}

/// All implemented organization-family commands commit through `organization::records`;
/// unbuilt HR/project commands return `unsupported`; `who` and `org_snapshot` omit archived
/// teams and deactivated people.
#[test]
fn the_office_wires_all_implemented_organization_operations_and_refuses_unbuilt_domains() {
    use org_proto::org::{
        MemberAddPersonParams, MemberMoveParams, MemberRemoveParams, MemberSetReportsToParams,
        MemberUpdatePersonParams, TeamDeleteParams, TeamMoveParams, TeamSetLeadParams,
        TeamSetTitleParams,
    };
    const PERSON_A: &str = "019a0000-0000-7000-8000-0000000000a1";
    let cmd = |n: u32| format!("018bce27-f800-7000-8000-{n:012x}");
    let office = Office::for_test(TestOffice::default());
    // 1. Create two teams under TOP_TEAM.
    let created = office
        .submit_org(CREATOR, team_create(1, TEAM_A, TOP_TEAM))
        .unwrap()
        .unwrap();
    assert_eq!(created.position.map(|p| p.seq), Some(2));
    office
        .submit_org(CREATOR, team_create(2, TEAM_B, TOP_TEAM))
        .unwrap()
        .unwrap();
    // Non-owner is still refused.
    let refused = office
        .submit_org(
            HR,
            team_create(3, "019a0000-0000-7000-8000-0000000000c3", TOP_TEAM),
        )
        .unwrap();
    assert_eq!(refused.unwrap_err().code, ErrorCode::NotAuthorized);
    // 2. Rename TEAM_A.
    let renamed = office
        .submit_org(
            CREATOR,
            OrgCommand::TeamRename(TeamRenameParams {
                command_id: cmd(300),
                team_id: TEAM_A.into(),
                expected_version: 1,
                name: "Platform".into(),
            }),
        )
        .unwrap()
        .unwrap();
    assert_eq!(renamed.position.map(|p| p.seq), Some(4));
    // 3. Add a person directly into TEAM_A, update their profile/title/manager, appoint as lead.
    office
        .submit_org(
            CREATOR,
            OrgCommand::MemberAddPerson(MemberAddPersonParams {
                command_id: cmd(301),
                member_id: PERSON_A.into(),
                name: "Ada Lovelace".into(),
                contact: Some("ada@example.test".into()),
                handle: Some("ada".into()),
                home_team_id: Some(TEAM_A.into()),
            }),
        )
        .unwrap()
        .unwrap();
    office
        .submit_org(
            CREATOR,
            OrgCommand::MemberUpdatePerson(MemberUpdatePersonParams {
                command_id: cmd(302),
                member_id: PERSON_A.into(),
                expected_version: 1,
                name: Some("Ada Byron".into()),
                contact: None,
                clear_contact: true,
            }),
        )
        .unwrap()
        .unwrap();
    office
        .submit_org(
            CREATOR,
            OrgCommand::TeamSetTitle(TeamSetTitleParams {
                command_id: cmd(303),
                membership_id: PERSON_A.into(),
                expected_version: 2,
                title_in_team: Some("Principal Engineer".into()),
            }),
        )
        .unwrap()
        .unwrap();
    office
        .submit_org(
            CREATOR,
            OrgCommand::MemberSetReportsTo(MemberSetReportsToParams {
                command_id: cmd(304),
                member_id: PERSON_A.into(),
                expected_version: 3,
                reports_to_member_id: Some(CREATOR.into()),
            }),
        )
        .unwrap()
        .unwrap();
    office
        .submit_org(
            CREATOR,
            OrgCommand::TeamSetLead(TeamSetLeadParams {
                command_id: cmd(305),
                team_id: TEAM_A.into(),
                expected_version: 2,
                lead_member_id: Some(PERSON_A.into()),
            }),
        )
        .unwrap()
        .unwrap();
    // Clear lead before moving person to TEAM_B, then deactivate person and archive empty TEAM_A.
    office
        .submit_org(
            CREATOR,
            OrgCommand::TeamSetLead(TeamSetLeadParams {
                command_id: cmd(306),
                team_id: TEAM_A.into(),
                expected_version: 3,
                lead_member_id: None,
            }),
        )
        .unwrap()
        .unwrap();
    office
        .submit_org(
            CREATOR,
            OrgCommand::MemberMove(MemberMoveParams {
                command_id: cmd(307),
                membership_id: PERSON_A.into(),
                expected_version: 4,
                new_membership_id: PERSON_A.into(),
                to_team_id: TEAM_B.into(),
                title_in_team: None,
            }),
        )
        .unwrap()
        .unwrap();
    // Before deactivation, PERSON_A is active in TEAM_B and visible in snapshot & directory.
    let mid = office.org_snapshot().unwrap();
    assert!(mid
        .members
        .iter()
        .any(|m| m.id == PERSON_A && m.home_team_id.as_deref() == Some(TEAM_B)));
    office
        .submit_org(
            CREATOR,
            OrgCommand::TeamMove(TeamMoveParams {
                command_id: cmd(308),
                team_id: TEAM_A.into(),
                expected_version: 4,
                owner_team_id: Some(TEAM_B.into()),
            }),
        )
        .unwrap()
        .unwrap();
    office
        .submit_org(
            CREATOR,
            OrgCommand::TeamDelete(TeamDeleteParams {
                command_id: cmd(309),
                team_id: TEAM_A.into(),
                expected_version: 5,
            }),
        )
        .unwrap()
        .unwrap();
    office
        .submit_org(
            CREATOR,
            OrgCommand::MemberRemove(MemberRemoveParams {
                command_id: cmd(310),
                member_id: PERSON_A.into(),
                expected_version: 5,
            }),
        )
        .unwrap()
        .unwrap();
    // Unbuilt HR agent domain still answers Unsupported.
    let unbuilt = office
        .submit_org(
            CREATOR,
            OrgCommand::MemberOffboard(org_proto::org::MemberOffboardParams {
                command_id: cmd(311),
                member_id: "019a0000-0000-7000-8000-0000000000d2".into(),
                expected_version: 1,
            }),
        )
        .unwrap();
    assert_eq!(unbuilt.unwrap_err().code, ErrorCode::Unsupported);

    let snapshot = office.org_snapshot().unwrap();
    let owners: Vec<&str> = snapshot
        .members
        .iter()
        .filter(|m| m.owner)
        .map(|m| m.id.as_str())
        .collect();
    assert_eq!(owners, [CREATOR]);
    // Archived TEAM_A and deactivated PERSON_A are excluded from snapshot and directory.
    assert!(!snapshot.teams.iter().any(|t| t.id == TEAM_A));
    assert!(!snapshot.members.iter().any(|m| m.id == PERSON_A));
    let who = office.who(&WhoParams::default()).unwrap().unwrap();
    assert!(who.members.iter().any(|c| c.id == CREATOR));
    assert!(!who.members.iter().any(|c| c.id == PERSON_A));
    office.close().unwrap();
}

/// `discard` removes an unfinished create's folder only: one with an intent or
/// `office.json` is an office, and one with anything else is not touched.
#[test]
fn discard_removes_only_an_unfinished_create() {
    let site = Site::new();
    let office_id = identity().office_id.to_string();
    // No folder: nothing to do.
    Office::discard(&site.folder, &office_id).unwrap();
    // A folder with only its lock and a temporary file: removed.
    std::fs::create_dir_all(&site.folder).unwrap();
    std::fs::write(site.folder.join("office.lock"), b"").unwrap();
    std::fs::write(site.folder.join(".store-tmp-one-1-1"), b"partial").unwrap();
    Office::discard(&site.folder, &office_id).unwrap();
    assert!(!site.folder.exists());
    // Something else: refused, kept.
    std::fs::create_dir_all(&site.folder).unwrap();
    std::fs::write(site.folder.join("notes.txt"), b"mine").unwrap();
    let refused = Office::discard(&site.folder, &office_id);
    assert!(
        matches!(refused, Err(OfficeError::Invalid(_))),
        "{refused:?}"
    );
    assert!(site.folder.join("notes.txt").is_file());

    // The host's custody marker, or a file it stages one in: the host reserved a writer,
    // so the folder is an office, kept with everything in it.
    for host in ["selected-writer.required", ".selected-writer-0190.tmp"] {
        let site = Site::new();
        std::fs::create_dir_all(&site.folder).unwrap();
        std::fs::write(site.folder.join("office.lock"), b"").unwrap();
        std::fs::write(site.folder.join(".store-tmp-one-1-1"), b"partial").unwrap();
        std::fs::write(site.folder.join(host), b"selected-writer/1\n").unwrap();
        let refused = Office::discard(&site.folder, &office_id);
        assert!(
            matches!(refused, Err(OfficeError::OfficeExists)),
            "{host}: {refused:?}"
        );
        assert!(site.folder.join(host).is_file(), "{host}");
        assert!(site.folder.join(".store-tmp-one-1-1").is_file(), "{host}");
    }

    // An unfinished create's intent, and a finished office: offices.
    let site = Site::new();
    let died = site.provision_with(EngineChoice::Memory, |prior| {
        site.faults.arm(CrashPoint::DuringDecide, 1);
        prior.map_or_else(|| testkit::record(testkit::CA_SEED), Ok)
    });
    assert!(died.is_err());
    site.faults.revive();
    let refused = Office::discard(&site.folder, &office_id);
    assert!(
        matches!(refused, Err(OfficeError::OfficeExists)),
        "{refused:?}"
    );
    site.provision(EngineChoice::Memory)
        .unwrap()
        .close()
        .unwrap();
    let refused = Office::discard(&site.folder, &office_id);
    assert!(
        matches!(refused, Err(OfficeError::OfficeExists)),
        "{refused:?}"
    );
    // The data roots are never touched.
    assert!(site.durable("state/organization.yaml").is_file());
}

/// The office folder is never reached through a link.
#[test]
fn an_office_folder_that_is_a_link_is_refused() {
    let site = Site::new();
    let real = site.root.join("real").join(identity().office_id.as_str());
    std::fs::create_dir_all(&real).unwrap();
    std::fs::create_dir_all(site.folder.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&real, &site.folder).unwrap();
    let refused = site.provision(EngineChoice::Memory);
    assert!(
        matches!(refused, Err(OfficeError::Invalid(_))),
        "{refused:?}"
    );
    assert_eq!(std::fs::read_dir(&real).unwrap().count(), 0);
}

/// An unfinished create's folder: only its lock file and a publication's temporary file,
/// which `discard` would remove.
fn unfinished_create(folder: &Path) {
    std::fs::create_dir_all(folder).unwrap();
    std::fs::write(folder.join("office.lock"), b"").unwrap();
    std::fs::write(folder.join(".store-tmp-one-1-1"), b"partial").unwrap();
}

/// `discard` takes only a UUIDv7 office id, and only for the folder that id names: an
/// unfinished create's folder under any other name is refused and kept.
#[test]
fn discard_refuses_an_id_that_is_not_a_uuidv7_or_does_not_name_the_folder() {
    let site = Site::new();
    let offices = site.folder.parent().unwrap().to_path_buf();

    // A plain id, naming its folder: refused for the id.
    let plain = offices.join("hq");
    unfinished_create(&plain);
    let refused = Office::discard(&plain, "hq");
    assert!(
        matches!(refused, Err(OfficeError::Invalid(why)) if why.contains("UUIDv7")),
        "{refused:?}"
    );
    assert!(plain.join(".store-tmp-one-1-1").is_file());

    // A UUIDv7 that is not the folder's name: refused for the name.
    unfinished_create(&site.folder);
    let other = "01900000-0000-7000-8000-0000000000ff";
    let refused = Office::discard(&site.folder, other);
    assert!(
        matches!(refused, Err(OfficeError::Invalid(why)) if why.contains("not named by")),
        "{refused:?}"
    );
    assert!(site.folder.join(".store-tmp-one-1-1").is_file());
}

/// `discard` never removes a folder another process holds locked: it waits for the lock,
/// gives up with `InUse` and leaves everything in place.
#[test]
fn discard_gives_up_on_a_held_lock() {
    let site = Site::new();
    let office_id = identity().office_id.to_string();
    unfinished_create(&site.folder);
    let held = lock_folder(&site.folder, LOCK_WAIT).unwrap();
    let refused = Office::discard(&site.folder, &office_id);
    assert!(matches!(refused, Err(OfficeError::InUse)), "{refused:?}");
    assert!(site.folder.join(".store-tmp-one-1-1").is_file());
    drop(held);
    Office::discard(&site.folder, &office_id).unwrap();
    assert!(!site.folder.exists());
}

/// Opening a folder that is not there is `NotFound` and makes nothing: no office folder,
/// no lock file, nothing in the data roots. A file where the folder should be is refused.
#[test]
fn opening_a_missing_folder_is_not_found_and_creates_nothing() {
    let site = Site::new();
    let before = tree(&site.root);
    let missing = site.open(EngineChoice::Memory);
    assert!(matches!(missing, Err(OfficeError::NotFound)), "{missing:?}");
    assert!(!site.folder.exists());
    assert!(!site.folder.parent().unwrap().exists());
    assert_eq!(tree(&site.root), before);
    assert!(std::fs::read_dir(&site.roots.durable)
        .unwrap()
        .next()
        .is_none());
    assert!(std::fs::read_dir(&site.roots.runtime)
        .unwrap()
        .next()
        .is_none());

    std::fs::create_dir_all(site.folder.parent().unwrap()).unwrap();
    std::fs::write(&site.folder, b"not a folder").unwrap();
    let refused = site.open(EngineChoice::Memory);
    assert!(
        matches!(
            refused,
            Err(OfficeError::Invalid(
                "the office folder is not a plain folder"
            ))
        ),
        "{refused:?}"
    );
    assert_eq!(std::fs::read(&site.folder).unwrap(), b"not a folder");
}

/// An `office.json` must name the folder it is in: a finished office moved under another
/// office id's name is refused, even though its intent, roots and custody all agree.
#[test]
fn opening_refuses_an_office_json_that_does_not_name_its_folder() {
    let site = Site::new();
    site.provision(EngineChoice::Memory)
        .unwrap()
        .close()
        .unwrap();
    let moved = site
        .folder
        .with_file_name("01900000-0000-7000-8000-0000000000ff");
    std::fs::rename(&site.folder, &moved).unwrap();
    let instance = identity().instance_id.to_string();
    let refused = Office::open_with(
        moved.clone(),
        None,
        &instance,
        site.roots.clone(),
        testkit::custody(Arc::clone(&site.custody)),
        site.options(EngineChoice::Memory),
    );
    assert!(
        matches!(refused, Err(OfficeError::Invalid(why)) if why.contains("not named by")),
        "{refused:?}"
    );
    // Back under its own name, the same office opens.
    std::fs::rename(&moved, &site.folder).unwrap();
    site.open(EngineChoice::Memory).unwrap().close().unwrap();
}

/// One office process at a time: a second opening waits for the lock and gives up.
#[test]
fn a_second_opening_waits_for_the_lock_and_gives_up() {
    let site = Site::new();
    let office = site.provision(EngineChoice::Memory).unwrap();
    let second = site.open(EngineChoice::Memory);
    assert!(matches!(second, Err(OfficeError::InUse)), "{second:?}");
    office.close().unwrap();
    site.open(EngineChoice::Memory).unwrap().close().unwrap();
}

/// The creation's operation on disk is the native UUIDv7, and a legacy ULID record is refused.
#[test]
fn the_creation_operation_on_disk_is_the_native_uuidv7() {
    let site = Site::new();
    let office = site.provision(EngineChoice::Memory).unwrap();
    let record = office.hq_creation_record().unwrap().unwrap();
    assert_eq!(record.operation_id, creation_id().as_str());
    assert!(org_proto::is_uuid_v7(&record.operation_id));
    let ulid = LocalRecord::new(
        identity().organization_id.to_string(),
        "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        record.public_key,
        record.package.clone(),
        record.wrapped_ca_key().to_vec(),
    );
    assert!(ulid.is_err());
    office.close().unwrap();
}

fn edit_incarnation(custody: &mut Custody) {
    custody.identity.journal_incarnation = org_store::testkit::id(0xD4);
}

fn edit_epoch(custody: &mut Custody) {
    custody.identity.writer_epoch = 2;
}

fn edit_machine(custody: &mut Custody) {
    custody.identity.machine_id = org_store::testkit::id(0xD5);
}

/// A retry under another custody reservation is another request, and a journal never
/// begins where another incarnation of the office's journal is.
#[test]
fn a_creation_never_begins_a_second_journal() {
    // A first attempt dies after the anchor; retries under another reservation are refused.
    let site = Site::new();
    let died = site.provision_with(EngineChoice::Memory, |prior| {
        site.faults.arm(CrashPoint::DuringDecide, 1);
        prior.map_or_else(|| testkit::record(testkit::CA_SEED), Ok)
    });
    assert!(died.is_err());
    site.faults.revive();
    let (new, binding) = testkit::request();
    let edits: [fn(&mut Custody); 3] = [edit_incarnation, edit_epoch, edit_machine];
    for edit in edits {
        let mut custody = testkit::custody(Arc::clone(&site.custody));
        edit(&mut custody);
        let refused = Office::provision_with(
            site.folder.clone(),
            &new,
            None,
            &binding,
            site.roots.clone(),
            custody,
            |prior| prior.map_or_else(|| testkit::record(1), Ok),
            site.options(EngineChoice::Memory),
        );
        assert!(
            matches!(refused, Err(OfficeError::OfficeExists)),
            "{refused:?}"
        );
    }
    let ours = org_store::journal_dir(&site.roots.durable, &identity());
    let journals = ours.parent().unwrap();
    assert_eq!(
        names(journals),
        [identity().journal_incarnation.to_string()]
    );
    site.provision(EngineChoice::Memory)
        .unwrap()
        .close()
        .unwrap();

    // Another incarnation's folder already there: no journal begins beside it.
    let site = Site::new();
    let mut other = identity();
    other.journal_incarnation = org_store::testkit::id(0xD4);
    std::fs::create_dir_all(org_store::journal_dir(&site.roots.durable, &other)).unwrap();
    let refused = site.provision(EngineChoice::Memory);
    assert!(
        matches!(
            refused,
            Err(OfficeError::Invalid(
                "the data roots hold another journal of this office"
            ))
        ),
        "{refused:?}"
    );
    let ours = org_store::journal_dir(&site.roots.durable, &identity());
    assert!(!ours.join("anchor.json").exists());
}

/// A fresh create refuses a folder in which an office has begun (an older build's files)
/// or that holds something neither an office nor the host makes there (a folder, a link).
/// Nothing is frozen then.
#[test]
fn a_fresh_create_refuses_a_folder_an_office_has_begun_in() {
    let older = OfficeError::Invalid("this office was made by an older build");
    let foreign = OfficeError::Invalid("the folder holds something an office does not make");
    let cases: Vec<(&str, Spoil, &OfficeError)> = vec![
        (
            "an older build's log",
            Box::new(|site| std::fs::write(site.folder.join("org.db-wal"), b"log").unwrap()),
            &older,
        ),
        (
            "an older build's unfinished office.json",
            Box::new(|site| std::fs::write(site.folder.join("office.json.tmp"), b"{").unwrap()),
            &older,
        ),
        (
            "an older build's workspace",
            Box::new(|site| std::fs::create_dir(site.folder.join("workspace")).unwrap()),
            &older,
        ),
        (
            "an unknown folder",
            Box::new(|site| std::fs::create_dir(site.folder.join("notes")).unwrap()),
            &foreign,
        ),
        (
            "a link",
            Box::new(|site| {
                std::os::unix::fs::symlink(&site.root, site.folder.join("elsewhere")).unwrap()
            }),
            &foreign,
        ),
    ];
    for (case, spoil, expected) in cases {
        let site = Site::new();
        std::fs::create_dir_all(&site.folder).unwrap();
        spoil(&site);
        let refused = site.provision(EngineChoice::Memory).unwrap_err();
        assert_eq!(refused.to_string(), expected.to_string(), "{case}");
        assert!(matches!(refused, OfficeError::Invalid(_)), "{case}");
        assert!(!site.folder.join(intent::FILE).exists(), "{case}");
        let anchor = org_store::journal_dir(&site.roots.durable, &identity());
        assert!(!anchor.join("anchor.json").exists(), "{case}");
    }
}

/// The host installs its custody marker in the office folder before it asks for the
/// create, and a host that died while installing one may leave its staging file: a create
/// finds both, keeps both, and completes. Other plain files are left alone too.
#[test]
fn a_create_completes_beside_the_hosts_custody_marker() {
    let site = Site::new();
    std::fs::create_dir_all(&site.folder).unwrap();
    let host_files: [(&str, &[u8]); 4] = [
        ("selected-writer.required", b"selected-writer/1\n"),
        (".selected-writer-0190.tmp", b"selected-writer/1\n"),
        (".store-tmp-one-9-9", b"partial"),
        ("notes.txt", b"mine"),
    ];
    for (name, bytes) in host_files {
        std::fs::write(site.folder.join(name), bytes).unwrap();
    }
    site.provision(EngineChoice::Memory)
        .unwrap()
        .close()
        .unwrap();
    assert!(site.folder.join("office.json").is_file());
    for (name, bytes) in host_files {
        assert_eq!(
            std::fs::read(site.folder.join(name)).unwrap(),
            bytes,
            "{name}"
        );
    }
    // And the office opens with the marker there.
    site.open(EngineChoice::Memory).unwrap().close().unwrap();
}

/// The checks just before `office.json` (the office's standing and its CA records) run
/// after the creation is durable: records spoiled there stop the office from being
/// published.
#[test]
fn the_final_checks_stop_office_json() {
    let site = Site::new();
    let (new, binding) = testkit::request();
    let mut options = site.options(EngineChoice::Sqlite);
    let key = site.durable(local::ca_key_path(&identity().office_id).as_str());
    options.before_final_checks = Some(Arc::new(move || {
        std::fs::remove_file(&key).unwrap();
    }));
    let refused = Office::provision_with(
        site.folder.clone(),
        &new,
        Some(Key::from_bytes([7; 32])),
        &binding,
        site.roots.clone(),
        testkit::custody(Arc::clone(&site.custody)),
        |prior| prior.map_or_else(|| testkit::record(testkit::CA_SEED), Ok),
        options,
    )
    .unwrap_err();
    assert!(
        matches!(
            refused,
            OfficeError::Invalid("HQ provisioning requires recovery")
        ),
        "{refused:?}"
    );
    // The creation itself is durable; only the office is not published.
    assert!(site.durable("state/organization.yaml").is_file());
    assert!(!site.folder.join("office.json").exists());
}

/// An intent of version 2, from before it bound the custody reservation, is an older
/// build's.
#[test]
fn an_intent_of_version_two_is_an_older_builds() {
    let site = Site::new();
    let died = site.provision_with(EngineChoice::Memory, |prior| {
        site.faults.arm(CrashPoint::DuringDecide, 1);
        prior.map_or_else(|| testkit::record(testkit::CA_SEED), Ok)
    });
    assert!(died.is_err());
    site.faults.revive();
    let path = site.folder.join(intent::FILE);
    let mut frozen: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    frozen["version"] = 2.into();
    std::fs::write(&path, serde_json::to_vec(&frozen).unwrap()).unwrap();
    let refused = site.provision(EngineChoice::Memory).unwrap_err();
    assert!(
        matches!(
            refused,
            OfficeError::Invalid("this office was made by an older build")
        ),
        "{refused:?}"
    );
}

/// The intent records the reservation and is dated by the office's clock.
#[test]
fn the_intent_records_the_reservation_and_the_offices_clock() {
    let site = Site::new();
    site.provision(EngineChoice::Memory)
        .unwrap()
        .close()
        .unwrap();
    let frozen = intent::read(&site.folder).unwrap().unwrap();
    let at = crate::organization::testkit::CREATION_AT;
    assert_eq!(
        frozen.created_at,
        crate::organization::records::time_text(at)
    );
    let here = identity();
    assert_eq!(
        frozen.journal_incarnation,
        here.journal_incarnation.as_str()
    );
    assert_eq!(
        (frozen.writer_epoch, frozen.machine_id.as_str()),
        (1, here.machine_id.as_str())
    );
}

/// A creation checks its whole request before it touches anything: a bad ID or name, or a
/// folder not named by the office, makes nothing.
#[test]
fn a_creation_checks_its_request_before_touching_anything() {
    let site = Site::new();
    let (good, binding) = testkit::request();
    let make = |folder: PathBuf, change: &dyn Fn(&mut crate::NewOffice)| {
        let mut new = good.clone();
        change(&mut new);
        Office::provision_with(
            folder,
            &new,
            None,
            &binding,
            site.roots.clone(),
            testkit::custody(Arc::clone(&site.custody)),
            |prior| prior.map_or_else(|| testkit::record(testkit::CA_SEED), Ok),
            site.options(EngineChoice::Memory),
        )
    };
    let folder = || site.folder.clone();
    for result in [
        make(folder(), &|n| n.office_id = "has-dash".into()),
        make(folder(), &|n| n.office_id = String::new()),
        make(folder(), &|n| n.org_id = "../up".into()),
        make(site.folder.with_file_name("OTHER"), &|_| {}),
    ] {
        assert!(matches!(result, Err(OfficeError::Invalid(_))), "{result:?}");
    }
    // A bad name is `InvalidName` (the host answers `invalid_name`), by the contract's rule:
    // a format character such as U+200B is refused too.
    for name in ["   ", "bad\u{7}name", "zero\u{200B}width"] {
        let result = make(folder(), &|n| n.office_name = name.into());
        assert!(
            matches!(result, Err(OfficeError::InvalidName(_))),
            "{name:?}: {result:?}"
        );
    }
    assert!(
        !site.folder.parent().unwrap().exists(),
        "something was made"
    );
    assert_eq!(tree(&site.roots.durable), BTreeMap::new());
}

/// The office's name is trimmed into `office.json`.
#[test]
fn the_office_name_is_trimmed_into_office_json() {
    let site = Site::new();
    let (mut new, binding) = testkit::request();
    new.office_name = format!("  {} ", new.office_name);
    let office = Office::provision_with(
        site.folder.clone(),
        &new,
        None,
        &binding,
        site.roots.clone(),
        testkit::custody(Arc::clone(&site.custody)),
        |prior| prior.map_or_else(|| testkit::record(testkit::CA_SEED), Ok),
        site.options(EngineChoice::Memory),
    )
    .unwrap();
    assert_eq!(office.info().office_name, new.office_name.trim());
    let written = office_file::read(&site.folder).unwrap().unwrap();
    assert_eq!(written.office_name, new.office_name.trim());
    office.close().unwrap();
}

/// A creation makes its folder owner-only, an existing loose one included.
#[test]
fn a_loose_office_folder_is_tightened_by_creation() {
    use std::os::unix::fs::PermissionsExt;
    let site = Site::new();
    std::fs::create_dir_all(&site.folder).unwrap();
    std::fs::set_permissions(&site.folder, std::fs::Permissions::from_mode(0o755)).unwrap();
    site.provision(EngineChoice::Memory)
        .unwrap()
        .close()
        .unwrap();
    let mode = std::fs::metadata(&site.folder)
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700);
}

/// The office keeps the creation's native UUIDv7 command ID at creation and after reopening,
/// matching the on-disk record without lossy conversion.
#[test]
fn the_office_keeps_the_creations_command_id_as_sent() {
    let site = Site::new();
    let office = site.provision(EngineChoice::Memory).unwrap();
    assert_eq!(office.creation_command_id(), OPERATION);
    office.close().unwrap();
    let office = site.open(EngineChoice::Memory).unwrap();
    assert_eq!(office.creation_command_id(), OPERATION);
    assert_eq!(
        office.hq_creation_record().unwrap().unwrap().operation_id,
        OPERATION
    );
    office.close().unwrap();
}

#[test]
fn dry_run_command_id_window_and_operation_lookup_enforce_b2_rules() {
    use org_proto::OperationOutcome;

    let uuid_at = |ms: u64, seq: u64| -> String {
        let hi = (ms >> 16) as u32;
        let mid = (ms & 0xffff) as u16;
        format!("{hi:08x}-{mid:04x}-7000-8000-{seq:012x}")
    };

    for engine in ENGINES {
        let site = Site::new();
        let office = site.provision(engine).unwrap();
        let now = crate::organization::testkit::CREATION_AT.0;
        let before = office.org_snapshot().unwrap();

        // 1. Dry-run succeeds with Checked + position: None, leaving snapshot and store untouched.
        let dry_ok = office
            .check_org(
                CREATOR,
                OrgCommand::TeamCreate(TeamCreateParams {
                    command_id: String::new(),
                    team_id: TEAM_A.into(),
                    name: "Dry Team".into(),
                    owner_team_id: Some(TOP_TEAM.into()),
                }),
            )
            .unwrap()
            .unwrap();
        assert_eq!(dry_ok.status, CommandStatus::Checked);
        assert_eq!(dry_ok.position, None);
        assert_eq!(office.org_snapshot().unwrap(), before);

        // Dry-run refuses a non-empty command_id and reports domain refusals without writing.
        let dry_with_id = office
            .check_org(
                CREATOR,
                OrgCommand::TeamCreate(TeamCreateParams {
                    command_id: uuid_at(now, 1),
                    team_id: TEAM_A.into(),
                    name: "Dry Team".into(),
                    owner_team_id: Some(TOP_TEAM.into()),
                }),
            )
            .unwrap()
            .unwrap_err();
        assert_eq!(dry_with_id.code, ErrorCode::InvalidRequest);

        let dry_bad_name = office
            .check_org(
                CREATOR,
                OrgCommand::TeamCreate(TeamCreateParams {
                    command_id: String::new(),
                    team_id: TEAM_A.into(),
                    name: "   ".into(),
                    owner_team_id: Some(TOP_TEAM.into()),
                }),
            )
            .unwrap()
            .unwrap_err();
        assert_eq!(dry_bad_name.code, ErrorCode::InvalidName);
        assert_eq!(office.org_snapshot().unwrap(), before);

        // 2. Unknown command_id outside the [-29d, +5m] window is refused unrecorded.
        let expired_id = uuid_at(now - wire::MAX_COMMAND_AGE_MS - 1, 2);
        let expired = office
            .submit_org(
                CREATOR,
                OrgCommand::TeamCreate(TeamCreateParams {
                    command_id: expired_id.clone(),
                    team_id: TEAM_A.into(),
                    name: "Too Old".into(),
                    owner_team_id: Some(TOP_TEAM.into()),
                }),
            )
            .unwrap()
            .unwrap_err();
        assert_eq!(expired.code, ErrorCode::Expired);
        assert_eq!(
            office
                .operation(CREATOR, &expired_id)
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );

        let skewed_id = uuid_at(now + wire::MAX_COMMAND_SKEW_MS + 1, 3);
        let skewed = office
            .submit_org(
                CREATOR,
                OrgCommand::TeamCreate(TeamCreateParams {
                    command_id: skewed_id.clone(),
                    team_id: TEAM_A.into(),
                    name: "Too Far Ahead".into(),
                    owner_team_id: Some(TOP_TEAM.into()),
                }),
            )
            .unwrap()
            .unwrap_err();
        assert_eq!(skewed.code, ErrorCode::ClockSkew);
        assert_eq!(
            office
                .operation(CREATOR, &skewed_id)
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );

        // Boundary IDs at exact -29d and +5m are accepted.
        let committed_id = uuid_at(now - wire::MAX_COMMAND_AGE_MS, 4);
        let committed = office
            .submit_org(
                CREATOR,
                OrgCommand::TeamCreate(TeamCreateParams {
                    command_id: committed_id.clone(),
                    team_id: TEAM_A.into(),
                    name: "Platform".into(),
                    owner_team_id: Some(TOP_TEAM.into()),
                }),
            )
            .unwrap()
            .unwrap();
        assert_eq!(committed.status, CommandStatus::Committed);

        // 3. operation.get returns Committed and Rejected receipts for the acting actor,
        //    and refuses another actor or unknown ID as not_found.
        let looked = office.operation(CREATOR, &committed_id).unwrap().unwrap();
        assert_eq!(
            looked,
            OperationOutcome::Committed {
                result: serde_json::to_value(committed).unwrap(),
            }
        );
        assert_eq!(
            office
                .operation(HR, &committed_id)
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            office
                .operation(CREATOR, OPERATION)
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );

        let rejected_id = uuid_at(now + wire::MAX_COMMAND_SKEW_MS, 5);
        let refusal = office
            .submit_org(
                CREATOR,
                OrgCommand::TeamCreate(TeamCreateParams {
                    command_id: rejected_id.clone(),
                    team_id: TEAM_B.into(),
                    name: "   ".into(),
                    owner_team_id: Some(TOP_TEAM.into()),
                }),
            )
            .unwrap()
            .unwrap_err();
        assert_eq!(refusal.code, ErrorCode::InvalidName);
        assert_eq!(
            office.operation(CREATOR, &rejected_id).unwrap().unwrap(),
            OperationOutcome::Rejected { refusal }
        );
        assert_eq!(
            office
                .operation(HR, &rejected_id)
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        // Non-owner gets NotAuthorized before receipt/window checks (no existence oracle).
        assert_eq!(
            office
                .submit_org(
                    HR,
                    OrgCommand::TeamCreate(TeamCreateParams {
                        command_id: committed_id.clone(),
                        team_id: TEAM_B.into(),
                        name: "Different Params".into(),
                        owner_team_id: Some(TOP_TEAM.into()),
                    }),
                )
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::NotAuthorized
        );
        assert_eq!(
            office
                .submit_org(
                    HR,
                    OrgCommand::TeamCreate(TeamCreateParams {
                        command_id: expired_id.clone(),
                        team_id: TEAM_B.into(),
                        name: "Non-owner Expired".into(),
                        owner_team_id: Some(TOP_TEAM.into()),
                    }),
                )
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::NotAuthorized
        );
        office.close().unwrap();
    }

    // 4. Advancing clock >29d allows replay of stored receipts while refusing unknown old IDs;
    //    stepping wall clock backward cannot reopen the expired window (monotonic high-water).
    struct SteppingClock(Arc<std::sync::atomic::AtomicU64>);
    impl org_store::Clock for SteppingClock {
        fn now(&self) -> org_store::Timestamp {
            org_store::Timestamp(self.0.load(std::sync::atomic::Ordering::Relaxed))
        }
    }
    let now = crate::organization::testkit::CREATION_AT.0;
    let clock_ms = Arc::new(std::sync::atomic::AtomicU64::new(now));
    let site = Site::new();
    let mut opts = site.options(EngineChoice::Memory);
    opts.clock = Arc::new(SteppingClock(Arc::clone(&clock_ms)));
    let (new, binding) = testkit::request();
    let office = Office::provision_with(
        site.folder.clone(),
        &new,
        Some(Key::from_bytes([7; 32])),
        &binding,
        site.roots.clone(),
        testkit::custody(Arc::clone(&site.custody)),
        |prior| prior.map_or_else(|| testkit::record(testkit::CA_SEED), Ok),
        opts,
    )
    .unwrap();
    let replay_id = uuid_at(now, 10);
    let create_cmd = || {
        OrgCommand::TeamCreate(TeamCreateParams {
            command_id: replay_id.clone(),
            team_id: TEAM_A.into(),
            name: "Platform".into(),
            owner_team_id: Some(TOP_TEAM.into()),
        })
    };
    let first = office.submit_org(CREATOR, create_cmd()).unwrap().unwrap();

    // Advance >29 days: stored receipt still replays, unknown ID at `now` is refused `expired`.
    clock_ms.store(
        now + wire::MAX_COMMAND_AGE_MS + 60_000,
        std::sync::atomic::Ordering::Relaxed,
    );
    let replayed = office.submit_org(CREATOR, create_cmd()).unwrap().unwrap();
    assert_eq!(replayed, first);
    let unknown_old = office
        .submit_org(
            CREATOR,
            OrgCommand::TeamCreate(TeamCreateParams {
                command_id: uuid_at(now, 11),
                team_id: TEAM_B.into(),
                name: "Stale".into(),
                owner_team_id: Some(TOP_TEAM.into()),
            }),
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(unknown_old.code, ErrorCode::Expired);

    // Step clock backward to `now`: monotonic high-water still rejects a new unknown ID at `now`.
    clock_ms.store(now, std::sync::atomic::Ordering::Relaxed);
    let after_rollback = office
        .submit_org(
            CREATOR,
            OrgCommand::TeamCreate(TeamCreateParams {
                command_id: uuid_at(now, 12),
                team_id: TEAM_B.into(),
                name: "Rollback".into(),
                owner_team_id: Some(TOP_TEAM.into()),
            }),
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(after_rollback.code, ErrorCode::Expired);
    office.close().unwrap();
}
