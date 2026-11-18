//! `agentic-org setup` (A2 B4, SRV-2..SRV-7, ENC-3): the steps, in order, with the platform's
//! service manager and disk-encryption probe injected by the caller.
//!
//! 1. Platform gate: a release build without a pinned runtime release refuses (SRV-1); the
//!    caller's [`crate::human::paths`] refuses systems without instance locations.
//! 2. Lock and freeze the setup record (`setup.lock`). The address is chosen before anything
//!    is written: port 7420 for `default`, the first free port from 7421 for a named instance;
//!    an occupied requested port is an actionable error and nothing is saved (SRV-4).
//! 3. Reserve the instance, record its listener and prepare the layout.
//! 4. Report disk encryption and the folder modes, read back from disk (ENC-3).
//! 5. Register the service and start it unless it already runs (SRV-6).
//! 6. Wait until `host.sock` answers hello and `instance.status` for this instance.
//! 7. Ask for the initial registration link once (`admin.link.initial`) and print it, only
//!    when the output is a terminal. Noninteractive setup asks for no link and names
//!    `agentic-org admin reset` instead (ADR-0022, UX §4a.2).
//!
//! Every step is idempotent; a second run reuses the frozen record and prints no link.
//! Ports other instances saved are never chosen, and a saved port another program holds
//! is reported before the service starts.
use std::ffi::OsString;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use org_host_control::admin::{
    AdminLinkInitial, AdminLinkInitialParams, AdminLinkResult, AdminReset, AdminResetParams,
};
use org_host_control::listen::ListenConfig;
use org_host_control::{
    Administrator, GatewayState, InstanceStatus, InstanceStatusParams, InstanceStatusResult,
};
use org_platform::disk::{DiskEncryption, EncryptionState};
use org_platform::service::{ServiceManager, ServiceStatus, Unit};
use org_proto::{Hello, HelloParams};

use crate::human::Wire;
use crate::instance::registry::Registry;
use crate::instance::setup::{Input, Journal, Paths, Record};
use crate::release::ReleasePin;

#[cfg(all(feature = "test-support", debug_assertions))]
pub mod fixture;
#[cfg(test)]
mod tests;

/// The port of the `default` instance; named instances take the first free one after it.
pub const DEFAULT_PORT: u16 = 7420;
const READY_WITHIN: Duration = Duration::from_secs(60);

/// The platform adapters setup composes.
pub struct Platform {
    pub service: Box<dyn ServiceManager>,
    pub disk: Box<dyn DiskEncryption>,
}

/// Asked once, on the first run only, with the default address; `Ok(None)` accepts it.
pub type PortPrompt<'a> = &'a mut dyn FnMut(&str) -> Result<Option<u16>, String>;

/// What the person asked for.
pub struct Request<'a> {
    pub name: Option<String>,
    /// `--port`; without it the default is offered to `prompt` (if any) and then used.
    pub port: Option<u16>,
    pub prompt: Option<PortPrompt<'a>>,
    /// The service program (`agentic-org` itself) and its explicit environment.
    pub program: PathBuf,
    pub env: Vec<(OsString, OsString)>,
    /// Whether `out` is a terminal. The one-time link is printed only to a terminal.
    pub terminal: bool,
}

fn fail(context: &str, why: impl std::fmt::Display) -> String {
    format!("{context}: {why}")
}

/// The release this setup records. Development builds record a fixed development pin; a
/// release build refuses until it carries a pinned runtime release (SRV-1).
pub(crate) fn release_pin() -> Result<ReleasePin, String> {
    if cfg!(debug_assertions) {
        use sha2::Digest;
        let digest = sha2::Sha256::digest(
            format!("agentic-org development {}", env!("CARGO_PKG_VERSION")).as_bytes(),
        );
        Ok(ReleasePin {
            release_id: "development".into(),
            sequence: 1,
            manifest_sha256: digest.iter().map(|b| format!("{b:02x}")).collect(),
        })
    } else {
        Err(
            "setup needs a pinned runtime release, which this build does not carry yet \
             (SRV-1); nothing was changed"
                .into(),
        )
    }
}

