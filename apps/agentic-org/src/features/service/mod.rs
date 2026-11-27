//! `agentic-org service start|stop|status`: the registered OS service of an instance setup
//! created, through the same platform adapter setup used.

mod start;
mod status;
mod stop;

use clap::{Args, Subcommand};
use org_cli_shell::context::ExecutionContext;
use org_cli_shell::output::Cell;
use org_cli_shell::terminal::{sanitize, sanitize_lines};
use org_host_setup::ServiceAction;
use serde_json::json;

use crate::args::InstanceArgs;
use crate::error::CliError;
use crate::{host, selection};

const EXAMPLES: &str = "Examples:
  agentic-org service status
  agentic-org service stop --name lab --yes
  agentic-org service start --json";

#[derive(Args, Debug)]
#[command(after_help = EXAMPLES)]
pub(crate) struct ServiceArgs {
    #[command(subcommand)]
    command: ServiceCommand,
}

#[derive(Subcommand, Debug)]
enum ServiceCommand {
    /// Start the instance's registered OS service
    /// Use when: starting a stopped host service managed by launchd/systemd.
    #[command(
        verbatim_doc_comment,
        after_help = "Examples:\n  agentic-org service start\n  agentic-org service start --name lab"
    )]
    Start(InstanceArgs),
    /// Stop the instance's registered OS service
    /// Use when: cleanly shutting down the host background service for maintenance.
    #[command(verbatim_doc_comment, after_help = stop::EXAMPLES)]
    Stop(stop::StopArgs),
    /// Show whether the instance's registered OS service is running
    /// Use when: checking host service health before running organization or office commands.
    #[command(
        verbatim_doc_comment,
        after_help = "Examples:\n  agentic-org service status\n  agentic-org service status -q\n  agentic-org service status --json"
    )]
    Status(InstanceArgs),
}

impl ServiceArgs {
    /// `--yes` of `service stop`, the destructive verb.
    pub(crate) fn yes(&self) -> bool {
        matches!(&self.command, ServiceCommand::Stop(args) if args.yes.yes)
    }
}

pub(crate) fn run(args: ServiceArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    match args.command {
        ServiceCommand::Start(args) => start::run(args, ctx),
        ServiceCommand::Stop(args) => stop::run(args, ctx),
        ServiceCommand::Status(args) => status::run(args, ctx),
    }
}

/// `start`, `stop` or `status` through the platform's service adapter (the one setup used).
fn act(
    args: InstanceArgs,
    action: ServiceAction,
    ctx: &mut ExecutionContext,
) -> Result<(), CliError> {
    let target = selection::select(args.name.as_deref(), None)?;
    if ctx.modes().dry_run && action != ServiceAction::Status {
        let plan = json!({"action": format!("service_{}", org_host_setup::wire_name(&action)), "instance": target.instance().unwrap_or("default")});
        return Ok(ctx.render_dry_run("would change the instance's registered service", &plan)?);
    }
    let platform = host::platform(false)?;
    let report = org_host_setup::service(target.instance(), &*platform.service, action)?;
    if ctx.modes().json {
        return Ok(ctx.write_json(&report)?);
    }
    let state = org_host_setup::wire_name(&report.state);
    if ctx.modes().quiet {
        return Ok(ctx.out(&format!("{state}\n"))?);
    }
    let summary = host::service_summary(&report);
    // On a terminal: the status glyph and its color, as `status` and `release status` show
    // it (sanitized by the cell). In a pipe: the sentence alone (a tested contract).
    let mut text = if ctx.is_tty_stdout() {
        Cell::status(summary, crate::view::status_style(&state)).line(ctx.stdout_color())
    } else {
        sanitize(&summary).into_owned()
    };
    text.push('\n');
    for note in &report.notes {
        text.push_str(&sanitize_lines(note));
        text.push('\n');
    }
    Ok(ctx.out(&text)?)
}
