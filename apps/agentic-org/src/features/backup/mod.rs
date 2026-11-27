//! `backup` and `restore` (`TASK-104`): a consistent local backup of an organization's
//! durable data, its verification, and its restore over a stopped office. The work is
//! `org-backup`'s; this slice reads the grammar, the passphrase sources and the instance,
//! and shows the result.

mod passphrase;
mod restore;
mod take;

use std::path::PathBuf;

use clap::Args;
use clap_complete::engine::ArgValueCompleter;
use org_backup::BackupError;
use org_cli_shell::context::ExecutionContext;

use crate::args::{InstanceArgs, YesArgs};
use crate::completers;
use crate::error::CliError;

pub(crate) use restore::run as restore;
pub(crate) use take::run as backup;

const BACKUP_EXAMPLES: &str = "Examples:
  agentic-org service stop
  agentic-org backup <org-id> --out ~/Backups/acme-2026-11-26.tar.gz
  agentic-org backup <org-id> --out acme.backup --encrypt
  agentic-org backup <org-id> --out acme.backup --encrypt --passphrase-file ~/.acme-pass --json

The office must be stopped: a running host or office (its host.lock or office.lock held) is \
refused. The passphrase is read from the terminal, --passphrase-file or --passphrase-fd, never \
from the command line.";

const RESTORE_EXAMPLES: &str = "Examples:
  agentic-org restore --verify acme.tar.gz
  agentic-org restore --verify acme.backup --to ./acme-check --passphrase-fd 3 3<pass.txt
  agentic-org service stop
  agentic-org restore acme.tar.gz --org <org-id> --yes

`--verify` changes nothing (with `--to`, it only fills that new folder); its disposable \
check copy is owner-only and made beside `--to` or the archive (or in `--work-dir`), never in \
the shared temporary folder. An in-place restore needs the office stopped, keeps the replaced \
durable folder in the owner-only `restore-backups` folder beside it, and never resets the \
host's writer custody: an archive behind the office's journal is refused.";

/// Where a passphrase comes from besides the terminal; never the command line itself.
#[derive(Args, Debug, Default)]
pub(crate) struct PassphraseArgs {
    /// Read the passphrase from this owner-only file (chmod 600)
    #[arg(long, value_name = "PATH", conflicts_with = "passphrase_fd")]
    passphrase_file: Option<PathBuf>,
    /// Read the passphrase from this open file descriptor (a pipe, `3<file`)
    #[arg(long, value_name = "FD")]
    passphrase_fd: Option<u32>,
}

impl PassphraseArgs {
    fn given(&self) -> bool {
        self.passphrase_file.is_some() || self.passphrase_fd.is_some()
    }
}

#[derive(Args, Debug)]
#[command(after_help = BACKUP_EXAMPLES)]
pub(crate) struct BackupArgs {
    /// The organization to back up
    #[arg(value_name = "ORG_ID", add = ArgValueCompleter::new(completers::orgs))]
    org: String,
    /// The archive to write: a new file, never replaced
    #[arg(long, value_name = "PATH")]
    out: PathBuf,
    /// The office, when the organization has more than one on this instance
    #[arg(long, value_name = "OFFICE_ID", add = ArgValueCompleter::new(completers::offices))]
    office: Option<String>,
    /// Encrypt the archive with a passphrase (Argon2id and AES-256-GCM)
    #[arg(long)]
    encrypt: bool,
    #[command(flatten)]
    passphrase: PassphraseArgs,
    #[command(flatten)]
    instance: InstanceArgs,
}

impl BackupArgs {
    /// Whether the passphrase may be asked on the terminal.
    pub(crate) fn reads_a_secret(&self) -> bool {
        self.encrypt && !self.passphrase.given()
    }
}

