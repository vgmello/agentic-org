//! `agentic-org setup [--name <instance>] [--port <port>]`: composes the platform adapters and
//! runs [`org_host::setup_flow`]. Satellite flags are refused before anything else.
//!
//! `agentic-org setup --non-interactive --config <file> [--name <instance>]` takes the whole
//! setup input from a strict JSON file ([`setup_flow::configuration`] documents it), asks
//! nothing, fetches nothing and prints no link. With `"service": "foreground"` in the file
//! nothing is registered or started: `agentic-org serve --setup <file>` runs that setup
//! when the instance is not set up yet and then serves it ([`configured`]).
//!
//! Development builds with the `test-support` feature compose test adapters instead of the
//! system's service manager when `ORG_DEV_SERVICE_DIR` names a folder: the service runs as a
//! detached child (`ChildServiceManager`), and `ORG_DEV_DISK_ENCRYPTION` (`on`, `off`, anything
//! else unknown) is the disk-encryption answer. `ORG_DEV_SERVICE_PROGRAMS` composes the
//! system's own service adapter instead, with the programs it runs taken from the folder the
//! variable names (on Linux, stand-ins for `systemctl` and `loginctl`), so the real adapter is
//! exercised without systemd. Release builds never contain either, and a build without the
//! adapters refuses both variables instead of registering a real service.
use std::ffi::OsString;
use std::io::{BufRead, IsTerminal, Write};

use org_host::setup_flow::{self, Platform, Request};

pub(crate) const HELP: &str = "Set up this computer's instance and register its service:\n  \
agentic-org setup [--name <instance>] [--port <port>]\nWithout --name the instance is \
\"default\" on port 7420; a named instance takes the first free port from 7421.\n\
Without a terminal, from a file (service mode, data folder, credential directory, \
address):\n  agentic-org setup --non-interactive --config <file> [--name <instance>]";

pub(crate) fn run(arguments: &[String]) -> Result<(), String> {
    run_as(arguments, org_host::Config::refuse_root)
}

/// [`run`], with the check of who may own an instance and its service given by the caller
/// (the composition passes the host's root refusal; a test passes its own).
fn run_as(
    arguments: &[String],
    may_run_the_service: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    // Never write a fake HQ or enroll a satellite from here.
    if arguments.iter().any(|arg| {
        matches!(arg.as_str(), "--hq" | "--satellite" | "--invitation")
            || arg.starts_with("--hq=")
            || arg.starts_with("--invitation=")
    }) {
        return Err("setup creates HQ only; use HQ's Add office flow for satellites".into());
    }
    if matches!(arguments, [flag] if flag == "--help" || flag == "-h") {
        println!("{HELP}");
        return Ok(());
    }
    // Setup reserves the instance, makes its custody and registers a service that runs as
    // the user who ran it, and the service refuses root (ADR-0017 decision 10). So root is
    // refused here, before anything is read or made, with the service's own words. Nothing
    // above reads more than the arguments. (`admin reset` run by root for the service user
    // is another command and is not judged here.)
    may_run_the_service()?;
    let non_interactive = arguments.iter().any(|arg| arg == "--non-interactive");
    let arguments: Vec<String> = arguments
        .iter()
        .filter(|arg| *arg != "--non-interactive")
        .cloned()
        .collect();
    let options = crate::options(&arguments, &["--name", "--port", "--config"])?;
    match (non_interactive, options.get("--config")) {
        (true, Some(file)) => {
            if options.contains_key("--port") {
                return Err("--port is not used with --config: the address is in the file".into());
            }
            return configured(
                std::path::Path::new(file),
                options.get("--name").map(String::as_str),
            );
        }
        (true, None) => return Err("--non-interactive needs --config <file>".into()),
        (false, Some(_)) => {
            return Err("--config is read only with --non-interactive".into());
        }
        (false, None) => {}
    }
    let port = options
        .get("--port")
        .map(|p| {
            p.parse::<u16>()
                .ok()
                .filter(|p| *p != 0)
                .ok_or_else(|| format!("--port must be a number from 1 to 65535, not {p:?}"))
        })
        .transpose()?;
    let platform = platform(false)?;
    let program = own_program()?;
    let mut ask = ask_port;
    let terminal = std::io::stdout().is_terminal();
    let interactive = std::io::stdin().is_terminal() && terminal;
    let request = Request {
        name: options.get("--name").cloned(),
        port,
        prompt: if interactive { Some(&mut ask) } else { None },
        named_port_base: named_port_base()?,
        program,
        env: service_environment()?,
        terminal,
        development_keys: development_keys()?,
        configuration: None,
    };
    let mut out = std::io::stdout().lock();
    setup_flow::run(&platform, request, &mut out).map(drop)
}

fn own_program() -> Result<std::path::PathBuf, String> {
    std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .map_err(|e| format!("locating agentic-org: {e}"))
}

