//! `office remove`.

use clap::Args;
use clap_complete::engine::ArgValueCompleter;
use org_cli_shell::context::ExecutionContext;

use crate::args::{InstanceArgs, LoginArgs, OrgArg, YesArgs};
use crate::completers;
use crate::error::CliError;
use serde_json::json;

use super::guard;
use crate::selection;
use crate::view::{self, column, Column, Show};

pub(super) const EXAMPLES: &str = "Examples:\n  agentic-org office remove <office-id> --org <org-id>\n  agentic-org office rm <office-id> --org <org-id> --yes --json";

#[derive(Args, Debug)]
pub(crate) struct RemoveArgs {
    /// The satellite office to remove
    #[arg(value_name = "OFFICE_ID", add = ArgValueCompleter::new(completers::offices))]
    pub(super) office_id: String,
    #[command(flatten)]
    pub(super) org: OrgArg,
    #[command(flatten)]
    pub(super) yes: YesArgs,
    #[command(flatten)]
    pub(super) instance: InstanceArgs,
    #[command(flatten)]
    pub(super) login: LoginArgs,
}

const FIELDS: [Column; 2] = [
    column("office_id", "office_id", Show::Id),
    column("revoked_serials", "revoked_serials", Show::Plain),
];

pub(super) fn run(args: RemoveArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let plan =
        json!({"action": "office_remove", "office_id": args.office_id, "org_id": args.org.org});
    let action = format!(
        "remove office {} and revoke its certificates",
        args.office_id
    );
    if !guard(ctx, &action, &plan)? {
        return Ok(());
    }
    let target = selection::select(args.instance.name.as_deref(), None)?;
    let removed = org_host_setup::office_remove(
        target.instance(),
        args.login.login.as_deref(),
        args.org.org.as_deref(),
        &args.office_id,
    )?;
    let json = view::json(&removed)?;
    view::detail(ctx, &json, &json, "office_id", &FIELDS)?;
    ctx.success(&format!("Removed office {}.", args.office_id));
    Ok(())
}
