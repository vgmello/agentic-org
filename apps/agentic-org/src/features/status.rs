//! `agentic-org status`: which instance and office commands act on, and which tier chose them
//! (flag, environment, `agentic-org.toml`, saved context, default); the saved context; the
//! instance's service.

use clap::Args;
use clap_complete::engine::ArgValueCompleter;
use org_cli_shell::context::ExecutionContext;
use org_cli_shell::output::{Cell, Style};
use serde::Serialize;

use crate::args::InstanceArgs;
use crate::completers;
use crate::error::CliError;
use crate::selection;

const EXAMPLES: &str = "Examples:
  agentic-org status
  agentic-org status --name lab
  agentic-org status --json
  agentic-org status -q";

#[derive(Args, Debug)]
#[command(after_help = EXAMPLES)]
pub(crate) struct StatusArgs {
    #[command(flatten)]
    instance: InstanceArgs,
    /// The office to report as selected
    #[arg(long, value_name = "OFFICE_ID", add = ArgValueCompleter::new(completers::offices))]
    office: Option<String>,
}

#[derive(Serialize)]
struct StatusView {
    version: &'static str,
    instance: String,
    instance_source: &'static str,
    office: Option<String>,
    office_source: &'static str,
    manifest: Option<String>,
    saved_context: Option<org_host_setup::cli_context::CliContext>,
    service: Option<crate::host::ServiceView>,
}

pub(crate) fn run(args: StatusArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let target = selection::select(args.instance.name.as_deref(), args.office.as_deref())?;
    let view = StatusView {
        version: env!("CARGO_PKG_VERSION"),
        instance: target.instance().unwrap_or("default").to_owned(),
        instance_source: target.selection.instance.tier.as_str(),
        office: target.office().map(str::to_owned),
        office_source: target.selection.office.tier.as_str(),
        manifest: target
            .selection
            .manifest
            .as_ref()
            .map(|path| path.display().to_string()),
        saved_context: target.saved.clone(),
        service: crate::host::service_view(target.instance()),
    };
    let office = view
        .office
        .clone()
        .unwrap_or_else(|| "(chosen by the host)".to_owned());
    let mut fields = vec![
        ("instance", Cell::name(&view.instance)),
        ("instance_source", Cell::dim(view.instance_source)),
        ("office", Cell::id(&office)),
        ("office_source", Cell::dim(view.office_source)),
        (
            "manifest",
            Cell::plain(view.manifest.clone().unwrap_or_else(|| "none".to_owned())),
        ),
        ("version", Cell::dim(view.version)),
    ];
    if let Some(service) = &view.service {
        fields.push(("service", Cell::status(&service.summary, service.style())));
    }
    let saved = match &view.saved_context {
        Some(saved) => format!(
            "{} on {}",
            saved.office_id,
            saved.instance_name.as_deref().unwrap_or("default")
        ),
        None => "none".to_owned(),
    };
    fields.push(("saved_context", Cell::new(saved, Style::Plain)));
    let json =
        serde_json::to_value(&view).map_err(|e| CliError::failed("cli::encode", e.to_string()))?;
    Ok(ctx.render_detail(&view.instance, &fields, &json)?)
}
