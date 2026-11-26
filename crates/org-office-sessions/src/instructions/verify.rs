//! The flag-level drift guard of every text that cites `agent-os` command lines: standing
//! instructions, built-in guides and generated hints (`INSTR-5`, `INSTR-7`).
//!
//! A citation is found in an inline code span (`` `agent-os task inspect --task <task>` ``),
//! on an `agent-os` line of a fenced code block, or as a `session attach` JSON line of a fenced
//! code block. Each is read as the shell would split it, and then:
//!
//! - an inline span of just a command or resource (`` `agent-os msg ask` ``) is a reference:
//!   it must exist;
//! - anything else is an invocation, dry-parsed against the live table with
//!   [`org_command::check`]: every flag must exist, every required input and every required
//!   option such as `--if-match` must be given, and every literal value must fit its type.
//!   A placeholder value (`<task>`, `'<json>'`, `@<handle>`) becomes a harmless value of the
//!   flag's type first.
//!
//! A citation whose resource is itself a placeholder (`agent-os <resource> <verb>`) describes
//! the grammar and is skipped.

use std::io;

use org_command::{Command, CommandTable, Sources, Type};
use serde_json::Value;

/// The program name cited in text.
const PROGRAM: &str = "agent-os";

/// Commands `agent-os` runs itself, before the table: `(resource, verb)`.
const OUTSIDE_TABLE: &[(&str, &str)] = &[("session", "attach")];

/// Common options that take a value, and the harmless value a placeholder becomes.
const VALUE_OPTIONS: &[(&str, &str)] = &[
    ("format", "json"),
    ("fields", ""),
    ("limit", "1"),
    ("cursor", "drift-guard-cursor"),
    ("json", "{}"),
    ("input-file", "drift-guard-input.md"),
    ("input-format", "text"),
    ("idempotency-key", "drift-guard-key"),
    ("if-match", "1"),
];

/// One `agent-os` command line cited in text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CitedLine {
    /// The citation as written, for messages.
    pub cited: String,
    /// The words after `agent-os`, split and unquoted as the shell would.
    pub args: Vec<String>,
    /// `false` for an inline span with no flag: a reference to a command, which only has to
    /// exist unless more words follow it.
    pub invocation: bool,
}

/// Every `agent-os` command line cited in `text`: the fenced code block lines in order, then
/// the inline code spans in order.
pub fn cited_command_lines(text: &str) -> Vec<CitedLine> {
    let mut cited = Vec::new();
    let mut prose = String::new();
    let mut in_block = false;
    let mut pending = String::new();
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_block = !in_block;
            pending.clear();
            prose.push('\n');
            continue;
        }
        if !in_block {
            prose.push_str(line);
            prose.push('\n');
            continue;
        }
        let trimmed = line.trim();
        if let Some(continued) = trimmed.strip_suffix('\\') {
            pending.push_str(continued);
            pending.push(' ');
            continue;
        }
        let whole = format!("{pending}{trimmed}");
        pending.clear();
        cite_block_line(&whole, &mut cited);
    }
    cite_inline_spans(&prose, &mut cited);
    cited
}

fn cite_block_line(line: &str, cited: &mut Vec<CitedLine>) {
    let line = line.strip_prefix("$ ").unwrap_or(line).trim();
    if line.starts_with('{') {
        if let Some(args) = attach_line_args(line) {
            cited.push(CitedLine {
                cited: line.to_owned(),
                args,
                invocation: true,
            });
        }
        return;
    }
    if let Some(args) = program_args(line) {
        cited.push(CitedLine {
            cited: line.to_owned(),
            args,
            invocation: true,
        });
    }
}

fn cite_inline_spans(prose: &str, cited: &mut Vec<CitedLine>) {
    let mut rest = prose;
    while let Some(open) = rest.find('`') {
        let ticks = rest[open..].bytes().take_while(|&b| b == b'`').count();
        let fence = &rest[open..open + ticks];
        let after = &rest[open + ticks..];
        let Some(close) = after.find(fence) else {
            break;
        };
        let span = after[..close].trim();
        if let Some(args) = program_args(span) {
            let invocation = args.iter().any(|a| a.starts_with('-'));
            cited.push(CitedLine {
                cited: span.to_owned(),
                args,
                invocation,
            });
        }
        rest = &after[close + ticks..];
    }
}

