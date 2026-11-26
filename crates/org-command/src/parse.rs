//! Reads a command line against the table: `<resource> <verb>`, named flags, no positional
//! argument. It knows no command by name; everything about a command comes from its row.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use crate::help;
use crate::office::{Request, Sources};
use crate::shape::{self, FieldPath};
use crate::table::{common_option, Command, CommandTable, Field, IfMatch, Kind, Type};

/// The default page of a list, and the largest.
pub const DEFAULT_LIMIT: u32 = 20;
pub const MAX_LIMIT: u32 = 200;
const MAX_KEY_LEN: usize = 64;
/// The most a command reads from `--input-file` or `--stdin` as text or JSON, and the longest
/// `--json` value: 1 MiB.
pub const MAX_INPUT_BYTES: usize = 1024 * 1024;
/// The most a command reads with `--input-format yaml`: the bound of the YAML reader, 64 KiB.
pub const MAX_YAML_INPUT_BYTES: usize = org_docs::MAX_FRONT_MATTER_BYTES;

/// Whether `key` is an idempotency key: 1 to 64 letters, digits and hyphens.
pub(crate) fn is_key(key: &str) -> bool {
    (1..=MAX_KEY_LEN).contains(&key.len())
        && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// How the answer is printed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Format {
    Json,
    Yaml,
    Compact,
}

/// A command line that could not be read: `invalid_request`, exit code 2.
#[derive(Debug)]
pub(crate) struct Usage {
    pub(crate) message: String,
    pub(crate) hint: String,
}

pub(crate) struct Invocation {
    pub(crate) request: Request,
    pub(crate) format: Format,
    pub(crate) fields: Option<Vec<FieldPath>>,
}

pub(crate) enum Parsed {
    Help(String),
    Schema(&'static Command, Format),
    Call(Box<Invocation>),
}

pub(crate) fn parse(
    table: &CommandTable,
    program: &str,
    args: &[String],
    sources: &mut dyn Sources,
) -> Result<Parsed, Usage> {
    let usage = |message: String, hint: String| Usage { message, hint };
    let Some(resource) = args.first() else {
        return Err(usage(
            "No command was given.".into(),
            help::resources_hint(table, program),
        ));
    };
    if is_help(resource) {
        return Ok(Parsed::Help(help::top(table, program)));
    }
    if table.verbs(resource).is_empty() {
        return Err(usage(
            format!("`{resource}` is not a resource. A command is `{program} <resource> <verb>`."),
            help::resources_hint(table, program),
        ));
    }
    let verb = match args.get(1) {
        Some(verb) if is_help(verb) => {
            return Ok(Parsed::Help(help::resource(table, program, resource)))
        }
        Some(verb) => verb,
        None => {
            return Err(usage(
                format!("`{resource}` needs a verb."),
                help::verbs_hint(table, program, resource),
            ))
        }
    };
    let Some(command) = table.find(resource, verb) else {
        return Err(usage(
            format!("`{resource} {verb}` is not a command."),
            help::verbs_hint(table, program, resource),
        ));
    };
    let rest = &args[2..];
    // A caller that asked for help gets it, whatever else is wrong with the line.
    if asks_for_help(command, rest) {
        return Ok(Parsed::Help(help::command(program, command)));
    }
    let mut read = || -> Result<Parsed, Problem> {
        let scanned = scan(command, rest)?;
        let format = scanned.format()?;
        if scanned.options.contains_key("schema") {
            return Ok(Parsed::Schema(command, format));
        }
        let fields = scanned
            .option("fields")
            .map(|text| shape::field_paths(command, text))
            .transpose()?;
        let request = build(command, &scanned, sources)?;
        Ok(Parsed::Call(Box::new(Invocation {
            request,
            format,
            fields,
        })))
    };
    match read() {
        Ok(parsed) => Ok(parsed),
        Err(problem) => {
            let flags = help::flags_hint(program, command);
            let hint = match problem.meant {
                Some(meant) => format!("{meant} {flags}"),
                None => flags,
            };
            Err(usage(problem.message, hint))
        }
    }
}

/// Whether `arg` asks for help: `--help`, or its short form `-h`.
pub(crate) fn is_help(arg: &str) -> bool {
    arg == "--help" || arg == "-h"
}

/// Whether `--help` (or `-h`) is written where a flag is read. The scan is tolerant: it stops
/// at nothing, so help is found on a line that is otherwise wrong. It skips the value of every
/// flag it knows takes one, so `--body --help` gives the body the text `--help` and is not a
/// request for help.
fn asks_for_help(command: &Command, rest: &[String]) -> bool {
    let mut at = 0;
    while let Some(arg) = rest.get(at) {
        at += 1;
        if is_help(arg) {
            return true;
        }
        let Some(name) = arg.strip_prefix("--") else {
            continue;
        };
        let takes_value = match common_option(name) {
            Some(option) => option.value.is_some(),
            None => command
                .input
                .iter()
                .any(|field| field.flag() == name && field.is_flag() && field.ty != Type::Boolean),
        };
        // `--name=value` is no flag's name, so nothing is skipped after it.
        if takes_value {
            at += 1;
        }
    }
    false
}

/// Why a command's flags could not be read, and what to do first: the flag that was probably
/// meant, or the limit that was passed. The hint then lists the command's flags.
struct Problem {
    message: String,
    meant: Option<String>,
}

impl From<String> for Problem {
    fn from(message: String) -> Self {
        Self {
            message,
            meant: None,
        }
    }
}

/// The flags of a command line, as written: nothing converted, nothing read.
struct Scanned {
    /// The common options given; a yes-or-no option has an empty value.
    options: BTreeMap<&'static str, String>,
    /// The input flags given, each with its values in the order written.
    fields: Vec<(&'static Field, Vec<String>)>,
}

impl Scanned {
    fn option(&self, name: &str) -> Option<&str> {
        self.options.get(name).map(String::as_str)
    }

