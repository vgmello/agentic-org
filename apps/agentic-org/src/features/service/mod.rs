//! `agentic-org service start|stop|status`: the registered OS service of an instance setup
//! created, through the same platform adapter setup used.

mod start;
mod status;
mod stop;

use clap::{Args, Subcommand};
use org_cli_shell::context::ExecutionContext;
use org_host::setup_flow::{ServiceAction, ServiceReport, ServiceState};
use serde_json::json;

use crate::args::InstanceArgs;
use crate::error::CliError;
use crate::{host, selection};

const EXAMPLES: &str = "Examples:
  agentic-org service status
  agentic-org service stop --name lab
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
    #[command(
        verbatim_doc_comment,
        after_help = "Examples:\n  agentic-org service stop\n  agentic-org service stop --dry-run"
    )]
    Stop(InstanceArgs),
    /// Show whether the instance's registered OS service is running
    /// Use when: checking host service health before running organization or office commands.
    #[command(
        verbatim_doc_comment,
        after_help = "Examples:\n  agentic-org service status\n  agentic-org service status -q\n  agentic-org service status --json"
    )]
    Status(InstanceArgs),
}

pub(crate) fn run(args: ServiceArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    match args.command {
        ServiceCommand::Start(args) => start::run(args, ctx),
        ServiceCommand::Stop(args) => stop::run(args, ctx),
        ServiceCommand::Status(args) => status::run(args, ctx),
    }
}

/// The line a person reads (also in a pipe: it is a tested contract): `running (pid 42)`,
/// `stopped`, `registered, not loaded`, `not registered`; `Service for "lab" started.`.
fn summary(report: &ServiceReport) -> String {
    match (report.state, report.foreground) {
        (ServiceState::Running, true) => {
            "running in the foreground (no service is registered)".to_owned()
        }
        (ServiceState::Stopped, true) => {
            "not running (foreground mode: it runs while agentic-org serve --setup <file> runs)"
                .to_owned()
        }
        (ServiceState::Running, false) => match report.pid {
            Some(pid) => format!("running (pid {pid})"),
            None => "running".to_owned(),
        },
        (ServiceState::Stopped, false) => "stopped".to_owned(),
        (ServiceState::Unloaded, _) => "registered, not loaded".to_owned(),
        (ServiceState::NotRegistered, _) => "not registered".to_owned(),
        (ServiceState::Started, _) => format!("Service for \"{}\" started.", report.instance),
        (ServiceState::StoppedByRequest, _) => {
            format!("Service for \"{}\" stopped.", report.instance)
        }
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
        let plan = json!({"action": format!("service_{}", org_host::setup_flow::wire_name(&action)), "instance": target.instance().unwrap_or("default")});
        return Ok(ctx.render_dry_run("would change the instance's registered service", &plan)?);
    }
    let platform = host::platform(false)?;
    let report = org_host::setup_flow::service(target.instance(), &*platform.service, action)?;
    if ctx.modes().json {
        return Ok(ctx.write_json(&report)?);
    }
    let state = org_host::setup_flow::wire_name(&report.state);
    if ctx.modes().quiet {
        return Ok(ctx.out(&format!("{state}\n"))?);
    }
    let mut text = summary(&report);
    if ctx.is_tty_stdout() {
        let glyph = org_cli_shell::output::glyph(crate::view::status_style(&state));
        text = format!("{glyph}{text}");
    }
    text.push('\n');
    for note in &report.notes {
        text.push_str(note);
        text.push('\n');
    }
    Ok(ctx.out(&org_cli_shell::terminal::sanitize_lines(&text))?)
}
