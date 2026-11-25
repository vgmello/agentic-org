//! Human CLI dispatch and host composition. No agent CLI or office data ownership.

#![deny(unsafe_code)]

mod admin;
mod context;
mod office;
mod org;
mod project;
mod provider;
mod release;
mod serve;
mod service;
mod setup;
mod sign_in;

use std::collections::BTreeMap;

const HELP: &str = "Agentic Org\n\
Use when: operating a computer's Agentic Org host, organizations, offices, or signed-in operator workflows (managed AI agents use `agent-os` instead).\n\n\
Usage: agentic-org <command>\n\n\
Commands:\n  \
serve      Run the installed service host in the foreground\n             \
Use when: running the host directly in a container, terminal, or service unit.\n  \
setup      Set up this computer's instance and register its service\n             \
Use when: initializing a new instance, registering its OS service, or enabling HQ mTLS federation.\n  \
sign-in    Sign in from this terminal via a browser approval link (no password)\n             \
Use when: authenticating a terminal or running one office command (`-- <cmd...>`) without passwords.\n  \
context    context show|use|clear: manage saved default instance and office\n             \
Use when: viewing, pinning, or clearing your default `--name`/`--office` target.\n  \
admin      admin reset | admin tls set: administrator recovery and HTTPS TLS rotation\n             \
Use when: issuing a fresh admin registration link or rotating an HTTPS certificate pair.\n  \
org        org create|list: create or list organizations on this host\n             \
Use when: creating an HQ organization from a Git repo or listing visible organizations.\n  \
office     office list|restart|invite|invitations|approve|decline|cancel|remove|sync-status|join|remove-pending|re-enroll\n             \
Use when: listing/restarting offices or running HQ/satellite mTLS enrollment and sync checks.\n  \
project    project list|inspect|create: list, inspect, or create projects\n             \
Use when: discovering, inspecting, or creating organization projects and owning teams.\n  \
provider   provider create|list|bind|bindings|models: manage AI provider connections and local office bindings\n             \
Use when: configuring org-wide Claude/Codex/Agy connections on HQ or binding local runtimes on HQ/satellite offices.\n  \
release    release install|status: verify and pin a signed release bundle into this host's release store\n             \
Use when: installing or upgrading prebuilt signed binaries on macOS or Linux without compiling from source.\n  \
msg        msg send|inbox|read|reply|ask|accept|withdraw|list|wait: message people and agents across offices\n             \
Use when: sending DMs, channel messages, or replies to local or remote office agents from your terminal.\n  \
channel    channel list|join|leave|summarize: manage organization messaging channels\n             \
Use when: discovering or inspecting shared channels across offices.\n  \
hr         hr hire|list|inspect|confirm|withdraw: hire and manage agents across HQ and satellite offices\n             \
Use when: hiring new agents onto HQ or a remote satellite office from the terminal.\n  \
agent      agent list|inspect|pause|resume|offboard: inspect or manage hired agents\n             \
Use when: checking agent status, execution settings, or lifecycle state.\n  \
team       team list|inspect|create|update: manage organization teams\n             \
Use when: discovering or creating teams before hiring agents or creating projects.\n  \
role       role list|inspect|create|update: manage role catalog entries\n             \
Use when: inspecting or creating reusable role definitions for hiring.\n  \
who        List people and agents with live presence and handles\n             \
Use when: discovering handles and online status across HQ and satellite offices.\n  \
repo       repo register|list|attach|status|push: manage repositories and push task branches\n             \
Use when: registering/attaching Git repos, checking office cache status, or pushing task branches.\n  \
worktree   worktree list|prepare: list or prepare isolated task worktrees\n             \
Use when: listing or preparing isolated Git worktrees for task work.\n  \
task       task create|list|inspect|update|transition|assign|comment|watch|unwatch|history|deps|dep-add|dep-remove|labels\n             \
Use when: creating, filtering, updating, assigning, or tracking tasks over an approved office channel.\n  \
checklist  checklist add|check|edit|remove|move: manage task checklists\n             \
Use when: adding, checking, editing, removing, or reordering task checklist steps.\n  \
task-note  task-note add|list|get|update|remove|publish (alias: `task note`)\n             \
Use when: recording private task notes or publishing a copy as a public task comment.\n  \
service    service start|stop|status: start, stop, or inspect the instance's registered service\n             \
Use when: managing the OS background service created by `agentic-org setup`.\n\n\
Options:\n  \
--help     Show this help\n  \
--version  Show the release version\n\n\
Satellite enrollment is initiated from HQ (`office invite`) and completed on the satellite (`office join`) and HQ (`office approve`).";

