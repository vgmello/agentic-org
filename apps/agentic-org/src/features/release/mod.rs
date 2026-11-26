//! `agentic-org release install|status` (`TASK-121`, ADR-0009): verify and atomically stage
//! a signed release bundle (`.tar.gz` archive or directory) into the account's protected
//! release store, or inspect the active pinned release.

mod install;
mod status;

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use org_cli_shell::context::ExecutionContext;
use org_cli_shell::output::{Cell, Style};
use org_host::release::ReleasePin;
use serde::Serialize;

use crate::error::CliError;

const INSTALL_EXAMPLES: &str = "Examples:
  agentic-org release install ./agentic-org-1.2.0.tar.gz
  agentic-org release install ./agentic-org-1.2.0.tar.gz --dry-run
  agentic-org release install ./release-dir --store /srv/agentic-org/releases --json";

const STATUS_EXAMPLES: &str = "Examples:
  agentic-org release status
  agentic-org release status -q
  agentic-org release status --json";

#[derive(Args, Debug)]
#[command(
    after_help = "Examples:\n  agentic-org release install ./agentic-org-1.2.0.tar.gz\n  agentic-org release status --json"
)]
pub(crate) struct ReleaseArgs {
    #[command(subcommand)]
    command: ReleaseCommand,
}

#[derive(Subcommand, Debug)]
enum ReleaseCommand {
    /// Verify the Ed25519 signature, closed inventory and binary hashes, then atomically pin the release
    /// Use when: installing or upgrading a prebuilt signed Agentic Org release package without compiling from source.
    #[command(verbatim_doc_comment, after_help = INSTALL_EXAMPLES)]
    Install {
        /// The release bundle: a `.tar.gz` archive or a release directory
        #[arg(value_name = "BUNDLE")]
        bundle: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Show the active pinned release ID, sequence, manifest SHA-256 and installed executables
    /// Use when: verifying which release version is currently installed and active on this host.
    #[command(verbatim_doc_comment, after_help = STATUS_EXAMPLES)]
    Status {
        #[command(flatten)]
        store: StoreArgs,
    },
}

#[derive(Args, Debug)]
struct StoreArgs {
    /// The release store (an absolute path) [default: this account's release store]
    #[arg(long, value_name = "DIR")]
    store: Option<String>,
}

pub(crate) fn run(args: ReleaseArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    match args.command {
        ReleaseCommand::Install { bundle, store } => {
            install::run(&bundle, store.store.as_deref(), ctx)
        }
        ReleaseCommand::Status { store } => status::run(store.store.as_deref(), ctx),
    }
}

fn store_dir(explicit: Option<&str>) -> Result<PathBuf, CliError> {
    match explicit {
        Some(raw) => {
            let path = PathBuf::from(raw);
            if !path.is_absolute() {
                return Err(CliError::usage(format!(
                    "--store ({raw}) must be an absolute path"
                )));
            }
            Ok(path)
        }
        None => Ok(org_host::default_release_store_dir()?),
    }
}

/// A pinned release, as `--json` prints it (the keys and their order are a contract).
#[derive(Serialize)]
struct PinView {
    state: &'static str,
    store: String,
    release_id: String,
    sequence: u64,
    manifest_sha256: String,
    release_dir: String,
    agentic_org: String,
    agent_os: String,
}

impl PinView {
    fn new(store_dir: &Path, pin: &ReleasePin, state: &'static str) -> Self {
        let release_dir = store_dir.join("releases").join(&pin.release_id);
        Self {
            state,
            store: store_dir.display().to_string(),
            release_id: pin.release_id.clone(),
            sequence: pin.sequence,
            manifest_sha256: pin.manifest_sha256.clone(),
            agentic_org: release_dir.join("agentic-org").display().to_string(),
            agent_os: release_dir.join("agent-os").display().to_string(),
            release_dir: release_dir.display().to_string(),
        }
    }

    fn render(&self, ctx: &mut ExecutionContext) -> Result<(), CliError> {
        let state_style = if self.state == "active" {
            Style::Active
        } else {
            Style::Ok
        };
        let fields = [
            ("release", Cell::id(&self.release_id)),
            ("state", Cell::status(self.state, state_style)),
            ("sequence", Cell::plain(self.sequence.to_string())),
            ("manifest_sha256", Cell::dim(&self.manifest_sha256)),
            ("store", Cell::plain(&self.store)),
            ("release_dir", Cell::plain(&self.release_dir)),
            ("agentic_org", Cell::plain(&self.agentic_org)),
            ("agent_os", Cell::plain(&self.agent_os)),
        ];
        let json = serde_json::to_value(self)
            .map_err(|e| CliError::failed("cli::encode", e.to_string()))?;
        Ok(ctx.render_detail(&self.release_id, &fields, &json)?)
    }
}
