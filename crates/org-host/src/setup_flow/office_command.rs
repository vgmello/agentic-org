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
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use org_host_control::ops::{
    AccountVerify, AccountVerifyParams, OfficeJoin, OfficeJoinParams, OfficeList, OfficeListParams,
    OfficeReEnroll, OfficeReEnrollParams, OfficeRemovePending, OfficeRemovePendingParams,
    OfficeRestart, OfficeRestartParams, SessionEnd, SessionLocal, SessionLocalParams,
    SessionParams,
};
use org_host_control::{
    Administrator, HostOpResult, InstanceStatus, InstanceStatusParams, InvitationTokenSecret,
    Password, SessionHandle,
};
use org_proto::{Call, ErrorCode, OfficeInfo};

use super::{connect_verified, not_running, registered, say};
use crate::human::Wire;

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

/// What office commands need from the person: their password, asked once after the host is verified.
pub(crate) type AskPassword<'a> = &'a mut dyn FnMut() -> Result<Password, String>;

/// The instance an office command acts on.
pub(crate) struct Target<'a> {
    pub(crate) socket: &'a Path,
    pub(crate) instance_id: &'a str,
    pub(crate) name: Option<&'a str>,
}

const APPROVAL_POLL_INTERVAL: Duration = Duration::from_millis(250);
const APPROVAL_MAX_WAIT: Duration = Duration::from_secs(10 * 60);

/// `agentic-org office list` for registered instance `name`.
pub fn office_list(
    name: Option<&str>,
    login: Option<&str>,
    org_id: Option<&str>,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    let (_, id, socket) = registered(&crate::human::paths(name)?, name)?;
    let mut err_out = std::io::stderr().lock();
    list(
        &Target {
            socket: &socket,
            instance_id: &id,
            name,
        },
        login,
        org_id,
        json,
        &mut crate::human::password,
        &mut err_out,
        out,
    )
}

/// `agentic-org office restart` for registered instance `name`.
pub fn office_restart(
    name: Option<&str>,
    login: Option<&str>,
    office_id: &str,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    let (_, id, socket) = registered(&crate::human::paths(name)?, name)?;
    let mut err_out = std::io::stderr().lock();
    restart(
        &Target {
            socket: &socket,
            instance_id: &id,
            name,
        },
        login,
        office_id,
        json,
        &mut crate::human::password,
        &mut err_out,
        out,
    )
}

/// `agentic-org office join` for registered instance `name`.
pub fn office_join(
    name: Option<&str>,
    login: Option<&str>,
    invitation: InvitationTokenSecret,
    consent: bool,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    let (_, id, socket) = registered(&crate::human::paths(name)?, name)?;
    let target = Target {
        socket: &socket,
        instance_id: &id,
        name,
    };
    let operation_id = crate::ids::uuid_v7().map_err(|_| "no operation ID could be generated")?;
    let mut err_out = std::io::stderr().lock();
    let answer = run_join_authority_call::<OfficeJoin, _>(
        &target,
        login,
        &mut crate::human::password,
        &mut err_out,
        move |session| {
            Ok(OfficeJoinParams {
                session,
                operation_id: operation_id.clone(),
                invitation,
                consent,
            })
        },
    )?;
    match answer {
        Ok(HostOpResult::Completed { result }) => {
            if json {
                let encoded = serde_json::to_string_pretty(&result)
                    .map_err(|_| "could not encode office join response".to_owned())?;
                say(out, encoded)
            } else if let Some(code) = &result.check_code {
                say(
                    out,
                    format!(
                        "Satellite office {} ({}) join pending for organization {} ({}).\nCheck code: {code}",
                        result.office_id, result.office_name, result.org_name, result.org_id
                    ),
                )
            } else {
                say(
                    out,
                    format!(
                        "Satellite office {} ({}) joined organization {} ({}).",
                        result.office_id, result.office_name, result.org_name, result.org_id
                    ),
                )
            }
        }
        Ok(HostOpResult::Rejected { refusal }) => {
            Err(format!("the host refused the join: {}", refusal.message))
        }
        Ok(_) => Err("the service's answer is not one this command knows".into()),
        Err(ErrorCode::AlreadyExists) => Err("this office is already enrolled on this host".into()),
        Err(ErrorCode::Expired) => Err("the invitation token has expired".into()),
        Err(ErrorCode::NotAuthorized) => {
            Err("only the instance administrator may join an office".into())
        }
        Err(code) => Err(format!("the service refused office join ({code:?})")),
    }
}

