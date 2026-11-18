//! `agentic-org setup [--name <instance>] [--port <port>]`: composes the platform adapters and
//! runs [`org_host::setup_flow`]. Satellite flags are refused before anything else.
//!
//! Development builds with the `test-support` feature compose test adapters instead of launchd
//! when `ORG_DEV_SERVICE_DIR` names a folder: the service runs as a detached child
//! (`ChildServiceManager`), and `ORG_DEV_DISK_ENCRYPTION` (`on`, `off`, anything else unknown)
//! is the disk-encryption answer. Release builds never contain them, and a build without the
//! adapters refuses `ORG_DEV_SERVICE_DIR` instead of registering a real service.
use std::ffi::OsString;
use std::io::{BufRead, IsTerminal, Write};

use org_host::setup_flow::{self, Platform, Request};

pub(crate) const HELP: &str = "Set up this Mac's instance and register its service:\n  \
agentic-org setup [--name <instance>] [--port <port>]\nWithout --name the instance is \
\"default\" on port 7420; a named instance takes the first free port from 7421.";

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
    let options = crate::options(arguments, &["--name", "--port"])?;
    let port = options
        .get("--port")
        .map(|p| {
            p.parse::<u16>()
                .ok()
                .filter(|p| *p != 0)
                .ok_or_else(|| format!("--port must be a number from 1 to 65535, not {p:?}"))
        })
        .transpose()?;
    let platform = platform()?;
    let program = std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .map_err(|e| format!("locating agentic-org: {e}"))?;
    let mut ask = ask_port;
    let terminal = std::io::stdout().is_terminal();
    let interactive = std::io::stdin().is_terminal() && terminal;
    let request = Request {
        name: options.get("--name").cloned(),
        port,
        prompt: if interactive { Some(&mut ask) } else { None },
        program,
        env: service_environment(),
        terminal,
    };
    let mut out = std::io::stdout().lock();
    setup_flow::run(&platform, request, &mut out)
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

/// The service's explicit environment. Development builds pass on their development settings
/// so the service finds the same instance and development key folder.
fn service_environment() -> Vec<(OsString, OsString)> {
    let mut env = Vec::new();
    if let Some(home) = std::env::var_os("HOME").filter(|h| !h.is_empty()) {
        env.push(("HOME".into(), home));
    }
    if cfg!(debug_assertions) {
        for key in ["ORG_DEV_ACCOUNT_BASE", "ORG_OFFICE_DEV_KEY_DIR"] {
            if let Some(value) = std::env::var_os(key).filter(|v| !v.is_empty()) {
                env.push((key.into(), value));
            }
        }
    }
    env
}

pub(crate) fn platform() -> Result<Platform, String> {
    #[cfg(all(debug_assertions, feature = "test-support"))]
    if let Some(dir) = std::env::var_os("ORG_DEV_SERVICE_DIR").filter(|d| !d.is_empty()) {
        use org_platform::disk::EncryptionState;
        use org_platform::testing::{ChildServiceManager, FakeDiskEncryption};
        let state = match std::env::var("ORG_DEV_DISK_ENCRYPTION").as_deref() {
            Ok("on") => EncryptionState::On,
            Ok("off") => EncryptionState::Off,
            _ => EncryptionState::Unknown("development test adapter".into()),
        };
        return Ok(Platform {
            service: Box::new(ChildServiceManager::new(dir)),
            disk: Box::new(FakeDiskEncryption(state)),
            // A development build keeps its secrets in its development key folder.
            credentials: setup_flow::CredentialDirectory::Unused,
        });
    }
    #[cfg(not(all(debug_assertions, feature = "test-support")))]
    if std::env::var_os("ORG_DEV_SERVICE_DIR").is_some_and(|d| !d.is_empty()) {
        return Err(
            "ORG_DEV_SERVICE_DIR names the development test service adapter, which \
                    this build does not contain; unset it to use the system service manager; \
                    nothing was changed"
                .into(),
        );
    }
    setup_flow::platform()
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