pub(crate) fn loopback(port: u16) -> Result<ListenConfig, String> {
    serde_json::from_value(serde_json::json!({
        "address": "127.0.0.1", "port": port, "scheme": "http_loopback",
        "origin": format!("http://127.0.0.1:{port}"), "hsts": "off"
    }))
    .map_err(|e| fail("building the address", e))
}

fn port_free(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_ok()
}

/// The ports other instances saved, by name: their frozen setup records and the browser
/// addresses in the instance locator. Unreadable entries are skipped; this only steers
/// the choice of a new port.
fn saved_ports(paths: &Paths, name: Option<&str>) -> Vec<(String, u16)> {
    let shown = name.unwrap_or("default");
    let mut saved = Vec::new();
    if let Ok(entries) = std::fs::read_dir(paths.base.join("setup")) {
        for entry in entries.flatten() {
            let Some(other) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if other == shown {
                continue;
            }
            if let Ok(record) = Journal::snapshot(&paths.base, Some(&other)) {
                saved.push((other, record.input.listen.port()));
            }
        }
    }
    if let Ok(registry) = Registry::open(&paths.locator, &paths.user_temp) {
        if let Ok(ports) = registry.saved_browser_ports() {
            saved.extend(ports.into_iter().filter(|(other, _)| other != shown));
        }
    }
    saved.sort();
    saved.dedup();
    saved
}

/// Refuses a saved port that another program holds while this instance's service is not
/// running: the service could not serve its address, and the address does not change.
fn port_held_elsewhere(port: u16, name: Option<&str>) -> String {
    format!(
        "port {port} on 127.0.0.1, this instance's saved address, is in use by another \
         program, so its service cannot serve it; stop that program, then run {}",
        match name {
            None | Some("default") => "agentic-org setup".to_owned(),
            Some(name) => format!("agentic-org setup --name {name}"),
        }
    )
}

/// The first run's port: the requested one if free, otherwise the instance's default.
/// `saved` holds the ports other instances saved, by name; none of them is chosen.
pub(crate) fn choose_port(
    name: Option<&str>,
    requested: Option<u16>,
    free: impl Fn(u16) -> bool,
    saved: &[(String, u16)],
) -> Result<u16, String> {
    let owner = |port: u16| saved.iter().find(|(_, p)| *p == port).map(|(n, _)| n);
    if let Some(port) = requested {
        if port == 0 {
            return Err("port 0 is not a fixed address; choose a port from 1 to 65535".into());
        }
        if let Some(other) = owner(port) {
            return Err(format!(
                "port {port} is the saved address of instance \"{other}\"; choose another \
                 port with --port; nothing was saved"
            ));
        }
        if !free(port) {
            return Err(format!(
                "port {port} on 127.0.0.1 is in use by another program; choose another \
                 port with --port, or stop that program; nothing was saved"
            ));
        }
        return Ok(port);
    }
    match name {
        None | Some("default") => {
            if let Some(other) = owner(DEFAULT_PORT) {
                Err(format!(
                    "port {DEFAULT_PORT} is the saved address of instance \"{other}\"; choose \
                     another port with --port; nothing was saved"
                ))
            } else if free(DEFAULT_PORT) {
                Ok(DEFAULT_PORT)
            } else {
                Err(format!(
                    "port {DEFAULT_PORT} on 127.0.0.1 is in use by another program; choose \
                     another port with --port, or stop that program; nothing was saved"
                ))
            }
        }
        Some(_) => (DEFAULT_PORT + 1..=u16::MAX)
            .find(|p| owner(*p).is_none() && free(*p))
            .ok_or_else(|| "no free port on 127.0.0.1 from 7421; nothing was saved".into()),
    }
}

