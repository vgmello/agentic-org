//! Small, tolerant Markdown helpers for the files agents and HR write: pipe
//! tables, ATX headings and sections, HTML-comment marker blocks and YAML-ish
//! frontmatter. Nothing here indexes strings by byte offset except at positions
//! returned by `find` for ASCII patterns, so no input can cause a panic.

use std::collections::BTreeMap;

/// The text's lines with 1-based line numbers, without a leading BOM or trailing `\r`.
pub(crate) fn numbered_lines(text: &str) -> Vec<(usize, &str)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    text.split('\n')
        .enumerate()
        .map(|(i, line)| (i + 1, line.strip_suffix('\r').unwrap_or(line)))
        .collect()
}

/// The fence character if the line is a code fence (``` or ~~~, indented at most 3 spaces).
fn fence_char(line: &str) -> Option<char> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    let rest = line.trim_start_matches(' ');
    if indent > 3 {
        None
    } else if rest.starts_with("```") {
        Some('`')
    } else if rest.starts_with("~~~") {
        Some('~')
    } else {
        None
    }
}

/// Tracks fenced code blocks line by line: a fence closes only with its own
/// character. Returns whether the line is a fence or inside one.
fn in_code(open: &mut Option<char>, line: &str) -> bool {
    match (fence_char(line), *open) {
        (Some(c), None) => {
            *open = Some(c);
            true
        }
        (Some(c), Some(o)) if c == o => {
            *open = None;
            true
        }
        (_, open) => open.is_some(),
    }
}

/// Splits a table row into trimmed cells. `\|` is a literal pipe. When
/// `code_aware` is set, pipes inside backtick code spans don't split cells either
/// (GFM would split them, but agents write `a | b` in code and mean one cell).
/// Returns `None` if the line has no pipe at all.
fn split_cells(line: &str, code_aware: bool) -> Option<Vec<String>> {
    let trimmed = line.trim();
    if !trimmed.contains('|') {
        return None;
    }
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut chars = trimmed.chars().peekable();
    // Length of the backtick run that opened the current code span, if inside one.
    let mut code_run: Option<usize> = None;
    let mut first = true;
    // Whether the last thing read was a separator pipe (then it closed the row).
    let mut ended_on_pipe = false;
    while let Some(c) = chars.next() {
        if first {
            first = false;
            if c == '|' {
                continue; // a leading pipe opens the row
            }
        }
        ended_on_pipe = false;
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                chars.next();
                cell.push('|');
            }
            '`' if code_aware => {
                let mut run = 1;
                while chars.peek() == Some(&'`') {
                    chars.next();
                    run += 1;
                }
                cell.extend(std::iter::repeat_n('`', run));
                code_run = match code_run {
                    None => Some(run),
                    Some(open) if open == run => None,
                    other => other,
                };
            }
            '|' if code_run.is_none() => {
                cells.push(std::mem::take(&mut cell).trim().to_string());
                ended_on_pipe = true;
            }
            _ => cell.push(c),
        }
    }
    if code_run.is_some() {
        // An unclosed code span: its backticks are literal, so split as plain text.
        return split_cells(line, false);
    }
    // Text after the last pipe is a last cell; a trailing pipe just closes the row.
    if !ended_on_pipe || cells.is_empty() {
        cells.push(cell.trim().to_string());
    }
    Some(cells)
}

/// Splits a row treating pipes inside backtick code spans as text.
pub(crate) fn split_row_code_aware(line: &str) -> Option<Vec<String>> {
    split_cells(line, true)
}

/// Splits a row, preferring the reading that gives `expected` cells: plain GFM
/// splitting first, then code-span-aware splitting.
pub(crate) fn split_row(line: &str, expected: Option<usize>) -> Option<Vec<String>> {
    let plain = split_cells(line, false)?;
    if let Some(n) = expected {
        if plain.len() != n {
            if let Some(aware) = split_cells(line, true) {
                if aware.len() == n {
                    return Some(aware);
                }
            }
        }
    }
    Some(plain)
}

