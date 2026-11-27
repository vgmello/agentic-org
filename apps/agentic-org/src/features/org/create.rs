//! `org create`: validates the plan before any host is located or contacted; `--dry-run`
//! stops there.

use clap::Args;
use org_cli_shell::context::ExecutionContext;
use org_host_setup::{OrgCreateInput, OrgCreateOutcome};
use serde::Serialize;

use super::shown_org;
use crate::args::{InstanceArgs, LoginArgs};
use crate::error::CliError;
use crate::selection;
use crate::view::{self, column, Column, Show};

#[derive(Args, Debug)]
pub(super) struct CreateArgs {
    /// The organization's display name
    #[arg(id = "display_name", value_name = "NAME")]
    pub(super) name: String,
    /// Its short unique slug (lower-case letters, digits and hyphens)
    #[arg(long)]
    pub(super) slug: String,
    /// The remote Git repository that keeps its state (https:// or ssh)
    #[arg(long, value_name = "URL")]
    pub(super) repo: String,
    /// The branch of the state repository
    #[arg(long, value_name = "BRANCH", default_value = "main")]
    pub(super) branch: String,
    /// Return once the host accepts the creation, without waiting for it to finish
    #[arg(long)]
    pub(super) no_wait: bool,
    #[command(flatten)]
    pub(super) instance: InstanceArgs,
    #[command(flatten)]
    pub(super) login: LoginArgs,
}

const CREATED: [Column; 6] = [
    column("org_id", "id", Show::Id),
    column("name", "name", Show::Name),
    column("slug", "slug", Show::Plain),
    column("hq_office_id", "hq_office_id", Show::Plain),
    column("employee_id", "employee_id", Show::Dim),
    column("recovery", "recovery", Show::Status),
];

const ACCEPTED: [Column; 2] = [
    column("operation_id", "operation_id", Show::Id),
    column("status", "status", Show::Status),
];

#[derive(Serialize)]
struct Plan<'a> {
    action: &'static str,
    name: &'a str,
    slug: &'a str,
    repository: &'a str,
    state_branch: &'a str,
}

pub(super) fn run(args: CreateArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let input = OrgCreateInput {
        name: &args.name,
        slug: &args.slug,
        repository_remote_url: &args.repo,
        state_branch: &args.branch,
        no_wait: args.no_wait,
    };
    org_host_setup::validate_create_plan(&input)?;
    if ctx.modes().dry_run {
        let plan = Plan {
            action: "org_create",
            name: input.name,
            slug: input.slug,
            repository: input.repository_remote_url,
            state_branch: input.state_branch,
        };
        let summary = format!(
            "would create organization \"{}\" ({}) from {} branch {}",
            plan.name, plan.slug, plan.repository, plan.state_branch
        );
        return Ok(ctx.render_dry_run(&summary, &plan)?);
    }
    let target = selection::select(args.instance.name.as_deref(), None)?;
    let outcome =
        org_host_setup::org_create(target.instance(), args.login.login.as_deref(), &input)?;
    let json = view::json(&outcome)?;
    match &outcome {
        OrgCreateOutcome::Created(_) => {
            let shown = shown_org(&json);
            view::detail(ctx, &shown, &json, "id", &CREATED)?;
            ctx.success(&format!("Created organization \"{}\".", args.name));
        }
        OrgCreateOutcome::Accepted { .. } => {
            view::detail(ctx, &json, &json, "operation_id", &ACCEPTED)?;
            ctx.note("Still being created: `agentic-org org list` shows it under `creating`.");
        }
    }
    Ok(())
}
