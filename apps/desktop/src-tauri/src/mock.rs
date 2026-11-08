//! A scripted office for building the app's organization flows before the server exists.
//!
//! `AGENTIC_ORG_MOCK` forces a failure: `not_installed`, `needs_approval`, `unreachable` or
//! `storage_error`. Otherwise it behaves like a healthy office on this Mac. Names are validated
//! like the server will: 1 to 64 characters after trimming, no control characters.

use crate::office::{OfficeConnection, SetupError, SetupErrorCode};
use crate::org_command::OrgCommand;
use crate::org_engine::{Ctx, Hq};
use crate::org_mock::Seed;
use crate::work_engine::{self, WorkError, WorkState};
use org_proto::{
    ApiError, CommandAccepted, ErrorCode, HelloResult, OfficeInfo, OfficeRole, OfficeStatus,
    OrgSnapshot, Organization, OrganizationName, API_VERSION,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scenario {
    Healthy,
    NotInstalled,
    NeedsApproval,
    Unreachable,
    StorageError,
}

impl Scenario {
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("not_installed") => Self::NotInstalled,
            Some("needs_approval") => Self::NeedsApproval,
            Some("unreachable") => Self::Unreachable,
            Some("storage_error") => Self::StorageError,
            _ => Self::Healthy,
        }
    }
}

pub struct MockOffice {
    scenario: Scenario,
    /// How long `ensure_running` pretends to take, so the progress state is visible.
    startup_delay: Duration,
    organizations: Mutex<Vec<Organization>>,
    /// What a new organization's structure starts as, and the structure of each one opened so far.
    seed: Seed,
    structures: Mutex<HashMap<String, Hq>>,
    /// Where the structures are kept (next to the organizations); `None` keeps them in memory only.
    structures_file: Option<PathBuf>,
    /// The work (tasks) of each organization, kept next to the structures in `work.json`.
    work: Mutex<HashMap<String, WorkState>>,
    work_file: Option<PathBuf>,
    /// Set when the stored structures are from a newer version: the office then refuses to read or
    /// change any rather than overwrite them.
    structures_unreadable: bool,
    /// Where the organizations are kept between launches, like the real office's storage. `None`
    /// keeps them in memory only (tests).
    store: Option<PathBuf>,
    /// Set when the stored organizations couldn't be read: the office then refuses to list or
    /// create rather than overwrite what it couldn't read.
    unreadable: Mutex<bool>,
}

impl MockOffice {
    pub fn new(scenario: Scenario, startup_delay: Duration) -> Self {
        Self {
            scenario,
            startup_delay,
            organizations: Mutex::new(Vec::new()),
            seed: Seed::Sample,
            structures: Mutex::new(HashMap::new()),
            structures_file: None,
            work: Mutex::new(HashMap::new()),
            work_file: None,
            structures_unreadable: false,
            store: None,
            unreadable: Mutex::new(false),
        }
    }

    /// An office that already hosts `organizations`.
    #[cfg(test)]
    pub fn with_organizations(mut self, organizations: Vec<Organization>) -> Self {
        self.organizations = Mutex::new(organizations);
        self
    }

    /// The office the app runs against for now. Its organizations live in `dir` between
    /// launches; the first time, it already hosts two sample organizations. A file that doesn't
    /// parse is set aside; one that can't be read, or has an unknown version, makes the office
    /// refuse to list or create (storage_error) instead of overwriting it.
    pub fn from_env(dir: PathBuf) -> Self {
        let scenario = Scenario::parse(std::env::var("AGENTIC_ORG_MOCK").ok().as_deref());
        let file = dir.join("organizations.json");
        let (organizations, unreadable) = match load_store(&file) {
            Ok(organizations) => (organizations, false),
            Err(_) => (Vec::new(), true),
        };
        let structures_file = dir.join("structures.json");
        let (structures, structures_unreadable) = match load_structures(&structures_file) {
            Ok(structures) => (structures, false),
            Err(_) => (HashMap::new(), true),
        };
        Self {
            structures: Mutex::new(structures),
            work: Mutex::new(load_work(&dir.join("work.json"))),
            work_file: Some(dir.join("work.json")),
            structures_file: Some(structures_file),
            structures_unreadable,
            organizations: Mutex::new(organizations),
            seed: Seed::parse(std::env::var("AGENTIC_ORG_MOCK_ORG").ok().as_deref()),
            store: Some(file),
            unreadable: Mutex::new(unreadable),
            ..Self::new(scenario, Duration::from_millis(900))
        }
    }

