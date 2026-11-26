//! `release status`: the release this host's store has pinned.

use org_cli_shell::context::ExecutionContext;
use org_cli_shell::output::{Cell, Style};
use org_host::release::ReleaseStore;

use super::{store_dir, PinView};
use crate::error::CliError;

pub(super) fn run(
    explicit_store: Option<&str>,
    ctx: &mut ExecutionContext,
) -> Result<(), CliError> {
    let store_dir = store_dir(explicit_store)?;
    if !store_dir.exists() {
        return none(ctx, &store_dir.display().to_string(), "installed");
    }
    let mut store = ReleaseStore::open(&store_dir).map_err(|e| {
        format!(
            "opening release store ({}): {e} (a running service host holds the store lock while active)",
            store_dir.display()
        )
    })?;
    let recovered = store
        .recover()
        .map_err(|e| format!("verifying release store ({}): {e}", store_dir.display()))?;
    match recovered {
        Some(pin) => PinView::new(&store_dir, &pin, "active").render(ctx),
        None => none(ctx, &store_dir.display().to_string(), "pinned"),
    }
}

/// No release: `{"store": .., "pinned": null}` under `--json`, nothing under `-q`, and a card
/// that says so otherwise.
fn none(ctx: &mut ExecutionContext, store: &str, missing: &str) -> Result<(), CliError> {
    let json = serde_json::json!({ "store": store, "pinned": serde_json::Value::Null });
    if ctx.modes().quiet {
        return Ok(());
    }
    let fields = [
        (
            "state",
            Cell::status(format!("no release {missing}"), Style::Warn),
        ),
        ("store", Cell::plain(store)),
    ];
    Ok(ctx.render_detail("", &fields, &json)?)
}
