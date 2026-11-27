//! The flag-level drift guard (`INSTR-5`, `INSTR-7`): every cited `agent-os` command line is
//! dry-parsed against the live table, so stale or invented flags and missing required options
//! fail, in inline spans, fenced shell blocks and `session attach` JSON lines alike.

use crate::instructions::{cited_command_lines, verify_command_references};

fn problems(text: &str) -> Vec<String> {
    verify_command_references(text, &org_agent_cli::table())
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
    // `msg ask` takes one question as --prompt, never an invented --question; it needs --to.
    assert_fails(
        "Ask with `agent-os msg ask --to <person> --question <text>`.",
        "--question is not a flag of `msg ask`",
    );
    assert_fails(
        "`agent-os msg ask --prompt <text> --subject <text>`",
        "`to` is required",
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
        "`agent-os msg ask --to @<handle> --prompt <text> --question-type choice --option <id>=<label> --default-value <id>`",
        "`agent-os member set-hiring --member <id> --allow --if-match <rev>`",
        "`agent-os process create --kind standard --title <t> --non-overridable --input-file <path>`",
        "`agent-os context --format yaml`",
        "`agent-os session context --offline`",
        "`agent-os team create --name <name> --dry-run`",
    ] {
        assert!(problems(good).is_empty(), "{good}: {:#?}", problems(good));
    }
    // Legacy `boot` and `instructions overview` shorthands are removed cleanly (zero shims).
    assert_fails("`agent-os boot --format yaml`", "`boot` is not a resource");
    assert_fails(
        "`agent-os instructions overview --offline`",
        "`instructions` is not a resource",
    );
    assert_fails(
        "`agent-os session boot --offline`",
        "`session boot` is not a command",
    );
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
        "`agent-os msg wait --timeout-ms many`",
        "takes a whole number",
    );
    // The flags that were replaced are gone.
    assert_fails(
        "`agent-os task list --max-results 50`",
        "--max-results is not a flag of `task list`",
    );
    assert_fails(
        "`agent-os member set-hiring --member <id> --may-hire --if-match <rev>`",
        "--may-hire is not a flag of `member set-hiring`",
    );
    assert_fails(
        "`agent-os process update --id <id> --kind standard --title <t> --overridable --if-match <rev>`",
        "--overridable is not a flag of `process update`",
    );
    assert_fails(
        "`agent-os skill read`  `agent-os skill read --nam x`",
        "--nam is not a flag",
    );
    assert_fails("`agent-os task inspect <ref>`", "is a bare word");
    assert_fails("`agent-os session attach --limit 5`", "takes no flags");
    // A placeholder still has to fit an enum or number flag it fills: it becomes a valid value.
    assert!(problems("`agent-os task list --status <status> --limit <n>`").is_empty());
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
    let cited = cited_command_lines(text, &org_agent_cli::table());
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

#[test]
fn json_placeholders_keep_every_key_and_type_so_unknown_fields_and_wrong_types_fail() {
    // A placeholder leaf never hides the field it sits in.
    assert_fails(
        r#"`agent-os task list --json '{"nonexistent":"<x>"}'`"#,
        "`nonexistent` is not a field",
    );
    assert_fails(
        "```text\n{\"command\":\"task list\",\"input\":{\"nonexistent\":\"<x>\"}}\n```",
        "`nonexistent` is not a field",
    );
    assert_fails(
        r#"`agent-os task list --json='{"status":"<s>","bogus":{"deep":"<x>"}}'`"#,
        "`bogus` is not a field",
    );
    // A literal of the wrong type still fails next to placeholders.
    assert_fails(
        r#"`agent-os task list --json '{"status":"finished","query":"<q>"}'`"#,
        "`status` must be one of",
    );
    assert_fails(
        r#"`agent-os msg wait --json '{"timeout_ms":"many"}'`"#,
        "`timeout_ms` must be a whole number",
    );
    assert_fails(
        "```text\n{\"command\":\"msg wait\",\"input\":{\"from\":\"<who>\",\"timeout_ms\":true}}\n```",
        "`timeout_ms` must be a whole number",
    );
    // JSON that is not JSON is not waved through either.
    assert_fails(
        "`agent-os msg wait --json '{\"timeout_ms\": <n>}'`",
        "--json",
    );
    // Placeholder leaves become values of their declared types; a bare placeholder is `{}`.
    for good in [
        r#"`agent-os task list --json '{"status":"<status>","exclude_status":["<status>"],"labels":["<label>"]}'`"#,
        r#"`agent-os task list --json '{"labels":"<labels>","label_match":"<mode>"}'`"#,
        "`agent-os task list --json '<json>'`",
        "```text\n{\"command\":\"msg wait\",\"input\":{\"timeout_ms\":\"<n>\"}}\n```",
    ] {
        assert!(problems(good).is_empty(), "{good}: {:#?}", problems(good));
    }
}

