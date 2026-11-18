//! The contract every row is held to, and the test office it is run against: a stub inside the
//! test process. An increment that adds rows calls [`check_contract`] on its table.
//!
//! The checks are CLI-1, 2, 3, 7, 8 and 11 of `docs/design/agent-os-command-line.md`, section 8.
//! Each panics with the row's name and what failed.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use org_proto::ErrorCode;
use serde_json::{json, Map, Value};

use crate::{
    envelope_schema, run, yaml_object, Ack, Answer, Command, CommandTable, Example, Field, IfMatch,
    Kind, Office, Output, Refusal, Request, Sources, Status, Type, CATALOGUE_LIMIT,
};

/// The test office. It answers every command with the row's example output and records what
/// it was sent; with `refuse` set it refuses every command with that.
#[derive(Debug, Default)]
pub struct StubOffice {
    pub calls: Vec<Request>,
    pub refuse: Option<Refusal>,
}

impl Office for StubOffice {
    fn call(&mut self, request: &Request) -> Result<Answer, Refusal> {
        self.calls.push(request.clone());
        if let Some(refusal) = &self.refuse {
            return Err(refusal.clone());
        }
        let command = request.command;
        let data = serde_json::from_str(command.example.output)
            .expect("the table checked the example output");
        let mut answer = Answer::completed(data);
        if let Kind::Write { ack, if_match } = command.kind {
            answer.operation_id = Some(
                request
                    .idempotency_key
                    .clone()
                    .unwrap_or_else(|| "op-stub".to_owned()),
            );
            answer.ack = Some(if request.durable { Ack::Durable } else { ack });
            if if_match != IfMatch::No && !request.dry_run {
                answer.revision = Some(request.if_match.unwrap_or(0) + 1);
            }
        }
        Ok(answer)
    }
}

/// Files and standard input held in memory. `stdin: None` is standard input closed. Every read
/// is counted.
#[derive(Debug, Default)]
pub struct StubSources {
    pub files: BTreeMap<String, Vec<u8>>,
    pub stdin: Option<Vec<u8>>,
    pub file_reads: usize,
    pub stdin_reads: usize,
}

impl Sources for StubSources {
    fn file(&mut self, path: &str) -> io::Result<Vec<u8>> {
        self.file_reads += 1;
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| io::ErrorKind::NotFound.into())
    }

    fn stdin(&mut self) -> io::Result<Vec<u8>> {
        self.stdin_reads += 1;
        self.stdin
            .clone()
            .ok_or_else(|| io::ErrorKind::BrokenPipe.into())
    }
}

const PROGRAM: &str = "agent-os";

/// Runs one command line against a fresh stub office.
pub fn run_stub(
    table: &CommandTable,
    args: &[String],
    sources: &mut StubSources,
) -> (Output, StubOffice) {
    let mut office = StubOffice::default();
    let output = run(table, PROGRAM, args, sources, &mut office);
    (output, office)
}

fn example_input(row: &Command) -> Map<String, Value> {
    serde_json::from_str(row.example.input).expect("the table checked the example input")
}

/// The command line that gives `input` to `row`, with every option the row requires.
pub fn args_for(row: &Command, input: &Map<String, Value>) -> Vec<String> {
    let mut args = vec![row.resource.to_owned(), row.verb.to_owned()];
    if !row.input.is_empty() {
        args.push("--json".to_owned());
        args.push(Value::Object(input.clone()).to_string());
    }
    if row.if_match() == IfMatch::Required {
        args.push("--if-match".to_owned());
        args.push("1".to_owned());
    }
    args
}

fn with(mut args: Vec<String>, more: &[&str]) -> Vec<String> {
    args.extend(more.iter().map(|arg| (*arg).to_owned()));
    args
}

