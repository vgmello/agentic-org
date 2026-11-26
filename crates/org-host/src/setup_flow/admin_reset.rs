//! `agentic-org admin reset`: asking the running instance for a new administrator link.

use std::io::Write;
use std::time::Duration;

use org_host_control::admin::{AdminLinkResult, AdminReset, AdminResetParams};

use crate::human::Wire;
use crate::instance::setup::Paths;

use super::account::{
    account_by_id, account_by_name, account_name, as_account, service_account, Account,
};
use super::output::{fail, say};
use super::report::{administrator_present, reset_hint, show_link};
use super::wire::{connect, connect_verified, registered_with_origin, within};

/// What `admin reset` answers when its output is not a terminal (spec §4a.4).
pub(crate) const NOT_A_TERMINAL: &str = "Run this in a terminal.";

/// `agentic-org admin reset`: a new registration link after presence at the host.
/// `terminal` says whether `out` is a terminal: the link is printed only there, so
/// otherwise this refuses before asking the host, and no link is issued or cancelled.
///
/// Run by root for the service user's instance (SIGN-33; ADR-0017 decisions 10 and 11): the
/// account is `user`, or the one `sudo` names (`SUDO_UID`). Root finds the instance as that
/// user ([`as_account`]), so nothing in the instance's folders is made or changed by root,
/// and then connects as root, which is what the service admits for this one call. Any other
/// user acts only for itself.
/// What `admin reset` answers on macOS when run over SSH (SIGN-30 / SIGN-34, spec §4a.2).
pub(crate) const MACOS_SSH_RESET_REFUSED: &str =
    "Resetting the administrator's password needs you at this Mac, to confirm it's you with Touch ID or the login password, and that can't be done over SSH. Run this command at the Mac itself or through Screen Sharing.";

/// Pure SIGN-34 check: on macOS (not named-by-user-alone), SSH environment variables
/// refuse `admin reset` before anything else (even before the terminal check or host call).
pub(crate) fn ssh_reset_refusal(
    named_by_user: bool,
    ssh_env_present: bool,
) -> Option<&'static str> {
    if !named_by_user && ssh_env_present {
        Some(MACOS_SSH_RESET_REFUSED)
    } else {
        None
    }
}

fn env_is_ssh() -> bool {
    ["SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY"]
        .iter()
        .any(|var| std::env::var_os(var).is_some_and(|v| !v.is_empty()))
}

pub fn admin_reset(
    name: Option<&str>,
    user: Option<&str>,
    terminal: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    let named_by_user = crate::human::host_is_named_by_user_alone();
    // SIGN-34: SSH refusal on macOS is shown before anything else.
    if let Some(refusal) = ssh_reset_refusal(named_by_user, env_is_ssh()) {
        return Err(refusal.into());
    }
    if !terminal {
        return Err(NOT_A_TERMINAL.into());
    }
    let acting_for = service_account(
        crate::home::euid(),
        named_by_user,
        user,
        std::env::var("SUDO_UID").ok().as_deref(),
        account_by_name,
        account_by_id,
    )?;
    // SIGN-33: where the system names a peer by its user alone (Linux), only root resets,
    // and anyone else is told so before the service is asked anything.
    if acting_for.is_none() && named_by_user {
        return Err(reset_needs_root(name));
    }
    match acting_for {
        Some(account) => reset_for(account, name, || crate::human::paths(name), out),
        None => {
            let (_record, id, socket, origin) =
                registered_with_origin(&crate::human::paths(name)?, name)?;
            let wire = connect(&socket, &id).map_err(|why| not_running(name, why))?;
            reset_over(wire, &origin, name, out)
        }
    }
}

/// How long root's reset waits for the instance's service, from the connect to its answer:
/// a service user's listener that never accepts, or never answers, does not hold root's
/// command (X5 review, fourth round, P2-8).
const RESET_WITHIN: Duration = Duration::from_secs(30);