    fn format(&self) -> Result<Format, String> {
        match self.option("format") {
            None | Some("json") => Ok(Format::Json),
            Some("yaml") => Ok(Format::Yaml),
            Some("compact") => Ok(Format::Compact),
            Some(other) => Err(format!("--format is json, yaml or compact, not `{other}`.")),
        }
    }
}

fn scan(command: &'static Command, rest: &[String]) -> Result<Scanned, Problem> {
    let mut scanned = Scanned {
        options: BTreeMap::new(),
        fields: Vec::new(),
    };
    let mut at = 0;
    while let Some(arg) = rest.get(at) {
        at += 1;
        let Some(body) = arg.strip_prefix("--").filter(|body| !body.is_empty()) else {
            return Err(positional(command, arg));
        };
        let (name, inline) = match body.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (body, None),
        };
        let option = common_option(name);
        let field = command.input.iter().find(|field| field.flag() == name);
        let takes_value = match (option, field) {
            (Some(option), _) if !option.applies.to(command) => {
                return Err(Problem::from(format!(
                    "--{name} is for {}, and `{}` is not one.",
                    option.applies.who(),
                    command.name()
                )))
            }
            (Some(option), _) => option.value.is_some(),
            (None, Some(field)) if !field.is_flag() => {
                return Err(Problem::from(format!(
                    "`{}` has structure and is not a flag. Give it in --json, or in a file with --input-format json or yaml.",
                    field.name
                )))
            }
            (None, Some(field)) => field.ty != Type::Boolean,
            (None, None) => return Err(Problem::from(format!("--{name} is not a flag of `{}`.", command.name()))),
        };
        let value = match (takes_value, inline) {
            (true, Some(value)) => value.to_owned(),
            (true, None) => {
                let value = rest
                    .get(at)
                    .ok_or_else(|| format!("--{name} needs a value."))?;
                at += 1;
                value.clone()
            }
            (false, Some(_)) => {
                return Err(Problem::from(format!(
                    "--{name} is a yes-or-no flag and takes no value."
                )))
            }
            (false, None) => String::new(),
        };
        match (option, field) {
            (Some(option), _) => {
                if scanned.options.insert(option.name, value).is_some() {
                    return Err(Problem::from(format!("--{name} is given twice.")));
                }
            }
            (None, Some(field)) => match scanned
                .fields
                .iter_mut()
                .find(|(seen, _)| seen.name == field.name)
            {
                Some((_, values)) => values.push(value),
                None => scanned.fields.push((field, vec![value])),
            },
            (None, None) => unreachable!("an unknown flag was refused above"),
        }
    }
    Ok(scanned)
}

fn positional(command: &Command, word: &str) -> Problem {
    let guess = command
        .content_field
        .and_then(|name| command.input_field(name))
        .or_else(|| {
            command
                .input
                .iter()
                .find(|field| field.required && field.is_flag())
        })
        .or_else(|| command.input.iter().find(|field| field.is_flag()))
        .filter(|field| field.ty != Type::Boolean);
    Problem {
        message: format!("`{word}` is a bare word. Every value is given with a named flag."),
        meant: guess.map(|field| {
            format!(
                "Did you mean --{} {}?",
                field.flag(),
                Value::String(word.to_owned())
            )
        }),
    }
}

/// The value of one input flag, from what was written for it.
fn flag_value(field: &Field, written: &[String]) -> Result<Value, String> {
    let flag = field.flag();
    let one = |ty: &Type, text: &str| -> Result<Value, String> {
        match ty {
            // The same range as an integer in `--json`: an `i64`, or a `u64` above it.
            Type::Integer => text
                .parse::<i64>()
                .map(Value::from)
                .or_else(|_| text.parse::<u64>().map(Value::from))
                .map_err(|_| format!("--{flag} takes a whole number, not `{text}`.")),
            Type::Enum(words) if !words.contains(&text) => Err(format!(
                "--{flag} is one of {}, not `{text}`.",
                words.join(", ")
            )),
            _ => Ok(Value::String(text.to_owned())),
        }
    };
    match (&field.ty, written) {
        (Type::List(inner), _) => written
            .iter()
            .map(|text| one(inner, text))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        (_, [_, _, ..]) => Err(format!("--{flag} is given twice.")),
        (Type::Boolean, _) => Ok(Value::Bool(true)),
        (ty, [text]) => one(ty, text),
        (_, []) => unreachable!("a scanned flag has a value"),
    }
}

/// Adds `from` to the input. A field given two ways with different values is refused.
fn merge(
    input: &mut Map<String, Value>,
    from: Map<String, Value>,
    source: &str,
) -> Result<(), String> {
    for (name, value) in from {
        match input.get(&name) {
            Some(given) if *given != value => {
                return Err(format!(
                    "`{name}` is given twice with different values: once before, and in {source}."
                ))
            }
            Some(_) => {}
            None => {
                input.insert(name, value);
            }
        }
    }
    Ok(())
}

fn json_object(text: &str, source: &str) -> Result<Map<String, Value>, String> {
    match serde_json::from_str(text) {
        Ok(Value::Object(object)) => match duplicate_key(text) {
            Some(key) => Err(format!(
                "{source} gives `{key}` twice in one object. Give each field once."
            )),
            None => Ok(object),
        },
        Ok(_) => Err(format!("{source} must be one JSON object.")),
        Err(error) => Err(format!("{source} is not JSON: {error}.")),
    }
}

/// The first key that one object of `text` holds twice, at any depth. `text` is JSON that
/// `serde_json` has already read, which keeps the last of two equal keys and says nothing. Two
/// keys are equal when they are the same text once their escapes are read.
fn duplicate_key(text: &str) -> Option<String> {
    enum Open {
        Object {
            keys: BTreeSet<String>,
            key_next: bool,
        },
        Array,
    }
    let bytes = text.as_bytes();
    let mut open: Vec<Open> = Vec::new();
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        match byte {
            b'{' => open.push(Open::Object {
                keys: BTreeSet::new(),
                key_next: true,
            }),
            b'[' => open.push(Open::Array),
            b'}' | b']' => {
                open.pop();
            }
            b',' => {
                if let Some(Open::Object { key_next, .. }) = open.last_mut() {
                    *key_next = true;
                }
            }
            b'"' => {
                let start = at;
                at += 1;
                // A byte of a character longer than one byte is never a quote or a backslash.
                while *bytes.get(at)? != b'"' {
                    at += if bytes[at] == b'\\' { 2 } else { 1 };
                }
                if let Some(Open::Object { keys, key_next }) = open.last_mut() {
                    if *key_next {
                        *key_next = false;
                        let key: String = serde_json::from_str(text.get(start..=at)?).ok()?;
                        if keys.contains(&key) {
                            return Some(key);
                        }
                        keys.insert(key);
                    }
                }
            }
            _ => {}
        }
        at += 1;
    }
    None
}

