//! What setup prints: the summary, the service hints and the one-time administrator link.

use std::io::Write;

use org_host_control::admin::AdminLinkResult;
use org_platform::service::{Lingering, ServiceError};

use org_host::instance::setup::Record;

use super::account::user_name;
use super::address::tls_notice;
use super::output::{fail, say};

pub(super) fn report_setup_summary(
    out: &mut dyn Write,
    record: &Record,
    tls_line: Option<String>,
) -> Result<(), String> {
    say(out, format!("Data: {}", record.input.root.display()))?;
    say(
        out,
        format!("Address: {}", record.input.listen.origin().as_str()),
    )?;
    if record.input.listen.scheme() == org_host_control::listen::ListenMode::Https {
        let line = tls_line.unwrap_or_else(|| tls_notice(record));
        say(out, format!("TLS: {line}"))?;
    }
    if let Some(federation) = &record.input.federation {
        if federation.advertise.is_empty() {
            say(out, format!("Federation: {}", federation.bind))?;
        } else {
            say(
                out,
                format!(
                    "Federation: {} (advertise: {})",
                    federation.bind,
                    federation.advertise.join(", ")
                ),
            )?;
        }
    }
    Ok(())
}

pub(super) fn report_foreground_service(
    out: &mut dyn Write,
    shown: &str,
    name: Option<&str>,
) -> Result<(), String> {
    let command = reset_hint(name);
    let link = if org_host::human::host_is_named_by_user_alone() {
        format!("run as root: {command} --user {}", user_name())
    } else {
        format!("run as the service user: {command}")
    };
    say(
        out,
        format!(
            "Service: none is registered (foreground mode). Instance \"{shown}\" runs \
             while agentic-org serve --setup <file> runs. For the administrator's \
             registration link, {link}"
        ),
    )
}

/// What setup and the service commands say when no service manager can register a service
/// here: the platform's reason, then the command that runs an instance without one
/// (SRV-34). Every other service error is shown as it is.
pub(crate) fn no_service_manager(error: ServiceError) -> String {
    match error {
        ServiceError::Unavailable(_) => format!(
            "{error}: write a setup file with \"service\": \"foreground\" and run \
             agentic-org serve --setup <file>; nothing was changed"
        ),
        error => fail("asking the service manager", error),
    }
}

/// What setup and `service status` say about a service that stops when its user logs out
/// (Linux host design section 3.4). The command is printed, never run.
pub(crate) fn lingering_report(lingering: &Lingering, user: &str) -> Vec<String> {
    match lingering {
        Lingering::NotApplicable | Lingering::On => Vec::new(),
        Lingering::Off { enable } => vec![
            format!(
                "The service runs only while {user} is logged in: it stops at logout and \
                 does not start when the machine starts."
            ),
            format!("To keep it running without a login session, run: {enable}"),
        ],
        Lingering::Unknown(why) => vec![format!(
            "Whether the service keeps running after {user} logs out could not be \
             determined ({why})."
        )],
    }
}

/// The service manager's name for an instance: its ID in lowercase (the label is
/// `com.ellosoft.agentic-org.instance.<id>`; instance IDs are canonical lowercase UUIDv7 text, so this is a no-op kept for older input).
pub(super) fn service_id(instance_id: &str) -> String {
    instance_id.to_ascii_lowercase()
}

pub(super) fn reset_hint(name: Option<&str>) -> String {
    match name {
        None | Some("default") => "agentic-org admin reset".into(),
        Some(name) => format!("agentic-org admin reset --name {name}"),
    }
}

/// What a command says when a service answers a link call with anything but this
/// instance's own registration link. Nothing of the answer is shown.
pub(crate) const NOT_THIS_INSTANCES_LINK: &str = "the service answered with something that is \
     not this instance's registration link, so nothing of its answer is shown; check that the \
     instance's own service is the one that runs";

