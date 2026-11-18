//! Native, terminal-only composition. No secret argv/config/stdout/JSON mode.
use super::{session, Wire};
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
const HELP:&str="agentic-org recovery <status|release> [--name NAME] [--org-id ID] [--login LOGIN] [--redeliver] [--package NEW_FILE]\nOr select an explicit --host-socket PATH --instance-id ID. Collection requires this Mac's foreground terminal and a fresh OS confirmation. REC-1 HQ-loss recovery is unsupported.";
fn error<T>(_: T) -> String {
    "recovery is unresolved; inspect persisted status before explicit redelivery; no retention acknowledgement was inferred".into()
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
fn show(status: &dto::StatusResult) {
    println!("Recovery: {:?}; delivery ordinal {}; first shown {:?}; last redelivered {:?}; redeliveries {}; acknowledged {:?} by {:?}.",status.state,status.delivery_ordinal,status.first_shown_ms,status.last_redelivered_ms,status.redelivery_count,status.acknowledged_ms,status.acknowledged_by);
    for event in &status.audit {
        println!(
            "Delivery {}: {} by {} at {} ms UTC",
            event.ordinal, event.action, event.account, event.at_ms
        );
    }
    println!(
        "REC-1 HQ-loss recovery is unsupported. Delivery/export does not prove off-HQ retention."
    );
}
pub(super) fn status(
    wire: &mut Wire,
    handle: &SessionHandle,
    org: &str,
) -> Result<dto::StatusResult, String> {
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
        show(&result);
        if result.next_after_ordinal.is_none() {
            return Ok(result);
        }
        let next = result.next_after_ordinal.clone();
        if next == after {
            return Err(error(()));
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
        .and_then(|_| file.sync_all())
        .map_err(error)?;
    #[cfg(target_os = "macos")]
    // SAFETY: live writable file descriptor, no pointer argument.
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_FULLFSYNC) } != 0 {
        return Err(error(()));
    }
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
) -> Result<(), String> {
    if !interactive() {
        return Err("recovery releaseion requires the local foreground console terminal; no credentials or code collected".into());
    }
    let observed = status(wire, handle, org)?;
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
        status(wire, handle, org)?;
        return Ok(());
    }
    if observed.state == dto::DeliveryState::Delivered && !redeliver {
        return Err("already shown; inspect the recorded dates and use --redeliver for a fresh authenticated showing of the same code".into());
    }
    let destination = match destination {
        Some(p) => p,
        None => PathBuf::from(prompt(
            "Absolute NEW file for encrypted public recovery package: ",
            4096,
        )?),
    };
    if !destination.is_absolute() {
        return Err("package destination must be absolute".into());
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
        return Err(error(()));
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
    let challenge = super::terminal::secret(b"Final code group (hidden): ")?;
    if challenge.expose().as_bytes()
        != result
            .code
            .expose()
            .as_bytes()
            .get(45..49)
            .ok_or_else(|| error(()))?
    {
        return Err("code group did not match; remains Shown; no ACK was sent".into());
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
        return Err(error(()));
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
    status(wire, handle, org)?;
    Ok(())
}
fn recovery_options<'a>(
    command: &str,
    arguments: &'a [String],
) -> Result<(BTreeMap<&'a str, &'a str>, bool), String> {
    let mut options = BTreeMap::new();
    let mut redeliver = false;
    let mut args = arguments.iter();
    while let Some(flag) = args.next() {
        if flag == "--redeliver" {
            if redeliver || command != "release" {
                return Err(HELP.into());
            }
            redeliver = true;
            continue;
        }
        if !matches!(
            flag.as_str(),
            "--name" | "--org-id" | "--login" | "--host-socket" | "--instance-id" | "--package"
        ) || options
            .insert(flag.as_str(), args.next().ok_or(HELP)?.as_str())
            .is_some()
        {
            return Err(HELP.into());
        }
    }
    if command == "status" && options.contains_key("--package") {
        return Err(HELP.into());
    }
    Ok((options, redeliver))
}

struct RecoveryTarget {
    instance: String,
    socket: PathBuf,
    default_org: Option<String>,
    default_login: Option<String>,
}
fn recovery_target(options: &BTreeMap<&str, &str>) -> Result<RecoveryTarget, String> {
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
                    return Err(error(()));
                }
                (
                    instance,
                    temp.join("host.sock"),
                    // Setup creates no organization and no account (A2).
                    None,
                    None,
                )
            }
            _ => return Err(HELP.into()),
        };
    // Every target (explicit or setup-located) must name an absolute socket and a canonical
    // UUIDv7 instance id.
    if !socket.is_absolute() || !org_proto::is_uuid_v7(&instance) {
        return Err(HELP.into());
    }
    Ok(RecoveryTarget {
        instance,
        socket,
        default_org,
        default_login,
    })
}

pub fn recovery(arguments: &[String]) -> Result<(), String> {
    if arguments.is_empty() || arguments == ["--help"] || arguments == ["-h"] {
        println!("{HELP}");
        return Ok(());
    }
    let command = arguments[0].as_str();
    if !matches!(command, "status" | "release") {
        return Err(HELP.into());
    }
    let (options, redeliver) = recovery_options(command, &arguments[1..])?;
    // No password collection before terminal/local-console preflight, even for
    // status sign-in. Native host status itself remains nonsecret/no-dialog.
    if !interactive() {
        return Err("use the foreground terminal on this Mac; SSH/noninteractive recovery releases no credentials".into());
    }
    let RecoveryTarget {
        instance,
        socket,
        default_org,
        default_login,
    } = recovery_target(&options)?;
    let org = options
        .get("--org-id")
        .map(|s| s.to_string())
        .or(default_org)
        .ok_or("explicit --org-id required")?;
    if !org_office_launch::is_storage_id(&org) {
        return Err(HELP.into());
    }
    let login = options
        .get("--login")
        .map(|s| s.to_string())
        .or(default_login)
        .ok_or("explicit --login required")?;
    let stream = UnixStream::connect(socket).map_err(error)?;
    super::verify_host(&stream)?;
    let mut wire = Wire::new(stream)?;
    let hello: HelloResult = wire
        .call::<Hello>(
            &HelloParams::offering(org_host_control::SUPPORTED_API, "agentic-org"),
            None,
        )
        .map_err(error)?;
    if hello.instance_id.as_deref() != Some(instance.as_str()) {
        return Err(error(()));
    }
    let signed = wire
        .call::<ops::AccountVerify>(
            &ops::AccountVerifyParams {
                login,
                password: super::password()?,
            },
            None,
        )
        .map_err(error)?;
    if !signed.admin {
        return Err("instance administrator required".into());
    }
    let result = if command == "release" {
        collect(
            &mut wire,
            &signed.session,
            &instance,
            &org,
            redeliver,
            options.get("--package").map(PathBuf::from),
        )
    } else {
        status(&mut wire, &signed.session, &org).map(drop)
    };
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
