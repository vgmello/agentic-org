//! The table of commands. A command is one row; the parser, `--help`, `--schema`, the launch
//! catalogue and the contract tests are all generated from the rows. Adding a command is adding
//! a row: nothing in the parser changes.

use std::collections::BTreeSet;
use std::fmt;

use org_proto::ErrorCode;
use serde_json::Value;

use crate::shape;

/// The type of one field of a command's input or output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    String,
    Integer,
    Boolean,
    /// One of the listed words.
    Enum(&'static [&'static str]),
    List(&'static Type),
    Object(&'static [Field]),
    /// Any JSON value. Use it only where the contract behind the command leaves the shape open.
    Json,
}

/// One field of a command's input or output. In an input, a field is also the flag
/// `--<name with hyphens>` when its type can be written as one (a text, a number, a word of a
/// list, a yes-or-no, or several of the first three).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    /// `snake_case`.
    pub name: &'static str,
    pub ty: Type,
    pub required: bool,
    /// One line, shown by `--help` and as the schema's description.
    pub help: &'static str,
}

impl Field {
    pub const fn required(name: &'static str, ty: Type, help: &'static str) -> Self {
        Self {
            name,
            ty,
            required: true,
            help,
        }
    }

    pub const fn optional(name: &'static str, ty: Type, help: &'static str) -> Self {
        Self {
            name,
            ty,
            required: false,
            help,
        }
    }

    /// The flag's name, without the two hyphens.
    pub fn flag(&self) -> String {
        self.name.replace('_', "-")
    }

    /// Whether the field can be given as a flag. A field with structure is given in `--json`,
    /// or in a file or standard input with `--input-format json` or `yaml`.
    pub fn is_flag(&self) -> bool {
        match self.ty {
            Type::String | Type::Integer | Type::Boolean | Type::Enum(_) => true,
            Type::List(inner) => matches!(inner, Type::String | Type::Integer | Type::Enum(_)),
            Type::Object(_) | Type::Json => false,
        }
    }
}

/// When a write is acknowledged (ADR-0014 decision 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ack {
    Committed,
    Durable,
}

impl Ack {
    pub fn as_str(self) -> &'static str {
        match self {
            Ack::Committed => "committed",
            Ack::Durable => "durable",
        }
    }
}

/// Whether a write states the revision its caller last saw (`--if-match`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IfMatch {
    /// The record has no revision; the flag is refused.
    No,
    Optional,
    Required,
}

/// What kind of command a row is. It decides which of the common options the command takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Answers one record.
    Read,
    /// Answers a list of records, in pages: `--limit`, `--cursor`.
    List,
    /// Changes something: `--dry-run`, `--idempotency-key`, `--durable`, and `--if-match` when
    /// the record has a revision.
    Write { ack: Ack, if_match: IfMatch },
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Read => "read",
            Kind::List => "list",
            Kind::Write { .. } => "write",
        }
    }
}

/// An input the command accepts and the `data` it then answers, both as JSON text. The contract
/// tests feed the input through the parser and the schema, and the stub office answers the output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Example {
    /// One JSON object.
    pub input: &'static str,
    /// One record, or for a list a JSON array of records.
    pub output: &'static str,
}

/// One row of the table: one command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Command {
    pub resource: &'static str,
    pub verb: &'static str,
    /// One line: the catalogue's and the help's.
    pub summary: &'static str,
    /// The name of the office's call. The wire keeps its own names.
    pub call: &'static str,
    pub kind: Kind,
    /// The input object's fields.
    pub input: &'static [Field],
    /// The one input field that carries long text, filled by `--input-file` or `--stdin`.
    pub content_field: Option<&'static str>,
    /// The fields of the record answered, or of each item of the list, in the order printed.
    pub output: &'static [Field],
    /// The error codes the office can answer this command with.
    pub errors: &'static [ErrorCode],
    pub example: Example,
}

impl Command {
    /// `<resource> <verb>`.
    pub fn name(&self) -> String {
        format!("{} {}", self.resource, self.verb)
    }

    pub fn input_field(&self, name: &str) -> Option<&'static Field> {
        self.input.iter().find(|field| field.name == name)
    }

    pub fn is_write(&self) -> bool {
        matches!(self.kind, Kind::Write { .. })
    }

    pub fn if_match(&self) -> IfMatch {
        match self.kind {
            Kind::Write { if_match, .. } => if_match,
            _ => IfMatch::No,
        }
    }

    pub fn ack(&self) -> Option<Ack> {
        match self.kind {
            Kind::Write { ack, .. } => Some(ack),
            _ => None,
        }
    }
}

