//! The output modes of a command's result.
//!
//! | Mode | List | Detail |
//! | --- | --- | --- |
//! | `--json` | the caller's JSON value, pretty on a terminal | same |
//! | `-q` | one ID per line | the ID |
//! | piped | headerless TSV, every cell sanitized, no color, glyphs or truncation | `key\tvalue` lines |
//! | terminal | borderless table, UPPERCASE headers, status glyphs, color when on | aligned `KEY  value` card |
//!
//! An empty list prints nothing on standard output (and its note on a terminal's standard
//! error). Notes never go to standard output.

use std::io;

use owo_colors::OwoColorize;
use serde::Serialize;
use serde_json::Value;

use crate::context::{ExecutionContext, OutputFormat};
use crate::error::ShellError;
use crate::escape::{escape_json_controls, sanitize};

mod table;
mod yaml;

pub use table::{column_priority, ColumnPriority};

/// How a value is shown on a terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Plain,
    /// An identifier: cyan.
    Id,
    /// A name: bold.
    Name,
    /// Secondary: dimmed.
    Dim,
    /// Statuses: a glyph, then green / green / yellow / yellow / red text.
    Ok,
    Active,
    Pending,
    Warn,
    Error,
}

/// The glyph and space a status gets on a terminal (`✓ `, `● `, `◐ `, `! `, `× `); empty for
/// the other styles.
pub fn glyph(style: Style) -> &'static str {
    match style {
        Style::Ok => "✓ ",
        Style::Active => "● ",
        Style::Pending => "◐ ",
        Style::Warn => "! ",
        Style::Error => "× ",
        Style::Plain | Style::Id | Style::Name | Style::Dim => "",
    }
}

/// One value of a row or detail, with its style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    pub text: String,
    pub style: Style,
}

impl Cell {
    pub fn new(text: impl Into<String>, style: Style) -> Self {
        Self {
            text: text.into(),
            style,
        }
    }

    pub fn plain(text: impl Into<String>) -> Self {
        Self::new(text, Style::Plain)
    }

    pub fn id(text: impl Into<String>) -> Self {
        Self::new(text, Style::Id)
    }

    pub fn name(text: impl Into<String>) -> Self {
        Self::new(text, Style::Name)
    }

    pub fn dim(text: impl Into<String>) -> Self {
        Self::new(text, Style::Dim)
    }

    /// A status; `style` is one of the status styles (any style is accepted).
    pub fn status(text: impl Into<String>, style: Style) -> Self {
        Self::new(text, style)
    }

    /// The text sanitized with its status glyph, colored when `color`: a status shown on a
    /// line of its own on a terminal, styled as the cards and tables style it.
    pub fn line(&self, color: bool) -> String {
        self.render(true, color)
    }

    /// The text sanitized, with the status glyph when `glyphs`, colored when `color`.
    pub(crate) fn render(&self, glyphs: bool, color: bool) -> String {
        let prefix = if glyphs { glyph(self.style) } else { "" };
        let body = format!("{prefix}{}", sanitize(&self.text));
        if !color {
            return body;
        }
        match self.style {
            Style::Plain => body,
            Style::Id => body.cyan().to_string(),
            Style::Name => body.bold().to_string(),
            Style::Dim => body.dimmed().to_string(),
            Style::Ok | Style::Active => body.green().to_string(),
            Style::Pending | Style::Warn => body.yellow().to_string(),
            Style::Error => body.red().to_string(),
        }
    }
}

/// One row: its ID (what `-q` prints) and its cells, in header order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub cells: Vec<Cell>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Table {
    pub headers: Vec<String>,
    pub rows: Vec<Row>,
}

/// A type listed as a table.
pub trait ToTableRow {
    /// Column headers, shown UPPERCASE on a terminal.
    const HEADERS: &'static [&'static str];
    /// What `-q` prints for it.
    fn primary_id(&self) -> String;
    /// The cells, in [`Self::HEADERS`] order.
    fn cells(&self) -> Vec<Cell>;
}

impl Table {
    pub fn from_rows<T: ToTableRow>(items: &[T]) -> Table {
        Table {
            headers: T::HEADERS
                .iter()
                .map(|header| (*header).to_owned())
                .collect(),
            rows: items
                .iter()
                .map(|item| Row {
                    id: item.primary_id(),
                    cells: item.cells(),
                })
                .collect(),
        }
    }
}

/// How a list is introduced around the rows.
#[derive(Clone, Copy, Debug, Default)]
pub struct ListView<'a> {
    /// The note for an empty list (terminal standard error only), e.g. `No offices yet.`
    pub empty: &'a str,
    /// The total when the rows are one page of it: `Showing N of T` (terminal only).
    pub total: Option<usize>,
}

/// A dry-run preview: `dry_run` first, then the plan's own fields in their order.
#[derive(Serialize)]
struct DryRun<'a, T: ?Sized> {
    dry_run: bool,
    #[serde(flatten)]
    plan: &'a T,
}

