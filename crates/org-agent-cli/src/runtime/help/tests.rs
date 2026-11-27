//! Help detection and enrichment: a flag's value is never read as `--help`, the value options
//! help lists are exactly the ones detection steps over, and `-h` is `--help`.

use std::collections::BTreeSet;

use org_command::{Answer, Office, Refusal, Request};

use super::{asks_for_help, quickstart};
use crate::execute_with_office;

struct PanicOffice;

impl Office for PanicOffice {
    fn call(&mut self, request: &Request) -> Result<Answer, Refusal> {
        panic!(
            "help must never contact the office: {}",
            request.command.name()
        );
    }
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
fn every_value_option_help_lists_is_one_help_detection_steps_over() {
    let table = crate::registry::table();
    let mut sources = crate::runtime::sources::StdSources;
    let mut keys = crate::runtime::keys::UuidV7KeySource;
    let mut office = PanicOffice;

    // `task list` covers All + Lists + WithInput; `task update` covers All + WithInput + Writes +
    // Revisioned; `msg send` adds the content flags.
    for (resource, verb) in [("task", "list"), ("task", "update"), ("msg", "send")] {
        let args = vec![resource.to_owned(), verb.to_owned(), "--help".to_owned()];
        let out = org_command::run(
            &table,
            "agent-os",
            &args,
            &mut sources,
            &mut keys,
            &mut office,
        );
        assert_eq!(out.exit_code, 0, "{args:?}");
        let row = table.find(resource, verb).unwrap();
        let listed = parse_value_option_names(&out.stdout);
        assert!(listed.contains("format"), "{listed:?}");
        if verb == "list" {
            assert!(listed.contains("limit"), "{listed:?}");
        }
        for name in &listed {
            assert_eq!(
                row.flag_takes_value(name),
                Some(true),
                "`{resource} {verb} --{name}` is listed with a value but help detection would not step over it"
            );
            let rest = [format!("--{name}"), "--help".to_owned()];
            assert!(
                !asks_for_help(row, &rest),
                "`--{name} --help` reads `--help` as the value"
            );
        }
    }
}

#[test]
fn help_detection_steps_over_values_and_finds_help_after_switches() {
    let table = crate::registry::table();
    let send = table.find("msg", "send").unwrap();
    let words = |line: &str| {
        line.split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    assert!(asks_for_help(send, &words("--help")));
    assert!(asks_for_help(send, &words("-h")));
    assert!(asks_for_help(send, &words("--to @ada -h")));
    assert!(asks_for_help(send, &words("--dry-run --help")));
    assert!(!asks_for_help(send, &words("--body --help")));
    assert!(!asks_for_help(send, &words("--format -h")));
    assert!(!asks_for_help(send, &words("--idempotency-key --help")));
    assert!(asks_for_help(send, &words("--unknown --help")));
}

#[test]
fn the_quickstart_names_how_to_get_bound_only_when_unbound() {
    assert!(quickstart(false).contains("agent-os login --as @<handle>"));
    assert!(quickstart(false).contains("agent-os context --offline"));
    assert!(quickstart(true).contains("agent-os context --format yaml"));
    assert_eq!(quickstart(false).lines().count(), 4);
    assert_eq!(quickstart(true).lines().count(), 4);
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

#[test]
fn short_help_matches_long_help_and_command_help_shows_a_runnable_example() {
    let words = |line: &[&str]| line.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>();
    let mut office = PanicOffice;
    for (short, long) in [
        (&["-h"][..], &["--help"][..]),
        (&["msg", "-h"], &["msg", "--help"]),
        (&["msg", "send", "-h"], &["msg", "send", "--help"]),
        (&["who", "-h"], &["who", "--help"]),
    ] {
        let short_out = execute_with_office("agent-os", &words(short), &mut office);
        let long_out = execute_with_office("agent-os", &words(long), &mut office);
        assert_eq!(short_out, long_out, "{short:?}");
        assert_eq!(short_out.exit_code, 0, "{short:?}");
        assert!(short_out.stdout.contains("Use when:"), "{short:?}");
    }
    // Every command's help ends with its example (AX-7), after the `Use when:` cue.
    let table = crate::registry::table();
    for row in table.rows() {
        let help = execute_with_office(
            "agent-os",
            &words(&[row.resource, row.verb, "-h"]),
            &mut office,
        );
        let example = help
            .stdout
            .rsplit_once("\nExample:\n  ")
            .map(|(_, line)| line)
            .unwrap_or_else(|| panic!("{}: no example in {}", row.name(), help.stdout));
        assert!(
            example.starts_with(&format!("agent-os {} {}", row.resource, row.verb))
                && example.lines().count() == 1,
            "{}: {example}",
            row.name()
        );
    }
}