/// The one JSON envelope of `output`, checked against the envelope's schema.
pub fn json_envelope(output: &Output, context: &str) -> Value {
    let line = output
        .stdout
        .strip_suffix('\n')
        .unwrap_or_else(|| panic!("{context}: the output does not end with a new line"));
    assert!(
        !line.contains('\n'),
        "{context}: the JSON envelope is not one line:\n{line}"
    );
    let envelope: Value = serde_json::from_str(line)
        .unwrap_or_else(|e| panic!("{context}: the output is not JSON ({e}):\n{line}"));
    if let Err(problem) = validate_schema(&envelope_schema(), &envelope) {
        panic!("{context}: the envelope does not validate: {problem}\n{line}");
    }
    envelope
}

fn assert_refused_unread(output: &Output, office: &StubOffice, context: &str) {
    let envelope = json_envelope(output, context);
    assert_eq!(output.exit_code, 2, "{context}: exit code");
    assert_eq!(envelope["ok"], json!(false), "{context}");
    assert_eq!(
        envelope["error"]["code"],
        json!("invalid_request"),
        "{context}"
    );
    assert!(
        envelope["error"]["hint"]
            .as_str()
            .is_some_and(|hint| !hint.trim().is_empty()),
        "{context}: no hint"
    );
    assert!(office.calls.is_empty(), "{context}: the office was called");
}

/// CLI-1: a positional argument, an unknown flag and a missing required flag are each refused
/// with `invalid_request`, a hint and exit code 2, and nothing reaches the office.
pub fn check_grammar(table: &CommandTable) {
    for row in table.rows() {
        let name = row.name();
        let input = example_input(row);
        let base = args_for(row, &input);
        let (output, office) = run_stub(table, &base, &mut StubSources::default());
        assert_eq!(
            output.exit_code, 0,
            "{name}: the example is refused: {}",
            output.stdout
        );
        assert_eq!(office.calls.len(), 1, "{name}: the example makes one call");
        assert_eq!(
            office.calls[0].input, input,
            "{name}: the office receives the example input"
        );

        for args in [
            with(base.clone(), &["stray"]),
            with(
                vec![row.resource.to_owned(), row.verb.to_owned()],
                &["stray"],
            ),
        ] {
            let (output, office) = run_stub(table, &args, &mut StubSources::default());
            assert_refused_unread(&output, &office, &format!("{name}: a positional argument"));
        }
        for extra in [
            &["--no-such-flag", "x"][..],
            &["--no-such-flag=x"],
            &["--no-such-flag"],
        ] {
            let (output, office) = run_stub(
                table,
                &with(base.clone(), extra),
                &mut StubSources::default(),
            );
            assert_refused_unread(&output, &office, &format!("{name}: an unknown flag"));
        }
        for field in row.input.iter().filter(|field| field.required) {
            let mut partial = input.clone();
            partial.remove(field.name);
            let (output, office) =
                run_stub(table, &args_for(row, &partial), &mut StubSources::default());
            assert_refused_unread(
                &output,
                &office,
                &format!("{name}: without the required --{}", field.flag()),
            );
        }
        if row.if_match() == IfMatch::Required {
            let mut args = vec![
                row.resource.to_owned(),
                row.verb.to_owned(),
                "--json".to_owned(),
            ];
            args.push(Value::Object(input.clone()).to_string());
            let (output, office) = run_stub(table, &args, &mut StubSources::default());
            assert_refused_unread(
                &output,
                &office,
                &format!("{name}: without the required --if-match"),
            );
        }
    }
}