/// Folder and file modes under `root`, read back from disk: the paths whose mode is not
/// 700 (folders) or 600 (files). Symbolic links are reported, never followed.
pub(crate) fn mode_exceptions(root: &Path) -> Result<Vec<(PathBuf, u32)>, String> {
    let mut exceptions = Vec::new();
    let mut pending = vec![root.to_owned()];
    while let Some(path) = pending.pop() {
        let meta = std::fs::symlink_metadata(&path).map_err(|e| fail("reading modes", e))?;
        let mode = meta.permissions().mode() & 0o7777;
        let expected = if meta.is_dir() { 0o700 } else { 0o600 };
        if meta.file_type().is_symlink() || mode != expected {
            exceptions.push((path.clone(), mode));
        }
        if meta.is_dir() {
            for entry in std::fs::read_dir(&path).map_err(|e| fail("reading modes", e))? {
                pending.push(entry.map_err(|e| fail("reading modes", e))?.path());
            }
        }
    }
    exceptions.sort();
    Ok(exceptions)
}

/// The ENC-3 lines: disk encryption, the modes actually found and the isolation mode.
pub(crate) fn protection_report(
    encryption: &EncryptionState,
    user: &str,
    exceptions: &[(PathBuf, u32)],
) -> Vec<String> {
    let mut lines = vec![match encryption {
        EncryptionState::On => "Disk encryption (FileVault): on.".to_owned(),
        EncryptionState::Off => "Disk encryption is off. Agentic Org does not encrypt its data; \
             turn on FileVault in System Settings › Privacy & Security."
            .to_owned(),
        EncryptionState::Unknown(why) => format!(
            "Disk encryption could not be determined ({why}). Agentic Org does not encrypt \
             its data; check FileVault in System Settings › Privacy & Security."
        ),
    }];
    if exceptions.is_empty() {
        lines.push(format!(
            "File permissions: the instance folder and everything in it are readable only by \
             {user} (folders 700, files 600)."
        ));
    } else {
        lines.push(format!(
            "File permissions: {} path(s) in the instance folder are not private to {user}:",
            exceptions.len()
        ));
        for (path, mode) in exceptions.iter().take(10) {
            lines.push(format!("  {:o} {}", mode, path.display()));
        }
    }
    lines.push(format!(
        "Agents run as {user} (same-user mode): only the tool sandbox separates agents from \
         the server's data."
    ));
    lines
}

fn user_name() -> String {
    std::env::var("USER")
        .ok()
        .filter(|u| !u.is_empty() && !u.contains(char::is_control))
        .unwrap_or_else(|| format!("uid {}", crate::home::euid()))
}

fn say(out: &mut dyn Write, line: impl std::fmt::Display) -> Result<(), String> {
    writeln!(out, "{line}").map_err(|e| fail("writing output", e))
}

/// The platform's own adapters: the account's service manager and disk-encryption probe.
/// Systems without them refuse here, before anything is changed.
pub fn platform() -> Result<Platform, String> {
    let home = crate::home::account_dir().ok_or("the account home folder is unavailable")?;
    let refused = |e: org_platform::Refused| e.to_string();
    Ok(Platform {
        service: org_platform::service::service(
            org_platform::service::ServiceMode::Registered,
            &home,
        )
        .map_err(refused)?,
        disk: org_platform::disk::disk_encryption().map_err(refused)?,
    })
}

/// Runs setup for `request`. See the module docs for the steps.
pub fn run(platform: &Platform, request: Request<'_>, out: &mut dyn Write) -> Result<(), String> {
    // A release build without a pinned runtime refuses before any path is resolved or made.
    release_pin()?;
    let paths = crate::human::paths(request.name.as_deref())?;
    run_at(&paths, platform, request, out)
}

