//! HQ satellite federation management and replication status CLI flows (`TASK-113`,
//! `FC-020`, `FC-024`, `FC-145`): `office invite`, `invitations`, `approve`, `decline`,
//! `cancel`, `remove`, and `sync-status` over grant-bound `host.sock` office channels.

use std::io::Write;

use org_proto::federation::{
    OfficeInvitationActionParams, OfficeInvitationApprove, OfficeInvitationApproveParams,
    OfficeInvitationCancel, OfficeInvitationCreate, OfficeInvitationCreateParams,
    OfficeInvitationDecline, OfficeInvitationList, OfficeInvitationListParams, OfficeRemove,
    OfficeRemoveParams, OfficeSyncStatus, OfficeSyncStatusParams, WireInvitationSummary,
};
use org_proto::{ApiError, Call};

use super::office_command::{connect_cli_session, end_session, Target};
use super::project_command::OfficeSession;
use super::{registered, say};

/// Input for `agentic-org office invite`.
#[derive(Debug, Clone)]
pub struct OfficeInviteInput<'a> {
    pub org_id: Option<&'a str>,
    pub office_name: &'a str,
    pub hq_addresses: Vec<String>,
    pub ttl_secs: Option<u64>,
    pub existing_office_id: Option<&'a str>,
    pub json: bool,
}

fn run_office_rpc<C: Call>(
    instance_name: Option<&str>,
    login: Option<&str>,
    org_id: Option<&str>,
    office_id: Option<&str>,
    require_hq: bool,
    params: &C::Params,
) -> Result<Result<C::Result, ApiError>, String> {
    let (_, id, socket) = registered(&crate::human::paths(instance_name)?, instance_name)?;
    let target = Target {
        socket: &socket,
        instance_id: &id,
        name: instance_name,
    };
    let mut err_out = std::io::stderr().lock();
    let (mut wire, session) =
        connect_cli_session(&target, login, &mut crate::human::password, &mut err_out)?;
    let answer = (|| {
        let mut office = if require_hq {
            OfficeSession::open_hq(&mut wire, &session, org_id)?
        } else {
            OfficeSession::open_for_office(&mut wire, &session, org_id, office_id)?
        };
        office.call::<C>(&mut wire, &session, params)
    })();
    end_session(&mut wire, &session);
    answer
}

fn office_error(action: &str, error: ApiError) -> String {
    format!(
        "the office refused {action} ({:?}): {}",
        error.code, error.message
    )
}

/// `agentic-org office invite <office_name> --address <host:port> ...`
pub fn office_invite(
    instance_name: Option<&str>,
    login: Option<&str>,
    input: &OfficeInviteInput<'_>,
    out: &mut dyn Write,
) -> Result<(), String> {
    let office_name = input.office_name.trim();
    if office_name.is_empty() {
        return Err("office name cannot be empty".into());
    }
    let hq_addresses: Vec<String> = input
        .hq_addresses
        .iter()
        .map(|a| a.trim().to_owned())
        .filter(|a| !a.is_empty())
        .collect();
    if hq_addresses.is_empty() {
        return Err("at least one --address <host:port> is required".into());
    }
    let params = OfficeInvitationCreateParams {
        office_name: office_name.to_owned(),
        hq_addresses,
        ttl_secs: input.ttl_secs,
        existing_office_id: input.existing_office_id.map(str::to_owned),
    };
    let created = match run_office_rpc::<OfficeInvitationCreate>(
        instance_name,
        login,
        input.org_id,
        None,
        true,
        &params,
    )? {
        Ok(created) => created,
        Err(error) => return Err(office_error("creating office invitation", error)),
    };
    if input.json {
        let encoded = serde_json::to_string_pretty(&created)
            .map_err(|_| "could not encode invitation response".to_owned())?;
        return say(out, encoded);
    }
    say(
        out,
        format!(
            "Invitation created for satellite office \"{}\"\n  invitation_id: {}\n  office_id:     {}\n  expires_at_ms: {}\n\nOne-time invitation token (pass to the satellite via --invitation-stdin or --invitation-file):\n{}",
            created.invitation.office_name,
            created.invitation.invitation_id,
            created.invitation.office_id,
            created.invitation.expires_at_ms,
            created.token
        ),
    )
}

/// `agentic-org office invitations [--org <org_id>]`
pub fn office_invitations(
    instance_name: Option<&str>,
    login: Option<&str>,
    org_id: Option<&str>,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    let listed = match run_office_rpc::<OfficeInvitationList>(
        instance_name,
        login,
        org_id,
        None,
        true,
        &OfficeInvitationListParams {},
    )? {
        Ok(listed) => listed,
        Err(error) => return Err(office_error("listing office invitations", error)),
    };
    if json {
        let encoded = serde_json::to_string_pretty(&listed)
            .map_err(|_| "could not encode invitations list".to_owned())?;
        return say(out, encoded);
    }
    if listed.invitations.is_empty() {
        return say(out, "No invitations.");
    }
    for inv in &listed.invitations {
        say(out, format_invitation_line(inv))?;
    }
    Ok(())
}