/// CLI-2: `json` and `yaml` print exactly one envelope that validates against the envelope's
/// schema, and `json` is one line; `compact` prints `data` alone on success and the JSON
/// envelope on failure; `--help` prints text.
pub fn check_answers(table: &CommandTable) {
    for row in table.rows() {
        let name = row.name();
        let base = args_for(row, &example_input(row));

        let (output, _) = run_stub(
            table,
            &with(base.clone(), &["--format", "json"]),
            &mut StubSources::default(),
        );
        let envelope = json_envelope(&output, &format!("{name}: json"));
        assert_eq!(
            (output.exit_code, &envelope["ok"]),
            (0, &json!(true)),
            "{name}: json"
        );
        assert_eq!(envelope["error"], Value::Null, "{name}: json");

        let (output, _) = run_stub(
            table,
            &with(base.clone(), &["--format=yaml"]),
            &mut StubSources::default(),
        );
        let from_yaml = yaml_object(&output.stdout).unwrap_or_else(|e| {
            panic!(
                "{name}: the yaml answer is not one YAML mapping ({e}):\n{}",
                output.stdout
            )
        });
        assert_eq!(
            Value::Object(from_yaml),
            envelope,
            "{name}: the yaml answer is the json envelope"
        );
        assert_eq!(output.exit_code, 0, "{name}: yaml");

        let (output, _) = run_stub(
            table,
            &with(base.clone(), &["--format", "compact"]),
            &mut StubSources::default(),
        );
        assert_eq!(output.exit_code, 0, "{name}: compact");
        let lines: Vec<&str> = output.stdout.lines().collect();
        match &envelope["data"] {
            Value::Array(items) => {
                assert_eq!(
                    lines.len(),
                    items.len(),
                    "{name}: compact prints one line an item"
                );
                for line in &lines {
                    assert_eq!(
                        line.split('\t').count(),
                        row.output.len(),
                        "{name}: compact line `{line}` does not have one cell a field"
                    );
                }
            }
            Value::Object(record) => {
                assert_eq!(
                    lines.len(),
                    record.len(),
                    "{name}: compact prints one line a field"
                );
                for line in &lines {
                    let field = line.split(": ").next().unwrap_or_default();
                    assert!(
                        record.contains_key(field),
                        "{name}: compact line `{line}` names no field"
                    );
                }
            }
            other => panic!("{name}: data is neither a record nor a list: {other}"),
        }

        let refusal = Refusal {
            retry_with: Some(Map::from_iter([("if-match".to_owned(), json!(4))])),
            operation_id: Some("op-refused".to_owned()),
            ..Refusal::new(
                ErrorCode::Conflict,
                "It changed since revision 3.",
                "Read it again and repeat the change.",
            )
        };
        for format in ["json", "yaml", "compact"] {
            let mut office = StubOffice {
                refuse: Some(refusal.clone()),
                ..StubOffice::default()
            };
            let args = with(base.clone(), &["--format", format]);
            let output = run(
                table,
                PROGRAM,
                &args,
                &mut StubSources::default(),
                &mut office,
            );
            let failed = json_envelope(&output, &format!("{name}: a failure in {format}"));
            assert_eq!(output.exit_code, 1, "{name}: a failure in {format}");
            assert_eq!(
                failed,
                json!({"ok": false, "data": null, "meta": {"operation_id": "op-refused"}, "error": {
                    "code": "conflict", "message": refusal.message, "hint": refusal.hint, "retry_with": {"if-match": 4}
                }}),
                "{name}: a failure in {format}"
            );
        }

        let (output, office) = run_stub(
            table,
            &with(base.clone(), &["--help"]),
            &mut StubSources::default(),
        );
        assert_eq!(output.exit_code, 0, "{name}: --help");
        assert!(
            serde_json::from_str::<Value>(&output.stdout).is_err(),
            "{name}: --help prints text"
        );
        assert!(
            output.stdout.contains(&name),
            "{name}: --help names the command"
        );
        for field in row.input.iter().filter(|field| field.is_flag()) {
            assert!(
                output.stdout.contains(&format!("--{}", field.flag())),
                "{name}: --help lists --{}",
                field.flag()
            );
        }
        assert!(office.calls.is_empty(), "{name}: --help called the office");
    }
}

