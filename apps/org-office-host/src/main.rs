//! # org-office-host: the login service
//!
//! A thin binary: reads its arguments, finds the office home, starts the [`Host`] and stops it
//! on SIGTERM or SIGINT. All logic lives in the library.
//!
//! ```text
//! org-office-host [--home <absolute path>] [--server-bin <path>] [--app-team-id <team id>]
//! ```
//! `--home` overrides the office home (otherwise `org_proto::location`'s rule) and
//! `--server-bin` the office server program (otherwise `org-office-server` next to this one);
//! both are for development and tests. `--app-team-id` is the team id that signs the desktop
//! app: without it, `office.app_token` and `office.restart` refuse every caller.
//! A debug build may accept one unsigned development app for those calls: its path is the
//! build-time constant `ORG_DEV_APP_PATH` (set when the host is compiled), never read from the
//! command line, a config file or the environment at run time. Release builds have no such path.

use std::path::PathBuf;
use std::process::ExitCode;

use org_office_host::signals::ShutdownSignals;
use org_office_host::{default_server_program, Config, Host};

const USAGE: &str = "usage: org-office-host [--home <absolute path>] [--server-bin <path>] \
                     [--app-team-id <team id>]";

/// What the command line sets.
#[derive(Default)]
struct Args {
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
    let args = parse_args(std::env::args().skip(1))?;
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
        None => default_server_program().map_err(|e| format!("finding the office server: {e}"))?,
    };
    let mut config = Config::new(home, server_bin);
    config.app_team_id = args.app_team_id;
    // Debug builds only: the development app, fixed when this binary was compiled.
    #[cfg(debug_assertions)]
    {
        config.dev_app_path = option_env!("ORG_DEV_APP_PATH").map(PathBuf::from);
    }
    let host = Host::start(config).map_err(|e| e.to_string())?;
    let waited = signals.wait();
    host.shutdown();
    waited
        .map(drop)
        .map_err(|e| format!("waiting for a signal: {e}"))
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut parsed = Args::default();
    while let Some(arg) = args.next() {
        if arg == "--version" {
            println!("org-office-host {}", env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        }
        let known = ["--home", "--server-bin", "--app-team-id"];
        if !known.contains(&arg.as_str()) {
            return Err(format!("unexpected argument {arg:?}\n{USAGE}"));
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
