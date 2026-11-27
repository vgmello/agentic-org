//! The flag-level drift guard of every text that cites `agent-os` command lines: standing
//! instructions, built-in guides and generated hints (`INSTR-5`, `INSTR-7`).
//!
//! A citation is found in an inline code span (`` `agent-os task inspect --task <task>` ``),
//! on an `agent-os` line of a fenced code block, or as a `session attach` JSON line of a fenced
//! code block. A line is split into the simple commands of its pipes and lists (`|`, `&&`,
//! `||`, `;`, `&`), so `agent-os` after any of them is cited too. Each is read as the shell
//! would split it, and then:
//!
//! - an inline span of just a command or resource (`` `agent-os msg ask` ``) is a reference:
//!   it must exist;
//! - anything else is an invocation, dry-parsed against the live table with
//!   [`org_command::check`]: every flag must exist, every required input and every required
//!   option such as `--if-match` must be given, and every literal value must fit its type.
//!   A placeholder value (`<task>`, `@<handle>`) becomes a harmless value of the flag's type
//!   first. In `--input-json` (and a `session attach` line's `input`) only placeholder leaves are
//!   filled, each with a value of the type its field declares; every key and the structure are
//!   kept, so an unknown field or a literal of the wrong type still fails. Only an `--input-json`
//!   that is a bare placeholder (`'<json>'`) becomes `{}`.
//! - a `session attach` line is first held to the rules `agent-os session attach` applies before
//!   the table (`command` is words, `options` is an object of strings, numbers and booleans).
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
    /// Why `agent-os` itself refuses the line before the table reads it (a `session attach`
    /// line it cannot turn into arguments), if it does.
    pub refused: Option<String>,
}

impl CitedLine {
    fn invocation(cited: &str, args: Vec<String>) -> Self {
        Self {
            cited: cited.to_owned(),
            args,
            invocation: true,
            refused: None,
        }
    }
}

/// Every `agent-os` command line cited in `text`: the fenced code block lines in order, then
/// the inline code spans in order. `table` is the one `session attach` reads its lines against.
pub fn cited_command_lines(text: &str, table: &CommandTable) -> Vec<CitedLine> {
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
        cite_block_line(&whole, table, &mut cited);
    }
    cite_inline_spans(&prose, &mut cited);
    cited
}

