//! A scripted office for building the app's organization flows before the server exists.
//!
//! `AGENTIC_ORG_MOCK` forces a failure: `not_installed`, `needs_approval`, `unreachable` or
//! `storage_error`. Otherwise it behaves like a healthy office on this Mac. Names are validated
//! like the server will: 1 to 64 characters after trimming, no control characters.

use crate::office::{OfficeConnection, SetupError, SetupErrorCode};
use org_proto::{
    ApiError, ErrorCode, HelloResult, OfficeInfo, OfficeRole, OfficeStatus, Organization,
    OrganizationName, API_VERSION,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;
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
        Self {
            organizations: Mutex::new(organizations),
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn healthy() -> MockOffice {
        MockOffice::new(Scenario::Healthy, Duration::ZERO)
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