fn format_invitation_line(inv: &WireInvitationSummary) -> String {
    let mut line = format!(
        "{}\tstatus={}\toffice={}\tname={}",
        inv.invitation_id, inv.status, inv.office_id, inv.office_name
    );
    if let Some(code) = &inv.check_code {
        line.push_str(&format!("\tcheck_code={code}"));
    }
    if let Some(serial) = &inv.issued_cert_serial_hex {
        line.push_str(&format!("\tcert={serial}"));
    }
    line.push_str(&format!("\texpires_at_ms={}", inv.expires_at_ms));
    line
}

/// `agentic-org office approve <invitation_id> --code <check_code>`
pub fn office_approve(
    instance_name: Option<&str>,
    login: Option<&str>,
    org_id: Option<&str>,
    invitation_id: &str,
    check_code: &str,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    let approved = match run_office_rpc::<OfficeInvitationApprove>(
        instance_name,
        login,
        org_id,
        None,
        true,
        &OfficeInvitationApproveParams {
            invitation_id: invitation_id.to_owned(),
            check_code: check_code.trim().to_owned(),
        },
    )? {
        Ok(approved) => approved,
        Err(error) => return Err(office_error("approving office invitation", error)),
    };
    if json {
        let encoded = serde_json::to_string_pretty(&approved)
            .map_err(|_| "could not encode approval response".to_owned())?;
        return say(out, encoded);
    }
    say(
        out,
        format!(
            "Approved satellite office {} (slug={}, code={}, cert={}).",
            approved.office_id, approved.slug, approved.office_code, approved.cert_serial_hex
        ),
    )
}

/// `agentic-org office decline <invitation_id>`
pub fn office_decline(
    instance_name: Option<&str>,
    login: Option<&str>,
    org_id: Option<&str>,
    invitation_id: &str,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    let declined = match run_office_rpc::<OfficeInvitationDecline>(
        instance_name,
        login,
        org_id,
        None,
        true,
        &OfficeInvitationActionParams {
            invitation_id: invitation_id.to_owned(),
        },
    )? {
        Ok(declined) => declined,
        Err(error) => return Err(office_error("declining office invitation", error)),
    };
    if json {
        let encoded = serde_json::to_string_pretty(&declined)
            .map_err(|_| "could not encode decline response".to_owned())?;
        return say(out, encoded);
    }
    say(
        out,
        format!(
            "Declined invitation {} (office {}, status={}).",
            declined.invitation.invitation_id,
            declined.invitation.office_id,
            declined.invitation.status
        ),
    )
}

/// `agentic-org office cancel <invitation_id>`
pub fn office_cancel(
    instance_name: Option<&str>,
    login: Option<&str>,
    org_id: Option<&str>,
    invitation_id: &str,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    let cancelled = match run_office_rpc::<OfficeInvitationCancel>(
        instance_name,
        login,
        org_id,
        None,
        true,
        &OfficeInvitationActionParams {
            invitation_id: invitation_id.to_owned(),
        },
    )? {
        Ok(cancelled) => cancelled,
        Err(error) => return Err(office_error("cancelling office invitation", error)),
    };
    if json {
        let encoded = serde_json::to_string_pretty(&cancelled)
            .map_err(|_| "could not encode cancel response".to_owned())?;
        return say(out, encoded);
    }
    say(
        out,
        format!(
            "Cancelled invitation {} (office {}, status={}).",
            cancelled.invitation.invitation_id,
            cancelled.invitation.office_id,
            cancelled.invitation.status
        ),
    )
}

/// `agentic-org office remove <office_id>`
pub fn office_remove(
    instance_name: Option<&str>,
    login: Option<&str>,
    org_id: Option<&str>,
    office_id: &str,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    let removed = match run_office_rpc::<OfficeRemove>(
        instance_name,
        login,
        org_id,
        None,
        true,
        &OfficeRemoveParams {
            office_id: office_id.to_owned(),
        },
    )? {
        Ok(removed) => removed,
        Err(error) => return Err(office_error("removing satellite office", error)),
    };
    if json {
        let encoded = serde_json::to_string_pretty(&removed)
            .map_err(|_| "could not encode remove response".to_owned())?;
        return say(out, encoded);
    }
    say(
        out,
        format!(
            "Removed satellite office {} (revoked {} certificate(s)).",
            removed.office_id,
            removed.revoked_serials.len()
        ),
    )
}

/// `agentic-org office sync-status [--org <org_id>] [--office <office_id>]`
pub fn office_sync_status(
    instance_name: Option<&str>,
    login: Option<&str>,
    org_id: Option<&str>,
    office_id: Option<&str>,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    let status = match run_office_rpc::<OfficeSyncStatus>(
        instance_name,
        login,
        org_id,
        office_id,
        false,
        &OfficeSyncStatusParams {},
    )? {
        Ok(status) => status,
        Err(error) => return Err(office_error("reading office sync status", error)),
    };
    if json {
        let encoded = serde_json::to_string_pretty(&status)
            .map_err(|_| "could not encode sync status".to_owned())?;
        return say(out, encoded);
    }
    say(
        out,
        format!(
            "org_id={}\tlocal_office={}\thq_office={}\tepoch={}\tseq={}\thighest_epoch={}\tprotocol_level={}\trestore_pending={}\tinitial_snapshot_required={}\tpending_work={}\tpending_messages={}",
            status.org_id,
            status.local_office,
            status.hq_office,
            status.epoch,
            status.seq,
            status.highest_epoch,
            status.protocol_level,
            status.restore_pending,
            status.initial_snapshot_required,
            status.pending_work_count,
            status.pending_message_count
        ),
    )
}
