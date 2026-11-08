//! A scripted office for building the Create organization flow before the server exists.
//!
//! `AGENTIC_ORG_MOCK` forces a failure: `not_installed`, `needs_approval`, `unreachable` or
//! `storage_error`. Otherwise it behaves like a healthy office on this Mac. Names are validated
//! like the server will: 1 to 64 characters after trimming, no control characters.

use crate::office::{OfficeConnection, SetupError, SetupErrorCode};
use org_proto::{
    ApiError, ErrorCode, HelloResult, OfficeInfo, Organization, API_VERSION,
    ORGANIZATION_NAME_MAX_CHARS,
};
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
}

impl MockOffice {
    pub fn new(scenario: Scenario, startup_delay: Duration) -> Self {
        Self {
            scenario,
            startup_delay,
            organizations: Mutex::new(Vec::new()),
        }
    }

    pub fn from_env() -> Self {
        let scenario = Scenario::parse(std::env::var("AGENTIC_ORG_MOCK").ok().as_deref());
        Self::new(scenario, Duration::from_millis(900))
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

/// The name as the server would store it, or `invalid_name`.
pub fn validate_name(name: &str) -> Result<String, ApiError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(api_error(
            ErrorCode::InvalidName,
            "Give the organization a name.",
        ));
    }
    if name.chars().count() > ORGANIZATION_NAME_MAX_CHARS {
        return Err(api_error(
            ErrorCode::InvalidName,
            &format!("Use {ORGANIZATION_NAME_MAX_CHARS} characters or fewer."),
        ));
    }
    if name.chars().any(char::is_control) {
        return Err(api_error(
            ErrorCode::InvalidName,
            "The name can't contain control characters.",
        ));
    }
    Ok(name.to_string())
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
                "Allow agentic.org to run in the background in System Settings, then try again.",
            )),
            Scenario::Unreachable => Err(setup_error(
                SetupErrorCode::Unreachable,
                "The office server started but isn't answering.",
            )),
            Scenario::Healthy | Scenario::StorageError => Ok(HelloResult {
                api_version: API_VERSION,
                server_version: "0.0.0-mock".to_string(),
                office: OfficeInfo {
                    id: "office-local".to_string(),
                    name: "This Mac".to_string(),
                    host: "This Mac".to_string(),
                },
            }),
        }
    }

    fn create_organization(&self, name: &str) -> Result<Organization, ApiError> {
        let name = validate_name(name)?;
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
        let organization = Organization {
            id: unique_id(organizations.len()),
            name,
            created_at: "1970-01-01T00:00:00.000Z".to_string(),
        };
        organizations.push(organization.clone());
        Ok(organization)
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

    /// The same vectors the TypeScript pre-check is tested against (`src/lib/name.test.ts`), so
    /// the two copies of the rule can't drift apart unnoticed.
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
        let vectors: Vectors =
            serde_json::from_str(include_str!("../../test-vectors/organization-names.json"))
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
}