    /// Refuse while the stored organizations can't be read. Each call looks again, so fixing the
    /// file (or restoring it) makes the office work without a restart.
    fn check_readable(&self) -> Result<(), ApiError> {
        let mut unreadable = self.unreadable.lock().unwrap_or_else(|e| e.into_inner());
        if !*unreadable {
            return Ok(());
        }
        let loaded = self.store.as_deref().map(load_store);
        match loaded {
            Some(Ok(organizations)) => {
                *self.organizations.lock().unwrap_or_else(|e| e.into_inner()) = organizations;
                *unreadable = false;
                Ok(())
            }
            _ => Err(api_error(
                ErrorCode::StorageError,
                "The office couldn't read its stored organizations: the file is unreadable or from a newer version.",
            )),
        }
    }

    fn persist(&self, organizations: &[Organization]) -> Result<(), ApiError> {
        let Some(file) = &self.store else {
            return Ok(());
        };
        let write = || -> std::io::Result<()> {
            if let Some(dir) = file.parent() {
                fs::create_dir_all(dir)?;
            }
            let tmp = file.with_extension("json.tmp");
            let file_contents = StoreFile {
                version: STORE_VERSION,
                organizations: organizations.to_vec(),
            };
            fs::write(&tmp, serde_json::to_vec_pretty(&file_contents)?)?;
            fs::rename(&tmp, file)
        };
        write().map_err(|_| {
            api_error(
                ErrorCode::StorageError,
                "The office couldn't write the organization's folder.",
            )
        })
    }

    /// The id of the organization's HQ office, after the checks every organization call makes.
    fn hq_office_id(&self, organization_id: &str) -> Result<String, ApiError> {
        self.check_readable()?;
        if self.structures_unreadable {
            return Err(api_error(
                ErrorCode::StorageError,
                "The office couldn't read the stored organization structures: the file is unreadable or from a newer version.",
            ));
        }
        let organizations = self.organizations.lock().unwrap_or_else(|e| e.into_inner());
        let organization = organizations
            .iter()
            .find(|o| o.id == organization_id)
            .ok_or_else(|| api_error(ErrorCode::NotFound, "That organization isn't here."))?;
        organization
            .offices
            .iter()
            .find(|o| o.role == OfficeRole::Hq)
            .map(|o| o.id.clone())
            .ok_or_else(|| api_error(ErrorCode::NotFound, "That organization has no HQ office."))
    }

    /// Every office the organization has: where an agent can be placed.
    fn office_ids(&self, organization_id: &str) -> Vec<String> {
        self.organizations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|o| o.id == organization_id)
            .map(|o| o.offices.iter().map(|office| office.id.clone()).collect())
            .unwrap_or_default()
    }

    /// The organization's HQ state, seeded the first time it is asked for.
    fn hq<'a>(
        &self,
        structures: &'a mut HashMap<String, Hq>,
        organization_id: &str,
        hq_office: &str,
    ) -> &'a mut Hq {
        structures
            .entry(organization_id.to_string())
            .or_insert_with(|| Hq::new(self.seed.snapshot(hq_office)))
    }

    fn persist_structures(&self, structures: &HashMap<String, Hq>) -> Result<(), ApiError> {
        let Some(file) = &self.structures_file else {
            return Ok(());
        };
        write_structures(file, structures).map_err(|_| {
            api_error(
                ErrorCode::StorageError,
                "The office couldn't write the organization's structure.",
            )
        })
    }
}

/// The office's refusal as the work calls report it: the same code, as text.
fn into_work_error(e: ApiError) -> WorkError {
    let code = match e.code {
        ErrorCode::NotFound => "not_found",
        ErrorCode::StorageError => "storage_error",
        _ => "unavailable",
    };
    WorkError {
        code,
        message: e.message,
    }
}

/// The tasks as they were last saved; a file that can't be read starts empty (it is a mock's sample data).
fn load_work(file: &Path) -> HashMap<String, WorkState> {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_work(file: &Path, work: &HashMap<String, WorkState>) -> std::io::Result<()> {
    let text = serde_json::to_string(work).map_err(std::io::Error::other)?;
    let tmp = file.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, file)
}

const STORE_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct StoreFile {
    version: u32,
    organizations: Vec<Organization>,
}