pub(crate) fn run_at(
    paths: &Paths,
    platform: &Platform,
    mut request: Request<'_>,
    out: &mut dyn Write,
) -> Result<(), String> {
    // 1. Platform gate.
    let pin = release_pin()?;
    let name = request.name.as_deref();
    let shown = name.unwrap_or("default");
    say(out, format!("Setting up instance \"{shown}\"."))?;

    // 2. Lock and freeze the record. The address is decided before anything is written.
    let journal = Journal::open(&paths.base, name).map_err(|e| fail("opening setup", e))?;
    let mut record = match journal.read().map_err(|e| fail("reading setup", e))? {
        Some(record) => {
            if request
                .port
                .is_some_and(|p| p != record.input.listen.port())
            {
                return Err(format!(
                    "this instance was set up on port {}; its address does not change on a \
                     later run; nothing was changed",
                    record.input.listen.port()
                ));
            }
            record
        }
        None => {
            let saved = saved_ports(paths, name);
            let mut port = choose_port(name, request.port, port_free, &saved)?;
            if request.port.is_none() {
                if let Some(prompt) = request.prompt.as_mut() {
                    if let Some(chosen) = prompt(&format!("http://127.0.0.1:{port}"))? {
                        port = choose_port(name, Some(chosen), port_free, &saved)?;
                    }
                }
            }
            let root = paths
                .instance_root(name)
                .map_err(|e| fail("preparing the instance folder", e))?;
            let input = Input {
                version: 1,
                root,
                listen: loopback(port)?,
            };
            journal
                .freeze(input, false, pin)
                .map_err(|e| fail("saving setup", e))?
        }
    };
    say(out, format!("Data: {}", record.input.root.display()))?;
    say(
        out,
        format!("Address: {}", record.input.listen.origin().as_str()),
    )?;

    // 3. Reserve the instance, record its listener and prepare its layout, then record its
    // ID. Once the ID is recorded all three are done, and a running host holds the root lock,
    // so a later run only locates the instance. The root lock is released before the service
    // starts, since the host reserves the instance again.
    let registry = Registry::open(&paths.locator, &paths.user_temp)
        .map_err(|e| fail("opening the instance locator", e))?;
    let (instance_id, socket) = match &record.instance_id {
        Some(recorded) => {
            let (id, temp) = registry
                .locate(name, &record.input.root)
                .map_err(|e| fail("locating the instance", e))?;
            if &id != recorded {
                return Err(
                    "the reserved instance is not the one setup recorded; nothing was changed"
                        .into(),
                );
            }
            (id, temp.join("host.sock"))
        }
        None => {
            registry
                .restore_temp_folders()
                .map_err(|e| fail("restoring instance temp folders", e))?;
            let reserved = {
                let instance = registry
                    .reserve(name, &record.input.root)
                    .map_err(|e| fail("reserving the instance", e))?;
                instance
                    .set_browser(&record.input.listen)
                    .map_err(|e| fail("recording the instance address", e))?;
                instance
                    .prepare_layout()
                    .map_err(|e| fail("preparing the instance folder", e))?;
                (
                    instance.id().to_owned(),
                    instance.temp_folder().join("host.sock"),
                )
            };
            record.instance_id = Some(reserved.0.clone());
            journal
                .write(&record)
                .map_err(|e| fail("saving setup", e))?;
            reserved
        }
    };

    // 4. ENC-3, with the modes read back from disk.
    let exceptions = mode_exceptions(&record.input.root)?;
    for line in protection_report(
        &platform.disk.state(&record.input.root),
        &user_name(),
        &exceptions,
    ) {
        say(out, line)?;
    }

    // 5. The service: registered, and started unless it already runs.
    let service_id = service_id(&instance_id);
    let unit = Unit {
        instance_id: service_id.clone(),
        program: request.program.clone(),
        args: ["serve", "--name", shown].map(OsString::from).to_vec(),
        env: std::mem::take(&mut request.env),
    };
    platform
        .service
        .register(&unit)
        .map_err(|e| fail("registering the service", e))?;
    if !record.service_registered {
        record.service_registered = true;
        journal
            .write(&record)
            .map_err(|e| fail("saving setup", e))?;
    }
    say(out, "Service registered.")?;
    let port = record.input.listen.port();
    match platform.service.status(&service_id) {
        Ok(ServiceStatus::Running { .. }) => {}
        _ => {
            // Nothing of this instance runs, so a held port belongs to another program.
            if !port_free(port) {
                return Err(port_held_elsewhere(port, name));
            }
            platform
                .service
                .start(&service_id)
                .map_err(|e| fail("starting the service", e))?
        }
    }

    // 6. Readiness: hello and instance.status for this instance within the deadline.
    let status = ready(&socket, &instance_id, READY_WITHIN)?;
    // A failed browser service no longer holds its port; if something else does, say so.
    if status.gateway.state == GatewayState::Failed && !port_free(port) {
        return Err(port_held_elsewhere(port, name));
    }
    let administrator = status.administrator;
    say(out, "Service running.")?;
    if !record.registry_created {
        record.registry_created = true;
        journal
            .write(&record)
            .map_err(|e| fail("saving setup", e))?;
    }

    // 7. The initial link, at most once.
    if administrator == Administrator::Present {
        return say(
            out,
            format!(
                "Instance {shown} is running. Open {} and sign in.",
                record.input.listen.origin().as_str()
            ),
        );
    }
    say(out, "Instance ready. It has no administrator yet.")?;
    // The link is shown only in a terminal; noninteractive setup does not ask for it.
    let result = if record.initial_link_issued || !request.terminal {
        AdminLinkResult::AlreadyIssued
    } else {
        let mut wire = connect(&socket, &instance_id)?;
        let result = wire
            .call::<AdminLinkInitial>(
                &AdminLinkInitialParams {
                    setup_operation: record.setup_operation.clone(),
                },
                None,
            )
            .map_err(|e| fail("asking for the registration link", e))?;
        record.initial_link_issued = true;
        journal
            .write(&record)
            .map_err(|e| fail("saving setup", e))?;
        result
    };
    show_link(out, result, name)
}