fn cite_block_line(line: &str, table: &CommandTable, cited: &mut Vec<CitedLine>) {
    let line = line.strip_prefix("$ ").unwrap_or(line).trim();
    if line.starts_with('{') {
        match attach_line_args(line, table) {
            Some(Ok(args)) => cited.push(CitedLine::invocation(line, args)),
            Some(Err(why)) => cited.push(CitedLine {
                refused: Some(why),
                ..CitedLine::invocation(line, Vec::new())
            }),
            None => {}
        }
        return;
    }
    for segment in shell_segments(line) {
        if let Some(args) = program_args(segment) {
            cited.push(CitedLine::invocation(segment, args));
        }
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
        for segment in shell_segments(after[..close].trim()) {
            if let Some(args) = program_args(segment) {
                let invocation = args.iter().any(|a| a.starts_with('-'));
                cited.push(CitedLine {
                    invocation,
                    ..CitedLine::invocation(segment, args)
                });
            }
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

/// The simple commands of `line`'s pipelines and lists, trimmed: it is split at every unquoted
/// `|`, `||`, `&&`, `;` and `&`, and ends at an unquoted `#` that starts a word (a comment).
fn shell_segments(line: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut start = 0;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut word_start = true;
    let mut end = line.len();
    for (at, c) in line.char_indices() {
        match (quote, c) {
            (_, _) if escaped => escaped = false,
            (Some('"'), '\\') | (None, '\\') => escaped = true,
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"') => quote = Some(c),
            (None, '#') if word_start => {
                end = at;
                break;
            }
            (None, '|' | '&' | ';') => {
                segments.push(&line[start..at]);
                start = at + c.len_utf8();
            }
            _ => {}
        }
        word_start = quote.is_none() && (c.is_whitespace() || matches!(c, '|' | '&' | ';'));
    }
    segments.push(&line[start..end.max(start)]);
    segments
        .into_iter()
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect()
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

/// The arguments `agent-os session attach` builds for one JSON line, by the same rules as
/// `org-agent-cli`'s `attach`: the command's words, `input` as `--input-json`, and `options` as flags
/// with string and number values inline (`--name=value`) and `true` bare for a yes-or-no flag of
/// the command only; a key other than `command`, `input` and `options`, a flag in `command`, an
/// option key with `=`, a space or another leading `-`, `true` for a flag that takes a value or
/// is no flag of the command, a `format` other than json, `stdin` and `help` are refused there
/// before the table reads the line, so here too. `None` for a JSON line without `command` (an
/// example, not an attach line); `Err` with attach's own message for a line it refuses before
/// the table.
fn attach_line_args(line: &str, table: &CommandTable) -> Option<Result<Vec<String>, String>> {
    let Value::Object(object) = serde_json::from_str::<Value>(line).ok()? else {
        return None;
    };
    let command = object.get("command")?;
    if let Some(key) = object
        .keys()
        .find(|key| !["command", "input", "options"].contains(&key.as_str()))
    {
        return Some(Err(format!(
            "`{key}` is not a key of a `session attach` line; a line has only `command`, `input` and `options`."
        )));
    }
    let Some(command) = command.as_str() else {
        return Some(Err(
            "`command` must be a string naming a resource and verb.".into(),
        ));
    };
    let mut args: Vec<String> = command.split_whitespace().map(str::to_owned).collect();
    if args.is_empty() {
        return Some(Err(
            "`command` must name a resource and verb (e.g. `msg inbox`).".into(),
        ));
    }
    if let Some(word) = args.iter().find(|word| word.starts_with('-')) {
        return Some(Err(format!(
            "`command` names only a resource and verb (or a shorthand), and `{word}` is a flag"
        )));
    }
    let row = table.command_for(&args);
    if let Some(input) = object.get("input").filter(|v| !v.is_null()) {
        args.push("--input-json".to_owned());
        args.push(input.to_string());
    }
    match object.get("options").filter(|v| !v.is_null()) {
        None => {}
        Some(Value::Object(options)) => {
            for (key, value) in options {
                let name = match key.strip_prefix("--") {
                    Some(name) => name.to_owned(),
                    None => key.replace('_', "-"),
                };
                if name.is_empty()
                    || name.starts_with('-')
                    || name.contains('=')
                    || name.chars().any(char::is_whitespace)
                {
                    return Some(Err(format!("Option `{key}` is not a flag name")));
                }
                let refused = match (name.as_str(), value) {
                    (_, Value::Bool(false) | Value::Null) => None,
                    ("format", Value::String(format)) if format == "json" => None,
                    ("format", _) => Some("`session attach` answers every line in JSON"),
                    ("stdin", _) => Some(
                        "`stdin` is refused: standard input carries the `session attach` lines",
                    ),
                    ("help", _) => Some("`help` is refused in `session attach`"),
                    _ => None,
                };
                if let Some(why) = refused {
                    return Some(Err(why.to_owned()));
                }
                match value {
                    Value::Bool(true) => match attach_switch(row, key, &name) {
                        Ok(flag) => args.extend(flag),
                        Err(why) => return Some(Err(why)),
                    },
                    Value::Bool(false) | Value::Null => {}
                    Value::String(text) => args.push(format!("--{name}={text}")),
                    Value::Number(n) => args.push(format!("--{name}={n}")),
                    Value::Array(_) | Value::Object(_) => {
                        let why = "in `session attach` must be a string, number, or boolean.";
                        return Some(Err(format!("Option `{key}` {why}")));
                    }
                }
            }
        }
        Some(_) => {
            return Some(Err(
                "`options` in `session attach` must be a JSON object.".into()
            ))
        }
    }
    Some(Ok(args))
}

/// The bare flag `session attach` writes for an option given `true`, by its rule: only a
/// yes-or-no flag of the command is written bare, since a flag that takes a value would read the
/// flag after it as that value; a line naming no command leaves it out (the table refuses that
/// line first).
fn attach_switch(row: Option<&Command>, key: &str, name: &str) -> Result<Option<String>, String> {
    let Some(row) = row else {
        return Ok(None);
    };
    match row.flag_takes_value(name) {
        Some(false) => Ok(Some(format!("--{name}"))),
        Some(true) => Err(format!("Option `{key}` takes a value, not `true`.")),
        None => Err(format!("Option `{key}` is not a flag of `{}`.", row.name())),
    }
}

fn is_placeholder(word: &str) -> bool {
    word.find('<')
        .is_some_and(|open| word[open..].contains('>'))
}

/// Whether `word` is one placeholder and nothing else (`<json>`).
fn is_bare_placeholder(word: &str) -> bool {
    word.len() > 2
        && word.starts_with('<')
        && word.ends_with('>')
        && !word[1..word.len() - 1].contains(['<', '>'])
}

/// A harmless JSON value of type `ty` for a placeholder leaf.
fn json_of_type(ty: &Type) -> Value {
    match ty {
        Type::Integer => Value::from(1),
        Type::Boolean => Value::Bool(true),
        Type::Enum(words) => Value::from(words.first().copied().unwrap_or("x")),
        Type::List(inner) => Value::Array(vec![json_of_type(inner)]),
        Type::Object(fields) => Value::Object(
            fields
                .iter()
                .filter(|f| f.required)
                .map(|f| (f.name.to_owned(), json_of_type(&f.ty)))
                .collect(),
        ),
        Type::String | Type::Json => Value::from("drift-guard-value"),
    }
}

/// Replaces every placeholder string leaf of `value` with a value of the type declared at its
/// place (`ty`, `None` where nothing is declared: an unknown field keeps its text). Keys,
/// literals and structure are untouched, so the check still sees every field as written.
fn fill_json_leaves(value: &mut Value, ty: Option<&Type>) {
    match value {
        Value::String(text) if is_placeholder(text) => {
            if let Some(ty) = ty {
                *value = json_of_type(ty);
            }
        }
        Value::Object(object) => {
            let fields = match ty {
                Some(Type::Object(fields)) => Some(*fields),
                _ => None,
            };
            for (key, item) in object.iter_mut() {
                let declared = fields
                    .and_then(|fields| fields.iter().find(|f| f.name == key))
                    .map(|f| &f.ty);
                fill_json_leaves(item, declared);
            }
        }
        Value::Array(items) => {
            let inner = match ty {
                Some(Type::List(inner)) => Some(*inner),
                _ => None,
            };
            for item in items {
                fill_json_leaves(item, inner);
            }
        }
        _ => {}
    }
}

/// The `--input-json` text to check for `text` as cited: `{}` for a bare placeholder, the object
/// with its placeholder leaves filled when it is JSON, and otherwise the text itself (which the
/// check then refuses).
fn fill_json_option(command: &Command, text: &str) -> String {
    if is_bare_placeholder(text) {
        return "{}".to_owned();
    }
    match serde_json::from_str::<Value>(text) {
        Ok(mut value) => {
            fill_json_leaves(&mut value, Some(&Type::Object(command.input)));
            value.to_string()
        }
        Err(_) => text.to_owned(),
    }
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
        if name == "input-json" {
            match (inline, args.get(at)) {
                (Some(value), _) => {
                    out.push(format!("--input-json={}", fill_json_option(command, value)))
                }
                (None, Some(value)) => {
                    at += 1;
                    out.extend([word.clone(), fill_json_option(command, value)]);
                }
                (None, None) => out.push(word.clone()),
            }
            continue;
        }
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
    if let Some(why) = &line.refused {
        return Some(why.clone());
    }
    // The table's own shorthands (`who`, `task note`, `context`), as `agent-os` reads them.
    let args = table.expand(&line.args).into_owned();
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
    for line in cited_command_lines(text, table) {
        if let Some(why) = check_line(&line, table) {
            let problem = format!("`{}`: {why}", line.cited);
            if !problems.contains(&problem) {
                problems.push(problem);
            }
        }
    }
    problems
}
