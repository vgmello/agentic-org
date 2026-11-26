//! `context use`.

use clap::Args;
use clap_complete::engine::ArgValueCompleter;
use org_cli_shell::context::ExecutionContext;

use crate::args::InstanceArgs;
use crate::completers;
use crate::error::CliError;
use serde_json::json;

use crate::view::{self, column, Column, Show};

pub(super) const EXAMPLES: &str = "Examples:\n  agentic-org context use --office <office-id>\n  agentic-org context use --name lab --office <office-id>";

#[derive(Args, Debug)]
pub(crate) struct UseArgs {
    /// The office to act on by default
    #[arg(long, value_name = "OFFICE_ID", add = ArgValueCompleter::new(completers::offices))]
    pub(super) office: String,
    #[command(flatten)]
    pub(super) instance: InstanceArgs,
}

const FIELDS: [Column; 3] = [
    column("office_id", "office_id", Show::Id),
    column("instance_name", "instance_name", Show::Name),
    column("instance_id", "instance_id", Show::Dim),
];

pub(super) fn run(args: UseArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    // A relative `AGENTIC_ORG_CONFIG_DIR` is refused before anything, a preview too.
    crate::selection::context_path()?;
    if ctx.modes().dry_run {
        let plan = json!({"action": "context_use", "office_id": args.office, "instance_name": args.instance.name});
        return Ok(ctx.render_dry_run("would save the default instance and office", &plan)?);
    }
    let saved = org_host::setup_flow::context_use(args.instance.name.as_deref(), &args.office)?;
    let mut json = view::json(&saved)?;
    if json["instance_name"].is_null() {
        json["instance_name"] = "default".into();
    }
    view::detail(ctx, &json, &json, "office_id", &FIELDS)?;
    ctx.success("Saved the default instance and office.");
    Ok(())
}
