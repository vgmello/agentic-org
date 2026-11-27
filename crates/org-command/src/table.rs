//! The table of commands. A command is one row; the parser, `--help`, `--schema`, the launch
//! catalogue and the contract tests are all generated from the rows. Adding a command is adding
//! a row: nothing in the parser changes.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::fmt;

use serde_json::Value;

use crate::row::{common_option, Command, Field, MutationPolicy, Type};
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

/// An explicit [`MutationPolicy`] override for one `(resource, verb)` row in a [`CommandTable`],
/// refining the row's default policy ([`Command::mutation_policy`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyOverride {
    pub resource: &'static str,
    pub verb: &'static str,
    pub policy: MutationPolicy,
}

/// The commands that exist. A command that is not in the table does not exist.
#[derive(Debug, Clone)]
pub struct CommandTable {
    rows: Vec<&'static Command>,
    aliases: &'static [Alias],
    policies: &'static [PolicyOverride],
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
        Ok(Self {
            rows,
            aliases: &[],
            policies: &[],
        })
    }

    /// The same table without the checks, for an executable's start: a common command does
    /// nothing its call does not need (design 2.11), and reading every row's example is such
    /// work. The contract tests build the same rows with [`CommandTable::new`], which is what
    /// makes skipping the checks here safe.
    pub fn unchecked(domains: &[&'static [Command]]) -> Self {
        let mut rows: Vec<&'static Command> = domains.iter().flat_map(|rows| rows.iter()).collect();
        rows.sort_by_key(|row| (row.resource, row.verb));
        Self {
            rows,
            aliases: &[],
            policies: &[],
        }
    }

    /// The same table with `aliases` read as the commands they stand for ([`Self::expand`]).
    pub fn with_aliases(mut self, aliases: &'static [Alias]) -> Self {
        self.aliases = aliases;
        self
    }

    /// The same table with explicit [`PolicyOverride`] entries for commands whose
    /// [`MutationPolicy`] differs from the default (`ReadOnly` for reads/lists,
    /// `ConfirmRequired` for writes).
    pub fn with_policies(mut self, policies: &'static [PolicyOverride]) -> Self {
        self.policies = policies;
        self
    }

    /// The explicit mutation policy overrides registered on this table.
    pub fn policies(&self) -> &'static [PolicyOverride] {
        self.policies
    }

    /// The effective [`MutationPolicy`] of `command`: explicit table override if present, else
    /// [`Command::mutation_policy`].
    pub fn mutation_policy(&self, command: &Command) -> MutationPolicy {
        self.policies
            .iter()
            .find(|o| o.resource == command.resource && o.verb == command.verb)
            .map_or_else(|| command.mutation_policy(), |o| o.policy)
    }

    /// The effective [`MutationPolicy`] of `(resource, verb)`, or `None` when not in the table.
    pub fn mutation_policy_by_name(&self, resource: &str, verb: &str) -> Option<MutationPolicy> {
        self.find(resource, verb)
            .map(|row| self.mutation_policy(row))
    }

    /// The effective [`MutationPolicy`] for all rows backed by wire RPC `call`, picking the
    /// strictest policy when multiple commands share a wire call (fail closed). `None` when no
    /// row in the table declares `call`.
    pub fn mutation_policy_for_call(&self, call: &str) -> Option<MutationPolicy> {
        self.rows
            .iter()
            .copied()
            .filter(|row| row.call == call)
            .map(|row| self.mutation_policy(row))
            .max_by_key(|policy| policy.strictness())
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

#[cfg(test)]
mod tests {
    use org_proto::ErrorCode;
    use serde_json::json;

    use super::*;
    use crate::row::{Ack, Example, IfMatch, Kind};

    const IN: &[Field] = &[Field::required("name", Type::String, "Name")];
    const OUT: &[Field] = &[Field::required("ok", Type::Boolean, "Done")];
    const ERRS: &[ErrorCode] = &[ErrorCode::InvalidRequest];

    const ROWS: &[Command] = &[
        Command {
            resource: "item",
            verb: "list",
            summary: "List items",
            call: "item.list",
            kind: Kind::List,
            input: &[],
            content_field: None,
            output: OUT,
            errors: ERRS,
            example: Example {
                input: "{}",
                output: r#"[{"ok":true}]"#,
            },
        },
        Command {
            resource: "item",
            verb: "create",
            summary: "Create item",
            call: "item.write",
            kind: Kind::Write {
                ack: Ack::Durable,
                if_match: IfMatch::No,
            },
            input: IN,
            content_field: None,
            output: OUT,
            errors: ERRS,
            example: Example {
                input: r#"{"name":"a"}"#,
                output: r#"{"ok":true}"#,
            },
        },
        Command {
            resource: "item",
            verb: "set",
            summary: "Set item",
            call: "item.write",
            kind: Kind::Write {
                ack: Ack::Committed,
                if_match: IfMatch::No,
            },
            input: IN,
            content_field: None,
            output: OUT,
            errors: ERRS,
            example: Example {
                input: r#"{"name":"a"}"#,
                output: r#"{"ok":true}"#,
            },
        },
    ];

    const OVERRIDES: &[PolicyOverride] = &[PolicyOverride {
        resource: "item",
        verb: "set",
        policy: MutationPolicy::AutoSafe,
    }];

    #[test]
    fn mutation_policy_defaults_overrides_and_strictest_call_merge() {
        let table = CommandTable::new(&[ROWS])
            .expect("valid table")
            .with_policies(OVERRIDES);
        assert_eq!(
            table.mutation_policy_by_name("item", "list"),
            Some(MutationPolicy::ReadOnly)
        );
        assert_eq!(
            table.mutation_policy_by_name("item", "create"),
            Some(MutationPolicy::ConfirmRequired)
        );
        assert_eq!(
            table.mutation_policy_by_name("item", "set"),
            Some(MutationPolicy::AutoSafe)
        );
        // Two commands share `item.write` (one AutoSafe, one ConfirmRequired): strictest wins.
        assert_eq!(
            table.mutation_policy_for_call("item.write"),
            Some(MutationPolicy::ConfirmRequired)
        );
        assert_eq!(
            table.mutation_policy_for_call("item.list"),
            Some(MutationPolicy::ReadOnly)
        );
        assert_eq!(table.mutation_policy_for_call("unknown.call"), None);
        assert_eq!(table.policies().len(), 1);
    }

    #[test]
    fn validate_input_enforces_required_and_unknown_fields() {
        let table = CommandTable::new(&[ROWS]).expect("valid table");
        let cmd = table.find("item", "create").unwrap();
        let good = json!({"name": "alpha"}).as_object().cloned().unwrap();
        assert!(crate::validate_input(cmd, &good).is_ok());

        let missing = serde_json::Map::new();
        assert!(crate::validate_input(cmd, &missing).is_err());

        let extra = json!({"name": "alpha", "bogus": 1})
            .as_object()
            .cloned()
            .unwrap();
        assert!(crate::validate_input(cmd, &extra).is_err());
    }
}
