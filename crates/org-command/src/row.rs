//! One row of the table of commands: a command, its fields and their types, and the options
//! every command takes. The table, the parser, `--help` and `--schema` all read these rows.

use org_proto::ErrorCode;

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
        "The whole input, or the part the flags do not give, as one JSON object of at most 1 MiB",
    ),
    option(
        "input-file",
        Some("path"),
        Applies::WithInput,
        "Read the content field from this file, bytes unchanged, at most 1 MiB",
    ),
    option(
        "stdin",
        None,
        Applies::WithInput,
        "Read the content field from standard input, bytes unchanged, at most 1 MiB",
    ),
    option(
        "input-format",
        Some("text|json|yaml"),
        Applies::WithInput,
        "With json or yaml, the file or standard input is the whole input (json: at most 1 MiB; yaml: at most 64 KiB). Default text",
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
        "A key you choose, up to 64 letters, digits and hyphens, for safe retries. Without it the command makes one and reports it in meta.operation_id",
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
