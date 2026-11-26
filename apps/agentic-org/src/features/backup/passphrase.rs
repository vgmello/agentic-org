//! A backup passphrase from `--passphrase-file`, `--passphrase-fd` or the controlling
//! terminal (echo off), never from the command line or the environment.

use org_backup::Passphrase;
use org_cli_shell::context::ExecutionContext;

use super::{cli_error, PassphraseArgs};
use crate::error::CliError;

/// Whether the passphrase seals a new archive (asked twice) or opens one.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Use {
    Seal,
    Open,
}

fn from_terminal(label: &str) -> Result<Passphrase, CliError> {
    let text = org_host::human::terminal_secret(label).map_err(|why| {
        CliError::usage(format!("the passphrase could not be read: {why}"))
            .with_help("give --passphrase-file <path> or --passphrase-fd <fd>")
    })?;
    Passphrase::new(text.as_bytes().to_vec()).map_err(cli_error)
}

/// The passphrase `args` names, or one asked on the terminal unless `--no-input` is on.
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
    if ctx.modes().no_input {
        return Err(
            CliError::usage("a passphrase is needed and --no-input is on")
                .with_help("give --passphrase-file <path> or --passphrase-fd <fd>"),
        );
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
