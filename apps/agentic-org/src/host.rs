//! The host adapters the commands compose: the platform (service manager, disk encryption,
//! credential directory), the non-interactive setup from a file that `setup` and
//! `serve --setup` share, and the service's environment. Not a feature: the vertical slices
//! under `features/` use it and never each other.
//!
//! Development builds with the `test-support` feature compose test adapters instead of the
//! system's service manager when `ORG_DEV_SERVICE_DIR` names a folder: the service runs as a
//! detached child (`ChildServiceManager`), and `ORG_DEV_DISK_ENCRYPTION` (`on`, `off`, anything
//! else unknown) is the disk-encryption answer. `ORG_DEV_SERVICE_PROGRAMS` composes the
//! system's own service adapter instead, with the programs it runs taken from the folder the
//! variable names (on Linux, stand-ins for `systemctl` and `loginctl`), so the real adapter is
//! exercised without systemd. Release builds never contain either, and a build without the
//! adapters refuses both variables instead of registering a real service.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{self, Write};

use org_cli_shell::error::ShellError;
use org_host::setup_flow::{self, Platform, Request, ServiceReport, ServiceState};

pub(crate) fn own_program() -> Result<std::path::PathBuf, String> {
    std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .map_err(|e| format!("locating agentic-org: {e}"))
}

/// Non-interactive, offline setup of instance `name` from the configuration at `file`:
/// nothing is asked, nothing is fetched and no link is printed. It is idempotent: a later
/// run with the same file finds the instance set up and changes nothing, and a file that
/// differs from what the instance was set up with is refused. The caller has already
/// refused a user who may not own the instance.
pub(crate) fn configured(
    file: &std::path::Path,
    name: Option<&str>,
    out: &mut dyn Write,
) -> Result<(), String> {
    apply(setup_flow::configuration(file)?, name, out).map(drop)
}

/// [`configured`] for `serve --setup`: the file must set up the foreground mode, in which
/// nothing is registered and the caller is the service. A file that sets up a registered
/// service is refused before anything is changed: its service manager runs it.
///
/// When this run reserved the instance, it returns the instance's browser listener, bound
/// since setup checked the address, for the host the caller starts next
/// ([`org_host::Config::browser_listener`]).
pub(crate) fn configured_foreground(
    file: &std::path::Path,
    name: Option<&str>,
    out: &mut dyn Write,
) -> Result<Option<org_host::BrowserListener>, String> {
    let configuration = setup_flow::configuration(file)?;
    if !configuration.foreground() {
        return Err(format!(
            "{} sets up a registered service, which its service manager runs; serve --setup \
             is the foreground mode and needs \"service\": \"foreground\"; nothing was changed",
            file.display()
        ));
    }
    apply(configuration, name, out)
}

fn apply(
    configuration: setup_flow::Configuration,
    name: Option<&str>,
    out: &mut dyn Write,
) -> Result<Option<org_host::BrowserListener>, String> {
    let foreground = configuration.foreground();
    let platform = platform(foreground)?;
    // The foreground mode registers no service: nothing is given an environment, and its
    // host takes its development key folder as `serve` does.
    let (env, development_keys) = if foreground {
        (Vec::new(), None)
    } else {
        (service_environment()?, development_keys()?)
    };
    let request = Request {
        name: name.map(str::to_owned),
        port: None,
        prompt: None,
        named_port_base: named_port_base()?,
        program: own_program()?,
        env,
        terminal: false,
        development_keys,
        configuration: Some(configuration),
        federation: None,
    };
    setup_flow::run(&platform, request, out)
}

/// Where a named instance's port search starts: 7421 in every build that people run. A
/// debug build with `test-support` reads `ORG_DEV_NAMED_PORT_BASE` so its tests search a
/// range of their own instead of the one real instances and other test runs use.
pub(crate) fn named_port_base() -> Result<u16, String> {
    #[cfg(all(debug_assertions, feature = "test-support"))]
    if let Some(value) = std::env::var_os("ORG_DEV_NAMED_PORT_BASE").filter(|v| !v.is_empty()) {
        return value
            .to_str()
            .and_then(|v| v.parse::<u16>().ok())
            .filter(|base| *base != 0)
            .ok_or_else(|| {
                "ORG_DEV_NAMED_PORT_BASE is not a port from 1 to 65535; nothing was changed"
                    .to_owned()
            });
    }
    Ok(setup_flow::NAMED_PORT_BASE)
}