pub(crate) const SERVE_HELP: &str = "Run the installed service host: agentic-org serve [--name <instance>]\n\
Use when: running an already-set-up host in the foreground (directly or under a service manager).\n\
In the foreground from a setup file (a container): agentic-org serve --setup <file> [--name <instance>]\n\
Use when: initializing (idempotently) and running a foreground/container instance from one JSON config file.\n\
Development builds also support --home, --server-bin and --app-team-id.";

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
        [verb, target, rest @ ..] if verb == "help" => {
            if matches!(target.as_str(), "help" | "--help" | "-h") {
                println!("{HELP}");
                Ok(())
            } else {
                let mut forwarded = Vec::with_capacity(rest.len() + 2);
                forwarded.push(target.clone());
                forwarded.extend(
                    rest.iter()
                        .filter(|arg| !matches!(arg.as_str(), "--help" | "-h"))
                        .cloned(),
                );
                forwarded.push("--help".to_owned());
                run(forwarded)
            }
        }
        [flag] if matches!(flag.as_str(), "--version" | "-V") => {
            println!("agentic-org {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        [command, flag] if command == "serve" && matches!(flag.as_str(), "--help" | "-h") => {
            println!("{SERVE_HELP}");
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
        [command, rest @ ..] if command == "provider" => provider::run(rest),
        [command, rest @ ..] if command == "release" => release::run(rest),
        [command, ..]
            if matches!(
                command.as_str(),
                "task"
                    | "checklist"
                    | "task-note"
                    | "repo"
                    | "worktree"
                    | "msg"
                    | "channel"
                    | "hr"
                    | "agent"
                    | "team"
                    | "role"
                    | "person"
                    | "who"
                    | "session"
                    | "grant"
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
    use super::{run, HELP, SERVE_HELP};

    const CLI_REFERENCE: &str = include_str!("../../../docs/cli-reference.md");

    #[test]
    fn help_subcommand_routes_to_command_help() {
        for args in [
            vec!["help"],
            vec!["help", "help"],
            vec!["help", "setup"],
            vec!["help", "serve"],
            vec!["help", "org"],
            vec!["help", "org", "create"],
            vec!["help", "office"],
            vec!["help", "office", "invite"],
            vec!["help", "project"],
            vec!["help", "project", "create"],
            vec!["help", "provider"],
            vec!["help", "provider", "bind"],
            vec!["help", "release"],
            vec!["help", "release", "install"],
            vec!["help", "msg"],
            vec!["help", "channel"],
            vec!["help", "hr"],
            vec!["help", "agent"],
            vec!["help", "team"],
            vec!["help", "role"],
            vec!["help", "who"],
            vec!["help", "sign-in"],
            vec!["help", "context"],
            vec!["help", "context", "show"],
            vec!["help", "context", "use"],
            vec!["help", "context", "clear"],
            vec!["help", "recovery"],
            vec!["help", "recovery", "status"],
            vec!["help", "recovery", "release"],
            vec!["help", "admin"],
            vec!["help", "admin", "reset"],
            vec!["help", "admin", "tls"],
            vec!["help", "admin", "tls", "set"],
            vec!["help", "service"],
            vec!["help", "service", "status"],
            vec!["help", "repo"],
            vec!["help", "worktree"],
            vec!["help", "task"],
            vec!["help", "task", "create"],
            vec!["help", "checklist"],
            vec!["help", "task-note"],
        ] {
            let owned: Vec<String> = args.into_iter().map(str::to_owned).collect();
            assert_eq!(run(owned), Ok(()), "expected help to succeed");
        }
    }

    #[test]
    fn every_agentic_org_help_block_includes_use_when_guidance() {
        for (name, help, min_cues) in [
            ("root HELP", HELP, 15usize),
            ("SERVE_HELP", SERVE_HELP, 2),
            ("setup::HELP", crate::setup::HELP, 2),
            ("admin::HELP", crate::admin::HELP, 1),
            ("service::HELP", crate::service::HELP, 4),
            ("ORG_HELP", org_host::setup_flow::ORG_HELP, 3),
            ("OFFICE_HELP", org_host::setup_flow::OFFICE_HELP, 13),
            ("PROJECT_HELP", org_host::setup_flow::PROJECT_HELP, 4),
            ("PROVIDER_HELP", org_host::setup_flow::PROVIDER_HELP, 6),
            ("release::HELP", crate::release::HELP, 3),
            ("SIGN_IN_HELP", org_host::setup_flow::SIGN_IN_HELP, 1),
            ("CONTEXT_HELP", org_host::setup_flow::CONTEXT_HELP, 4),
            (
                "ADMIN_TLS_SET_HELP",
                org_host::setup_flow::ADMIN_TLS_SET_HELP,
                1,
            ),
        ] {
            let count = help.matches("Use when:").count();
            assert!(
                count >= min_cues,
                "{name} expected at least {min_cues} `Use when:` cue(s), found {count}:\n{help}"
            );
        }
    }

    fn assert_cli_reference_entry_has_use_when(needle: &str) {
        let anchored = format!("\n- {needle}");
        let Some(pos) = CLI_REFERENCE.find(&anchored) else {
            panic!("docs/cli-reference.md missing bullet entry `{needle}`");
        };
        let rest = &CLI_REFERENCE[pos + 1..];
        let next_entry = ["\n- `", "\n### ", "\n## ", "\n---"]
            .into_iter()
            .filter_map(|delim| rest[1..].find(delim).map(|offset| offset + 1))
            .min()
            .unwrap_or(rest.len());
        let block = &rest[..next_entry];
        assert!(
            block.contains("Use when:"),
            "docs/cli-reference.md entry `{needle}` is missing `Use when:` guidance:\n{block}"
        );
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
            "provider",
            "release",
            "msg",
            "channel",
            "hr",
            "agent",
            "team",
            "role",
            "who",
            "repo",
            "worktree",
            "task",
            "checklist",
            "task-note",
            "service",
        ] {
            assert!(HELP.contains(command), "top-level HELP missing `{command}`");
            assert_cli_reference_entry_has_use_when(&format!("`agentic-org {command}"));
        }
        assert_cli_reference_entry_has_use_when("`agentic-org setup --non-interactive");
        for sub in ["start", "stop", "status"] {
            assert_cli_reference_entry_has_use_when(&format!("`agentic-org service {sub}"));
        }
        for sub in ["show", "use", "clear"] {
            assert_cli_reference_entry_has_use_when(&format!("`agentic-org context {sub}"));
        }
        for sub in ["reset", "tls set"] {
            assert_cli_reference_entry_has_use_when(&format!("`agentic-org admin {sub}"));
        }
        for sub in ["create", "list"] {
            assert_cli_reference_entry_has_use_when(&format!("`agentic-org org {sub}"));
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
            assert_cli_reference_entry_has_use_when(&format!("`agentic-org office {sub}"));
        }
        for sub in ["list", "inspect", "create"] {
            assert_cli_reference_entry_has_use_when(&format!("`agentic-org project {sub}"));
        }
        // Core `agent-os` quick-pointer entries with `Use when:` intent cues.
        assert!(
            CLI_REFERENCE.contains("## `agent-os` Quick Pointer"),
            "docs/cli-reference.md missing `## `agent-os` Quick Pointer` section"
        );
        for pointer in [
            "`agent-os msg",
            "`agent-os channel",
            "`agent-os task ",
            "`agent-os checklist",
            "`agent-os task-note",
            "`agent-os repo",
            "`agent-os worktree",
            "`agent-os status",
            "`agent-os who`",
            "`agent-os process",
            "`agent-os skill",
            "`agent-os session",
            "`agent-os grant",
            "`agent-os budget",
            "`agent-os org|team|person|agent|project|role|hr|member",
        ] {
            assert_cli_reference_entry_has_use_when(pointer);
        }
        assert!(
            CLI_REFERENCE.contains("DM") || CLI_REFERENCE.contains("direct message"),
            "docs/cli-reference.md agent-os quick pointer should explain how DMs are sent"
        );
    }
}
