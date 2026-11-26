//! The flag-level drift guard (`INSTR-5`, `INSTR-7`): every cited `agent-os` command line is
//! dry-parsed against the live table, so stale or invented flags and missing required options
//! fail, in inline spans, fenced shell blocks and `session attach` JSON lines alike.

use crate::instructions::{cited_command_lines, verify_command_references};

fn problems(text: &str) -> Vec<String> {
    verify_command_references(text, &org_agent_cli::table::table())
}

fn assert_fails(text: &str, why: &str) {
    let found = problems(text);
    assert_eq!(found.len(), 1, "{text}: {found:#?}");
    assert!(found[0].contains(why), "{text}: {found:#?}");
}

#[test]
fn the_four_audited_inaccuracies_fail_the_guard() {
    // `channel summarize` takes --question, not the stale --about.
    assert_fails(
        "Use `agent-os channel summarize --channel <ch> --about <topic>`.",
        "--about is not a flag of `channel summarize`",
    );
    // `msg ask` has no --prompt; its questions are structure given in --json.
    assert_fails(
        "Ask with `agent-os msg ask --to <person> --prompt <text>`.",
        "--prompt is not a flag of `msg ask`",
    );
    assert_fails(
        "`agent-os msg ask --to <person> --subject <text>`",
        "`questions` is required",
    );
    // `task transition` takes --blocked-reason, and requires --if-match.
    assert_fails(
        "`agent-os task transition --task <ref> --status blocked --reason <text> --if-match <rev>`",
        "--reason is not a flag of `task transition`",
    );
    assert_fails(
        "`agent-os task transition --task <ref> --status <status>`",
        "--if-match is required",
    );
    // --dry-run is read only on writes.
    assert_fails(
        "`agent-os task inspect --task <ref> --dry-run`",
        "--dry-run is for",
    );
    // Their corrections hold.
    for good in [
        "`agent-os channel summarize --channel <ch> --question <text>`",
        "`agent-os task transition --task <ref> --status blocked --blocked-reason <text> --if-match <rev>`",
        r#"`agent-os msg ask --to @<handle> --json '{"questions":[{"id":"q1","question_type":"yes_no","prompt":"Ship?"}]}'`"#,
        "`agent-os team create --name <name> --dry-run`",
    ] {
        assert!(problems(good).is_empty(), "{good}: {:#?}", problems(good));
    }
}

#[test]
fn unknown_commands_resources_flags_and_values_fail() {
    assert_fails(
        "`agent-os task frobnicate`",
        "`task frobnicate` is not a command",
    );
    assert_fails("`agent-os tasks list`", "`tasks` is not a resource");
    assert_fails(
        "`agent-os task list --status finished`",
        "--status is one of",
    );
    assert_fails(
        "`agent-os task list --max-results many`",
        "takes a whole number",
    );
    assert_fails(
        "`agent-os skill read`  `agent-os skill read --nam x`",
        "--nam is not a flag",
    );
    assert_fails("`agent-os task inspect <ref>`", "is a bare word");
    assert_fails("`agent-os session attach --limit 5`", "takes no flags");
    // A placeholder still has to fit an enum or number flag it fills: it becomes a valid value.
    assert!(problems("`agent-os task list --status <status> --max-results <n>`").is_empty());
}

#[test]
fn fenced_blocks_are_invocations_including_continuations_and_attach_lines() {
    let text = "\
Intro with a reference: `agent-os task transition`.

```bash
$ agent-os task inspect --task <task>
agent-os task transition --task <task> \\
  --status done
agent-os msg inbox --state unread | head -5
```

```text
{\"command\":\"task inspect\",\"input\":{\"task\":\"T-A1\"}}
{\"command\":\"task transition\",\"input\":{\"task\":\"T-A1\",\"status\":\"done\"}}
{\"command\":\"who\",\"options\":{\"limit\":5}}
not a command line
```
";
    let cited = cited_command_lines(text);
    assert_eq!(cited.len(), 7, "{cited:#?}");
    assert!(
        !cited
            .iter()
            .find(|c| c.cited == "agent-os task transition")
            .unwrap()
            .invocation
    );
    let found = problems(text);
    assert_eq!(found.len(), 2, "{found:#?}");
    assert!(
        found.iter().all(|p| p.contains("--if-match is required")),
        "{found:#?}"
    );
    assert!(
        found[0].contains("--status done"),
        "the continuation is joined: {found:#?}"
    );
}

#[test]
fn aliases_help_grammar_and_attach_are_understood() {
    for good in [
        "`agent-os who`",
        "`agent-os who --team <team-id>`",
        "`agent-os task note add --task <task> --text <text>`",
        "`agent-os --help`",
        "`agent-os task --help`",
        "`agent-os task transition --schema`",
        "`agent-os <resource> <verb> --flag value`",
        "`agent-os task <verb>`",
        "`agent-os session attach`",
        "`agent-os session attach < <path>`",
        "`agent-os task comment --task <task> --input-file <path>`",
        "`agent-os task list --fields <fields> --format compact`",
        "`agent-os msg send --to '#<channel>' --body '...'`",
        "`agent-osx task list` is not ours, nor is `cargo test`.",
    ] {
        assert!(problems(good).is_empty(), "{good}: {:#?}", problems(good));
    }
    // An unquoted `#channel` starts a shell comment: the command line loses its value.
    assert_fails("`agent-os msg send --to #general --body hi`", "--to");
}
