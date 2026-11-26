//! An office command's JSON envelope shown for a person on a terminal: a borderless table
//! for a list (its declared columns), an aligned card for a record, the IDs under `-q`, and a
//! refusal as a diagnostic on standard error. Every value is sanitized by the shell's
//! renderer before it reaches the terminal.

use org_cli_shell::context::ExecutionContext;
use org_cli_shell::output::{Cell, ListView, Row, Style, Table};
use org_command::{Command, Output};
use serde_json::{Map, Value};

use crate::error::CliError;

/// Shows `output` (a `--format json` answer of `command`).
pub(super) fn answer(
    output: &Output,
    command: Option<&Command>,
    quiet: bool,
    ctx: &mut ExecutionContext,
) -> Result<(), CliError> {
    let Ok(envelope) = serde_json::from_str::<Value>(output.stdout.trim_end()) else {
        // Not an envelope (help text): as it is.
        return print_answer(ctx, &output.stdout, output.exit_code);
    };
    if envelope["ok"] != Value::Bool(true) {
        return Err(refusal(&envelope["error"], output.exit_code));
    }
    note_meta(&envelope["meta"], ctx);
    match &envelope["data"] {
        Value::Array(items) => list(items, command, quiet, ctx),
        Value::Object(record) => detail(record, command, quiet, ctx),
        Value::Null => Ok(()),
        other => Ok(ctx.out(&format!(
            "{}\n",
            org_cli_shell::terminal::sanitize(&plain(other))
        ))?),
    }
}

/// Prints the table's answer. A reader that went away does not turn a refusal into a
/// success: the table's own exit code stands (0 only when it succeeded).
pub(super) fn print_answer(
    ctx: &mut ExecutionContext,
    text: &str,
    code: u8,
) -> Result<(), CliError> {
    match ctx.out(text) {
        Err(org_cli_shell::error::ShellError::OutputClosed) if code != 0 => exited(code),
        Err(error) => Err(error.into()),
        Ok(()) => exited(code),
    }
}

/// The table's exit code, its answer already printed.
fn exited(code: u8) -> Result<(), CliError> {
    if code == 0 {
        Ok(())
    } else {
        Err(CliError::Exited(code))
    }
}

fn refusal(error: &Value, exit: u8) -> CliError {
    let text = |key: &str| error[key].as_str().unwrap_or_default().to_owned();
    let hint = text("hint");
    CliError::Office {
        code: format!("office::{}", error["code"].as_str().unwrap_or("error")),
        message: text("message"),
        hint: (!hint.is_empty()).then_some(hint),
        exit: if exit == 0 { 1 } else { exit },
    }
}

fn note_meta(meta: &Value, ctx: &mut ExecutionContext) {
    if let Some(status) = meta["status"]
        .as_str()
        .filter(|status| *status != "completed")
    {
        ctx.note(&format!("status: {status}"));
    }
    if meta["dry_run"] == Value::Bool(true) {
        ctx.note("[DRY-RUN] nothing was changed");
    }
    if let Some(cursor) = meta["next_cursor"].as_str() {
        ctx.note(&format!("More results: add --cursor {cursor}"));
    }
}

/// The columns of a list: the command's declared output fields, or the first item's keys.
fn columns(items: &[Value], command: Option<&Command>) -> Vec<String> {
    match command {
        Some(command) if !command.output.is_empty() => command
            .output
            .iter()
            .map(|field| field.name.to_owned())
            .collect(),
        _ => items
            .first()
            .and_then(Value::as_object)
            .map(|first| first.keys().cloned().collect())
            .unwrap_or_default(),
    }
}

fn primary_id(record: &Map<String, Value>, columns: &[String]) -> String {
    ["id", "reference", "handle", "name"]
        .iter()
        .find_map(|key| record.get(*key))
        .or_else(|| columns.first().and_then(|first| record.get(first)))
        .map(plain)
        .unwrap_or_default()
}

fn list(
    items: &[Value],
    command: Option<&Command>,
    quiet: bool,
    ctx: &mut ExecutionContext,
) -> Result<(), CliError> {
    let columns = columns(items, command);
    let empty = Map::new();
    let rows = items
        .iter()
        .map(|item| {
            let record = item.as_object().unwrap_or(&empty);
            Row {
                id: if record.is_empty() {
                    plain(item)
                } else {
                    primary_id(record, &columns)
                },
                cells: columns
                    .iter()
                    .map(|column| cell(column, record.get(column).unwrap_or(&Value::Null)))
                    .collect(),
            }
        })
        .collect();
    let table = Table {
        headers: columns
            .iter()
            .map(|column| column.replace('_', " "))
            .collect(),
        rows,
    };
    let view = ListView {
        empty: "Nothing found.",
        total: None,
    };
    let _ = quiet;
    Ok(ctx.render_table(&table, &Value::Null, &view)?)
}

