//! Thin composition root for `agent-os`. All grammar, schema, envelope, and office-client
//! logic lives in `org-agent-cli` + `org-command`.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    ExitCode::from(org_agent_cli::run(&args))
}
