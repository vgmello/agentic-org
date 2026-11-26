//! `agentic-org org create|list|inspect` (`TASK-113`): create or list organizations on this
//! host; `inspect` is the office command `org inspect`.

mod create;
mod list;

use clap::{Args, Subcommand};
use org_cli_shell::context::ExecutionContext;

use crate::args::PassArgs;
use crate::error::CliError;

const EXAMPLES: &str = "Examples:
  agentic-org org create \"Acme\" --slug acme --repo https://git.example.com/acme/org.git
  agentic-org org list
  agentic-org org list --json
  agentic-org org inspect";

const CREATE_EXAMPLES: &str = "Examples:
  agentic-org org create \"Acme\" --slug acme --repo https://git.example.com/acme/org.git
  agentic-org org create \"Acme\" --slug acme --repo https://git.example.com/acme/org.git --dry-run
  agentic-org org create \"Acme\" --slug acme --repo git@git.example.com:acme/org.git --branch state -q";

const LIST_EXAMPLES: &str = "Examples:
  agentic-org org list
  agentic-org org ls -q
  agentic-org org list --json";

#[derive(Args, Debug)]
#[command(after_help = EXAMPLES)]
pub(crate) struct OrgArgs {
    #[command(subcommand)]
    command: OrgCommand,
}

#[derive(Subcommand, Debug)]
enum OrgCommand {
    /// Create an organization (its HQ office and state repository) on this host
    /// Use when: creating an HQ organization from a remote Git repository.
    #[command(visible_alias = "new", verbatim_doc_comment, after_help = CREATE_EXAMPLES)]
    Create(create::CreateArgs),
    /// List the organizations this host has, and those still being created
    /// Use when: finding an organization's ID, slug or HQ office.
    #[command(visible_alias = "ls", verbatim_doc_comment, after_help = LIST_EXAMPLES)]
    List(list::ListArgs),
    /// Inspect the organization (the office command `org inspect`)
    /// Use when: reading the organization's record from its office.
    #[command(verbatim_doc_comment, disable_help_flag = true)]
    Inspect(PassArgs),
}

impl OrgArgs {
    pub(crate) fn reads_a_secret(&self) -> bool {
        match &self.command {
            OrgCommand::Create(args) => args.login.reads_a_secret(),
            OrgCommand::List(args) => args.login.reads_a_secret(),
            OrgCommand::Inspect(_) => false,
        }
    }

    /// `org inspect ...`: the office command's words.
    pub(crate) fn office_words(&self) -> Option<Vec<String>> {
        match &self.command {
            OrgCommand::Inspect(pass) => Some(
                ["org".to_owned(), "inspect".to_owned()]
                    .into_iter()
                    .chain(pass.args.iter().cloned())
                    .collect(),
            ),
            _ => None,
        }
    }
}

/// An organization as its card and row show it: its JSON with its HQ office's ID (the
/// office whose role is `hq`, else its first).
fn shown_org(org: &serde_json::Value) -> serde_json::Value {
    let offices = crate::view::items(org, "offices");
    let hq = offices
        .iter()
        .find(|office| office["role"] == "hq")
        .or_else(|| offices.first())
        .map(|office| office["id"].clone())
        .unwrap_or(serde_json::Value::Null);
    let mut shown = org.clone();
    shown["hq_office_id"] = hq;
    shown
}

pub(crate) fn run(args: OrgArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    match args.command {
        OrgCommand::Create(args) => create::run(args, ctx),
        OrgCommand::List(args) => list::run(args, ctx),
        OrgCommand::Inspect(_) => Err(CliError::usage("org inspect runs as an office command")),
    }
}