/// A delimiter row such as `| --- | :---: |`.
pub(crate) fn is_delimiter(cells: &[String]) -> bool {
    !cells.is_empty()
        && cells.iter().all(|cell| {
            let inner = cell.trim().trim_start_matches(':').trim_end_matches(':');
            !inner.is_empty() && inner.chars().all(|c| c == '-')
        })
}

/// A header cell reduced to its words: lowercase, without emphasis or code marks.
pub(crate) fn header_key(cell: &str) -> String {
    let stripped: String = cell
        .chars()
        .filter(|c| !matches!(c, '*' | '_' | '`'))
        .collect();
    stripped
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// A table row: its line number, the line as written, and its cells.
#[derive(Debug, Clone)]
pub(crate) struct Row {
    pub line_no: usize,
    pub raw: String,
    pub cells: Vec<String>,
}

/// A table found in a document: its header cells and body rows.
#[derive(Debug, Clone)]
pub(crate) struct Table {
    pub header_line: usize,
    pub header: Vec<String>,
    pub rows: Vec<Row>,
}

/// Finds every pipe table whose header row satisfies `is_header`, outside fenced
/// code. Delimiter rows are skipped wherever they are; all-empty rows are dropped.
/// A blank line ends a table unless the next non-blank line is another body row
/// (not a header and not followed by a delimiter row).
pub(crate) fn find_tables(
    lines: &[(usize, &str)],
    is_header: impl Fn(&[String]) -> bool,
) -> Vec<Table> {
    let header_cells = |line: &str| split_row(line, None).filter(|cells| is_header(cells));
    let mut tables = Vec::new();
    let mut fence = None;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].1;
        if in_code(&mut fence, line) {
            i += 1;
            continue;
        }
        let header_line = lines[i].0;
        let header = match header_cells(line) {
            Some(header) => header,
            None => {
                i += 1;
                continue;
            }
        };
        i += 1;
        let width = header.len();
        let mut rows = Vec::new();
        while i < lines.len() {
            let (line_no, line) = lines[i];
            if line.trim().is_empty() {
                let mut j = i + 1;
                while j < lines.len() && lines[j].1.trim().is_empty() {
                    j += 1;
                }
                let continues = j < lines.len()
                    && lines[j].1.trim_start().starts_with('|')
                    && header_cells(lines[j].1).is_none()
                    && !lines
                        .get(j + 1)
                        .and_then(|(_, next)| split_row(next, None))
                        .is_some_and(|cells| is_delimiter(&cells));
                if continues {
                    i = j;
                    continue;
                }
                break;
            }
            if fence_char(line).is_some() || header_cells(line).is_some() {
                break;
            }
            let Some(cells) = split_row(line, Some(width)) else {
                break; // no pipe: the table is over
            };
            i += 1;
            if is_delimiter(&cells) || cells.iter().all(|c| c.is_empty()) {
                continue;
            }
            rows.push(Row {
                line_no,
                raw: line.to_string(),
                cells,
            });
        }
        tables.push(Table {
            header_line,
            header,
            rows,
        });
    }
    tables
}

/// Pipe rows outside fenced code that none of `tables` holds: rows cut off from
/// their header by a heading, a comment or a prose line. Delimiter rows, all-empty
/// rows, one-cell lines and the rows of other tables (a row followed by a
/// delimiter row starts one) aren't included.
pub(crate) fn stray_rows(lines: &[(usize, &str)], tables: &[Table]) -> Vec<Row> {
    let held: std::collections::HashSet<usize> = tables
        .iter()
        .flat_map(|t| std::iter::once(t.header_line).chain(t.rows.iter().map(|r| r.line_no)))
        .collect();
    let mut out = Vec::new();
    let mut fence = None;
    let mut in_other_table = false;
    for (i, &(line_no, line)) in lines.iter().enumerate() {
        if in_code(&mut fence, line)
            || !line.trim_start().starts_with('|')
            || held.contains(&line_no)
        {
            in_other_table = false;
            continue;
        }
        let cells = split_row(line, None).unwrap_or_default();
        let starts_table = lines
            .get(i + 1)
            .and_then(|(_, next)| split_row(next, None))
            .is_some_and(|next| is_delimiter(&next));
        if starts_table {
            in_other_table = true;
            continue;
        }
        if in_other_table
            || is_delimiter(&cells)
            || cells.len() < 2
            || cells.iter().all(|c| c.is_empty())
        {
            continue;
        }
        out.push(Row {
            line_no,
            raw: line.to_string(),
            cells,
        });
    }
    out
}

