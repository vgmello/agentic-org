//! Everything a caller reads to find out what exists: the three levels of `--help`, the
//! `--schema` document, the hint of a refused command line and the launch catalogue. All of it
//! is generated from the table.

use crate::envelope::code_name;
use crate::node::Node;
use crate::shape;
use crate::table::{Command, CommandTable, Field, Type, OPTIONS};

/// The launch catalogue may not be longer than this (design 2.5).
pub const CATALOGUE_LIMIT: usize = 4096;

impl CommandTable {
    /// The short catalogue an agent receives at launch: one line for each command.
    pub fn catalogue(&self) -> String {
        self.rows()
            .iter()
            .map(|row| format!("{} {}: {}\n", row.resource, row.verb, row.summary))
            .collect()
    }
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
    out
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
    format!(
        "Flags of `{}`: {flags}. Options: {}. `{program} {} --help` says what each does.",
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
