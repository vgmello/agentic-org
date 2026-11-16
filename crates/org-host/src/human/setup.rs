//! Human composition of the selected protected workflow. Public argv/config
//! contain no credentials; every irreversible request retains its original ID.
mod service;
use super::{session, Wire};
use crate::{
    bootstrap::BootstrapHost,
    instance::{
        registry::Registry,
        setup::{Input, Journal, Paths, Record},
        Name,
    },
    release::{ArchiveBundle, DirectoryBundle, ReleasePin, ReleaseStore},
    Config, Host,
};
use org_host_control::{ops, provision, HostOpResult, Password};
use org_proto::{Hello, HelloParams, HelloResult};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
        net::{UnixListener, UnixStream},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};
const HELP:&str="agentic-org setup [--name NAME] [--bundle DIRECTORY_OR_TAR_GZ] [--offline] [--non-interactive --config FILE --password-file FILE]\nInitial setup creates HQ only. Passwords never belong in arguments or configuration.";
fn error<T>(_: T) -> String {
    "setup is incomplete; retained original operation IDs and data must be resumed".into()
}
#[derive(Default, Debug, PartialEq, Eq)]
struct Options {
    name: Option<String>,
    bundle: Option<PathBuf>,
    offline: bool,
    noninteractive: bool,
    config: Option<PathBuf>,
    password: Option<PathBuf>,
    help: bool,
}
fn options(args: &[String]) -> Result<Options, String> {
    let mut o = Options::default();
    let mut args = args.iter();
    let mut used = std::collections::BTreeSet::new();
    while let Some(a) = args.next() {
        if !used.insert(a.as_str()) {
            return Err("duplicate setup option".into());
        }
        match a.as_str() {
            "--help" | "-h" => o.help = true,
            "--offline" => o.offline = true,
            "--non-interactive" => o.noninteractive = true,
            "--name" | "--bundle" | "--config" | "--password-file" => {
                let v = args.next().ok_or("missing setup option value")?;
                if v.is_empty() {
                    return Err("empty setup option value".into());
                }
                match a.as_str() {
                    "--name" => o.name = Some(v.clone()),
                    "--bundle" => o.bundle = Some(v.into()),
                    "--config" => o.config = Some(v.into()),
                    _ => o.password = Some(v.into()),
                }
            }
            _ => {
                return Err(format!(
                "unsupported setup option; satellite enrollment uses HQ's Add office flow\n{HELP}"
            ))
            }
        }
    }
    Name::parse(o.name.as_deref()).map_err(error)?;
    if o.noninteractive && (o.config.is_none() || o.password.is_none()) && !o.help {
        return Err("noninteractive setup requires --config and protected --password-file".into());
    }
    Ok(o)
}
fn checked_file(path: &Path, max: u64, private: bool) -> Result<Vec<u8>, String> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(error)?;
    let m = file.metadata().map_err(error)?;
    validate_input_metadata(&m, max, private)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(error)?;
    let after = file.metadata().map_err(error)?;
    if bytes.len() as u64 > max || input_changed(&m, &after) {
        crate::secrets::zero(&mut bytes);
        return Err(error(()));
    }
    Ok(bytes)
}
fn credential(path: Option<&Path>) -> Result<Password, String> {
    let Some(path) = path else {
        return super::password();
    };
    let mut bytes = checked_file(path, 1024, true)?;
    let result = std::str::from_utf8(&bytes)
        .map_err(|_| "password must be UTF-8".to_owned())
        .and_then(|s| {
            Password::new(s.strip_suffix('\n').unwrap_or(s).into()).map_err(str::to_owned)
        });
    crate::secrets::zero(&mut bytes);
    result
}
fn prompt(label: &str, default: Option<&str>) -> Result<String, String> {
    let mut tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|_| {
            "interactive setup requires a controlling terminal or explicit --non-interactive inputs"
        })?;
    if let Some(default) = default {
        write!(tty, "{label} [{default}]: ").map_err(error)?;
    } else {
        write!(tty, "{label}: ").map_err(error)?;
    }
    tty.flush().map_err(error)?;
    let mut bytes = Vec::new();
    loop {
        let mut b = [0];
        tty.read_exact(&mut b).map_err(error)?;
        if b[0] == b'\n' {
            break;
        }
        if bytes.len() >= 4096 {
            return Err("setup input exceeds its profile".into());
        }
        bytes.push(b[0]);
    }
    let value = String::from_utf8(bytes).map_err(error)?;
    if value.is_empty() {
        Ok(default.unwrap_or("").into())
    } else {
        Ok(value)
    }
}
fn input(options: &Options, paths: &Paths, existing: Option<&Record>) -> Result<Input, String> {
    if let Some(path) = &options.config {
        let bytes = checked_file(path, 64 * 1024, false)?;
        let input: Input =
            serde_json::from_slice(&bytes).map_err(|_| "invalid version-1 setup configuration")?;
        input.validate().map_err(error)?;
        paths.validate_root(&input.root).map_err(error)?;
        return Ok(input);
    }
    if let Some(record) = existing {
        return Ok(record.input.clone());
    }
    let root = prompt("Instance data directory", paths.default_root.to_str())?;
    let name = prompt("Organization name", None)?;
    let slug = prompt("Organization slug", None)?;
    let remote = prompt("Remote organization repository URL", None)?;
    let branch = prompt("Organization state branch", Some("org/state"))?;
    let home = prompt("Bootstrap home team name", None)?;
    let login = prompt("Instance administrator login", None)?;
    let port = prompt("Loopback HTTP port", Some("8080"))?
        .parse::<u16>()
        .map_err(|_| "invalid port")?;
    let listen=serde_json::from_value(serde_json::json!({"address":"127.0.0.1","port":port,"scheme":"http_loopback","origin":format!("http://127.0.0.1:{port}"),"hsts":"off"})).map_err(error)?;
    let input = Input {
        version: 1,
        root: PathBuf::from(root),
        login,
        organization: org_proto::selected_bootstrap::PlanV1 {
            version: 1,
            name,
            slug,
            repository_remote_url: remote,
            state_branch: branch,
            home_team_name: home,
        },
        listen,
    };
    input.validate().map_err(error)?;
    paths.validate_root(&input.root).map_err(error)?;
    Ok(input)
}
pub(super) fn paths(name: Option<&str>) -> Result<Paths, String> {
    if !cfg!(target_os = "macos") {
        return Err(
            "protected platform setup/custody is unavailable on this target; no HQ was provisioned"
                .into(),
        );
    }
    let account = crate::home::account_dir().ok_or("account home unavailable")?;
    Paths::prepare(&account, user_temp()?, name).map_err(error)
}
fn user_temp() -> Result<PathBuf, String> {
    #[cfg(target_os = "macos")]
    {
        let mut bytes = vec![0_u8; 4096];
        // SAFETY: module-owned writable storage and supported Darwin query.
        let n = unsafe {
            libc::confstr(
                libc::_CS_DARWIN_USER_TEMP_DIR,
                bytes.as_mut_ptr().cast(),
                bytes.len(),
            )
        };
        if n == 0 || n > bytes.len() || bytes[n - 1] != 0 {
            return Err("OS private temporary directory unavailable".into());
        }
        bytes.truncate(n - 1);
        let path = PathBuf::from(String::from_utf8(bytes).map_err(error)?);
        path.canonicalize().map_err(error)
    }
    #[cfg(not(target_os = "macos"))]
    Err("protected platform setup is unavailable".into())
}
fn config(record: &Record, runtime: crate::release::RuntimeRelease) -> Result<Config, String> {
    let home = record.input.root.join("office-state");
    Config::with(home, runtime.office_program(), crate::APP_TEAM_ID)
        .with_runtime_release(runtime)
        .map_err(error)
}
fn pin(manifest: &org_release::VerifiedManifest) -> ReleasePin {
    ReleasePin {
        release_id: manifest.manifest().release_id().into(),
        sequence: manifest.manifest().sequence(),
        manifest_sha256: manifest
            .manifest_sha256()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    }
}

