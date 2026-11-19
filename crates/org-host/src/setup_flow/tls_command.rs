//! `agentic-org admin tls set --cert <file> --key <file> --login <login> [--name <instance>]`:
//! replaces the TLS pair of an `https` instance's browser listener with a supplied one, by
//! `tls.set` on `host.sock` (X-02 §6.2), and prints only the new certificate's SHA-256
//! fingerprint.
//!
//! The call is admitted as the contract says, and no wider: a `host.sock` peer that passes the
//! administrator calls' admission (the signed `agentic-org` on a Mac; the service's own user
//! where a peer is named by its user alone), with the administrator's session. The session
//! comes from the administrator's password, typed at the terminal (native `account.verify`,
//! the interim sign-in `recovery` uses until command-line sign-in is approved in the browser,
//! FC-173), so the command sends it only to a host whose program it could identify
//! ([`crate::human::verify_host`]): where a peer is known by its user alone (GNU/Linux,
//! containers) it refuses before a password is asked or anything is sent. Root has no route: the call does
//! not admit it.
//!
//! The files are read as setup reads them (`web_tls::read_pem_file`: the last component not
//! followed, a regular file of at most 64 KiB) and checked in `tls.set`'s order before the
//! host is asked, so a refusal names the files; the host checks again. Nothing of the key is
//! printed, logged or kept by the command.
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use org_host_control::listen::{ListenOrigin, OriginScheme};
use org_host_control::ops::{
    AccountVerify, AccountVerifyParams, SessionEnd, SessionParams, TlsSet, TlsSetParams,
};
use org_host_control::{HostOpResult, Password, SessionHandle, TlsPrivateKey};
use org_proto::ErrorCode;

use super::{connect_verified, not_running, registered, say};

/// The command's help.
pub const HELP: &str = "agentic-org admin tls set --cert <file> --key <file> --login <login> \
[--name <instance>]\nReplaces the TLS certificate and key of an instance whose browser listener \
is https, without restarting it, and prints the new certificate's SHA-256 fingerprint. The \
files are PEM: the certificate chain, leaf first, and an unencrypted private key; they must \
name the instance's host and be valid now. Asks for the administrator's password at the \
terminal. Run it on the server, as the user the service runs as. On a system that cannot \
identify the service's program (Linux, a container) no password is sent from the command \
line, so this is refused there until command-line sign-in is approved in the browser.";

/// What [`set`] needs from the person: the administrator's password, asked once, after the
/// files were checked and the host was identified.
pub(crate) type AskPassword<'a> = &'a mut dyn FnMut() -> Result<Password, String>;

/// `agentic-org admin tls set` for the registered instance `name`.
pub fn admin_tls_set(
    name: Option<&str>,
    cert: &Path,
    key: &Path,
    login: &str,
    out: &mut dyn Write,
) -> Result<(), String> {
    let (record, id, socket) = registered(&crate::human::paths(name)?, name)?;
    let origin = record.input.listen.origin().clone();
    set(
        &Target {
            socket: &socket,
            instance_id: &id,
            origin: &origin,
            name,
        },
        &absolute(cert)?,
        &absolute(key)?,
        login,
        &mut crate::human::password,
        out,
    )
}

/// The instance [`set`] acts on.
pub(crate) struct Target<'a> {
    pub(crate) socket: &'a Path,
    pub(crate) instance_id: &'a str,
    pub(crate) origin: &'a ListenOrigin,
    pub(crate) name: Option<&'a str>,
}

/// `path`, made absolute against the current directory without resolving anything in it.
fn absolute(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        return Ok(path.to_owned());
    }
    std::env::current_dir()
        .map(|here| here.join(path))
        .map_err(|_| "the current directory is unavailable; give the files' absolute paths".into())
}

/// Reads and checks the pair, identifies the host, signs the administrator in, asks
/// `tls.set`, ends the session, and prints the new fingerprint.
pub(crate) fn set(
    target: &Target<'_>,
    cert: &Path,
    key: &Path,
    login: &str,
    password: AskPassword<'_>,
    out: &mut dyn Write,
) -> Result<(), String> {
    if target.origin.scheme() != OriginScheme::Https {
        return Err(format!(
            "instance {} listens at {}, which is not https: it serves no TLS pair",
            shown(target.name),
            target.origin.as_str()
        ));
    }
    let now = crate::web_tls::now().ok_or("the clock is unavailable")?;
    let checked = crate::web_tls::read_supplied(cert, key, target.origin, now)?;
    // Made before a session exists, so that nothing between the sign-in and its end can
    // return early.
    let key =
        TlsPrivateKey::new(checked.key().to_owned()).map_err(|_| "the TLS key cannot be sent")?;
    let operation_id = crate::ids::generate().map_err(|_| "no operation ID could be generated")?;
    // Before a password is asked for: it is sent only to a host this command identified. A
    // host it may not send one to is refused in its own words; only a failure to reach the
    // service says that the service is not running.
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
        Err(_) => return Err("the login or password is not the administrator's".into()),
    };
    let session = || SessionHandle::new(signed.session.expose().into()).expect("a live handle");
    let answer = if signed.admin {
        wire.call_answer::<TlsSet>(
            &TlsSetParams {
                session: session(),
                operation_id,
                cert: checked.chain().to_owned(),
                key,
            },
            None,
        )
    } else {
        Ok(Err(ErrorCode::NotAuthorized))
    };
    // The session ends whatever the answer.
    let _ = wire.call::<SessionEnd>(&SessionParams { session: session() }, None);
    match answer? {
        Ok(HostOpResult::Completed { .. }) => say(
            out,
            format!(
                "The TLS certificate of instance {} ({}) was replaced: SHA-256 fingerprint {}.",
                shown(target.name),
                target.origin.as_str(),
                checked.fingerprint()
            ),
        ),
        Ok(HostOpResult::Rejected { refusal }) => Err(format!(
            "the host refused the pair: {}",
            crate::web_tls::refusal_text(&refusal)
        )),
        Ok(_) => Err("the service's answer is not one this command knows".into()),
        Err(ErrorCode::NotAuthorized) if !signed.admin => {
            Err("only the instance's administrator replaces its TLS pair".into())
        }
        Err(ErrorCode::NotAuthorized) => Err("the service did not admit this program for \
             administrator calls: run the signed agentic-org on the server, as the user the \
             service runs as"
            .into()),
        Err(ErrorCode::Unavailable) => Err(
            "the service cannot replace its TLS pair now (it is not an https instance, \
                 or its administrator calls are not ready yet)"
                .into(),
        ),
        Err(code) => Err(format!("the service refused the replacement ({code:?})")),
    }
}

fn shown(name: Option<&str>) -> &str {
    name.unwrap_or("default")
}
