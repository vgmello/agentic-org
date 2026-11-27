//! The command contract, run against the test office: CLI-1, 2, 3, 7, 8 and 11 of
//! `docs/design/agent-os-command-line.md` for every row of the sample table, a row added here
//! without touching the parser, and the parser's and the formats' edge cases.

use std::collections::BTreeMap;
use std::path::Path;

use org_command::testkit::{
    answer_with, check_contract, check_contract_with, check_rows, check_rows_with, check_schema,
    json_envelope, run_recorded, run_stub, sample_table, stub_office, validate_schema, StubKeys,
    StubOffice, StubSources, HARD_TEXT, SAMPLE_ROWS,
};
use org_command::{
    envelope_schema, run, Ack, Alias, Answer, Command, CommandTable, Example, Field, IfMatch,
    KeySource, Kind, Node, Office, Output, Refusal, RefusalDetails, Request, Status, Type,
    MAX_INPUT_BYTES, MAX_YAML_INPUT_BYTES,
};
use org_proto::ErrorCode;
use serde_json::{json, Map, Value};

fn args(line: &[&str]) -> Vec<String> {
    line.iter().map(|arg| (*arg).to_owned()).collect()
}

fn sent(table: &CommandTable, line: &[&str]) -> Request {
    let (output, mut office) = run_stub(table, &args(line), &mut StubSources::default());
    assert_eq!(output.exit_code, 0, "{line:?}: {}", output.stdout);
    office.calls.pop().expect("one call")
}

/// The message of a command line refused with exit code 2 before the office is called.
fn refused(table: &CommandTable, line: &[&str]) -> String {
    let (output, office) = run_stub(table, &args(line), &mut StubSources::default());
    let envelope = json_envelope(&output, &format!("{line:?}"));
    assert_eq!(output.exit_code, 2, "{line:?}: {}", output.stdout);
    assert_eq!(envelope["error"]["code"], json!("invalid_request"));
    assert!(office.calls.is_empty(), "{line:?}: the office was called");
    format!(
        "{} {}",
        envelope["error"]["message"].as_str().expect("a message"),
        envelope["error"]["hint"].as_str().expect("a hint")
    )
}

#[test]
fn the_contract_holds_for_every_row() {
    let snapshots = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    check_contract(&sample_table(), &snapshots);
}

/// A row of another domain, declared in this test. Nothing in the library names it.
static EXTRA_ROWS: [Command; 1] = [Command {
    resource: "widget",
    verb: "rename",
    summary: "Rename a widget",
    call: "widget.rename",
    kind: Kind::Write {
        ack: Ack::Committed,
        if_match: IfMatch::Optional,
    },
    input: &[
        Field::required("widget_id", Type::String, "The widget's ID"),
        Field::required("new_name", Type::String, "Its new name"),
        Field::optional("note", Type::String, "Why, as Markdown"),
    ],
    content_field: Some("note"),
    output: &[
        Field::required("widget_id", Type::String, "The widget's ID"),
        Field::required("name", Type::String, "Its name"),
    ],
    errors: &[ErrorCode::NotFound, ErrorCode::Conflict],
    example: Example {
        input: r#"{"widget_id":"W-1","new_name":"Sprocket"}"#,
        output: r#"{"widget_id":"W-1","name":"Sprocket"}"#,
    },
}];

#[test]
fn a_row_is_added_without_touching_the_parser() {
    let table = CommandTable::new(&[&SAMPLE_ROWS, &EXTRA_ROWS]).expect("the table");
    check_rows(&table);

    let request = sent(
        &table,
        &[
            "widget",
            "rename",
            "--widget-id",
            "W-1",
            "--new-name=Sprocket",
            "--if-match",
            "7",
            "--durable",
        ],
    );
    assert_eq!(request.command.call, "widget.rename");
    assert_eq!(
        Value::Object(request.input),
        json!({"widget_id": "W-1", "new_name": "Sprocket"})
    );
    assert_eq!((request.if_match, request.durable), (Some(7), true));
    assert!(table.catalogue().contains("\nwidget: rename\n"));
    let (help, _) = run_stub(&table, &args(&["--help"]), &mut StubSources::default());
    assert!(help.stdout.contains("widget"), "{}", help.stdout);
}

#[test]
fn flags_and_json_give_the_same_input() {
    let table = sample_table();
    let row = table.find("sample", "create").expect("the row");
    let example: Map<String, Value> = serde_json::from_str(row.example.input).expect("example");
    let mut flat = example.clone();
    // A field with structure has no flag: it is given in --input-json beside the flags.
    let structure =
        json!({"owner": flat.remove("owner"), "extra": flat.remove("extra")}).to_string();
    let request = sent(
        &table,
        &[
            "sample",
            "create",
            "--title",
            "1.5",
            "--body=Short body",
            "--state",
            "open",
            "--priority",
            "2",
            "--labels",
            "a b",
            "--labels=yes",
            "--urgent",
            "--input-json",
            &structure,
        ],
    );
    assert_eq!(request.input, example);
    // A text flag keeps text that looks like a number or a flag.
    let request = sent(
        &table,
        &["sample", "create", "--title", "--help", "--body", "-1"],
    );
    assert_eq!(
        Value::Object(request.input),
        json!({"title": "--help", "body": "-1"})
    );
    // The same value given both ways is accepted; different values are refused.
    sent(
        &table,
        &[
            "sample",
            "create",
            "--title",
            "A",
            "--input-json",
            r#"{"title":"A"}"#,
        ],
    );
    let message = refused(
        &table,
        &[
            "sample",
            "create",
            "--title",
            "A",
            "--input-json",
            r#"{"title":"B"}"#,
        ],
    );
    assert!(message.contains("`title` is given twice"), "{message}");
}

