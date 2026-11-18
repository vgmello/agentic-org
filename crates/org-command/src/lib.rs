//! The command contract of `agent-os` and `agentic-org`
//! (`docs/design/agent-os-command-line.md`, section 2): one grammar, one answer envelope, one
//! table of commands.
//!
//! - A command is one row ([`Command`]) in a [`CommandTable`]. A domain declares its rows as a
//!   `static` slice; the composition lists the slices. Adding a command touches no parser code.
//! - [`run`] reads a command line against the table, sends the command to the [`Office`] and
//!   returns the one envelope to print and the exit code. It reads standard input and files only
//!   through [`Sources`], and only when `--stdin` or `--input-file` is given.
//! - The office authorizes and decides. Nothing checked here protects anything: the checks here
//!   only spare a call that could not succeed.
//!
//! The library knows no socket, database or operating system. The executable supplies the
//! office (how the caller is identified) and the sources.

#![deny(unsafe_code)]

mod envelope;
mod help;
mod node;
mod office;
mod parse;
mod shape;
mod table;
#[cfg(feature = "testkit")]
pub mod testkit;

pub use envelope::{code_name, envelope_schema};
pub use help::CATALOGUE_LIMIT;
pub use node::Node;
pub use office::{Answer, Office, Refusal, Request, Sources, Status};
pub use parse::{yaml_object, DEFAULT_LIMIT, MAX_LIMIT};
pub use table::{Ack, Command, CommandTable, Example, Field, IfMatch, Kind, TableError, Type};

use org_proto::ErrorCode;
use parse::{Format, Parsed};

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

/// Runs one command line. `program` is the executable's name, for help and hints; `args` are
/// the arguments after it. The command line is read first: one that cannot be read is answered
/// with `invalid_request` and exit code 2, and the office is not called.
pub fn run(
    table: &CommandTable,
    program: &str,
    args: &[String],
    sources: &mut dyn Sources,
    office: &mut dyn Office,
) -> Output {
    let invocation = match parse::parse(table, program, args, sources) {
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
    let request = &invocation.request;
    match office.call(request) {
        Ok(answer) => {
            let mut data = shape::order_data(request.command, &answer.data);
            // The columns of a compact list: the chosen fields, or every declared one, in the
            // record's own order.
            let mut columns: Vec<&str> = request
                .command
                .output
                .iter()
                .map(|field| field.name)
                .collect();
            if let Some(paths) = &invocation.fields {
                let paths: Vec<&[String]> = paths.iter().map(Vec::as_slice).collect();
                data = shape::project(data, &paths);
                columns.retain(|column| paths.iter().any(|path| path[0] == *column));
            }
            let stdout = match invocation.format {
                Format::Compact => data.to_compact(&columns),
                format => print(&envelope::success(data, &answer, request.dry_run), format),
            };
            Output {
                stdout,
                exit_code: 0,
            }
        }
        // A failure prints the JSON envelope in every format.
        Err(refusal) => Output {
            stdout: print(&envelope::failure(&refusal, request.dry_run), Format::Json),
            exit_code: 1,
        },
    }
}
