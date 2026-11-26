//! Which instance and office a command acts on, in five tiers (REQ-CLI-06): the flag
//! (`--name`/`--instance`, `--office`), the environment (`AGENTIC_ORG_INSTANCE`,
//! `AGENTIC_ORG_OFFICE`), the nearest `agentic-org.toml` above the working directory (only
//! these two non-secret names: never a socket, a release pin or custody), the context saved
//! by `agentic-org context use`, and the built-in default (the `default` instance; the office
//! the host picks when there is one HQ or one office). A leaf module beside `host`.

use org_cli_shell::config::{resolve_selection, Selection, SelectionInput, Tier};
use org_host::setup_flow::cli_context::{self, CliContext};

use crate::error::CliError;

/// The selection of one run.
#[derive(Debug, Clone)]
pub(crate) struct Target {
    pub(crate) selection: Selection,
    pub(crate) saved: Option<CliContext>,
}

impl Target {
    /// The instance to act on; `None` is the `default` instance.
    pub(crate) fn instance(&self) -> Option<&str> {
        self.selection.instance.value.as_deref()
    }

    /// The office, when a tier named one.
    pub(crate) fn office(&self) -> Option<&str> {
        self.selection.office.value.as_deref()
    }

    /// The instance as the user gave it (flag, environment or manifest), for commands whose
    /// host operation applies the saved context itself and checks it against the instance.
    pub(crate) fn explicit_instance(&self) -> Option<&str> {
        explicit(self.selection.instance.tier)
            .then(|| self.instance())
            .flatten()
    }

    /// The office as the user gave it (flag, environment or manifest).
    pub(crate) fn explicit_office(&self) -> Option<&str> {
        explicit(self.selection.office.tier)
            .then(|| self.office())
            .flatten()
    }
}

fn explicit(tier: Tier) -> bool {
    matches!(tier, Tier::Flag | Tier::Env | Tier::Manifest)
}

/// The saved context, if there is a readable one. An unreadable one is the host's to refuse
/// when a command needs it; selection then goes on without it.
fn saved() -> Option<CliContext> {
    let path = cli_context::default_context_path().ok()?;
    cli_context::load_from(&path).ok().flatten()
}

/// Selects the instance and office for flags `instance` and `office`.
pub(crate) fn select(instance: Option<&str>, office: Option<&str>) -> Result<Target, CliError> {
    let saved = saved();
    let start = std::env::current_dir().ok();
    let selection = resolve_selection(
        &SelectionInput {
            flag_instance: instance,
            flag_office: office,
            saved_instance: saved.as_ref().and_then(|ctx| ctx.instance_name.as_deref()),
            saved_office: saved.as_ref().map(|ctx| ctx.office_id.as_str()),
            default_instance: None,
            start_dir: start.as_deref(),
        },
        &|name| std::env::var_os(name),
    )?;
    Ok(Target { selection, saved })
}