/// Which commands take a common option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Applies {
    All,
    Lists,
    WithInput,
    Writes,
    Revisioned,
}

impl Applies {
    pub(crate) fn to(self, command: &Command) -> bool {
        match self {
            Applies::All => true,
            Applies::Lists => command.kind == Kind::List,
            Applies::WithInput => !command.input.is_empty(),
            Applies::Writes => command.is_write(),
            Applies::Revisioned => command.if_match() != IfMatch::No,
        }
    }

    pub(crate) fn who(self) -> &'static str {
        match self {
            Applies::All => "every command",
            Applies::Lists => "a command that lists",
            Applies::WithInput => "a command with input",
            Applies::Writes => "a command that writes",
            Applies::Revisioned => "a write to a record with a revision",
        }
    }
}

/// One of the options every command takes (design 2.1).
pub(crate) struct CommonOption {
    pub(crate) name: &'static str,
    /// The value's placeholder; `None` for a yes-or-no option.
    pub(crate) value: Option<&'static str>,
    pub(crate) applies: Applies,
    pub(crate) help: &'static str,
}

const fn option(
    name: &'static str,
    value: Option<&'static str>,
    applies: Applies,
    help: &'static str,
) -> CommonOption {
    CommonOption {
        name,
        value,
        applies,
        help,
    }
}

pub(crate) const OPTIONS: &[CommonOption] = &[
    option(
        "format",
        Some("json|yaml|compact"),
        Applies::All,
        "How the answer is printed. Default json",
    ),
    option(
        "fields",
        Some("a,b.c"),
        Applies::All,
        "Keep only these fields of data; a dot reaches into a nested field",
    ),
    option(
        "schema",
        None,
        Applies::All,
        "Print the command's input and output schema and stop",
    ),
    option("help", None, Applies::All, "Print this text and stop"),
    option(
        "limit",
        Some("n"),
        Applies::Lists,
        "Page size, 1 to 200. Default 20",
    ),
    option(
        "cursor",
        Some("token"),
        Applies::Lists,
        "Where to continue: meta.next_cursor of the page before",
    ),
    option(
        "json",
        Some("object"),
        Applies::WithInput,
        "The whole input, or the part the flags do not give, as one JSON object",
    ),
    option(
        "input-file",
        Some("path"),
        Applies::WithInput,
        "Read the content field from this file, bytes unchanged",
    ),
    option(
        "stdin",
        None,
        Applies::WithInput,
        "Read the content field from standard input, bytes unchanged",
    ),
    option(
        "input-format",
        Some("text|json|yaml"),
        Applies::WithInput,
        "With json or yaml, the file or standard input is the whole input (yaml: at most 64 KiB). Default text",
    ),
    option(
        "dry-run",
        None,
        Applies::Writes,
        "Decide and report; change nothing",
    ),
    option(
        "idempotency-key",
        Some("key"),
        Applies::Writes,
        "A key you choose, up to 64 letters, digits and hyphens, for safe retries",
    ),
    option(
        "if-match",
        Some("revision"),
        Applies::Revisioned,
        "The revision you last saw",
    ),
    option(
        "durable",
        None,
        Applies::Writes,
        "Answer only after the drive has it",
    ),
];

pub(crate) fn common_option(name: &str) -> Option<&'static CommonOption> {
    OPTIONS.iter().find(|option| option.name == name)
}

/// Why a table was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableError(pub String);

impl fmt::Display for TableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TableError {}

/// The commands that exist. A command that is not in the table does not exist.
#[derive(Debug, Clone)]
pub struct CommandTable {
    rows: Vec<&'static Command>,
}