/// CLI-3: every `--schema` output is valid JSON Schema, and every row's example input and
/// recorded output validate against it. The example input also goes through the parser.
pub fn check_schemas(table: &CommandTable) {
    for row in table.rows() {
        let name = row.name();
        let args = with(
            vec![row.resource.to_owned(), row.verb.to_owned()],
            &["--schema"],
        );
        let (output, office) = run_stub(table, &args, &mut StubSources::default());
        let envelope = json_envelope(&output, &format!("{name}: --schema"));
        assert_eq!(output.exit_code, 0, "{name}: --schema");
        assert!(
            office.calls.is_empty(),
            "{name}: --schema called the office"
        );
        let data = &envelope["data"];
        assert_eq!(data["command"], json!(name));
        assert_eq!(
            data["content_field"],
            json!(row.content_field),
            "{name}: content_field"
        );
        assert_eq!(
            data["ack"],
            json!(row.ack().map(Ack::as_str)),
            "{name}: ack"
        );
        assert_eq!(
            data["errors"].as_array().map(Vec::len),
            Some(row.errors.len()),
            "{name}: errors"
        );
        for part in ["input", "output"] {
            if let Err(problem) = check_schema(&data[part]) {
                panic!("{name}: the {part} schema is not valid JSON Schema: {problem}");
            }
            assert_eq!(
                data[part]["$schema"],
                json!(crate::shape::SCHEMA_DIALECT),
                "{name}: {part} dialect"
            );
        }

        let input = example_input(row);
        if let Err(problem) = validate_schema(&data["input"], &Value::Object(input.clone())) {
            panic!(
                "{name}: the example input does not validate against the input schema: {problem}"
            );
        }
        let (output, office) = run_stub(table, &args_for(row, &input), &mut StubSources::default());
        assert_eq!(
            office.calls.first().map(|call| &call.input),
            Some(&input),
            "{name}: the parser fills the example"
        );
        let recorded = json_envelope(&output, &format!("{name}: the recorded output"));
        if let Err(problem) = validate_schema(&data["output"], &recorded["data"]) {
            panic!("{name}: the recorded output does not validate against the output schema: {problem}");
        }
    }
}

/// Text a shell would mangle: quotes, backticks, dollar signs, new lines and non-ASCII text.
pub const HARD_TEXT: &str = "# Title with \"double\" and 'single' quotes\n\n`code` and ```fenced```, $HOME, $(rm -rf /), ${x}\n\\ back\\slash \\n not a new line\n\ttab, trailing spaces  \n- ünïcödé — 日本語 — 🚀\nkey: \"value\" # not a comment\n";

/// CLI-7: Markdown with quotes, backticks, dollar signs, new lines and non-ASCII text arrives
/// byte-identical through `--input-file`, `--stdin` and `--json` (and the field's own flag). A
/// row with no content field refuses `--input-file` and `--stdin` and reads neither.
pub fn check_content(table: &CommandTable) {
    for row in table.rows() {
        let name = row.name();
        let mut input = example_input(row);
        let Some(content) = row.content_field else {
            for extra in [&["--input-file", "content.md"][..], &["--stdin"]] {
                let mut sources = sources_with_content();
                let (output, office) =
                    run_stub(table, &with(args_for(row, &input), extra), &mut sources);
                assert_refused_unread(
                    &output,
                    &office,
                    &format!("{name}: {} with no content field", extra[0]),
                );
                assert_eq!(
                    (sources.file_reads, sources.stdin_reads),
                    (0, 0),
                    "{name}: content was read"
                );
            }
            continue;
        };
        input.remove(content);
        let mut whole = input.clone();
        whole.insert(content.to_owned(), json!(HARD_TEXT));
        let flag = format!("--{}", content.replace('_', "-"));
        let ways: [(&str, Vec<String>); 4] = [
            (
                "--input-file",
                with(args_for(row, &input), &["--input-file", "content.md"]),
            ),
            ("--stdin", with(args_for(row, &input), &["--stdin"])),
            ("--json", args_for(row, &whole)),
            (
                "its own flag",
                with(args_for(row, &input), &[&flag, HARD_TEXT]),
            ),
        ];
        for (way, args) in ways {
            let (output, office) = run_stub(table, &args, &mut sources_with_content());
            assert_eq!(
                output.exit_code, 0,
                "{name}: content through {way}: {}",
                output.stdout
            );
            let arrived = office.calls[0].input.get(content).and_then(Value::as_str);
            assert_eq!(
                arrived.map(str::as_bytes),
                Some(HARD_TEXT.as_bytes()),
                "{name}: content through {way}"
            );
        }
        let (output, office) = run_stub(
            table,
            &with(
                args_for(row, &input),
                &["--input-file", "content.md", "--stdin"],
            ),
            &mut sources_with_content(),
        );
        assert_refused_unread(
            &output,
            &office,
            &format!("{name}: --input-file with --stdin"),
        );
        let mut sources = StubSources {
            stdin: Some(vec![b'a', 0xff, b'b']),
            ..StubSources::default()
        };
        let (output, office) = run_stub(
            table,
            &with(args_for(row, &input), &["--stdin"]),
            &mut sources,
        );
        assert_refused_unread(
            &output,
            &office,
            &format!("{name}: bytes that are not UTF-8"),
        );
    }
}

