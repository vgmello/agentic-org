//! The table of commands. A command is one row; the parser, `--help`, `--schema`, the launch
//! catalogue and the contract tests are all generated from the rows. Adding a command is adding
//! a row: nothing in the parser changes.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::fmt;

use serde_json::Value;

use crate::row::{common_option, Command, Field, Type};
use crate::shape;

/// Why a table was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableError(pub String);

impl fmt::Display for TableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TableError {}

/// A shorthand: the words a command line may start with in place of a command's
/// (`who` for `directory list`). The table rewrites them before it reads a line, so a shorthand
/// is the same command for a call, its `--help` and `--schema`, and every reader of the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Alias {
    /// The words written, such as `["who"]` or `["task", "note"]`.
    pub words: &'static [&'static str],
    /// The words they stand for: a resource and a verb, or a resource alone.
    pub command: &'static [&'static str],
}

/// The commands that exist. A command that is not in the table does not exist.
#[derive(Debug, Clone)]
pub struct CommandTable {
    rows: Vec<&'static Command>,
    aliases: &'static [Alias],
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
        Ok(Self { rows, aliases: &[] })
    }

    /// The same table without the checks, for an executable's start: a common command does
    /// nothing its call does not need (design 2.11), and reading every row's example is such
    /// work. The contract tests build the same rows with [`CommandTable::new`], which is what
    /// makes skipping the checks here safe.
    pub fn unchecked(domains: &[&'static [Command]]) -> Self {
        let mut rows: Vec<&'static Command> = domains.iter().flat_map(|rows| rows.iter()).collect();
        rows.sort_by_key(|row| (row.resource, row.verb));
        Self { rows, aliases: &[] }
    }

    /// The same table with `aliases` read as the commands they stand for ([`Self::expand`]).
    pub fn with_aliases(mut self, aliases: &'static [Alias]) -> Self {
        self.aliases = aliases;
        self
    }

    /// The shorthands of the table.
    pub fn aliases(&self) -> &'static [Alias] {
        self.aliases
    }

    /// `args` with a leading shorthand replaced by the words it stands for; any other line as
    /// it is.
    pub fn expand<'a>(&self, args: &'a [String]) -> Cow<'a, [String]> {
        let found = self.aliases.iter().find(|alias| {
            args.len() >= alias.words.len()
                && alias.words.iter().zip(args).all(|(word, arg)| arg == word)
        });
        let Some(alias) = found else {
            return Cow::Borrowed(args);
        };
        let rest = &args[alias.words.len()..];
        let mut out = Vec::with_capacity(alias.command.len() + rest.len());
        out.extend(alias.command.iter().map(|word| (*word).to_owned()));
        out.extend_from_slice(rest);
        Cow::Owned(out)
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

    /// The row a command line runs, read as the parser reads it: a leading shorthand expanded,
    /// then the first two words as resource and verb. `None` when they name no command.
    pub fn command_for(&self, args: &[String]) -> Option<&'static Command> {
        match self.expand(args).as_ref() {
            [resource, verb, ..] => self.find(resource, verb),
            _ => None,
        }
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