/// absolute path ([`development_keys`]).
pub(crate) fn service_environment() -> Result<Vec<(OsString, OsString)>, String> {
    let mut env = Vec::new();
    if let Some(home) = std::env::var_os("HOME").filter(|h| !h.is_empty()) {
        env.push(("HOME".into(), home));
    }
    env.extend(setup_flow::path_environment());
    if cfg!(debug_assertions) {
        if let Some(value) = std::env::var_os("ORG_DEV_ACCOUNT_BASE").filter(|v| !v.is_empty()) {
            env.push(("ORG_DEV_ACCOUNT_BASE".into(), value));
        }
        if let Some(folder) = development_keys()? {
            env.push(("ORG_OFFICE_DEV_KEY_DIR".into(), folder.into()));
        }
    }
    Ok(env)
}

/// A development build's key folder, `ORG_OFFICE_DEV_KEY_DIR`, which the service is given:
/// an absolute path, or refused before anything is changed (X5 review P3-1): the service
/// does not run in the folder setup ran in, so a relative one would name another folder
/// there. A release build has none.
pub(crate) fn development_keys() -> Result<Option<std::path::PathBuf>, String> {
    if !cfg!(debug_assertions) {
        return Ok(None);
    }
    match std::env::var_os("ORG_OFFICE_DEV_KEY_DIR").filter(|v| !v.is_empty()) {
        None => Ok(None),
        Some(folder) if std::path::Path::new(&folder).is_absolute() => Ok(Some(folder.into())),
        Some(folder) => Err(format!(
            "ORG_OFFICE_DEV_KEY_DIR ({}) is not an absolute path: the service does not run in \
             this folder; name the development key folder by its absolute path; nothing was \
             changed",
            std::path::Path::new(&folder).display()
        )),
    }
}

/// The platform adapters setup and the service commands compose. `foreground`: the
/// service manager that registers nothing, on every system and in every build; the
/// development stand-ins are for a registered service only.
pub(crate) fn platform(foreground: bool) -> Result<Platform, String> {
    if foreground {
        return setup_flow::platform(true);
    }
    #[cfg(all(debug_assertions, feature = "test-support"))]
    if let Some(service) = development_service()? {
        use org_platform::disk::EncryptionState;
        use org_platform::testing::FakeDiskEncryption;
        let state = match std::env::var("ORG_DEV_DISK_ENCRYPTION").as_deref() {
            Ok("on") => EncryptionState::On,
            Ok("off") => EncryptionState::Off,
            _ => EncryptionState::Unknown("development test adapter".into()),
        };
        return Ok(Platform {
            service,
            disk: Box::new(FakeDiskEncryption(state)),
            // A development build keeps its secrets in its development key folder.
            credentials: setup_flow::CredentialDirectory::Unused,
        });
    }
    #[cfg(not(all(debug_assertions, feature = "test-support")))]
    refuse_development_service()?;
    setup_flow::platform(false)
}

/// The account's registered service manager alone, as [`platform`] chooses it (the
/// development stand-ins included), without the disk-encryption probe or the credential
/// folder: `release install` only moves services, and neither of those stops it.
pub(crate) fn service_manager() -> Result<Box<dyn org_platform::service::ServiceManager>, String> {
    #[cfg(all(debug_assertions, feature = "test-support"))]
    if let Some(service) = development_service()? {
        return Ok(service);
    }
    #[cfg(not(all(debug_assertions, feature = "test-support")))]
    refuse_development_service()?;
    setup_flow::service_manager(false)
}

/// A build without the development adapters refuses the variables that name one.
#[cfg(not(all(debug_assertions, feature = "test-support")))]
fn refuse_development_service() -> Result<(), String> {
    for variable in [DEV_SERVICE_DIR, DEV_SERVICE_PROGRAMS] {
        if std::env::var_os(variable).is_some_and(|d| !d.is_empty()) {
            return Err(format!(
                "{variable} names a development test service adapter, which this build does \
                 not contain; unset it to use the system service manager; nothing was changed"
            ));
        }
    }
    Ok(())
}

