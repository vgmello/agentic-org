//! Native, terminal-only composition. No secret argv/config/stdout/JSON mode.
//!
//! What it says goes to the `out` it is given (the command's standard output): a write
//! there that fails because the reader went away is [`HostCliError::output_closed`], never a
//! panic. The recovery code itself is written only to `/dev/tty`. Invalid arguments are
//! `Usage` errors whose help is the command's help.
use super::{session, Wire};
use crate::cli_error::{wire_name, HostCliError};
use crate::host_log::untrusted;
use org_host_control::{ops, recovery as dto, SessionHandle};
use org_proto::{Hello, HelloParams, HelloResult};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{IsTerminal, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::OpenOptionsExt, net::UnixStream},
    },
    path::{Path, PathBuf},
};
const HELP: &str = "agentic-org recovery <status|release> [--name NAME] [--org ORGANIZATION_NAME | --org-id ID] [--login LOGIN] [--redeliver] [--package NEW_FILE]\n\
Use when: inspecting recovery code delivery status (`status`) or releasing and exporting an organization's encrypted public recovery package on this Mac's foreground terminal (`release`).\n\
Or select an explicit --host-socket PATH --instance-id ID. Collection requires this Mac's foreground terminal and a fresh OS confirmation. REC-1 HQ-loss recovery is unsupported.";
fn error<T>(_: T) -> String {
    "recovery is unresolved; inspect persisted status before explicit redelivery; no retention acknowledgement was inferred".into()
}
/// Invalid recovery arguments: `Usage`, with the command's help.
fn usage() -> HostCliError {
    HostCliError::usage("the recovery arguments are not valid").with_help(HELP)
}
/// `ms` (Unix milliseconds) as RFC 3339 UTC, or `never`.
fn when(ms: Option<i64>) -> String {
    match ms {
        None => "never".into(),
        Some(ms) => u64::try_from(ms)
            .ok()
            .and_then(org_proto::format_rfc3339_ms)
            .unwrap_or_else(|| format!("{ms} ms from the Unix epoch")),
    }
}
pub(super) fn interactive() -> bool {
    std::io::stdin().is_terminal()
        && std::io::stdout().is_terminal()
        && crate::recovery::presence::current()
}
fn tty() -> Result<File, String> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open("/dev/tty")
        .map_err(error)
}
fn prompt(label: &str, max: usize) -> Result<String, String> {
    let mut tty = tty()?;
    tty.write_all(label.as_bytes())
        .and_then(|_| tty.flush())
        .map_err(error)?;
    let mut bytes = Vec::new();
    loop {
        let mut b = [0];
        tty.read_exact(&mut b).map_err(error)?;
        if b[0] == b'\n' {
            break;
        }
        if bytes.len() >= max {
            return Err(error(()));
        }
        bytes.push(b[0]);
    }
    String::from_utf8(bytes).map_err(error)
}
/// The delivery status as lines: the state in its wire spelling, times as RFC 3339 UTC or
/// `never`, an absent account as `none`; the host's own words escaped.
fn shown(status: &dto::StatusResult) -> Vec<String> {
    let mut lines = vec![format!(
        "Recovery: {}; delivery ordinal {}; first shown {}; last redelivered {}; redeliveries {}; acknowledged {} by {}.",
        wire_name(&status.state),
        untrusted(&status.delivery_ordinal),
        when(status.first_shown_ms),
        when(status.last_redelivered_ms),
        untrusted(&status.redelivery_count),
        when(status.acknowledged_ms),
        status.acknowledged_by.as_deref().map_or_else(|| "none".into(), untrusted),
    )];
    for event in &status.audit {
        lines.push(format!(
            "Delivery {}: {} by {} at {}",
            untrusted(&event.ordinal),
            untrusted(&event.action),
            untrusted(&event.account),
            when(Some(event.at_ms))
        ));
    }
    lines.push(
        "REC-1 HQ-loss recovery is unsupported. Delivery/export does not prove off-HQ retention."
            .into(),
    );
    lines
}
fn show(status: &dto::StatusResult, out: &mut dyn Write) -> Result<(), HostCliError> {
    for line in shown(status) {
        writeln!(out, "{line}").map_err(HostCliError::output)?;
    }
    Ok(())
}
pub(super) fn status(
    wire: &mut Wire,
    handle: &SessionHandle,
    org: &str,
    out: &mut dyn Write,
) -> Result<dto::StatusResult, HostCliError> {
    let mut after = None;
    loop {
        let result = wire
            .call::<dto::Status>(
                &dto::StatusParams {
                    session: session(handle),
                    org_id: org.into(),
                    after_ordinal: after.clone(),
                },
                None,
            )
            .map_err(error)?;
        show(&result, out)?;
        if result.next_after_ordinal.is_none() {
            return Ok(result);
        }
        let next = result.next_after_ordinal.clone();
        if next == after {
            return Err(error(()).into());
        }
        after = next;
    }
}
/// Export through an already-open parent descriptor. No truncation, overwrite,
/// symlink-following final component or organization-relative implicit path.
fn export(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if !path.is_absolute() || bytes.len() > 4475 {
        return Err("package destination must be an explicit absolute new file".into());
    }
    let name = path.file_name().ok_or_else(|| error(()))?;
    let name = std::ffi::CString::new(name.as_bytes()).map_err(error)?;
    let parent = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path.parent().ok_or_else(|| error(()))?)
        .map_err(error)?;
    // SAFETY: held directory, valid C filename, fixed exclusive-create flags/mode.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(
            "package export refused; destination must be a new local file; no ACK was sent".into(),
        );
    }
    // SAFETY: openat returned a new owned descriptor exactly once.
    let mut file = unsafe { File::from_raw_fd(fd) };
    file.write_all(bytes)
        .and_then(|_| org_platform::fs::full_sync(&file))
        .map_err(error)?;
    parent.sync_all().map_err(error)
}
fn package_bytes(p: dto::PackageResult, instance: &str, org: &str) -> Result<Vec<u8>, String> {
    use sha2::{Digest, Sha256};
    let public = org_office_launch::recovery::PublicRecord {
        version: p.version,
        instance_id: p.instance_id,
        office_id: p.office_id,
        organization_id: p.organization_id,
        operation_id: p.operation_id,
        escrow_version: p.escrow_version,
        public_key: p.public_key,
        package_sha256: p.package_sha256,
        package_hex: p.package_hex,
    };
    let bytes = public.package().map_err(error)?;
    if public.instance_id != instance
        || public.organization_id != org
        || public.package_sha256 != org_office_launch::recovery::hex(&Sha256::digest(&bytes))
    {
        return Err(error(()));
    }
    Ok(bytes)
}
/// Setup and standalone collection use this same connection/session throughout.
/// A wrong group, EOF, export failure or decline never sends ACK.
pub(super) fn collect(
    wire: &mut Wire,
    handle: &SessionHandle,
    instance: &str,
    org: &str,
    redeliver: bool,
    destination: Option<PathBuf>,
    out: &mut dyn Write,
) -> Result<(), HostCliError> {
    if !interactive() {
        return Err(HostCliError::usage("recovery releaseion requires the local foreground console terminal; no credentials or code collected"));
    }
    let observed = status(wire, handle, org, out)?;
    if observed.state == dto::DeliveryState::Acknowledged {
        return Ok(());
    }
    if observed.state == dto::DeliveryState::AckPending {
        let _: org_proto::Empty = wire
            .call::<ops::RecoveryAcknowledge>(
                &ops::RecoveryAcknowledgeParams {
                    session: session(handle),
                    org_id: org.into(),
                },
                None,
            )
            .map_err(error)?;
        status(wire, handle, org, out)?;
        return Ok(());
    }
    if observed.state == dto::DeliveryState::Delivered && !redeliver {
        return Err(HostCliError::refused(None, "already shown; inspect the recorded dates and use --redeliver for a fresh authenticated showing of the same code"));
    }
    let destination = match destination {
        Some(p) => p,
        None => PathBuf::from(prompt(
            "Absolute NEW file for encrypted public recovery package: ",
            4096,
        )?),
    };
    if !destination.is_absolute() {
        return Err(HostCliError::usage("package destination must be absolute"));
    }
    if !prompt("Press Enter to show the code and record BOTH code and package as stored off this Mac; any text defers: ",256)?.is_empty(){return Ok(());}
    let result = wire
        .call::<ops::RecoveryRelease>(
            &ops::RecoveryReleaseParams {
                session: session(handle),
                org_id: org.into(),
                redeliver,
            },
            None,
        )
        .map_err(error)?;
    if !interactive() {
        return Err(error(()).into());
    }
    let mut terminal = tty()?;
    writeln!(terminal, "Recovery code: {}", result.code.expose())
        .and_then(|_| terminal.flush())
        .map_err(error)?;
    let package = wire
        .call::<dto::Package>(
            &dto::PackageParams {
                session: session(handle),
                org_id: org.into(),
            },
            None,
        )
        .map_err(error)?;
    export(&destination, &package_bytes(package, instance, org)?)?;
    writeln!(terminal,"Encrypted public package exported. Copy BOTH the code and this package off HQ. Export alone is not a backup. REC-1 HQ-loss recovery is unsupported.").map_err(error)?;
    let challenge =
        super::terminal::secret(b"Final code group (hidden): ").map_err(HostCliError::password)?;
    if challenge.expose().as_bytes()
        != result
            .code
            .expose()
            .as_bytes()
            .get(45..49)
            .ok_or_else(|| error(()))?
    {
        return Err(HostCliError::refused(
            None,
            "code group did not match; remains Shown; no ACK was sent",
        ));
    }
    drop(challenge);
    if prompt(
        "Have BOTH code AND package been stored off HQ? Type yes to acknowledge: ",
        32,
    )? != "yes"
    {
        return Ok(());
    }
    if !interactive() {
        return Err(error(()).into());
    }
    let _: org_proto::Empty = wire
        .call::<ops::RecoveryAcknowledge>(
            &ops::RecoveryAcknowledgeParams {
                session: session(handle),
                org_id: org.into(),
            },
            None,
        )
        .map_err(error)?;
    status(wire, handle, org, out)?;
    Ok(())
}
fn recovery_options<'a>(
    command: &str,
    arguments: &'a [String],
) -> Result<(BTreeMap<&'a str, &'a str>, bool), HostCliError> {
    let mut options = BTreeMap::new();
    let mut redeliver = false;
    let mut args = arguments.iter();
    while let Some(flag) = args.next() {
        if flag == "--redeliver" {
            if redeliver || command != "release" {
                return Err(usage());
            }
            redeliver = true;
            continue;
        }
        if !matches!(
            flag.as_str(),
            "--name"
                | "--org"
                | "--org-id"
                | "--login"
                | "--host-socket"
                | "--instance-id"
                | "--package"
        ) || options
            .insert(flag.as_str(), args.next().ok_or_else(usage)?.as_str())
            .is_some()
        {
            return Err(usage());
        }
    }
    if command == "status" && options.contains_key("--package") {
        return Err(usage());
    }
    if options.contains_key("--org") && options.contains_key("--org-id") {
        return Err(usage());
    }
    Ok((options, redeliver))
}