#[derive(Deserialize)]
struct StoreVersion {
    version: u32,
}

/// The stored organizations. Missing: the samples. Unparseable: set aside as
/// `organizations.json.corrupt-<ms>`, then the samples. Unknown version or an I/O error: an error,
/// and the file is left alone.
fn load_store(file: &std::path::Path) -> io::Result<Vec<Organization>> {
    let bytes = match fs::read(file) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(sample_organizations()),
        Err(e) => return Err(e),
    };
    match serde_json::from_slice::<StoreVersion>(&bytes) {
        Ok(StoreVersion { version }) if version != STORE_VERSION => {
            return Err(io::Error::other("unsupported organizations.json version"))
        }
        Ok(_) => {
            if let Ok(stored) = serde_json::from_slice::<StoreFile>(&bytes) {
                return Ok(stored.organizations);
            }
        }
        Err(_) => {}
    }
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    fs::rename(file, file.with_extension(format!("json.corrupt-{millis}")))?;
    Ok(sample_organizations())
}

const STRUCTURES_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct StructuresFile {
    version: u32,
    organizations: HashMap<String, Hq>,
}

/// The stored structures. Missing: none yet. Unparseable: set aside as `structures.json.corrupt-<ms>`
/// and started again. Unknown version or an I/O error: an error, and the file is left alone.
fn load_structures(file: &std::path::Path) -> io::Result<HashMap<String, Hq>> {
    let bytes = match fs::read(file) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(e) => return Err(e),
    };
    match serde_json::from_slice::<StoreVersion>(&bytes) {
        Ok(StoreVersion { version }) if version != STRUCTURES_VERSION => {
            return Err(io::Error::other("unsupported structures.json version"))
        }
        Ok(_) => {
            if let Ok(mut stored) = serde_json::from_slice::<StructuresFile>(&bytes) {
                stored.organizations.values_mut().for_each(Hq::tidy);
                return Ok(stored.organizations);
            }
        }
        Err(_) => {}
    }
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    fs::rename(file, file.with_extension(format!("json.corrupt-{millis}")))?;
    Ok(HashMap::new())
}

fn write_structures(file: &std::path::Path, structures: &HashMap<String, Hq>) -> io::Result<()> {
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = file.with_extension("json.tmp");
    let contents = StructuresFile {
        version: STRUCTURES_VERSION,
        organizations: structures.clone(),
    };
    let mut out = fs::File::create(&tmp)?;
    io::Write::write_all(&mut out, &serde_json::to_vec_pretty(&contents)?)?;
    out.sync_all()?; // on disk before it replaces the old one, so a crash can't leave an empty file
    drop(out);
    fs::rename(&tmp, file)
}

/// RFC 3339 UTC with milliseconds, like the real office writes.
fn rfc3339(secs: i64, millis: u32) -> String {
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

fn now_rfc3339() -> String {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    rfc3339(since.as_secs() as i64, since.subsec_millis())
}

/// What a fresh mock office already hosts, so there is something to open on first launch.
fn sample_organizations() -> Vec<Organization> {
    let sample = |id: &str, name: &str, created_at: &str| Organization {
        id: id.to_string(),
        name: name.to_string(),
        created_at: created_at.to_string(),
        offices: vec![hq_office(id)],
    };
    vec![
        sample(
            "sample-northwind",
            "Northwind Labs",
            "2026-09-14T09:30:00.000Z",
        ),
        sample(
            "sample-globex",
            "Globex Research",
            "2026-10-02T16:05:00.000Z",
        ),
    ]
}

/// The HQ office every organization starts with, on this Mac.
fn hq_office(organization_id: &str) -> OfficeInfo {
    OfficeInfo {
        id: format!("office-{organization_id}"),
        organization_id: organization_id.to_string(),
        name: "HQ".to_string(),
        role: OfficeRole::Hq,
        host: "This Mac".to_string(),
        endpoint: None,
        status: OfficeStatus::Active,
    }
}

/// An id that is unique across launches (the real office issues ULIDs; ids are opaque).
fn unique_id(n: usize) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    format!("org-{millis:x}-{n}")
}

fn setup_error(code: SetupErrorCode, message: &str) -> SetupError {
    SetupError {
        code,
        message: message.to_string(),
    }
}

fn api_error(code: ErrorCode, message: &str) -> ApiError {
    ApiError {
        code,
        message: message.to_string(),
    }
}

