//! The command contract, run against the test office: CLI-1, 2, 3, 7, 8 and 11 of
//! `docs/design/agent-os-command-line.md` for every row of the sample table, a row added here
//! without touching the parser, and the parser's and the formats' edge cases.

use std::collections::BTreeMap;
use std::path::Path;

use org_command::testkit::{
    answer_with, check_contract, check_rows, json_envelope, run_stub, sample_table, StubOffice,
    StubSources, HARD_TEXT, SAMPLE_ROWS,
};
use org_command::{
    run, yaml_object, Ack, Answer, Command, CommandTable, Example, Field, IfMatch, Kind, Office,
    Refusal, Request, Status, Type,
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
    assert!(table
        .catalogue()
        .contains("widget rename: Rename a widget\n"));
    let (help, _) = run_stub(&table, &args(&["--help"]), &mut StubSources::default());
    assert!(help.stdout.contains("widget"), "{}", help.stdout);
}

#[test]
fn flags_and_json_give_the_same_input() {
    let table = sample_table();
    let row = table.find("sample", "create").expect("the row");
    let example: Map<String, Value> = serde_json::from_str(row.example.input).expect("example");
    let mut flat = example.clone();
    // A field with structure has no flag: it is given in --json beside the flags.
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
            "--json",
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
            "--json",
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
            "--json",
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
            &["sample", "ping", "--json", "{}"],
            "--json is for a command with input",
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
            &["sample", "create", "--title", "A", "--json", "[1]"],
            "--json must be one JSON object",
        ),
        (
            &["sample", "create", "--title", "A", "--json", "{"],
            "--json is not JSON",
        ),
        (
            &["sample", "create", "--json", r#"{"title":1}"#],
            "`title` must be text",
        ),
        (
            &[
                "sample",
                "create",
                "--json",
                r#"{"title":"A","owner":{"name":"x"}}"#,
            ],
            "`owner.id` is required",
        ),
        (
            &["sample", "create", "--json", r#"{"title":"A","nope":1}"#],
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
        json!({"status": "completed", "operation_id": "op-stub", "ack": "durable", "dry_run": true})
    );
    let line = args(&["sample", "update", "--id", "S-1", "--if-match", "3"]);
    let (output, _) = run_stub(&table, &line, &mut StubSources::default());
    assert_eq!(
        json_envelope(&output, "a write")["meta"],
        json!({"status": "completed", "operation_id": "op-stub", "ack": "durable", "revision": 4})
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
        &mut Fixed(Err(refusal)),
    );
    assert_eq!(output.exit_code, 1);
    assert_eq!(
        output.stdout,
        "{\"ok\":false,\"data\":null,\"meta\":{},\"error\":{\"code\":\"not_authorized\",\"message\":\"Only the owner lists samples.\",\"hint\":\"Ask the owner.\"}}\n"
    );
    // An unlisted field of the office's answer is printed after the declared ones.
    let answer = Answer::completed(json!({"zeta": 1, "office": "O-1", "alpha": 2}));
    let output = run(
        &table,
        "agent-os",
        &args(&["sample", "ping"]),
        &mut StubSources::default(),
        &mut Fixed(Ok(answer)),
    );
    assert!(
        output
            .stdout
            .contains("\"data\":{\"office\":\"O-1\",\"alpha\":2,\"zeta\":1}"),
        "{}",
        output.stdout
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
            &mut Fixed(Ok(answer)),
        );
        let read = yaml_object(&output.stdout)
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
    assert!(org_command::testkit::validate_schema(&schema, &json!({"office": "O-1"})).is_ok());
    for bad in [
        json!({"office": 1}),
        json!({}),
        json!({"office": "O-1", "more": true}),
        json!([]),
    ] {
        assert!(
            org_command::testkit::validate_schema(&schema, &bad).is_err(),
            "{bad}"
        );
    }
    assert!(org_command::testkit::check_schema(&json!({"type": "strng"})).is_err());
    assert!(org_command::testkit::check_schema(&json!({"typo": "string"})).is_err());
    // The unchecked table of an executable's start holds the same rows in the same order.
    let unchecked = CommandTable::unchecked(&[&EXTRA_ROWS, &SAMPLE_ROWS]);
    let checked = CommandTable::new(&[&SAMPLE_ROWS, &EXTRA_ROWS]).expect("the table");
    assert_eq!(unchecked.catalogue(), checked.catalogue());
    let _ = StubOffice::default();
}