#[test]
fn attach_lines_are_held_to_the_rules_session_attach_applies_first() {
    for (line, why) in [
        (
            r#"{"command":"task list","options":{"status":["todo"]}}"#,
            "Option `status` in `session attach` must be a string, number, or boolean.",
        ),
        (
            r#"{"command":"task list","options":{"max_results":{"n":5}}}"#,
            "Option `max_results` in `session attach` must be a string, number, or boolean.",
        ),
        (
            r#"{"command":"task list","options":["--limit","5"]}"#,
            "`options` in `session attach` must be a JSON object.",
        ),
        (
            r#"{"command":5}"#,
            "`command` must be a string naming a resource and verb.",
        ),
        (
            r#"{"command":"  "}"#,
            "`command` must name a resource and verb",
        ),
        (
            r#"{"command":"task list","options":{"format":"yaml"}}"#,
            "answers every line in JSON",
        ),
        (
            r#"{"command":"msg send","input":{"to":"owner"},"options":{"stdin":true}}"#,
            "`stdin` is refused",
        ),
        (
            r#"{"command":"task list","options":{"help":true}}"#,
            "`help` is refused",
        ),
        (
            r#"{"command":"task list","inputs":{"status":"todo"}}"#,
            "`inputs` is not a key of a `session attach` line",
        ),
        (
            r#"{"command":"task list --format yaml"}"#,
            "`--format` is a flag",
        ),
        (
            r#"{"command":"task list","options":{"--format=yaml":true}}"#,
            "Option `--format=yaml` is not a flag name",
        ),
        (
            r#"{"command":"task list","options":{"limit 5":true}}"#,
            "Option `limit 5` is not a flag name",
        ),
    ] {
        assert_fails(&format!("```text\n{line}\n```"), why);
    }
    // A value that looks like a flag stays that option's value, as attach passes it inline.
    assert_fails(
        "```text\n{\"command\":\"context\",\"options\":{\"offline\":\"--format=yaml\"}}\n```",
        "yes-or-no",
    );
    // Scalars and booleans become flags as attach makes them; a JSON example without
    // `command` is not an attach line.
    for good in [
        "```text\n{\"command\":\"task list\",\"options\":{\"status\":\"todo\",\"exclude_status\":\"done\",\"format\":\"json\",\"limit\":null}}\n```",
        "```text\n{\"command\":\"context\",\"options\":{\"offline\":true}}\n```",
        "```text\n{\"questions\":[{\"id\":\"q1\"}]}\n```",
    ] {
        assert!(problems(good).is_empty(), "{good}: {:#?}", problems(good));
    }
}

#[test]
fn attach_lines_give_true_only_to_yes_or_no_flags_as_session_attach_does() {
    // A flag that takes a value, given `true`, would read the next flag as its value (`--body
    // --dry-run`), so attach refuses the line, and the guard with it.
    for (line, why) in [
        (
            r#"{"command":"msg send","input":{"to":"@ada"},"options":{"body":true,"dry-run":true}}"#,
            "Option `body` takes a value, not `true`.",
        ),
        (
            r#"{"command":"msg send","input":{"to":"@ada"},"options":{"body":true,"durable":true}}"#,
            "Option `body` takes a value, not `true`.",
        ),
        (
            r#"{"command":"status set","options":{"--text":true,"dry-run":true}}"#,
            "Option `--text` takes a value, not `true`.",
        ),
        (
            r#"{"command":"status set","input":{"text":"ok"},"options":{"idempotency-key":true,"dry-run":true}}"#,
            "Option `idempotency-key` takes a value, not `true`.",
        ),
        (
            r#"{"command":"who","options":{"cursor":true}}"#,
            "Option `cursor` takes a value, not `true`.",
        ),
        (
            r#"{"command":"status set","input":{"text":"ok"},"options":{"bogus":true}}"#,
            "Option `bogus` is not a flag of `status set`.",
        ),
        (
            r#"{"command":"status set","input":{"text":"ok"},"options":{"-h":true}}"#,
            "Option `-h` is not a flag name",
        ),
    ] {
        assert_fails(&format!("```text\n{line}\n```"), why);
    }
    // A yes-or-no flag, common or the command's own, with or without `--`, holds; a line naming
    // no command fails on its command.
    for good in [
        r#"{"command":"status set","input":{"text":"ok"},"options":{"dry-run":true}}"#,
        r#"{"command":"task list","options":{"unassigned":true,"cursor":false}}"#,
        r#"{"command":"task list","options":{"--unassigned":true,"limit":5}}"#,
    ] {
        let text = format!("```text\n{good}\n```");
        assert!(problems(&text).is_empty(), "{good}: {:#?}", problems(&text));
    }
    assert_fails(
        "```text\n{\"command\":\"task frobnicate\",\"options\":{\"body\":true}}\n```",
        "`task frobnicate` is not a command",
    );
}

#[test]
fn agent_os_after_a_pipe_or_list_operator_is_checked_too() {
    for text in [
        "`agent-os msg inbox | agent-os task frobnicate`",
        "`cd /tmp && agent-os task frobnicate`",
        "`true || agent-os task frobnicate`",
        "`echo hi; agent-os task frobnicate`",
        "`sleep 1 & agent-os task frobnicate`",
        "```bash\nagent-os msg inbox --state unread | agent-os task frobnicate\n```",
        "```bash\n$ git status && agent-os task frobnicate\n```",
    ] {
        assert_fails(text, "`task frobnicate` is not a command");
    }
    // Quoted operators, comments and other programs are not command lines of ours.
    for good in [
        r#"`agent-os msg send --to @<handle> --body 'a | b && c; d'`"#,
        "`agent-os msg inbox # then agent-os task frobnicate`",
        "`cargo test && git push`",
        "```bash\nagent-os task list --status todo 2>&1 | head -5\n```",
    ] {
        assert!(problems(good).is_empty(), "{good}: {:#?}", problems(good));
    }
}
