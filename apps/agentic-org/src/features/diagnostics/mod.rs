//! Operator diagnostics and clean uninstall slice (`TASK-60`, FC-022 & FC-023):
//! - `agentic-org logs`: bounded, symlink-safe, secret-redacted host & office logs
//! - `agentic-org doctor`: host, runtime, security, AppArmor/userns, and org-state integrity checks
//! - `agentic-org support`: deterministic, 100% secret-redacted JSON diagnostic bundle
//! - `agentic-org uninstall`: clean instance teardown clearing `host-registry.created`

mod doctor;
mod doctor_host;
mod logs;
mod org_integrity;
mod org_integrity_checks;
mod paths;
mod redact;
mod redact_fields;
mod redact_pem;
mod sqlite_read;
mod sqlite_wal;
mod support;
mod uninstall;
mod uninstall_guard;
mod yaml_front;

#[cfg(test)]
mod tests;

use std::path::PathBuf;

use clap::Args;
use clap_complete::engine::ArgValueCompleter;

use crate::args::{InstanceArgs, YesArgs};
use crate::completers;

pub(crate) use doctor::run as doctor;
pub(crate) use logs::run as logs;
pub(crate) use support::run as support;
pub(crate) use uninstall::run as uninstall;

const LOGS_EXAMPLES: &str = "Examples:
  agentic-org logs
  agentic-org logs --instance lab --lines 100
  agentic-org logs --office 0193a5c0-0000-7000-8000-000000000001 --json
  agentic-org logs -q";

const DOCTOR_EXAMPLES: &str = "Examples:
  agentic-org doctor
  agentic-org doctor --instance lab --json
  agentic-org doctor --org-dir ./durable/state --json
  agentic-org doctor -q";

const SUPPORT_EXAMPLES: &str = "Examples:
  agentic-org support --output ./support-bundle.json
  agentic-org support --instance lab --lines 500 --output ./lab-support.json --json
  agentic-org support --json";

const UNINSTALL_EXAMPLES: &str = "Examples:
  agentic-org uninstall --dry-run
  agentic-org uninstall --instance lab --yes
  agentic-org uninstall --instance lab --yes --json";

#[derive(Args, Debug)]
#[command(after_help = LOGS_EXAMPLES)]
pub(crate) struct LogsArgs {
    #[command(flatten)]
    pub(super) instance: InstanceArgs,
    /// Filter logs to a single office ID or socket handle
    #[arg(long, value_name = "OFFICE_ID", add = ArgValueCompleter::new(completers::offices))]
    pub(super) office: Option<String>,
    /// Maximum number of recent redacted lines to emit (1..=2000)
    #[arg(long, value_name = "N", default_value_t = logs::DEFAULT_LINES)]
    pub(super) lines: usize,
}

#[derive(Args, Debug)]
#[command(after_help = DOCTOR_EXAMPLES)]
pub(crate) struct DoctorArgs {
    #[command(flatten)]
    pub(super) instance: InstanceArgs,
    /// Filter office integrity checks to a single office ID
    #[arg(long, value_name = "OFFICE_ID", add = ArgValueCompleter::new(completers::offices))]
    pub(super) office: Option<String>,
    /// Inspect an organization durable/state directory directly offline
    #[arg(long, value_name = "PATH")]
    pub(super) org_dir: Option<PathBuf>,
}

#[derive(Args, Debug)]
#[command(after_help = SUPPORT_EXAMPLES)]
pub(crate) struct SupportArgs {
    #[command(flatten)]
    pub(super) instance: InstanceArgs,
    /// Write the redacted JSON bundle atomically (0600) to this file instead of stdout
    #[arg(long, value_name = "PATH")]
    pub(super) output: Option<PathBuf>,
    /// Maximum number of recent redacted log lines to include (1..=2000)
    #[arg(long, value_name = "N", default_value_t = logs::DEFAULT_LINES)]
    pub(super) lines: usize,
}

#[derive(Args, Debug)]
#[command(after_help = UNINSTALL_EXAMPLES)]
pub(crate) struct UninstallArgs {
    #[command(flatten)]
    pub(super) instance: InstanceArgs,
    #[command(flatten)]
    pub(super) yes: YesArgs,
}

impl UninstallArgs {
    pub(crate) fn yes(&self) -> bool {
        self.yes.yes
    }
}