fn sources_with_content() -> StubSources {
    StubSources {
        files: BTreeMap::from([("content.md".to_owned(), HARD_TEXT.as_bytes().to_vec())]),
        stdin: Some(HARD_TEXT.as_bytes().to_vec()),
        ..StubSources::default()
    }
}

/// CLI-8: with standard input closed, every row completes, and no row reads standard input
/// without `--stdin`.
pub fn check_no_stdin(table: &CommandTable) {
    for row in table.rows() {
        let name = row.name();
        let base = args_for(row, &example_input(row));
        for extra in [
            &[][..],
            &["--format", "yaml"],
            &["--format", "compact"],
            &["--schema"],
            &["--help"],
        ] {
            let mut sources = StubSources::default();
            let (output, _) = run_stub(table, &with(base.clone(), extra), &mut sources);
            assert_eq!(
                output.exit_code, 0,
                "{name} {extra:?}: does not complete with standard input closed"
            );
            assert_eq!(
                sources.stdin_reads, 0,
                "{name} {extra:?}: read standard input without --stdin"
            );
            assert_eq!(
                sources.file_reads, 0,
                "{name} {extra:?}: read a file without --input-file"
            );
        }
    }
}

/// One line of JSON for each row: its `--schema` data.
pub fn schema_snapshot(table: &CommandTable) -> String {
    let mut out = String::new();
    for row in table.rows() {
        let args = with(
            vec![row.resource.to_owned(), row.verb.to_owned()],
            &["--schema"],
        );
        let (output, _) = run_stub(table, &args, &mut StubSources::default());
        let line = output.stdout.trim_end();
        let data = line
            .strip_prefix("{\"ok\":true,\"data\":")
            .and_then(|rest| {
                rest.strip_suffix(",\"meta\":{\"status\":\"completed\"},\"error\":null}")
            })
            .unwrap_or_else(|| {
                panic!(
                    "{}: --schema did not print the expected envelope: {line}",
                    row.name()
                )
            });
        out.push_str(data);
        out.push('\n');
    }
    out
}

/// CLI-11, first half: the launch catalogue is generated from the table, has one line for each
/// command and is at most 4 KiB.
pub fn check_catalogue(table: &CommandTable) {
    let catalogue = table.catalogue();
    let lines: Vec<&str> = catalogue.lines().collect();
    assert_eq!(
        lines.len(),
        table.rows().len(),
        "the catalogue has one line for each command"
    );
    for (line, row) in lines.iter().zip(table.rows()) {
        assert!(
            line.starts_with(&format!("{}: ", row.name())),
            "catalogue line `{line}`"
        );
    }
    assert!(
        catalogue.len() <= CATALOGUE_LIMIT,
        "the launch catalogue is {} bytes; the limit is {CATALOGUE_LIMIT}",
        catalogue.len()
    );
}