/// `agentic-org office remove-pending <office_id>` for registered instance `name`.
pub fn office_remove_pending(
    name: Option<&str>,
    login: Option<&str>,
    office_id: &str,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    let (_, id, socket) = registered(&crate::human::paths(name)?, name)?;
    let target = Target {
        socket: &socket,
        instance_id: &id,
        name,
    };
    let operation_id = crate::ids::uuid_v7().map_err(|_| "no operation ID could be generated")?;
    let mut err_out = std::io::stderr().lock();
    let answer = run_join_authority_call::<OfficeRemovePending, _>(
        &target,
        login,
        &mut crate::human::password,
        &mut err_out,
        |session| {
            Ok(OfficeRemovePendingParams {
                session,
                operation_id: operation_id.clone(),
                office_id: office_id.to_owned(),
            })
        },
    )?;
    match answer {
        Ok(HostOpResult::Completed { .. }) => {
            if json {
                say(
                    out,
                    serde_json::to_string(&serde_json::json!({
                        "operation_id": operation_id,
                        "office_id": office_id,
                        "outcome": "completed",
                    }))
                    .map_err(|_| "could not encode response".to_owned())?,
                )
            } else {
                say(out, format!("Removed pending join for office {office_id}."))
            }
        }
        Ok(HostOpResult::Rejected { refusal }) => Err(format!(
            "the host refused removing pending join: {}",
            refusal.message
        )),
        Ok(_) => Err("the service's answer is not one this command knows".into()),
        Err(ErrorCode::NotFound) => Err(format!("no pending join found for office {office_id}")),
        Err(ErrorCode::Conflict) => Err("office join already completed".into()),
        Err(code) => Err(format!("the service refused remove-pending ({code:?})")),
    }
}

