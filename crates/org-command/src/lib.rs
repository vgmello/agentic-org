//! The command contract of `agent-os` and `agentic-org`
//! (`docs/design/agent-os-command-line.md`, section 2): one grammar, one answer envelope, one
//! table of commands.
//!
//! - A command is one row ([`Command`]) in a [`CommandTable`]. A domain declares its rows as a
//!   `static` slice; the composition lists the slices. Adding a command touches no parser code.
//! - [`run`] reads a command line against the table, sends the command to the [`Office`] and
//!   returns the one envelope to print and the exit code. It reads standard input and files only
//!   through [`Sources`], only when `--stdin` or `--input-file` is given, and never more than
//!   [`MAX_INPUT_BYTES`].
//! - `data` is exactly what the row declares, in every format: a field the office adds is
//!   dropped, and an answer without a required field is a failure.
//! - A write that carries no `--idempotency-key` is given one from the [`KeySource`].
//! - The office authorizes and decides. Nothing checked here protects anything: the checks here
//!   only spare a call that could not succeed.
//!
//! The library knows no socket, database or operating system, and makes no randomness. The
//! executable supplies the office (how the caller is identified), the sources and the keys.

#![forbid(unsafe_code)]

mod envelope;
mod help;
mod node;
mod office;
mod parse;
mod row;
mod shape;
mod table;
#[cfg(feature = "testkit")]
pub mod testkit;

pub use envelope::{code_name, envelope_schema};
pub use help::CATALOGUE_LIMIT;
pub use node::Node;
pub use office::{Answer, KeySource, Office, Refusal, RefusalDetails, Request, Sources, Status};
pub use parse::{DEFAULT_LIMIT, MAX_INPUT_BYTES, MAX_LIMIT, MAX_YAML_INPUT_BYTES};
pub use row::{Ack, Command, Example, Field, IfMatch, Kind, MutationPolicy, Type};
pub use table::{Alias, CommandTable, PolicyOverride, TableError};

/// Validates a JSON input object against a [`Command`]'s declared input schema (required fields,
/// types, enum values, and unknown-field rejection). Used by server-side `suggestion.create`
/// preflight so proposals cannot carry malformed or hallucinated flags.
pub fn validate_input(command: &Command, input: &Map<String, Value>) -> Result<(), String> {
    shape::validate(command.input, input)
}

use org_proto::ErrorCode;
use parse::{Format, Parsed};
use serde_json::{Map, Value};

/// What a command prints on standard output, and the exit code: 0 when `ok` is true, 1 when it
/// is false, 2 when the command line itself could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// Exactly one envelope (or the help text), ending with a new line.
    pub stdout: String,
    pub exit_code: u8,
}

/// The exit code of a command line that could not be read.
pub const EXIT_USAGE: u8 = 2;

fn print(envelope: &Node, format: Format) -> String {
    match format {
        Format::Json | Format::Compact => format!("{}\n", envelope.to_json()),
        Format::Yaml => envelope.to_yaml(),
    }
}

/// A failure prints the JSON envelope in every format; the exit code is 1.
fn failed(refusal: &Refusal, dry_run: bool) -> Output {
    Output {
        stdout: print(&envelope::failure(refusal, dry_run), Format::Json),
        exit_code: 1,
    }
}

/// The failure envelope of `refusal` as one line of JSON, without the new line: `ok` false,
/// `data` null, `meta` (its `operation_id`, and `dry_run` when `dry_run`), and `error` with the
/// refusal's code, message, hint and `retry_with`. It is the envelope [`run`] prints for a
/// refusal, for a caller that is refused before any command line is read (`session attach`
/// when its office cannot be reached), so that caller never writes an envelope by hand.
pub fn failure_envelope_json(refusal: &Refusal, dry_run: bool) -> String {
    envelope::failure(refusal, dry_run).to_one_line()
}

/// The success envelope of `data` as one line of JSON, without the new line: `ok` true, `data`
/// (its object keys in name order), `meta` `{"status":"completed"}` and `error` null. It is the
/// envelope [`run`] prints for an answer with no table row, for a caller that answers outside
/// the table (`agent-os login|whoami|logout`), so that caller never writes an envelope by hand:
/// DEL, C1 controls, the Unicode line and paragraph separators and the bidirectional controls
/// are escaped exactly as in every other envelope (CVE-2021-25743 class).
pub fn success_envelope_json(data: &Value) -> String {
    envelope::local(Node::from_value(data)).to_one_line()
}

/// Reads one command line against `table` exactly as [`run`] does, and stops there: no key is
/// made and no office is called. `Ok` when [`run`] would print help or a schema, or send the
/// call; otherwise the `invalid_request` refusal (exit code 2) [`run`] would print. Text that
/// cites command lines, such as an agent's standing instructions, is checked with it against
/// the live table, so a cited flag that does not exist or a missing required option fails.
pub fn check(
    table: &CommandTable,
    program: &str,
    args: &[String],
    sources: &mut dyn Sources,
) -> Result<(), Refusal> {
    match parse::parse(table, program, args, sources) {
        Ok(_) => Ok(()),
        Err(usage) => Err(Refusal::new(
            ErrorCode::InvalidRequest,
            usage.message,
            usage.hint,
        )),
    }
}