/// The name as the server would store it, or `invalid_name`: the contract's own rule.
pub fn validate_name(name: &str) -> Result<String, ApiError> {
    OrganizationName::parse(name)
        .map(OrganizationName::into_string)
        .map_err(ApiError::from)
}

impl OfficeConnection for MockOffice {
    fn ensure_running(&self) -> Result<HelloResult, SetupError> {
        std::thread::sleep(self.startup_delay);
        match self.scenario {
            Scenario::NotInstalled => Err(setup_error(
                SetupErrorCode::NotInstalled,
                "The office server isn't installed on this Mac.",
            )),
            Scenario::NeedsApproval => Err(setup_error(
                SetupErrorCode::NeedsApproval,
                "Allow Agentic Org to run in the background in System Settings, then try again.",
            )),
            Scenario::Unreachable => Err(setup_error(
                SetupErrorCode::Unreachable,
                "The office server started but isn't answering.",
            )),
            Scenario::Healthy | Scenario::StorageError => Ok(HelloResult {
                api_version: API_VERSION,
                server_version: "0.0.0-mock".to_string(),
                host: "This Mac".to_string(),
                office: None,
            }),
        }
    }

    fn create_organization(&self, name: &str) -> Result<Organization, ApiError> {
        let name = validate_name(name)?;
        self.check_readable()?;
        if self.scenario == Scenario::StorageError {
            return Err(api_error(
                ErrorCode::StorageError,
                "The office couldn't write the organization's folder.",
            ));
        }
        let mut organizations = self.organizations.lock().unwrap_or_else(|e| e.into_inner());
        if organizations
            .iter()
            .any(|o| o.name.to_lowercase() == name.to_lowercase())
        {
            return Err(api_error(
                ErrorCode::AlreadyExists,
                "An organization with that name already exists.",
            ));
        }
        let id = unique_id(organizations.len());
        let organization = Organization {
            offices: vec![hq_office(&id)],
            id,
            name,
            created_at: now_rfc3339(),
        };
        organizations.push(organization.clone());
        if let Err(e) = self.persist(&organizations) {
            organizations.pop();
            return Err(e);
        }
        Ok(organization)
    }

    fn list_organizations(&self) -> Result<Vec<Organization>, ApiError> {
        self.check_readable()?;
        Ok(self
            .organizations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone())
    }

    fn org_snapshot(&self, organization_id: &str) -> Result<OrgSnapshot, ApiError> {
        let hq_office = self.hq_office_id(organization_id)?;
        let mut structures = self.structures.lock().unwrap_or_else(|e| e.into_inner());
        Ok(self
            .hq(&mut structures, organization_id, &hq_office)
            .snapshot
            .clone())
    }

    fn org_command(
        &self,
        organization_id: &str,
        command: &OrgCommand,
    ) -> Result<CommandAccepted, ApiError> {
        let hq_office = self.hq_office_id(organization_id)?;
        let offices = self.office_ids(organization_id);
        let mut structures = self.structures.lock().unwrap_or_else(|e| e.into_inner());
        let hq = self.hq(&mut structures, organization_id, &hq_office);
        let before = hq.clone();
        let ctx = Ctx {
            now: now_rfc3339(),
            offices,
        };
        let (answer, _changed) = hq.handle_with(command, &ctx);
        // A retry changes nothing; anything else (a commit, or a first refusal) is kept.
        if *hq != before {
            if let Err(e) = self.persist_structures(&structures) {
                structures.insert(organization_id.to_string(), before);
                return Err(e);
            }
        }
        answer
    }

    fn work_snapshot(
        &self,
        organization_id: &str,
        project_id: Option<&str>,
        closed_since: Option<&str>,
    ) -> Result<Value, WorkError> {
        self.hq_office_id(organization_id)
            .map_err(into_work_error)?;
        let work = self.work.lock().unwrap_or_else(|e| e.into_inner());
        let state = work
            .get(organization_id)
            .cloned()
            .unwrap_or_else(work_engine::new_state);
        Ok(json!({
            "position": {"incarnation": state.incarnation, "seq": state.seq},
            "items": work_engine::summaries(&state, project_id, closed_since),
        }))
    }

