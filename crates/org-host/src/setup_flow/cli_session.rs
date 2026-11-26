//! Connecting a CLI command to its instance: the local peer check, signing in with a password
//! or browser approval, and ending the session.

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

use org_host_control::ops::{
    AccountVerify, AccountVerifyParams, CliApprovalCreate, CliApprovalCreateParams,
    CliApprovalPoll, CliApprovalPollParams, CliApprovalPollResult, LinkedOrg, SessionEnd,
    SessionLocal, SessionLocalParams, SessionParams,
};
use org_host_control::{Password, SessionHandle};
use org_proto::ErrorCode;

use crate::human::Wire;

use super::admin_reset::not_running;
use super::output::say;
use super::wire::connect_verified;

/// What office commands need from the person: their password, asked once after the host is verified.
pub(crate) type AskPassword<'a> = &'a mut dyn FnMut() -> Result<Password, String>;

/// The instance an office command acts on.
pub(crate) struct Target<'a> {
    pub(crate) socket: &'a Path,
    pub(crate) instance_id: &'a str,
    pub(crate) name: Option<&'a str>,
}

pub(super) const APPROVAL_POLL_INTERVAL: Duration = Duration::from_millis(250);

pub(super) const APPROVAL_MAX_WAIT: Duration = Duration::from_secs(10 * 60);

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
            let (wire, session, _, _) =
                approve_on_wire(wire, APPROVAL_POLL_INTERVAL, APPROVAL_MAX_WAIT, prompt_out)?;
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

/// Runs the browser-approved sign-in flow over an already-verified `host.sock` [`Wire`].
pub(super) fn approve_on_wire(
    mut wire: Wire,
    poll_interval: Duration,
    max_wait: Duration,
    out: &mut dyn Write,
) -> Result<(Wire, SessionHandle, String, Vec<LinkedOrg>), String> {
    let created = match wire.call_answer::<CliApprovalCreate>(&CliApprovalCreateParams {}, None)? {
        Ok(created) => created,
        Err(ErrorCode::NotAuthorized) => {
            return Err("the host refused terminal sign-in approval".into())
        }
        Err(code) => return Err(format!("could not start terminal sign-in ({code:?})")),
    };
    say(
        out,
        format!(
            "Open this approval link in a signed-in browser (expires in 10 minutes):\n  {}",
            created.url.expose()
        ),
    )?;
    let _ = out.flush();

    let deadline = Instant::now() + max_wait;
    loop {
        match wire.call_answer::<CliApprovalPoll>(
            &CliApprovalPollParams {
                approval_id: created.approval_id.clone(),
            },
            None,
        )? {
            Ok(CliApprovalPollResult::Pending) => {
                if Instant::now() >= deadline {
                    return Err(
                        "approval link expired before it was approved; run the command again"
                            .into(),
                    );
                }
                std::thread::sleep(poll_interval);
            }
            Ok(CliApprovalPollResult::Approved {
                session,
                account,
                orgs,
                ..
            }) => return Ok((wire, session, account, orgs)),
            Ok(CliApprovalPollResult::Declined) => {
                return Err("terminal sign-in was declined in the browser".into())
            }
            Ok(CliApprovalPollResult::Expired) => {
                return Err("approval link expired; run the command again".into())
            }
            Err(ErrorCode::NotAuthorized) => {
                return Err("terminal sign-in approval is not authorized".into())
            }
            Err(code) => return Err(format!("terminal sign-in poll failed ({code:?})")),
        }
    }
}