/// Runs one command line. `program` is the executable's name, for help and hints; `args` are
/// the arguments after it. The command line is read first: one that cannot be read is answered
/// with `invalid_request` and exit code 2, and neither the key source nor the office is called.
pub fn run(
    table: &CommandTable,
    program: &str,
    args: &[String],
    sources: &mut dyn Sources,
    keys: &mut dyn KeySource,
    office: &mut dyn Office,
) -> Output {
    let mut invocation = match parse::parse(table, program, args, sources) {
        Ok(Parsed::Help(text)) => {
            return Output {
                stdout: text,
                exit_code: 0,
            }
        }
        Ok(Parsed::Schema(command, format)) => {
            let data = help::schema(command);
            let stdout = match format {
                Format::Compact => data.to_compact(&[]),
                _ => print(&envelope::local(data), format),
            };
            return Output {
                stdout,
                exit_code: 0,
            };
        }
        Ok(Parsed::Call(invocation)) => invocation,
        Err(usage) => {
            let refusal = Refusal::new(ErrorCode::InvalidRequest, usage.message, usage.hint);
            return Output {
                stdout: print(&envelope::failure(&refusal, false), Format::Json),
                exit_code: EXIT_USAGE,
            };
        }
    };
    let command = invocation.request.command;
    // Every change carries a key (design 2.7). A dry run changes nothing and takes none.
    if command.is_write()
        && !invocation.request.dry_run
        && invocation.request.idempotency_key.is_none()
    {
        let key = keys.new_key();
        if !parse::is_key(&key) {
            let refusal = Refusal::new(
                ErrorCode::Unavailable,
                format!("`{program}` could not make an idempotency key for this change."),
                "Nothing was sent. Give a key yourself with --idempotency-key: 1 to 64 letters, digits and hyphens.",
            );
            return failed(&refusal, false);
        }
        invocation.request.idempotency_key = Some(key);
    }
    let request = &invocation.request;
    let key = request.idempotency_key.as_deref();
    let answer = match office.call(request) {
        Ok(answer) => answer,
        Err(mut refusal) => {
            if refusal.operation_id.is_none() {
                refusal.operation_id = key.map(str::to_owned);
            }
            // Repeating the call with the same key is what can succeed (design 2.4). A timeout
            // is the same: the office may have made the change, and only the same key makes a
            // repeat safe.
            if let (ErrorCode::Unavailable | ErrorCode::NotReady | ErrorCode::Timeout, Some(key)) =
                (refusal.code, key)
            {
                refusal
                    .retry_with
                    .get_or_insert_with(Map::new)
                    .insert("idempotency-key".to_owned(), Value::String(key.to_owned()));
            }
            return failed(&refusal, request.dry_run);
        }
    };
    // `data` is exactly what the row declares, or the command fails: nothing the row does not
    // declare is printed in any format, and a caller never gets a record with a hole in it.
    let mut data = match shape::conform(command, &answer.data) {
        Ok(data) => data,
        Err(problem) => {
            let changed = if command.is_write() && !request.dry_run {
                " The change may have been made: read the record before you repeat it."
            } else {
                ""
            };
            let refusal = Refusal {
                operation_id: answer.operation_id.clone().or(key.map(str::to_owned)),
                ..Refusal::new(
                    ErrorCode::Unavailable,
                    format!(
                        "The office's answer to `{}` is not the answer the command declares: {problem}.",
                        command.name()
                    ),
                    format!(
                        "This is a fault between `{program}` and the office, not in what you gave.{changed} `{program} {} --schema` shows the declared answer.",
                        command.name()
                    ),
                )
            };
            return failed(&refusal, request.dry_run);
        }
    };
    // The columns of a compact list: the chosen fields, or every declared one, in the
    // record's own order.
    let mut columns: Vec<&str> = command.output.iter().map(|field| field.name).collect();
    if let Some(paths) = &invocation.fields {
        let paths: Vec<&[String]> = paths.iter().map(Vec::as_slice).collect();
        data = shape::project(data, &paths);
        columns.retain(|column| paths.iter().any(|path| path[0] == *column));
    }
    let meta = envelope::success_meta(&answer, request.dry_run, key);
    let stdout = match invocation.format {
        Format::Compact => {
            let mut out = data.to_compact(&columns);
            // `compact` prints `data` alone, except when the caller cannot act without `meta`:
            // the command did not complete, there is another page, or a list that reports its
            // total printed no line (`task list --count`, or nothing matched).
            let counted_nothing =
                answer.total.is_some() && matches!(&data, Node::List(items) if items.is_empty());
            if answer.status != Status::Completed || answer.next_cursor.is_some() || counted_nothing
            {
                out.push_str(&format!("meta: {}\n", meta.to_one_line()));
            }
            out
        }
        format => print(&envelope::success(data, meta), format),
    };
    Output {
        stdout,
        exit_code: 0,
    }
}

#[cfg(test)]
mod tests;
