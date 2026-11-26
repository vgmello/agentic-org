//! `agentic-org office ...`: list and restart offices, and run HQ/satellite mTLS enrollment
//! and sync checks. Every destructive verb asks for confirmation on a terminal and needs
//! `--yes` where no one can be asked (REQ-CLI-08).

mod approve;
mod cancel;
mod decline;
mod invitations;
mod invite;
mod join;
mod list;
mod re_enroll;
mod remove;
mod remove_pending;
mod restart;
mod sync_status;

use std::io::Read;

use clap::{ArgGroup, Args, Subcommand};
use org_cli_shell::context::ExecutionContext;
use org_cli_shell::prompt::require_confirmation;
use org_host::setup_flow::InvitationTokenSecret;
use serde_json::Value;

use crate::error::CliError;
use crate::view::{column, Column, Show};

/// The columns of an office operation's answer, as `office restart` and
/// `office remove-pending` show it.
const DONE: [Column; 3] = [
    column("office_id", "office_id", Show::Id),
    column("outcome", "outcome", Show::Status),
    column("operation_id", "operation_id", Show::Dim),
];

const EXAMPLES: &str = "Examples:
  agentic-org office list
  agentic-org office invite Berlin --address hq.example.com:7443 --org <org-id>
  agentic-org office join --invitation-file ./invite.txt
  agentic-org office approve <invitation-id> --code <check-code> --org <org-id>
  agentic-org office remove <office-id> --org <org-id> --yes

Satellite enrollment is initiated from HQ (`office invite`) and completed on the satellite
(`office join`) and HQ (`office approve`).";

#[derive(Args, Debug)]
#[command(after_help = EXAMPLES)]
pub(crate) struct OfficeArgs {
    #[command(subcommand)]
    command: OfficeCommand,
}

#[derive(Subcommand, Debug)]
enum OfficeCommand {
    /// List the offices of this instance's organizations
    /// Use when: listing offices, their role (HQ or satellite) and status.
    #[command(visible_alias = "ls", verbatim_doc_comment, after_help = list::EXAMPLES)]
    List(list::ListArgs),
    /// Restart an office's server
    /// Use when: an office is stuck, or after changing its configuration.
    #[command(verbatim_doc_comment, after_help = restart::EXAMPLES)]
    Restart(restart::RestartArgs),
    /// Invite a satellite office to the organization (on HQ)
    /// Use when: starting satellite enrollment; prints the one-time invitation token.
    #[command(verbatim_doc_comment, after_help = invite::EXAMPLES)]
    Invite(invite::InviteArgs),
    /// List satellite invitations (on HQ)
    /// Use when: checking which invitations wait for a join or an approval.
    #[command(verbatim_doc_comment, after_help = invitations::EXAMPLES)]
    Invitations(invitations::InvitationsArgs),
    /// Approve a satellite's enrollment with its check code (on HQ)
    /// Use when: completing enrollment after the satellite joined and read you its code.
    #[command(verbatim_doc_comment, after_help = approve::EXAMPLES)]
    Approve(approve::ApproveArgs),
    /// Decline a satellite's enrollment (on HQ)
    /// Use when: refusing a satellite that joined with an invitation.
    #[command(verbatim_doc_comment, after_help = decline::EXAMPLES)]
    Decline(decline::DeclineArgs),
    /// Cancel a pending invitation (on HQ)
    /// Use when: withdrawing an invitation that was never used.
    #[command(verbatim_doc_comment, after_help = cancel::EXAMPLES)]
    Cancel(cancel::CancelArgs),
    /// Remove a satellite office and revoke its certificates (on HQ)
    /// Use when: retiring a satellite office for good.
    #[command(visible_alias = "rm", verbatim_doc_comment, after_help = remove::EXAMPLES)]
    Remove(remove::RemoveArgs),
    /// Show the federation sync state of this host's office
    /// Use when: checking that a satellite caught up with HQ over mTLS.
    #[command(verbatim_doc_comment, after_help = sync_status::EXAMPLES)]
    SyncStatus(sync_status::SyncStatusArgs),
    /// Join this host to an organization with an invitation token (on the satellite)
    /// Use when: enrolling this host as a satellite office.
    #[command(verbatim_doc_comment, after_help = join::EXAMPLES)]
    Join(join::JoinArgs),
    /// Remove this host's pending satellite enrollment
    /// Use when: abandoning an enrollment HQ never approved.
    #[command(verbatim_doc_comment, after_help = remove_pending::EXAMPLES)]
    RemovePending(remove_pending::RemovePendingArgs),
    /// Re-enroll a satellite office with a new invitation token
    /// Use when: renewing a satellite whose certificate was revoked or expired.
    #[command(verbatim_doc_comment, after_help = re_enroll::EXAMPLES)]
    ReEnroll(re_enroll::ReEnrollArgs),
}

