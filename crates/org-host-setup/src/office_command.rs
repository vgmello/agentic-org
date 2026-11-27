//! `agentic-org office` host-level commands (`list`, `restart`, `join`, `remove-pending`,
//! `re-enroll`) and shared CLI session establishment (`TASK-113`, FC-019, FC-020, FC-021).
//!
//! Authentication selection for CLI commands:
//! - When `--login <user>` is supplied, signs in via `account.verify` after verifying the host
//!   binary (`verify_host` + `may_receive_secrets`).
//! - When `--login` is omitted, tries `session.local` (immediate on local authless HTTP
//!   instances) and falls back to browser-approved CLI sign-in (`cli.approval.*`), printing
//!   the approval URL to stderr so `--json` stdout stays pure JSON.
//! - Satellite bootstrap commands (`join`, `remove-pending`, `re-enroll`) also support
//!   sessionless `local-bootstrap` on fresh HTTPS satellites with zero administrator accounts.

use std::io::Write;

use org_host_control::ops::{
    OfficeJoin, OfficeJoinParams, OfficeJoinResult, OfficeList, OfficeListParams, OfficeReEnroll,
    OfficeReEnrollParams, OfficeRemovePending, OfficeRemovePendingParams, OfficeRestart,
    OfficeRestartParams, SessionLocal, SessionLocalParams,
};
use org_host_control::{
    Administrator, HostOpResult, InstanceStatus, InstanceStatusParams, InvitationTokenSecret,
    SessionHandle,
};
use org_proto::{Call, ErrorCode, ListOfficesResult, OfficeInfo};
use serde::Serialize;

use super::cli_session::{
    connect_and_sign_in, connect_cli_session, connect_local_peer, end_session, AskPassword, Target,
    APPROVAL_MAX_WAIT, APPROVAL_POLL_INTERVAL,
};
use super::wire::registered;
use org_host::cli_error::{refused_by, HostCliError};
use org_host::human::Wire;

/// Help text for `agentic-org office`.
pub const HELP: &str = "agentic-org office <subcommand>\n\
Use when: inspecting offices, restarting an offline office, or managing HQ/satellite mTLS federation.\n\n\
Subcommands:\n  \
list [--org <org_id>] [--login <login>] [--name <instance>] [--json]\n      \
List offices visible to the account with their live readiness status\n      \
Use when: finding office IDs, HQ/satellite roles, or checking live readiness and crash backoff.\n  \
restart <office_id> [--login <login>] [--name <instance>] [--json]\n      \
Restart an offline or unavailable office (requires administrator or organization owner)\n      \
Use when: recovering an offline or crashed office without restarting the whole host.\n  \
invite <office_name> --address <host:port> [--address <host:port>...] [--org <org_id>] [--ttl <secs>] [--re-enroll-office <office_id>] [--login <login>] [--name <instance>] [--json]\n      \
Create a one-time satellite office invitation on HQ (owner-only)\n      \
Use when: starting satellite enrollment from HQ (or re-enrollment with --re-enroll-office).\n  \
invitations [--org <org_id>] [--login <login>] [--name <instance>] [--json]\n      \
List satellite office invitations on HQ with status and expiry (check codes appear only on the satellite)\n      \
Use when: inspecting pending or expired satellite invitations on HQ before approving or cancelling.\n  \
approve <invitation_id> --code <check_code> [--org <org_id>] [--login <login>] [--name <instance>] [--json]\n      \
Approve a pending satellite enrollment on HQ using its check code\n      \
Use when: completing satellite enrollment on HQ after reading the out-of-band check code from the satellite.\n  \
decline <invitation_id> [--org <org_id>] [--login <login>] [--name <instance>] [--json]\n      \
Decline an active satellite invitation on HQ\n      \
Use when: rejecting a pending satellite join request on HQ.\n  \
cancel <invitation_id> [--org <org_id>] [--login <login>] [--name <instance>] [--json]\n      \
Cancel an active satellite invitation on HQ\n      \
Use when: revoking an unused satellite invitation on HQ before approval completes.\n  \
remove <office_id> [--org <org_id>] [--login <login>] [--name <instance>] [--json]\n      \
Revoke a satellite office's certificates and remove it on HQ\n      \
Use when: decommissioning a satellite office from HQ and revoking its Org-CA mTLS certificates.\n  \
sync-status [--org <org_id>] [--office <office_id>] [--login <login>] [--name <instance>] [--json]\n      \
Show live replication and federation sync status for an HQ or satellite office\n      \
Use when: verifying live mTLS federation and replication health on HQ or a satellite.\n  \
join (--invitation-file <path> | --invitation-stdin) --yes [--login <login>] [--name <instance>] [--json]\n      \
Join this host as a satellite office using an HQ-issued invitation token\n      \
Use when: enrolling this remote host as a new satellite office using an HQ invitation token.\n  \
remove-pending <office_id> [--login <login>] [--name <instance>] [--json]\n      \
Cancel and remove a pending satellite join before approval completes\n      \
Use when: aborting an unapproved join on the satellite host before HQ approves it.\n  \
re-enroll <office_id> (--invitation-file <path> | --invitation-stdin) --yes [--login <login>] [--name <instance>] [--json]\n      \
Re-enroll an existing satellite onto a replacement HQ after R1 import (D25)\n      \
Use when: reconnecting an existing satellite office to a replacement HQ after Git import.\n\n\
When --login is omitted, uses the local authless session if authentication is disabled,\n\
or prints a browser approval link to stderr when authentication is enabled.";

