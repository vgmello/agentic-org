//! `tasks/BACKLOG.md`: the backlog table HR's template starts and agents keep.
//!
//! ```text
//! # Backlog · <name>
//!
//! | # | Task | Status | Requested by | Notes |
//! | --- | --- | --- | --- | --- |
//! | 1 | ... | todo | HR | |
//! ```
//!
//! Agents (LLMs) edit these tables by hand, so the parser accepts what they
//! actually write: columns found by header name in any order, extra text after
//! the status, missing trailing cells, pipes in code spans, several tables. A
//! row it still can't read becomes [`BacklogItem::Unreadable`], never an error.

use crate::markdown::{
    find_tables, header_key, heading, numbered_lines, split_row_code_aware, stray_rows, Row,
};
use crate::model::{Backlog, BacklogItem, Issue, Parsed, ProblemKind, Task, TaskStatus};

/// Column positions of a backlog table, found by header name.
struct Columns {
    number: Option<usize>,
    task: usize,
    status: usize,
    requested_by: Option<usize>,
    notes: Option<usize>,
    width: usize,
}

impl Columns {
    fn from_header(header: &[String]) -> Option<Self> {
        let keys: Vec<String> = header.iter().map(|h| header_key(h)).collect();
        let find = |names: &[&str]| keys.iter().position(|k| names.contains(&k.as_str()));
        Some(Self {
            number: find(&["#", "no", "no.", "nr", "id", "number"]),
            task: find(&["task", "tasks", "title"])?,
            status: find(&["status", "state"])?,
            requested_by: find(&["requested by", "requester", "requested", "from"]),
            notes: find(&["notes", "note", "comments", "details"]),
            width: header.len(),
        })
    }
}

fn is_backlog_header(cells: &[String]) -> bool {
    Columns::from_header(cells).is_some()
}

/// Parses a backlog file's text. Never fails: rows it can't read become
/// [`BacklogItem::Unreadable`], and file-level oddities are returned as issues.
pub fn parse_backlog(text: &str) -> Parsed<Backlog> {
    let lines = numbered_lines(text);
    let mut issues = Vec::new();

    let owner = lines.iter().find_map(|(_, line)| {
        let (level, title) = heading(line)?;
        let rest = title
            .strip_prefix("Backlog")
            .or_else(|| title.strip_prefix("backlog"))?;
        let name = rest.trim_start_matches(|c: char| {
            c.is_whitespace() || matches!(c, '·' | '-' | '—' | '–' | ':' | '|')
        });
        (level == 1 && !name.trim().is_empty()).then(|| name.trim().to_string())
    });

    let tables = find_tables(&lines, is_backlog_header);
    if tables.is_empty() {
        issues.push(Issue::warning(
            ProblemKind::Malformed,
            "no backlog table found (a table whose header has Task and Status columns)",
        ));
    }

    let mut items = Vec::new();
    for table in &tables {
        // `find_tables` only returns tables whose header passed this check.
        let Some(columns) = Columns::from_header(&table.header) else {
            continue;
        };
        items.extend(table.rows.iter().map(|row| parse_row(row, &columns)));
    }
    // Rows cut off from the table (by a heading, a comment or a prose line) are
    // shown as unreadable rather than dropped.
    items.extend(stray_rows(&lines, &tables).iter().map(|row| {
        unreadable(
            row,
            "the row isn't under a table header with Task and Status columns",
        )
    }));
    items.sort_by_key(BacklogItem::line_no);

    let unreadable: Vec<String> = items
        .iter()
        .filter_map(|item| match item {
            BacklogItem::Unreadable { line_no, .. } => Some(line_no.to_string()),
            BacklogItem::Task(_) => None,
        })
        .collect();
    if !unreadable.is_empty() {
        let s = if unreadable.len() == 1 { "" } else { "s" };
        issues.push(Issue::warning(
            ProblemKind::UnreadableRows,
            format!(
                "{} row{s} can't be read (line{s} {})",
                unreadable.len(),
                if unreadable.len() > 10 {
                    format!("{}, …", unreadable[..10].join(", "))
                } else {
                    unreadable.join(", ")
                }
            ),
        ));
    }

    Parsed::new(Backlog { owner, items }, issues)
}

