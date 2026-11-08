//! The app's view of an office server.
//!
//! `OfficeConnection` is the seam between the Tauri shell and an office. Today only `MockOffice`
//! implements it. The real implementation will sit on `org-client` and the office-setup layer
//! (install, login-service registration, start), neither of which exists yet.

use crate::org_command::OrgCommand;
use org_proto::{ApiError, CommandAccepted, HelloResult, OrgSnapshot, Organization};
use serde::Serialize;

/// Why the office could not be made ready. These belong to the app's setup layer, not to
/// `org-proto`: the server cannot report that it was never installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupErrorCode {
    NotInstalled,
    NeedsApproval,
    /// The owner turned the office's login service off in Settings: it stays off until they turn it on.
    TurnedOff,
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

    /// `org.snapshot`: the structure of an organization, from this machine's office. The call has no
    /// parameters on the wire: `organization_id` picks which organization's office the shell asks (the real
    /// implementation looks up that connection, and refuses an id it has none for with `not_found`).
    fn org_snapshot(&self, organization_id: &str) -> Result<OrgSnapshot, ApiError>;

    /// Send an organization command to HQ and return its answer. A retry with the same
    /// `command_id` gets the same answer and changes nothing.
    fn org_command(
        &self,
        organization_id: &str,
        command: &OrgCommand,
    ) -> Result<CommandAccepted, ApiError>;

    /// True while the data this connection serves is invented (the mock office), so the UI says so.
    /// Every implementation says so: a stand-in that forgot would show invented data as real.
    fn serves_sample_data(&self) -> bool;
}
