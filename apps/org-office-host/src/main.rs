//! # org-office-host: the login service
//!
//! A thin binary: reads its arguments, finds the office home, starts the [`Host`] and stops it
//! on SIGTERM or SIGINT. All logic lives in the library.
//!
//! ```text
//! org-office-host [--version]
//! org-office-host [--home <absolute path>] [--server-bin <path>] [--app-team-id <team id>]   (debug builds)
//! ```
//! **A release build trusts only what it was built with** ([`Config::trusted`]): the app's
//! bundle id and team id (`ORG_APP_TEAM_ID` at build time; without it no signed app can use
//! `office.app_token` or `office.restart`, logged loudly), its Keychain access group
//! (`ORG_HOST_KEYCHAIN_GROUP` at build time; without it the host doesn't start), and the office
//! server next to its own executable. It refuses `--home`, `--server-bin` and `--app-team-id`.
//!
//! A debug build takes them for development and tests: `--home` overrides the office home
//! (otherwise `org_proto::location`'s rule), `--server-bin` the office server program (otherwise
//! `org-office-server` next to this one), and `--app-team-id` the team id it was built with.
//! It may also accept one unsigned development app for the app-only calls: the path of its
//! executable (`Contents/MacOS/<exe>`) is the build-time constant `ORG_DEV_APP_PATH` (set when
//! the host is compiled), never read from the command line, a config file or the environment at
//! run time. If it doesn't exist when the host starts, the fallback is off (logged).
//!
//! A debug build keeps the offices' database keys as files in the folder `ORG_OFFICE_DEV_KEY_DIR`
//! names, never in the Keychain, and refuses to start without it. Release builds ignore the
//! variable. The host never passes the variable on to its servers.

use std::path::PathBuf;
use std::process::ExitCode;

use org_office_host::signals::ShutdownSignals;
use org_office_host::{Config, Host};

const USAGE: &str = "usage: org-office-host [--version] | (debug builds only) [--home <absolute \
                     path>] [--server-bin <path>] [--app-team-id <team id>]";

/// Which kind of build this is: a release build refuses the arguments that change what it
/// trusts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Build {
    Debug,
    Release,
}

impl Build {
    const CURRENT: Build = if cfg!(debug_assertions) {
        Build::Debug
    } else {
        Build::Release
    };
}