/// `url` if it is exactly this instance's registration link: the configured `origin`, then
/// `/register#`, then a token in the product's own form (32 bytes as unpadded base64url).
/// On a system that cannot identify the program behind `host.sock`, the answer comes from
/// a peer the service user controls, and it is printed to a terminal that may be root's:
/// nothing else is ever printed from it (X5 review V4).
pub(crate) fn this_instances_link<'a>(
    url: &'a str,
    origin: &org_host_control::listen::ListenOrigin,
) -> Result<&'a str, String> {
    this_instances_link_at(url, origin, "/register#")
}

pub(crate) fn this_instances_reset_link<'a>(
    url: &'a str,
    origin: &org_host_control::listen::ListenOrigin,
) -> Result<&'a str, String> {
    this_instances_link_at(url, origin, "/set-password#")
}

fn this_instances_link_at<'a>(
    url: &'a str,
    origin: &org_host_control::listen::ListenOrigin,
    path_prefix: &str,
) -> Result<&'a str, String> {
    let token = url
        .strip_prefix(origin.as_str().trim_end_matches('/'))
        .and_then(|rest| rest.strip_prefix(path_prefix));
    match token {
        Some(token) if org_host::ports::links::Token::parse(token).is_some() => Ok(url),
        _ => Err(NOT_THIS_INSTANCES_LINK.into()),
    }
}

pub(super) fn show_link(
    out: &mut dyn Write,
    result: AdminLinkResult,
    name: Option<&str>,
    origin: &org_host_control::listen::ListenOrigin,
) -> Result<(), String> {
    match result {
        AdminLinkResult::Issued { url, expires_at_ms } => {
            let url = this_instances_link(url.expose(), origin)?;
            say(out, "")?;
            say(
                out,
                "Open this link in a browser to create the administrator account.",
            )?;
            say(
                out,
                format!(
                    "It works once and expires in 24 hours (at {}):",
                    local_time(expires_at_ms)
                ),
            )?;
            say(out, format!("  {url}"))?;
            say(
                out,
                format!("If it expires or is lost, run: {}", reset_hint(name)),
            )
        }
        AdminLinkResult::ResetPassword {
            login,
            url,
            expires_at_ms,
        } => {
            let url = this_instances_reset_link(url.expose(), origin)?;
            let safe_login = org_host::host_log::untrusted(&login);
            say(out, "")?;
            say(
                out,
                format!("Open this link in a browser to set a new password for {safe_login}."),
            )?;
            say(
                out,
                format!(
                    "It works once and expires in 24 hours (at {}):",
                    local_time(expires_at_ms)
                ),
            )?;
            say(out, format!("  {url}"))?;
            say(
                out,
                format!("If it expires or is lost, run: {}", reset_hint(name)),
            )
        }
        AdminLinkResult::AlreadyIssued => {
            say(out, format!("For a new link, run: {}", reset_hint(name)))
        }
        AdminLinkResult::AdministratorPresent => say(out, "It has an administrator."),
    }
}

/// `expires_at_ms` as local `YYYY-MM-DD HH:MM`, or UTC milliseconds if the conversion fails.
fn local_time(ms: i64) -> String {
    let secs = ms.div_euclid(1000) as libc::time_t;
    let Some(tm) = org_platform::posix::local_time(secs) else {
        return format!("{ms} ms after the Unix epoch, UTC");
    };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min
    )
}

/// What a repeated setup says once the instance has an administrator (spec §4a.2).
pub(super) fn running_with_administrator(shown: &str, origin: &str) -> String {
    format!("Instance {shown} is running. Open {origin} and sign in.")
}

/// What `admin reset` refuses with when an administrator was registered concurrently during the
/// initial-registration-link confirmation dialog.
pub(crate) fn administrator_present(name: Option<&str>, origin: &str) -> String {
    format!(
        "instance {} already has an administrator, so no registration link is issued: sign in at \
         {origin}, or run `agentic-org admin reset` again to issue a password-reset link",
        name.unwrap_or("default")
    )
}
