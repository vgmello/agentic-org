//! `context clear`: forget the saved instance and office (asks first on a terminal).

use clap::Args;
use org_cli_shell::context::ExecutionContext;
use org_cli_shell::prompt::require_confirmation;
use org_host::setup_flow::cli_context;
use serde::Serialize;

use crate::args::YesArgs;
use crate::error::CliError;

pub(super) const EXAMPLES: &str = "Examples:\n  agentic-org context clear\n  agentic-org context clear --yes\n  agentic-org context clear --dry-run";

#[derive(Args, Debug)]
pub(crate) struct ClearArgs {
    #[command(flatten)]
    pub(super) yes: YesArgs,
}

#[derive(Serialize)]
struct Cleared {
    cleared: bool,
}

pub(super) fn run(_args: ClearArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let path = crate::selection::context_path()?;
    let saved = cli_context::load_from(&path).ok().flatten().is_some() || path.exists();
    if ctx.modes().dry_run {
        let summary = if saved {
            "would clear the saved CLI context"
        } else {
            "no saved CLI context; nothing would change"
        };
        return Ok(ctx.render_dry_run(summary, &Cleared { cleared: saved })?);
    }
    if !saved {
        if ctx.modes().json {
            return Ok(ctx.write_json(&Cleared { cleared: false })?);
        }
        ctx.note("No saved CLI context.");
        return Ok(());
    }
    require_confirmation(ctx, "Clear the saved CLI context")?;
    let cleared = cli_context::clear_at(&path)?;
    if ctx.modes().json {
        return Ok(ctx.write_json(&Cleared { cleared })?);
    }
    ctx.success("Cleared the saved CLI context.");
    Ok(())
}
