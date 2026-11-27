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
//! is sent for it. Flags are read only from `options`, one bare name per key with its value passed
//! inline, so no flag can reach the table past those checks (`command` with a flag, a key with
//! `=`, or a value that looks like a flag). Only a yes-or-no flag of the command is written bare
//! for `true`: a flag that takes a value would read the next flag as that value.

use std::io::{self, BufRead, Write};

use org_command::{Command, CommandTable, KeySource, Office, Output, Refusal, Sources};
use org_proto::ErrorCode;
use serde_json::{Map, Value};

use crate::runtime::keys::UuidV7KeySource;
use crate::runtime::sources::StdSources;

/// Help text for `agent-os session attach`.
pub const SESSION_ATTACH_HELP: &str = "Usage: agent-os session attach\n\
Use when: batching multiple agent-os commands in one shell turn over a persistent office socket.\n\
Binds once to the office socket and executes one JSON command per line of stdin:\n  \
{\"command\":\"msg inbox\",\"input\":{},\"options\":{\"limit\":5}}\n\
A line has only `command`, `input` and `options`; every answer is one line of JSON, so the \
`format` option is json or absent, and `stdin` and `help` are refused.\n";

/// The hint of a line `session attach` could not read.
const LINE_HINT: &str =
    "Send one JSON object per line with `command`, optional `input`, and optional `options`.";

/// The keys a line may have.
const LINE_KEYS: [&str; 3] = ["command", "input", "options"];

/// Where a line's `--input-file` is read from. Standard input is the stream of lines itself, so
/// it is never read as a command's content: a line that asks for it is refused before it runs,
/// and this refuses it again should one get through.
#[derive(Debug, Default)]
pub(crate) struct AttachSources(StdSources);

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

/// Runs the `session attach` line loop of `table` over arbitrary `reader`, `writer`, and
/// `office`. Answers 0 when the input ends or the reader of `writer` goes away (a closed pipe),
/// and 1 when `writer` fails otherwise.
#[cfg(test)]
pub(crate) fn run_attach_loop<R: BufRead, W: Write>(
    table: &CommandTable,
    program: &str,
    reader: R,
    writer: W,
    office: &mut dyn Office,
) -> u8 {
    run_attach_loop_with(table, program, reader, writer, office, |p, a, s, k, o| {
        org_command::run(table, p, a, s, k, o)
    })
}

