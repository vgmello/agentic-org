//! Rendering a [`Table`] or a detail for a terminal (`tabled`) or a pipe (TSV).

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

/// A borderless table with UPPERCASE headers: cells sanitized, then given their glyph and
/// (when `color`) their color, each cut to [`CELL_MAX_WIDTH`] columns, and the whole table
/// cut to `width` (the widest column first).
pub(super) fn render_tty(table: &Table, color: bool, width: Option<usize>) -> String {
    let mut builder = Builder::default();
    builder.push_record(
        table
            .headers
            .iter()
            .map(|header| sanitize(header).to_uppercase()),
    );
    for row in &table.rows {
        builder.push_record(row.cells.iter().map(|cell| cell.render(true, color)));
    }
    let mut grid = builder.build();
    let last = grid.count_columns().saturating_sub(1);
    grid.with(TableStyle::blank())
        .with(Padding::zero())
        .modify(Columns::new(..last), Padding::new(0, 3, 0, 0))
        .modify(
            Segment::all(),
            Width::truncate(CELL_MAX_WIDTH).suffix(ELLIPSIS),
        );
    if let Some(width) = width {
        grid.with(
            Width::truncate(width)
                .suffix(ELLIPSIS)
                .priority(PriorityMax::default()),
        );
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