/// What `office restart` and `office remove-pending` answer once the host completed them.
/// The keys are in the order the commands' earlier `json!` maps wrote them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OfficeOperationCompleted {
    pub office_id: String,
    /// The operation's ID: the same ID replays the same operation.
    pub operation_id: String,
    pub outcome: OfficeOperationOutcome,
}

/// How a host office operation ended; a refusal is an error, so only `completed` is left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OfficeOperationOutcome {
    Completed,
}

/// `agentic-org office list` for registered instance `name`.
pub fn office_list(
    name: Option<&str>,
    login: Option<&str>,
    org_id: Option<&str>,
) -> Result<ListOfficesResult, HostCliError> {
    let (_, id, socket) = registered(&org_host::human::paths(name)?, name)?;
    let mut err_out = std::io::stderr().lock();
    list(
        &Target {
            socket: &socket,
            instance_id: &id,
            name,
        },
        login,
        org_id,
        &mut org_host::human::password,
        &mut err_out,
    )
}

/// `agentic-org office restart` for registered instance `name`.
pub fn office_restart(
    name: Option<&str>,
    login: Option<&str>,
    office_id: &str,
) -> Result<OfficeOperationCompleted, HostCliError> {
    let (_, id, socket) = registered(&org_host::human::paths(name)?, name)?;
    let mut err_out = std::io::stderr().lock();
    restart(
        &Target {
            socket: &socket,
            instance_id: &id,
            name,
        },
        login,
        office_id,
        &mut org_host::human::password,
        &mut err_out,
    )
}

/// `agentic-org office join` for registered instance `name`: the satellite's enrollment,
/// with its check code while HQ has not approved it.
pub fn office_join(
    name: Option<&str>,
    login: Option<&str>,
    invitation: InvitationTokenSecret,
    consent: bool,
) -> Result<OfficeJoinResult, HostCliError> {
    let (_, id, socket) = registered(&org_host::human::paths(name)?, name)?;
    let target = Target {
        socket: &socket,
        instance_id: &id,
        name,
    };
    let operation_id =
        org_host::ids::uuid_v7().map_err(|_| "no operation ID could be generated")?;
    let mut err_out = std::io::stderr().lock();
    let answer = run_join_authority_call::<OfficeJoin, _>(
        &target,
        login,
        &mut org_host::human::password,
        &mut err_out,
        move |session| OfficeJoinParams {
            session,
            operation_id: operation_id.clone(),
            invitation,
            consent,
        },
    )?;
    match answer {
        Ok(HostOpResult::Completed { result }) => Ok(result),
        Ok(HostOpResult::Rejected { refusal }) => Err(HostCliError::refused(
            refusal.code,
            format!("the host refused the join: {}", refusal.message),
        )),
        Ok(_) => Err("the service's answer is not one this command knows".into()),
        Err(code @ ErrorCode::AlreadyExists) => Err(HostCliError::refused(
            code,
            "this office is already enrolled on this host",
        )),
        Err(code @ ErrorCode::Expired) => Err(HostCliError::refused(
            code,
            "the invitation token has expired",
        )),
        Err(code @ ErrorCode::NotAuthorized) => Err(HostCliError::refused(
            code,
            "only the instance administrator may join an office",
        )),
        Err(code) => Err(refused_by("the office join", code)),
    }
}