    fn work_get(&self, organization_id: &str, item_id: &str) -> Result<Value, WorkError> {
        self.hq_office_id(organization_id)
            .map_err(into_work_error)?;
        let work = self.work.lock().unwrap_or_else(|e| e.into_inner());
        let state = work
            .get(organization_id)
            .cloned()
            .unwrap_or_else(work_engine::new_state);
        work_engine::get(&state, item_id, &now_rfc3339())
    }

    fn work_command(&self, organization_id: &str, call: &Value) -> Result<Value, WorkError> {
        let snapshot = self
            .org_snapshot(organization_id)
            .map_err(into_work_error)?;
        let ctx = work_engine::Ctx {
            now: now_rfc3339(),
            actor: snapshot
                .members
                .iter()
                .find(|m| m.owner)
                .map(|m| m.id.clone())
                .unwrap_or_else(|| "owner".into()),
            projects: snapshot
                .projects
                .iter()
                .map(|p| work_engine::Project {
                    id: p.id.clone(),
                    archived: false,
                    team_ids: snapshot
                        .project_teams
                        .iter()
                        .filter(|l| l.project_id == p.id)
                        .map(|l| l.team_id.clone())
                        .collect(),
                })
                .collect(),
            members: snapshot
                .members
                .iter()
                .filter(|m| {
                    !(m.kind == org_proto::MemberKind::Agent
                        && m.agent
                            .as_ref()
                            .is_some_and(|a| a.status == org_proto::AgentStatus::Offboarded))
                })
                .map(|m| m.id.clone())
                .collect(),
        };
        let mut work = self.work.lock().unwrap_or_else(|e| e.into_inner());
        let mut state = work
            .get(organization_id)
            .cloned()
            .unwrap_or_else(work_engine::new_state);
        let answer = work_engine::apply(&mut state, call, &ctx)?;
        let before = work.insert(organization_id.to_string(), state);
        if let Some(file) = &self.work_file {
            if write_work(file, &work).is_err() {
                // Not saved, so not kept either (as the structure commands do).
                match before {
                    Some(previous) => work.insert(organization_id.to_string(), previous),
                    None => work.remove(organization_id),
                };
                return Err(WorkError {
                    code: "storage_error",
                    message: "The office couldn't write the tasks.".into(),
                });
            }
        }
        Ok(answer)
    }