/// What the command line asks for.
#[derive(Debug, Default, PartialEq, Eq)]
// A release build reads only `version`: it refuses the rest.
#[cfg_attr(not(debug_assertions), allow(dead_code))]
struct Args {
    version: bool,
    home: Option<PathBuf>,
    server_bin: Option<PathBuf>,
    app_team_id: Option<String>,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("org-office-host: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    // First, before any thread exists, so every thread inherits the mask.
    let signals = ShutdownSignals::block().map_err(|e| format!("blocking signals: {e}"))?;
    let args = parse_args(std::env::args().skip(1), Build::CURRENT)?;
    if args.version {
        println!("org-office-host {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let host = Host::start(config(args)?).map_err(|e| e.to_string())?;
    let waited = signals.wait();
    host.shutdown();
    waited
        .map(drop)
        .map_err(|e| format!("waiting for a signal: {e}"))
}

/// A release build's config: only what it was built with.
#[cfg(not(debug_assertions))]
fn config(_args: Args) -> Result<Config, String> {
    Config::trusted()
}

/// A debug build's config: what it was built with, with the arguments' overrides and the
/// development settings.
#[cfg(debug_assertions)]
fn config(args: Args) -> Result<Config, String> {
    let home = match args.home {
        Some(home) => home,
        None => org_proto::location::default_office_home()
            .ok_or("no office home: neither ORG_OFFICE_HOME nor HOME is an absolute path")?,
    };
    let server_bin = match args.server_bin {
        // Servers run with `/` as their working directory, so a relative path must be
        // resolved here, against ours.
        Some(path) => {
            std::path::absolute(&path).map_err(|e| format!("resolving {}: {e}", path.display()))?
        }
        None => org_office_host::default_server_program()
            .map_err(|e| format!("finding the office server: {e}"))?,
    };
    let mut config = Config::new(home, server_bin);
    config.set_app_team_id(
        args.app_team_id
            .or(org_office_host::APP_TEAM_ID.map(str::to_owned)),
    );
    // The development app, fixed when this binary was compiled.
    config.dev_app_path = option_env!("ORG_DEV_APP_PATH").map(PathBuf::from);
    // The development key folder, resolved against our working directory.
    config.dev_key_dir = match std::env::var_os("ORG_OFFICE_DEV_KEY_DIR") {
        Some(dir) if !dir.is_empty() => Some(
            std::path::absolute(&dir)
                .map_err(|e| format!("resolving ORG_OFFICE_DEV_KEY_DIR: {e}"))?,
        ),
        _ => None,
    };
    Ok(config)
}

/// Parses the arguments (without the program name) for a `build`. Pure: it reads nothing else
/// and exits nothing. A release build refuses `--home`, `--server-bin` and `--app-team-id`.
fn parse_args(args: impl IntoIterator<Item = String>, build: Build) -> Result<Args, String> {
    let mut parsed = Args::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--version" {
            parsed.version = true;
            continue;
        }
        let known = ["--home", "--server-bin", "--app-team-id"];
        if !known.contains(&arg.as_str()) {
            return Err(format!("unexpected argument {arg:?}\n{USAGE}"));
        }
        if build == Build::Release {
            return Err(format!(
                "{arg} isn't accepted by a release build: it uses only the office home, office \
                 server and app team id it was built with"
            ));
        }
        let value = args
            .next()
            .ok_or_else(|| format!("{arg} needs a value\n{USAGE}"))?;
        match arg.as_str() {
            "--home" => parsed.home = Some(PathBuf::from(value)),
            "--server-bin" => parsed.server_bin = Some(PathBuf::from(value)),
            _ => parsed.app_team_id = Some(value),
        }
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn a_debug_build_takes_the_development_arguments() {
        let parsed = parse_args(
            args(&[
                "--home",
                "/tmp/h",
                "--server-bin",
                "srv",
                "--app-team-id",
                "AB12CD34EF",
            ]),
            Build::Debug,
        )
        .unwrap();
        assert_eq!(
            parsed,
            Args {
                version: false,
                home: Some("/tmp/h".into()),
                server_bin: Some("srv".into()),
                app_team_id: Some("AB12CD34EF".into()),
            }
        );
        assert_eq!(
            parse_args(args(&[]), Build::Debug).unwrap(),
            Args::default()
        );
    }

    #[test]
    fn a_release_build_refuses_every_argument_that_changes_what_it_trusts() {
        for flag in ["--home", "--server-bin", "--app-team-id"] {
            for list in [
                vec![flag, "/tmp/x"],
                vec![flag],
                vec!["--version", flag, "x"],
            ] {
                let err = parse_args(args(&list), Build::Release).unwrap_err();
                assert!(err.contains(flag), "{list:?}: {err}");
                assert!(err.contains("release build"), "{list:?}: {err}");
            }
        }
        assert_eq!(
            parse_args(args(&[]), Build::Release).unwrap(),
            Args::default()
        );
    }

    #[test]
    fn version_works_in_both_builds_and_unknown_or_incomplete_arguments_are_refused() {
        for build in [Build::Debug, Build::Release] {
            assert!(parse_args(args(&["--version"]), build).unwrap().version);
            let err = parse_args(args(&["--frobnicate"]), build).unwrap_err();
            assert!(err.contains("unexpected argument"), "{err}");
        }
        let err = parse_args(args(&["--home"]), Build::Debug).unwrap_err();
        assert!(err.contains("needs a value"), "{err}");
    }
}
