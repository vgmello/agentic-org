//! Showing a host result. A slice serializes the host's typed answer to the JSON its
//! `--json` prints (the stable contract) and declares which fields its table or card shows;
//! this module turns that into the shell's tables and cards. The values are taken from the
//! JSON, so a person sees the same lower-case words a script reads (`hq`, `active`), never a
//! Rust type's name, and every value is sanitized by the shell's renderer. A leaf module
//! beside `host`.

use org_cli_shell::cache::CachedCandidate;
use org_cli_shell::context::ExecutionContext;
use org_cli_shell::output::{Cell, ListView, Row, Style, Table};
use serde::Serialize;
use serde_json::Value;

use crate::error::CliError;

/// How a column shows its value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Show {
    Plain,
    Id,
    Name,
    Dim,
    /// A state word, with its glyph and color on a terminal.
    Status,
    /// The number of items of a list.
    Count,
}

/// One column of a table, or one line of a card: its header and the dotted path of its
/// value in the JSON (`invitation.status`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Column {
    pub(crate) header: &'static str,
    pub(crate) path: &'static str,
    pub(crate) show: Show,
}

pub(crate) const fn column(header: &'static str, path: &'static str, show: Show) -> Column {
    Column { header, path, show }
}

/// The JSON a typed host answer prints under `--json`.
pub(crate) fn json<T: Serialize>(answer: &T) -> Result<Value, CliError> {
    serde_json::to_value(answer).map_err(|e| CliError::failed("cli::encode", e.to_string()))
}

/// The value at a dotted `path` (`a.b`, `items.0.id`).
pub(crate) fn lookup<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(value, |at, key| match at {
        Value::Array(items) => key.parse::<usize>().ok().and_then(|i| items.get(i)),
        other => other.get(key),
    })
}

/// A value as one line of text: text as it is, nothing for null or missing, a list of
/// words joined with `, `, anything else as compact JSON.
pub(crate) fn text(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) if items.iter().all(|item| item.is_string()) => items
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", "),
        Some(other) => other.to_string(),
    }
}

/// The style of a state word.
pub(crate) fn status_style(word: &str) -> Style {
    match word.to_ascii_lowercase().as_str() {
        "active" | "online" | "running" | "open" | "ready" | "healthy" | "bound" | "enabled" => {
            Style::Active
        }
        "completed" | "approved" | "committed" | "cleared" | "installed" | "accepted"
        | "started" => Style::Ok,
        "pending" | "awaiting_approval" | "awaiting_join" | "join_pending" | "starting"
        | "restarting" | "creating" | "stale" | "unloaded" => Style::Pending,
        "offline" | "stopped" | "failed" | "error" | "declined" | "cancelled" | "revoked"
        | "removed" | "expired" | "disabled" | "archived" => Style::Error,
        _ => Style::Plain,
    }
}

fn cell(value: &Value, column: &Column) -> Cell {
    let found = lookup(value, column.path);
    match column.show {
        Show::Plain => Cell::plain(text(found)),
        Show::Id => Cell::id(text(found)),
        Show::Name => Cell::name(text(found)),
        Show::Dim => Cell::dim(text(found)),
        Show::Status => {
            let word = text(found);
            let style = status_style(&word);
            Cell::status(word, style)
        }
        Show::Count => Cell::plain(
            found
                .and_then(Value::as_array)
                .map_or(0, Vec::len)
                .to_string(),
        ),
    }
}

/// The table of `items`, each row's ID at `id`.
pub(crate) fn table(items: &[Value], id: &str, columns: &[Column]) -> Table {
    Table {
        headers: columns
            .iter()
            .map(|column| column.header.to_owned())
            .collect(),
        rows: items
            .iter()
            .map(|item| Row {
                id: text(lookup(item, id)),
                cells: columns.iter().map(|column| cell(item, column)).collect(),
            })
            .collect(),
    }
}

/// The items of the list at `path` of `json` (none when it is missing).
pub(crate) fn items<'a>(json: &'a Value, path: &str) -> &'a [Value] {
    lookup(json, path)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

/// `rows` as a table, piped lines, `-q` IDs (each row's `id` path), or `json` (the whole
/// document the host answered) under `--json`.
pub(crate) fn list(
    ctx: &mut ExecutionContext,
    rows: &[Value],
    json: &Value,
    id: &str,
    columns: &[Column],
    empty: &str,
) -> Result<(), CliError> {
    let view = ListView { empty, total: None };
    Ok(ctx.render_table(&table(rows, id, columns), json, &view)?)
}

