//! `agentic-org release verify|install|status|check|update|rollback` (`TASK-121`, `TASK-134`,
//! ADR-0009): verify and atomically stage signed release bundles, inspect active pins, check
//! signed release pointers, run idle-drained pull-verified auto-updates, and roll back safely.

mod check;
mod install;
mod rollback;
mod status;
mod update;
mod verify;

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use org_cli_shell::context::ExecutionContext;
use org_cli_shell::output::{Cell, Style};
use org_host::release::{default_update_state_dir, ReleasePin, DEFAULT_POINTER_URL};
use serde::Serialize;

use crate::error::CliError;

const INSTALL_EXAMPLES: &str = "Examples:
  agentic-org release install ./agentic-org-1.2.0.tar.gz
  agentic-org release install ./agentic-org-1.2.0.tar.gz --dry-run
  agentic-org release install ./agentic-org-1.2.0.tar.gz --no-restart
  agentic-org release install ./release-dir --store /srv/agentic-org/releases --json

Each instance's registered service (launchd/systemd) that setup wrote moves to the new
release: a running one is stopped for the install and started again on it, a stopped one
stays stopped. A service definition changed by hand is left untouched, with the steps to
upgrade it.";

const STATUS_EXAMPLES: &str = "Examples:
  agentic-org release status
  agentic-org release status -q
  agentic-org release status --json";

const VERIFY_EXAMPLES: &str = "Examples:
  agentic-org release verify ./agentic-org-1.2.0.tar.gz
  agentic-org release verify ./release-dir --json";

const CHECK_EXAMPLES: &str = "Examples:
  agentic-org release check
  agentic-org release check --pointer-url https://github.com/vgmello/agentic-org/releases/latest/download/release-pointer.v1.json --json";

const UPDATE_EXAMPLES: &str = "Examples:
  agentic-org release update
  agentic-org release update --dry-run --json
  agentic-org release update --drain-timeout 180 --health-timeout 60
  agentic-org release update --pointer-url https://releases.example.com/release-pointer.v1.json --force";

const ROLLBACK_EXAMPLES: &str = "Examples:
  agentic-org release rollback
  agentic-org release rollback --to v0.1.0 --quarantine-current
  agentic-org release rollback --dry-run --json";

#[derive(Args, Debug)]
#[command(
    after_help = "Examples:\n  agentic-org release check --json\n  agentic-org release update\n  agentic-org release rollback\n  agentic-org release verify ./agentic-org-1.2.0.tar.gz\n  agentic-org release install ./agentic-org-1.2.0.tar.gz\n  agentic-org release status --json"
)]
pub(crate) struct ReleaseArgs {
    #[command(subcommand)]
    command: ReleaseCommand,
}

