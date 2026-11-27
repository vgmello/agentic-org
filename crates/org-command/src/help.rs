//! Everything a caller reads to find out what exists: the three levels of `--help`, the
//! `--schema` document, the hint of a refused command line and the launch catalogue. All of it
//! is generated from the table.

use serde_json::{Map, Value};

use crate::envelope::code_name;
use crate::node::Node;
use crate::row::{Command, Field, IfMatch, Type, OPTIONS};
use crate::shape;
use crate::table::CommandTable;

/// The launch catalogue may not be longer than this (design 2.5).
pub const CATALOGUE_LIMIT: usize = 4096;

/// The lines that open the launch catalogue, before one line for each resource.
pub(crate) const CATALOGUE_HEADING: &str = "## Commands\n\
Each line is a resource and its verbs: run `<resource> <verb> --flag value`. `--help` after a \
resource or a command lists its verbs or flags, with an example; `--schema` shows a command's \
input and answer.\n";

impl CommandTable {
    /// The short catalogue an agent receives at launch, grouped by resource: the heading, then
    /// one line for each resource with its verbs in name order (`task: create, inspect, list`),
    /// then the shorthands when there are any.
    /// The summaries are in `--help`, so the catalogue grows by a word, not a line, for each
    /// command and stays far under [`CATALOGUE_LIMIT`]. An empty table has the heading alone.
    pub fn catalogue(&self) -> String {
        let mut out = CATALOGUE_HEADING.to_owned();
        for resource in self.resources() {
            out.push_str(resource);
            out.push_str(": ");
            out.push_str(&verb_names(&self.verbs(resource)));
            out.push('\n');
        }
        if let Some(line) = shorthands_line(self) {
            out.push_str(&line);
            out.push('\n');
        }
        out
    }
}

/// The catalogue's last line when the table has shorthands:
/// `Shorthands: who = directory list; context = session context.`
pub(crate) fn shorthands_line(table: &CommandTable) -> Option<String> {
    let aliases = table.aliases();
    if aliases.is_empty() {
        return None;
    }
    let pairs: Vec<String> = aliases
        .iter()
        .map(|alias| format!("{} = {}", alias.words.join(" "), alias.command.join(" ")))
        .collect();
    Some(format!("Shorthands: {}.", pairs.join("; ")))
}