/// Reads one YAML mapping with the limits of `org-docs`: 64 KiB, one document, no anchor, alias,
/// tag or merge key, no key twice.
fn yaml_object(text: &str, source: &str) -> Result<Map<String, Value>, String> {
    use org_docs::Reason;
    let error = match org_docs::parse_mapping(text) {
        Ok(object) => return Ok(object),
        Err(error) => error,
    };
    let what = match &error.reason {
        Reason::Syntax(text) => format!("it is not YAML ({text})"),
        Reason::NotAMapping => "it is not one mapping of names to values".to_owned(),
        Reason::TooLarge => "it is over 64 KiB".to_owned(),
        Reason::Alias | Reason::Anchor => "an anchor or an alias is not read".to_owned(),
        Reason::Tag => "a tag is not read".to_owned(),
        Reason::MergeKey => "a `<<` merge key is not read".to_owned(),
        Reason::DuplicateKey => "the name is given twice".to_owned(),
        Reason::NonStringKey => "a name is not text".to_owned(),
        Reason::TooDeep => "it is nested too deep".to_owned(),
        Reason::TooManyEvents => "it holds too many values".to_owned(),
        Reason::MultipleDocuments => "it holds more than one document".to_owned(),
        Reason::BadNumber => "a number is out of range".to_owned(),
        Reason::Unclosed => "it is not closed".to_owned(),
    };
    Err(if error.field.is_empty() {
        format!("{source} is not a YAML object: {what}.")
    } else {
        format!(
            "{source} is not a YAML object: at `{}`, {what}.",
            error.field
        )
    })
}

