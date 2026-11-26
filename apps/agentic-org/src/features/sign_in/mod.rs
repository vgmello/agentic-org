//! `agentic-org sign-in` (A5 / FC-173 / SIGN-40 / SES-8): browser-approved terminal sign-in
//! with optional `-- <command...>`, and the office resources (`task`, `msg`, `who`, ...) that
//! run over the approved, grant-bound office channel.

mod login;
pub(crate) mod office_proxy;
mod render;
mod selectors;

use clap::Args;
use clap_complete::engine::ArgValueCompleter;

pub(crate) use login::run;

use crate::args::InstanceArgs;
use crate::completers;

const EXAMPLES: &str = "Examples:
  agentic-org sign-in
  agentic-org sign-in --office <office-id>
  agentic-org sign-in -- task list --status open
  agentic-org sign-in --name lab -- msg inbox --format compact";

#[derive(Args, Debug)]
#[command(after_help = EXAMPLES)]
pub(crate) struct SignInArgs {
    #[command(flatten)]
    instance: InstanceArgs,
    /// The office to sign in to [default: AGENTIC_ORG_OFFICE, agentic-org.toml, the saved
    /// context, then the host's choice]
    #[arg(long, value_name = "OFFICE_ID", add = ArgValueCompleter::new(completers::offices))]
    office: Option<String>,
    /// One office command to run once signed in (after `--`)
    #[arg(last = true, value_name = "COMMAND")]
    command: Vec<String>,
}
