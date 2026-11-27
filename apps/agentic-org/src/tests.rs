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
        &["help", "agent", "bind"],
        &["help", "agent", "renew"],
        &["help", "agent", "sessions"],
        &["help", "repo", "set-credential"],
        &["help", "repo", "credential-status"],
        &["help", "repo", "revoke-credential"],
        &["help", "task", "create"],
        &["help", "who"],
        &["help", "sign-in"],
        &["help", "context", "use"],
        &["help", "recovery"],
        &["help", "admin", "tls", "set"],
        &["help", "service", "status"],
        &["help", "status"],
        &["help", "logs"],
        &["help", "doctor"],
        &["help", "support"],
        &["help", "uninstall"],
        &["help", "completion"],
        &["help", "backup"],
        &["help", "restore"],
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
        "note",
        "review",
        "search",
        "service",
        "status",
        "completion",
        "recovery",
        "backup",
        "restore",
    ] {
        assert!(help.contains(command), "top-level help missing `{command}`");
        assert_cli_reference_entry_has_use_when(&format!("`agentic-org {command}"));
    }
    assert_cli_reference_entry_has_use_when("`agentic-org setup --non-interactive");
    assert_cli_reference_entry_has_use_when("`agentic-org restore --verify");
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
    for sub in ["approve", "bind", "renew", "sessions", "revoke"] {
        assert_cli_reference_entry_has_use_when(&format!("`agentic-org agent {sub}"));
    }
    for verb in ["login", "whoami", "logout", "exec"] {
        assert_cli_reference_entry_has_use_when(&format!("`agent-os {verb}"));
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
        "`agent-os context`",
        "`agent-os resume`",
        "`agent-os msg",
        "`agent-os channel",
        "`agent-os task ",
        "`agent-os checklist",
        "`agent-os note",
        "`agent-os review",
        "`agent-os search",
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

/// B-01: the host's saved context and the shell's configuration directory are moved by one
/// and the same variable.
#[test]
fn the_host_and_the_shell_name_the_same_config_variable() {
    assert_eq!(
        org_host_setup::cli_context::CONFIG_DIR_ENV,
        org_cli_shell::paths::CONFIG_DIR_ENV
    );
}

#[test]
fn office_flag_hoisting_handles_json_before_and_after_subcommand_and_preserves_values() {
    let owned = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();

    // Bare `--json` after an office subcommand is hoisted cleanly.
    let (cleaned, hoisted) =
        crate::hoist_office_words(&owned(&["member", "list", "--json"])).unwrap();
    assert_eq!(cleaned, owned(&["member", "list"]));
    assert!(hoisted.json);

    // `--json` as the value of a value-taking flag stays untouched (never hoisted).
    let (cleaned, hoisted) =
        crate::hoist_office_words(&owned(&["msg", "send", "--to", "ada", "--body", "--json"]))
            .unwrap();
    assert_eq!(
        cleaned,
        owned(&["msg", "send", "--to", "ada", "--body", "--json"])
    );
    assert!(!hoisted.json);

    // `--json` after `--` stays untouched.
    let (cleaned, hoisted) =
        crate::hoist_office_words(&owned(&["task", "list", "--", "--json"])).unwrap();
    assert_eq!(cleaned, owned(&["task", "list", "--", "--json"]));
    assert!(!hoisted.json);

    // `--input-json <object>` stays attached while trailing `--json` is hoisted.
    let (cleaned, hoisted) = crate::hoist_office_words(&owned(&[
        "task",
        "list",
        "--input-json",
        r#"{"status":"open"}"#,
        "--json",
    ]))
    .unwrap();
    assert_eq!(
        cleaned,
        owned(&["task", "list", "--input-json", r#"{"status":"open"}"#])
    );
    assert!(hoisted.json);

    // `--json=<object>` is refused with guidance pointing to `--input-json`.
    let err = crate::hoist_office_words(&owned(&["task", "list", "--json={}"]))
        .err()
        .expect("refused");
    assert!(err.to_string().contains("--input-json"), "{err}");

    // `--format table` and `--format=tsv` hoist to shell OutputFormat; `--format yaml` passes through.
    let (cleaned, hoisted) =
        crate::hoist_office_words(&owned(&["task", "list", "--format", "table"])).unwrap();
    assert_eq!(cleaned, owned(&["task", "list"]));
    assert_eq!(
        hoisted.format,
        Some(org_cli_shell::context::OutputFormat::Table)
    );
    let (cleaned, hoisted) =
        crate::hoist_office_words(&owned(&["task", "list", "--format=yaml"])).unwrap();
    assert_eq!(cleaned, owned(&["task", "list", "--format=yaml"]));
    assert!(hoisted.table_format);

    // Repeated shell-only `--format` flags: idempotent is allowed, conflicting is refused.
    let (cleaned, hoisted) = crate::hoist_office_words(&owned(&[
        "task", "list", "--format", "table", "--format", "table",
    ]))
    .unwrap();
    assert_eq!(cleaned, owned(&["task", "list"]));
    assert_eq!(
        hoisted.format,
        Some(org_cli_shell::context::OutputFormat::Table)
    );
    let (cleaned, hoisted) =
        crate::hoist_office_words(&owned(&["task", "list", "--format=tsv", "--format", "tsv"]))
            .unwrap();
    assert_eq!(cleaned, owned(&["task", "list"]));
    assert_eq!(
        hoisted.format,
        Some(org_cli_shell::context::OutputFormat::Tsv)
    );
    for conflict in [
        &["task", "list", "--format", "table", "--format", "tsv"][..],
        &["task", "list", "--format=tsv", "--format=table"],
        &["task", "list", "--format", "table", "--format=tsv"],
    ] {
        let err = crate::hoist_office_words(&owned(conflict))
            .err()
            .expect("conflicting repeated shell --format refused");
        assert!(
            err.to_string().contains("conflicting values"),
            "{conflict:?}: {err}"
        );
    }
}

#[test]
fn sign_in_prepare_modes_strips_hoisted_flags_and_applies_office_output_format() {
    use clap::FromArgMatches;
    use org_cli_shell::context::testing::{captured, tty};
    use org_cli_shell::context::{OutputFormat, OUTPUT_FORMAT_ENV};

    let prepare = |argv: &[&str]| {
        let mut cmd = cli::command();
        let matches = cmd.try_get_matches_from_mut(words(argv)).unwrap();
        let parsed = cli::Cli::from_arg_matches(&matches).unwrap();
        let explicit = crate::ExplicitOutput {
            json: parsed.global.json,
            quiet: parsed.global.quiet,
            format: parsed.global.format.map(crate::args::FormatArg::into_shell),
            table_format: false,
            table_format_kind: None,
            dry_run: parsed.global.dry_run,
        };
        let mut modes = parsed.modes();
        let (top, explicit, passthrough) =
            crate::prepare_modes(parsed.command.unwrap(), explicit, &mut modes).unwrap();
        (top, explicit, modes, passthrough)
    };

    // `sign-in -- task list --format table` strips `--format table` and sets `OutputFormat::Table`.
    let (top, explicit, modes, passthrough) =
        prepare(&["sign-in", "--", "task", "list", "--format", "table"]);
    let cli::TopCommand::SignIn(args) = top else {
        panic!("expected SignIn");
    };
    assert_eq!(args.command(), ["task", "list"]);
    assert_eq!(modes.format, Some(OutputFormat::Table));
    assert!(!passthrough);
    let (ctx, _) = captured(modes, org_cli_shell::terminal::Terminal::piped(), &[]);
    assert_eq!(
        crate::apply_office_output_format(args.command().to_vec(), &explicit, passthrough, &ctx),
        ["task", "list"]
    );

    // `sign-in -- task list -q` strips `-q` and sets `modes.quiet`.
    let (top, explicit, modes, passthrough) = prepare(&["sign-in", "--", "task", "list", "-q"]);
    let cli::TopCommand::SignIn(args) = top else {
        panic!("expected SignIn");
    };
    assert_eq!(args.command(), ["task", "list"]);
    assert!(modes.quiet);
    let (ctx, _) = captured(modes, tty(Some(80)), &[]);
    assert_eq!(
        crate::apply_office_output_format(args.command().to_vec(), &explicit, passthrough, &ctx),
        ["task", "list"]
    );

    // `AGENT_OS_FORMAT=yaml sign-in -- task list` injects `--format yaml` after `task list`.
    let (top, explicit, modes, passthrough) = prepare(&["sign-in", "--", "task", "list"]);
    let cli::TopCommand::SignIn(args) = top else {
        panic!("expected SignIn");
    };
    let (ctx, _) = captured(modes, tty(Some(80)), &[(OUTPUT_FORMAT_ENV, "yaml")]);
    assert_eq!(ctx.output_format(), OutputFormat::Yaml);
    assert_eq!(
        crate::apply_office_output_format(args.command().to_vec(), &explicit, passthrough, &ctx),
        ["task", "list", "--format", "yaml"]
    );

    // `--format yaml sign-in -- task list` injects `--format yaml` after `task list`.
    let (top, explicit, modes, passthrough) =
        prepare(&["--format", "yaml", "sign-in", "--", "task", "list"]);
    let cli::TopCommand::SignIn(args) = top else {
        panic!("expected SignIn");
    };
    let (ctx, _) = captured(modes, tty(Some(80)), &[]);
    assert_eq!(
        crate::apply_office_output_format(args.command().to_vec(), &explicit, passthrough, &ctx),
        ["task", "list", "--format", "yaml"]
    );

    // `sign-in -- agent sessions --member @ada --format yaml` keeps flags intact for clap and
    // sets `OutputFormat::Yaml`.
    let (top, _, modes, _) = prepare(&[
        "sign-in", "--", "agent", "sessions", "--member", "@ada", "--format", "yaml",
    ]);
    let cli::TopCommand::SignIn(args) = top else {
        panic!("expected SignIn");
    };
    assert_eq!(
        args.command(),
        ["agent", "sessions", "--member", "@ada", "--format", "yaml"]
    );
    assert_eq!(modes.format, Some(OutputFormat::Yaml));
}
