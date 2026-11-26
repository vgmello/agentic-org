//! Long-running `agent-os session attach` mode (`doc-28` §2.9, `CLI-13`, `ACK-1`).
//!
//! Binds ONCE on startup, reads one JSON command object per line of stdin, executes each
//! through the shared `org-command` table on the same persistent office connection, and
//! writes one compact single-line JSON envelope per line to stdout. A failing line writes an
//! error envelope and never stops subsequent lines.
//!
//! A line is checked whole before anything runs: only `command`, `input` and `options` are
//! read, the answer is always one line of JSON, and standard input is the stream of lines, so a
//! line that names another key, another `format`, `--stdin` or `--help` is refused and nothing
//! is sent for it.

use std::io::{self, BufRead, Write};

use org_command::{CommandTable, Office, Refusal, Sources};
use org_proto::ErrorCode;
use serde_json::{Map, Value};

use crate::aliases::normalize_args;
use crate::keys::UuidV7KeySource;
use crate::sources::StdSources;
use crate::table;

/// The hint of a line `session attach` could not read.
const LINE_HINT: &str =
    "Send one JSON object per line with `command`, optional `input`, and optional `options`.";

/// The keys a line may have.
const LINE_KEYS: [&str; 3] = ["command", "input", "options"];

/// Where a line's `--input-file` is read from. Standard input is the stream of lines itself, so
/// it is never read as a command's content: a line that asks for it is refused before it runs,
/// and this refuses it again should one get through.
#[derive(Debug, Default)]
pub struct AttachSources(StdSources);

impl Sources for AttachSources {
    fn file(&mut self, path: &str, max_bytes: usize) -> io::Result<Vec<u8>> {
        self.0.file(path, max_bytes)
    }

    fn stdin(&mut self, _max_bytes: usize) -> io::Result<Vec<u8>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "standard input carries the session attach lines",
        ))
    }
}