fn unreadable(row: &Row, reason: impl Into<String>) -> BacklogItem {
    BacklogItem::Unreadable {
        line_no: row.line_no,
        raw: row.raw.clone(),
        reason: reason.into(),
    }
}

/// Whether the row's status column holds a known status.
fn status_is_known(cells: &[String], columns: &Columns) -> bool {
    cells
        .get(columns.status)
        .is_some_and(|c| !matches!(parse_status(c), TaskStatus::Other(_)))
}

/// A cell that is nothing but a known status word, e.g. `todo` or `**done**`.
fn is_bare_status(cell: &str) -> bool {
    let word = cell
        .trim_matches(|c: char| c.is_whitespace() || matches!(c, '*' | '_' | '`'))
        .to_lowercase();
    STATUS_WORDS.iter().any(|(token, _)| *token == word)
}

/// The index of a cell that holds the row's status when the Status column
/// doesn't, i.e. the columns are shifted. `bad_number` is whether the `#` cell
/// holds text that isn't a number (the sign of a missing leading cell).
fn shifted_status(cells: &[String], columns: &Columns, bad_number: bool) -> Option<usize> {
    let s = columns.status;
    let full_width = cells.len() == columns.width;
    let known = |i: usize| {
        cells
            .get(i)
            .is_some_and(|c| !matches!(parse_status(c), TaskStatus::Other(_)))
    };
    if status_is_known(cells, columns) {
        // An unescaped pipe left of the status column: the status cell holds task
        // text that happens to start with a status word, and the real status
        // sits further right.
        if cells.len() > columns.width {
            return (s + 1..cells.len()).find(|&i| is_bare_status(&cells[i]));
        }
        return None;
    }
    if full_width {
        return (0..cells.len()).find(|&i| i != s && is_bare_status(&cells[i]));
    }
    // A short or long row: the status moved one column right (an extra pipe)
    // or left (a missing cell, which also leaves task text in the # column).
    if known(s + 1) {
        return Some(s + 1);
    }
    if s > 0 && known(s - 1) && (bad_number || columns.number.is_none()) {
        return Some(s - 1);
    }
    None
}

fn parse_row(row: &Row, columns: &Columns) -> BacklogItem {
    let mut cells = row.cells.clone();
    let mut warnings = Vec::new();

    // Plain GFM splitting can cut a code span such as `a | b` in two and still
    // give the right number of cells; prefer the reading whose status column
    // holds a status.
    if !status_is_known(&cells, columns) {
        if let Some(aware) = split_row_code_aware(&row.raw) {
            if status_is_known(&aware, columns) {
                cells = aware;
            }
        }
    }

    let needed = columns.task.max(columns.status) + 1;
    if cells.len() < needed {
        return unreadable(
            row,
            format!(
                "the row has {} cell{} but the table has {} columns, so there is no {} cell",
                cells.len(),
                if cells.len() == 1 { "" } else { "s" },
                columns.width,
                if cells.len() <= columns.status {
                    "status"
                } else {
                    "task"
                },
            ),
        );
    }
    // The cells as split, before any overflow is merged.
    let split = cells.clone();
    if cells.len() > columns.width {
        // More cells than columns: a pipe inside some text. Keep the overflow in
        // the last column, where free text (notes) usually is.
        let overflow = cells.split_off(columns.width);
        if let Some(last) = cells.last_mut() {
            for extra in overflow {
                last.push_str(" | ");
                last.push_str(&extra);
            }
        }
        warnings.push(format!(
            "the row has more cells than the table's {} columns; the extra text is kept in the last column",
            columns.width
        ));
    } else if cells.len() < columns.width {
        warnings.push(format!(
            "the row has {} of the table's {} cells; the missing trailing cells are empty",
            cells.len(),
            columns.width
        ));
    }

    let cell = |index: Option<usize>| {
        index
            .and_then(|i| cells.get(i))
            .cloned()
            .unwrap_or_default()
    };
    let task = cell(Some(columns.task));
    let status_raw = cell(Some(columns.status));
    if task.is_empty() {
        return unreadable(row, "the task cell is empty");
    }
    if status_raw.is_empty() {
        return unreadable(row, "the status cell is empty");
    }

    let number_raw = cell(columns.number);
    let number = parse_number(&number_raw);

    // A missing cell or an unescaped `|` shifts every column after it. Reading
    // such a row as a task would show wrong data, so it's unreadable instead.
    if let Some(index) = shifted_status(&split, columns, number.is_none() && !number_raw.is_empty())
    {
        return unreadable(
            row,
            format!(
                "the Status column holds `{status_raw}` but cell {} is `{}`: the columns look shifted \
                 (a missing cell or an unescaped |)",
                index + 1,
                split[index]
            ),
        );
    }

    if number.is_none() && !number_raw.is_empty() {
        warnings.push(format!("the # cell `{number_raw}` isn't a number"));
    }

    BacklogItem::Task(Task {
        line_no: row.line_no,
        number,
        task,
        status: parse_status(&status_raw),
        status_raw,
        requested_by: cell(columns.requested_by),
        notes: cell(columns.notes),
        warnings,
    })
}

