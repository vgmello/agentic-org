//! `project create`.

use clap::Args;
use org_cli_shell::context::ExecutionContext;

use crate::args::{InstanceArgs, LoginArgs, OrgArg};
use crate::error::CliError;
use org_host_setup::ProjectCreateInput;
use serde_json::json;

use super::RECORD;
use crate::selection;
use crate::view;

pub(super) const EXAMPLES: &str = "Examples:\n  agentic-org project create Billing\n  agentic-org project create Billing --slug billing --owner-team <team-id> --dry-run\n  agentic-org project new Billing --parent-team <team-id> -q";

#[derive(Args, Debug)]
pub(crate) struct CreateArgs {
    /// The project's name
    #[arg(id = "display_name", value_name = "NAME")]
    pub(super) name: String,
    /// Its slug [default: derived from the name]
    #[arg(long)]
    pub(super) slug: Option<String>,
    /// The team that owns it [default: a new team for the project]
    #[arg(long, value_name = "TEAM_ID")]
    pub(super) owner_team: Option<String>,
    /// The parent of the new owning team
    #[arg(long, value_name = "TEAM_ID")]
    pub(super) parent_team: Option<String>,
    #[command(flatten)]
    pub(super) org: OrgArg,
    #[command(flatten)]
    pub(super) instance: InstanceArgs,
    #[command(flatten)]
    pub(super) login: LoginArgs,
}

pub(super) fn run(args: CreateArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let input = ProjectCreateInput {
        org_id: args.org.org.as_deref(),
        name: &args.name,
        slug: args.slug.as_deref(),
        owner_team_id: args.owner_team.as_deref(),
        parent_team_id: args.parent_team.as_deref(),
    };
    if ctx.modes().dry_run {
        let plan = json!({
            "action": "project_create",
            "name": args.name,
            "slug": args.slug,
            "owner_team_id": args.owner_team,
            "parent_team_id": args.parent_team,
            "org_id": args.org.org,
        });
        return Ok(ctx.render_dry_run(&format!("would create project \"{}\"", args.name), &plan)?);
    }
    let target = selection::select(args.instance.name.as_deref(), None)?;
    let created =
        org_host_setup::project_create(target.instance(), args.login.login.as_deref(), &input)?;
    let json = view::json(&created)?;
    view::detail(ctx, &json, &json, "id", &RECORD)?;
    ctx.success(&format!("Created project \"{}\".", args.name));
    Ok(())
}