/// Runs the `session attach` line loop over arbitrary `reader`, `writer`, and `office`.
/// Answers 0 when the input ends or the reader of `writer` goes away (a closed pipe), and 1
/// when `writer` fails otherwise.
pub fn run_attach_loop<R: BufRead, W: Write>(
    program: &str,
    reader: R,
    mut writer: W,
    office: &mut dyn Office,
) -> u8 {
    let table = table::runtime_table();
    let mut sources = AttachSources::default();
    let mut keys = UuidV7KeySource;
    for line in reader.lines() {
        let Ok(raw) = line else {
            break;
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let reply = match parse_line_args(trimmed) {
            Ok(args) => run_one(table, program, &args, &mut sources, &mut keys, office),
            Err(message) => error_envelope(ErrorCode::InvalidRequest, &message),
        };
        let written = writeln!(writer, "{reply}").and_then(|()| writer.flush());
        match written {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::BrokenPipe => return 0,
            Err(_) => return 1,
        }
    }
    0
}

fn run_one(
    table: &CommandTable,
    program: &str,
    args: &[String],
    sources: &mut AttachSources,
    keys: &mut UuidV7KeySource,
    office: &mut dyn Office,
) -> String {
    let out = org_command::run(table, program, args, sources, keys, office);
    let trimmed = out.stdout.trim_end_matches('\n');
    if !trimmed.contains('\n') && serde_json::from_str::<Value>(trimmed).is_ok() {
        trimmed.to_owned()
    } else {
        error_envelope(
            ErrorCode::InvalidRequest,
            "Command produced non-JSON or multi-line output (run `--help` outside `session attach`).",
        )
    }
}

/// The command line of one attach line, refusing what attach cannot answer in one JSON line.
fn parse_line_args(raw: &str) -> Result<Vec<String>, String> {
    let value: Value = serde_json::from_str(raw).map_err(|_| {
        "Each line in `session attach` must be a JSON object with `command`.".to_owned()
    })?;
    let Some(obj) = value.as_object() else {
        return Err("Each line in `session attach` must be a JSON object.".into());
    };
    if let Some(key) = obj.keys().find(|key| !LINE_KEYS.contains(&key.as_str())) {
        return Err(format!(
            "`{key}` is not a key of a `session attach` line; a line has only `command`, `input` and `options`."
        ));
    }
    let Some(command) = obj.get("command").and_then(Value::as_str) else {
        return Err("`command` must be a string naming a resource and verb.".into());
    };
    let parts: Vec<String> = command.split_whitespace().map(str::to_owned).collect();
    if parts.is_empty() {
        return Err("`command` must name a resource and verb (e.g. `msg inbox`).".into());
    }
    let mut args = normalize_args(&parts).into_owned();
    if let Some(input) = obj.get("input").filter(|v| !v.is_null()) {
        let json = serde_json::to_string(input)
            .map_err(|_| "Could not serialize `input` as JSON.".to_owned())?;
        args.push("--json".into());
        args.push(json);
    }
    if let Some(opt_val) = obj.get("options").filter(|v| !v.is_null()) {
        let Some(options) = opt_val.as_object() else {
            return Err("`options` in `session attach` must be a JSON object.".into());
        };
        append_options(&mut args, options)?;
    }
    Ok(args)
}

/// An option `session attach` refuses, and why: each would answer something other than one
/// line of JSON, or read the stream of lines as a command's content.
fn refused_option(flag: &str, value: &Value) -> Option<String> {
    match (flag, value) {
        ("--format", Value::String(format)) if format == "json" => None,
        ("--format", _) => Some(format!(
            "`session attach` answers every line in JSON, so `format` {value} is refused. Drop `format`, or give \"json\"."
        )),
        ("--stdin", _) => Some(
            "`stdin` is refused: standard input carries the `session attach` lines. Give the text in `input`, or a file with `input-file`."
                .to_owned(),
        ),
        ("--help" | "-h", _) => Some(
            "`help` is refused in `session attach`; run `agent-os <resource> <verb> --help` outside it."
                .to_owned(),
        ),
        _ => None,
    }
}

fn append_options(args: &mut Vec<String>, options: &Map<String, Value>) -> Result<(), String> {
    for (key, value) in options {
        let flag = if key.starts_with("--") {
            key.clone()
        } else {
            format!("--{}", key.replace('_', "-"))
        };
        if matches!(value, Value::Bool(false) | Value::Null) {
            continue;
        }
        if let Some(refusal) = refused_option(&flag, value) {
            return Err(refusal);
        }
        match value {
            Value::Bool(_) => args.push(flag),
            Value::String(s) => {
                args.push(flag);
                args.push(s.clone());
            }
            Value::Number(n) => {
                args.push(flag);
                args.push(n.to_string());
            }
            _ => {
                return Err(format!(
                    "Option `{key}` in `session attach` must be a string, number, or boolean."
                ));
            }
        }
    }
    Ok(())
}

/// The single-line `org-command` failure envelope of a line `session attach` could not run:
/// the same envelope, key order and escaping as every other refusal (`data` null, `meta` {}),
/// with the line hint.
pub fn error_envelope(code: ErrorCode, message: &str) -> String {
    org_command::failure_envelope_json(&Refusal::new(code, message, LINE_HINT), false)
}

/// The refusal of `session attach` written with anything after it but `--help`: it reads its
/// commands from standard input and takes no flag of its own.
pub fn usage_refusal(args: &[String]) -> Option<Refusal> {
    let extra = args.get(2..).unwrap_or_default();
    if extra.is_empty() {
        return None;
    }
    let named = extra.join(" ");
    Some(Refusal::new(
        ErrorCode::InvalidRequest,
        format!("`session attach` takes no flags, and `{named}` was given."),
        "Run `agent-os session attach` alone and send one JSON line per command on standard input; `agent-os session attach --help` shows the line format.",
    ))
}

#[cfg(test)]
mod tests;
