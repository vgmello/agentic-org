//! `office invite`.

use clap::Args;
use org_cli_shell::context::ExecutionContext;

use crate::args::{InstanceArgs, LoginArgs, OrgArg};
use crate::error::CliError;
use org_host_setup::OfficeInviteInput;
use serde_json::json;

use crate::selection;
use crate::view::{self, column, Column, Show};

pub(super) const EXAMPLES: &str = "Examples:\n  agentic-org office invite Berlin --address hq.example.com:7443 --org <org-id>\n  agentic-org office invite Berlin --address 203.0.113.5:7443 --json";

#[derive(Args, Debug)]
pub(crate) struct InviteArgs {
    /// The new office's name
    #[arg(value_name = "OFFICE_NAME")]
    pub(super) office_name: String,
    /// HQ's federation address the satellite dials (HOST:PORT); repeat for several
    #[arg(
        long = "address",
        value_name = "HOST:PORT",
        required = true,
        value_delimiter = ','
    )]
    pub(super) addresses: Vec<String>,
    /// How long the invitation stays valid, in seconds
    #[arg(long, value_name = "SECONDS", value_parser = clap::value_parser!(u64).range(1..))]
    pub(super) ttl: Option<u64>,
    /// Invite an existing office again (re-enrollment)
    #[arg(long, value_name = "OFFICE_ID")]
    pub(super) re_enroll_office: Option<String>,
    #[command(flatten)]
    pub(super) org: OrgArg,
    #[command(flatten)]
    pub(super) instance: InstanceArgs,
    #[command(flatten)]
    pub(super) login: LoginArgs,
}

const FIELDS: [Column; 6] = [
    column("invitation_id", "invitation.invitation_id", Show::Id),
    column("office_id", "invitation.office_id", Show::Plain),
    column("office_name", "invitation.office_name", Show::Name),
    column("status", "invitation.status", Show::Status),
    column("expires_at_ms", "invitation.expires_at_ms", Show::Dim),
    column("token", "token", Show::Plain),
];

pub(super) fn run(args: InviteArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let input = OfficeInviteInput {
        org_id: args.org.org.as_deref(),
        office_name: &args.office_name,
        hq_addresses: args.addresses.clone(),
        ttl_secs: args.ttl,
        existing_office_id: args.re_enroll_office.as_deref(),
    };
    if ctx.modes().dry_run {
        let plan = json!({
            "action": "office_invite",
            "office_name": args.office_name,
            "addresses": args.addresses,
            "org_id": args.org.org,
            "ttl_secs": args.ttl,
        });
        let summary = format!("would invite satellite office \"{}\"", args.office_name);
        return Ok(ctx.render_dry_run(&summary, &plan)?);
    }
    let target = selection::select(args.instance.name.as_deref(), None)?;
    let created =
        org_host_setup::office_invite(target.instance(), args.login.login.as_deref(), &input)?;
    let json = view::json(&created)?;
    // `-q` prints the token: what the satellite needs.
    view::detail(ctx, &json, &json, "token", &FIELDS)?;
    ctx.note("Give the token to the satellite's operator: `agentic-org office join --invitation-stdin` reads it there.");
    Ok(())
}