/// An ATX heading (`## Title`): its level and text, or `None`.
pub(crate) fn heading(line: &str) -> Option<(usize, String)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = line.trim_start_matches(' ');
    let level = rest.len() - rest.trim_start_matches('#').len();
    if level == 0 || level > 6 {
        return None;
    }
    let after = &rest[level..]; // '#' is ASCII, so `level` is a char boundary
    if !after.is_empty() && !after.starts_with([' ', '\t']) {
        return None;
    }
    let text = after.trim();
    // An optional closing sequence of '#' preceded by a space.
    let text = match text.trim_end_matches('#') {
        stripped
            if stripped.len() < text.len()
                && (stripped.is_empty() || stripped.ends_with([' ', '\t'])) =>
        {
            stripped.trim()
        }
        _ => text,
    };
    Some((level, text.to_string()))
}

/// The document's headings outside fenced code: (index into `lines`, level, text).
pub(crate) fn headings(lines: &[(usize, &str)]) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let mut fence = None;
    for (i, (_, line)) in lines.iter().enumerate() {
        if !in_code(&mut fence, line) {
            if let Some((level, text)) = heading(line) {
                out.push((i, level, text));
            }
        }
    }
    out
}

/// The trimmed body of the first section whose heading (any level) matches one of
/// `names` (case-insensitive), up to the next heading of the same or a higher level.
pub(crate) fn section(lines: &[(usize, &str)], names: &[&str]) -> Option<String> {
    let all = headings(lines);
    let (pos, (start, level, _)) = all
        .iter()
        .enumerate()
        .find(|(_, (_, _, text))| names.iter().any(|n| text.eq_ignore_ascii_case(n)))?;
    let end = all[pos + 1..]
        .iter()
        .find(|(_, l, _)| l <= level)
        .map_or(lines.len(), |(i, _, _)| *i);
    let body = lines[start + 1..end]
        .iter()
        .map(|(_, l)| *l)
        .collect::<Vec<_>>()
        .join("\n");
    Some(body.trim().to_string())
}

/// Where a marker block was found.
pub(crate) enum Block<'a> {
    /// The lines strictly between the start and end markers.
    Found(Vec<(usize, &'a str)>),
    NoStart,
    NoEnd,
}

/// The lines between the first `start` marker line and the first `end` marker
/// line after it (matching hr.py, which replaces the first such block).
pub(crate) fn marker_block<'a>(lines: &[(usize, &'a str)], start: &str, end: &str) -> Block<'a> {
    let Some(open) = lines.iter().position(|(_, l)| l.contains(start)) else {
        return Block::NoStart;
    };
    match lines[open + 1..].iter().position(|(_, l)| l.contains(end)) {
        Some(close) => Block::Found(lines[open + 1..open + 1 + close].to_vec()),
        None => Block::NoEnd,
    }
}

