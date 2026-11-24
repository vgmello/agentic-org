//! `agentic-org office list` and `agentic-org office restart` (implementation plan O1,
//! FC-019, FC-021): native session calls over `host.sock` (`office.list` and `office.restart`).
//!
//! Uses the interim terminal sign-in (`account.verify`) until browser-approved CLI sign-in
//! lands in A5: before asking for a password it verifies the host binary (`verify_host` +
//! `may_receive_secrets`), then always ends the session before returning.
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::Path;

use org_host_control::ops::{
    AccountVerify, AccountVerifyParams, OfficeList, OfficeListParams, OfficeRestart,
    OfficeRestartParams, SessionEnd, SessionParams,
};
use org_host_control::{HostOpResult, Password, SessionHandle};
use org_proto::{ErrorCode, OfficeInfo};

use super::{connect_verified, not_running, registered, say};

/// Help text for `agentic-org office`.
pub const HELP: &str = "agentic-org office <subcommand>\n\n\
Subcommands:\n  \
list --login <login> [--org <org_id>] [--name <instance>] [--json]\n      \
List offices visible to the signed-in account with their live readiness status\n  \
restart <office_id> --login <login> [--name <instance>] [--json]\n      \
Restart an offline or unavailable office (requires administrator or organization owner)\n\n\
Asks for the account password at the terminal. On systems that cannot identify the host's \
program (Linux, containers), terminal password sign-in is refused until browser-approved \
CLI sign-in is available.";

/// What office commands need from the person: their password, asked once after the host is verified.
pub(crate) type AskPassword<'a> = &'a mut dyn FnMut() -> Result<Password, String>;

/// The instance an office command acts on.
pub(crate) struct Target<'a> {
    pub(crate) socket: &'a Path,
    pub(crate) instance_id: &'a str,
    pub(crate) name: Option<&'a str>,
}

/// `agentic-org office list` for registered instance `name`.
pub fn office_list(
    name: Option<&str>,
    login: &str,
    org_id: Option<&str>,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    let (_, id, socket) = registered(&crate::human::paths(name)?, name)?;
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
        out,
    )
}

/// `agentic-org office restart` for registered instance `name`.
pub fn office_restart(
    name: Option<&str>,
    login: &str,
    office_id: &str,
    json: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    let (_, id, socket) = registered(&crate::human::paths(name)?, name)?;
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
        out,
    )
}

pub(super) fn connect_and_sign_in(
    target: &Target<'_>,
    login: &str,
    password: AskPassword<'_>,
) -> Result<(crate::human::Wire, SessionHandle), String> {
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
    login: &str,
    org_id: Option<&str>,
    json: bool,
    password: AskPassword<'_>,
    out: &mut dyn Write,
) -> Result<(), String> {
    let (mut wire, session) = connect_and_sign_in(target, login, password)?;
    let handle = || SessionHandle::new(session.expose().into()).expect("a live handle");
    let answer = wire.call_answer::<OfficeList>(
        &OfficeListParams {
            session: handle(),
            org_id: org_id.map(str::to_owned),
        },
        None,
    );
    let _ = wire.call::<SessionEnd>(&SessionParams { session: handle() }, None);
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
    login: &str,
    office_id: &str,
    json: bool,
    password: AskPassword<'_>,
    out: &mut dyn Write,
) -> Result<(), String> {
    let operation_id = crate::ids::uuid_v7().map_err(|_| "no operation ID could be generated")?;
    let (mut wire, session) = connect_and_sign_in(target, login, password)?;
    let handle = || SessionHandle::new(session.expose().into()).expect("a live handle");
    let answer = wire.call_answer::<OfficeRestart>(
        &OfficeRestartParams {
            session: handle(),
            operation_id: operation_id.clone(),
            office_id: office_id.to_owned(),
        },
        None,
    );
    let _ = wire.call::<SessionEnd>(&SessionParams { session: handle() }, None);
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
            host: "This Mac".into(),
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
}