/// Root's reset of the instance `name` of the service user `account`, whose instance
/// locations `paths` gives. Root finds the instance as that user ([`as_account`]) and connects
/// as root, within [`RESET_WITHIN`].
///
/// Every refusal is escaped before it is returned (`host_log::untrusted`): its text comes from
/// that user's folders (a registered data folder's path, a setup record) and from its service,
/// all of which that user controls, and it is printed on root's terminal, where a control
/// character could act (X5 review, second round).
pub(super) fn reset_for(
    account: Account,
    name: Option<&str>,
    paths: impl FnOnce() -> Result<Paths, String>,
    out: &mut dyn Write,
) -> Result<(), String> {
    let reset = || {
        let (_record, id, socket, origin) =
            as_account(account, || registered_with_origin(&paths()?, name))?;
        // The first line names the user whose instance this is, so that a reset never
        // lands on another user's instance unnoticed (SIGN-33).
        say(
            out,
            format!(
                "Instance {} of Linux user {}.",
                name.unwrap_or("default"),
                account_name(account.uid)
            ),
        )?;
        let result = within(RESET_WITHIN, move || {
            let wire = connect_verified(&socket, &id, |stream| {
                crate::human::verify_service_host(stream, account.uid)
            })?;
            ask_reset(wire)
        })
        .map_err(|why| not_running(name, why))?;
        answer_reset(result, &origin, name, out)
    };
    reset().map_err(|why| crate::host_log::untrusted(&why))
}

/// What `admin reset` says when it cannot reach the instance's service.
pub(super) fn not_running(name: Option<&str>, why: String) -> String {
    format!(
        "{why}; the service is not running: start it with agentic-org service start{}",
        name.map(|n| format!(" --name {n}")).unwrap_or_default()
    )
}

/// What `admin reset` answers on Linux to anyone but root (spec §4a.2, SIGN-33).
pub(crate) fn reset_needs_root(name: Option<&str>) -> String {
    format!(
        "Resetting the administrator's password on Linux needs root. Run it with sudo: sudo {}",
        reset_hint(name)
    )
}

/// The reset itself, over a connection to the instance's service: one call, and of its
/// answer only this instance's own link is ever shown.
pub(super) fn reset_over(
    wire: Wire,
    origin: &org_host_control::listen::ListenOrigin,
    name: Option<&str>,
    out: &mut dyn Write,
) -> Result<(), String> {
    answer_reset(ask_reset(wire)?, origin, name, out)
}

fn ask_reset(mut wire: Wire) -> Result<AdminLinkResult, String> {
    wire.call::<AdminReset>(&AdminResetParams::default(), None)
        .map_err(|e| fail("asking for a new registration link", e))
}

/// What the command says of the service's answer: this instance's own link, or, for an
/// instance that has an administrator, a refusal that says what to do instead (X5 review,
/// fourth round, P2-10). A forgotten administrator password cannot be reset yet: no link is
/// issued for it, and nothing says otherwise.
pub(super) fn answer_reset(
    result: AdminLinkResult,
    origin: &org_host_control::listen::ListenOrigin,
    name: Option<&str>,
    out: &mut dyn Write,
) -> Result<(), String> {
    if matches!(result, AdminLinkResult::AdministratorPresent) {
        return Err(administrator_present(name, origin.as_str()));
    }
    show_link(out, result, name, origin)
}

#[cfg(test)]
mod admin_reset_sign34_tests {
    use super::*;
    use org_host_control::listen::ListenOrigin;
    use org_host_control::OneTimeLinkUrl;

    #[test]
    fn sign34_refuses_macos_ssh_before_terminal_and_allows_linux_ssh() {
        let refused = ssh_reset_refusal(false, true).expect("macOS SSH must refuse");
        assert_eq!(refused, MACOS_SSH_RESET_REFUSED);
        assert!(refused.contains("needs you at this Mac"));
        assert!(refused.contains("Screen Sharing"));
        assert_eq!(ssh_reset_refusal(false, false), None);
        assert_eq!(ssh_reset_refusal(true, true), None);
    }

    #[test]
    fn show_link_formats_admin_reset_password_link() {
        let origin = ListenOrigin::parse("http://127.0.0.1:7420").unwrap();
        let token = "A".repeat(43);
        let url =
            OneTimeLinkUrl::new(format!("http://127.0.0.1:7420/set-password#{token}")).unwrap();
        let mut out = Vec::new();
        show_link(
            &mut out,
            AdminLinkResult::ResetPassword {
                login: "Sage".into(),
                url,
                expires_at_ms: 1_800_000_000_000,
            },
            None,
            &origin,
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("set a new password for Sage"), "{text}");
        assert!(text.contains("/set-password#"), "{text}");
    }
}