struct RecoveryTarget {
    instance: String,
    socket: PathBuf,
    default_org: Option<String>,
    default_login: Option<String>,
}
fn recovery_target(options: &BTreeMap<&str, &str>) -> Result<RecoveryTarget, HostCliError> {
    let (instance, socket, default_org, default_login) =
        match (options.get("--host-socket"), options.get("--instance-id")) {
            (Some(socket), Some(instance)) if !options.contains_key("--name") => {
                ((*instance).to_owned(), PathBuf::from(socket), None, None)
            }
            (None, None) => {
                let name = options.get("--name").copied();
                let paths = super::paths(name)?;
                let record =
                    crate::instance::setup::Journal::snapshot(&paths.base, name).map_err(error)?;
                let registry =
                    crate::instance::registry::Registry::open(&paths.locator, &paths.user_temp)
                        .map_err(error)?;
                let (instance, temp) = registry.locate(name, &record.input.root).map_err(error)?;
                if record.instance_id.as_deref() != Some(&instance) {
                    return Err(error(()).into());
                }
                (
                    instance,
                    temp.join("host.sock"),
                    // Setup creates no organization and no account (A2).
                    None,
                    None,
                )
            }
            _ => return Err(usage()),
        };
    // Every target (explicit or setup-located) must name an absolute socket and a canonical
    // UUIDv7 instance id.
    if !socket.is_absolute() || !org_proto::is_uuid_v7(&instance) {
        return Err(usage());
    }
    Ok(RecoveryTarget {
        instance,
        socket,
        default_org,
        default_login,
    })
}