/// The variable that rewrites the snapshots instead of comparing them.
pub const UPDATE_SNAPSHOTS: &str = "ORG_COMMAND_UPDATE_SNAPSHOTS";

/// CLI-11, second half: the catalogue and every schema are compared with the snapshot in `dir`
/// (`catalogue.txt`, `schemas.jsonl`). A row that changes without its snapshot fails. The bytes
/// do not depend on the operating system. Set `ORG_COMMAND_UPDATE_SNAPSHOTS=1` to rewrite them.
pub fn check_snapshots(table: &CommandTable, dir: &Path) {
    let update = std::env::var_os(UPDATE_SNAPSHOTS).is_some_and(|value| value == "1");
    for (file, current) in [
        ("catalogue.txt", table.catalogue()),
        ("schemas.jsonl", schema_snapshot(table)),
    ] {
        let path = dir.join(file);
        if update {
            std::fs::create_dir_all(dir).expect("create the snapshot directory");
            std::fs::write(&path, &current).expect("write the snapshot");
            continue;
        }
        let recorded = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            recorded == current,
            "{} does not match the table. If the change is meant, run the test again with {UPDATE_SNAPSHOTS}=1 and commit the file.\n--- recorded\n{recorded}\n--- current\n{current}",
            path.display()
        );
    }
}

/// Everything K1 holds a table to, except the snapshots: CLI-1, 2, 3, 7, 8 and the catalogue.
pub fn check_rows(table: &CommandTable) {
    check_grammar(table);
    check_answers(table);
    check_schemas(table);
    check_content(table);
    check_no_stdin(table);
    check_catalogue(table);
}

/// The whole contract, with the snapshots in `snapshots`.
pub fn check_contract(table: &CommandTable, snapshots: &Path) {
    check_rows(table);
    check_snapshots(table, snapshots);
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Validates `instance` against `schema`, for the JSON Schema keywords this library writes:
/// `type`, `properties`, `required`, `additionalProperties` (a boolean), `items`, `enum`,
/// `const`, `minimum` and `oneOf`. It is a test tool, not a general validator.
pub fn validate_schema(schema: &Value, instance: &Value) -> Result<(), String> {
    validate_at(schema, instance, "$")
}

fn validate_at(schema: &Value, instance: &Value, at: &str) -> Result<(), String> {
    let Value::Object(schema) = schema else {
        return Err(format!("{at}: the schema is not an object"));
    };
    if let Some(ty) = schema.get("type") {
        let actual = type_name(instance);
        let fits = |ty: &Value| ty == actual || (ty == "number" && actual == "integer");
        let ok = match ty {
            Value::Array(types) => types.iter().any(fits),
            ty => fits(ty),
        };
        if !ok {
            return Err(format!("{at}: is {actual}, not {ty}"));
        }
    }
    if let Some(Value::Array(allowed)) = schema.get("enum") {
        if !allowed.contains(instance) {
            return Err(format!(
                "{at}: {instance} is not one of {}",
                Value::Array(allowed.clone())
            ));
        }
    }
    if let Some(expected) = schema.get("const") {
        if expected != instance {
            return Err(format!("{at}: is not {expected}"));
        }
    }
    if let (Some(minimum), Some(number)) = (
        schema.get("minimum").and_then(Value::as_f64),
        instance.as_f64(),
    ) {
        if number < minimum {
            return Err(format!("{at}: {number} is below {minimum}"));
        }
    }
    if let Value::Object(object) = instance {
        let properties = schema.get("properties").and_then(Value::as_object);
        for (key, value) in object {
            match properties.and_then(|properties| properties.get(key)) {
                Some(property) => validate_at(property, value, &format!("{at}.{key}"))?,
                None if schema.get("additionalProperties") == Some(&Value::Bool(false)) => {
                    return Err(format!("{at}.{key}: is not a declared property"))
                }
                None => {}
            }
        }
        for name in schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if !name.as_str().is_some_and(|name| object.contains_key(name)) {
                return Err(format!("{at}: {name} is required"));
            }
        }
    }
    if let (Some(items), Value::Array(array)) = (schema.get("items"), instance) {
        for (i, item) in array.iter().enumerate() {
            validate_at(items, item, &format!("{at}[{i}]"))?;
        }
    }
    if let Some(Value::Array(branches)) = schema.get("oneOf") {
        let passing = branches
            .iter()
            .filter(|branch| validate_at(branch, instance, at).is_ok())
            .count();
        if passing != 1 {
            return Err(format!(
                "{at}: matches {passing} of the oneOf branches, not one"
            ));
        }
    }
    Ok(())
}