/// Non-interactive, offline setup of instance `name` from the configuration at `file`:
/// nothing is asked, nothing is fetched and no link is printed. It is idempotent: a later
/// run with the same file finds the instance set up and changes nothing, and a file that
/// differs from what the instance was set up with is refused. The caller has already
/// refused a user who may not own the instance.
pub(crate) fn configured(file: &std::path::Path, name: Option<&str>) -> Result<(), String> {
    apply(setup_flow::configuration(file)?, name).map(drop)
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
) -> Result<Option<org_host::BrowserListener>, String> {
    let configuration = setup_flow::configuration(file)?;
    if !configuration.foreground() {
        return Err(format!(
            "{} sets up a registered service, which its service manager runs; serve --setup \
             is the foreground mode and needs \"service\": \"foreground\"; nothing was changed",
            file.display()
        ));
    }
    apply(configuration, name)
}

fn apply(
    configuration: setup_flow::Configuration,
    name: Option<&str>,
) -> Result<Option<org_host::BrowserListener>, String> {
    let platform = platform(configuration.foreground())?;
    let request = Request {
        name: name.map(str::to_owned),
        port: None,
        prompt: None,
        named_port_base: named_port_base()?,
        program: own_program()?,
        env: service_environment()?,
        terminal: false,
        development_keys: development_keys()?,
        configuration: Some(configuration),
    };
    let mut out = std::io::stdout().lock();
    setup_flow::run(&platform, request, &mut out)
}

/// Where a named instance's port search starts: 7421 in every build that people run. A
/// debug build with `test-support` reads `ORG_DEV_NAMED_PORT_BASE` so its tests search a
/// range of their own instead of the one real instances and other test runs use.
fn named_port_base() -> Result<u16, String> {
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

fn ask_port(address: &str) -> Result<Option<u16>, String> {
    print!("Address: {address}  (Enter to accept, or type a port) ");
    std::io::stdout().flush().map_err(|e| e.to_string())?;
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    match line.trim() {
        "" => Ok(None),
        port => port
            .parse::<u16>()
            .ok()
            .filter(|p| *p != 0)
            .map(Some)
            .ok_or_else(|| format!("{port:?} is not a port from 1 to 65535; nothing was saved")),
    }
}

/// The service's explicit environment: the home, and the variables that decided this
/// instance's locations when setup ran, so that the service resolves the same paths
/// (`setup_flow::path_environment`). Development builds pass on their development settings
/// so the service finds the same instance and development key folder, which must be an
/// absolute path ([`development_keys`]).
fn service_environment() -> Result<Vec<(OsString, OsString)>, String> {
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
fn development_keys() -> Result<Option<std::path::PathBuf>, String> {
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
    for variable in [DEV_SERVICE_DIR, DEV_SERVICE_PROGRAMS] {
        if std::env::var_os(variable).is_some_and(|d| !d.is_empty()) {
            return Err(format!(
                "{variable} names a development test service adapter, which this build does \
                 not contain; unset it to use the system service manager; nothing was changed"
            ));
        }
    }
    setup_flow::platform(false)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    /// A user who may not own a service is refused before setup reads or makes anything:
    /// before its options are even parsed, so no instance is reserved, no custody is made
    /// and no service is registered. Help and the satellite refusal read nothing and are
    /// answered to anyone.
    /// X5 review V-d: the help says what setup does on every system it runs on.
    #[test]
    fn the_help_names_no_one_system() {
        for help in [HELP, crate::HELP] {
            assert!(!help.contains("Mac"), "{help}");
        }
        assert!(HELP.starts_with("Set up this computer's instance"));
    }

    #[test]
    fn a_user_who_may_not_run_the_service_is_refused_before_setup_reads_anything() {
        let refusal = || Err("the service does not run as this user".to_owned());
        for list in [
            vec![],
            vec!["--name", "lab"],
            vec!["--port", "7420"],
            // Refused as the user, not as a bad option: nothing was parsed yet.
            vec!["--frobnicate"],
            vec!["--port", "not-a-port"],
        ] {
            assert_eq!(
                run_as(&args(&list), refusal),
                Err("the service does not run as this user".into()),
                "{list:?}"
            );
        }
        let unasked = || -> Result<(), String> { panic!("asked who runs a help query") };
        assert_eq!(run_as(&args(&["--help"]), unasked), Ok(()));
        let unasked = || -> Result<(), String> { panic!("asked before the satellite refusal") };
        assert!(run_as(&args(&["--satellite"]), unasked)
            .unwrap_err()
            .contains("setup creates HQ only"));
        // The composition's own check is the host's rule for this process's user.
        // SAFETY: no arguments and no side effects; it cannot fail.
        assert_eq!(
            org_host::Config::refuse_root().is_err(),
            unsafe { libc::geteuid() } == 0
        );
    }
}