/// `shown` as a card of `fields` (its ID at `id`, what `-q` prints), or `json` under
/// `--json`. `shown` is `json` with what the card derives added (an HQ office's ID, ...).
pub(crate) fn detail(
    ctx: &mut ExecutionContext,
    shown: &Value,
    json: &Value,
    id: &str,
    fields: &[Column],
) -> Result<(), CliError> {
    let cells: Vec<(&str, Cell)> = fields
        .iter()
        .filter(|field| lookup(shown, field.path).is_some_and(|value| !value.is_null()))
        .map(|field| (field.header, cell(shown, field)))
        .collect();
    Ok(ctx.render_detail(&text(lookup(shown, id)), &cells, json)?)
}

/// Milliseconds since the Unix epoch as `YYYY-MM-DD HH:MM UTC` (proleptic Gregorian).
pub(crate) fn utc_time(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil from days (Howard Hinnant's algorithm), for 1970-01-01 = day 0.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02} UTC",
        rest / 3600,
        rest % 3600 / 60
    )
}

/// The completion candidates of a list: each item's `name` path, with `help` as its help.
pub(crate) fn candidates(
    json: &Value,
    items: &str,
    name: &str,
    help: &str,
) -> Vec<CachedCandidate> {
    lookup(json, items)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| CachedCandidate {
                    name: text(lookup(item, name)),
                    help: Some(text(lookup(item, help))).filter(|help| !help.is_empty()),
                })
                .filter(|candidate| !candidate.name.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use org_cli_shell::context::testing::{captured, tty};
    use org_cli_shell::context::Modes;
    use org_cli_shell::terminal::Terminal;
    use serde_json::json;

    use super::*;

    const COLUMNS: [Column; 4] = [
        column("ID", "id", Show::Id),
        column("ROLE", "role", Show::Status),
        column("OFFICES", "offices", Show::Count),
        column("NAME", "name", Show::Name),
    ];

    fn sample() -> Value {
        json!({"items": [
            {"id": "o-1", "role": "hq", "offices": [1, 2], "name": "Acme\u{1b}[2J"},
            {"id": "o-2", "role": "satellite", "offices": [], "name": "Ber\tlin"}
        ]})
    }

    #[test]
    fn a_list_is_lower_case_words_tsv_in_a_pipe_and_ids_under_quiet() {
        let (mut ctx, out) = captured(Modes::default(), Terminal::piped(), &[]);
        list(
            &mut ctx,
            items(&sample(), "items"),
            &sample(),
            "id",
            &COLUMNS,
            "none",
        )
        .unwrap();
        assert_eq!(
            out.stdout.text(),
            "o-1\thq\t2\tAcme\\u{1b}[2J\no-2\tsatellite\t0\tBer\\u{9}lin\n"
        );
        let quiet = Modes {
            quiet: true,
            ..Modes::default()
        };
        let (mut ctx, out) = captured(quiet, tty(Some(80)), &[]);
        list(
            &mut ctx,
            items(&sample(), "items"),
            &sample(),
            "id",
            &COLUMNS,
            "none",
        )
        .unwrap();
        assert_eq!(out.stdout.text(), "o-1\no-2\n");
        let json_mode = Modes {
            json: true,
            ..Modes::default()
        };
        let (mut ctx, out) = captured(json_mode, Terminal::piped(), &[]);
        list(
            &mut ctx,
            items(&sample(), "items"),
            &sample(),
            "id",
            &COLUMNS,
            "none",
        )
        .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&out.stdout.text()).unwrap(),
            sample()
        );
    }

    #[test]
    fn utc_times() {
        assert_eq!(utc_time(0), "1970-01-01 00:00 UTC");
        assert_eq!(utc_time(1_800_000_000_000), "2027-01-15 08:00 UTC");
        assert_eq!(utc_time(951_782_400_000), "2000-02-29 00:00 UTC");
    }

    #[test]
    fn lookups_and_text() {
        let value = json!({"a": {"b": [{"c": "x"}]}, "words": ["a", "b"], "n": 3});
        assert_eq!(text(lookup(&value, "a.b.0.c")), "x");
        assert_eq!(text(lookup(&value, "words")), "a, b");
        assert_eq!(text(lookup(&value, "n")), "3");
        assert_eq!(text(lookup(&value, "missing.path")), "");
        let names = candidates(
            &json!({"o": [{"id": "1", "slug": "acme"}, {"id": ""}]}),
            "o",
            "id",
            "slug",
        );
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].help.as_deref(), Some("acme"));
    }
}
