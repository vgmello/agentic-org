use std::collections::BTreeSet;

use org_command::{Answer, Office, Refusal, Request, CATALOGUE_LIMIT};

use super::*;
use crate::{execute_with_office, table, SESSION_ATTACH_HELP};

struct PanicOffice;

impl Office for PanicOffice {
    fn call(&mut self, request: &Request) -> Result<Answer, Refusal> {
        panic!(
            "help must never contact the office: {}",
            request.command.name()
        );
    }
}

#[test]
fn root_catalogue_stays_well_under_catalogue_limit() {
    let table = table::table();
    let catalogue = table.catalogue();
    assert!(
        catalogue.len() <= CATALOGUE_LIMIT,
        "root catalogue is {} bytes; limit is {CATALOGUE_LIMIT}",
        catalogue.len()
    );
    assert!(
        catalogue.len() <= 3800,
        "expected root catalogue comfortably below {CATALOGUE_LIMIT} bytes, got {}",
        catalogue.len()
    );
}

#[test]
fn guidance_tables_match_live_command_table_exactly() {
    let table = table::table();
    let live_resources: BTreeSet<&str> = table.resources().into_iter().collect();
    let guided_resources: BTreeSet<&str> =
        RESOURCE_GUIDANCE.iter().map(|(name, _)| *name).collect();
    assert_eq!(
        RESOURCE_GUIDANCE.len(),
        guided_resources.len(),
        "duplicate resource in RESOURCE_GUIDANCE"
    );
    assert_eq!(
        guided_resources, live_resources,
        "RESOURCE_GUIDANCE drifted from live table resources"
    );

    let live_commands: BTreeSet<(&str, &str)> = table
        .rows()
        .iter()
        .map(|row| (row.resource, row.verb))
        .collect();
    let guided_commands: BTreeSet<(&str, &str)> =
        COMMAND_GUIDANCE.iter().map(|(r, v, _)| (*r, *v)).collect();
    assert_eq!(
        COMMAND_GUIDANCE.len(),
        guided_commands.len(),
        "duplicate command in COMMAND_GUIDANCE"
    );
    assert_eq!(
        guided_commands, live_commands,
        "COMMAND_GUIDANCE drifted from live CommandTable rows"
    );
}

#[test]
fn every_agent_os_help_tier_and_alias_includes_use_when_guidance() {
    let table = table::table();
    let mut office = PanicOffice;

    let top = execute_with_office("agent-os", &["--help".to_owned()], &mut office);
    assert_eq!(top.exit_code, 0);
    assert!(top.stdout.contains("Use when:"), "{}", top.stdout);
    for resource in table.resources() {
        let cue = resource_use_when(resource).unwrap();
        assert!(
            top.stdout.contains(cue),
            "top --help missing resource guidance for `{resource}`:\n{}",
            top.stdout
        );

        let res_help = execute_with_office(
            "agent-os",
            &[resource.to_owned(), "--help".to_owned()],
            &mut office,
        );
        assert_eq!(res_help.exit_code, 0, "{resource} --help");
        assert!(
            res_help.stdout.contains(&format!("Use when: {cue}")),
            "{resource} --help missing resource Use when:\n{}",
            res_help.stdout
        );

        for cmd in table.verbs(resource) {
            let cmd_cue = command_use_when(cmd.resource, cmd.verb).unwrap();
            assert!(
                res_help.stdout.contains(&format!("Use when: {cmd_cue}")),
                "{resource} --help missing verb guidance for `{}`:\n{}",
                cmd.name(),
                res_help.stdout
            );

            let cmd_help = execute_with_office(
                "agent-os",
                &[
                    cmd.resource.to_owned(),
                    cmd.verb.to_owned(),
                    "--help".to_owned(),
                ],
                &mut office,
            );
            assert_eq!(cmd_help.exit_code, 0, "{} --help", cmd.name());
            assert!(
                cmd_help.stdout.contains(&format!("Use when: {cmd_cue}")),
                "{} --help missing Use when:\n{}",
                cmd.name(),
                cmd_help.stdout
            );
        }
    }

    // Aliases & special transport help.
    let who_help = execute_with_office(
        "agent-os",
        &["who".to_owned(), "--help".to_owned()],
        &mut office,
    );
    assert_eq!(who_help.exit_code, 0);
    assert!(who_help.stdout.contains("Use when:"), "{}", who_help.stdout);

    let note_help = execute_with_office(
        "agent-os",
        &["task".to_owned(), "note".to_owned(), "--help".to_owned()],
        &mut office,
    );
    assert_eq!(note_help.exit_code, 0);
    assert!(
        note_help.stdout.contains("Use when:"),
        "{}",
        note_help.stdout
    );

    let note_add_help = execute_with_office(
        "agent-os",
        &[
            "task".to_owned(),
            "note".to_owned(),
            "add".to_owned(),
            "--help".to_owned(),
        ],
        &mut office,
    );
    assert_eq!(note_add_help.exit_code, 0);
    assert!(
        note_add_help.stdout.contains("Use when:"),
        "{}",
        note_add_help.stdout
    );

    assert!(
        SESSION_ATTACH_HELP.contains("Use when:"),
        "{SESSION_ATTACH_HELP}"
    );

    let session_help = execute_with_office(
        "agent-os",
        &["session".to_owned(), "--help".to_owned()],
        &mut office,
    );
    assert!(
        session_help.stdout.contains("attach") && session_help.stdout.contains("Use when:"),
        "{}",
        session_help.stdout
    );

    let human_session_help = execute_with_office(
        "agentic-org",
        &["session".to_owned(), "--help".to_owned()],
        &mut office,
    );
    assert_eq!(human_session_help.exit_code, 0);
    assert!(
        human_session_help.stdout.contains("Use when:")
            && !human_session_help.stdout.contains("attach"),
        "agentic-org session --help must not mention `attach`:\n{}",
        human_session_help.stdout
    );

    let human_top_help = execute_with_office("agentic-org", &["--help".to_owned()], &mut office);
    assert_eq!(human_top_help.exit_code, 0);
    assert!(
        !human_top_help.stdout.contains("session attach"),
        "agentic-org top --help must not mention `session attach`:\n{}",
        human_top_help.stdout
    );
}