/// `agentic-org office re-enroll <office_id>` (D25) for registered instance `name`.
pub fn office_re_enroll(
    name: Option<&str>,
    login: Option<&str>,
    office_id: &str,
    invitation: InvitationTokenSecret,
    consent: bool,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    let (_, id, socket) = registered(&crate::human::paths(name)?, name)?;
    let target = Target {
        socket: &socket,
        instance_id: &id,
        name,
    };
    let operation_id = crate::ids::uuid_v7().map_err(|_| "no operation ID could be generated")?;
    let mut err_out = std::io::stderr().lock();
    let answer = run_join_authority_call::<OfficeReEnroll, _>(
        &target,
        login,
        &mut crate::human::password,
        &mut err_out,
        move |session| {
            Ok(OfficeReEnrollParams {
                session,
                operation_id: operation_id.clone(),
                office_id: office_id.to_owned(),
                invitation,
                consent,
            })
        },
    )?;
    match answer {
        Ok(HostOpResult::Completed { result }) => {
            if json {
                let encoded = serde_json::to_string_pretty(&result)
                    .map_err(|_| "could not encode re-enroll response".to_owned())?;
                say(out, encoded)
            } else if let Some(code) = &result.check_code {
                say(
                    out,
                    format!(
                        "Satellite office {} re-enrollment pending for replacement HQ {}.\nCheck code: {code}",
                        result.office_id, result.hq_office_id
                    ),
                )
            } else {
                say(
                    out,
                    format!(
                        "Satellite office {} re-enrolled onto replacement HQ {}.",
                        result.office_id, result.hq_office_id
                    ),
                )
            }
        }
        Ok(HostOpResult::Rejected { refusal }) => Err(format!(
            "the host refused re-enrollment: {}",
            refusal.message
        )),
        Ok(_) => Err("the service's answer is not one this command knows".into()),
        Err(ErrorCode::NotFound) => Err(format!("satellite office {office_id} not found")),
        Err(ErrorCode::Expired) => Err("the re-enrollment invitation token has expired".into()),
        Err(code) => Err(format!("the service refused re-enroll ({code:?})")),
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
) -> Result<Result<C::Result, ErrorCode>, String>
where
    C: Call,
    F: FnOnce(Option<SessionHandle>) -> Result<C::Params, String>,
{
    if let Some(user) = login {
        let (mut wire, session) = connect_and_sign_in(target, user, password)?;
        let handle = SessionHandle::new(session.expose().into()).expect("a live handle");
        let params = build_params(Some(handle))?;
        let answer = wire.call_answer::<C>(&params, None);
        end_session(&mut wire, &session);
        return answer;
    }

    let mut wire = connect_local_peer(target)?;
    match wire.call_answer::<SessionLocal>(&SessionLocalParams {}, None)? {
        Ok(signed) => {
            let handle = SessionHandle::new(signed.session.expose().into()).expect("a live handle");
            let params = build_params(Some(handle))?;
            let answer = wire.call_answer::<C>(&params, None);
            end_session(&mut wire, &signed.session);
            return answer;
        }
        Err(ErrorCode::NotAuthorized) => {}
        Err(code) => return Err(format!("the service refused local sign-in ({code:?})")),
    }

    // Check open `instance.status` BEFORE building or sending secret-bearing join parameters:
    // only a fresh host with `Administrator::None` admits sessionless `local-bootstrap`.
    let status = match wire.call_answer::<InstanceStatus>(&InstanceStatusParams {}, None)? {
        Ok(status) => status,
        Err(code) => return Err(format!("the service refused status lookup ({code:?})")),
    };
    if status.administrator == Administrator::None {
        let params = build_params(None)?;
        return wire.call_answer::<C>(&params, None);
    }

    // Host has an administrator and authentication is enabled: authenticate via browser approval
    // on the already-verified `host.sock` wire before transmitting any join secret.
    let (mut wire, session, _, _) = super::sign_in_command::approve_on_wire(
        wire,
        APPROVAL_POLL_INTERVAL,
        APPROVAL_MAX_WAIT,
        prompt_out,
    )?;
    let handle = SessionHandle::new(session.expose().into()).expect("a live handle");
    let params = build_params(Some(handle))?;
    let answer = wire.call_answer::<C>(&params, None);
    end_session(&mut wire, &session);
    answer
}

pub(super) fn connect_local_peer(target: &Target<'_>) -> Result<Wire, String> {
    let refused = std::cell::RefCell::new(None);
    connect_verified(target.socket, target.instance_id, |stream: &UnixStream| {
        crate::human::verify_host(stream)
            .map(|_| ())
            .inspect_err(|why| *refused.borrow_mut() = Some(why.clone()))
    })
    .map_err(|why| {
        refused
            .borrow_mut()
            .take()
            .unwrap_or_else(|| not_running(target.name, why))
    })
}

pub(super) fn end_session(wire: &mut Wire, session: &SessionHandle) {
    let handle = SessionHandle::new(session.expose().into()).expect("a live handle");
    let _ = wire.call::<SessionEnd>(&SessionParams { session: handle }, None);
}

/// Establishes an authenticated `host.sock` session for a CLI command:
/// - `--login <user>` -> verified-host password sign-in (`account.verify`).
/// - no `--login` -> `session.local` (instant on local authless HTTP), falling back to
///   browser-approved CLI sign-in (`cli.approval.*`) when authentication is enabled.
pub(super) fn connect_cli_session(
    target: &Target<'_>,
    login: Option<&str>,
    password: AskPassword<'_>,
    prompt_out: &mut dyn Write,
) -> Result<(Wire, SessionHandle), String> {
    if let Some(user) = login {
        return connect_and_sign_in(target, user, password);
    }
    let mut wire = connect_local_peer(target)?;
    match wire.call_answer::<SessionLocal>(&SessionLocalParams {}, None)? {
        Ok(signed) => Ok((wire, signed.session)),
        Err(ErrorCode::NotAuthorized) => {
            let (wire, session, _, _) = super::sign_in_command::approve_on_wire(
                wire,
                APPROVAL_POLL_INTERVAL,
                APPROVAL_MAX_WAIT,
                prompt_out,
            )?;
            Ok((wire, session))
        }
        Err(code) => Err(format!("the service refused local sign-in ({code:?})")),
    }
}

pub(super) fn connect_and_sign_in(
    target: &Target<'_>,
    login: &str,
    password: AskPassword<'_>,
) -> Result<(Wire, SessionHandle), String> {
    let refused = std::cell::RefCell::new(None);
    let mut wire = connect_verified(target.socket, target.instance_id, |stream: &UnixStream| {
        crate::human::verify_host(stream)
            .and_then(|host| host.may_receive_secrets())
            .inspect_err(|why| *refused.borrow_mut() = Some(why.clone()))
    })
    .map_err(|why| {
        refused
            .borrow_mut()
            .take()
            .unwrap_or_else(|| not_running(target.name, why))
    })?;
    let signed = match wire.call_answer::<AccountVerify>(
        &AccountVerifyParams {
            login: login.to_owned(),
            password: password()?,
        },
        None,
    )? {
        Ok(signed) => signed,
        Err(ErrorCode::RateLimited) => {
            return Err("too many failed sign-ins; try again later".into())
        }
        Err(ErrorCode::Unavailable) => {
            return Err("the service cannot sign anyone in now; try again".into())
        }
        Err(_) => return Err("invalid login or password".into()),
    };
    let session = SessionHandle::new(signed.session.expose().into()).expect("a live handle");
    Ok((wire, session))
}

/// Lists offices visible to the signed-in account.
pub(crate) fn list(
    target: &Target<'_>,
    login: Option<&str>,
    org_id: Option<&str>,
    json: bool,
    password: AskPassword<'_>,
    prompt_out: &mut dyn Write,
    out: &mut dyn Write,
) -> Result<(), String> {
    let (mut wire, session) = connect_cli_session(target, login, password, prompt_out)?;
    let handle = SessionHandle::new(session.expose().into()).expect("a live handle");
    let answer = wire.call_answer::<OfficeList>(
        &OfficeListParams {
            session: handle,
            org_id: org_id.map(str::to_owned),
        },
        None,
    );
    end_session(&mut wire, &session);
    let listed = match answer? {
        Ok(listed) => listed,
        Err(ErrorCode::NotFound) => return Err("organization not found".into()),
        Err(ErrorCode::NotAuthorized) => return Err("not authorized to list offices".into()),
        Err(code) => return Err(format!("the service refused office list ({code:?})")),
    };
    if json {
        let encoded = serde_json::to_string_pretty(&listed)
            .map_err(|_| "could not encode office list".to_owned())?;
        return say(out, encoded);
    }
    if listed.offices.is_empty() {
        return say(out, "No offices.");
    }
    for office in &listed.offices {
        say(out, format_office_line(office))?;
    }
    Ok(())
}

fn format_office_line(office: &OfficeInfo) -> String {
    let mut line = format!(
        "{}\torg={}\trole={:?}\tstatus={:?}\tname={}",
        office.id, office.organization_id, office.role, office.status, office.name
    );
    if let Some(reason) = &office.status_reason {
        line.push_str(&format!("\treason={reason:?}"));
    }
    if let Some(next) = &office.next_restart_at {
        line.push_str(&format!("\tnext_restart_at={next}"));
    }
    line
}

/// Requests a restart of `office_id` through `office.restart` on `host.sock`.
pub(crate) fn restart(
    target: &Target<'_>,
    login: Option<&str>,
    office_id: &str,
    json: bool,
    password: AskPassword<'_>,
    prompt_out: &mut dyn Write,
    out: &mut dyn Write,
) -> Result<(), String> {
    let operation_id = crate::ids::uuid_v7().map_err(|_| "no operation ID could be generated")?;
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
    match answer? {
        Ok(HostOpResult::Completed { .. }) => {
            if json {
                let encoded = serde_json::to_string(&serde_json::json!({
                    "operation_id": operation_id,
                    "office_id": office_id,
                    "outcome": "completed",
                }))
                .map_err(|_| "could not encode restart response".to_owned())?;
                say(out, encoded)
            } else {
                say(out, format!("Restart requested for office {office_id}."))
            }
        }
        Ok(HostOpResult::Rejected { refusal }) => {
            Err(format!("the host refused the restart: {}", refusal.message))
        }
        Ok(_) => Err("the service's answer is not one this command knows".into()),
        Err(ErrorCode::NotFound) => Err(format!("office {office_id} not found")),
        Err(ErrorCode::NotAuthorized) => Err(
            "only the instance administrator or organization owner may restart an office".into(),
        ),
        Err(ErrorCode::Conflict) => {
            Err("the office is not offline or unavailable; no restart needed".into())
        }
        Err(ErrorCode::NotReady) => Err("the office is still starting or being created".into()),
        Err(ErrorCode::Unavailable) => Err("a restart cannot recover this office right now".into()),
        Err(code) => Err(format!("the service refused the restart ({code:?})")),
    }
}

#[cfg(test)]
mod tests {
    use super::format_office_line;
    use org_proto::{OfficeInfo, OfficeRole, OfficeStatus, OfficeStatusReason};

    #[test]
    fn format_office_line_includes_status_reason_and_backoff_without_endpoint() {
        let office = OfficeInfo {
            id: "0193a5c0-0000-7000-8000-000000000002".into(),
            organization_id: "0193a5c0-0000-7000-8000-000000000001".into(),
            name: "HQ".into(),
            role: OfficeRole::Hq,
            host: "This Computer".into(),
            endpoint: Some("/private/should-never-appear.sock".into()),
            status: OfficeStatus::Offline,
            status_reason: Some(OfficeStatusReason::Crashed),
            next_restart_at: Some("2026-11-21T07:00:00Z".into()),
        };
        let line = format_office_line(&office);
        assert!(line.contains(&office.id), "{line}");
        assert!(line.contains("status=Offline"), "{line}");
        assert!(line.contains("reason=Crashed"), "{line}");
        assert!(
            line.contains("next_restart_at=2026-11-21T07:00:00Z"),
            "{line}"
        );
        assert!(!line.contains("should-never-appear"), "{line}");
    }

    #[test]
    fn help_includes_use_when_guidance_for_every_subcommand() {
        assert!(
            super::HELP.matches("Use when:").count() >= 13,
            "expected command + 12 subcommand Use when cues:\n{}",
            super::HELP
        );
    }
}
