//! Human CLI dispatch and host composition. No agent CLI or office data ownership.

#![deny(unsafe_code)]

mod admin;
mod context;
mod office;
mod org;
mod project;
mod serve;
mod service;
mod setup;
mod sign_in;

use std::collections::BTreeMap;

const HELP: &str = "Agentic Org\n\nUsage: agentic-org <command>\n\nCommands:\n  serve      Run the installed service host in the foreground\n  setup      Set up this computer's instance and register its service\n  sign-in    Sign in from this terminal via a browser approval link (no password)\n  context    context show|use|clear: manage saved default instance and office\n  admin      admin reset: a new administrator registration link; admin tls set: replace the TLS pair\n  org        org create|list: create or list organizations on this host\n  office     office list|restart|invite|invitations|approve|decline|cancel|remove|sync-status|join|remove-pending|re-enroll\n  project    project list|inspect|create: list, inspect, or create projects\n  repo       repo register|list|attach|status|push: manage repositories and push task branches\n  worktree   worktree list|prepare: list or prepare isolated task worktrees\n  task       task create|list|inspect|update|transition|assign|comment|watch|deps|labels\n  checklist  checklist add|check|edit|remove|move: manage task checklists\n  service    start, stop or show the instance's service\n\nOptions:\n  --help     Show this help\n  --version  Show the release version\n\nSatellite enrollment is initiated from HQ (`office invite`) and completed on the satellite (`office join`) and HQ (`office approve`).";

/// Dispatches a human CLI command.
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
        [verb, target, ..] if verb == "help" => {
            if matches!(target.as_str(), "help" | "--help" | "-h") {
                println!("{HELP}");
                Ok(())
            } else {
                run(vec![target.clone(), "--help".to_owned()])
            }
        }
        [flag] if matches!(flag.as_str(), "--version" | "-V") => {
            println!("agentic-org {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        [command, flag] if command == "serve" && matches!(flag.as_str(), "--help" | "-h") => {
            println!("Run the installed service host: agentic-org serve [--name <instance>]\nIn the foreground from a setup file (a container): agentic-org serve --setup <file> [--name <instance>]\nDevelopment builds also support --home, --server-bin and --app-team-id.");
            Ok(())
        }
        [command, rest @ ..] if command == "serve" => serve::run(rest.to_vec()),
        [command, rest @ ..] if command == "recovery" => org_host::human::recovery(rest),
        [command, rest @ ..] if command == "setup" => setup::run(rest),
        [command, rest @ ..] if command == "sign-in" => sign_in::run(rest),
        [command, rest @ ..] if command == "context" => context::run(rest),
        [command, rest @ ..] if command == "admin" => admin::run(rest),
        [command, rest @ ..] if command == "org" => org::run(rest),
        [command, rest @ ..] if command == "office" => office::run(rest),
        [command, rest @ ..] if command == "project" => project::run(rest),
        [command, ..]
            if matches!(
                command.as_str(),
                "task" | "checklist" | "task-note" | "repo" | "worktree"
            ) =>
        {
            sign_in::run_office_command(&arguments)
        }
        [command, rest @ ..] if command == "service" => service::run(rest),
        _ => Err(format!("unrecognized command or arguments\n{HELP}")),
    }
}

/// `--option value` pairs, each at most once, from `allowed` only.
pub(crate) fn options(
    arguments: &[String],
    allowed: &[&str],
) -> Result<BTreeMap<String, String>, String> {
    let mut options = BTreeMap::new();
    let mut arguments = arguments.iter();
    while let Some(flag) = arguments.next() {
        if !allowed.contains(&flag.as_str()) {
            return Err(format!("unrecognized argument {flag:?}"));
        }
        let value = arguments
            .next()
            .filter(|v| !v.starts_with("--"))
            .ok_or_else(|| format!("{flag} needs a value"))?;
        if options.insert(flag.clone(), value.clone()).is_some() {
            return Err(format!("{flag} is given more than once"));
        }
    }
    Ok(options)
}

#[cfg(test)]
mod tests {
    use super::{run, HELP};

    const CLI_REFERENCE: &str = include_str!("../../../docs/cli-reference.md");

    #[test]
    fn help_subcommand_routes_to_command_help() {
        for args in [
            vec!["help"],
            vec!["help", "help"],
            vec!["help", "setup"],
            vec!["help", "serve"],
            vec!["help", "org"],
            vec!["help", "office"],
            vec!["help", "project"],
            vec!["help", "sign-in"],
            vec!["help", "context"],
            vec!["help", "admin"],
            vec!["help", "service"],
        ] {
            let owned: Vec<String> = args.into_iter().map(str::to_owned).collect();
            assert_eq!(run(owned), Ok(()), "expected help to succeed");
        }
    }

    #[test]
    fn cli_reference_documents_every_top_level_and_office_command() {
        for command in [
            "serve",
            "setup",
            "sign-in",
            "context",
            "admin",
            "org",
            "office",
            "project",
            "repo",
            "worktree",
            "task",
            "checklist",
            "service",
        ] {
            assert!(HELP.contains(command), "top-level HELP missing `{command}`");
            assert!(
                CLI_REFERENCE.contains(&format!("agentic-org {command}")),
                "docs/cli-reference.md missing `agentic-org {command}`"
            );
        }
        for sub in [
            "list",
            "restart",
            "invite",
            "invitations",
            "approve",
            "decline",
            "cancel",
            "remove",
            "sync-status",
            "join",
            "remove-pending",
            "re-enroll",
        ] {
            assert!(
                org_host::setup_flow::OFFICE_HELP.contains(sub),
                "OFFICE_HELP missing `{sub}`"
            );
            assert!(
                CLI_REFERENCE.contains(&format!("agentic-org office {sub}")),
                "docs/cli-reference.md missing `agentic-org office {sub}`"
            );
        }
        for sub in ["create", "list"] {
            assert!(
                CLI_REFERENCE.contains(&format!("agentic-org org {sub}")),
                "docs/cli-reference.md missing `agentic-org org {sub}`"
            );
        }
    }
}