/// The service is a detached child, never registered with the system (`ChildServiceManager`).
#[cfg_attr(
    all(debug_assertions, feature = "test-support"),
    allow(
        dead_code,
        reason = "named in the refusal of a build without the adapters"
    )
)]
const DEV_SERVICE_DIR: &str = "ORG_DEV_SERVICE_DIR";
/// The system's own service adapter, with the programs it runs taken from this folder
/// instead of the system's (on Linux, stand-ins for `systemctl` and `loginctl`).
#[cfg_attr(
    all(debug_assertions, feature = "test-support"),
    allow(
        dead_code,
        reason = "named in the refusal of a build without the adapters"
    )
)]
const DEV_SERVICE_PROGRAMS: &str = "ORG_DEV_SERVICE_PROGRAMS";

/// The development service adapter the environment asks for, if it asks for one.
#[cfg(all(debug_assertions, feature = "test-support"))]
fn development_service() -> Result<Option<Box<dyn org_platform::service::ServiceManager>>, String> {
    use org_platform::service::{testing::registered_with_programs, Placement};
    let named = |variable: &str| std::env::var_os(variable).filter(|d| !d.is_empty());
    if let Some(dir) = named(DEV_SERVICE_DIR) {
        return Ok(Some(Box::new(
            org_platform::testing::ChildServiceManager::new(dir),
        )));
    }
    let Some(programs) = named(DEV_SERVICE_PROGRAMS) else {
        return Ok(None);
    };
    let home = named("HOME").ok_or("HOME is not set")?;
    let refused = |e: org_platform::Refused| e.to_string();
    let placement = Placement::user(std::path::Path::new(&home)).map_err(refused)?;
    registered_with_programs(
        placement,
        std::path::Path::new(&programs),
        named("XDG_RUNTIME_DIR").map(Into::into),
    )
    .map(Some)
    .map_err(refused)
}

/// `--option value` pairs, each at most once, from `allowed` only: the grammar `setup` keeps.
pub(crate) fn options(
    arguments: &[String],
    allowed: &[&str],
) -> Result<BTreeMap<String, String>, String> {
    let mut options = BTreeMap::new();
    let mut arguments = arguments.iter();
    while let Some(flag) = arguments.next() {
        if !allowed.contains(&flag.as_str()) {
            return Err(format!("unrecognized argument {flag:?}"));
        }
        let value = arguments
            .next()
            .filter(|v| !v.starts_with("--"))
            .ok_or_else(|| format!("{flag} needs a value"))?;
        if options.insert(flag.clone(), value.clone()).is_some() {
            return Err(format!("{flag} is given more than once"));
        }
    }
    Ok(options)
}

/// Standard output for a host operation that reports in prose as it works (setup, the
/// service commands). A reader that goes away (`| head`) does not fail the work: the rest of
/// the report is dropped, the work completes, and the run then ends as a closed output does
/// (exit 0). Any other write error is the host's to report.
pub(crate) struct ReportOut<W: Write> {
    inner: W,
    closed: bool,
}

impl<W: Write> ReportOut<W> {
    pub(crate) fn new(inner: W) -> Self {
        Self {
            inner,
            closed: false,
        }
    }

    /// Whether the reader went away while the work ran.
    pub(crate) fn closed(&self) -> bool {
        self.closed
    }
}

impl<W: Write> Write for ReportOut<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.closed {
            return Ok(buf.len());
        }
        match self.inner.write(buf) {
            Err(error) if error.kind() == io::ErrorKind::BrokenPipe => {
                self.closed = true;
                Ok(buf.len())
            }
            other => other,
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.closed {
            return Ok(());
        }
        match self.inner.flush() {
            Err(error) if error.kind() == io::ErrorKind::BrokenPipe => {
                self.closed = true;
                Ok(())
            }
            other => other,
        }
    }
}