fn detail(
    record: &Map<String, Value>,
    command: Option<&Command>,
    quiet: bool,
    ctx: &mut ExecutionContext,
) -> Result<(), CliError> {
    let mut keys: Vec<String> = command
        .map(|command| {
            command
                .output
                .iter()
                .map(|field| field.name.to_owned())
                .collect()
        })
        .unwrap_or_default();
    keys.retain(|key| record.contains_key(key));
    keys.extend(
        record
            .keys()
            .filter(|key| !keys.contains(key))
            .cloned()
            .collect::<Vec<_>>(),
    );
    let id = primary_id(record, &keys);
    if quiet {
        // A record without an ID prints nothing, never an empty line.
        if id.is_empty() {
            return Ok(());
        }
        return Ok(ctx.out(&format!("{}\n", org_cli_shell::terminal::sanitize(&id)))?);
    }
    let fields: Vec<(&str, Cell)> = keys
        .iter()
        .map(|key| (key.as_str(), cell(key, &record[key])))
        .collect();
    Ok(ctx.render_detail(&id, &fields, &Value::Null)?)
}

/// A value on one line: text as it is, `null` as nothing, anything else as compact JSON.
fn plain(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn cell(name: &str, value: &Value) -> Cell {
    let text = plain(value);
    let style = match name {
        "id" | "reference" => Style::Id,
        _ if name.ends_with("_id") => Style::Dim,
        "name" | "title" | "handle" | "display_name" => Style::Name,
        "status" | "state" | "presence" | "lifecycle" => status_style(&text),
        _ if name.ends_with("_at") => Style::Dim,
        _ => Style::Plain,
    };
    Cell::new(text, style)
}

fn status_style(text: &str) -> Style {
    match text.to_ascii_lowercase().as_str() {
        "active" | "online" | "running" | "open" | "in_progress" | "working" | "building" => {
            Style::Active
        }
        "done" | "completed" | "committed" | "approved" | "closed" | "ready" | "merged" => {
            Style::Ok
        }
        "pending" | "queued" | "blocked" | "waiting" | "away" | "paused" | "idle" | "draft" => {
            Style::Pending
        }
        "failed" | "error" | "offline" | "cancelled" | "canceled" | "rejected" | "offboarded" => {
            Style::Error
        }
        _ => Style::Plain,
    }
}

#[cfg(test)]
mod tests {
    use org_cli_shell::context::testing::{captured, tty};
    use org_cli_shell::context::Modes;

    use super::*;

    fn output(stdout: &str, exit_code: u8) -> Output {
        Output {
            stdout: format!("{stdout}\n"),
            exit_code,
        }
    }

    #[test]
    fn a_list_is_a_sanitized_table_on_a_terminal_and_ids_under_quiet() {
        let envelope = r#"{"ok":true,"data":[{"id":"T-1","title":"Fix \u001b[31mred","status":"open"}],"meta":{"status":"completed"},"error":null}"#;
        let (mut ctx, streams) = captured(Modes::default(), tty(Some(80)), &[("NO_COLOR", "1")]);
        answer(&output(envelope, 0), None, false, &mut ctx).unwrap();
        let shown = streams.stdout.text();
        assert!(shown.contains("TITLE") && shown.contains("T-1"), "{shown}");
        // The office's text is escaped, never a terminal control sequence.
        assert!(!shown.contains('\u{1b}'), "{shown:?}");
        assert!(shown.contains("Fix \\u{1b}[31mred"), "{shown:?}");
        let quiet = Modes {
            quiet: true,
            ..Modes::default()
        };
        let (mut ctx, streams) = captured(quiet, tty(Some(80)), &[]);
        answer(&output(envelope, 0), None, true, &mut ctx).unwrap();
        assert_eq!(streams.stdout.text(), "T-1\n");
    }

    #[test]
    fn a_refusal_keeps_the_tables_exit_code_and_hint() {
        let envelope = r#"{"ok":false,"data":null,"meta":{},"error":{"code":"invalid_request","message":"`x` is not a resource.","hint":"The resources are: task."}}"#;
        let (mut ctx, streams) = captured(Modes::default(), tty(Some(80)), &[]);
        let error = answer(&output(envelope, 2), None, false, &mut ctx).unwrap_err();
        use org_cli_shell::error::CliDiagnostic;
        assert_eq!(error.exit_code(), 2);
        assert_eq!(error.code(), "office::invalid_request");
        assert_eq!(error.help().as_deref(), Some("The resources are: task."));
        assert!(streams.stdout.text().is_empty());
    }
}
