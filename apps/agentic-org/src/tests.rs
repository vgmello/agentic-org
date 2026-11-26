//! The grammar's contract: help routing, a `Use when:` cue and examples on every command,
//! and the CLI reference documenting every command.

use std::ffi::OsString;

use clap::error::ErrorKind;

use crate::{cli, help_routing};

const CLI_REFERENCE: &str = include_str!("../../../docs/cli-reference.md");

fn words(list: &[&str]) -> Vec<OsString> {
    std::iter::once("agentic-org")
        .chain(list.iter().copied())
        .map(OsString::from)
        .collect()
}

/// `help <command...>` is `<command...> --help`: clap's help for a host command, and the
/// command's own (`--help` reaches it as a word) for `serve`, `setup`, `recovery` and the
/// office resources.
#[test]
fn help_subcommand_routes_to_command_help() {
    for args in [
        &["help"][..],
        &["help", "setup"],
        &["help", "serve"],
        &["help", "org"],
        &["help", "org", "create"],
        &["help", "office"],
        &["help", "office", "invite"],
        &["help", "project", "create"],
        &["help", "provider", "bind"],
        &["help", "release", "install"],
        &["help", "msg"],
        &["help", "task", "create"],
        &["help", "who"],
        &["help", "sign-in"],
        &["help", "context", "use"],
        &["help", "recovery"],
        &["help", "admin", "tls", "set"],
        &["help", "service", "status"],
        &["help", "status"],
        &["help", "completion"],
    ] {
        let routed = help_routing(words(args));
        assert_eq!(
            routed.last().map(OsString::as_os_str),
            Some("--help".as_ref()),
            "{args:?}"
        );
        match cli::command().try_get_matches_from(&routed) {
            Err(error) => assert_eq!(error.kind(), ErrorKind::DisplayHelp, "{args:?}: {error}"),
            Ok(matches) => {
                // A command that reads its own words got `--help` as one of them.
                let (_, sub) = matches.subcommand().expect("a command");
                let passed: Vec<&String> = sub
                    .get_many::<String>("args")
                    .into_iter()
                    .flatten()
                    .collect();
                assert!(
                    passed.iter().any(|word| *word == "--help"),
                    "{args:?}: {passed:?}"
                );
            }
        }
    }
}

/// Every command and verb says when to use it and shows examples (REQ-CLI-01, AX-7).
#[test]
fn every_command_help_has_a_use_when_cue_and_examples() {
    fn walk(command: &mut clap::Command, path: &str, checked: &mut usize) {
        let name = format!("{path} {}", command.get_name());
        let passthrough = command.is_disable_help_flag_set();
        let help = command.render_long_help().to_string();
        if !path.is_empty() && !passthrough {
            assert!(
                help.contains("Use when:"),
                "{name} has no `Use when:` cue:\n{help}"
            );
            if !command.has_subcommands() {
                assert!(
                    help.contains("Examples:"),
                    "{name} has no examples:\n{help}"
                );
            }
            *checked += 1;
        }
        for sub in command.get_subcommands_mut() {
            walk(sub, &name, checked);
        }
    }
    let mut command = cli::command();
    command.build();
    let mut checked = 0;
    walk(&mut command, "", &mut checked);
    assert!(checked >= 45, "only {checked} commands checked");
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
    let help = cli::command().render_long_help().to_string();
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
        "status",
        "completion",
        "recovery",
    ] {
        assert!(help.contains(command), "top-level help missing `{command}`");
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
    for sub in ["create", "list", "inspect"] {
        assert_cli_reference_entry_has_use_when(&format!("`agentic-org org {sub}"));
    }
    let office = cli::command()
        .find_subcommand_mut("office")
        .expect("office")
        .render_long_help()
        .to_string();
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
        assert!(office.contains(sub), "office help missing `{sub}`");
        assert_cli_reference_entry_has_use_when(&format!("`agentic-org office {sub}"));
    }
    for sub in ["list", "inspect", "create"] {
        assert_cli_reference_entry_has_use_when(&format!("`agentic-org project {sub}"));
    }
    assert_cli_reference_entry_has_use_when(
        "`agentic-org directory|person|member|session|grant|process|skill|budget",
    );
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
