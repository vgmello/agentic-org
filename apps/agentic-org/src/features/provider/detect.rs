//! `provider detect`: auto-detect installed local CLI runtimes (`claude`, `codex`, `agy`,
//! `gemini`, `kimi`, `pi`).

use clap::Args;
use org_cli_shell::context::ExecutionContext;
use serde::Serialize;

use crate::error::CliError;
use crate::view::{self, column, Column, Show};

pub(super) const EXAMPLES: &str = "Examples:\n  agentic-org provider detect\n  agentic-org provider detect --installed --json\n  agentic-org provider detect --runtime gemini";

#[derive(Args, Debug)]
pub(crate) struct DetectArgs {
    /// Only show runtimes whose executable is installed on this host
    #[arg(long)]
    pub(super) installed: bool,
    /// Require and inspect one specific runtime (`claude`, `codex`, `agy`, `gemini`, `kimi`, `pi`)
    #[arg(long, value_name = "RUNTIME")]
    pub(super) runtime: Option<String>,
}

#[derive(Serialize)]
struct DetectPayload {
    installed_count: usize,
    total_count: usize,
    runtimes: Vec<org_proto::runtimes::DetectedCliRuntime>,
}

const COLUMNS: [Column; 6] = [
    column("RUNTIME", "runtime_id", Show::Id),
    column("NAME", "display_name", Show::Name),
    column("BINARY", "binary_name", Show::Plain),
    column("PROTOCOL", "protocol", Show::Dim),
    column("INSTALLED", "installed", Show::Plain),
    column("PATH", "binary_path", Show::Dim),
];

pub(super) fn run(args: DetectArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    if let Some(runtime_id) = args.runtime.as_deref() {
        let resolved = org_host_setup::resolve_host_runtime_binary(runtime_id, None)
            .map_err(|err| CliError::failed("runtime::detect", err.to_string()))?;
        let canonical = org_proto::runtimes::known_cli_runtime(runtime_id)
            .expect("resolved runtime is in KNOWN_CLI_RUNTIMES");
        let item = org_proto::runtimes::DetectedCliRuntime {
            runtime_id: canonical.id.to_owned(),
            display_name: canonical.display_name.to_owned(),
            binary_name: canonical.binary_name.to_owned(),
            protocol: canonical.protocol.to_owned(),
            provider_type: canonical.provider_type.to_owned(),
            installed: true,
            binary_path: Some(resolved.display().to_string()),
        };
        let payload = DetectPayload {
            installed_count: 1,
            total_count: 1,
            runtimes: vec![item],
        };
        let json = view::json(&payload)?;
        let rows = view::items(&json, "runtimes");
        return view::list(
            ctx,
            rows,
            &json,
            "runtime_id",
            &COLUMNS,
            "No CLI runtimes detected.",
        );
    }

    let all = org_host_setup::detect_host_runtimes(None);
    let total_count = all.len();
    let installed_count = all.iter().filter(|r| r.installed).count();
    let runtimes = if args.installed {
        all.into_iter().filter(|r| r.installed).collect()
    } else {
        all
    };
    let payload = DetectPayload {
        installed_count,
        total_count,
        runtimes,
    };
    let json = view::json(&payload)?;
    let rows = view::items(&json, "runtimes");
    view::list(
        ctx,
        rows,
        &json,
        "runtime_id",
        &COLUMNS,
        "No supported agent CLI runtimes detected on this host.",
    )
}