/// The service manager's name for an instance: its ID in lowercase (the label is
/// `com.ellosoft.agentic-org.instance.<id>`; instance IDs are case-insensitive ULIDs/UUIDs).
fn service_id(instance_id: &str) -> String {
    instance_id.to_ascii_lowercase()
}

fn reset_hint(name: Option<&str>) -> String {
    match name {
        None | Some("default") => "agentic-org admin reset".into(),
        Some(name) => format!("agentic-org admin reset --name {name}"),
    }
}

fn show_link(
    out: &mut dyn Write,
    result: AdminLinkResult,
    name: Option<&str>,
) -> Result<(), String> {
    match result {
        AdminLinkResult::Issued { url, expires_at_ms } => {
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
            say(out, format!("  {}", url.expose()))?;
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
    // SAFETY: `tm` is plain data written by localtime_r from a valid time_t.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&secs, &mut tm) }.is_null() {
        return format!("{ms} ms after the Unix epoch, UTC");
    }
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min
    )
}

fn connect(socket: &Path, instance_id: &str) -> Result<Wire, String> {
    let stream = UnixStream::connect(socket).map_err(|e| fail("connecting to the service", e))?;
    crate::human::verify_host(&stream)?;
    let mut wire = Wire::new(stream)?;
    let hello = wire
        .call::<Hello>(
            &HelloParams::offering(org_host_control::SUPPORTED_API, "agentic-org"),
            None,
        )
        .map_err(|e| fail("greeting the service", e))?;
    if hello.instance_id.as_deref() != Some(instance_id) {
        return Err("the service answering is not this instance".into());
    }
    Ok(wire)
}

