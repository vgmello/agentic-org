//! Rendering a [`Table`] or a detail for a terminal (`tabled`) or a pipe (TSV / compact).
//!
//! On a narrow terminal (such as an 80-column TTY), columns are budgeted by priority
//! ([`column_priority`]): low-signal columns (`ID`, timestamps, `NOTES`, `OFFICE`, `*_ID`) shrink
//! and elide BEFORE distinguishing identifiers (`HANDLE`, `NAME`, `KEY`, `SLUG`, `EMAIL`, `TITLE`,
//! `STATUS`, `ROLE`) are truncated, so `@backend` and `@backend-2` remain clearly distinguishable.

use console::measure_text_width;
use owo_colors::OwoColorize;
use tabled::builder::Builder;
use tabled::settings::object::{Columns, Segment};
use tabled::settings::peaker::PriorityMax;
use tabled::settings::{Padding, Style as TableStyle, Width};

use super::{Cell, Table};
use crate::escape::sanitize;

/// The widest a terminal cell gets before it is cut with `...`.
pub(super) const CELL_MAX_WIDTH: usize = 48;
/// What a cut cell ends with.
const ELLIPSIS: &str = "...";
/// Right padding added to non-final columns on a terminal.
const COLUMN_GUTTER: usize = 3;
/// Total visual spacing between adjacent columns: `COLUMN_GUTTER` right padding plus the 1-cell
/// vertical separator from `TableStyle::blank()`.
const EFFECTIVE_GUTTER: usize = COLUMN_GUTTER + 1;

/// Protection tier for a terminal table column when fitting into a narrow terminal width.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ColumnPriority {
    /// Identifiers/timestamps/secondary metadata (`ID`, `*_ID`, `CREATED`, `UPDATED`, `NOTES`, `OFFICE`).
    Low = 0,
    /// General context columns (`DESCRIPTION`, `ADDRESS`, `BRANCH`, `TEAMS`, ...).
    Medium = 1,
    /// Core operational state (`STATUS`, `STATE`, `PRESENCE`, `ROLE`, `KIND`, `PRIORITY`, ...).
    High = 2,
    /// Primary distinguishing identifiers (`HANDLE`, `NAME`, `KEY`, `SLUG`, `EMAIL`, `TITLE`, ...).
    Critical = 3,
}

impl ColumnPriority {
    /// Minimum soft width protected during Phase 1 before lower-priority columns begin eliding.
    fn soft_floor(self) -> usize {
        match self {
            Self::Low => 8,
            Self::Medium => 12,
            Self::High => 12,
            Self::Critical => 24,
        }
    }
}