/// Checks that `schema` is a JSON Schema this library's validator understands in full: only
/// the keywords above (and `$schema`, `description`), each with a value of the right kind.
pub fn check_schema(schema: &Value) -> Result<(), String> {
    const TYPES: [&str; 7] = [
        "null", "boolean", "integer", "number", "string", "array", "object",
    ];
    let Value::Object(object) = schema else {
        return Err(format!("{schema} is not a schema object"));
    };
    for (keyword, value) in object {
        let ok = match keyword.as_str() {
            "$schema" | "description" => value.is_string(),
            "type" => value.as_str().is_some_and(|ty| TYPES.contains(&ty)),
            "properties" => match value {
                Value::Object(properties) => {
                    properties.values().try_for_each(check_schema)?;
                    true
                }
                _ => false,
            },
            "required" => value.as_array().is_some_and(|names| {
                names.iter().all(|name| {
                    name.as_str().is_some_and(|name| {
                        object
                            .get("properties")
                            .and_then(Value::as_object)
                            .is_some_and(|p| p.contains_key(name))
                    })
                })
            }),
            "additionalProperties" => value.is_boolean(),
            "items" => {
                check_schema(value)?;
                true
            }
            "enum" => value.as_array().is_some_and(|values| !values.is_empty()),
            "const" => true,
            "minimum" => value.is_number(),
            "oneOf" => match value {
                Value::Array(branches) if !branches.is_empty() => {
                    branches.iter().try_for_each(check_schema)?;
                    true
                }
                _ => false,
            },
            _ => return Err(format!("`{keyword}` is not a keyword the contract uses")),
        };
        if !ok {
            return Err(format!("`{keyword}` has the wrong kind of value: {value}"));
        }
    }
    Ok(())
}

const OWNER: &[Field] = &[
    Field::required("id", Type::String, "The owner's ID"),
    Field::optional("name", Type::String, "The owner's name"),
];

const SAMPLE: &[Field] = &[
    Field::required("id", Type::String, "The sample's ID"),
    Field::required("title", Type::String, "The title"),
    Field::optional("body", Type::String, "The body, as Markdown"),
    Field::required(
        "state",
        Type::Enum(&["open", "closed"]),
        "Whether it is open",
    ),
    Field::optional("priority", Type::Integer, "The priority, 1 first"),
    Field::required("labels", Type::List(&Type::String), "Its labels"),
    Field::optional("owner", Type::Object(OWNER), "Who owns it"),
    Field::required("revision", Type::Integer, "The revision"),
];

const SAMPLE_RECORD: &str = r##"{"id":"S-1","title":"1.5","body":"# Heading\n\nA `body` with \"quotes\", $x and — ünïcödé.\n","state":"open","priority":2,"labels":["a b","yes"],"owner":{"id":"E-1","name":"Ada"},"revision":3}"##;

const SAMPLE_ERRORS: &[ErrorCode] = &[ErrorCode::NotFound, ErrorCode::NotAuthorized];