    fn serves_sample_data(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn healthy() -> MockOffice {
        MockOffice::new(Scenario::Healthy, Duration::ZERO)
    }

    #[test]
    fn tasks_are_kept_per_organization_and_survive_a_restart() {
        let dir = std::env::temp_dir().join(format!("agentic-work-{}", unique_id(7)));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let first = MockOffice::from_env(dir.clone());
        let org = first.create_organization("Alpha").unwrap();
        let project = first.org_snapshot(&org.id).unwrap().projects[0].id.clone();
        let create = json!({"method": "task.create", "change_id": "c1", "item_id": "01JC0000000000000000000001", "project_id": project, "title": "Ship it"});
        let answer = first.work_command(&org.id, &create).unwrap();
        assert_eq!(answer["items"][0]["number"], 1);
        // The same change id gets the same answer and changes nothing.
        assert_eq!(first.work_command(&org.id, &create).unwrap(), answer);
        let other = first.create_organization("Beta").unwrap();
        assert_eq!(
            first.work_snapshot(&other.id, None, None).unwrap()["items"]
                .as_array()
                .unwrap()
                .len(),
            0
        );

        let second = MockOffice::from_env(dir.clone());
        let items = second.work_snapshot(&org.id, Some(&project), None).unwrap();
        assert_eq!(items["items"][0]["title"], "Ship it");
        assert_eq!(
            second
                .work_get(&org.id, "01JC0000000000000000000001")
                .unwrap()["summary"]["status"],
            "todo"
        );
        // A refusal carries the contract code.
        let bad = json!({"method": "task.create", "change_id": "c2", "item_id": "01JC0000000000000000000002", "project_id": "nope", "title": "x"});
        assert_eq!(
            second.work_command(&org.id, &bad).unwrap_err().code,
            "not_found"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn org_snapshot_seeds_each_organization_once_with_agents_placed_in_its_hq() {
        let office = healthy();
        let a = office.create_organization("Alpha").unwrap();
        let b = office.create_organization("Beta").unwrap();
        let hq = |o: &Organization| o.offices[0].id.clone();
        let sa = office.org_snapshot(&a.id).unwrap();
        assert!(sa
            .members
            .iter()
            .any(|m| m.office_id.as_deref() == Some(hq(&a).as_str())));
        assert!(!sa
            .members
            .iter()
            .any(|m| m.office_id.as_deref() == Some(hq(&b).as_str())));
        assert_eq!(
            office.org_snapshot(&a.id).unwrap(),
            sa,
            "the same structure on every call"
        );
        assert!(office
            .org_snapshot(&b.id)
            .unwrap()
            .members
            .iter()
            .any(|m| m.office_id.as_deref() == Some(hq(&b).as_str())));
        assert_eq!(
            office.org_snapshot("missing").unwrap_err().code,
            ErrorCode::NotFound
        );
        assert!(office.serves_sample_data());
    }

    #[test]
    fn org_snapshot_refuses_when_the_stored_organizations_cannot_be_read() {
        let dir =
            std::env::temp_dir().join(format!("agentic-org-test-snapshot-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("organizations.json"),
            br#"{"version": 999, "organizations": []}"#,
        )
        .unwrap();
        let office = MockOffice::from_env(dir.clone());
        assert_eq!(
            office.org_snapshot("a").unwrap_err().code,
            ErrorCode::StorageError
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn org_snapshot_needs_an_hq_office() {
        let mut org = healthy().create_organization("Alpha").unwrap();
        org.offices.clear();
        let office = healthy().with_organizations(vec![org.clone()]);
        let err = office.org_snapshot(&org.id).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
    }

    fn create_team(id: &str, command_id: &str) -> OrgCommand {
        serde_json::from_value(serde_json::json!({
            "method": "team.create",
            "params": {"command_id": command_id, "team_id": id, "name": id}
        }))
        .unwrap()
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("agentic-org-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_committed_command_is_kept_between_launches_and_a_retry_is_not_applied_twice() {
        let dir = temp_dir("commands");
        let first = MockOffice::from_env(dir.clone());
        let org = first.list_organizations().unwrap().remove(0);
        let c = create_team("ops", "C1");
        let a = first.org_command(&org.id, &c).unwrap();
        assert_eq!(a.status, org_proto::CommandStatus::Committed);
        drop(first);
        let second = MockOffice::from_env(dir.clone());
        let snapshot = second.org_snapshot(&org.id).unwrap();
        assert!(snapshot.teams.iter().any(|t| t.id == "ops"));
        assert_eq!(
            second.org_command(&org.id, &c).unwrap(),
            a,
            "the retry is answered, not applied"
        );
        assert_eq!(
            second.org_snapshot(&org.id).unwrap().position,
            snapshot.position
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_refusal_is_remembered_between_launches_and_a_failed_write_can_be_retried() {
        let dir = temp_dir("refusal");
        let first = MockOffice::from_env(dir.clone());
        let org = first.list_organizations().unwrap().remove(0);
        let bad = create_team("eng", "R1"); // Engineering exists in the sample
        assert_eq!(
            first.org_command(&org.id, &bad).unwrap_err().code,
            ErrorCode::AlreadyExists
        );
        drop(first);
        let second = MockOffice::from_env(dir.clone());
        assert_eq!(
            second.org_command(&org.id, &bad).unwrap_err().code,
            ErrorCode::AlreadyExists
        );
        // A write that fails is not remembered, so the same command can be sent again.
        let good = create_team("ops", "R2");
        let saved = fs::read(dir.join("structures.json")).unwrap();
        fs::remove_file(dir.join("structures.json")).unwrap();
        fs::create_dir(dir.join("structures.json")).unwrap(); // a folder where the file goes: the rename fails
        assert_eq!(
            second.org_command(&org.id, &good).unwrap_err().code,
            ErrorCode::StorageError
        );
        assert!(!second
            .org_snapshot(&org.id)
            .unwrap()
            .teams
            .iter()
            .any(|t| t.id == "ops"));
        fs::remove_dir(dir.join("structures.json")).unwrap();
        fs::write(dir.join("structures.json"), saved).unwrap();
        assert!(second.org_command(&org.id, &good).is_ok());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_refused_command_is_an_api_error_and_changes_nothing() {
        let office = healthy();
        let org = office.create_organization("Alpha").unwrap();
        let before = office.org_snapshot(&org.id).unwrap();
        let err = office
            .org_command(&org.id, &create_team("eng", "C1"))
            .unwrap_err();
        assert_eq!(
            err.code,
            ErrorCode::AlreadyExists,
            "Engineering exists in the sample"
        );
        assert_eq!(office.org_snapshot(&org.id).unwrap(), before);
        assert_eq!(
            office
                .org_command("missing", &create_team("x", "C2"))
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn structures_from_a_newer_version_are_never_overwritten() {
        let dir = temp_dir("newer");
        fs::write(
            dir.join("structures.json"),
            br#"{"version": 99, "organizations": {}}"#,
        )
        .unwrap();
        let office = MockOffice::from_env(dir.clone());
        let org = office.list_organizations().unwrap().remove(0);
        assert_eq!(
            office.org_snapshot(&org.id).unwrap_err().code,
            ErrorCode::StorageError
        );
        assert_eq!(
            office
                .org_command(&org.id, &create_team("x", "C"))
                .unwrap_err()
                .code,
            ErrorCode::StorageError
        );
        assert!(fs::read_to_string(dir.join("structures.json"))
            .unwrap()
            .contains("99"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn unparseable_structures_are_set_aside_and_started_again() {
        let dir = temp_dir("corrupt");
        fs::write(dir.join("structures.json"), b"{ not json").unwrap();
        let office = MockOffice::from_env(dir.clone());
        let org = office.list_organizations().unwrap().remove(0);
        assert!(office.org_snapshot(&org.id).is_ok());
        let aside = fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .any(|e| e.file_name().to_string_lossy().contains("corrupt"));
        assert!(aside, "the unreadable file was kept");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_failed_write_refuses_the_command_and_leaves_the_structure_as_it_was() {
        let dir = temp_dir("writefail");
        let office = MockOffice::from_env(dir.clone());
        let org = office.list_organizations().unwrap().remove(0);
        let before = office.org_snapshot(&org.id).unwrap();
        // The folder disappears and a file takes its place, so the structure can't be written.
        fs::remove_dir_all(&dir).unwrap();
        fs::write(&dir, b"in the way").unwrap();
        let err = office
            .org_command(&org.id, &create_team("ops", "C1"))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::StorageError);
        assert_eq!(office.org_snapshot(&org.id).unwrap(), before);
        let _ = fs::remove_file(&dir);
    }

    #[test]
    fn an_empty_seed_starts_an_organization_with_only_its_owner() {
        let mut office = healthy();
        office.seed = Seed::Empty;
        let a = office.create_organization("Alpha").unwrap();
        let s = office.org_snapshot(&a.id).unwrap();
        assert_eq!(s.members.len(), 1);
        assert!(s.members[0].owner);
    }

    #[test]
    fn trims_and_creates() {
        let org = healthy().create_organization("  Acme  ").unwrap();
        assert_eq!(org.name, "Acme");
    }

    /// org-proto's own vectors, the ones the TypeScript pre-check is tested against too (`src/lib/name.test.ts`).
    #[test]
    fn name_rules_match_the_shared_vectors() {
        #[derive(serde::Deserialize)]
        struct Vector {
            input: String,
            name: Option<String>,
        }
        #[derive(serde::Deserialize)]
        struct Vectors {
            vectors: Vec<Vector>,
        }
        let vectors: Vectors = serde_json::from_str(include_str!(
            "../../../../crates/org-proto/test-vectors/organization-names.json"
        ))
        .unwrap();
        for v in vectors.vectors {
            match (validate_name(&v.input), v.name) {
                (Ok(got), Some(want)) => assert_eq!(got, want),
                (Err(e), None) => assert_eq!(e.code, ErrorCode::InvalidName),
                (got, want) => panic!("{:?}: got {got:?}, want {want:?}", v.input),
            }
        }
    }

    #[test]
    fn duplicate_names_collide_case_insensitively() {
        let office = healthy();
        office.create_organization("Acme").unwrap();
        let err = office.create_organization("aCME").unwrap_err();
        assert_eq!(err.code, ErrorCode::AlreadyExists);
    }

    #[test]
    fn scenarios_parse_and_fail_as_scripted() {
        assert_eq!(Scenario::parse(None), Scenario::Healthy);
        assert_eq!(Scenario::parse(Some("bogus")), Scenario::Healthy);
        let office = MockOffice::new(Scenario::parse(Some("needs_approval")), Duration::ZERO);
        assert_eq!(
            office.ensure_running().unwrap_err().code,
            SetupErrorCode::NeedsApproval
        );
        let office = MockOffice::new(Scenario::parse(Some("storage_error")), Duration::ZERO);
        assert!(office.ensure_running().is_ok());
        assert_eq!(
            office.create_organization("Acme").unwrap_err().code,
            ErrorCode::StorageError
        );
    }

    #[test]
    fn organizations_are_kept_between_launches_and_samples_are_seeded_once() {
        let dir =
            std::env::temp_dir().join(format!("agentic-org-test-mock-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let first = MockOffice::from_env(dir.clone());
        let seeded = first.list_organizations().unwrap();
        assert_eq!(seeded.len(), 2);
        first.create_organization("Brand New").unwrap();

        let second = MockOffice::from_env(dir.clone());
        let names: Vec<_> = second
            .list_organizations()
            .unwrap()
            .into_iter()
            .map(|o| o.name)
            .collect();
        assert_eq!(names, ["Northwind Labs", "Globex Research", "Brand New"]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_failed_write_does_not_leave_the_organization_behind() {
        // Reading finds no file (so the samples are seeded), but the temp file can't be written:
        // a directory sits where it would go.
        let dir = temp("failed-write");
        fs::create_dir_all(dir.join("organizations.json.tmp")).unwrap();
        let office = MockOffice::from_env(dir.clone());
        let before = office.list_organizations().unwrap().len();
        let err = office.create_organization("Nope").unwrap_err();
        assert_eq!(err.code, ErrorCode::StorageError);
        assert_eq!(office.list_organizations().unwrap().len(), before);
        assert!(!dir.join("organizations.json").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn dates_are_real_rfc3339() {
        assert_eq!(rfc3339(0, 0), "1970-01-01T00:00:00.000Z");
        assert_eq!(rfc3339(1_000_000_000, 7), "2001-09-09T01:46:40.007Z");
        assert_eq!(rfc3339(1_700_000_000, 123), "2023-11-14T22:13:20.123Z");
        assert_eq!(rfc3339(951_782_400, 0), "2000-02-29T00:00:00.000Z");
        let created = healthy().create_organization("Dated").unwrap().created_at;
        assert!(created.starts_with("20"), "{created}");
        assert!(created.ends_with('Z') && created.len() == 24, "{created}");
    }

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("agentic-org-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_corrupt_store_is_set_aside_and_the_samples_are_back() {
        let dir = temp("corrupt-store");
        fs::write(dir.join("organizations.json"), b"{ not json").unwrap();
        let office = MockOffice::from_env(dir.clone());
        assert_eq!(office.list_organizations().unwrap().len(), 2);
        let aside = fs::read_dir(&dir)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .contains(".corrupt-")
            })
            .count();
        assert_eq!(aside, 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_unreadable_or_future_store_is_refused_and_left_untouched() {
        let dir = temp("future-store");
        let future = br#"{"version":9,"organizations":[]}"#;
        fs::write(dir.join("organizations.json"), future).unwrap();
        let office = MockOffice::from_env(dir.clone());
        assert_eq!(
            office.list_organizations().unwrap_err().code,
            ErrorCode::StorageError
        );
        assert_eq!(
            office.create_organization("Nope").unwrap_err().code,
            ErrorCode::StorageError
        );
        assert_eq!(fs::read(dir.join("organizations.json")).unwrap(), future);

        // Once the file is fixed, the same office recovers without a restart.
        let fixed = br#"{"version":1,"organizations":[]}"#;
        fs::write(dir.join("organizations.json"), fixed).unwrap();
        assert!(office.list_organizations().unwrap().is_empty());
        office.create_organization("Back Again").unwrap();
        let after = fs::read_to_string(dir.join("organizations.json")).unwrap();
        assert!(after.contains("Back Again"));
        fs::remove_dir_all(&dir).unwrap();

        // A directory where the file should be: not NotFound, so also refused.
        let dir = temp("dir-store");
        fs::create_dir_all(dir.join("organizations.json")).unwrap();
        let office = MockOffice::from_env(dir.clone());
        assert_eq!(
            office.list_organizations().unwrap_err().code,
            ErrorCode::StorageError
        );
        // Remove the directory: the file is now simply missing, so the samples are seeded.
        fs::remove_dir_all(dir.join("organizations.json")).unwrap();
        assert_eq!(office.list_organizations().unwrap().len(), 2);
        fs::remove_dir_all(dir).unwrap();
    }
}
