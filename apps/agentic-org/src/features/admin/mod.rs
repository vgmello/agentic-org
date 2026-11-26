//! `agentic-org admin reset`: after Touch ID or the login password at the service host, a new
//! administrator registration link; the previous one is cancelled. `agentic-org admin tls
//! set`: replaces an https instance's TLS pair.
//!
//! On Linux the local authority is root (SIGN-33): `sudo agentic-org admin reset`, or, with
//! no `sudo` (a container), `agentic-org admin reset --user <service user>` run as root. Root
//! acts on that user's instance and writes nothing into its folders.

mod reset;
mod tls_set;

use clap::{Args, Subcommand};
use org_cli_shell::context::ExecutionContext;

use crate::error::CliError;

const EXAMPLES: &str = "Examples:
  agentic-org admin reset
  sudo agentic-org admin reset --user agentic
  agentic-org admin tls set --cert ./cert.pem --key ./key.pem --login owner";

#[derive(Args, Debug)]
#[command(after_help = EXAMPLES)]
pub(crate) struct AdminArgs {
    #[command(subcommand)]
    command: AdminCommand,
}

#[derive(Subcommand, Debug)]
enum AdminCommand {
    /// Issue a new one-time administrator registration link after local confirmation
    /// Use when: the administrator lost access, or the first link expired (cancels any previous unused link).
    #[command(verbatim_doc_comment, after_help = reset::EXAMPLES, long_about = reset::LONG_ABOUT)]
    Reset(reset::ResetArgs),
    /// HTTPS certificate management
    /// Use when: rotating the instance's HTTPS certificate pair.
    #[command(verbatim_doc_comment)]
    Tls(TlsArgs),
}

#[derive(Args, Debug)]
struct TlsArgs {
    #[command(subcommand)]
    command: TlsCommand,
}

#[derive(Subcommand, Debug)]
enum TlsCommand {
    /// Replace an https instance's TLS certificate and key
    /// Use when: rotating an HTTPS certificate pair before it expires or after a compromise.
    #[command(verbatim_doc_comment, after_help = tls_set::EXAMPLES)]
    Set(tls_set::SetArgs),
}

impl AdminArgs {
    pub(crate) fn yes(&self) -> bool {
        match &self.command {
            AdminCommand::Reset(args) => args.yes.yes,
            AdminCommand::Tls(TlsArgs {
                command: TlsCommand::Set(args),
            }) => args.yes.yes,
        }
    }

    /// `admin tls set` signs in with a password; `admin reset` asks the service host's own
    /// confirmation (Touch ID or the login password) through the host.
    pub(crate) fn reads_a_secret(&self) -> bool {
        true
    }
}

pub(crate) fn run(args: AdminArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    match args.command {
        AdminCommand::Reset(args) => reset::run(args, ctx),
        AdminCommand::Tls(TlsArgs {
            command: TlsCommand::Set(args),
        }) => tls_set::run(args, ctx),
    }
}