/// The words after `agent-os` when `line` runs it, split as the shell would.
fn program_args(line: &str) -> Option<Vec<String>> {
    let rest = line.strip_prefix(PROGRAM)?;
    if !(rest.is_empty() || rest.starts_with(char::is_whitespace)) {
        return None;
    }
    Some(shell_words(rest))
}

/// Splits `text` into words as a POSIX shell does for one simple command: quotes group and are
/// removed, and the command ends at a pipe, list or redirection operator or a comment. A word
/// like `<task>` is a placeholder, not a redirection: only a bare `<`, `<<` or `<<<` redirects.
fn shell_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut chars = text.chars().peekable();
    loop {
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        if chars.peek().is_none_or(|&c| c == '#') {
            break;
        }
        let mut word = String::new();
        let mut quoted = false;
        while let Some(c) = chars.next_if(|c| !c.is_whitespace()) {
            match c {
                '\'' => {
                    quoted = true;
                    word.extend(chars.by_ref().take_while(|&q| q != '\''));
                }
                '"' => {
                    quoted = true;
                    while let Some(q) = chars.next() {
                        match q {
                            '"' => break,
                            '\\' => word.extend(chars.next()),
                            other => word.push(other),
                        }
                    }
                }
                other => word.push(other),
            }
        }
        if !quoted && is_shell_operator(&word) {
            break;
        }
        words.push(word);
    }
    words
}

/// Whether an unquoted `word` ends the simple command.
fn is_shell_operator(word: &str) -> bool {
    matches!(word, "<" | "<<" | "<<<")
        || word.starts_with(['|', '&', ';', '>'])
        || word.starts_with("2>")
        || (word.starts_with("<<") && !word.ends_with('>'))
}

/// The arguments `agent-os session attach` builds for one JSON line (`org-agent-cli`'s
/// `attach`): the command's words, `input` as `--json`, and `options` as flags.
fn attach_line_args(line: &str) -> Option<Vec<String>> {
    let Value::Object(object) = serde_json::from_str::<Value>(line).ok()? else {
        return None;
    };
    let command = object.get("command")?.as_str()?;
    let mut args: Vec<String> = command.split_whitespace().map(str::to_owned).collect();
    if let Some(input) = object.get("input").filter(|v| !v.is_null()) {
        args.push("--json".to_owned());
        args.push(input.to_string());
    }
    if let Some(Value::Object(options)) = object.get("options") {
        for (key, value) in options {
            let flag = format!("--{}", key.trim_start_matches("--").replace('_', "-"));
            match value {
                Value::Bool(true) => args.push(flag),
                Value::Bool(false) | Value::Null => {}
                Value::String(text) => args.extend([flag, text.clone()]),
                Value::Number(n) => args.extend([flag, n.to_string()]),
                other => args.extend([flag, other.to_string()]),
            }
        }
    }
    Some(args)
}

/// `who` and `task note` as the command line rewrites them (`org-agent-cli`).
fn expand_aliases(args: &[String]) -> Vec<String> {
    match args {
        [who, rest @ ..] if who == "who" => ["directory".to_owned(), "list".to_owned()]
            .into_iter()
            .chain(rest.iter().cloned())
            .collect(),
        [task, note, rest @ ..] if task == "task" && note == "note" => {
            std::iter::once("task-note".to_owned())
                .chain(rest.iter().cloned())
                .collect()
        }
        _ => args.to_vec(),
    }
}

fn is_placeholder(word: &str) -> bool {
    word.find('<')
        .is_some_and(|open| word[open..].contains('>'))
}