/// Runs the `session attach` line loop using `run_cmd` for each parsed command so caller-level
/// wrappers (such as `registry::run_command` offline skill formatting) apply identically in attach
/// mode without violating VSA slice isolation.
pub(crate) fn run_attach_loop_with<R, W, F>(
    table: &CommandTable,
    program: &str,
    reader: R,
    mut writer: W,
    office: &mut dyn Office,
    mut run_cmd: F,
) -> u8
where
    R: BufRead,
    W: Write,
    F: FnMut(&str, &[String], &mut dyn Sources, &mut dyn KeySource, &mut dyn Office) -> Output,
{
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
        let reply = match parse_line_args(table, trimmed) {
            Ok(args) => run_one(
                &mut run_cmd,
                program,
                &args,
                &mut sources,
                &mut keys,
                office,
            ),
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

fn run_one<F>(
    run_cmd: &mut F,
    program: &str,
    args: &[String],
    sources: &mut AttachSources,
    keys: &mut UuidV7KeySource,
    office: &mut dyn Office,
) -> String
where
    F: FnMut(&str, &[String], &mut dyn Sources, &mut dyn KeySource, &mut dyn Office) -> Output,
{
    let out = run_cmd(program, args, sources, keys, office);
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
fn parse_line_args(table: &CommandTable, raw: &str) -> Result<Vec<String>, String> {
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
    if let Some(word) = parts.iter().find(|word| word.starts_with('-')) {
        return Err(command_flag(word));
    }
    let mut args = table.expand(&parts).into_owned();
    let row = table.command_for(&args);
    if let Some(input) = obj.get("input").filter(|v| !v.is_null()) {
        let json = serde_json::to_string(input)
            .map_err(|_| "Could not serialize `input` as JSON.".to_owned())?;
        args.push("--input-json".into());
        args.push(json);
    }
    if let Some(opt_val) = obj.get("options").filter(|v| !v.is_null()) {
        let Some(options) = opt_val.as_object() else {
            return Err("`options` in `session attach` must be a JSON object.".into());
        };
        append_options(&mut args, row, options)?;
    }
    Ok(args)
}

/// An option `session attach` refuses, and why: each would answer something other than one
/// line of JSON, or read the stream of lines as a command's content.
fn refused_option(name: &str, value: &Value) -> Option<String> {
    match (name, value) {
        ("format", Value::String(format)) if format == "json" => None,
        ("format", _) => Some(format!(
            "`session attach` answers every line in JSON, so `format` {value} is refused. Drop `format`, or give \"json\"."
        )),
        ("stdin", _) => Some(
            "`stdin` is refused: standard input carries the `session attach` lines. Give the text in `input`, or a file with `input-file`."
                .to_owned(),
        ),
        ("help", _) => Some(
            "`help` is refused in `session attach`; run `agent-os <resource> <verb> --help` outside it."
                .to_owned(),
        ),
        _ => None,
    }
}

/// The refusal of a flag written into `command`, which names only a resource, a verb or a
/// shorthand: every flag goes in `options`, where the refused ones are checked.
fn command_flag(word: &str) -> String {
    format!(
        "`command` names only a resource and verb (or a shorthand), and `{word}` is a flag. Give flags in `options`, e.g. {{\"options\":{{\"limit\":5}}}}."
    )
}

/// The flag name of an `options` key (`limit`, `--limit`, `max_results` as `max-results`), or
/// the refusal of a key that is not one bare flag name: a key with `=` or a space would carry a
/// value or a second flag past the refused options, and one with another leading `-` (`-h`) is
/// no flag the table reads.
fn option_name(key: &str) -> Result<String, String> {
    let name = match key.strip_prefix("--") {
        Some(name) => name.to_owned(),
        None => key.replace('_', "-"),
    };
    if name.is_empty()
        || name.starts_with('-')
        || name.contains('=')
        || name.chars().any(char::is_whitespace)
    {
        return Err(format!(
            "Option `{key}` is not a flag name: name one flag per key (e.g. `limit` or `--limit`) and give its value as the key's value, never with `=`, spaces or a short `-x` form."
        ));
    }
    Ok(name)
}

/// The bare flag of an option given `true`. Only a yes-or-no flag of the command is written
/// bare: a flag that takes a value would read the flag after it as that value (`--body
/// --dry-run` sends the text `--dry-run`). A line naming no command is refused by the table
/// before it reads any flag, so its switches are left out.
fn switch(row: Option<&Command>, key: &str, name: &str) -> Result<Option<String>, String> {
    let Some(row) = row else {
        return Ok(None);
    };
    match row.flag_takes_value(name) {
        Some(false) => Ok(Some(format!("--{name}"))),
        Some(true) => Err(format!("Option `{key}` takes a value, not `true`.")),
        None => Err(format!("Option `{key}` is not a flag of `{}`.", row.name())),
    }
}

/// Turns `options` into flags for `row`, the command the line runs. A string or number is passed
/// inline (`--name=value`), so a value is only ever read as that flag's value, never as a flag of
/// its own; `true` is a bare flag only for a yes-or-no flag ([`switch`]).
fn append_options(
    args: &mut Vec<String>,
    row: Option<&Command>,
    options: &Map<String, Value>,
) -> Result<(), String> {
    for (key, value) in options {
        let name = option_name(key)?;
        if matches!(value, Value::Bool(false) | Value::Null) {
            continue;
        }
        if let Some(refusal) = refused_option(&name, value) {
            return Err(refusal);
        }
        match value {
            Value::Bool(_) => args.extend(switch(row, key, &name)?),
            Value::String(s) => args.push(format!("--{name}={s}")),
            Value::Number(n) => args.push(format!("--{name}={n}")),
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
pub(crate) fn error_envelope(code: ErrorCode, message: &str) -> String {
    org_command::failure_envelope_json(&Refusal::new(code, message, LINE_HINT), false)
}

/// Whether `session attach` was asked for its help: `--help` (or `-h`) anywhere after it wins
/// over every other word, as it does on every command (`doc-28` §2.1).
pub(crate) fn wants_help(args: &[String]) -> bool {
    args.get(2..)
        .unwrap_or_default()
        .iter()
        .any(|arg| arg == "--help" || arg == "-h")
}

/// The refusal of `session attach` written with anything after it but `--help`: it reads its
/// commands from standard input and takes no flag of its own.
pub(crate) fn usage_refusal(args: &[String]) -> Option<Refusal> {
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