/// Normalizes a column header (`team_id` / `team-id` / `Team  Id` -> `"TEAM ID"`).
fn normalize_header(header: &str) -> String {
    let mut out = String::with_capacity(header.len());
    let mut prev_space = true;
    for c in sanitize(header).chars() {
        let mapped = match c {
            '_' | '-' => ' ',
            other if other.is_whitespace() => ' ',
            other => other.to_ascii_uppercase(),
        };
        if mapped == ' ' {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(mapped);
            prev_space = false;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Classifies a column header into its truncation-protection [`ColumnPriority`].
pub fn column_priority(header: &str) -> ColumnPriority {
    let key = normalize_header(header);
    match key.as_str() {
        "HANDLE" | "NAME" | "DISPLAY NAME" | "TITLE" | "KEY" | "SLUG" | "EMAIL" | "REFERENCE"
        | "COMMAND" | "CHANNEL" => ColumnPriority::Critical,
        "STATUS" | "STATE" | "PRESENCE" | "LIFECYCLE" | "ROLE" | "ROLE TITLE" | "KIND"
        | "PRIORITY" | "TYPE" | "SUBJECT" | "FROM" | "TO" | "ASSIGNEE" | "OWNER" | "SCOPE"
        | "PHASE" => ColumnPriority::High,
        "ID" | "CREATED" | "UPDATED" | "LAST SEEN" | "EXPIRES" | "AT" | "TIMESTAMP"
        | "SNOOZED UNTIL" | "WAIT UNTIL" | "NOTES" | "NOTE" | "OFFICE" | "MANAGED HOME"
        | "WORKSPACE PATH" | "HOSTNAME" | "FINGERPRINT" | "PUBKEY FINGERPRINT" | "SHA256"
        | "PAYLOAD SHA256" | "REPORTS TO" | "CAVEAT" | "REASON" | "CLOSE REASON"
        | "BLOCKED REASON" | "RATIONALE" => ColumnPriority::Low,
        other if other.ends_with(" ID") || other.ends_with(" AT") => ColumnPriority::Low,
        _ => ColumnPriority::Medium,
    }
}

/// Headerless TSV: one line per row, cells sanitized (a tab or newline inside a value is
/// escaped, so it cannot split a column or a row), no color, glyph or truncation.
pub(super) fn render_tsv(table: &Table) -> String {
    let mut out = String::new();
    for row in &table.rows {
        let cells: Vec<String> = row
            .cells
            .iter()
            .map(|cell| sanitize(&cell.text).into_owned())
            .collect();
        out.push_str(&cells.join("\t"));
        out.push('\n');
    }
    out
}

/// Column budgeting state for fitting a terminal table into `max_width`.
struct ColumnPlan {
    priorities: Vec<ColumnPriority>,
    capped: Vec<usize>,
    widths: Vec<usize>,
    kept: Vec<usize>,
}

impl ColumnPlan {
    fn new(headers: &[String], natural: &[usize]) -> Self {
        let priorities = headers.iter().map(|h| column_priority(h)).collect();
        let capped: Vec<usize> = natural.iter().map(|&w| w.min(CELL_MAX_WIDTH)).collect();
        let widths = capped.clone();
        let kept = (0..headers.len()).collect();
        Self {
            priorities,
            capped,
            widths,
            kept,
        }
    }

    fn total_width(&self) -> usize {
        let content: usize = self.kept.iter().map(|&i| self.widths[i]).sum();
        content + EFFECTIVE_GUTTER * self.kept.len().saturating_sub(1)
    }

    fn has_tier(&self, tier: ColumnPriority) -> bool {
        self.kept.iter().any(|&i| self.priorities[i] == tier)
    }

    fn shrink_tier(&mut self, tier: ColumnPriority, floor: usize, max_width: usize) {
        while self.total_width() > max_width {
            let candidate = self
                .kept
                .iter()
                .copied()
                .filter(|&i| self.priorities[i] == tier && self.widths[i] > floor)
                .max_by_key(|&i| (self.widths[i], i));
            let Some(idx) = candidate else { break };
            self.widths[idx] -= 1;
        }
    }

    fn elide_tier(&mut self, tier: ColumnPriority, max_width: usize) {
        while self.total_width() > max_width && self.kept.len() > 1 {
            let Some(pos) = self.kept.iter().rposition(|&i| self.priorities[i] == tier) else {
                break;
            };
            self.kept.remove(pos);
        }
    }

    fn shrink_any(&mut self, max_width: usize) {
        while self.total_width() > max_width {
            let candidate = self
                .kept
                .iter()
                .copied()
                .filter(|&i| self.widths[i] > 1)
                .max_by_key(|&i| (std::cmp::Reverse(self.priorities[i]), self.widths[i], i));
            let Some(idx) = candidate else { break };
            self.widths[idx] -= 1;
        }
    }

    fn reclaim_slack(&mut self, max_width: usize) {
        while self.total_width() < max_width {
            let candidate = self
                .kept
                .iter()
                .copied()
                .filter(|&i| self.widths[i] < self.capped[i])
                .max_by_key(|&i| {
                    (
                        self.priorities[i],
                        self.capped[i] - self.widths[i],
                        std::cmp::Reverse(i),
                    )
                });
            let Some(idx) = candidate else { break };
            self.widths[idx] += 1;
        }
    }

    fn finish(self) -> Vec<(usize, usize)> {
        self.kept.into_iter().map(|i| (i, self.widths[i])).collect()
    }
}

/// Computes the kept column indices and their content width budgets for `max_width`.
///
/// Elides `Low`-priority metadata columns BEFORE shrinking `Critical` columns down to their
/// 24-cell soft floor so distinguishing handles, names, and titles keep full width whenever
/// dropping low-signal columns makes room.
fn plan_columns(headers: &[String], natural: &[usize], max_width: usize) -> Vec<(usize, usize)> {
    if headers.is_empty() {
        return Vec::new();
    }
    let mut plan = ColumnPlan::new(headers, natural);
    if plan.total_width() <= max_width {
        return plan.finish();
    }
    plan.shrink_tier(
        ColumnPriority::Low,
        ColumnPriority::Low.soft_floor(),
        max_width,
    );
    plan.shrink_tier(
        ColumnPriority::Medium,
        ColumnPriority::Medium.soft_floor(),
        max_width,
    );
    plan.shrink_tier(
        ColumnPriority::High,
        ColumnPriority::High.soft_floor(),
        max_width,
    );
    plan.elide_tier(ColumnPriority::Low, max_width);
    plan.shrink_tier(
        ColumnPriority::Critical,
        ColumnPriority::Critical.soft_floor(),
        max_width,
    );
    plan.shrink_tier(ColumnPriority::Medium, 6, max_width);
    plan.elide_tier(ColumnPriority::Medium, max_width);
    plan.shrink_tier(ColumnPriority::High, 6, max_width);
    if plan.has_tier(ColumnPriority::Critical) {
        plan.elide_tier(ColumnPriority::High, max_width);
    }
    plan.shrink_any(max_width);
    plan.reclaim_slack(max_width);
    plan.finish()
}

fn measure_natural_widths(upper_headers: &[String], table: &Table) -> Vec<usize> {
    let col_count = upper_headers.len();
    let mut natural: Vec<usize> = upper_headers
        .iter()
        .map(|h| measure_text_width(h))
        .collect();
    for row in &table.rows {
        for (idx, cell) in row.cells.iter().enumerate().take(col_count) {
            let plain = cell.render(true, false);
            natural[idx] = natural[idx].max(measure_text_width(&plain));
        }
    }
    natural
}

fn apply_column_budgets(grid: &mut tabled::Table, plan: &[(usize, usize)], natural: &[usize]) {
    for (out_idx, &(col, alloc)) in plan.iter().enumerate() {
        let cap = natural[col].min(CELL_MAX_WIDTH);
        if alloc < cap {
            let trunc = if alloc > ELLIPSIS.len() {
                Width::truncate(alloc).suffix(ELLIPSIS)
            } else {
                Width::truncate(alloc)
            };
            grid.modify(Columns::new(out_idx..=out_idx), trunc);
        }
    }
}

/// A borderless table with UPPERCASE headers: cells sanitized, then given their glyph and
/// (when `color`) their color, each cut to [`CELL_MAX_WIDTH`] columns, and the whole table
/// fit to `width` by protecting high-priority distinguishing columns before shrinking or eliding
/// low-priority metadata columns.
pub(super) fn render_tty(table: &Table, color: bool, width: Option<usize>) -> String {
    let upper_headers: Vec<String> = table
        .headers
        .iter()
        .map(|header| sanitize(header).to_uppercase())
        .collect();
    if upper_headers.is_empty() {
        return String::new();
    }

    let natural = measure_natural_widths(&upper_headers, table);
    let plan: Vec<(usize, usize)> = match width {
        Some(max_width) => plan_columns(&table.headers, &natural, max_width),
        None => natural
            .iter()
            .enumerate()
            .map(|(i, &w)| (i, w.min(CELL_MAX_WIDTH)))
            .collect(),
    };

    let mut builder = Builder::default();
    builder.push_record(plan.iter().map(|&(col, _)| upper_headers[col].as_str()));
    for row in &table.rows {
        builder.push_record(plan.iter().map(|&(col, _)| {
            row.cells
                .get(col)
                .map_or_else(String::new, |cell| cell.render(true, color))
        }));
    }
    let mut grid = builder.build();
    let last = grid.count_columns().saturating_sub(1);
    grid.with(TableStyle::blank()).with(Padding::zero()).modify(
        Segment::all(),
        Width::truncate(CELL_MAX_WIDTH).suffix(ELLIPSIS),
    );
    apply_column_budgets(&mut grid, &plan, &natural);
    grid.modify(Columns::new(..last), Padding::new(0, COLUMN_GUTTER, 0, 0));
    if let Some(max_width) = width {
        let trunc = if max_width > ELLIPSIS.len() {
            Width::truncate(max_width).suffix(ELLIPSIS)
        } else {
            Width::truncate(max_width)
        };
        grid.with(trunc.priority(PriorityMax::default()));
    }
    let mut out = String::new();
    for line in grid.to_string().lines() {
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

/// An aligned `KEY  value` card: keys UPPERCASE (dimmed with color), values sanitized and
/// styled.
pub(super) fn render_card(fields: &[(&str, Cell)], color: bool) -> String {
    let keys: Vec<String> = fields
        .iter()
        .map(|(key, _)| sanitize(key).to_uppercase())
        .collect();
    let key_width = keys
        .iter()
        .map(|key| key.chars().count())
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for (key, (_, value)) in keys.iter().zip(fields) {
        let padded = format!("{key:<key_width$}");
        let key = if color {
            padded.dimmed().to_string()
        } else {
            padded
        };
        let line = format!("{key}  {}", value.render(true, color));
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

/// `key\tvalue` lines, both sanitized, no glyph or color.
pub(super) fn render_pairs(fields: &[(&str, Cell)]) -> String {
    fields
        .iter()
        .map(|(key, value)| format!("{}\t{}\n", sanitize(key), sanitize(&value.text)))
        .collect()
}

/// `key: value` lines (compact machine-readable record form), both sanitized, no glyph or color.
pub(super) fn render_compact_pairs(fields: &[(&str, Cell)]) -> String {
    fields
        .iter()
        .map(|(key, value)| format!("{}: {}\n", sanitize(key), sanitize(&value.text)))
        .collect()
}