/// The CLI's configuration and cache directories ([`org_cli_shell::paths::resolve_dirs`]):
/// `AGENTIC_ORG_CONFIG_DIR` / `AGENTIC_ORG_CACHE_DIR`, an absolute `XDG_CONFIG_HOME` /
/// `XDG_CACHE_HOME`, or this account's own folders (`org_platform::dirs`: the application
/// base's `cli`, and [`org_platform::dirs::cli_cache_dir`], never a shared temporary
/// folder). A relative or otherwise unusable variable is [`ShellError::InvalidConfig`]
/// (exit 1), never ignored. `Ok(None)` only when there is no home and neither
/// `AGENTIC_ORG_*` variable is set: there is then nowhere to keep a cache.
pub(crate) fn cli_dirs() -> Result<Option<org_cli_shell::paths::ShellDirs>, ShellError> {
    use org_cli_shell::paths::{resolve_dirs, CACHE_DIR_ENV, CONFIG_DIR_ENV};
    let env = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
    let home = env("HOME").map(std::path::PathBuf::from);
    let named = [CONFIG_DIR_ENV, CACHE_DIR_ENV]
        .iter()
        .any(|name| env(name).is_some());
    if home.is_none() && !named {
        return Ok(None);
    }
    let config = || {
        let home = home.as_deref()?;
        org_platform::dirs::application_base(home)
            .ok()
            .map(|base| base.join("cli"))
    };
    let cache = || org_platform::dirs::cli_cache_dir(home.as_deref()?).ok();
    resolve_dirs(&env, &config, &cache).map(Some)
}

/// The line a person reads for a service report (also in a pipe: it is a tested contract):
/// `running (pid 42)`, `stopped`, `registered, not loaded`, `not registered`;
/// `Service for "lab" started.`.
pub(crate) fn service_summary(report: &ServiceReport) -> String {
    match (report.state, report.foreground) {
        (ServiceState::Running, true) => {
            "running in the foreground (no service is registered)".to_owned()
        }
        (ServiceState::Stopped, true) => {
            "not running (foreground mode: it runs while agentic-org serve --setup <file> runs)"
                .to_owned()
        }
        (ServiceState::Running, false) => match report.pid {
            Some(pid) => format!("running (pid {pid})"),
            None => "running".to_owned(),
        },
        (ServiceState::Stopped, false) => "stopped".to_owned(),
        (ServiceState::Unloaded, _) => "registered, not loaded".to_owned(),
        (ServiceState::NotRegistered, _) => "not registered".to_owned(),
        (ServiceState::Started, _) => format!("Service for \"{}\" started.", report.instance),
        (ServiceState::StoppedByRequest, _) => {
            format!("Service for \"{}\" stopped.", report.instance)
        }
    }
}

/// The instance's service, as `status` shows it.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct ServiceView {
    /// `running`, `stopped`, `unloaded` or `not_registered`; `foreground` is set for an
    /// instance that runs in the foreground mode.
    pub(crate) state: String,
    pub(crate) foreground: bool,
    pub(crate) pid: Option<u32>,
    /// One line for a person: `running (pid 42)`, `stopped`, ...
    pub(crate) summary: String,
}

impl ServiceView {
    pub(crate) fn style(&self) -> org_cli_shell::output::Style {
        crate::view::status_style(&self.state)
    }
}

/// The service of `instance`, or `None` when it cannot be read (no such instance, no
/// service manager): `status` reports what it can and never fails on it.
pub(crate) fn service_view(instance: Option<&str>) -> Option<ServiceView> {
    let platform = platform(false).ok()?;
    let report = setup_flow::service(
        instance,
        &*platform.service,
        setup_flow::ServiceAction::Status,
    )
    .ok()?;
    Some(ServiceView {
        state: setup_flow::wire_name(&report.state),
        foreground: report.foreground,
        pid: report.pid,
        summary: service_summary(&report),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Closed;

    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
    }

    #[test]
    fn a_closed_report_does_not_fail_the_work_and_is_remembered() {
        let mut out = ReportOut::new(Closed);
        assert!(writeln!(out, "Setting up instance").is_ok());
        assert!(out.flush().is_ok());
        assert!(out.closed());
        let mut open = ReportOut::new(Vec::new());
        writeln!(open, "ok").unwrap();
        assert!(!open.closed());
        assert_eq!(open.inner, b"ok\n");
    }

    #[test]
    fn options_are_named_once_and_take_a_value() {
        let args = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert!(options(&args(&["--name", "a"]), &["--name"]).is_ok());
        assert!(options(&args(&["--name"]), &["--name"]).is_err());
        assert!(options(&args(&["--name", "a", "--name", "b"]), &["--name"]).is_err());
        assert!(options(&args(&["--port", "1"]), &["--name"]).is_err());
    }
}
