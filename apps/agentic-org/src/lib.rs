//! Human CLI dispatch and host composition. No agent CLI or office data ownership.
mod serve;

const HELP: &str = "Agentic Org\n\nUsage: agentic-org <command>\n\nCommands:\n  serve      Run the installed service host in the foreground\n  setup      Create HQ (requires the pending protected bootstrap integration)\n\nOptions:\n  --help     Show this help\n  --version  Show the release version\n\nSatellite enrollment is initiated separately from HQ.";

/// Dispatches a human CLI command, preserving unavailable bootstrap stages.
pub fn run(arguments: Vec<String>) -> Result<(), String> {
    match arguments.as_slice() {
        [] => {
            println!("{HELP}");
            Ok(())
        }
        [flag] if matches!(flag.as_str(), "--help" | "-h" | "help") => {
            println!("{HELP}");
            Ok(())
        }
        [flag] if matches!(flag.as_str(), "--version" | "-V") => {
            println!("agentic-org {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        [command, flag] if command == "serve" && matches!(flag.as_str(), "--help" | "-h") => {
            println!("Run the installed service host: agentic-org serve\nDevelopment builds also support --home, --server-bin and --app-team-id.");
            Ok(())
        }
        [command, rest @ ..] if command == "serve" => serve::run(rest.to_vec()),
        [command, rest @ ..] if command == "recovery" => org_host::human::recovery(rest),
        [command, rest @ ..] if command == "setup" => {
            // Never write a fake HQ, register a broken service or bypass bootstrap authority.
            if rest.iter().any(|arg| {
                matches!(arg.as_str(), "--hq" | "--satellite" | "--invitation")
                    || arg.starts_with("--hq=")
                    || arg.starts_with("--invitation=")
            }) {
                return Err(
                    "setup creates HQ only; use HQ's Add office flow for satellites".into(),
                );
            }
            Err("HQ setup is unavailable: protected administrator/bootstrap and service registration are not wired in v1 yet; no setup state was changed".into())
        }
        _ => Err(format!("unrecognized command or arguments\n{HELP}")),
    }
}
