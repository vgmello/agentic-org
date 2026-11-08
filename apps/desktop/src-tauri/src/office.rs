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

    /// Ask the running office whether it answers (`hello`). Read-only: it never installs, registers or
    /// starts anything, so it is safe to call often (the Offices screen checks every half minute). Every failure is
    /// reported as `setup/unreachable`. This says the host (or the mock) answers, not that the connected office is up:
    /// the screen shows Online only when the office's reported status is also `active` (OFC-12).
    fn ping(&self) -> Result<(), SetupError>;

    fn create_organization(&self, name: &str) -> Result<Organization, ApiError>;

    /// Every organization this office hosts (`organization.list`).
    fn list_organizations(&self) -> Result<Vec<Organization>, ApiError>;
}