/// A harmless value of type `ty` for a placeholder.
fn value_of_type(ty: &Type) -> String {
    match ty {
        Type::Integer => "1".to_owned(),
        Type::Enum(words) => words.first().copied().unwrap_or("x").to_owned(),
        Type::List(inner) => value_of_type(inner),
        _ => "drift-guard-value".to_owned(),
    }
}

/// `args` with every placeholder value replaced by a harmless value of its flag's type.
fn fill_placeholders(command: &Command, args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    let mut at = 0;
    while at < args.len() {
        let word = &args[at];
        at += 1;
        let Some(flag) = word.strip_prefix("--") else {
            out.push(word.clone());
            continue;
        };
        let (name, inline) = match flag.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (flag, None),
        };
        let filler = VALUE_OPTIONS
            .iter()
            .find(|(option, _)| *option == name)
            .map(|(option, value)| match *option {
                "fields" => command.output.first().map_or("id", |f| f.name).to_owned(),
                _ => (*value).to_owned(),
            })
            .or_else(|| {
                command
                    .input
                    .iter()
                    .find(|f| f.flag() == name && f.is_flag() && f.ty != Type::Boolean)
                    .map(|f| value_of_type(&f.ty))
            });
        match (filler, inline) {
            (Some(filler), Some(value)) if is_placeholder(value) => {
                out.push(format!("--{name}={filler}"));
            }
            (Some(filler), None) => {
                out.push(word.clone());
                if let Some(value) = args.get(at) {
                    at += 1;
                    out.push(if is_placeholder(value) {
                        filler
                    } else {
                        value.clone()
                    });
                }
            }
            _ => out.push(word.clone()),
        }
    }
    out
}

/// Answers every `--input-file` and `--stdin` with a few bytes of text; reads nothing.
struct PlaceholderSources;

impl Sources for PlaceholderSources {
    fn file(&mut self, _path: &str, _max_bytes: usize) -> io::Result<Vec<u8>> {
        Ok(b"drift guard text".to_vec())
    }

    fn stdin(&mut self, _max_bytes: usize) -> io::Result<Vec<u8>> {
        Ok(b"drift guard text".to_vec())
    }
}

/// Why `line` does not hold against `table`, or `None` when it does.
fn check_line(line: &CitedLine, table: &CommandTable) -> Option<String> {
    let args = expand_aliases(&line.args);
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        [] | ["--version" | "-V" | "--help" | "-h"] => return None,
        [resource, ..] if is_placeholder(resource) => return None,
        [resource] | [resource, _, ..] if table.verbs(resource).is_empty() => {
            return Some(format!("`{resource}` is not a resource"));
        }
        [_] => return None,
        [_, verb, ..] if is_placeholder(verb) => return None,
        [resource, verb] | [resource, verb, "--help" | "-h"]
            if OUTSIDE_TABLE.contains(&(*resource, *verb)) =>
        {
            return None
        }
        [resource, verb, ..] if OUTSIDE_TABLE.contains(&(*resource, *verb)) => {
            return Some(format!("`{resource} {verb}` takes no flags"));
        }
        [resource, verb, ..] if !verb.starts_with('-') && table.find(resource, verb).is_none() => {
            return Some(format!("`{resource} {verb}` is not a command"));
        }
        _ => {}
    }
    if !line.invocation && args.len() <= 2 {
        return None;
    }
    let filled = match table.find(&args[0], args.get(1).map_or("", String::as_str)) {
        Some(command) => fill_placeholders(command, &args),
        None => args,
    };
    org_command::check(table, PROGRAM, &filled, &mut PlaceholderSources)
        .err()
        .map(|refusal| refusal.message)
}

/// Checks every `agent-os` command line cited in `text` against the live `table`. Returns one
/// problem per citation that does not hold, as `` `<citation>`: <why> ``; empty when all do.
pub fn verify_command_references(text: &str, table: &CommandTable) -> Vec<String> {
    let mut problems = Vec::new();
    for line in cited_command_lines(text) {
        if let Some(why) = check_line(&line, table) {
            let problem = format!("`{}`: {why}", line.cited);
            if !problems.contains(&problem) {
                problems.push(problem);
            }
        }
    }
    problems
}