/// `3`, `#3`, `3.`, `**3**` -> 3.
fn parse_number(raw: &str) -> Option<u32> {
    let trimmed = raw.trim_matches(|c: char| c.is_whitespace() || matches!(c, '*' | '_' | '`'));
    let trimmed = trimmed.strip_prefix('#').unwrap_or(trimmed);
    let trimmed = trimmed.strip_suffix('.').unwrap_or(trimmed);
    trimmed.trim().parse().ok()
}

/// Status words and their spellings, matched at the start of a status cell.
const STATUS_WORDS: &[(&str, TaskStatus)] = &[
    ("in-progress", TaskStatus::InProgress),
    ("in progress", TaskStatus::InProgress),
    ("in_progress", TaskStatus::InProgress),
    ("inprogress", TaskStatus::InProgress),
    ("todo", TaskStatus::Todo),
    ("to-do", TaskStatus::Todo),
    ("to do", TaskStatus::Todo),
    ("blocked", TaskStatus::Blocked),
    ("done", TaskStatus::Done),
];

/// Normalizes a status cell by its leading word: `in-progress (awaiting go)` is
/// in progress, `**Done** ✅` is done, `Blocked: waiting on design` is blocked.
/// Anything else is kept as [`TaskStatus::Other`] with the cell's text.
pub fn parse_status(raw: &str) -> TaskStatus {
    // LLMs sometimes write `in‑progress` with a non-breaking or other Unicode hyphen.
    let raw_hyphens = raw.replace(['\u{2010}', '\u{2011}', '\u{2012}', '\u{2013}'], "-");
    let lowered = raw_hyphens
        .trim()
        .trim_start_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    for (token, status) in STATUS_WORDS {
        if let Some(rest) = lowered.strip_prefix(token) {
            if !rest.starts_with(|c: char| c.is_alphanumeric()) {
                return status.clone();
            }
        }
    }
    TaskStatus::Other(raw.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tasks(text: &str) -> Vec<Task> {
        parse_backlog(text).value.tasks().cloned().collect()
    }

    const HEADER: &str =
        "| # | Task | Status | Requested by | Notes |\n| --- | --- | --- | --- | --- |\n";

    #[test]
    fn statuses() {
        assert_eq!(parse_status("todo"), TaskStatus::Todo);
        assert_eq!(parse_status(" In Progress "), TaskStatus::InProgress);
        assert_eq!(
            parse_status("in-progress (awaiting the lead's go)"),
            TaskStatus::InProgress
        );
        assert_eq!(parse_status("`blocked`: waiting"), TaskStatus::Blocked);
        assert_eq!(parse_status("**Done** ✅"), TaskStatus::Done);
        assert_eq!(parse_status("✅ done"), TaskStatus::Done);
        assert_eq!(parse_status("done."), TaskStatus::Done);
        assert_eq!(parse_status("doneish"), TaskStatus::Other("doneish".into()));
        assert_eq!(
            parse_status("cancelled"),
            TaskStatus::Other("cancelled".into())
        );
        assert_eq!(parse_status("İ"), TaskStatus::Other("İ".into()));
    }

    #[test]
    fn numbers() {
        assert_eq!(parse_number("3"), Some(3));
        assert_eq!(parse_number("#12"), Some(12));
        assert_eq!(parse_number("**4.**"), Some(4));
        assert_eq!(parse_number("x"), None);
        assert_eq!(parse_number(""), None);
        assert_eq!(parse_number("99999999999"), None);
    }

    #[test]
    fn reads_the_template_backlog() {
        let text = format!("# Backlog · Ada\n\nStatus values: `todo` · `in-progress`\n\n{HEADER}| 1 | Onboarding | todo | HR | |\n");
        let parsed = parse_backlog(&text);
        assert!(parsed.issues.is_empty(), "{:?}", parsed.issues);
        assert_eq!(parsed.value.owner.as_deref(), Some("Ada"));
        let t = &tasks(&text)[0];
        assert_eq!(
            (t.number, t.task.as_str(), &t.status),
            (Some(1), "Onboarding", &TaskStatus::Todo)
        );
        assert_eq!(
            (t.requested_by.as_str(), t.notes.as_str(), t.line_no),
            ("HR", "", 7)
        );
        assert!(t.warnings.is_empty());
    }

    #[test]
    fn missing_trailing_cell_is_tolerated() {
        let t = &tasks(&format!(
            "{HEADER}| 4 | Fold it in | done | lead. Spec rev 3 |\n"
        ))[0];
        assert_eq!(t.status, TaskStatus::Done);
        assert_eq!(t.requested_by, "lead. Spec rev 3");
        assert_eq!(t.notes, "");
        assert_eq!(t.warnings.len(), 1);
    }

    #[test]
    fn too_few_cells_is_unreadable() {
        let parsed = parse_backlog(&format!(
            "{HEADER}| 7 | half a row\n| 8 | ok | todo | me | |\n"
        ));
        match &parsed.value.items[0] {
            BacklogItem::Unreadable {
                line_no,
                raw,
                reason,
            } => {
                assert_eq!(*line_no, 3);
                assert_eq!(raw, "| 7 | half a row");
                assert!(reason.contains("no status cell"), "{reason}");
            }
            other => panic!("expected unreadable, got {other:?}"),
        }
        assert!(matches!(parsed.value.items[1], BacklogItem::Task(_)));
        assert_eq!(parsed.issues.len(), 1);
        assert_eq!(parsed.issues[0].kind, ProblemKind::UnreadableRows);
    }

    #[test]
    fn empty_task_or_status_is_unreadable() {
        let parsed = parse_backlog(&format!(
            "{HEADER}| 1 | | todo | | |\n| 2 | t |  | | |\n|  |  |  |  |  |\n"
        ));
        assert_eq!(parsed.value.items.len(), 2, "the all-empty row is dropped");
        assert!(parsed.value.tasks().next().is_none());
    }

    #[test]
    fn pipes_in_code_and_escapes() {
        let text = format!(
            "{HEADER}| 1 | run `a | b` | todo | me | n |\n| 2 | x \\| y | done | me | a | b |\n"
        );
        let ts = tasks(&text);
        assert_eq!(ts[0].task, "run `a | b`");
        assert!(ts[0].warnings.is_empty());
        assert_eq!(ts[1].task, "x | y");
        assert_eq!(ts[1].notes, "a | b");
        assert_eq!(ts[1].warnings.len(), 1);
    }

    #[test]
    fn columns_by_name_extra_tables_and_crlf() {
        let text = "# Backlog\r\n\r\n| Status | Task |\r\n|---|---|\r\n| done | first |\r\n\r\n## Later\r\n\r\n| # | Task | Status |\r\n| - | - | - |\r\n| 9 | second | blocked on x |\r\n";
        let parsed = parse_backlog(text);
        assert_eq!(parsed.value.owner, None);
        let ts: Vec<_> = parsed.value.tasks().collect();
        assert_eq!(ts.len(), 2);
        assert_eq!(
            (ts[0].task.as_str(), &ts[0].status, ts[0].number),
            ("first", &TaskStatus::Done, None)
        );
        assert_eq!(
            (ts[1].number, &ts[1].status, ts[1].status_raw.as_str()),
            (Some(9), &TaskStatus::Blocked, "blocked on x")
        );
    }

    #[test]
    fn shifted_columns_are_unreadable_not_misread() {
        let rows = [
            // The # cell is missing.
            "| Fold it in | done | user | note |",
            // An unescaped pipe in the task text.
            "| 3 | Choose A | B for the cache | todo | me | |",
        ];
        for row in rows {
            let parsed = parse_backlog(&format!("{HEADER}{row}\n"));
            match &parsed.value.items[0] {
                BacklogItem::Unreadable { reason, .. } => {
                    assert!(reason.contains("shifted"), "{reason}")
                }
                other => panic!("{row}: expected unreadable, got {other:?}"),
            }
        }
        // A code span cut in two by plain splitting still reads correctly.
        let t = &tasks(&format!("{HEADER}| 1 | run `a | b` | todo | me |\n"))[0];
        assert_eq!(
            (t.task.as_str(), &t.status, t.requested_by.as_str()),
            ("run `a | b`", &TaskStatus::Todo, "me")
        );
        // An unknown status next to notes that merely start with a status word is fine.
        let t = &tasks(&format!(
            "{HEADER}| 2 | Spike | parked | sam | Done when: it works |\n"
        ))[0];
        assert_eq!(t.status, TaskStatus::Other("parked".into()));
    }

    #[test]
    fn shifted_rows_with_status_text_are_unreadable() {
        for row in [
            // The # cell is missing and the status carries extra text.
            "| Fold it | in-progress (awaiting go) | user | |",
            "| Fold it | done: shipped | user | note |",
            // An unescaped pipe whose second half starts with a status word.
            "| 3 | Choose A | done or B | todo | me | |",
        ] {
            let parsed = parse_backlog(&format!("{HEADER}{row}\n"));
            assert!(
                matches!(&parsed.value.items[0], BacklogItem::Unreadable { reason, .. } if reason.contains("shifted")),
                "{row}: {:?}",
                parsed.value.items
            );
        }
        // Short rows that aren't shifted still read.
        let t = &tasks(&format!(
            "{HEADER}| 4 | Done-criteria review | parked | me |\n"
        ))[0];
        assert_eq!(t.status, TaskStatus::Other("parked".into()));
        let t = &tasks(&format!("{HEADER}| 5 | t | todo | me | a | b |\n"))[0];
        assert_eq!(
            (t.status.clone(), t.notes.as_str()),
            (TaskStatus::Todo, "a | b")
        );
        assert_eq!(parse_status("in\u{2011}progress"), TaskStatus::InProgress);
    }

    #[test]
    fn rows_cut_off_from_the_table_are_unreadable_not_dropped() {
        for separator in ["**Archive**", "<!-- moved -->", "## Done"] {
            let text = format!("{HEADER}| 1 | a | todo | | |\n{separator}\n| 2 | b | done | | |\n| 3 | c | done | | |\n");
            let items = parse_backlog(&text).value.items;
            let kinds: Vec<_> = items
                .iter()
                .map(|i| (i.line_no(), matches!(i, BacklogItem::Task(_))))
                .collect();
            assert_eq!(kinds, [(3, true), (5, false), (6, false)], "{separator}");
        }
        // Another kind of table (its own header and delimiter) isn't the backlog's business.
        let text = format!("{HEADER}| 1 | a | todo | | |\n\n| Date | Event |\n| --- | --- |\n| 2026-01-01 | Kickoff |\n");
        assert_eq!(parse_backlog(&text).value.items.len(), 1);
    }

    #[test]
    fn no_table_is_an_issue_not_an_error() {
        let parsed = parse_backlog("# Backlog · Kai\n\nNothing here yet.\n");
        assert!(parsed.value.items.is_empty());
        assert_eq!(parsed.issues[0].kind, ProblemKind::Malformed);
    }

    #[test]
    fn tables_in_code_fences_are_ignored() {
        let text = format!("```\n{HEADER}| 1 | fenced | todo | | |\n```\n");
        assert!(parse_backlog(&text).value.items.is_empty());
    }

    #[test]
    fn a_blank_line_inside_the_table_does_not_end_it() {
        let text = format!("{HEADER}| 1 | a | todo | | |\n\n| 2 | b | done | | |\n\nAfter.\n");
        assert_eq!(tasks(&text).len(), 2);
    }
}
