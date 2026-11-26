//! A backup passphrase from `--passphrase-file`, `--passphrase-fd` or the controlling
//! terminal (echo off), never from the command line or the environment.

use org_backup::Passphrase;
use org_cli_shell::context::ExecutionContext;
use org_cli_shell::error::ShellError;
use org_host::human::SecretEntryError;

use super::{cli_error, PassphraseArgs};
use crate::error::CliError;

/// Whether the passphrase seals a new archive (asked twice) or opens one.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Use {
    Seal,
    Open,
}

fn from_terminal(label: &str) -> Result<Passphrase, CliError> {
    let text = org_host::human::terminal_secret(label).map_err(|error| match error {
        // Ctrl+C or end of input at the prompt is a cancellation (exit 130), not a misuse.
        SecretEntryError::Cancelled => CliError::from(ShellError::Cancelled),
        SecretEntryError::Unavailable(why) => {
            CliError::usage(format!("the passphrase could not be read: {why}"))
                .with_help("give --passphrase-file <path> or --passphrase-fd <fd>")
        }
    })?;
    Passphrase::new(text.as_bytes().to_vec()).map_err(cli_error)
}

/// The passphrase `args` names, or one asked on the terminal when someone can be asked:
/// never under `--json`, `--quiet` or `--no-input`, or without a terminal, where it fails at
/// once (exit 2) instead of waiting on `/dev/tty`.
pub(super) fn read(
    args: &PassphraseArgs,
    ctx: &ExecutionContext,
    purpose: Use,
) -> Result<Passphrase, CliError> {
    if let Some(path) = &args.passphrase_file {
        return Passphrase::from_file(path).map_err(cli_error);
    }
    if let Some(fd) = args.passphrase_fd {
        return Passphrase::from_fd(fd).map_err(cli_error);
    }
    if !ctx.can_prompt() {
        let why = if ctx.modes().no_input {
            "--no-input is on"
        } else {
            "no one can be asked here (--json, --quiet, or no terminal)"
        };
        return Err(CliError::usage(format!("a passphrase is needed and {why}"))
            .with_help("give --passphrase-file <path> or --passphrase-fd <fd>"));
    }
    match purpose {
        Use::Open => from_terminal("Backup passphrase: "),
        Use::Seal => {
            let first = from_terminal("New backup passphrase: ")?;
            let again = from_terminal("Repeat the passphrase: ")?;
            if !first.same_as(&again) {
                return Err(CliError::usage("the two passphrases differ"));
            }
            Ok(first)
        }
    }
}