/// The id of the organization named `name` among those the signed-in account is linked
/// to. Names are unique on an instance whatever their letter case, so at most one
/// matches. An unknown name is refused with the names the account can see.
fn resolve_org(orgs: &[ops::LinkedOrg], name: &str) -> Result<String, HostCliError> {
    // The instance's own rule for "the same name" (`OrganizationName::key`), not a copy
    // of it. What is not an organization name has no key and matches nothing.
    let key = |name: &str| {
        org_proto::OrganizationName::parse(name)
            .ok()
            .map(|name| name.key().to_owned())
    };
    let wanted = key(name);
    let mut found = orgs
        .iter()
        .filter(|o| wanted.is_some() && key(&o.name) == wanted);
    if let (Some(org), None) = (found.next(), found.next()) {
        return Ok(org.org_id.clone());
    }
    // Quoted, with anything a terminal would interpret escaped.
    let quoted = |name: &str| format!("\"{}\"", untrusted(name));
    let visible: Vec<String> = orgs
        .iter()
        .filter(|o| !o.name.is_empty())
        .map(|o| quoted(&o.name))
        .collect();
    Err(HostCliError::refused(
        None,
        format!(
            "no organization named {} for this account; it can see: {}",
            quoted(name),
            if visible.is_empty() {
                "none".into()
            } else {
                visible.join(", ")
            }
        ),
    ))
}