impl CommandTable {
    /// Builds the table from each domain's rows and checks every row: its names, its fields,
    /// its content field and its example. The rows are kept in name order whatever order the
    /// domains are listed in.
    pub fn new(domains: &[&'static [Command]]) -> Result<Self, TableError> {
        let mut rows: Vec<&'static Command> = domains.iter().flat_map(|rows| rows.iter()).collect();
        rows.sort_by_key(|row| (row.resource, row.verb));
        for pair in rows.windows(2) {
            if (pair[0].resource, pair[0].verb) == (pair[1].resource, pair[1].verb) {
                return Err(TableError(format!(
                    "`{}` is in the table twice",
                    pair[0].name()
                )));
            }
        }
        for row in &rows {
            check_row(row).map_err(|problem| TableError(format!("`{}`: {problem}", row.name())))?;
        }
        Ok(Self { rows })
    }

    /// The same table without the checks, for an executable's start: a common command does
    /// nothing its call does not need (design 2.11), and reading every row's example is such
    /// work. The contract tests build the same rows with [`CommandTable::new`], which is what
    /// makes skipping the checks here safe.
    pub fn unchecked(domains: &[&'static [Command]]) -> Self {
        let mut rows: Vec<&'static Command> = domains.iter().flat_map(|rows| rows.iter()).collect();
        rows.sort_by_key(|row| (row.resource, row.verb));
        Self { rows }
    }

    /// Every row, in name order.
    pub fn rows(&self) -> &[&'static Command] {
        &self.rows
    }

    pub fn find(&self, resource: &str, verb: &str) -> Option<&'static Command> {
        self.rows
            .iter()
            .copied()
            .find(|row| row.resource == resource && row.verb == verb)
    }

    /// The resources, in name order, each once.
    pub fn resources(&self) -> Vec<&'static str> {
        let mut resources: Vec<&'static str> = self.rows.iter().map(|row| row.resource).collect();
        resources.dedup();
        resources
    }

    /// The verbs of one resource, in name order.
    pub fn verbs(&self, resource: &str) -> Vec<&'static Command> {
        self.rows
            .iter()
            .copied()
            .filter(|row| row.resource == resource)
            .collect()
    }
}

/// A single lower-case word, or two joined by a hyphen (design 2.1).
fn is_command_word(word: &str) -> bool {
    let mut parts = word.split('-');
    let ok = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_lowercase());
    matches!((parts.next(), parts.next(), parts.next()), (Some(a), b, None) if ok(a) && b.is_none_or(ok))
}

fn is_field_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b) if b.is_ascii_lowercase())
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        && !name.ends_with('_')
        && !name.contains("__")
}

fn is_one_line(text: &str) -> bool {
    !text.trim().is_empty() && !text.chars().any(char::is_control)
}

fn check_fields(fields: &[Field], what: &str) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for field in fields {
        if !is_field_name(field.name) {
            return Err(format!("{what} field `{}` is not snake_case", field.name));
        }
        if !seen.insert(field.name) {
            return Err(format!("{what} field `{}` is declared twice", field.name));
        }
        if !is_one_line(field.help) {
            return Err(format!(
                "{what} field `{}` needs one line of help",
                field.name
            ));
        }
        check_type(&field.ty, field.name, what)?;
    }
    Ok(())
}

fn check_type(ty: &Type, name: &str, what: &str) -> Result<(), String> {
    match ty {
        Type::Enum([]) => Err(format!("{what} field `{name}` lists no value")),
        Type::List(inner) => check_type(inner, name, what),
        Type::Object(fields) => check_fields(fields, what),
        _ => Ok(()),
    }
}

fn check_row(row: &Command) -> Result<(), String> {
    if !is_command_word(row.resource) || !is_command_word(row.verb) {
        return Err(
            "a resource and a verb are lower-case words, at most two joined by a hyphen".into(),
        );
    }
    if !is_one_line(row.summary) {
        return Err("the summary is one line".into());
    }
    if row.call.trim().is_empty() {
        return Err("the row names no office call".into());
    }
    check_fields(row.input, "input")?;
    check_fields(row.output, "output")?;
    for field in row.input {
        if common_option(&field.flag()).is_some() {
            return Err(format!(
                "input field `{}` has the name of a common option",
                field.name
            ));
        }
    }
    if let Some(name) = row.content_field {
        match row.input_field(name) {
            Some(field) if field.ty == Type::String => {}
            _ => {
                return Err(format!(
                    "the content field `{name}` is not a text field of the input"
                ))
            }
        }
    }
    let input: Value = serde_json::from_str(row.example.input)
        .map_err(|e| format!("the example input is not JSON: {e}"))?;
    let Value::Object(input) = input else {
        return Err("the example input is not a JSON object".into());
    };
    shape::validate(row.input, &input).map_err(|e| format!("the example input: {e}"))?;
    let output: Value = serde_json::from_str(row.example.output)
        .map_err(|e| format!("the example output is not JSON: {e}"))?;
    shape::validate_data(row, &output).map_err(|e| format!("the example output: {e}"))?;
    Ok(())
}
