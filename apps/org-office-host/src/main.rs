//! # org-office-host: the login service
//!
//! A thin binary: reads its arguments, finds the office home, starts the [`Host`] and stops it
//! on SIGTERM or SIGINT. All logic lives in the library.
//!
//! ```text
//! org-office-host [--home <absolute path>] [--server-bin <path>]
//! ```
//! `--home` overrides the office home (otherwise `org_proto::location`'s rule) and
//! `--server-bin` the office server program (otherwise `org-office-server` next to this one);
//! both are for development and tests.

use std::path::PathBuf;
use std::process::ExitCode;

use org_office_host::signals::ShutdownSignals;
use org_office_host::{default_server_program, Config, Host};

const USAGE: &str = "usage: org-office-host [--home <absolute path>] [--server-bin <path>]";

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
    let (home, server_bin) = parse_args(std::env::args().skip(1))?;
    let home = match home {
        Some(home) => home,
        None => org_proto::location::default_office_home()
            .ok_or("no office home: neither ORG_OFFICE_HOME nor HOME is an absolute path")?,
    };
    let server_bin = match server_bin {
        // Servers run with `/` as their working directory, so a relative path must be
        // resolved here, against ours.
        Some(path) => {
            std::path::absolute(&path).map_err(|e| format!("resolving {}: {e}", path.display()))?
        }
        None => default_server_program().map_err(|e| format!("finding the office server: {e}"))?,
    };
    let host = Host::start(Config::new(home, server_bin)).map_err(|e| e.to_string())?;
    let waited = signals.wait();
    host.shutdown();
    waited
        .map(drop)
        .map_err(|e| format!("waiting for a signal: {e}"))
}

fn parse_args(
    mut args: impl Iterator<Item = String>,
) -> Result<(Option<PathBuf>, Option<PathBuf>), String> {
    let (mut home, mut server_bin) = (None, None);
    while let Some(arg) = args.next() {
        let slot = match arg.as_str() {
            "--home" => &mut home,
            "--server-bin" => &mut server_bin,
            "--version" => {
                println!("org-office-host {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            _ => return Err(format!("unexpected argument {arg:?}\n{USAGE}")),
        };
        let value = args
            .next()
            .ok_or_else(|| format!("{arg} needs a value\n{USAGE}"))?;
        *slot = Some(PathBuf::from(value));
    }
    Ok((home, server_bin))
}
