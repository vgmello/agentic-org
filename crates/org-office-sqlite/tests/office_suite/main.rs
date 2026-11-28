//! Dual-engine (`Sqlite` + `Memory`) and SQLite-projection conformance suites moved from
//! `crates/org-office/src/office/` into `org-office-sqlite` (`TASK-255` Part 2, ENG-1).

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};

use org_proto::org::TeamCreateParams;
use org_store::{
    testkit::{identity, FakeCustody, Faults},
    Id,
};

use org_office::{
    hq_creation::LocalRecord,
    rules::OrgCommand,
    testkit_support::{office_harness, Options},
    DataRoots, EngineChoice, Key, Office, OfficeError, Testing,
};

mod accounting_budget_tests;
mod attachments;
mod chief_of_staff_hardening_tests;
mod chief_of_staff_tests;
mod git_parity;
mod hiring_tests;
mod import_tests;
mod office_lifecycle;
mod private_notes_adec_tests;
mod private_notes_tests;
mod processes_tests;
mod project_docs_tests;
mod provider_pool_tests;
mod reviews_tests;
mod satellite_snapshot_resync_tests;
mod satellite_tests;
mod search_tests;

pub(crate) fn sqlite_engine() -> EngineChoice {
    EngineChoice::Sqlite(org_office_sqlite::sqlite_backend())
}

pub(crate) fn engines() -> [EngineChoice; 2] {
    [sqlite_engine(), EngineChoice::Memory]
}
pub(crate) const CREATOR: &str = "01900000-0000-7000-8000-000000000012";
pub(crate) const HR: &str = "01900000-0000-7000-8000-000000000013";
pub(crate) const TOP_TEAM: &str = "01900000-0000-7000-8000-000000000011";
pub(crate) const TEAM_A: &str = "01900000-0000-7000-8000-000000000021";
pub(crate) const TEAM_B: &str = "01900000-0000-7000-8000-000000000022";

/// One office's folder, data roots, custody and disk, which outlive any one process.
pub(crate) struct Site {
    pub(crate) root: PathBuf,
    pub(crate) folder: PathBuf,
    pub(crate) roots: DataRoots,
    pub(crate) custody: Arc<FakeCustody>,
    pub(crate) faults: Arc<Faults>,
    _dir: tempfile::TempDir,
}

impl Site {
    pub(crate) fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let (folder, roots) = office_harness::places(&root);
        Self {
            root,
            folder,
            roots,
            custody: Arc::new(FakeCustody::new()),
            faults: Faults::new(),
            _dir: dir,
        }
    }

    pub(crate) fn options(&self, engine: EngineChoice) -> Options {
        let mut options = office_harness::options(engine);
        options.testing = Testing {
            faults: Some(Arc::clone(&self.faults)),
            flush_interval: Some(std::time::Duration::from_secs(3_600)),
            ..Testing::default()
        };
        options
    }

    pub(crate) fn provision_with(
        &self,
        engine: EngineChoice,
        material: impl FnOnce(Option<LocalRecord>) -> Result<LocalRecord, OfficeError>,
    ) -> Result<Office, OfficeError> {
        let (new, binding) = office_harness::request();
        Office::provision_with(
            self.folder.clone(),
            &new,
            Some(Key::from_bytes([7; 32])),
            &binding,
            self.roots.clone(),
            office_harness::custody(Arc::clone(&self.custody)),
            material,
            self.options(engine),
        )
    }

    pub(crate) fn provision(&self, engine: EngineChoice) -> Result<Office, OfficeError> {
        self.provision_with(engine, |prior| {
            prior.map_or_else(|| office_harness::record(office_harness::CA_SEED), Ok)
        })
    }

    pub(crate) fn open(&self, engine: EngineChoice) -> Result<Office, OfficeError> {
        let instance = identity().instance_id.to_string();
        Office::open_with(
            self.folder.clone(),
            None,
            &instance,
            self.roots.clone(),
            office_harness::custody(Arc::clone(&self.custody)),
            self.options(engine),
        )
    }

    pub(crate) fn killed(&self, office: Office) {
        self.faults.kill();
        drop(office);
        self.faults.revive();
    }

    pub(crate) fn durable(&self, path: &str) -> PathBuf {
        self.roots.durable.join(path)
    }

    pub(crate) fn runtime_folder(&self) -> PathBuf {
        self.roots.runtime.join(identity().office_id.as_str())
    }
}

pub(crate) fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
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

pub(crate) fn folders(root: &Path) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut pending = vec![root.to_owned()];
    while let Some(folder) = pending.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                let name = path.strip_prefix(root).unwrap().to_str().unwrap();
                found.insert(name.to_owned());
                pending.push(path);
            }
        }
    }
    found
}

pub(crate) fn names(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

pub(crate) fn creation_id() -> Id {
    Id::parse(office_harness::OPERATION).unwrap()
}

pub(crate) fn team_create(n: u64, team: &str, parent: &str) -> OrgCommand {
    OrgCommand::TeamCreate(TeamCreateParams {
        command_id: format!("018bce27-f800-7000-8000-{n:012x}"),
        team_id: team.into(),
        name: format!("Team {n}"),
        owner_team_id: Some(parent.into()),
    })
}