/// `agentic-org office remove-pending <office_id>` for registered instance `name`.
pub fn office_remove_pending(
    name: Option<&str>,
    login: Option<&str>,
    office_id: &str,
) -> Result<OfficeOperationCompleted, HostCliError> {
    let (_, id, socket) = registered(&org_host::human::paths(name)?, name)?;
    let target = Target {
        socket: &socket,
        instance_id: &id,
        name,
    };
    let operation_id =
        org_host::ids::uuid_v7().map_err(|_| "no operation ID could be generated")?;
    let mut err_out = std::io::stderr().lock();
    let answer = run_join_authority_call::<OfficeRemovePending, _>(
        &target,
        login,
        &mut org_host::human::password,
        &mut err_out,
        |session| OfficeRemovePendingParams {
            session,
            operation_id: operation_id.clone(),
            office_id: office_id.to_owned(),
        },
    )?;
    match answer {
        Ok(HostOpResult::Completed { .. }) => Ok(completed(office_id, operation_id)),
        Ok(HostOpResult::Rejected { refusal }) => Err(HostCliError::refused(
            refusal.code,
            format!(
                "the host refused removing pending join: {}",
                refusal.message
            ),
        )),
        Ok(_) => Err("the service's answer is not one this command knows".into()),
        Err(code @ ErrorCode::NotFound) => Err(HostCliError::refused(
            code,
            format!("no pending join found for office {office_id}"),
        )),
        Err(code @ ErrorCode::Conflict) => {
            Err(HostCliError::refused(code, "office join already completed"))
        }
        Err(code) => Err(refused_by("removing the pending join", code)),
    }
}

/// `agentic-org office re-enroll <office_id>` (D25) for registered instance `name`.
pub fn office_re_enroll(
    name: Option<&str>,
    login: Option<&str>,
    office_id: &str,
    invitation: InvitationTokenSecret,
    consent: bool,
) -> Result<OfficeJoinResult, HostCliError> {
    let (_, id, socket) = registered(&org_host::human::paths(name)?, name)?;
    let target = Target {
        socket: &socket,
        instance_id: &id,
        name,
    };
    let operation_id =
        org_host::ids::uuid_v7().map_err(|_| "no operation ID could be generated")?;
    let mut err_out = std::io::stderr().lock();
    let answer = run_join_authority_call::<OfficeReEnroll, _>(
        &target,
        login,
        &mut org_host::human::password,
        &mut err_out,
        move |session| OfficeReEnrollParams {
            session,
            operation_id: operation_id.clone(),
            office_id: office_id.to_owned(),
            invitation,
            consent,
        },
    )?;
    match answer {
        Ok(HostOpResult::Completed { result }) => Ok(result),
        Ok(HostOpResult::Rejected { refusal }) => Err(HostCliError::refused(
            refusal.code,
            format!("the host refused re-enrollment: {}", refusal.message),
        )),
        Ok(_) => Err("the service's answer is not one this command knows".into()),
        Err(code @ ErrorCode::NotFound) => Err(HostCliError::refused(
            code,
            format!("satellite office {office_id} not found"),
        )),
        Err(code @ ErrorCode::Expired) => Err(HostCliError::refused(
            code,
            "the re-enrollment invitation token has expired",
        )),
        Err(code) => Err(refused_by("the re-enrollment", code)),
    }
}

