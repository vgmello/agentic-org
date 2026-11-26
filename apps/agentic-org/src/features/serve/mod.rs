//! agentic-org serve: the service host
//!
//! A thin binary: reads its arguments, finds the office home, starts the [`Host`] and stops it
//! on SIGTERM or SIGINT. All logic lives in the library.
//!
//! ```text
//! agentic-org serve [--version]
//! agentic-org serve --name <instance>
//! agentic-org serve --setup <file> [--name <instance>]
//! agentic-org serve [--home <absolute path>] [--server-bin <path>] [--app-team-id <team id>]   (debug builds)
//! ```
//! `--name` serves the registered instance setup reserved under that name ([`Host::start_registered`]):
//! it creates the instance's host registry at its first start and answers the administrator
//! link calls. It is accepted by both builds and excludes `--home`. A release build without a
//! pinned runtime release refuses at runtime admission, before any custody is opened.
//! `--setup` is the foreground mode, a container's command (ADR-0017 decision 10): when the
//! instance is not set up yet it first runs the non-interactive, offline setup of that file
//! (`agentic-org setup --non-interactive --config <file>`), which changes nothing on a later
//! start, and then serves the instance as `--name` does. The file's service mode must be
//! `foreground`: nothing is registered, and this process is the service. It is accepted by
//! both builds and excludes `--home`.
//! **A release build trusts only what it was built with** ([`Config::trusted`]): the app's
//! bundle id and team id (`ORG_APP_TEAM_ID` at build time; without it no signed app can use
//! `office.restart`, logged loudly), its Keychain access group
//! (`ORG_HOST_KEYCHAIN_GROUP` at build time; without it the host doesn't start), and the office
//! server next to its own executable. It refuses `--home`, `--server-bin` and `--app-team-id`.
//!
//! A debug build takes them for development and tests: `--home` overrides the office home
//! (otherwise `org_proto::location`'s rule), `--server-bin` the office server program (otherwise
//! `agentic-org-office` next to this one), and `--app-team-id` the team id it was built with.
//! It may also accept one unsigned development app for the app-only calls: the path of its
//! executable (`Contents/MacOS/<exe>`) is the build-time constant `ORG_DEV_APP_PATH` (set when
//! the host is compiled), never read from the command line, a config file or the environment at
//! run time. If it doesn't exist when the host starts, the fallback is off (logged).
//!
//! `serve` refuses to run as root, on every system and in both builds, before it opens or
//! creates anything ([`Config::refuse_root`], ADR-0017 decision 10): the service account is
//! an ordinary user. The host then closes itself to the other processes of that user before
//! it reads a key (no core dump; on Linux not dumpable).
//!
//! **The stop.** `serve` stops on SIGTERM or SIGINT, within 2.5 s of the signal (Linux host
//! design, section 3.4), from the moment it blocks them, before setup runs: a thread takes
//! the signal as soon as it arrives ([`Stop`]), so a stop that comes while the instance is
//! set up or the host starts is never left waiting for them (X5 review, third round, P1).
//! - A stop that comes before setup has ended: setup runs to its end (it waits for nothing
//!   but the disk), and `serve` exits 0 without starting the host. Setup is idempotent and
//!   what it did is kept: the next `serve --setup` with the same file continues it, and
//!   then serves.
//! - A stop that comes while the host starts: the host stops as soon as its start returns,
//!   with the stop's deadline fixed from the signal ([`Host::shutdown_from`]), and `serve`
//!   exits 0.
//! - A start (setup, or the host's) that has not returned
//!   [`stop_ends_within`](org_host::Timings::stop_ends_within) (2 s) after the signal (a
//!   slow volume, say) ends the process there, with a line that says so, inside the 2.5 s.
//!   Its status is 0 too: the stop was asked for, and a service manager must not take the
//!   exit for a failure (launchd restarts a job that does not exit 0, and systemd marks the
//!   unit failed). What setup or the start completed is kept: setup and every store the host
//!   writes recover from a process that ends at any point, as from the container runtime's
//!   own kill a moment later, and the next start continues. Office servers and the web
//!   process started meanwhile end with it (the end of their input and of their control
//!   connection).
//! - Any stop that has not ended 2.4 s after the signal (the host's last steps waiting on a
//!   slow volume, say) ends the process there, with status 0 and a line that says so. That
//!   includes writing the lines that end it: "stopped before serving" goes through the host
//!   log, which waits for it 100 ms at most, and an error from setup or the start is returned
//!   with the stop still bounded, so a standard error nobody reads cannot hold the process
//!   past the limit (X5 final review).
//! - Waiting for the signal failing (it cannot, in practice) is an error, whenever it is
//!   found: `serve` stops what it started and exits non-zero.
//!
//! A debug build keeps the offices' secrets keys as files in the folder `ORG_OFFICE_DEV_KEY_DIR`
//! names, never in the Keychain, and refuses to start without it. Release builds ignore the
//! variable. The host never passes the variable on to its servers.

mod args;
mod run;
#[cfg(test)]
mod tests;

use org_cli_shell::context::ExecutionContext;

use crate::error::CliError;

/// `agentic-org serve ...`: `serve` reads its own arguments (a release build refuses the
/// development ones), so its words reach it as they were written.
pub(crate) fn run(arguments: Vec<String>, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    if matches!(arguments.as_slice(), [flag] if flag == "--help" || flag == "-h") {
        return Ok(ctx.out(&format!("{HELP}\n"))?);
    }
    let served = run::run(arguments).map_err(|message| match message.split_once('\n') {
        // An argument error: its first line, and the usage as its help (exit 2).
        Some((first, usage)) => CliError::usage(first).with_help(usage),
        None => CliError::from(message),
    })?;
    match served {
        run::Served::Version => {
            Ok(ctx.out(&format!("agentic-org {}\n", env!("CARGO_PKG_VERSION")))?)
        }
        run::Served::Stopped => Ok(()),
    }
}

/// `agentic-org serve --help`.
pub(crate) const HELP: &str = "Run the installed service host: agentic-org serve [--name <instance>]\n\
Use when: running an already-set-up host in the foreground (directly or under a service manager).\n\
In the foreground from a setup file (a container): agentic-org serve --setup <file> [--name <instance>]\n\
Use when: initializing (idempotently) and running a foreground/container instance from one JSON config file.\n\
Development builds also support --home, --server-bin and --app-team-id.";