#[derive(Subcommand, Debug)]
enum ReleaseCommand {
    /// Verify the Ed25519 signature, closed inventory and binary hashes without modifying the release store
    /// Use when: checking an offline release archive or directory before installing or activating it.
    #[command(verbatim_doc_comment, after_help = VERIFY_EXAMPLES)]
    Verify {
        /// The release bundle: a `.tar.gz` archive or a release directory
        #[arg(value_name = "BUNDLE")]
        bundle: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Verify the Ed25519 signature, closed inventory and binary hashes, atomically pin the release, and move registered services to it
    /// Use when: installing or upgrading a prebuilt signed Agentic Org release package without compiling from source.
    #[command(verbatim_doc_comment, after_help = INSTALL_EXAMPLES)]
    Install {
        /// The release bundle: a `.tar.gz` archive or a release directory
        #[arg(value_name = "BUNDLE")]
        bundle: String,
        #[command(flatten)]
        store: StoreArgs,
        /// Do not start again the services the install stopped (they still move to the new release)
        #[arg(long)]
        no_restart: bool,
    },
    /// Show the active pinned release ID, sequence, manifest SHA-256 and installed executables
    /// Use when: verifying which release version is currently installed and active on this host.
    #[command(verbatim_doc_comment, after_help = STATUS_EXAMPLES)]
    Status {
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Fetch and verify a signed release pointer without downloading or modifying the release store
    /// Use when: checking whether a newer signed release is available for this host's platform and architecture.
    #[command(verbatim_doc_comment, after_help = CHECK_EXAMPLES)]
    Check {
        /// HTTPS URL of the signed release pointer JSON
        #[arg(long, value_name = "URL", default_value = DEFAULT_POINTER_URL)]
        pointer_url: String,
        /// Optional explicit HTTPS URL for the raw 64-byte Ed25519 pointer signature
        #[arg(long, value_name = "URL")]
        signature_url: Option<String>,
        /// Metadata fetch timeout in seconds
        #[arg(long, value_name = "SECS", default_value_t = 30)]
        timeout: u64,
        /// Report quarantined releases as eligible instead of skipped
        #[arg(long)]
        allow_quarantined: bool,
        #[command(flatten)]
        store: StoreArgs,
        #[command(flatten)]
        state: StateDirArgs,
    },
    /// Pull-verify a signed release, idle-drain active agent turns, swap services, verify post-restart health, and roll back automatically on failure
    /// Use when: performing zero-SSH pull-verified upgrades on macOS HQ or remote Linux satellites.
    #[command(verbatim_doc_comment, after_help = UPDATE_EXAMPLES)]
    Update {
        /// HTTPS URL of the signed release pointer JSON
        #[arg(long, value_name = "URL", default_value = DEFAULT_POINTER_URL)]
        pointer_url: String,
        /// Optional explicit HTTPS URL for the raw 64-byte Ed25519 pointer signature
        #[arg(long, value_name = "URL")]
        signature_url: Option<String>,
        /// Seconds to wait for active agent turns to drain cleanly before refusing the swap
        #[arg(long, value_name = "SECS", default_value_t = 120)]
        drain_timeout: u64,
        /// Seconds to wait for post-restart host health checks (`hello` + `instance.status`) before rolling back
        #[arg(long, value_name = "SECS", default_value_t = 60)]
        health_timeout: u64,
        /// Bundle download timeout in seconds
        #[arg(long, value_name = "SECS", default_value_t = 300)]
        fetch_timeout: u64,
        /// Bypass idle-drain wait even when agent turns are active
        #[arg(long)]
        force: bool,
        /// Do not restart stopped services after pinning and redefining them
        #[arg(long)]
        no_restart: bool,
        /// Retry a release previously quarantined after a failed health check
        #[arg(long)]
        allow_quarantined: bool,
        /// Operate in foreground mode (only permitted when zero registered system services exist)
        #[arg(long)]
        foreground: bool,
        #[command(flatten)]
        store: StoreArgs,
        #[command(flatten)]
        state: StateDirArgs,
    },
    /// Idle-drain and roll back the active pin and registered services to the previous known-good release
    /// Use when: reverting an active release to a previously pinned version in this host's release store.
    #[command(verbatim_doc_comment, after_help = ROLLBACK_EXAMPLES)]
    Rollback {
        /// Specific previously-committed release ID to restore (defaults to the previous pin)
        #[arg(long, value_name = "RELEASE_ID")]
        to: Option<String>,
        /// Seconds to wait for active agent turns to drain cleanly before refusing rollback
        #[arg(long, value_name = "SECS", default_value_t = 120)]
        drain_timeout: u64,
        /// Seconds to wait for post-rollback host health checks
        #[arg(long, value_name = "SECS", default_value_t = 60)]
        health_timeout: u64,
        /// Bypass idle-drain wait even when agent turns are active
        #[arg(long)]
        force: bool,
        /// Do not restart stopped services after rolling back
        #[arg(long)]
        no_restart: bool,
        /// Quarantine the current release so subsequent auto-updates skip it
        #[arg(long)]
        quarantine_current: bool,
        /// Operate in foreground mode (only permitted when zero registered system services exist)
        #[arg(long)]
        foreground: bool,
        #[command(flatten)]
        store: StoreArgs,
        #[command(flatten)]
        state: StateDirArgs,
    },
}

#[derive(Args, Debug)]
struct StoreArgs {
    /// The release store (an absolute path) [default: this account's release store]
    #[arg(long, value_name = "DIR")]
    store: Option<String>,
}

#[derive(Args, Debug)]
struct StateDirArgs {
    /// Dedicated owner-only auto-update checkpoint and quarantine directory (absolute path)
    #[arg(long, value_name = "DIR")]
    state_dir: Option<String>,
}

pub(crate) fn run(args: ReleaseArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    match args.command {
        ReleaseCommand::Verify { bundle, store } => {
            verify::run(&bundle, store.store.as_deref(), ctx)
        }
        ReleaseCommand::Install {
            bundle,
            store,
            no_restart,
        } => install::run(&bundle, store.store.as_deref(), !no_restart, ctx),
        ReleaseCommand::Status { store } => status::run(store.store.as_deref(), ctx),
        ReleaseCommand::Check {
            pointer_url,
            signature_url,
            timeout,
            allow_quarantined,
            store,
            state,
        } => check::run(
            check::CheckArgs {
                pointer_url: &pointer_url,
                signature_url: signature_url.as_deref(),
                store: store.store.as_deref(),
                state_dir: state.state_dir.as_deref(),
                timeout_secs: timeout,
                allow_quarantined,
            },
            ctx,
        ),
        ReleaseCommand::Update {
            pointer_url,
            signature_url,
            drain_timeout,
            health_timeout,
            fetch_timeout,
            force,
            no_restart,
            allow_quarantined,
            foreground,
            store,
            state,
        } => update::run(
            update::UpdateArgs {
                pointer_url: &pointer_url,
                signature_url: signature_url.as_deref(),
                store: store.store.as_deref(),
                state_dir: state.state_dir.as_deref(),
                drain_timeout_secs: drain_timeout,
                health_timeout_secs: health_timeout,
                fetch_timeout_secs: fetch_timeout,
                force,
                restart: !no_restart,
                allow_quarantined,
                foreground,
            },
            ctx,
        ),
        ReleaseCommand::Rollback {
            to,
            drain_timeout,
            health_timeout,
            force,
            no_restart,
            quarantine_current,
            foreground,
            store,
            state,
        } => rollback::run(
            rollback::RollbackArgs {
                to: to.as_deref(),
                store: store.store.as_deref(),
                state_dir: state.state_dir.as_deref(),
                drain_timeout_secs: drain_timeout,
                health_timeout_secs: health_timeout,
                force,
                restart: !no_restart,
                quarantine_current,
                foreground,
            },
            ctx,
        ),
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

fn state_dir_for(
    store: &Path,
    explicit_store: bool,
    explicit_state: Option<&str>,
) -> Result<PathBuf, CliError> {
    match explicit_state {
        Some(raw) => {
            let path = PathBuf::from(raw);
            if !path.is_absolute() {
                return Err(CliError::usage(format!(
                    "--state-dir ({raw}) must be an absolute path"
                )));
            }
            Ok(path)
        }
        None => default_update_state_dir(store, explicit_store)
            .map_err(|e| CliError::from(format!("resolving update state directory: {e}"))),
    }
}

pub(super) struct ResolvedReleaseServices {
    pub(super) manager: Box<dyn org_platform::service::ServiceManager>,
    pub(super) drain_targets: Vec<(String, PathBuf)>,
}

/// Composes the service manager and inspects registered services fail-closed for `release update`
/// and `release rollback` (P1-3).
///
/// Never maps arbitrary `host::service_manager()` errors (such as `refuse_development_service()`
/// or an unavailable `systemd --user` bus) to `ServiceMode::Foreground`, and never swallows
/// `ServiceRefresh::inspect` or per-service inspection errors. Foreground mode is entered ONLY
/// when explicitly opted into via `--foreground` AND zero registered system services exist.
pub(super) fn resolve_release_services(
    store: &Path,
    setup_base: Option<&Path>,
    foreground: bool,
) -> Result<ResolvedReleaseServices, CliError> {
    let manager = if foreground {
        let registered = org_host_setup::ServiceRefresh::registered_setup_instances(setup_base);
        if !registered.is_empty() {
            return Err(CliError::from(format!(
                "refusing `--foreground`: registered system service(s) exist ({}) — omit `--foreground` so they are drained, redefined, and health-checked",
                registered.join(", ")
            )));
        }
        crate::host::foreground_service_manager()?
    } else {
        crate::host::service_manager()?
    };

    let inspected = org_host_setup::ServiceRefresh::inspect(&*manager, store)?;
    let failures = inspected.inspection_failures();
    if !failures.is_empty() {
        let detail = failures
            .into_iter()
            .map(|(instance, why)| format!("{instance}: {why}"))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(CliError::from(format!(
            "cannot inspect registered services for release operation ({detail})"
        )));
    }
    let drain_targets = inspected
        .managed_targets()
        .into_iter()
        .map(|t| (t.instance, t.root))
        .collect();
    Ok(ResolvedReleaseServices {
        manager,
        drain_targets,
    })
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