fn completed(office_id: &str, operation_id: String) -> OfficeOperationCompleted {
    OfficeOperationCompleted {
        office_id: office_id.to_owned(),
        operation_id,
        outcome: OfficeOperationOutcome::Completed,
    }
}

/// Executes a satellite join-authority call (`office.join`, `office.remove_pending`,
/// `office.re_enroll`) using:
/// 1. `--login <user>` password sign-in when explicitly given,
/// 2. `session.local` when local authless mode is active,
/// 3. sessionless `local-bootstrap` when the host has no administrator account, or
/// 4. browser-approved CLI sign-in (`cli.approval.*`) when authentication is enabled.
fn run_join_authority_call<C, F>(
    target: &Target<'_>,
    login: Option<&str>,
    password: AskPassword<'_>,
    prompt_out: &mut dyn Write,
    build_params: F,
) -> Result<Result<C::Result, ErrorCode>, HostCliError>
where
    C: Call,
    F: FnOnce(Option<SessionHandle>) -> C::Params,
{
    if let Some(user) = login {
        let (mut wire, session) = connect_and_sign_in(target, user, password)?;
        let handle = SessionHandle::new(session.expose().into()).expect("a live handle");
        let answer = wire.call_answer::<C>(&build_params(Some(handle)), None);
        end_session(&mut wire, &session);
        return Ok(answer?);
    }

    let mut wire = connect_local_peer(target)?;
    match wire.call_answer::<SessionLocal>(&SessionLocalParams {}, None)? {
        Ok(signed) => {
            let handle = SessionHandle::new(signed.session.expose().into()).expect("a live handle");
            let answer = wire.call_answer::<C>(&build_params(Some(handle)), None);
            end_session(&mut wire, &signed.session);
            return Ok(answer?);
        }
        Err(ErrorCode::NotAuthorized) => {}
        Err(code) => return Err(refused_by("local sign-in", code)),
    }

    // Check open `instance.status` BEFORE building or sending secret-bearing join parameters:
    // only a fresh host with `Administrator::None` admits sessionless `local-bootstrap`.
    let status = match wire.call_answer::<InstanceStatus>(&InstanceStatusParams {}, None)? {
        Ok(status) => status,
        Err(code) => return Err(refused_by("the status lookup", code)),
    };
    if status.administrator == Administrator::None {
        return Ok(wire.call_answer::<C>(&build_params(None), None)?);
    }

    // Host has an administrator and authentication is enabled: authenticate via browser approval
    // on the already-verified `host.sock` wire before transmitting any join secret.
    let (mut wire, session, _, _) = super::cli_session::approve_on_wire(
        wire,
        APPROVAL_POLL_INTERVAL,
        APPROVAL_MAX_WAIT,
        prompt_out,
    )?;
    let handle = SessionHandle::new(session.expose().into()).expect("a live handle");
    let answer = wire.call_answer::<C>(&build_params(Some(handle)), None);
    end_session(&mut wire, &session);
    Ok(answer?)
}

/// Lists offices visible to the signed-in account, with the remote offices of each
/// organization's directory. No office's `endpoint` (a local socket path) is returned.
pub fn list(
    target: &Target<'_>,
    login: Option<&str>,
    org_id: Option<&str>,
    password: AskPassword<'_>,
    prompt_out: &mut dyn Write,
) -> Result<ListOfficesResult, HostCliError> {
    let (mut wire, session) = connect_cli_session(target, login, password, prompt_out)?;
    let handle = SessionHandle::new(session.expose().into()).expect("a live handle");
    let answer = wire.call_answer::<OfficeList>(
        &OfficeListParams {
            session: handle,
            org_id: org_id.map(str::to_owned),
        },
        None,
    );
    let mut listed = match answer {
        Ok(Ok(listed)) => listed,
        Ok(Err(code)) => {
            end_session(&mut wire, &session);
            return Err(match code {
                ErrorCode::NotFound => HostCliError::refused(code, "organization not found"),
                ErrorCode::NotAuthorized => {
                    HostCliError::refused(code, "not authorized to list offices")
                }
                code => refused_by("the office list", code),
            });
        }
        Err(err) => {
            end_session(&mut wire, &session);
            return Err(err.into());
        }
    };
    merge_remote_directory_offices(&mut wire, &session, &mut listed.offices);
    end_session(&mut wire, &session);
    for office in &mut listed.offices {
        office.endpoint = None;
    }
    Ok(listed)
}