/// Concrete HQ-only setup. No network acquisition/probe or provider call occurs.
pub fn setup(arguments: &[String]) -> Result<(), String> {
    let options = options(arguments)?;
    if options.help {
        println!("{HELP}");
        return Ok(());
    }
    crate::release::require_build_trust().map_err(|e| {
        format!("{e}; an authentic trusted complete release is required before setup")
    })?;
    let paths = paths(options.name.as_deref())?;
    let journal = Journal::open(&paths.base, options.name.as_deref()).map_err(error)?;
    let existing = journal.read().map_err(error)?;
    let input = input(&options, &paths, existing.as_ref())?;
    if existing
        .as_ref()
        .is_some_and(|r| r.input != input || r.offline != options.offline)
    {
        return Err("setup inputs/offline policy differ from the original request".into());
    }
    let mut store = match ReleaseStore::open(&paths.release_store) {
        Ok(s) => s,
        Err(crate::release::Error::Busy) if existing.as_ref().is_some_and(|r| r.exited) => {
            return existing_ready(&paths, existing.as_ref().unwrap(), &options)
        }
        Err(e) => return Err(e.to_string()),
    };
    let recovered = store.recover().map_err(error)?;
    let selected = if let Some(selected) = recovered {
        let r = existing.ok_or(
            "release is already pinned without a setup intent; explicit adoption/repair required",
        )?;
        if selected != r.release {
            return Err("saved setup and release pin differ; repair required".into());
        }
        // Once the exact private release is durably pinned, retry no longer
        // depends on the original removable input. A --bundle argument cannot
        // replace the frozen release or implicitly upgrade this setup.
        r
    } else if let Some(source) = &options.bundle {
        // Source is inspected no-follow by the actual adapter, never executed.
        let m = fs::symlink_metadata(source).map_err(error)?;
        if m.is_dir() {
            let bundle = DirectoryBundle::open(source).map_err(error)?;
            let r = journal
                .freeze(input.clone(), options.offline, pin(bundle.manifest()))
                .map_err(error)?;
            store.install_directory(&bundle).map_err(error)?;
            r
        } else {
            let mut bundle =
                ArchiveBundle::open(source, paths.release_store.join("staging").as_path())
                    .map_err(error)?;
            let r = journal
                .freeze(input.clone(), options.offline, pin(bundle.manifest()))
                .map_err(error)?;
            store.install_archive(&mut bundle).map_err(error)?;
            r
        }
    } else if existing.is_some() {
        return Err(
            "saved release is not yet installed; supply its original --bundle to resume".into(),
        );
    } else {
        let source = std::env::current_exe()
            .map_err(error)?
            .parent()
            .ok_or("installer path unavailable")?
            .to_owned();
        let bundle=DirectoryBundle::open(&source).map_err(|_|"no complete local release; supply --bundle DIRECTORY_OR_TAR_GZ (downloads are unavailable)")?;
        let r = journal
            .freeze(input, options.offline, pin(bundle.manifest()))
            .map_err(error)?;
        store.install_directory(&bundle).map_err(error)?;
        r
    };
    let installed = paths
        .release_store
        .join("releases")
        .join(&selected.release.release_id)
        .join("agentic-org");
    if std::env::current_exe()
        .map_err(error)?
        .canonicalize()
        .map_err(error)?
        != installed
    {
        // No credentials have been accepted. The next process reopens/verifies
        // the protected pin and its actual loaded image before custody.
        drop(store);
        drop(journal);
        let mut command = Command::new(installed);
        command
            .arg("setup")
            .args(arguments)
            .env_clear()
            .env("PATH", "/usr/bin:/bin");
        return Err(format!(
            "selected release re-exec failed: {:?}",
            command.exec().kind()
        ));
    }
    let runtime = store.admit_self().map_err(error)?;
    let mut selected = selected;
    if !selected.exited {
        provision(&paths, &journal, &mut selected, &options, runtime.clone())?;
    }
    let program = runtime.cli_program();
    drop(runtime);
    service::register(&journal, &selected, options.name.as_deref(), &program)?;
    selected.registered = true;
    journal.write(&selected).map_err(error)?;
    existing_ready(&paths, &selected, &options)
}
fn existing_ready(paths: &Paths, r: &Record, options: &Options) -> Result<(), String> {
    let bundle = DirectoryBundle::open(
        &paths
            .release_store
            .join("releases")
            .join(&r.release.release_id),
    )
    .map_err(error)?;
    if pin(bundle.manifest()) != r.release {
        return Err("service release differs from setup pin; repair required".into());
    }
    let registry = Registry::open(&paths.locator, &paths.user_temp).map_err(error)?;
    let (instance, temp) = registry
        .locate(options.name.as_deref(), &r.input.root)
        .map_err(error)?;
    if Some(instance.as_str()) != r.instance_id.as_deref() {
        return Err("registered instance differs from setup; repair required".into());
    }
    let socket = temp.join("host.sock");
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        if let Ok(stream) = UnixStream::connect(&socket) {
            super::verify_host(&stream)?;
            verify_service_image(&stream, bundle.manifest())?;
            let mut wire = Wire::new(stream)?;
            let hello: HelloResult = wire.call::<Hello>(
                &HelloParams::offering(org_host_control::SUPPORTED_API, "agentic-org"),
                None,
            )?;
            if hello.instance_id != r.instance_id {
                return Err("service instance mismatch".into());
            }
            let status = wire.call::<org_host_control::InstanceStatus>(
                &org_host_control::InstanceStatusParams {},
                None,
            )?;
            if status.instance_id == hello.instance_id.unwrap()
                && status.gateway.state == org_host_control::GatewayState::Running
                && status.offices.active > 0
            {
                println!("HQ ready: {}/tasks\nRemote repository connectivity/publication: unverified. Agent/provider readiness: unverified. Recovery: inspect persisted status with agentic-org recovery status; off-HQ retention is not inferred. REC-1 HQ-loss recovery is unsupported.",r.input.listen.origin());
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            return Err("HQ service not ready; original setup retained; inspect the selected instance and resume".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
fn verify_service_image(
    stream: &UnixStream,
    manifest: &org_release::VerifiedManifest,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let token = crate::peer::audit_token(stream).map_err(|_| "host image unavailable")?;
        let loaded = org_socket::image::loaded_cdhash_for_audit_token(&token)
            .map_err(|_| "host image unavailable")?;
        if manifest.file("agentic-org").and_then(|f| f.macos_cdhash()) == Some(&loaded) {
            return Ok(());
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (stream, manifest);
    Err("running service differs from the original signed release".into())
}
fn provision(
    paths: &Paths,
    journal: &Journal,
    r: &mut Record,
    options: &Options,
    runtime: crate::release::RuntimeRelease,
) -> Result<(), String> {
    let socket_dir = paths.user_temp.join(format!(
        "ao-setup-{}",
        crate::ids::generate().map_err(error)?
    ));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&socket_dir)
        .map_err(error)?;
    struct SocketDir(PathBuf);
    impl Drop for SocketDir {
        fn drop(&mut self) {
            let _ = fs::remove_file(self.0.join("setup.sock"));
            let _ = fs::remove_dir(&self.0);
        }
    }
    let owned = SocketDir(socket_dir);
    let socket = owned.0.join("setup.sock");
    org_proto::location::check_socket_path(&socket).map_err(error)?;
    let listener = UnixListener::bind(&socket).map_err(error)?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).map_err(error)?;
    let host = BootstrapHost::start(
        config(r, runtime)?,
        &paths.locator,
        &paths.user_temp,
        options.name.as_deref(),
        &r.input.root,
        r.input.listen.clone(),
        &socket,
    )
    .map_err(error)?;
    listener.set_nonblocking(true).map_err(error)?;
    let stream = accept_bootstrap(&listener)?;
    super::verify_host(&stream)?;
    let mut wire = Wire::new(stream)?;
    let hello: HelloResult = wire.call::<Hello>(
        &HelloParams::offering(org_host_control::SUPPORTED_API, "agentic-org"),
        None,
    )?;
    if r.instance_id
        .as_ref()
        .is_some_and(|i| Some(i) != hello.instance_id.as_ref())
    {
        return Err("bootstrap instance changed; repair required".into());
    }
    r.instance_id = Some(
        hello
            .instance_id
            .ok_or("protected bootstrap omitted its instance identity")?,
    );
    journal.write(r).map_err(error)?;
    let outcome = wire.call::<ops::Bootstrap>(
        &ops::BootstrapParams {
            operation_id: r.bootstrap_operation.clone(),
            hq_operation_id: r.hq_operation.clone(),
            login: r.input.login.clone(),
            password: credential(options.password.as_deref())?,
            tls: None,
        },
        None,
    )?;
    let result = match outcome {
        ops::BootstrapOutcome::Completed { result } => result,
        ops::BootstrapOutcome::Rejected {
            refusal: ops::BootstrapRefusal::BootstrapFinished,
        } if r.provisioned.is_some() => {
            r.exited = true;
            journal.write(r).map_err(error)?;
            host.shutdown();
            return Ok(());
        }
        _ => return Err("protected bootstrap refused; original setup retained".into()),
    };
    let params = provision::Params {
        session: session(&result.session),
        operation_id: r.hq_operation.clone(),
        plan: r.input.organization.clone(),
    };
    let provisioned = await_provisioning(&mut wire, &params)?;
    if r.provisioned.as_ref().is_some_and(|p| p != &provisioned) {
        return Err("original HQ result differs; repair required".into());
    }
    r.provisioned = Some(provisioned);
    journal.write(r).map_err(error)?;
    collect_optional_recovery(&mut wire, &result.session, r, options)?;
    let _: org_proto::Empty = wire.call::<ops::Exit>(&org_proto::Empty {}, None)?;
    r.exited = true;
    journal.write(r).map_err(error)?;
    drop(wire);
    host.shutdown();
    Ok(())
}

fn accept_bootstrap(listener: &UnixListener) -> Result<UnixStream, String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match listener.accept() {
            Ok((stream, _)) => return Ok(stream),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5))
            }
            Err(_) => return Err(error(())),
        }
    }
}