/// Rows of no domain, for the contract's own tests: a read, a read with no input, a list, a
/// write with a content field and every type of flag, and a write that requires `--if-match`.
pub static SAMPLE_ROWS: [Command; 5] = [
    Command {
        resource: "sample",
        verb: "inspect",
        summary: "One sample",
        call: "sample.inspect",
        kind: Kind::Read,
        input: &[Field::required("id", Type::String, "The sample's ID")],
        content_field: None,
        output: SAMPLE,
        errors: SAMPLE_ERRORS,
        example: Example {
            input: r#"{"id":"S-1"}"#,
            output: SAMPLE_RECORD,
        },
    },
    Command {
        resource: "sample",
        verb: "ping",
        summary: "Whether the office answers",
        call: "sample.ping",
        kind: Kind::Read,
        input: &[],
        content_field: None,
        output: &[Field::required("office", Type::String, "The office's ID")],
        errors: &[],
        example: Example {
            input: "{}",
            output: r#"{"office":"O-1"}"#,
        },
    },
    Command {
        resource: "sample",
        verb: "list",
        summary: "The samples, newest first",
        call: "sample.list",
        kind: Kind::List,
        input: &[
            Field::optional(
                "state",
                Type::Enum(&["open", "closed"]),
                "Only samples in this state",
            ),
            Field::optional(
                "label",
                Type::List(&Type::String),
                "Only samples with this label",
            ),
        ],
        content_field: None,
        output: SAMPLE,
        errors: &[ErrorCode::NotAuthorized],
        example: Example {
            input: r#"{"state":"open","label":["a b"]}"#,
            output: r#"[{"id":"S-2","title":"Second","state":"open","labels":[],"revision":1},{"id":"S-1","title":"First","state":"open","priority":1,"labels":["a b"],"owner":{"id":"E-1"},"revision":3}]"#,
        },
    },
    Command {
        resource: "sample",
        verb: "create",
        summary: "Create a sample",
        call: "sample.create",
        kind: Kind::Write {
            ack: Ack::Committed,
            if_match: IfMatch::No,
        },
        input: &[
            Field::required("title", Type::String, "The title"),
            Field::optional("body", Type::String, "The body, as Markdown"),
            Field::optional(
                "state",
                Type::Enum(&["open", "closed"]),
                "The state to create it in",
            ),
            Field::optional("priority", Type::Integer, "The priority, 1 first"),
            Field::optional("labels", Type::List(&Type::String), "A label"),
            Field::optional("urgent", Type::Boolean, "Mark it urgent"),
            Field::optional("owner", Type::Object(OWNER), "Who owns it"),
            Field::optional("extra", Type::Json, "Anything else to keep with it"),
        ],
        content_field: Some("body"),
        output: SAMPLE,
        errors: &[
            ErrorCode::NotAuthorized,
            ErrorCode::AlreadyExists,
            ErrorCode::OperationReused,
        ],
        example: Example {
            input: r#"{"title":"1.5","body":"Short body","state":"open","priority":2,"labels":["a b","yes"],"urgent":true,"owner":{"id":"E-1","name":"Ada"},"extra":{"z":1,"a":[null]}}"#,
            output: SAMPLE_RECORD,
        },
    },
    Command {
        resource: "sample",
        verb: "update",
        summary: "Change a sample",
        call: "sample.update",
        kind: Kind::Write {
            ack: Ack::Durable,
            if_match: IfMatch::Required,
        },
        input: &[
            Field::required("id", Type::String, "The sample's ID"),
            Field::optional("title", Type::String, "The new title"),
            Field::optional("body", Type::String, "The new body, as Markdown"),
        ],
        content_field: Some("body"),
        output: SAMPLE,
        errors: &[
            ErrorCode::NotFound,
            ErrorCode::NotAuthorized,
            ErrorCode::Conflict,
        ],
        example: Example {
            input: r#"{"id":"S-1","title":"Renamed"}"#,
            output: SAMPLE_RECORD,
        },
    },
];

/// The table of the sample rows.
pub fn sample_table() -> CommandTable {
    CommandTable::new(&[&SAMPLE_ROWS]).expect("the sample rows are valid")
}

/// A status the stub never answers by itself; kept here so a test can build such an answer.
pub fn answer_with(status: Status, data: Value) -> Answer {
    Answer {
        status,
        ..Answer::completed(data)
    }
}