fn is_recovery_help_request(arguments: &[String]) -> bool {
    let is_help_flag = |flag: &str| flag == "--help" || flag == "-h";
    match arguments {
        [] => true,
        [flag] => is_help_flag(flag),
        [command, flag] => matches!(command.as_str(), "status" | "release") && is_help_flag(flag),
        _ => false,
    }
}

/// `agentic-org recovery <status|release> ...`, saying what it does to `out`.
pub fn recovery(arguments: &[String], out: &mut dyn Write) -> Result<(), HostCliError> {
    if is_recovery_help_request(arguments) {
        return writeln!(out, "{HELP}").map_err(HostCliError::output);
    }
    let command = arguments[0].as_str();
    if !matches!(command, "status" | "release") {
        return Err(usage());
    }
    let (options, redeliver) = recovery_options(command, &arguments[1..])?;
    // No password collection before terminal/local-console preflight, even for
    // status sign-in. Native host status itself remains nonsecret/no-dialog.
    if !interactive() {
        return Err(HostCliError::usage("use the foreground terminal on this Mac; SSH/noninteractive recovery releases no credentials"));
    }
    let RecoveryTarget {
        instance,
        socket,
        default_org,
        default_login,
    } = recovery_target(&options)?;
    let org_name = options.get("--org").map(|s| s.to_string());
    let org_id = options
        .get("--org-id")
        .map(|s| s.to_string())
        .or(default_org);
    if org_name.is_none() && org_id.is_none() {
        return Err(HostCliError::usage(
            "explicit --org ORGANIZATION_NAME or --org-id ID required",
        )
        .with_help(HELP));
    }
    if org_name.as_deref().is_some_and(str::is_empty)
        || org_id
            .as_deref()
            .is_some_and(|id| !org_office_launch::is_storage_id(id))
    {
        return Err(usage());
    }
    let login = options
        .get("--login")
        .map(|s| s.to_string())
        .or(default_login)
        .ok_or_else(|| HostCliError::usage("explicit --login required").with_help(HELP))?;
    let stream = UnixStream::connect(socket).map_err(error)?;
    // Before anything is said to it, and before a password is asked for: this command signs
    // in with a password and collects a recovery secret, and a host known only by its user
    // is sent neither.
    super::verify_host(&stream)?
        .may_receive_secrets()
        .map_err(HostCliError::auth)?;
    let mut wire = Wire::new(stream)?;
    let hello: HelloResult = wire
        .call::<Hello>(
            &HelloParams::offering(org_host_control::SUPPORTED_API, "agentic-org"),
            None,
        )
        .map_err(error)?;
    if hello.instance_id.as_deref() != Some(instance.as_str()) {
        return Err(error(()).into());
    }
    let password = super::password().map_err(HostCliError::password)?;
    let signed = match wire
        .call_answer::<ops::AccountVerify>(&ops::AccountVerifyParams { login, password }, None)
        .map_err(error)?
    {
        Ok(signed) => signed,
        Err(code) => {
            return Err(crate::cli_error::sign_in_refused(
                code,
                "invalid login or password",
            ))
        }
    };
    if !signed.admin {
        // The sign-in made a session; it ends before the refusal is returned.
        let _ = wire.call::<ops::SessionEnd>(
            &ops::SessionParams {
                session: session(&signed.session),
            },
            None,
        );
        return Err(HostCliError::auth("instance administrator required"));
    }
    // A name is resolved among the organizations this account is linked to, after it
    // signed in; the session is ended whatever the answer.
    // A name the person gave wins over an organization the target supplied.
    let org = match (org_name, org_id) {
        (Some(name), _) => resolve_org(&signed.orgs, &name),
        (None, Some(id)) => Ok(id),
        (None, None) => Err(usage()),
    };
    let result = org.and_then(|org| {
        if command == "release" {
            collect(
                &mut wire,
                &signed.session,
                &instance,
                &org,
                redeliver,
                options.get("--package").map(PathBuf::from),
                out,
            )
        } else {
            status(&mut wire, &signed.session, &org, out).map(drop)
        }
    });
    // Logout cannot undo accepted office transactions or acknowledged delivery.
    let _ = wire.call::<ops::SessionEnd>(
        &ops::SessionParams {
            session: session(&signed.session),
        },
        None,
    );
    result
}
#[cfg(test)]
mod tests;