fn await_provisioning(
    wire: &mut Wire,
    params: &provision::Params,
) -> Result<provision::Provisioned, String> {
    let until = Instant::now() + Duration::from_secs(120);
    loop {
        match wire.call::<provision::OrganizationProvision>(params, None)? {
            HostOpResult::Completed { result } => return Ok(result),
            HostOpResult::Accepted if Instant::now() < until => {
                std::thread::sleep(Duration::from_millis(100))
            }
            _ => {
                return Err(
                    "HQ provisioning is unresolved; resume the original setup operation".into(),
                )
            }
        }
    }
}

fn collect_optional_recovery(
    wire: &mut Wire,
    session: &org_host_control::SessionHandle,
    r: &Record,
    options: &Options,
) -> Result<(), String> {
    // Same native connection/session as protected setup; never fabricate a
    // delivery/ACK. A deferred/failed collection does not block otherwise-ready HQ.
    if !options.noninteractive
        && super::recovery::interactive()
        && prompt(
            "Collect the recovery code and public package now? Type yes, or Enter to defer",
            None,
        )? == "yes"
    {
        let org = &r
            .provisioned
            .as_ref()
            .ok_or_else(|| error(()))?
            .organization_id;
        if let Err(message) = super::recovery::collect(
            wire,
            session,
            r.instance_id.as_deref().ok_or_else(|| error(()))?,
            org,
            false,
            None,
        ) {
            eprintln!("{message}");
        }
    }
    println!("Recovery status is persisted; deferral is allowed. Later: agentic-org recovery status{}; agentic-org recovery release{}. REC-1 HQ-loss recovery is unsupported.",
        options.name.as_ref().map(|n|format!(" --name {n}")).unwrap_or_default(),
        options.name.as_ref().map(|n|format!(" --name {n}")).unwrap_or_default());
    Ok(())
}