impl ExecutionContext {
    /// `value` as JSON on standard output: indented on a human terminal, one compact line in a
    /// pipe or agent-harness PTY, DEL / C1 / line separators escaped, then a newline.
    pub fn write_json<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), ShellError> {
        let encoded = if self.is_tty_stdout() && !self.is_agent_harness() {
            serde_json::to_string_pretty(value)
        } else {
            serde_json::to_string(value)
        }
        .map_err(|e| ShellError::io("encoding the JSON output", io::Error::other(e)))?;
        let mut text = escape_json_controls(encoded);
        text.push('\n');
        self.out(&text)
    }

    /// `value` as deterministic, terminal-safe YAML on standard output.
    pub fn write_yaml(&mut self, value: &Value) -> Result<(), ShellError> {
        let text = yaml::render_yaml(value);
        self.out(&text)
    }

    /// Prints a list in the current mode (see the module table). `json` is printed as given
    /// under `--json` / `AGENT_OS_FORMAT=json` / agent-harness PTY, or as YAML under `yaml`;
    /// `view` supplies the empty-list note and the total.
    pub fn render_table(
        &mut self,
        table: &Table,
        json: &Value,
        view: &ListView<'_>,
    ) -> Result<(), ShellError> {
        if self.modes().quiet {
            // A row without an ID prints nothing, never an empty line.
            let ids: String = table
                .rows
                .iter()
                .filter(|row| !row.id.is_empty())
                .map(|row| format!("{}\n", sanitize(&row.id)))
                .collect();
            return if ids.is_empty() {
                Ok(())
            } else {
                self.out(&ids)
            };
        }
        match self.output_format() {
            OutputFormat::Json => return self.write_json(json),
            OutputFormat::Yaml => return self.write_yaml(json),
            OutputFormat::Compact | OutputFormat::Tsv => {
                if table.rows.is_empty() {
                    return Ok(());
                }
                let text = table::render_tsv(table);
                return self.out(&text);
            }
            OutputFormat::Table => {}
        }
        if table.rows.is_empty() {
            if self.is_tty_stderr() && !view.empty.is_empty() {
                self.note(view.empty);
            }
            return Ok(());
        }
        let text = table::render_tty(table, self.stdout_color(), self.term_width());
        self.out(&text)?;
        if let Some(total) = view.total.filter(|total| *total > table.rows.len()) {
            if self.is_tty_stderr() {
                self.note(&format!("Showing {} of {total}", table.rows.len()));
            }
        }
        Ok(())
    }

    /// [`Self::render_table`] of `items`.
    pub fn render_list<T: ToTableRow>(
        &mut self,
        items: &[T],
        json: &Value,
        view: &ListView<'_>,
    ) -> Result<(), ShellError> {
        self.render_table(&Table::from_rows(items), json, view)
    }

    /// Prints one record: `json` under JSON, YAML under YAML, `id` under `-q`, an aligned
    /// `KEY  value` card under table, `key: value` lines under compact, or `key\tvalue` lines
    /// under TSV.
    pub fn render_detail(
        &mut self,
        id: &str,
        fields: &[(&str, Cell)],
        json: &Value,
    ) -> Result<(), ShellError> {
        if self.modes().quiet {
            // A record without an ID prints nothing, never an empty line.
            if id.is_empty() {
                return Ok(());
            }
            return self.out(&format!("{}\n", sanitize(id)));
        }
        let text = match self.output_format() {
            OutputFormat::Json => return self.write_json(json),
            OutputFormat::Yaml => return self.write_yaml(json),
            OutputFormat::Compact => table::render_compact_pairs(fields),
            OutputFormat::Tsv => table::render_pairs(fields),
            OutputFormat::Table => table::render_card(fields, self.stdout_color()),
        };
        if text.is_empty() {
            return Ok(());
        }
        self.out(&text)
    }

    /// Previews a mutation that `--dry-run` skips. Under JSON standard output gets
    /// `{"dry_run":true, ..plan}` (`plan` must serialize as a map or struct, without its own
    /// `dry_run` key); under YAML the same structure as YAML; under `-q` nothing is printed;
    /// otherwise standard error gets `[DRY-RUN] summary` (bold magenta tag with color) and
    /// standard output nothing.
    pub fn render_dry_run<T: Serialize + ?Sized>(
        &mut self,
        summary: &str,
        plan: &T,
    ) -> Result<(), ShellError> {
        if self.modes().quiet {
            return Ok(());
        }
        match self.output_format() {
            OutputFormat::Json => {
                return self.write_json(&DryRun {
                    dry_run: true,
                    plan,
                });
            }
            OutputFormat::Yaml => {
                let value = serde_json::to_value(DryRun {
                    dry_run: true,
                    plan,
                })
                .map_err(|e| ShellError::io("encoding the YAML output", io::Error::other(e)))?;
                return self.write_yaml(&value);
            }
            OutputFormat::Compact | OutputFormat::Tsv | OutputFormat::Table => {}
        }
        let tag = if self.stderr_color() {
            "[DRY-RUN]".magenta().bold().to_string()
        } else {
            String::from("[DRY-RUN]")
        };
        self.err_raw(&format!("{tag} {}\n", sanitize(summary)));
        Ok(())
    }
}

#[cfg(test)]
mod detail_tests;
#[cfg(test)]
mod tests;