/// Parses a `---`-delimited frontmatter block at the very start of the text into
/// `key -> value` (flat keys only; nested YAML is ignored). Double-quoted values
/// are read as JSON strings (hr.py writes them with `json.dumps`), single-quoted
/// values with YAML's `''` escape. Returns `None` if there is no frontmatter.
pub(crate) fn frontmatter(text: &str) -> Option<BTreeMap<String, String>> {
    let lines = numbered_lines(text);
    if lines.first().map(|(_, l)| l.trim_end()) != Some("---") {
        return None;
    }
    let close = lines[1..].iter().position(|(_, l)| l.trim_end() == "---")?;
    let mut map = BTreeMap::new();
    for (_, line) in &lines[1..1 + close] {
        if line.starts_with([' ', '\t', '#', '-']) {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        map.entry(key.to_string())
            .or_insert_with(|| scalar(value.trim()));
    }
    Some(map)
}

fn scalar(value: &str) -> String {
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        if let Ok(s) = serde_json::from_str::<String>(value) {
            return s;
        }
        return value[1..value.len() - 1].to_string(); // '"' is ASCII: boundaries are safe
    }
    if value.len() >= 2 && value.starts_with('\'') && value.ends_with('\'') {
        return value[1..value.len() - 1].replace("''", "'");
    }
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells(line: &str) -> Vec<String> {
        split_row(line, None).unwrap()
    }

    #[test]
    fn splits_rows_with_and_without_outer_pipes() {
        assert_eq!(cells("| a | b |"), ["a", "b"]);
        assert_eq!(cells("a | b"), ["a", "b"]);
        assert_eq!(cells("| a | b"), ["a", "b"]);
        assert_eq!(cells("| a |  |"), ["a", ""]);
        assert_eq!(cells("|"), [""]);
        assert_eq!(cells("||"), [""]);
        assert!(split_row("no pipes", None).is_none());
    }

    #[test]
    fn escaped_pipes_stay_in_the_cell() {
        assert_eq!(cells(r"| a \| b | c |"), ["a | b", "c"]);
    }

    #[test]
    fn code_spans_keep_pipes_only_when_that_fits_the_header() {
        let line = "| 1 | run `a | b` now | done |";
        assert_eq!(
            split_row(line, Some(3)).unwrap(),
            ["1", "run `a | b` now", "done"]
        );
        assert_eq!(split_row(line, Some(4)).unwrap().len(), 4);
        // An unclosed backtick is literal.
        assert_eq!(split_row("| it`s | x |", Some(2)).unwrap(), ["it`s", "x"]);
    }

    #[test]
    fn delimiter_rows() {
        assert!(is_delimiter(&cells("| --- | :---: | ---: |")));
        assert!(!is_delimiter(&cells("| --- | x |")));
        assert!(!is_delimiter(&cells("| |")));
    }

    #[test]
    fn headings_and_sections() {
        assert_eq!(heading("# Title #"), Some((1, "Title".into())));
        assert_eq!(heading("## Goals"), Some((2, "Goals".into())));
        assert_eq!(heading("#hashtag"), None);
        assert_eq!(heading("####### seven"), None);
        assert_eq!(heading("#"), Some((1, String::new())));
        let text = "# P\n\n## Goals\n\nShip it.\n\n### Sub\nmore\n\n## Context\n```\n## not a heading\n```\nctx";
        let lines = numbered_lines(text);
        assert_eq!(
            section(&lines, &["goals"]).unwrap(),
            "Ship it.\n\n### Sub\nmore"
        );
        assert_eq!(
            section(&lines, &["Context"]).unwrap(),
            "```\n## not a heading\n```\nctx"
        );
        assert!(section(&lines, &["Team"]).is_none());
    }

    #[test]
    fn a_fence_closes_only_with_its_own_marker() {
        let text = "```\n~~~\n## inside\n```\n## outside";
        let found: Vec<_> = headings(&numbered_lines(text))
            .into_iter()
            .map(|(_, _, t)| t)
            .collect();
        assert_eq!(found, ["outside"]);
    }

    #[test]
    fn frontmatter_values() {
        let text = "---\nname: hr\ndescription: \"Says \\\"hi\\\"\"\nquote: 'it''s'\n  nested: x\n---\nbody";
        let fm = frontmatter(text).unwrap();
        assert_eq!(fm["name"], "hr");
        assert_eq!(fm["description"], "Says \"hi\"");
        assert_eq!(fm["quote"], "it's");
        assert!(!fm.contains_key("nested"));
        assert!(frontmatter("no frontmatter").is_none());
        assert!(frontmatter("---\nname: unclosed").is_none());
    }
}
