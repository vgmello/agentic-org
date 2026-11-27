//! Which instance and office a command acts on, in five tiers (REQ-CLI-06): the flag
//! (`--name`/`--instance`, `--office`), the environment (`AGENTIC_ORG_INSTANCE`,
//! `AGENTIC_ORG_OFFICE`), the nearest `agentic-org.toml` above the working directory (only
//! these two non-secret names: never a socket, a release pin or custody), the context saved
//! by `agentic-org context use`, and the built-in default (the `default` instance; the office
//! the host picks when there is one HQ or one office). A leaf module beside `host`.

use std::path::PathBuf;

use org_cli_shell::config::{resolve_selection, Selection, SelectionInput, Tier};
use org_cli_shell::error::ShellError;
use org_host_setup::cli_context::{self, CliContext};

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

/// Where the saved context lives (`org-host`'s rule: `AGENTIC_ORG_CONFIG_DIR`, a development
/// build's account base, the account folder), once the CLI's directory variables are
/// checked: a relative or unusable `AGENTIC_ORG_CONFIG_DIR` or `AGENTIC_ORG_CACHE_DIR` is
/// `cli::config::invalid` (exit 1), never ignored or read against the working directory.
pub(crate) fn context_path() -> Result<PathBuf, CliError> {
    crate::host::cli_dirs()?;
    Ok(cli_context::default_context_path()?)
}

/// The saved context: `None` when nothing is saved. A context file that cannot be read, or
/// that holds what no context may hold (a name with a control or bidirectional character,
/// an office that is not an ID), is `cli::config::invalid` (exit 1): selection never goes on
/// as if it were not there, and `context clear` still removes it.
pub(crate) fn saved_context() -> Result<Option<CliContext>, CliError> {
    let path = context_path()?;
    cli_context::load_from(&path).map_err(|message| {
        ShellError::InvalidConfig {
            message,
            help: Some(
                "Clear it with `agentic-org context clear`, then save it again with \
                 `agentic-org context use`."
                    .to_owned(),
            ),
        }
        .into()
    })
}

/// Selects the instance and office for flags `instance` and `office`.
pub(crate) fn select(instance: Option<&str>, office: Option<&str>) -> Result<Target, CliError> {
    let saved = saved_context()?;
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
