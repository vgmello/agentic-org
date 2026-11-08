//! The app's view of an office server.
//!
//! `OfficeConnection` is the seam between the Tauri shell and an office. Today only `MockOffice`
//! implements it. The real implementation will sit on `org-client` and the office-setup layer
//! (install, login-service registration, start), neither of which exists yet.

use org_proto::{ApiError, HelloResult, Organization};
use serde::Serialize;

/// Why the office could not be made ready. These belong to the app's setup layer, not to
/// `org-proto`: the server cannot report that it was never installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupErrorCode {
    NotInstalled,
    NeedsApproval,
    Unreachable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SetupError {
    pub code: SetupErrorCode,
    pub message: String,
}

pub trait OfficeConnection: Send + Sync {
    /// Make sure an office is installed, registered as a login service, running and answering
    /// `hello`.
    fn ensure_running(&self) -> Result<HelloResult, SetupError>;

    fn create_organization(&self, name: &str) -> Result<Organization, ApiError>;
}