#[derive(Args, Debug)]
#[command(after_help = RESTORE_EXAMPLES)]
pub(crate) struct RestoreArgs {
    /// The archive to restore in place over the organization's stopped office (with --org)
    #[arg(
        value_name = "ARCHIVE",
        required_unless_present = "verify",
        conflicts_with = "verify"
    )]
    archive: Option<PathBuf>,
    /// Verify this archive and change nothing: every digest, the notes database's integrity,
    /// the journal head and a disposable rebuild of the office's projections
    #[arg(long, value_name = "ARCHIVE")]
    verify: Option<PathBuf>,
    /// With --verify: extract the verified archive into this new or empty folder
    #[arg(long, value_name = "DIR", requires = "verify")]
    to: Option<PathBuf>,
    /// With --verify: make the disposable check copy in this folder of yours [default: beside
    /// --to, else beside the archive, else your own temporary folder; never the shared one]
    #[arg(long, value_name = "DIR", requires = "verify")]
    work_dir: Option<PathBuf>,
    /// The organization whose office the archive replaces
    #[arg(
        long,
        value_name = "ORG_ID",
        required_unless_present = "verify",
        conflicts_with = "verify",
        add = ArgValueCompleter::new(completers::orgs)
    )]
    org: Option<String>,
    /// The office, when the organization has more than one on this instance
    #[arg(
        long,
        value_name = "OFFICE_ID",
        conflicts_with = "verify",
        add = ArgValueCompleter::new(completers::offices)
    )]
    office: Option<String>,
    #[command(flatten)]
    passphrase: PassphraseArgs,
    #[command(flatten)]
    yes: YesArgs,
    #[command(flatten)]
    instance: InstanceArgs,
}

impl RestoreArgs {
    pub(crate) fn yes(&self) -> bool {
        self.yes.yes
    }

    /// Whether the passphrase of an encrypted archive may be asked on the terminal.
    pub(crate) fn reads_a_secret(&self) -> bool {
        !self.passphrase.given()
    }
}

/// The office home of the selected instance.
fn office_home(instance: &InstanceArgs) -> Result<PathBuf, CliError> {
    let target = crate::selection::select(instance.name.as_deref(), None)?;
    Ok(org_host_setup::instance_office_home(target.instance())?)
}

/// A library refusal as the command line reports it.
fn cli_error(error: BackupError) -> CliError {
    let message = error.to_string();
    match error {
        BackupError::Usage(_) => CliError::usage(message),
        BackupError::PassphraseRequired => CliError::usage(message).with_help(
            "give --passphrase-file <path> or --passphrase-fd <fd>, or run it on a terminal",
        ),
        BackupError::NotFound(_) => {
            CliError::failed("backup::not_found", message).with_help("agentic-org org list")
        }
        BackupError::Live { .. } => CliError::failed("backup::office_running", message)
            .with_help("agentic-org service stop"),
        BackupError::Invalid(_) => CliError::failed("backup::invalid_archive", message),
        BackupError::Authentication => CliError::failed("backup::authentication", message),
        BackupError::Refused(_) => CliError::failed("backup::refused", message),
        BackupError::Io { .. } => CliError::failed("backup::io", message),
        BackupError::Office { .. } => CliError::failed("backup::office", message),
    }
}

/// Shows `json` as a card, or prints it under `--json`. A nested object (`rebuilt`,
/// `live_journal`) is one row of its own `key value` pairs, sorted by key.
fn show(ctx: &mut ExecutionContext, id: &str, json: &serde_json::Value) -> Result<(), CliError> {
    use org_cli_shell::output::Cell;
    let mut cells = Vec::new();
    if let serde_json::Value::Object(fields) = json {
        for (key, value) in fields {
            if let Some(text) = card_text(value) {
                cells.push((key.as_str(), Cell::plain(text)));
            }
        }
    }
    Ok(ctx.render_detail(id, &cells, json)?)
}

/// The card text of one field: `None` for a null (left out), an object's non-null fields as
/// `key value` pairs separated by commas.
fn card_text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Null => None,
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Object(fields) => {
            let pairs: Vec<String> = fields
                .iter()
                .filter_map(|(key, value)| card_text(value).map(|text| format!("{key} {text}")))
                .collect();
            (!pairs.is_empty()).then(|| pairs.join(", "))
        }
        other => Some(other.to_string()),
    }
}