fn parse_value_option_names(stdout: &str) -> BTreeSet<String> {
    stdout
        .lines()
        .skip_while(|line| *line != "Options:")
        .skip(1)
        .take_while(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let mut tokens = line.split_whitespace();
            let name = tokens.next()?.strip_prefix("--")?;
            let arg = tokens.next()?;
            arg.starts_with('<').then(|| name.to_owned())
        })
        .collect()
}

#[test]
fn common_value_options_match_live_org_command_help() {
    let table = table::table();
    let mut sources = crate::sources::StdSources;
    let mut keys = crate::keys::UuidV7KeySource;
    let mut office = PanicOffice;

    // `task list` covers All + Lists + WithInput; `task update` covers All + WithInput + Writes + Revisioned.
    let mut observed = BTreeSet::new();
    for args in [
        vec!["task".to_owned(), "list".to_owned(), "--help".to_owned()],
        vec!["task".to_owned(), "update".to_owned(), "--help".to_owned()],
    ] {
        let out = org_command::run(
            &table,
            "agent-os",
            &args,
            &mut sources,
            &mut keys,
            &mut office,
        );
        assert_eq!(out.exit_code, 0, "{args:?}");
        observed.extend(parse_value_option_names(&out.stdout));
    }

    let declared: BTreeSet<String> = COMMON_VALUE_OPTIONS
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    assert_eq!(
        COMMON_VALUE_OPTIONS.len(),
        declared.len(),
        "duplicate entry in COMMON_VALUE_OPTIONS"
    );
    assert_eq!(
        declared, observed,
        "COMMON_VALUE_OPTIONS drifted from live org_command Options:"
    );
}

struct RecordingOffice {
    calls: Vec<serde_json::Map<String, serde_json::Value>>,
}

impl Office for RecordingOffice {
    fn call(&mut self, request: &Request) -> Result<Answer, Refusal> {
        self.calls.push(request.input.clone());
        let data = serde_json::from_str(request.command.example.output)
            .expect("example output is valid JSON");
        Ok(Answer::completed(data))
    }
}

#[test]
fn literal_help_flag_value_is_not_mistaken_for_help_request() {
    let mut office = RecordingOffice { calls: Vec::new() };
    let body_out = execute_with_office(
        "agent-os",
        &[
            "msg".to_owned(),
            "send".to_owned(),
            "--to".to_owned(),
            "#general".to_owned(),
            "--body".to_owned(),
            "--help".to_owned(),
        ],
        &mut office,
    );
    assert_eq!(body_out.exit_code, 0, "{}", body_out.stdout);
    let env: serde_json::Value =
        serde_json::from_str(&body_out.stdout).expect("expected JSON envelope, not help text");
    assert_eq!(env["ok"], true, "{env}");
    assert_eq!(office.calls.len(), 1);
    assert_eq!(
        office.calls[0].get("body").and_then(|v| v.as_str()),
        Some("--help")
    );

    let mut panic_office = PanicOffice;
    let json_out = execute_with_office(
        "agent-os",
        &[
            "msg".to_owned(),
            "send".to_owned(),
            "--json".to_owned(),
            "--help".to_owned(),
        ],
        &mut panic_office,
    );
    assert_eq!(json_out.exit_code, 2, "{}", json_out.stdout);
    let err_env: serde_json::Value =
        serde_json::from_str(&json_out.stdout).expect("expected JSON error envelope");
    assert_eq!(err_env["ok"], false, "{err_env}");
    assert_eq!(err_env["error"]["code"], "invalid_request", "{err_env}");

    let real_help = execute_with_office(
        "agent-os",
        &[
            "msg".to_owned(),
            "send".to_owned(),
            "--to".to_owned(),
            "#general".to_owned(),
            "--help".to_owned(),
        ],
        &mut panic_office,
    );
    assert_eq!(real_help.exit_code, 0);
    assert!(
        real_help.stdout.contains("Use when:"),
        "{}",
        real_help.stdout
    );
}
