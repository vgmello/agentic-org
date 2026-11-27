//! HQ satellite federation management and replication status CLI flows (`TASK-113`,
//! `FC-020`, `FC-024`, `FC-145`): `office invite`, `invitations`, `approve`, `decline`,
//! `cancel`, `remove`, and `sync-status` over grant-bound `host.sock` office channels.

use org_proto::federation::{
    OfficeInvitationActionParams, OfficeInvitationActionResult, OfficeInvitationApprove,
    OfficeInvitationApproveParams, OfficeInvitationApproveResult, OfficeInvitationCancel,
    OfficeInvitationCreate, OfficeInvitationCreateParams, OfficeInvitationCreateResult,
    OfficeInvitationDecline, OfficeInvitationList, OfficeInvitationListParams,
    OfficeInvitationListResult, OfficeRemove, OfficeRemoveParams, OfficeRemoveResult,
    OfficeSyncStatus, OfficeSyncStatusParams, OfficeSyncStatusResult,
};
use org_proto::Call;

use super::project_command::call_office;
use org_host::cli_error::{office_refused, HostCliError};

/// Input for `agentic-org office invite`.
#[derive(Debug, Clone)]
pub struct OfficeInviteInput<'a> {
    pub org_id: Option<&'a str>,
    pub office_name: &'a str,
    pub hq_addresses: Vec<String>,
    pub ttl_secs: Option<u64>,
    pub existing_office_id: Option<&'a str>,
}

/// [`call_office`] on HQ, with an office refusal of `action` as an error.
fn on_hq<C: Call>(
    instance_name: Option<&str>,
    login: Option<&str>,
    org_id: Option<&str>,
    action: &str,
    params: &C::Params,
) -> Result<C::Result, HostCliError> {
    call_office::<C>(instance_name, login, org_id, None, true, params)?
        .map_err(|error| office_refused(action, &error))
}

/// `agentic-org office invite <office_name> --address <host:port> ...`: the invitation and
/// its one-time token (a secret the satellite takes with `--invitation-stdin`).
pub fn office_invite(
    instance_name: Option<&str>,
    login: Option<&str>,
    input: &OfficeInviteInput<'_>,
) -> Result<OfficeInvitationCreateResult, HostCliError> {
    let office_name = input.office_name.trim();
    if office_name.is_empty() {
        return Err(HostCliError::usage("office name cannot be empty"));
    }
    let hq_addresses: Vec<String> = input
        .hq_addresses
        .iter()
        .map(|a| a.trim().to_owned())
        .filter(|a| !a.is_empty())
        .collect();
    if hq_addresses.is_empty() {
        return Err(HostCliError::usage(
            "at least one --address <host:port> is required",
        ));
    }
    let params = OfficeInvitationCreateParams {
        office_name: office_name.to_owned(),
        hq_addresses,
        ttl_secs: input.ttl_secs,
        existing_office_id: input.existing_office_id.map(str::to_owned),
    };
    on_hq::<OfficeInvitationCreate>(
        instance_name,
        login,
        input.org_id,
        "creating office invitation",
        &params,
    )
}

/// `agentic-org office invitations [--org <org_id>]` (no check codes: those appear only on
/// the satellite).
pub fn office_invitations(
    instance_name: Option<&str>,
    login: Option<&str>,
    org_id: Option<&str>,
) -> Result<OfficeInvitationListResult, HostCliError> {
    on_hq::<OfficeInvitationList>(
        instance_name,
        login,
        org_id,
        "listing office invitations",
        &OfficeInvitationListParams {},
    )
}

/// `agentic-org office approve <invitation_id> --code <check_code>`
pub fn office_approve(
    instance_name: Option<&str>,
    login: Option<&str>,
    org_id: Option<&str>,
    invitation_id: &str,
    check_code: &str,
) -> Result<OfficeInvitationApproveResult, HostCliError> {
    on_hq::<OfficeInvitationApprove>(
        instance_name,
        login,
        org_id,
        "approving office invitation",
        &OfficeInvitationApproveParams {
            invitation_id: invitation_id.to_owned(),
            check_code: check_code.trim().to_owned(),
        },
    )
}

/// `agentic-org office decline <invitation_id>`
pub fn office_decline(
    instance_name: Option<&str>,
    login: Option<&str>,
    org_id: Option<&str>,
    invitation_id: &str,
) -> Result<OfficeInvitationActionResult, HostCliError> {
    on_hq::<OfficeInvitationDecline>(
        instance_name,
        login,
        org_id,
        "declining office invitation",
        &OfficeInvitationActionParams {
            invitation_id: invitation_id.to_owned(),
        },
    )
}

/// `agentic-org office cancel <invitation_id>`
pub fn office_cancel(
    instance_name: Option<&str>,
    login: Option<&str>,
    org_id: Option<&str>,
    invitation_id: &str,
) -> Result<OfficeInvitationActionResult, HostCliError> {
    on_hq::<OfficeInvitationCancel>(
        instance_name,
        login,
        org_id,
        "cancelling office invitation",
        &OfficeInvitationActionParams {
            invitation_id: invitation_id.to_owned(),
        },
    )
}

/// `agentic-org office remove <office_id>`: the removed office and the certificate serials
/// HQ revoked.
pub fn office_remove(
    instance_name: Option<&str>,
    login: Option<&str>,
    org_id: Option<&str>,
    office_id: &str,
) -> Result<OfficeRemoveResult, HostCliError> {
    on_hq::<OfficeRemove>(
        instance_name,
        login,
        org_id,
        "removing satellite office",
        &OfficeRemoveParams {
            office_id: office_id.to_owned(),
        },
    )
}

/// `agentic-org office sync-status [--org <org_id>] [--office <office_id>]`
pub fn office_sync_status(
    instance_name: Option<&str>,
    login: Option<&str>,
    org_id: Option<&str>,
    office_id: Option<&str>,
) -> Result<OfficeSyncStatusResult, HostCliError> {
    call_office::<OfficeSyncStatus>(
        instance_name,
        login,
        org_id,
        office_id,
        false,
        &OfficeSyncStatusParams {},
    )?
    .map_err(|error| office_refused("reading office sync status", &error))
}
