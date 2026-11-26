//! `context show`: the saved instance and office.

use clap::Args;
use org_cli_shell::context::ExecutionContext;
use org_cli_shell::output::{Cell, Style};
use org_host::setup_flow::cli_context;
use serde_json::json;

use crate::error::CliError;

pub(super) const EXAMPLES: &str = "Examples:\n  agentic-org context show\n  agentic-org context show --json\n  agentic-org context show -q";

#[derive(Args, Debug)]
pub(crate) struct ShowArgs {}

/// `--json`: the saved context (`instance_id`, `instance_name`, `office_id`) or `null`;
/// `-q`: the office ID; otherwise a card, or a notice on standard error when nothing is
/// saved.
pub(super) fn run(_args: ShowArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let path = cli_context::default_context_path()?;
    let Some(saved) = cli_context::load_from(&path)? else {
        if ctx.modes().json {
            return Ok(ctx.write_json(&serde_json::Value::Null)?);
        }
        ctx.note("No saved CLI context. `agentic-org context use --office <office-id>` saves one.");
        return Ok(());
    };
    let instance = saved
        .instance_name
        .clone()
        .unwrap_or_else(|| "default".to_owned());
    let fields = [
        ("office_id", Cell::id(&saved.office_id)),
        ("instance_name", Cell::name(&instance)),
        ("instance_id", Cell::new(&saved.instance_id, Style::Dim)),
    ];
    let value = json!({
        "instance_id": saved.instance_id,
        "instance_name": instance,
        "office_id": saved.office_id,
    });
    Ok(ctx.render_detail(&saved.office_id, &fields, &value)?)
}
