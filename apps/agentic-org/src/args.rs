//! Argument groups several commands share. The vertical slices and the grammar (`cli.rs`)
//! use it; it uses only the completers.

use clap::Args;
use clap_complete::engine::ArgValueCompleter;

use crate::completers;

/// The instance a host command acts on (`--name`, also `--instance`).
#[derive(Args, Debug, Clone, Default)]
pub(crate) struct InstanceArgs {
    /// The instance to act on [default: AGENTIC_ORG_INSTANCE, agentic-org.toml, the saved
    /// context, then "default"]
    #[arg(
        long = "name",
        visible_alias = "instance",
        value_name = "INSTANCE",
        add = ArgValueCompleter::new(completers::instances)
    )]
    pub(crate) name: Option<String>,
}

/// How a host command signs in (`--login`): without it the local session is used, and when
/// the host requires authentication a browser approval link is shown on standard error.
#[derive(Args, Debug, Clone, Default)]
pub(crate) struct LoginArgs {
    /// Sign in with this login and a password asked on the terminal instead of a browser
    /// approval
    #[arg(long, value_name = "LOGIN")]
    pub(crate) login: Option<String>,
}

impl LoginArgs {
    /// Whether this run may read a password from the terminal: then the interrupt handler
    /// is left out, so the password prompt's own signal handling restores the terminal.
    pub(crate) fn reads_a_secret(&self) -> bool {
        self.login.is_some()
    }
}

/// `--yes` on a destructive command.
#[derive(Args, Debug, Clone, Copy, Default)]
pub(crate) struct YesArgs {
    /// Confirm without asking (required when no terminal can be asked)
    #[arg(short = 'y', long)]
    pub(crate) yes: bool,
}

/// `--org` on a host command.
#[derive(Args, Debug, Clone, Default)]
pub(crate) struct OrgArg {
    /// The organization [default: the only one this instance has]
    #[arg(long = "org", value_name = "ORG_ID", add = ArgValueCompleter::new(completers::orgs))]
    pub(crate) org: Option<String>,
}

/// The words of a command whose own parser reads them (`serve`, `setup`, `recovery`, the
/// office resources and `org inspect`): everything after the command, `--help` included.
#[derive(Args, Debug, Clone, Default)]
pub(crate) struct PassArgs {
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        num_args = 0..,
        value_name = "ARGS"
    )]
    pub(crate) args: Vec<String>,
}
