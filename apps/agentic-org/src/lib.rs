//! Human CLI dispatch and host composition. No agent CLI or office data ownership.
mod serve;

const HELP: &str = "Agentic Org\n\nUsage: agentic-org <command>\n\nCommands:\n  serve      Run the selected service host in the foreground\n  adopt      Stage and adopt on an explicit owner-authenticated instance\n  recovery   Inspect or release native HQ recovery material\n  setup      Create or resume HQ using a trusted complete release\n\nOptions:\n  --help     Show this help\n  --version  Show the release version\n\nSatellite enrollment is initiated separately from HQ.";

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
        [command, rest @ ..] if command == "adopt" => org_host::human::adopt(rest),
        [command, rest @ ..] if command == "recovery" => org_host::human::recovery(rest),
        [command, rest @ ..] if command == "setup" => org_host::human::setup(rest),
        _ => Err(format!("unrecognized command or arguments\n{HELP}")),
    }
}