/// Polls until the service answers for `instance_id`, or the deadline passes.
fn ready(
    socket: &Path,
    instance_id: &str,
    within: Duration,
) -> Result<InstanceStatusResult, String> {
    let deadline = Instant::now() + within;
    loop {
        let attempt = connect(socket, instance_id).and_then(|mut wire| {
            wire.call::<InstanceStatus>(&InstanceStatusParams {}, None)
                .map_err(|e| fail("reading the instance status", e))
        });
        match attempt {
            Ok(status) if status.instance_id == instance_id => return Ok(status),
            Ok(_) => return Err("the service answering is not this instance".into()),
            Err(why) if Instant::now() >= deadline => {
                return Err(format!(
                    "the service did not become ready within {}s ({why}); setup can be run \
                     again and continues where it stopped",
                    within.as_secs()
                ))
            }
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

/// Finds the registered instance `name`: its ID and `host.sock`.
fn registered(paths: &Paths, name: Option<&str>) -> Result<(Record, String, PathBuf), String> {
    let record =
        Journal::snapshot(&paths.base, name).map_err(|e| fail("reading the setup record", e))?;
    let registry = Registry::open(&paths.locator, &paths.user_temp)
        .map_err(|e| fail("opening the instance locator", e))?;
    let (id, temp) = registry
        .locate(name, &record.input.root)
        .map_err(|e| fail("locating the instance", e))?;
    if record.instance_id.as_deref() != Some(id.as_str()) {
        return Err("the instance is not the one setup recorded; run agentic-org setup".into());
    }
    Ok((record, id, temp.join("host.sock")))
}

/// `agentic-org admin reset`: a new registration link after presence at the host.
/// `terminal` says whether `out` is a terminal: the link is printed only there, so
/// otherwise this refuses before asking the host, and no link is issued or cancelled.
pub fn admin_reset(name: Option<&str>, terminal: bool, out: &mut dyn Write) -> Result<(), String> {
    if !terminal {
        return Err(
            "admin reset prints a one-time link only to a terminal; run it in Terminal on \
             this Mac without redirecting its output; nothing was changed"
                .into(),
        );
    }
    let (_, id, socket) = registered(&crate::human::paths(name)?, name)?;
    let mut wire = connect(&socket, &id).map_err(|why| {
        format!(
            "{why}; the service is not running: start it with agentic-org service start{}",
            name.map(|n| format!(" --name {n}")).unwrap_or_default()
        )
    })?;
    let result = wire
        .call::<AdminReset>(&AdminResetParams::default(), None)
        .map_err(|e| fail("asking for a new registration link", e))?;
    if matches!(result, AdminLinkResult::AdministratorPresent) {
        return say(
            out,
            "The instance has an administrator; resetting its password is not available yet.",
        );
    }
    show_link(out, result, name)
}

/// `agentic-org service start|stop|status`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceAction {
    Start,
    Stop,
    Status,
}

pub fn service(
    name: Option<&str>,
    manager: &dyn ServiceManager,
    action: ServiceAction,
    out: &mut dyn Write,
) -> Result<(), String> {
    let (_, id, _) = registered(&crate::human::paths(name)?, name)?;
    let id = service_id(&id);
    let shown = name.unwrap_or("default");
    match action {
        ServiceAction::Start => {
            manager
                .start(&id)
                .map_err(|e| fail("starting the service", e))?;
            say(out, format!("Service for \"{shown}\" started."))
        }
        ServiceAction::Stop => {
            manager
                .stop(&id)
                .map_err(|e| fail("stopping the service", e))?;
            say(out, format!("Service for \"{shown}\" stopped."))
        }
        ServiceAction::Status => {
            let status = manager
                .status(&id)
                .map_err(|e| fail("reading the service", e))?;
            say(
                out,
                match status {
                    ServiceStatus::Running { pid } => format!("running (pid {pid})"),
                    ServiceStatus::Stopped => "stopped".into(),
                    ServiceStatus::Unloaded => "registered, not loaded".into(),
                    ServiceStatus::NotRegistered => "not registered".into(),
                },
            )
        }
    }
}