fn verb_names(rows: &[&'static Command]) -> String {
    rows.iter()
        .map(|row| row.verb)
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn resources_hint(table: &CommandTable, program: &str) -> String {
    let resources = table.resources();
    if resources.is_empty() {
        return format!("{program} has no command yet.");
    }
    format!(
        "The resources are: {}. `{program} <resource> --help` lists a resource's verbs.",
        resources.join(", ")
    )
}

pub(crate) fn verbs_hint(table: &CommandTable, program: &str, resource: &str) -> String {
    format!(
        "The verbs of `{resource}` are: {}. `{program} {resource} <verb> --help` lists a command's flags.",
        verb_names(&table.verbs(resource))
    )
}

/// `--help` with no command: the resources.
pub(crate) fn top(table: &CommandTable, program: &str) -> String {
    let mut out = format!("Usage: {program} <resource> <verb> [--flag <value>]...\n\nResources:\n");
    let width = table
        .resources()
        .iter()
        .map(|name| name.len())
        .max()
        .unwrap_or(0);
    for resource in table.resources() {
        out.push_str(&format!(
            "  {resource:width$}  {}\n",
            verb_names(&table.verbs(resource))
        ));
    }
    out.push_str(&format!(
        "\n`{program} <resource> --help` lists a resource's verbs.\n`{program} <resource> <verb> --help` lists a command's flags.\n"
    ));
    out
}

/// `<resource> --help`: its verbs.
pub(crate) fn resource(table: &CommandTable, program: &str, resource: &str) -> String {
    let rows = table.verbs(resource);
    let width = rows.iter().map(|row| row.verb.len()).max().unwrap_or(0);
    let mut out = format!("Usage: {program} {resource} <verb> [--flag <value>]...\n\nVerbs:\n");
    for row in rows {
        out.push_str(&format!("  {:width$}  {}\n", row.verb, row.summary));
    }
    out.push_str(&format!(
        "\n`{program} {resource} <verb> --help` lists a command's flags.\n"
    ));
    out
}

fn placeholder(ty: &Type) -> String {
    match ty {
        Type::String => "<text>".to_owned(),
        Type::Integer => "<number>".to_owned(),
        Type::Boolean => String::new(),
        Type::Enum(words) => format!("<{}>", words.join("|")),
        Type::List(inner) => format!("{} (repeat for several)", placeholder(inner)),
        Type::Object(_) | Type::Json => "(structure)".to_owned(),
    }
}

fn flag_usage(field: &Field) -> String {
    format!("--{} {}", field.flag(), placeholder(&field.ty))
        .trim_end()
        .to_owned()
}

/// `<resource> <verb> --help`: the command's flags.
pub(crate) fn command(program: &str, command: &Command) -> String {
    let mut out = format!(
        "Usage: {program} {} [--flag <value>]...\n\n{}\n",
        command.name(),
        command.summary
    );
    let mut lines: Vec<(String, String)> = Vec::new();
    for field in command.input.iter().filter(|field| field.is_flag()) {
        let required = if field.required { "Required. " } else { "" };
        lines.push((flag_usage(field), format!("{required}{}", field.help)));
    }
    if !lines.is_empty() {
        out.push_str("\nFlags:\n");
        push_lines(&mut out, &lines);
    }
    let structured: Vec<(String, String)> = command
        .input
        .iter()
        .filter(|field| !field.is_flag())
        .map(|field| {
            let required = if field.required { "Required. " } else { "" };
            (field.name.to_owned(), format!("{required}{}", field.help))
        })
        .collect();
    if !structured.is_empty() {
        out.push_str(
            "\nInput with structure, given in --json or with --input-format json or yaml:\n",
        );
        push_lines(&mut out, &structured);
    }
    if let Some(name) = command.content_field {
        out.push_str(&format!(
            "\nContent: `{name}`. Give long text with --input-file <path> or --stdin; it needs no escaping.\n"
        ));
    }
    let options: Vec<(String, String)> = OPTIONS
        .iter()
        .filter(|option| option.applies.to(command))
        .map(|option| {
            let usage = match option.value {
                Some(value) => format!("--{} <{value}>", option.name),
                None => format!("--{}", option.name),
            };
            (usage, option.help.to_owned())
        })
        .collect();
    out.push_str("\nOptions:\n");
    push_lines(&mut out, &options);
    let words: Vec<String> = example_args(command)
        .iter()
        .map(|word| shell_word(word))
        .collect();
    out.push_str(&format!("\nExample:\n  {program} {}\n", words.join(" ")));
    out
}

/// The command line of the row's example (AX-7), as the words a shell hands the program: each
/// input field a flag can carry as that flag, in the row's field order, and every other field
/// in one `--json` object. A text that holds a character that does not print goes in `--json`
/// too, where it is an escape, so the example is always one line a caller can copy. A write
/// that requires `--if-match` gets revision 1.
pub(crate) fn example_args(command: &Command) -> Vec<String> {
    let mut args = vec![command.resource.to_owned(), command.verb.to_owned()];
    let input = match serde_json::from_str::<Value>(command.example.input) {
        Ok(Value::Object(input)) => input,
        _ => Map::new(),
    };
    let mut rest = Map::new();
    for (name, value) in &input {
        match command.input_field(name) {
            Some(field) if flag_words(field, value).is_some() => {}
            _ => {
                rest.insert(name.clone(), value.clone());
            }
        }
    }
    for field in command.input {
        if let Some(words) = input
            .get(field.name)
            .and_then(|value| flag_words(field, value))
        {
            args.extend(words);
        }
    }
    if !rest.is_empty() {
        args.push("--json".to_owned());
        args.push(Node::from_value(&Value::Object(rest)).to_one_line());
    }
    if command.if_match() == IfMatch::Required {
        args.push("--if-match".to_owned());
        args.push("1".to_owned());
    }
    args
}

/// `value` written as `field`'s flag, when a flag carries it exactly.
fn flag_words(field: &Field, value: &Value) -> Option<Vec<String>> {
    if !field.is_flag() {
        return None;
    }
    let flag = format!("--{}", field.flag());
    match (&field.ty, value) {
        (Type::Boolean, Value::Bool(true)) => Some(vec![flag]),
        (Type::List(_), Value::Array(items)) if !items.is_empty() => {
            let mut words = Vec::with_capacity(items.len() * 2);
            for item in items {
                words.push(flag.clone());
                words.push(scalar_word(item)?);
            }
            Some(words)
        }
        (Type::String | Type::Integer | Type::Enum(_), value) => {
            Some(vec![flag, scalar_word(value)?])
        }
        _ => None,
    }
}

fn scalar_word(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.chars().any(|c| c.is_control() || is_separator(c)) => {
            Some(text.clone())
        }
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn is_separator(c: char) -> bool {
    matches!(c, '\u{2028}' | '\u{2029}')
}

/// `word` as a POSIX shell reads it back: bare when it holds only characters no shell treats
/// specially, otherwise in single quotes (a single quote inside is written `'\''`).
pub(crate) fn shell_word(word: &str) -> String {
    let bare = !word.is_empty()
        && word
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_./:,@%+".contains(&b));
    if bare {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

fn push_lines(out: &mut String, lines: &[(String, String)]) {
    let width = lines
        .iter()
        .map(|(usage, _)| usage.len())
        .max()
        .unwrap_or(0);
    for (usage, help) in lines {
        out.push_str(&format!("  {usage:width$}  {help}\n"));
    }
}

/// The hint of a refused command line: the flags the command takes.
pub(crate) fn flags_hint(program: &str, command: &Command) -> String {
    let flags: Vec<String> = command
        .input
        .iter()
        .filter(|field| field.is_flag())
        .map(|field| {
            let usage = flag_usage(field);
            if field.required {
                format!("{usage} (required)")
            } else {
                usage
            }
        })
        .collect();
    let options: Vec<String> = OPTIONS
        .iter()
        .filter(|option| option.applies.to(command))
        .map(|option| format!("--{}", option.name))
        .collect();
    let flags = if flags.is_empty() {
        "none".to_owned()
    } else {
        flags.join(", ")
    };
    // A field with structure has no flag: name it, so a caller knows to give it in `--json`.
    let structured: Vec<String> = command
        .input
        .iter()
        .filter(|field| !field.is_flag())
        .map(|field| {
            if field.required {
                format!("{} (required)", field.name)
            } else {
                field.name.to_owned()
            }
        })
        .collect();
    let structured = if structured.is_empty() {
        String::new()
    } else {
        format!(" Fields given in --json: {}.", structured.join(", "))
    };
    format!(
        "Flags of `{}`: {flags}.{structured} Options: {}. `{program} {} --help` says what each does.",
        command.name(),
        options.join(", "),
        command.name()
    )
}

/// What `--schema` answers as `data`.
pub(crate) fn schema(command: &Command) -> Node {
    Node::map([
        ("command", Node::String(command.name())),
        ("call", Node::text(command.call)),
        ("kind", Node::text(command.kind.as_str())),
        ("input", shape::input_schema(command)),
        ("output", shape::output_schema(command)),
        (
            "content_field",
            command.content_field.map_or(Node::Null, Node::text),
        ),
        (
            "errors",
            Node::List(
                command
                    .errors
                    .iter()
                    .map(|code| Node::String(code_name(*code)))
                    .collect(),
            ),
        ),
        (
            "ack",
            command
                .ack()
                .map_or(Node::Null, |ack| Node::text(ack.as_str())),
        ),
    ])
}