/// Where the invitation token is read from: never the command line, where other users of
/// the computer could read it.
#[derive(Args, Debug)]
#[command(group(ArgGroup::new("invitation").required(true).args(["invitation_stdin", "invitation_file"])))]
pub(crate) struct InvitationArgs {
    /// Read the invitation token from standard input
    #[arg(long)]
    pub(super) invitation_stdin: bool,
    /// Read the invitation token from this file
    #[arg(long, value_name = "PATH")]
    pub(super) invitation_file: Option<String>,
}

impl OfficeArgs {
    /// `--yes` of a destructive verb.
    pub(crate) fn yes(&self) -> bool {
        match &self.command {
            OfficeCommand::Restart(args) => args.yes.yes,
            OfficeCommand::Decline(args) => args.yes.yes,
            OfficeCommand::Cancel(args) => args.yes.yes,
            OfficeCommand::Remove(args) => args.yes.yes,
            OfficeCommand::Join(args) => args.yes.yes,
            OfficeCommand::RemovePending(args) => args.yes.yes,
            OfficeCommand::ReEnroll(args) => args.yes.yes,
            _ => false,
        }
    }

    pub(crate) fn reads_a_secret(&self) -> bool {
        let login = match &self.command {
            OfficeCommand::List(args) => &args.login,
            OfficeCommand::Restart(args) => &args.login,
            OfficeCommand::Invite(args) => &args.login,
            OfficeCommand::Invitations(args) => &args.login,
            OfficeCommand::Approve(args) => &args.login,
            OfficeCommand::Decline(args) => &args.login,
            OfficeCommand::Cancel(args) => &args.login,
            OfficeCommand::Remove(args) => &args.login,
            OfficeCommand::SyncStatus(args) => &args.login,
            OfficeCommand::Join(args) => &args.login,
            OfficeCommand::RemovePending(args) => &args.login,
            OfficeCommand::ReEnroll(args) => &args.login,
        };
        login.reads_a_secret()
    }
}

pub(crate) fn run(args: OfficeArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    match args.command {
        OfficeCommand::List(args) => list::run(args, ctx),
        OfficeCommand::Restart(args) => restart::run(args, ctx),
        OfficeCommand::Invite(args) => invite::run(args, ctx),
        OfficeCommand::Invitations(args) => invitations::run(args, ctx),
        OfficeCommand::Approve(args) => approve::run(args, ctx),
        OfficeCommand::Decline(args) => decline::run(args, ctx),
        OfficeCommand::Cancel(args) => cancel::run(args, ctx),
        OfficeCommand::Remove(args) => remove::run(args, ctx),
        OfficeCommand::SyncStatus(args) => sync_status::run(args, ctx),
        OfficeCommand::Join(args) => join::run(args, ctx),
        OfficeCommand::RemovePending(args) => remove_pending::run(args, ctx),
        OfficeCommand::ReEnroll(args) => re_enroll::run(args, ctx),
    }
}

/// A destructive verb's guard: `--dry-run` shows `plan` and stops (`false`); otherwise it
/// asks on a terminal (no by default) or needs `--yes`, before any host is contacted.
fn guard(ctx: &mut ExecutionContext, action: &str, plan: &Value) -> Result<bool, CliError> {
    if ctx.modes().dry_run {
        ctx.render_dry_run(&format!("would {action}"), plan)?;
        return Ok(false);
    }
    let mut question = action.to_owned();
    if let Some(first) = question.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    require_confirmation(ctx, &question)?;
    Ok(true)
}

impl InvitationArgs {
    /// The invitation token, from the file or standard input; never echoed.
    fn read(&self) -> Result<InvitationTokenSecret, CliError> {
        let mut raw = match (&self.invitation_file, self.invitation_stdin) {
            (Some(path), _) => std::fs::read(path).map_err(|e| {
                CliError::failed(
                    "cli::io",
                    format!("cannot read invitation file `{path}`: {e}"),
                )
            })?,
            (None, _) => {
                let mut buf = Vec::new();
                std::io::stdin().read_to_end(&mut buf).map_err(|e| {
                    CliError::failed(
                        "cli::io",
                        format!("cannot read invitation token from stdin: {e}"),
                    )
                })?;
                buf
            }
        };
        let secret = std::str::from_utf8(&raw)
            .map_err(|_| CliError::usage("the invitation token must be valid UTF-8"))
            .and_then(|text| {
                InvitationTokenSecret::new(text.trim().to_owned())
                    .map_err(|_| CliError::usage("the invitation token cannot be empty"))
            });
        org_proto::zero_bytes(&mut raw);
        secret
    }
}