/// Registered service entry. Identity/root/listener/offline policy and pin come
/// from the protected original setup record, never service argv overrides.
pub fn serve_selected(arguments: &[String]) -> Result<(), String> {
    let signals = crate::signals::ShutdownSignals::block().map_err(error)?;
    let name = match arguments {
        [] => None,
        [flag, name] if flag == "--name" => Some(name.as_str()),
        _ => return Err("usage: agentic-org serve [--name NAME]".into()),
    };
    crate::release::require_build_trust().map_err(error)?;
    let paths = paths(name)?;
    let r = Journal::snapshot(&paths.base, name).map_err(error)?;
    if !r.exited {
        return Err("bootstrap is incomplete; resume setup".into());
    }
    let runtime = ReleaseStore::open(&paths.release_store)
        .map_err(error)?
        .admit_self()
        .map_err(error)?;
    if runtime.pin() != &r.release {
        return Err(
            "service release differs from setup pin; explicit upgrade/recovery required".into(),
        );
    }
    let registry = Registry::open(&paths.locator, &paths.user_temp).map_err(error)?;
    let host = Host::start_instance(config(&r, runtime)?, &registry, name, &r.input.root)
        .map_err(error)?;
    let stopped = signals.wait();
    host.shutdown();
    stopped.map(drop).map_err(error)
}

#[cfg(test)]
mod tests;

fn validate_input_metadata(m: &fs::Metadata, max: u64, private: bool) -> Result<(), String> {
    if !m.is_file()
        || m.nlink() != 1
        || m.uid() != org_socket::effective_uid()
        || m.mode() & 0o022 != 0
        || m.len() > max
        || private && m.mode() & 0o7777 != 0o600
    {
        return Err("unsafe or over-limit setup input".into());
    }
    Ok(())
}

fn input_changed(m: &fs::Metadata, after: &fs::Metadata) -> bool {
    m.dev() != after.dev()
        || m.ino() != after.ino()
        || m.len() != after.len()
        || m.mtime() != after.mtime()
        || m.mtime_nsec() != after.mtime_nsec()
        || m.ctime() != after.ctime()
        || m.ctime_nsec() != after.ctime_nsec()
}