/// An input over its limit: `invalid_request`, with the limits as what to do first.
fn too_long(source: &str, limit: usize) -> Problem {
    Problem {
        message: format!("{source} is longer than the {limit} bytes a command reads."),
        meant: Some(format!(
            "Give less: text and JSON are at most 1 MiB ({MAX_INPUT_BYTES} bytes), YAML at most 64 KiB ({MAX_YAML_INPUT_BYTES} bytes)."
        )),
    }
}

fn build(
    command: &'static Command,
    scanned: &Scanned,
    sources: &mut dyn Sources,
) -> Result<Request, Problem> {
    let mut input = Map::new();
    for (field, written) in &scanned.fields {
        input.insert(field.name.to_owned(), flag_value(field, written)?);
    }
    if let Some(text) = scanned.option("json") {
        if text.len() > MAX_INPUT_BYTES {
            return Err(too_long("--json", MAX_INPUT_BYTES));
        }
        let mut object = json_object(text, "--json")?;
        shape::normalize(command.input, &mut object);
        merge(&mut input, object, "--json")?;
    }
    let input_format = scanned.option("input-format");
    if !matches!(input_format, None | Some("text" | "json" | "yaml")) {
        return Err(Problem::from(
            "--input-format is text, json or yaml.".to_owned(),
        ));
    }
    let from_stdin = scanned.options.contains_key("stdin");
    let source = match (scanned.option("input-file"), from_stdin) {
        (Some(_), true) => {
            return Err(Problem::from(
                "--input-file and --stdin cannot be given together.".to_owned(),
            ))
        }
        (Some(_), false) => Some("--input-file"),
        (None, true) => Some("--stdin"),
        (None, false) if input_format.is_some() => {
            return Err(Problem::from(
                "--input-format needs --input-file or --stdin.".to_owned(),
            ))
        }
        (None, false) => None,
    };
    if let Some(source) = source {
        let whole_input = matches!(input_format, Some("json" | "yaml"));
        // Refused before anything is read: a command with no content field reads no content.
        let content_field = match (whole_input, command.content_field) {
            (false, None) => {
                return Err(Problem::from(format!(
                    "`{}` has no content field, so {source} needs --input-format json or yaml.",
                    command.name()
                )))
            }
            (_, field) => field,
        };
        // The source reads no more than one byte over the limit, whatever it is given.
        let limit = match input_format {
            Some("yaml") => MAX_YAML_INPUT_BYTES,
            _ => MAX_INPUT_BYTES,
        };
        let bytes = match scanned.option("input-file") {
            Some(path) => sources.file(path, limit),
            None => sources.stdin(limit),
        }
        .map_err(|error| format!("{source} could not be read: {}.", error.kind()))?;
        if bytes.len() > limit {
            return Err(too_long(source, limit));
        }
        let text = String::from_utf8(bytes).map_err(|_| format!("{source} is not valid UTF-8."))?;
        let mut object = match (input_format, content_field) {
            (Some("json"), _) => json_object(&text, source)?,
            (Some("yaml"), _) => yaml_object(&text, source)?,
            (_, Some(name)) => Map::from_iter([(name.to_owned(), Value::String(text))]),
            (_, None) => unreachable!("refused above"),
        };
        shape::normalize(command.input, &mut object);
        merge(&mut input, object, source)?;
    }
    shape::validate(command.input, &input)
        .map_err(|problem| format!("The input is not valid: {problem}."))?;

    let limit = match (command.kind, scanned.option("limit")) {
        (Kind::List, None) => Some(DEFAULT_LIMIT),
        (_, None) => None,
        (_, Some(text)) => match text.parse::<u32>() {
            Ok(limit) if (1..=MAX_LIMIT).contains(&limit) => Some(limit),
            _ => {
                return Err(Problem::from(format!(
                    "--limit is a number from 1 to {MAX_LIMIT}, not `{text}`."
                )))
            }
        },
    };
    let cursor = scanned.option("cursor").map(str::to_owned);
    if cursor.as_deref() == Some("") {
        return Err(Problem::from(
            "--cursor is empty. Give meta.next_cursor of the page before.".to_owned(),
        ));
    }
    let dry_run = scanned.options.contains_key("dry-run");
    let idempotency_key = scanned.option("idempotency-key").map(str::to_owned);
    if let Some(key) = &idempotency_key {
        if dry_run {
            return Err(Problem::from(
                "A dry run changes nothing, so it takes no --idempotency-key.".to_owned(),
            ));
        }
        if !is_key(key) {
            return Err(Problem::from(format!(
                "--idempotency-key is 1 to {MAX_KEY_LEN} letters, digits and hyphens."
            )));
        }
    }
    let if_match = match scanned.option("if-match") {
        Some(text) => Some(
            text.parse::<u64>()
                .map_err(|_| format!("--if-match is a revision number, not `{text}`."))?,
        ),
        None if command.if_match() == IfMatch::Required => {
            return Err(Problem::from(
                "--if-match is required: give the revision you last read.".to_owned(),
            ))
        }
        None => None,
    };
    Ok(Request {
        command,
        input,
        limit,
        cursor,
        dry_run,
        idempotency_key,
        if_match,
        durable: scanned.options.contains_key("durable"),
    })
}