#[test]
fn a_line_that_cannot_be_read_is_refused_with_what_to_do() {
    let table = sample_table();
    for (line, expected) in [
        (&[][..], "The resources are: sample"),
        (&["--format", "yaml"], "is not a resource"),
        (&["nothing", "list"], "The resources are: sample"),
        (
            &["sample"],
            "The verbs of `sample` are: create, inspect, list, ping, update",
        ),
        (&["sample", "destroy"], "is not a command"),
        (&["sample", "inspect", "S-1"], "Did you mean --id \"S-1\"?"),
        (
            &["sample", "create", "--title", "A", "some text"],
            "Did you mean --body \"some text\"?",
        ),
        (&["sample", "inspect", "--id"], "--id needs a value"),
        (
            &["sample", "inspect", "--id", "S-1", "--id", "S-2"],
            "--id is given twice",
        ),
        (
            &["sample", "inspect", "--id", "S-1", "--format", "xml"],
            "--format is json, yaml or compact",
        ),
        (
            &[
                "sample", "inspect", "--id", "S-1", "--format", "yaml", "--format", "json",
            ],
            "--format is given twice",
        ),
        (
            &["sample", "inspect", "--id", "S-1", "--limit", "5"],
            "--limit is for a command that lists",
        ),
        (
            &["sample", "inspect", "--id", "S-1", "--dry-run"],
            "--dry-run is for a command that writes",
        ),
        (
            &["sample", "inspect", "--id", "S-1", "--fields", "id,nope"],
            "`nope` in `nope` is not a field",
        ),
        (
            &["sample", "inspect", "--id", "S-1", "--fields", "id.x"],
            "`x` in `id.x` is not a field",
        ),
        (
            &["sample", "inspect", "--id", "S-1", "--fields", "id,"],
            "is not a field name",
        ),
        (
            &["sample", "ping", "--input-json", "{}"],
            "--input-json is for a command with input",
        ),
        (
            &["sample", "ping", "--stdin"],
            "--stdin is for a command with input",
        ),
        (
            &["sample", "list", "--state", "ajar"],
            "--state is one of open, closed",
        ),
        (
            &["sample", "list", "--limit", "0"],
            "--limit is a number from 1 to 200",
        ),
        (
            &["sample", "list", "--limit", "201"],
            "--limit is a number from 1 to 200",
        ),
        (&["sample", "list", "--cursor="], "--cursor is empty"),
        (
            &["sample", "list", "--input-format", "json"],
            "--input-format needs --input-file or --stdin",
        ),
        (&["sample", "list", "--stdin"], "has no content field"),
        (
            &["sample", "create", "--title", "A", "--priority", "high"],
            "--priority takes a whole number",
        ),
        (
            &["sample", "create", "--title", "A", "--urgent=true"],
            "yes-or-no flag",
        ),
        (
            &["sample", "create", "--title", "A", "--owner", "E-1"],
            "`owner` has structure",
        ),
        (
            &["sample", "create", "--title", "A", "--input-json", "[1]"],
            "--input-json must be one JSON object",
        ),
        (
            &["sample", "create", "--title", "A", "--input-json", "{"],
            "--input-json is not JSON",
        ),
        (
            &["sample", "create", "--input-json", r#"{"title":1}"#],
            "`title` must be text",
        ),
        (
            &[
                "sample",
                "create",
                "--input-json",
                r#"{"title":"A","owner":{"name":"x"}}"#,
            ],
            "`owner.id` is required",
        ),
        (
            &[
                "sample",
                "create",
                "--input-json",
                r#"{"title":"A","nope":1}"#,
            ],
            "`nope` is not a field",
        ),
        (
            &["sample", "create", "--title", "A", "--if-match", "3"],
            "--if-match is for a write to a record with a revision",
        ),
        (
            &[
                "sample",
                "create",
                "--title",
                "A",
                "--dry-run",
                "--idempotency-key",
                "k",
            ],
            "takes no --idempotency-key",
        ),
        (
            &[
                "sample",
                "create",
                "--title",
                "A",
                "--idempotency-key",
                "not ok",
            ],
            "letters, digits and hyphens",
        ),
        (
            &[
                "sample",
                "create",
                "--title",
                "A",
                "--idempotency-key",
                &"k".repeat(65),
            ],
            "letters, digits and hyphens",
        ),
        (
            &[
                "sample",
                "create",
                "--title",
                "A",
                "--input-file",
                "missing.md",
            ],
            "--input-file could not be read",
        ),
        (
            &[
                "sample",
                "create",
                "--title",
                "A",
                "--input-format",
                "toml",
                "--stdin",
            ],
            "--input-format is text, json or yaml",
        ),
        (
            &["sample", "update", "--id", "S-1"],
            "--if-match is required",
        ),
        (
            &["sample", "update", "--id", "S-1", "--if-match", "new"],
            "--if-match is a revision number",
        ),
    ] {
        let message = refused(&table, line);
        assert!(
            message.contains(expected),
            "{line:?}: `{expected}` is not in: {message}"
        );
        if line.len() >= 2 && table.find(line[0], line[1]).is_some() {
            assert!(
                message.contains("Flags of `sample"),
                "{line:?}: the hint lists the flags: {message}"
            );
        }
    }
}

#[test]
fn the_options_reach_the_office() {
    let table = sample_table();
    let request = sent(&table, &["sample", "list"]);
    assert_eq!((request.limit, request.cursor.as_deref()), (Some(20), None));
    let request = sent(
        &table,
        &[
            "sample", "list", "--limit", "200", "--cursor", "c-2", "--label", "x",
        ],
    );
    assert_eq!(
        (request.limit, request.cursor.as_deref()),
        (Some(200), Some("c-2"))
    );
    assert_eq!(Value::Object(request.input), json!({"label": ["x"]}));
    let request = sent(&table, &["sample", "inspect", "--id", "S-1"]);
    assert_eq!(
        (request.limit, request.dry_run, request.durable),
        (None, false, false)
    );
    let request = sent(
        &table,
        &[
            "sample",
            "create",
            "--title",
            "A",
            "--idempotency-key",
            "retry-1",
        ],
    );
    assert_eq!(request.idempotency_key.as_deref(), Some("retry-1"));

    let line = args(&[
        "sample",
        "update",
        "--id",
        "S-1",
        "--if-match",
        "3",
        "--dry-run",
    ]);
    let (output, office) = run_stub(&table, &line, &mut StubSources::default());
    let envelope = json_envelope(&output, "a dry run");
    assert!(office.calls[0].dry_run);
    assert_eq!(
        envelope["meta"],
        json!({"status": "completed", "ack": "durable", "dry_run": true})
    );
    let line = args(&["sample", "update", "--id", "S-1", "--if-match", "3"]);
    let (output, _) = run_stub(&table, &line, &mut StubSources::default());
    assert_eq!(
        json_envelope(&output, "a write")["meta"],
        json!({"status": "completed", "operation_id": "key-1", "ack": "durable", "revision": 4})
    );
}

#[test]
fn the_envelope_prints_the_same_bytes_in_the_stated_order() {
    let table = sample_table();
    let (output, _) = run_stub(
        &table,
        &args(&["sample", "inspect", "--id", "S-1"]),
        &mut StubSources::default(),
    );
    assert_eq!(
        output.stdout,
        "{\"ok\":true,\"data\":{\"id\":\"S-1\",\"title\":\"1.5\",\"body\":\"# Heading\\n\\nA `body` with \\\"quotes\\\", $x and — ünïcödé.\\n\",\"state\":\"open\",\"priority\":2,\"labels\":[\"a b\",\"yes\"],\"owner\":{\"id\":\"E-1\",\"name\":\"Ada\"},\"revision\":3},\"meta\":{\"status\":\"completed\"},\"error\":null}\n"
    );
    let line = args(&["sample", "inspect", "--id", "S-1", "--format", "yaml"]);
    let (output, _) = run_stub(&table, &line, &mut StubSources::default());
    assert_eq!(
        output.stdout,
        "ok: true\ndata:\n  id: S-1\n  title: \"1.5\"\n  body: |\n    # Heading\n\n    A `body` with \"quotes\", $x and — ünïcödé.\n  state: open\n  priority: 2\n  labels:\n    - a b\n    - \"yes\"\n  owner:\n    id: E-1\n    name: Ada\n  revision: 3\nmeta:\n  status: completed\nerror: null\n"
    );
    let line = args(&[
        "sample",
        "inspect",
        "--id",
        "S-1",
        "--format",
        "compact",
        "--fields",
        "revision,id,owner",
    ]);
    let (output, _) = run_stub(&table, &line, &mut StubSources::default());
    assert_eq!(
        output.stdout,
        "id: S-1\nowner: {\"id\":\"E-1\",\"name\":\"Ada\"}\nrevision: 3\n"
    );
    let line = args(&[
        "sample",
        "list",
        "--format",
        "compact",
        "--fields",
        "title,id,owner.id",
    ]);
    let (output, _) = run_stub(&table, &line, &mut StubSources::default());
    assert_eq!(
        output.stdout,
        "S-2\tSecond\t\nS-1\tFirst\t{\"id\":\"E-1\"}\n"
    );
}

#[test]
fn fields_keep_only_what_is_asked_in_the_records_own_order() {
    let table = sample_table();
    let line = args(&[
        "sample",
        "inspect",
        "--id",
        "S-1",
        "--fields",
        "owner.name,state,id",
    ]);
    let (output, _) = run_stub(&table, &line, &mut StubSources::default());
    assert!(
        output.stdout.starts_with("{\"ok\":true,\"data\":{\"id\":\"S-1\",\"state\":\"open\",\"owner\":{\"name\":\"Ada\"}},"),
        "{}",
        output.stdout
    );
    let line = args(&["sample", "list", "--fields=revision,id"]);
    let (output, _) = run_stub(&table, &line, &mut StubSources::default());
    assert_eq!(
        json_envelope(&output, "a list")["data"],
        json!([{"id": "S-2", "revision": 1}, {"id": "S-1", "revision": 3}])
    );
}

/// An office that answers what the test gives it.
struct Fixed(Result<Answer, Refusal>);

impl Office for Fixed {
    fn call(&mut self, _: &Request) -> Result<Answer, Refusal> {
        self.0.clone()
    }
}

#[test]
fn every_status_and_exit_code() {
    let table = sample_table();
    let line = args(&["sample", "list", "--format", "json"]);
    for status in Status::ALL {
        let mut answer = answer_with(status, json!([]));
        answer.next_cursor = Some("c-9".to_owned());
        let output = run(
            &table,
            "agent-os",
            &line,
            &mut StubSources::default(),
            &mut StubKeys::default(),
            &mut Fixed(Ok(answer)),
        );
        let envelope = json_envelope(&output, status.as_str());
        assert_eq!(output.exit_code, 0);
        assert_eq!(
            envelope,
            json!({"ok": true, "data": [], "meta": {"status": status.as_str(), "next_cursor": "c-9"}, "error": null})
        );
    }
    let refusal = Refusal::new(
        ErrorCode::NotAuthorized,
        "Only the owner lists samples.",
        "Ask the owner.",
    );
    let output = run(
        &table,
        "agent-os",
        &line,
        &mut StubSources::default(),
        &mut StubKeys::default(),
        &mut Fixed(Err(refusal)),
    );
    assert_eq!(output.exit_code, 1);
    assert_eq!(
        output.stdout,
        "{\"ok\":false,\"data\":null,\"meta\":{},\"error\":{\"code\":\"not_authorized\",\"message\":\"Only the owner lists samples.\",\"hint\":\"Ask the owner.\"}}\n"
    );
}

#[test]
fn a_file_or_standard_input_can_be_the_whole_input() {
    let table = sample_table();
    let yaml = "title: \"1.5\"\nstate: open\npriority: 2\nlabels:\n  - a b\n  - \"yes\"\nurgent: true\nowner:\n  id: E-1\n  name: Ada\nbody: |\n  # Heading\n\n  `code` $HOME \"q\"\n";
    let expected = json!({
        "title": "1.5", "state": "open", "priority": 2, "labels": ["a b", "yes"], "urgent": true,
        "owner": {"id": "E-1", "name": "Ada"}, "body": "# Heading\n\n`code` $HOME \"q\"\n"
    });
    let mut sources = StubSources {
        files: BTreeMap::from([
            ("input.yaml".to_owned(), yaml.as_bytes().to_vec()),
            ("input.json".to_owned(), expected.to_string().into_bytes()),
        ]),
        stdin: Some(yaml.as_bytes().to_vec()),
        ..StubSources::default()
    };
    for line in [
        &[
            "sample",
            "create",
            "--input-file",
            "input.yaml",
            "--input-format",
            "yaml",
        ][..],
        &[
            "sample",
            "create",
            "--input-file",
            "input.json",
            "--input-format",
            "json",
        ],
        &[
            "sample",
            "create",
            "--stdin",
            "--input-format=yaml",
            "--title",
            "1.5",
        ],
    ] {
        let (output, office) = run_stub(&table, &args(line), &mut sources);
        assert_eq!(output.exit_code, 0, "{line:?}: {}", output.stdout);
        assert_eq!(
            Value::Object(office.calls[0].input.clone()),
            expected,
            "{line:?}"
        );
    }
    // A row with no content field takes the whole input the same way.
    let mut sources = StubSources {
        stdin: Some(b"state: closed\n".to_vec()),
        ..StubSources::default()
    };
    let line = args(&["sample", "list", "--stdin", "--input-format", "yaml"]);
    let (output, office) = run_stub(&table, &line, &mut sources);
    assert_eq!(output.exit_code, 0, "{}", output.stdout);
    assert_eq!(
        Value::Object(office.calls[0].input.clone()),
        json!({"state": "closed"})
    );
    // A document marker and Windows line ends are read.
    let mut sources = StubSources {
        stdin: Some(b"---\r\nstate: closed\r\n".to_vec()),
        ..StubSources::default()
    };
    let (output, office) = run_stub(&table, &line, &mut sources);
    assert_eq!(output.exit_code, 0, "{}", output.stdout);
    assert_eq!(
        Value::Object(office.calls[0].input.clone()),
        json!({"state": "closed"})
    );
    // A file that is not an object is refused.
    let mut sources = StubSources {
        stdin: Some(b"- a\n- b\n".to_vec()),
        ..StubSources::default()
    };
    let (output, office) = run_stub(&table, &line, &mut sources);
    assert_eq!(
        (output.exit_code, office.calls.len()),
        (2, 0),
        "{}",
        output.stdout
    );
}

#[test]
fn yaml_holds_any_text_unchanged() {
    let table = sample_table();
    let line = args(&["sample", "ping", "--format", "yaml"]);
    for text in [
        HARD_TEXT,
        "",
        " leading space\nsecond",
        "\nstarts with a new line",
        "two at the end\n\n",
        "no new line at the end\nlast",
        "one line: with # marks",
        "null",
        "-1",
        "1e3",
        "bell \u{7} and \u{85} and \u{2028} and \u{feff}\nnext",
        "carriage\r\nreturn",
        "noncharacters \u{fffe} and \u{ffff}\nnext",
        "---\nlooks like a document\n...\n",
        "trailing space \n  indented\n\n\nblank lines",
    ] {
        let answer = Answer::completed(json!({"office": text}));
        let output = run(
            &table,
            "agent-os",
            &line,
            &mut StubSources::default(),
            &mut StubKeys::default(),
            &mut Fixed(Ok(answer)),
        );
        let read = org_docs::parse_mapping(&output.stdout)
            .unwrap_or_else(|e| panic!("{text:?}: {e}\n{}", output.stdout));
        assert_eq!(read["data"]["office"], json!(text), "{}", output.stdout);
    }
}

#[test]
fn help_is_text_at_three_levels_and_never_calls_the_office() {
    let table = sample_table();
    for (line, expected) in [
        (
            &["--help"][..],
            "Resources:\n  sample  create, inspect, list, ping, update\n",
        ),
        (&["sample", "--help"], "  inspect  One sample\n"),
        (
            &["sample", "create", "--help"],
            "  --labels <text> (repeat for several)  A label\n",
        ),
        (&["sample", "create", "--help"], "Content: `body`."),
        (&["sample", "create", "--help"], "  owner  Who owns it\n"),
        (&["sample", "create", "--help"], "--idempotency-key <key>"),
        // Help wins over whatever else is wrong with the line.
        (
            &["sample", "create", "stray", "--no-such-flag", "--help"],
            "Usage: agent-os sample create",
        ),
        (
            &["sample", "create", "--priority", "high", "--help"],
            "Usage: agent-os sample create",
        ),
        (
            &["sample", "create", "--no-such-flag=--x", "--help", "--body"],
            "Usage: agent-os sample create",
        ),
        // A yes-or-no flag and a field with structure take no value, so help follows them.
        (
            &["sample", "create", "--urgent", "--help"],
            "Usage: agent-os sample create",
        ),
        (
            &["sample", "create", "--owner", "--help"],
            "Usage: agent-os sample create",
        ),
        // The limits of what a command reads are stated.
        (&["sample", "create", "--help"], "at most 1 MiB"),
        (&["sample", "create", "--help"], "yaml: at most 64 KiB"),
        (
            &["sample", "list", "--help"],
            "Page size, 1 to 200. Default 20",
        ),
        (
            &["sample", "create", "--help"],
            "Without it the command makes one and reports it in meta.operation_id",
        ),
    ] {
        let (output, office) = run_stub(&table, &args(line), &mut StubSources::default());
        assert_eq!(output.exit_code, 0, "{line:?}");
        assert!(
            output.stdout.contains(expected),
            "{line:?}: `{expected}` is not in:\n{}",
            output.stdout
        );
        assert!(office.calls.is_empty());
    }
    let (output, _) = run_stub(
        &table,
        &args(&["sample", "inspect", "--help"]),
        &mut StubSources::default(),
    );
    assert!(
        !output.stdout.contains("--limit") && !output.stdout.contains("--dry-run"),
        "{}",
        output.stdout
    );
    // `--help` is help only where a flag is read. As the value of a flag that takes one it is
    // that value: the line is read as written, and here it lacks the required --title.
    for line in [
        &["sample", "create", "--body", "--help"][..],
        &["sample", "create", "--input-json", "--help"],
        &["sample", "create", "--format", "--help"],
        &["sample", "list", "--limit", "--help"],
        &["sample", "create", "--title", "A", "--help=yes"],
    ] {
        let message = refused(&table, line);
        assert!(!message.contains("Usage:"), "{line:?}: {message}");
    }
    let request = sent(&table, &["sample", "create", "--title", "--help"]);
    assert_eq!(Value::Object(request.input), json!({"title": "--help"}));
}

#[test]
fn short_help_is_help_at_three_levels_and_a_value_where_a_flag_takes_one() {
    let table = sample_table();
    for (short, long) in [
        (&["-h"][..], &["--help"][..]),
        (&["sample", "-h"], &["sample", "--help"]),
        (&["sample", "create", "-h"], &["sample", "create", "--help"]),
        (
            &["sample", "create", "--priority", "high", "-h"],
            &["sample", "create", "--help"],
        ),
    ] {
        let (output, office) = run_stub(&table, &args(short), &mut StubSources::default());
        let (expected, _) = run_stub(&table, &args(long), &mut StubSources::default());
        assert_eq!(output, expected, "{short:?}");
        assert_eq!(output.exit_code, 0, "{short:?}");
        assert!(office.calls.is_empty(), "{short:?}");
    }
    // As the value of a flag that takes one, `-h` is that value.
    let request = sent(&table, &["sample", "create", "--title", "-h"]);
    assert_eq!(Value::Object(request.input), json!({"title": "-h"}));
}

#[test]
fn command_help_ends_with_its_example_as_one_line_a_shell_reads_back() {
    let table = sample_table();
    let (output, _) = run_stub(
        &table,
        &args(&["sample", "create", "--help"]),
        &mut StubSources::default(),
    );
    let example = output
        .stdout
        .split("\nExample:\n  ")
        .nth(1)
        .expect("an Example section");
    assert!(
        example.ends_with('\n') && example.lines().count() == 1,
        "{example}"
    );
    assert!(
        example.starts_with("agent-os sample create --title 1.5 "),
        "{example}"
    );
    // A list repeats its flag, a yes-or-no is bare, text a shell would split is quoted and
    // fields with structure go in one --input-json object, in name order.
    for part in [
        "--body 'Short body'",
        "--labels 'a b' --labels yes",
        "--urgent",
        r#"--input-json '{"extra":{"a":[null],"z":1},"owner":{"id":"E-1","name":"Ada"}}'"#,
    ] {
        assert!(example.contains(part), "`{part}` is not in {example}");
    }
    let (inspect, _) = run_stub(
        &table,
        &args(&["sample", "update", "--help"]),
        &mut StubSources::default(),
    );
    assert!(
        inspect.stdout.ends_with(
            "\nExample:\n  agent-os sample update --id S-1 --title Renamed --if-match 1\n"
        ),
        "{}",
        inspect.stdout
    );
    let words =
        org_command::testkit::shell_words("a 'b c' 'it'\\''s' --input-json '{\"k\":\"v\"}'");
    assert_eq!(words, ["a", "b c", "it's", "--input-json", r#"{"k":"v"}"#]);
}

#[test]
fn json_escapes_del_c1_and_line_separators_and_reads_back_the_same() {
    let hostile =
        "ok\u{7f}del\u{80}c1\u{9b}31mcsi\u{9f}\u{2028}ls\u{2029}ps\u{1b}[2Jesc\u{a0}nbsp é";
    let node = Node::map([
        ("text", Node::text(hostile)),
        (hostile, Node::List(vec![Node::text(hostile)])),
    ]);
    let json = node.to_json();
    for raw in [
        '\u{7f}', '\u{80}', '\u{9b}', '\u{9f}', '\u{2028}', '\u{2029}', '\u{1b}',
    ] {
        assert!(!json.contains(raw), "{raw:?} is raw in {json}");
    }
    for escape in [
        "\\u007f", "\\u0080", "\\u009b", "\\u009f", "\\u2028", "\\u2029", "\\u001b",
    ] {
        assert!(json.contains(escape), "{escape} is not in {json}");
    }
    // Printable text past the C1 block is kept as it is.
    assert!(json.contains("\u{a0}nbsp é"), "{json}");
    let back: Value = serde_json::from_str(&json).expect("valid JSON");
    assert_eq!(back, node.to_value());
    assert_eq!(json.lines().count(), 1);
}

/// B-07: the bidirectional marks, embeddings, overrides and isolates never reach a reader
/// raw in any printed form (JSON, compact, YAML), and the JSON reads back the same.
#[test]
fn every_format_escapes_bidi_controls_and_json_reads_back_the_same() {
    const BIDI: [char; 12] = [
        '\u{061c}', '\u{200e}', '\u{200f}', '\u{202a}', '\u{202b}', '\u{202c}', '\u{202d}',
        '\u{202e}', '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}',
    ];
    let hostile: String = BIDI.iter().flat_map(|c| ['x', *c]).collect();
    let node = Node::map([
        ("office", Node::text(hostile.as_str())),
        (
            hostile.as_str(),
            Node::List(vec![Node::text(hostile.as_str())]),
        ),
    ]);
    let json = node.to_json();
    let compact = node.to_compact(&[]);
    let yaml = node.to_yaml();
    for c in BIDI {
        let escape = format!("\\u{:04x}", u32::from(c));
        for (format, printed) in [("json", &json), ("compact", &compact), ("yaml", &yaml)] {
            assert!(!printed.contains(c), "{c:?} is raw in {format}: {printed}");
            assert!(
                printed.contains(&escape),
                "{escape} not in {format}: {printed}"
            );
        }
    }
    let back: Value = serde_json::from_str(&json).expect("valid JSON");
    assert_eq!(back, node.to_value());
    // Neighbours of the escaped ranges are ordinary text, printed as they are.
    let plain = Node::text("\u{2027}\u{202f}\u{2065}\u{206a}");
    assert!(plain.to_json().contains("\u{2027}\u{202f}\u{2065}\u{206a}"));
}

#[test]
fn a_refusal_envelope_line_matches_the_envelope_schema_and_keeps_the_hint() {
    let mut refusal = Refusal::new(
        ErrorCode::NotAuthorized,
        "No office binding.\u{9b}31m",
        "Run agent-os inside an office-started agent session.",
    );
    refusal.operation_id = Some("op-1".to_owned());
    let line = org_command::failure_envelope_json(&refusal, true);
    assert!(!line.contains('\n') && !line.contains('\u{9b}'), "{line}");
    let envelope: Value = serde_json::from_str(&line).expect("one JSON line");
    validate_schema(&envelope_schema(), &envelope).expect("the envelope schema");
    assert_eq!(
        envelope,
        json!({"ok": false, "data": null, "meta": {"operation_id": "op-1", "dry_run": true}, "error": {
            "code": "not_authorized", "message": "No office binding.\u{9b}31m",
            "hint": "Run agent-os inside an office-started agent session."
        }})
    );
    assert!(
        line.starts_with(r#"{"ok":false,"data":null,"meta":{"#),
        "{line}"
    );
}

#[test]
fn a_table_refuses_a_row_that_breaks_the_grammar() {
    let good = SAMPLE_ROWS[0];
    const RESERVED: &[Field] = &[Field::optional("dry_run", Type::Boolean, "No")];
    let cases: Vec<(Command, &str)> = vec![
        (good, "is in the table twice"),
        (
            Command {
                resource: "Sample",
                verb: "show",
                ..good
            },
            "lower-case words",
        ),
        (
            Command {
                verb: "show_all",
                ..good
            },
            "lower-case words",
        ),
        (
            Command {
                verb: "a-b-c",
                ..good
            },
            "lower-case words",
        ),
        (
            Command {
                verb: "show",
                summary: "two\nlines",
                ..good
            },
            "the summary is one line",
        ),
        (
            Command {
                verb: "show",
                input: RESERVED,
                example: Example {
                    input: "{}",
                    ..good.example
                },
                ..good
            },
            "name of a common option",
        ),
        (
            Command {
                verb: "show",
                content_field: Some("body"),
                ..good
            },
            "content field `body` is not a text field",
        ),
        (
            Command {
                verb: "show",
                example: Example {
                    input: "{}",
                    ..good.example
                },
                ..good
            },
            "the example input: `id` is required",
        ),
        (
            Command {
                verb: "show",
                example: Example {
                    output: "[]",
                    ..good.example
                },
                ..good
            },
            "the example output",
        ),
    ];
    for (row, expected) in cases {
        let rows: &'static [Command] = Box::leak(Box::new([good, row]));
        let error = CommandTable::new(&[rows]).expect_err(expected);
        assert!(
            error.to_string().contains(expected),
            "`{expected}` is not in: {error}"
        );
    }
}

#[test]
fn the_contract_checks_catch_a_broken_office_answer() {
    // The stub's recorded output is the row's example; an answer outside the row's output
    // fails validation against the schema `--schema` prints.
    let table = sample_table();
    let (output, _) = run_stub(
        &table,
        &args(&["sample", "ping", "--schema"]),
        &mut StubSources::default(),
    );
    let schema = json_envelope(&output, "schema")["data"]["output"].clone();
    assert!(validate_schema(&schema, &json!({"office": "O-1"})).is_ok());
    for bad in [
        json!({"office": 1}),
        json!({}),
        json!({"office": "O-1", "more": true}),
        json!([]),
    ] {
        assert!(validate_schema(&schema, &bad).is_err(), "{bad}");
    }
    assert!(check_schema(&json!({"type": "strng"})).is_err());
    assert!(check_schema(&json!({"typo": "string"})).is_err());
    // The envelope's own schema is held to the same check as a row's, and a required name
    // that the schema's properties do not list is still caught.
    assert_eq!(check_schema(&envelope_schema()), Ok(()));
    assert!(check_schema(&json!({"properties": {"a": {}}, "required": ["b"]})).is_err());
    assert!(check_schema(&json!({"required": [1]})).is_err());
    // A number with no fractional part is an integer to the schema, as it is to the parser.
    let integer = json!({"type": "integer", "minimum": 0});
    assert!(validate_schema(&integer, &json!(1.0)).is_ok());
    assert!(validate_schema(&integer, &json!(1.5)).is_err());
    assert!(validate_schema(&integer, &json!(-1.0)).is_err());
    // The unchecked table of an executable's start holds the same rows in the same order.
    let unchecked = CommandTable::unchecked(&[&EXTRA_ROWS, &SAMPLE_ROWS]);
    let checked = CommandTable::new(&[&SAMPLE_ROWS, &EXTRA_ROWS]).expect("the table");
    assert_eq!(unchecked.catalogue(), checked.catalogue());
    let _ = StubOffice::default();
}

/// Runs one line against `office` with `keys`.
fn run_with(
    table: &CommandTable,
    line: &[&str],
    keys: &mut dyn KeySource,
    office: &mut dyn Office,
) -> Output {
    run(
        table,
        "agent-os",
        &args(line),
        &mut StubSources::default(),
        keys,
        office,
    )
}

fn answered(table: &CommandTable, line: &[&str], answer: Result<Answer, Refusal>) -> Output {
    run_with(table, line, &mut StubKeys::default(), &mut Fixed(answer))
}

#[test]
fn what_a_command_reads_is_bounded() {
    let table = sample_table();
    let message_of = |line: &[&str], sources: &mut StubSources| {
        let (output, office) = run_stub(&table, &args(line), sources);
        let envelope = json_envelope(&output, &format!("{line:?}"));
        assert_eq!(
            (output.exit_code, &envelope["error"]["code"]),
            (2, &json!("invalid_request")),
            "{line:?}"
        );
        assert!(office.calls.is_empty(), "{line:?}: the office was called");
        format!(
            "{} {}",
            envelope["error"]["message"].as_str().expect("a message"),
            envelope["error"]["hint"].as_str().expect("a hint")
        )
    };
    assert_eq!(
        (MAX_INPUT_BYTES, MAX_YAML_INPUT_BYTES),
        (1024 * 1024, 64 * 1024)
    );
    let text = &["sample", "create", "--title", "A", "--stdin"][..];
    let file = &["sample", "create", "--title", "A", "--input-file", "f"][..];
    let json_file = &[
        "sample",
        "create",
        "--input-file",
        "f",
        "--input-format",
        "json",
    ][..];
    let yaml = &["sample", "create", "--stdin", "--input-format", "yaml"][..];

    // Text of exactly the limit is taken whole; one byte more is refused, and the source was
    // asked for no more than the limit and one byte, however much it holds.
    for (line, limit) in [(text, MAX_INPUT_BYTES), (file, MAX_INPUT_BYTES)] {
        let sources = |len: usize| StubSources {
            files: BTreeMap::from([("f".to_owned(), vec![b'x'; len])]),
            stdin: Some(vec![b'x'; len]),
            ..StubSources::default()
        };
        let mut at_limit = sources(limit);
        let (output, office) = run_stub(&table, &args(line), &mut at_limit);
        assert_eq!(output.exit_code, 0, "{line:?}");
        assert_eq!(
            office.calls[0].input["body"].as_str().map(str::len),
            Some(limit)
        );
        assert_eq!(at_limit.last_limit, Some(limit));
        for len in [limit + 1, 8 * limit] {
            let message = message_of(line, &mut sources(len));
            assert!(
                message.contains("is longer than the 1048576 bytes a command reads"),
                "{message}"
            );
            assert!(
                message.contains("text and JSON are at most 1 MiB (1048576 bytes), YAML at most 64 KiB (65536 bytes)"),
                "{message}"
            );
        }
    }
    // A JSON file has the same limit as text.
    let padded = |len: usize| {
        let mut json = br#"{"title":"A","body":""#.to_vec();
        json.resize(len - 2, b'x');
        json.extend_from_slice(b"\"}");
        json
    };
    let mut sources = StubSources {
        files: BTreeMap::from([("f".to_owned(), padded(MAX_INPUT_BYTES))]),
        ..StubSources::default()
    };
    let (output, _) = run_stub(&table, &args(json_file), &mut sources);
    assert_eq!(output.exit_code, 0, "{}", output.stdout);
    let mut sources = StubSources {
        files: BTreeMap::from([("f".to_owned(), padded(MAX_INPUT_BYTES + 1))]),
        ..StubSources::default()
    };
    let message = message_of(json_file, &mut sources);
    assert!(message.contains("--input-file is longer"), "{message}");
    // YAML keeps the YAML reader's limit, and the source is asked for no more than that.
    let padded = |len: usize| {
        let mut yaml = b"title: A\nbody: \"".to_vec();
        yaml.resize(len - 1, b'x');
        yaml.push(b'"');
        yaml
    };
    let mut sources = StubSources {
        stdin: Some(padded(MAX_YAML_INPUT_BYTES)),
        ..StubSources::default()
    };
    let (output, _) = run_stub(&table, &args(yaml), &mut sources);
    assert_eq!(output.exit_code, 0, "{}", output.stdout);
    assert_eq!(sources.last_limit, Some(MAX_YAML_INPUT_BYTES));
    let mut sources = StubSources {
        stdin: Some(padded(MAX_YAML_INPUT_BYTES + 1)),
        ..StubSources::default()
    };
    let message = message_of(yaml, &mut sources);
    assert!(
        message.contains("--stdin is longer than the 65536 bytes"),
        "{message}"
    );
    // The value of --input-json has the limit of a JSON file.
    let long = format!(r#"{{"title":"{}"}}"#, "x".repeat(MAX_INPUT_BYTES));
    let message = message_of(
        &["sample", "create", "--input-json", &long],
        &mut StubSources::default(),
    );
    assert!(message.contains("--input-json is longer"), "{message}");
}

#[test]
fn data_is_exactly_what_the_row_declares_in_every_format() {
    let table = sample_table();
    // A field the row does not declare is dropped, at every depth and in every format.
    let record = json!({
        "id": "S-1", "title": "T", "state": "open", "labels": [], "revision": 1,
        "owner": {"id": "E-1", "secret": "x"}, "internal_note": "x", "zeta": 1
    });
    let expected = json!({
        "id": "S-1", "title": "T", "state": "open", "labels": [], "revision": 1,
        "owner": {"id": "E-1"}
    });
    for (line, data, expected) in [
        (
            &["sample", "inspect", "--id", "S-1"][..],
            record.clone(),
            expected.clone(),
        ),
        (
            &["sample", "list"],
            json!([record.clone()]),
            json!([expected.clone()]),
        ),
    ] {
        let output = answered(&table, line, Ok(Answer::completed(data.clone())));
        assert_eq!(json_envelope(&output, "json")["data"], expected, "{line:?}");
        let mut yaml = line.to_vec();
        yaml.extend(["--format", "yaml"]);
        let output = answered(&table, &yaml, Ok(Answer::completed(data.clone())));
        let read = org_docs::parse_mapping(&output.stdout).expect("yaml");
        assert_eq!(read["data"], expected, "{line:?}");
        let mut compact = line.to_vec();
        compact.extend(["--format", "compact"]);
        let output = answered(&table, &compact, Ok(Answer::completed(data)));
        assert_eq!(output.exit_code, 0);
        for dropped in ["secret", "internal_note", "zeta"] {
            assert!(
                !output.stdout.contains(dropped),
                "{line:?}: {}",
                output.stdout
            );
        }
    }
    let output = answered(
        &table,
        &["sample", "inspect", "--id", "S-1", "--format", "compact"],
        Ok(Answer::completed(record.clone())),
    );
    assert_eq!(
        output.stdout,
        "id: S-1\ntitle: T\nstate: open\nlabels: []\nowner: {\"id\":\"E-1\"}\nrevision: 1\n"
    );

    // An answer that is not the declared one is a failure in every format: exit code 1, the
    // JSON envelope, no data.
    for (line, data, problem) in [
        (
            &["sample", "ping"][..],
            json!({"zeta": 1}),
            "`office` is required",
        ),
        (
            &["sample", "ping"],
            json!({"office": 7}),
            "`office` must be text",
        ),
        (
            &["sample", "ping"],
            json!("O-1"),
            "the answer is not an object",
        ),
        (
            &["sample", "ping"],
            json!(null),
            "the answer is not an object",
        ),
        (
            &["sample", "inspect", "--id", "S-1"],
            json!({"id": "S-1", "title": "T", "state": "open", "labels": [], "revision": 1, "owner": {"name": "Ada"}}),
            "`owner.id` is required",
        ),
        (
            &["sample", "list"],
            json!([expected.clone(), {"id": "S-2"}]),
            "`[1].title` is required",
        ),
        (
            &["sample", "list"],
            json!({"id": "S-2"}),
            "a list answers an array",
        ),
    ] {
        for format in ["json", "yaml", "compact"] {
            let mut line = line.to_vec();
            line.extend(["--format", format]);
            let output = answered(&table, &line, Ok(Answer::completed(data.clone())));
            let envelope = json_envelope(&output, &format!("{line:?}"));
            assert_eq!(output.exit_code, 1, "{line:?}");
            assert_eq!(envelope["ok"], json!(false));
            assert_eq!(envelope["data"], Value::Null);
            assert_eq!(envelope["error"]["code"], json!("unavailable"), "{line:?}");
            assert!(
                envelope["error"]["message"]
                    .as_str()
                    .is_some_and(|message| message.contains(problem)),
                "{line:?}: {envelope}"
            );
            assert!(envelope["error"].get("retry_with").is_none());
        }
    }
    // For a write the change may have happened: the key is reported and the hint says so.
    let output = answered(
        &table,
        &["sample", "create", "--title", "A"],
        Ok(Answer::completed(json!({"id": "S-9"}))),
    );
    let envelope = json_envelope(&output, "a write");
    assert_eq!(output.exit_code, 1);
    assert_eq!(envelope["meta"], json!({"operation_id": "key-1"}));
    assert!(
        envelope["error"]["hint"]
            .as_str()
            .is_some_and(|hint| hint.contains("The change may have been made")),
        "{envelope}"
    );
}

/// An office that answers every row's example with one more field than the row declares.
struct Chatty(StubOffice);

impl Office for Chatty {
    fn call(&mut self, request: &Request) -> Result<Answer, Refusal> {
        let mut answer = self.0.call(request)?;
        match &mut answer.data {
            Value::Object(record) => {
                record.insert("internal".to_owned(), json!(true));
            }
            Value::Array(items) => {
                for item in items.iter_mut().filter_map(Value::as_object_mut) {
                    item.insert("internal".to_owned(), json!(true));
                }
            }
            _ => {}
        }
        Ok(answer)
    }
}

/// An office that leaves a required field out of one command's answer.
struct Forgetful(StubOffice);

impl Office for Forgetful {
    fn call(&mut self, request: &Request) -> Result<Answer, Refusal> {
        let mut answer = self.0.call(request)?;
        if let (Value::Object(record), "sample.ping") = (&mut answer.data, request.command.call) {
            record.remove("office");
        }
        Ok(answer)
    }
}

#[test]
fn the_checks_run_against_a_given_office() {
    let snapshots = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    check_contract_with(&sample_table(), &snapshots, &stub_office);
    let (output, recorded) = run_recorded(
        &sample_table(),
        &args(&["sample", "ping"]),
        &mut StubSources::default(),
        &|| Box::new(Chatty(StubOffice::default())),
    );
    // The library drops the field; the record of what the office answered still has it.
    assert_eq!(
        json_envelope(&output, "ping")["data"],
        json!({"office": "O-1"})
    );
    let answer = recorded.answers[0].as_ref().expect("an answer");
    assert_eq!(answer.data, json!({"office": "O-1", "internal": true}));
}

#[test]
#[should_panic(
    expected = "the office answered outside the row's declared output: `internal` is not a field"
)]
fn the_checks_fail_an_office_that_answers_an_undeclared_field() {
    check_rows_with(&sample_table(), &|| Box::new(Chatty(StubOffice::default())));
}

#[test]
#[should_panic(expected = "sample ping: the example is refused")]
fn the_checks_fail_an_office_that_leaves_out_a_required_field() {
    check_rows_with(&sample_table(), &|| {
        Box::new(Forgetful(StubOffice::default()))
    });
}

/// A key source that gives what the test says.
struct Keys(&'static str, usize);

impl KeySource for Keys {
    fn new_key(&mut self) -> String {
        self.1 += 1;
        self.0.to_owned()
    }
}

#[test]
fn a_write_without_a_key_is_given_one() {
    let table = sample_table();
    let create = &["sample", "create", "--title", "A"][..];
    let run_counted = |line: &[&str], answer: Option<Refusal>| {
        let mut keys = StubKeys::default();
        let mut office = StubOffice {
            refuse: answer,
            ..StubOffice::default()
        };
        let output = run_with(&table, line, &mut keys, &mut office);
        (output, office.calls, keys.made)
    };

    // A write with no key: one is asked for, sent to the office and reported.
    let (output, calls, made) = run_counted(create, None);
    assert_eq!((output.exit_code, made), (0, 1));
    assert_eq!(calls[0].idempotency_key.as_deref(), Some("key-1"));
    assert_eq!(
        json_envelope(&output, "a write")["meta"],
        json!({"status": "completed", "operation_id": "key-1", "ack": "committed"})
    );
    // The caller's key is used as given, and none is made.
    let mut with_key = create.to_vec();
    with_key.extend(["--idempotency-key", "mine-7"]);
    let (output, calls, made) = run_counted(&with_key, None);
    assert_eq!(made, 0);
    assert_eq!(calls[0].idempotency_key.as_deref(), Some("mine-7"));
    assert_eq!(
        json_envelope(&output, "a write")["meta"]["operation_id"],
        json!("mine-7")
    );
    // No key is made for a read, a dry run, help, a schema or a line that cannot be read.
    for (line, exit_code, called) in [
        (&["sample", "inspect", "--id", "S-1"][..], 0, true),
        (&["sample", "list"], 0, true),
        (&["sample", "create", "--title", "A", "--dry-run"], 0, true),
        (&["sample", "create", "--help"], 0, false),
        (&["sample", "create", "--schema"], 0, false),
        (&["sample", "create"], 2, false),
        (&["sample", "create", "--title", "A", "stray"], 2, false),
    ] {
        let (output, calls, made) = run_counted(line, None);
        assert_eq!((output.exit_code, made), (exit_code, 0), "{line:?}");
        assert_eq!(calls.len(), usize::from(called), "{line:?}");
        if let Some(call) = calls.first() {
            assert_eq!(call.idempotency_key, None, "{line:?}");
            let envelope = json_envelope(&output, "no key");
            assert!(envelope["meta"].get("operation_id").is_none(), "{line:?}");
        }
    }

    // On failure the key is reported too. For `unavailable`, `not_ready` and `timeout`,
    // `retry_with` carries it, beside what the office already said to change.
    let refusal = |code| Refusal::new(code, "It did not answer.", "Repeat the call.");
    for code in [
        ErrorCode::Unavailable,
        ErrorCode::NotReady,
        ErrorCode::Timeout,
    ] {
        let (output, _, _) = run_counted(create, Some(refusal(code)));
        let envelope = json_envelope(&output, "a failure");
        assert_eq!(output.exit_code, 1);
        assert_eq!(envelope["meta"], json!({"operation_id": "key-1"}));
        assert_eq!(
            envelope["error"]["retry_with"],
            json!({"idempotency-key": "key-1"})
        );
    }
    let told = Refusal {
        retry_with: Some(Map::from_iter([("durable".to_owned(), json!(false))])),
        ..refusal(ErrorCode::Unavailable)
    };
    let (output, _, _) = run_counted(&with_key, Some(told));
    assert_eq!(
        json_envelope(&output, "a failure")["error"]["retry_with"],
        json!({"durable": false, "idempotency-key": "mine-7"})
    );
    // Any other failure reports the key and adds nothing to `retry_with`.
    let (output, _, _) = run_counted(create, Some(refusal(ErrorCode::NotAuthorized)));
    let envelope = json_envelope(&output, "a refusal");
    assert_eq!(envelope["meta"], json!({"operation_id": "key-1"}));
    assert!(envelope["error"].get("retry_with").is_none());
    // The office's own operation ID is the one reported, on success and on failure.
    let named = Refusal {
        operation_id: Some("op-41".to_owned()),
        ..refusal(ErrorCode::Unavailable)
    };
    let (output, _, _) = run_counted(create, Some(named));
    let envelope = json_envelope(&output, "a failure");
    assert_eq!(envelope["meta"], json!({"operation_id": "op-41"}));
    assert_eq!(
        envelope["error"]["retry_with"],
        json!({"idempotency-key": "key-1"})
    );
    let row = table.find("sample", "create").expect("the row");
    let answer = Answer {
        operation_id: Some("op-42".to_owned()),
        ..Answer::completed(serde_json::from_str(row.example.output).expect("example"))
    };
    let output = answered(&table, create, Ok(answer));
    assert_eq!(
        json_envelope(&output, "a write")["meta"],
        json!({"status": "completed", "operation_id": "op-42"})
    );
    // A read that fails has no key to carry.
    let (output, _, _) = run_counted(&["sample", "list"], Some(refusal(ErrorCode::Unavailable)));
    let envelope = json_envelope(&output, "a read");
    assert_eq!(envelope["meta"], json!({}));
    assert!(envelope["error"].get("retry_with").is_none());

    // A key source that gives something that is not a key: nothing is sent.
    for bad in ["", "not a key", "ünïcödé"] {
        let mut keys = Keys(bad, 0);
        let mut office = StubOffice::default();
        let output = run_with(&table, create, &mut keys, &mut office);
        let envelope = json_envelope(&output, "a bad key");
        assert_eq!((output.exit_code, keys.1), (1, 1));
        assert_eq!(envelope["error"]["code"], json!("unavailable"));
        assert!(office.calls.is_empty());
    }
    let mut keys = Keys("0193-abc", 0);
    let mut office = StubOffice::default();
    let output = run_with(&table, create, &mut keys, &mut office);
    assert_eq!(output.exit_code, 0);
    assert_eq!(office.calls[0].idempotency_key.as_deref(), Some("0193-abc"));
}

#[test]
fn a_whole_number_is_read_the_same_from_a_flag_and_from_json() {
    let table = sample_table();
    let largest = u64::MAX.to_string();
    for number in ["2", "-9223372036854775808", largest.as_str()] {
        let by_flag = sent(
            &table,
            &["sample", "create", "--title", "A", "--priority", number],
        );
        let json = format!(r#"{{"title":"A","priority":{number}}}"#);
        let by_json = sent(&table, &["sample", "create", "--input-json", &json]);
        assert_eq!(by_flag.input, by_json.input, "{number}");
        assert_eq!(by_flag.input["priority"].to_string(), number);
        // Both ways at once are the same value, not two.
        sent(
            &table,
            &[
                "sample",
                "create",
                "--title",
                "A",
                "--priority",
                number,
                "--input-json",
                &json,
            ],
        );
    }
    for (line, expected) in [
        (
            &[
                "sample",
                "create",
                "--title",
                "A",
                "--priority",
                "18446744073709551616",
            ][..],
            "--priority takes a whole number",
        ),
        (
            &["sample", "create", "--title", "A", "--priority", "2.0"],
            "--priority takes a whole number",
        ),
        (
            &[
                "sample",
                "create",
                "--input-json",
                r#"{"title":"A","priority":1.5}"#,
            ],
            "`priority` must be a whole number",
        ),
        (
            &[
                "sample",
                "create",
                "--input-json",
                r#"{"title":"A","priority":1e300}"#,
            ],
            "`priority` must be a whole number",
        ),
    ] {
        let message = refused(&table, line);
        assert!(message.contains(expected), "{line:?}: {message}");
    }
    // JSON Schema's integer is any number with no fractional part. `2.0` in JSON or YAML is
    // the integer 2, and the office receives 2: the same value as --priority 2.
    let request = sent(
        &table,
        &[
            "sample",
            "create",
            "--priority",
            "2",
            "--input-json",
            r#"{"title":"A","priority":2.0}"#,
        ],
    );
    assert_eq!(request.input["priority"].to_string(), "2");
    let mut sources = StubSources {
        stdin: Some(b"title: A\npriority: 2.0\n".to_vec()),
        ..StubSources::default()
    };
    let line = args(&["sample", "create", "--stdin", "--input-format", "yaml"]);
    let (output, office) = run_stub(&table, &line, &mut sources);
    assert_eq!(output.exit_code, 0, "{}", output.stdout);
    assert_eq!(office.calls[0].input["priority"].to_string(), "2");
    // The same holds for what the office answers.
    let output = answered(
        &table,
        &["sample", "inspect", "--id", "S-1"],
        Ok(Answer::completed(json!({
            "id": "S-1", "title": "T", "state": "open", "labels": [], "revision": 3.0
        }))),
    );
    assert!(
        output.stdout.contains("\"revision\":3}"),
        "{}",
        output.stdout
    );
}

#[test]
fn a_name_given_twice_in_one_object_is_refused() {
    let table = sample_table();
    for (json, key) in [
        (r#"{"title":"A","title":"B"}"#, "title"),
        (r#"{"title":"A","title":"A"}"#, "title"),
        (r#"{"title":"A","title":"A"}"#, "title"),
        (r#"{"title":"A","owner":{"id":"E-1","id":"E-2"}}"#, "id"),
        (
            r#"{"title":"A","extra":[{"a":1},{"b":{"é\"":1,"é\"":2}}]}"#,
            "é\"",
        ),
    ] {
        let expected = format!("gives `{key}` twice in one object");
        let message = refused(&table, &["sample", "create", "--input-json", json]);
        assert!(
            message.contains(&format!("--input-json {expected}")),
            "{json}: {message}"
        );
        let mut sources = StubSources {
            files: BTreeMap::from([("in.json".to_owned(), json.as_bytes().to_vec())]),
            ..StubSources::default()
        };
        let line = args(&[
            "sample",
            "create",
            "--input-file",
            "in.json",
            "--input-format",
            "json",
        ]);
        let (output, office) = run_stub(&table, &line, &mut sources);
        let envelope = json_envelope(&output, json);
        assert_eq!((output.exit_code, office.calls.len()), (2, 0), "{json}");
        assert!(
            envelope["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains(&format!("--input-file {expected}"))),
            "{json}: {envelope}"
        );
    }
    // The same name in two different objects, and text that only looks like a name, are fine.
    let request = sent(
        &table,
        &[
            "sample",
            "create",
            "--input-json",
            r#"{"title":"\"title\":","body":"{\"title\":1,\"title\":2}","owner":{"id":"E-1"},"extra":[{"id":1},{"id":2},["title","title"]]}"#,
        ],
    );
    assert_eq!(request.input["title"], json!("\"title\":"));
    // YAML refuses the same, and says where.
    let mut sources = StubSources {
        stdin: Some(b"title: A\nowner:\n  id: E-1\n  id: E-2\n".to_vec()),
        ..StubSources::default()
    };
    let line = args(&["sample", "create", "--stdin", "--input-format", "yaml"]);
    let (output, office) = run_stub(&table, &line, &mut sources);
    let envelope = json_envelope(&output, "yaml");
    assert_eq!((output.exit_code, office.calls.len()), (2, 0));
    assert_eq!(
        envelope["error"]["message"],
        json!("--stdin is not a YAML object: at `owner.id`, the name is given twice.")
    );
}

#[test]
fn compact_prints_meta_only_when_the_caller_needs_it() {
    let table = sample_table();
    let item = json!({"id": "S-1", "title": "T", "state": "open", "labels": [], "revision": 1});
    let list = &[
        "sample", "list", "--format", "compact", "--fields", "id,title",
    ][..];
    // A page with another after it: the last line is `meta`, with the cursor.
    let mut page = Answer::completed(json!([item.clone()]));
    page.next_cursor = Some("c-2".to_owned());
    let output = answered(&table, list, Ok(page));
    assert_eq!(
        output.stdout,
        "S-1\tT\nmeta: {\"status\":\"completed\",\"next_cursor\":\"c-2\"}\n"
    );
    // The last page, completed: `data` alone.
    let output = answered(&table, list, Ok(Answer::completed(json!([item.clone()]))));
    assert_eq!(output.stdout, "S-1\tT\n");
    // An answer that did not complete says so, for a record and for an empty list.
    for status in Status::ALL {
        let output = answered(
            &table,
            &["sample", "ping", "--format", "compact"],
            Ok(answer_with(status, json!({"office": "O-1"}))),
        );
        let expected = match status {
            Status::Completed => "office: O-1\n".to_owned(),
            status => format!(
                "office: O-1\nmeta: {{\"status\":\"{}\"}}\n",
                status.as_str()
            ),
        };
        assert_eq!(output.stdout, expected);
        assert_eq!(output.exit_code, 0);
    }
    let output = answered(&table, list, Ok(answer_with(Status::Suspended, json!([]))));
    assert_eq!(output.stdout, "meta: {\"status\":\"suspended\"}\n");
    // A write that waits for approval: `meta` is whole, on one line.
    let row = table.find("sample", "update").expect("the row");
    let mut gated = answer_with(
        Status::GateRequired,
        serde_json::from_str(row.example.output).expect("example"),
    );
    gated.ack = Some(Ack::Durable);
    let output = answered(
        &table,
        &[
            "sample",
            "update",
            "--id",
            "S-1",
            "--if-match",
            "3",
            "--format",
            "compact",
            "--fields",
            "id",
        ],
        Ok(gated),
    );
    assert_eq!(
        output.stdout,
        "id: S-1\nmeta: {\"status\":\"gate_required\",\"operation_id\":\"key-1\",\"ack\":\"durable\"}\n"
    );
}

#[test]
fn compact_keeps_one_value_on_one_line_and_tells_values_apart() {
    let table = sample_table();
    for (text, printed) in [
        ("O-1", "O-1"),
        ("two words: and # marks", "two words: and # marks"),
        ("", "\"\""),
        ("null", "\"null\""),
        ("\"quoted", "\"\\\"quoted\""),
        ("a\tb\nc", "\"a\\tb\\nc\""),
        ("a\u{2028}b\u{2029}c", "\"a\\u2028b\\u2029c\""),
        ("a\u{85}b\u{7f}c", "\"a\\u0085b\\u007fc\""),
        ("ünïcödé — 日本語", "ünïcödé — 日本語"),
        // No text can pass for the call's `meta:` line.
        (
            "meta: {\"status\":\"suspended\"}",
            "\"meta: {\\\"status\\\":\\\"suspended\\\"}\"",
        ),
        ("metadata", "metadata"),
        ("a meta: b", "a meta: b"),
        (" meta: x", "\" meta: x\""),
        ("META: x", "\"META: x\""),
        (
            "\u{a0}\u{feff}\u{200b}Meta: x",
            "\"\u{a0}\u{feff}\u{200b}Meta: x\"",
        ),
    ] {
        let output = answered(
            &table,
            &["sample", "ping", "--format", "compact"],
            Ok(Answer::completed(json!({"office": text}))),
        );
        assert_eq!(output.stdout, format!("office: {printed}\n"), "{text:?}");
        assert_eq!(output.stdout.split('\n').count(), 2, "{text:?}");
        // What is printed as a JSON string reads back as the text.
        if printed.starts_with('"') {
            assert_eq!(serde_json::from_str::<String>(printed).expect("JSON"), text);
        }
    }
    // In a list a cell never holds a tab or a line break, in text or inside a value.
    let output = answered(
        &table,
        &[
            "sample",
            "list",
            "--format",
            "compact",
            "--fields",
            "id,title,labels,owner",
        ],
        Ok(Answer::completed(json!([
            {"id": "S-1", "title": "a\tb", "state": "open", "labels": ["x\u{2028}y", ""], "revision": 1},
            {"id": "S-2", "title": "", "state": "open", "labels": [], "owner": {"id": "E\n1"}, "revision": 1}
        ]))),
    );
    assert_eq!(
        output.stdout,
        "S-1\t\"a\\tb\"\t[\"x\\u2028y\",\"\"]\t\nS-2\t\"\"\t[]\t{\"id\":\"E\\n1\"}\n"
    );
    // `null`, empty text and a field that is not there are three different cells; a name that
    // is not a plain word is printed as a JSON string.
    let record = Node::map([
        ("nothing", Node::Null),
        ("empty", Node::text("")),
        ("odd\u{2028}name: x", Node::text("v")),
        ("", Node::Bool(true)),
        ("meta", Node::text("x")),
        ("Meta", Node::text("y")),
    ]);
    assert_eq!(
        record.to_compact(&[]),
        "nothing: null\nempty: \"\"\n\"odd\\u2028name: x\": v\n\"\": true\n\"meta\": x\n\"Meta\": y\n"
    );
    // In a list of one column a title cannot end the page as a forged `meta:` line.
    let output = answered(
        &table,
        &["sample", "list", "--format", "compact", "--fields", "title"],
        Ok(Answer::completed(json!([
            {"id": "S-1", "title": "meta: {\"status\":\"suspended\"}", "state": "open", "labels": [], "revision": 1}
        ]))),
    );
    assert!(output.stdout.starts_with("\"meta: "), "{}", output.stdout);
    let list = Node::List(vec![
        Node::map([("a", Node::Null)]),
        Node::map([("a", Node::text("")), ("b", Node::text("null"))]),
        Node::map([("b", Node::text("x"))]),
    ]);
    assert_eq!(
        list.to_compact(&["a", "b"]),
        "null\t\n\"\"\t\"null\"\n\tx\n"
    );
}

/// `check` reads a line exactly as `run` does and stops there: it refuses what `run` refuses
/// with exit code 2, with the same message, and accepts what `run` would answer or send, without
/// asking for a key or calling the office.
#[test]
fn check_reads_a_line_like_run_and_calls_nothing() {
    let table = sample_table();
    let lines: [&[&str]; 9] = [
        &["sample", "inspect", "--id", "S-1"],
        &["sample", "inspect", "--help"],
        &["sample", "list", "--schema"],
        &["sample", "create", "--title", "A", "--body", "B"],
        &["sample", "destroy"],
        &["sample", "inspect", "S-1"],
        &["sample", "inspect", "--id"],
        &["sample", "inspect", "--id", "S-1", "--dry-run"],
        &["sample", "inspect", "--id", "S-1", "--nope", "x"],
    ];
    for line in lines {
        let checked =
            org_command::check(&table, "agent-os", &args(line), &mut StubSources::default());
        let (output, office) = run_stub(&table, &args(line), &mut StubSources::default());
        match checked {
            Ok(()) => assert_ne!(output.exit_code, 2, "{line:?}: {}", output.stdout),
            Err(refusal) => {
                assert_eq!(output.exit_code, 2, "{line:?}");
                assert_eq!(refusal.code, ErrorCode::InvalidRequest);
                let envelope = json_envelope(&output, &format!("{line:?}"));
                assert_eq!(envelope["error"]["message"], json!(refusal.message));
                assert_eq!(envelope["error"]["hint"], json!(refusal.hint));
                assert!(office.calls.is_empty());
            }
        }
    }
}

#[test]
fn a_refusal_carries_the_offices_retry_delay_field_and_rule() {
    let table = sample_table();
    let refusal = Refusal {
        details: Some(Box::new(RefusalDetails {
            retry_after_ms: Some(1500),
            field: Some("title".to_owned()),
            rule: Some("too_long".to_owned()),
            conflict: None,
        })),
        ..Refusal::new(
            ErrorCode::RateLimited,
            "Too many attempts.",
            "Wait, then retry.",
        )
    };
    let output = answered(&table, &["sample", "inspect", "--id", "S-1"], Err(refusal));
    assert_eq!(output.exit_code, 1);
    assert_eq!(
        output.stdout,
        "{\"ok\":false,\"data\":null,\"meta\":{},\"error\":{\"code\":\"rate_limited\",\"message\":\"Too many attempts.\",\"hint\":\"Wait, then retry.\",\"field\":\"title\",\"rule\":\"too_long\",\"retry_after_ms\":1500}}\n"
    );
    let envelope: Value = serde_json::from_str(&output.stdout).expect("json");
    assert_eq!(validate_schema(&envelope_schema(), &envelope), Ok(()));
}

#[test]
fn a_list_reports_its_total_and_compact_shows_it_when_no_line_printed() {
    let table = sample_table();
    let item = json!({"id": "S-1", "title": "T", "state": "open", "labels": [], "revision": 1});
    let mut page = Answer::completed(json!([item]));
    page.total = Some(7);
    page.next_cursor = Some("1".to_owned());
    let output = answered(&table, &["sample", "list"], Ok(page));
    let envelope = json_envelope(&output, "a page");
    assert_eq!(
        envelope["meta"],
        json!({"status": "completed", "next_cursor": "1", "total": 7})
    );
    assert_eq!(validate_schema(&envelope_schema(), &envelope), Ok(()));
    // Only a count: no line of data, so compact prints `meta` with the total.
    let mut counted = Answer::completed(json!([]));
    counted.total = Some(7);
    let output = answered(
        &table,
        &["sample", "list", "--format", "compact"],
        Ok(counted),
    );
    assert_eq!(
        output.stdout,
        "meta: {\"status\":\"completed\",\"total\":7}\n"
    );
    // A list that reports no total and printed nothing stays empty, as before.
    let output = answered(
        &table,
        &["sample", "list", "--format", "compact"],
        Ok(Answer::completed(json!([]))),
    );
    assert_eq!(output.stdout, "");
}

#[test]
fn a_refused_line_names_the_fields_given_only_in_json() {
    let table = sample_table();
    let hint = refused(&table, &["sample", "create", "--owner", "x"]);
    assert!(
        hint.contains("Fields given in --input-json: owner, extra."),
        "{hint}"
    );
    let hint = refused(&table, &["sample", "inspect", "--nope", "x"]);
    assert!(!hint.contains("Fields given in --input-json"), "{hint}");
}

static SAMPLE_ALIASES: &[Alias] = &[
    Alias {
        words: &["ls"],
        command: &["sample", "list"],
    },
    Alias {
        words: &["sample", "all"],
        command: &["sample", "list"],
    },
];

#[test]
fn a_shorthand_is_its_command_for_a_call_its_help_its_schema_and_a_check() {
    let table = sample_table().with_aliases(SAMPLE_ALIASES);
    check_rows(&table);
    assert!(
        table
            .catalogue()
            .ends_with("\nShorthands: ls = sample list; sample all = sample list.\n"),
        "{}",
        table.catalogue()
    );
    for (short, long) in [
        (
            &["ls", "--limit", "2"][..],
            &["sample", "list", "--limit", "2"][..],
        ),
        (
            &["sample", "all", "--schema"],
            &["sample", "list", "--schema"],
        ),
        (&["ls", "--help"], &["sample", "list", "--help"]),
        (&["ls", "--bogus"], &["sample", "list", "--bogus"]),
    ] {
        let (a, a_office) = run_stub(&table, &args(short), &mut StubSources::default());
        let (b, b_office) = run_stub(&table, &args(long), &mut StubSources::default());
        assert_eq!(a, b, "{short:?}");
        assert_eq!(a_office.calls, b_office.calls, "{short:?}");
    }
    let mut sources = StubSources::default();
    assert!(org_command::check(&table, "agent-os", &args(&["ls"]), &mut sources).is_ok());
    assert!(
        org_command::check(&table, "agent-os", &args(&["ls", "--nope"]), &mut sources).is_err()
    );
    // Without the shorthands the same words are not a command.
    assert!(org_command::check(&sample_table(), "agent-os", &args(&["ls"]), &mut sources).is_err());
}

#[test]
fn fields_option_propagates_deduplicated_top_level_roots_to_request() {
    let table = sample_table();
    let (out_with, office_with) = run_stub(
        &table,
        &args(&[
            "sample",
            "inspect",
            "--id",
            "S-1",
            "--fields",
            "id,title,labels,id",
        ]),
        &mut StubSources::default(),
    );
    assert_eq!(out_with.exit_code, 0, "{}", out_with.stdout);
    assert_eq!(
        office_with.calls[0].fields,
        Some(vec!["id".into(), "title".into(), "labels".into()])
    );

    let (out_without, office_without) = run_stub(
        &table,
        &args(&["sample", "inspect", "--id", "S-1"]),
        &mut StubSources::default(),
    );
    assert_eq!(out_without.exit_code, 0, "{}", out_without.stdout);
    assert_eq!(office_without.calls[0].fields, None);
}

#[test]
fn conflict_refusal_serializes_structured_conflict_and_validates_against_envelope_schema() {
    use org_proto::{ConflictDetail, ConflictEvent, ConflictSummary};

    let table = sample_table();
    let detail = ConflictDetail {
        expected_revision: 3,
        current_revision: 5,
        changed_sections: vec!["title".into(), "checklist".into()],
        events_since: vec![ConflictEvent {
            revision: 4,
            occurred_at: Some("2026-11-27T10:00:00.000Z".into()),
            actor_kind: "employee".into(),
            actor_id: Some("0193a5c0-0000-7000-8000-000000000001".into()),
            change: json!({"type": "task_updated", "fields": ["title"]}),
        }],
        events_truncated: false,
        omitted_events: 0,
        reread_recommended: false,
        current_summary: Some(ConflictSummary {
            key: Some("T-A1".into()),
            title: "Updated".into(),
            status: "in_progress".into(),
            priority: "high".into(),
            assignee_id: None,
            assigned_team_id: None,
            checklist_total: 2,
            checklist_done: 1,
            ac_total: 1,
            ac_done: 1,
            dod_total: 1,
            dod_done: 0,
            work_steps_total: 0,
            work_steps_done: 0,
            comment_count: 0,
            watcher_count: 1,
        }),
    };
    let mut retry_with = serde_json::Map::new();
    retry_with.insert("if-match".into(), json!(5));
    let mut refusal = Refusal::new(
        ErrorCode::Conflict,
        "revision conflict: expected 3, current is 5",
        "Retry with --if-match 5.",
    )
    .with_conflict(detail);
    refusal.retry_with = Some(retry_with);

    let output = answered(&table, &["sample", "inspect", "--id", "S-1"], Err(refusal));
    assert_eq!(output.exit_code, 1);
    let envelope: Value = serde_json::from_str(&output.stdout).expect("json");
    assert_eq!(validate_schema(&envelope_schema(), &envelope), Ok(()));
    assert_eq!(envelope["error"]["code"], "conflict");
    assert_eq!(envelope["error"]["retry_with"]["if-match"], 5);
    assert_eq!(envelope["error"]["conflict"]["expected_revision"], 3);
    assert_eq!(envelope["error"]["conflict"]["current_revision"], 5);
    assert_eq!(
        envelope["error"]["conflict"]["events_since"][0]["revision"],
        4
    );
    assert_eq!(
        envelope["error"]["conflict"]["current_summary"]["title"],
        "Updated"
    );
}

#[test]
fn json_output_switch_works_before_and_after_subcommand_and_honors_env_format_precedence() {
    let table = sample_table().with_aliases(SAMPLE_ALIASES);
    let run_env = |line: &[&str], env: Option<&str>| {
        let mut sources = StubSources::default();
        let mut keys = StubKeys::default();
        let mut office = StubOffice::default();
        org_command::run_with_env_format(
            &table,
            "agent-os",
            &args(line),
            &mut sources,
            &mut keys,
            &mut office,
            env,
        )
    };

    // 1. AGENT_OS_FORMAT=yaml and compact (case-insensitive, trimmed) apply when no CLI flag is set.
    let yaml = run_env(&["sample", "list"], Some("  YAML "));
    assert_eq!(yaml.exit_code, 0, "{}", yaml.stdout);
    assert!(yaml.stdout.starts_with("ok: true\n"), "{}", yaml.stdout);

    let compact = run_env(&["sample", "list"], Some("Compact"));
    assert_eq!(compact.exit_code, 0, "{}", compact.stdout);
    assert!(!compact.stdout.starts_with('{'), "{}", compact.stdout);

    // 2. Bare `--json` before OR after subcommand (and before shorthands, or idempotently with
    //    `--format json`) beats AGENT_OS_FORMAT.
    for line in [
        &["--json", "sample", "list"][..],
        &["sample", "list", "--json"],
        &["--json", "sample", "list", "--format", "json"],
        &["sample", "list", "--json", "--format", "json"],
        &["--json", "ls"],
        &["ls", "--json"],
        &["--json", "sample", "ping"],
        &["sample", "ping", "--json"],
    ] {
        let out = run_env(line, Some("yaml"));
        assert_eq!(out.exit_code, 0, "{line:?}: {}", out.stdout);
        let env = json_envelope(&out, &format!("{line:?}"));
        assert_eq!(env["ok"], json!(true), "{line:?}");
    }

    // 3. Explicit --format or --json beats even an invalid AGENT_OS_FORMAT.
    let out = run_env(&["sample", "list", "--format", "compact"], Some("bogus"));
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    let out = run_env(&["--json", "sample", "list"], Some("bogus"));
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    assert_eq!(json_envelope(&out, "leading --json")["ok"], json!(true));

    // 4. Invalid AGENT_OS_FORMAT without explicit CLI flag exits 2 cleanly with JSON error envelope.
    for bad in ["xml", "table", "tsv"] {
        let out = run_env(&["sample", "list"], Some(bad));
        assert_eq!(out.exit_code, 2, "bad={bad}: {}", out.stdout);
        let env = json_envelope(&out, bad);
        assert_eq!(env["error"]["code"], json!("invalid_request"));
        assert!(
            env["error"]["message"]
                .as_str()
                .is_some_and(|m| m.contains("AGENT_OS_FORMAT")),
            "{env}"
        );
    }

    // 5. Empty / whitespace AGENT_OS_FORMAT defaults to JSON.
    for empty in ["", "   "] {
        let out = run_env(&["sample", "list"], Some(empty));
        assert_eq!(out.exit_code, 0, "{}", out.stdout);
        assert_eq!(json_envelope(&out, "empty env")["ok"], json!(true));
    }

    // 6. Conflicting --json + --format yaml, duplicate --json, and --json=<value> are refused.
    for (line, expected) in [
        (
            &["--json", "sample", "list", "--format", "yaml"][..],
            "--json cannot be combined with --format",
        ),
        (
            &["sample", "list", "--json", "--format", "compact"],
            "--json cannot be combined with --format",
        ),
        (
            &["--json", "sample", "list", "--json"],
            "--json is given twice",
        ),
        (
            &["--json", "--json", "sample", "list"],
            "--json is given twice",
        ),
        (
            &["sample", "create", "--json={\"title\":\"A\"}"],
            "--input-json",
        ),
        (
            &["--json={\"title\":\"A\"}", "sample", "create"],
            "--input-json",
        ),
    ] {
        let out = run_env(line, None);
        assert_eq!(out.exit_code, 2, "{line:?}: {}", out.stdout);
        let env = json_envelope(&out, &format!("{line:?}"));
        let msg = env["error"]["message"].as_str().unwrap_or("");
        assert!(msg.contains(expected), "{line:?}: {msg}");
    }

    // 7. --json before --help still renders help (exit 0).
    for help_line in [
        &["--json", "--help"][..],
        &["--json", "sample", "--help"],
        &["--json", "sample", "list", "--help"],
    ] {
        let out = run_env(help_line, Some("bogus"));
        assert_eq!(out.exit_code, 0, "{help_line:?}: {}", out.stdout);
        assert!(
            out.stdout.starts_with("Usage: "),
            "{help_line:?}: {}",
            out.stdout
        );
    }
}
