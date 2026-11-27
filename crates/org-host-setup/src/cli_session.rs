//! Connecting a CLI command to its instance: the local peer check, signing in with a password
//! or browser approval, and ending the session. Every failure is a [`HostCliError`]: a
//! sign-in the person cannot make (a wrong password, a declined or expired approval, a
//! password this system may not send) is `Auth`, a cancelled password entry `Cancelled`.

use std::cell::RefCell;
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

use org_host::human::Wire;

use super::admin_reset::not_running;
use super::wire::connect_verified;
use org_host::cli_error::{refused_by, sign_in_refused, HostCliError};

/// What office commands need from the person: their password, asked once after the host is verified.
pub(crate) type AskPassword<'a> = &'a mut dyn FnMut() -> Result<Password, String>;

/// The instance an office command acts on.
pub struct Target<'a> {
    pub socket: &'a Path,
    pub instance_id: &'a str,
    pub name: Option<&'a str>,
}

pub(super) const APPROVAL_POLL_INTERVAL: Duration = Duration::from_millis(250);

pub(super) const APPROVAL_MAX_WAIT: Duration = Duration::from_secs(10 * 60);

/// Connects to the instance's `host.sock` after checking who serves it. A host that fails
/// the check is refused in the check's own words; only a failure to reach the service says
/// that it is not running.
pub(super) fn connect_local_peer(target: &Target<'_>) -> Result<Wire, HostCliError> {
    let refused = RefCell::new(None);
    connect_verified(target.socket, target.instance_id, |stream: &UnixStream| {
        org_host::human::verify_host(stream)
            .map(|_| ())
            .inspect_err(|why| *refused.borrow_mut() = Some(why.clone()))
    })
    .map_err(|why| {
        HostCliError::runtime(
            refused
                .take()
                .unwrap_or_else(|| not_running(target.name, why)),
        )
    })
}

/// [`connect_local_peer`] for a command that sends a password: a host this system can
/// identify only by its user is sent none, and that refusal is `Auth` (sign in through the
/// browser instead).
pub(super) fn connect_for_secrets(
    socket: &Path,
    instance_id: &str,
    name: Option<&str>,
) -> Result<Wire, HostCliError> {
    let refused = RefCell::new(None);
    connect_verified(socket, instance_id, |stream: &UnixStream| {
        let host = org_host::human::verify_host(stream)
            .inspect_err(|why| *refused.borrow_mut() = Some(HostCliError::runtime(why.clone())))?;
        host.may_receive_secrets()
            .inspect_err(|why| *refused.borrow_mut() = Some(HostCliError::auth(why.clone())))
    })
    .map_err(|why| {
        refused
            .take()
            .unwrap_or_else(|| HostCliError::runtime(not_running(name, why)))
    })
}

pub(super) fn end_session(wire: &mut Wire, session: &SessionHandle) {
    let handle = SessionHandle::new(session.expose().into()).expect("a live handle");
    let _ = wire.call::<SessionEnd>(&SessionParams { session: handle }, None);
}

/// Establishes an authenticated `host.sock` session for a CLI command:
/// - `--login <user>` -> verified-host password sign-in (`account.verify`).
/// - no `--login` -> `session.local` (instant on local authless HTTP), falling back to
///   browser-approved CLI sign-in (`cli.approval.*`) when authentication is enabled; the
///   approval link goes to `prompt_out`.
pub(super) fn connect_cli_session(
    target: &Target<'_>,
    login: Option<&str>,
    password: AskPassword<'_>,
    prompt_out: &mut dyn Write,
) -> Result<(Wire, SessionHandle), HostCliError> {
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
        Err(code) => Err(refused_by("local sign-in", code)),
    }
}

/// Password sign-in (`account.verify`) on a host this command could identify.
pub(super) fn connect_and_sign_in(
    target: &Target<'_>,
    login: &str,
    password: AskPassword<'_>,
) -> Result<(Wire, SessionHandle), HostCliError> {
    let mut wire = connect_for_secrets(target.socket, target.instance_id, target.name)?;
    let password = password().map_err(HostCliError::password)?;
    let signed = match wire.call_answer::<AccountVerify>(
        &AccountVerifyParams {
            login: login.to_owned(),
            password,
        },
        None,
    )? {
        Ok(signed) => signed,
        Err(code) => return Err(sign_in_refused(code, "invalid login or password")),
    };
    let session = SessionHandle::new(signed.session.expose().into()).expect("a live handle");
    Ok((wire, session))
}

/// Runs the browser-approved sign-in flow over an already-verified `host.sock` [`Wire`],
/// writing the one-time approval link to `out` (the prompt stream).
pub(super) fn approve_on_wire(
    mut wire: Wire,
    poll_interval: Duration,
    max_wait: Duration,
    out: &mut dyn Write,
) -> Result<(Wire, SessionHandle, String, Vec<LinkedOrg>), HostCliError> {
    let created = match wire.call_answer::<CliApprovalCreate>(&CliApprovalCreateParams {}, None)? {
        Ok(created) => created,
        Err(ErrorCode::NotAuthorized) => {
            return Err(HostCliError::refused(
                ErrorCode::NotAuthorized,
                "the host refused terminal sign-in approval",
            ))
        }
        Err(code) => return Err(refused_by("terminal sign-in", code)),
    };
    writeln!(
        out,
        "Open this approval link in a signed-in browser (expires in 10 minutes):\n  {}",
        created.url.expose()
    )
    .map_err(|e| HostCliError::runtime(format!("could not show the approval link: {e}")))?;
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
                    return Err(HostCliError::auth(
                        "approval link expired before it was approved; run the command again",
                    ));
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
                return Err(HostCliError::auth(
                    "terminal sign-in was declined in the browser",
                ))
            }
            Ok(CliApprovalPollResult::Expired) => {
                return Err(HostCliError::auth(
                    "approval link expired; run the command again",
                ))
            }
            Err(ErrorCode::NotAuthorized) => {
                return Err(HostCliError::refused(
                    ErrorCode::NotAuthorized,
                    "terminal sign-in approval is not authorized",
                ))
            }
            Err(code) => return Err(refused_by("the terminal sign-in check", code)),
        }
    }
}