fn merge_remote_directory_offices(
    wire: &mut Wire,
    session: &SessionHandle,
    offices: &mut Vec<OfficeInfo>,
) {
    let mut by_org: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    // Prefer Active local offices first for each organization, then fall back to non-Active ones.
    for active_pass in [true, false] {
        for office in offices.iter() {
            let is_active = office.status == org_proto::OfficeStatus::Active;
            if is_active != active_pass {
                continue;
            }
            let entry = by_org.entry(office.organization_id.clone()).or_default();
            if !entry.iter().any(|id| id == &office.id) {
                entry.push(office.id.clone());
            }
        }
    }
    for (org_id, candidates) in by_org {
        for local_office_id in candidates {
            let Ok(mut channel) = super::project_command::OfficeSession::open_for_office(
                wire,
                session,
                Some(&org_id),
                Some(&local_office_id),
            ) else {
                continue;
            };
            let Ok(Ok(dir)) = channel.call::<org_proto::federation::OfficeDirectoryList>(
                wire,
                session,
                &org_proto::federation::OfficeDirectoryListParams {},
            ) else {
                continue;
            };
            for remote in dir.offices {
                if !offices.iter().any(|existing| existing.id == remote.id) {
                    offices.push(OfficeInfo {
                        id: remote.id,
                        organization_id: remote.organization_id,
                        name: remote.name,
                        role: remote.role,
                        host: format!("Remote ({})", remote.slug),
                        endpoint: None,
                        status: org_proto::OfficeStatus::Active,
                        status_reason: None,
                        next_restart_at: None,
                    });
                }
            }
            break;
        }
    }
}

/// Requests a restart of `office_id` through `office.restart` on `host.sock`.
pub fn restart(
    target: &Target<'_>,
    login: Option<&str>,
    office_id: &str,
    password: AskPassword<'_>,
    prompt_out: &mut dyn Write,
) -> Result<OfficeOperationCompleted, HostCliError> {
    let operation_id =
        org_host::ids::uuid_v7().map_err(|_| "no operation ID could be generated")?;
    let (mut wire, session) = connect_cli_session(target, login, password, prompt_out)?;
    let handle = SessionHandle::new(session.expose().into()).expect("a live handle");
    let answer = wire.call_answer::<OfficeRestart>(
        &OfficeRestartParams {
            session: handle,
            operation_id: operation_id.clone(),
            office_id: office_id.to_owned(),
        },
        None,
    );
    end_session(&mut wire, &session);
    let refused = |code, message: &str| Err(HostCliError::refused(code, message));
    match answer? {
        Ok(HostOpResult::Completed { .. }) => Ok(completed(office_id, operation_id)),
        Ok(HostOpResult::Rejected { refusal }) => Err(HostCliError::refused(
            refusal.code,
            format!("the host refused the restart: {}", refusal.message),
        )),
        Ok(_) => Err("the service's answer is not one this command knows".into()),
        Err(code @ ErrorCode::NotFound) => refused(code, &format!("office {office_id} not found")),
        Err(code @ ErrorCode::NotAuthorized) => refused(
            code,
            "only the instance administrator or organization owner may restart an office",
        ),
        Err(code @ ErrorCode::Conflict) => refused(
            code,
            "the office is not offline or unavailable; no restart needed",
        ),
        Err(code @ ErrorCode::NotReady) => {
            refused(code, "the office is still starting or being created")
        }
        Err(code @ ErrorCode::Unavailable) => {
            refused(code, "a restart cannot recover this office right now")
        }
        Err(code) => Err(refused_by("the restart", code)),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn help_includes_use_when_guidance_for_every_subcommand() {
        assert!(
            super::HELP.matches("Use when:").count() >= 13,
            "expected command + 12 subcommand Use when cues:\n{}",
            super::HELP
        );
    }
}
