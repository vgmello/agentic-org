use serde_json::json;

use super::*;
use crate::context::testing::{captured, tty, Captured};
use crate::context::{ColorMode, Modes};
use crate::terminal::Terminal;

struct Office {
    id: &'static str,
    name: &'static str,
    state: (&'static str, Style),
}

impl ToTableRow for Office {
    const HEADERS: &'static [&'static str] = &["id", "name", "state"];

    fn primary_id(&self) -> String {
        self.id.to_owned()
    }

    fn cells(&self) -> Vec<Cell> {
        vec![
            Cell::id(self.id),
            Cell::name(self.name),
            Cell::status(self.state.0, self.state.1),
        ]
    }
}

fn offices() -> Vec<Office> {
    vec![
        Office {
            id: "off_1",
            name: "Head\toffice",
            state: ("running", Style::Active),
        },
        Office {
            id: "off_2\x1b[2J",
            name: "Branch",
            state: ("stopped", Style::Warn),
        },
    ]
}

fn offices_json() -> Value {
    json!([{"id": "off_1", "name": "Head\toffice"}, {"id": "off_2", "name": "Branch"}])
}

fn never_color() -> Modes {
    Modes {
        color: ColorMode::Never,
        ..Modes::default()
    }
}

fn render(modes: Modes, terminal: Terminal, view: &ListView<'_>) -> Captured {
    let (mut context, streams) = captured(modes, terminal, &[]);
    context
        .render_list(&offices(), &offices_json(), view)
        .unwrap();
    streams
}

#[test]
fn json_is_the_callers_value() {
    let json_mode = Modes {
        json: true,
        quiet: false,
        ..Modes::default()
    };
    let streams = render(json_mode.clone(), Terminal::piped(), &ListView::default());
    assert_eq!(
        streams.stdout.text(),
        format!("{}\n", serde_json::to_string(&offices_json()).unwrap())
    );
    assert_eq!(streams.stderr.text(), "");
    let pretty = render(json_mode, tty(Some(80)), &ListView::default());
    assert!(pretty.stdout.text().lines().count() > 2);
    assert!(!pretty.stdout.text().contains('\u{1b}'), "no color in JSON");
}

#[test]
fn json_output_escapes_c1() {
    let (mut context, streams) = captured(Modes::default(), Terminal::piped(), &[]);
    context.write_json(&json!({"name": "a\u{9b}b"})).unwrap();
    assert_eq!(streams.stdout.text(), "{\"name\":\"a\\u009bb\"}\n");
}

#[test]
fn quiet_prints_sanitized_ids() {
    let quiet = Modes {
        quiet: true,
        ..Modes::default()
    };
    let streams = render(quiet.clone(), tty(Some(80)), &ListView::default());
    assert_eq!(streams.stdout.text(), "off_1\noff_2\\u{1b}[2J\n");
    let (mut context, streams) = captured(quiet, tty(None), &[]);
    let view = ListView {
        empty: "No offices.",
        total: None,
    };
    context
        .render_list::<Office>(&[], &json!([]), &view)
        .unwrap();
    assert_eq!(streams.stdout.text(), "");
    assert_eq!(streams.stderr.text(), "");
}

#[test]
fn piped_is_headerless_tsv() {
    let streams = render(Modes::default(), Terminal::piped(), &ListView::default());
    assert_eq!(
        streams.stdout.text(),
        "off_1\tHead\\u{9}office\trunning\noff_2\\u{1b}[2J\tBranch\tstopped\n"
    );
    let forced = Modes {
        color: ColorMode::Always,
        ..Modes::default()
    };
    let streams = render(forced, Terminal::piped(), &ListView::default());
    assert!(!streams.stdout.text().contains('\u{1b}'), "no ANSI in TSV");
    assert!(!streams.stdout.text().contains('●'), "no glyphs in TSV");
}

#[test]
fn piped_cells_are_not_truncated() {
    let long = "x".repeat(200);
    let table = Table {
        headers: vec!["name".into()],
        rows: vec![Row {
            id: "1".into(),
            cells: vec![Cell::plain(long.clone())],
        }],
    };
    let (mut context, streams) = captured(Modes::default(), Terminal::piped(), &[]);
    context
        .render_table(&table, &Value::Null, &ListView::default())
        .unwrap();
    assert_eq!(streams.stdout.text(), format!("{long}\n"));
}

#[test]
fn terminal_table_without_color() {
    let streams = render(never_color(), tty(Some(120)), &ListView::default());
    let text = streams.stdout.text();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "{text}");
    assert!(
        lines[0].starts_with("ID") && lines[0].contains("NAME"),
        "{text}"
    );
    assert!(lines[0].contains("STATE"), "{text}");
    assert!(
        lines[1].contains("● running") && lines[2].contains("! stopped"),
        "{text}"
    );
    assert!(lines[1].contains("Head\\u{9}office"), "{text}");
    assert!(lines[2].contains("off_2\\u{1b}[2J"), "{text}");
    assert!(!text.contains('\u{1b}'), "no ANSI: {text:?}");
    assert!(
        lines.iter().all(|line| line == &line.trim_end()),
        "{text:?}"
    );
    let name_column = lines[0].find("NAME").unwrap();
    assert_eq!(lines[1].find("Head"), Some(name_column), "aligned: {text}");
}

#[test]
fn terminal_table_with_color() {
    let streams = render(Modes::default(), tty(Some(120)), &ListView::default());
    let text = streams.stdout.text();
    assert!(text.contains("\u{1b}["), "{text:?}");
    assert!(text.contains("ID"), "{text}");
    // Values were sanitized before they were styled: the only ESC bytes are the styling's.
    assert!(text.contains("off_2\\u{1b}[2J"), "{text:?}");
}

#[test]
fn terminal_cells_are_cut_to_48_columns() {
    let table = Table {
        headers: vec!["id".into(), "note".into()],
        rows: vec![Row {
            id: "1".into(),
            cells: vec![Cell::plain("1"), Cell::plain("y".repeat(100))],
        }],
    };
    let (mut context, streams) = captured(never_color(), tty(Some(500)), &[]);
    context
        .render_table(&table, &Value::Null, &ListView::default())
        .unwrap();
    let text = streams.stdout.text();
    let row = text.lines().nth(1).unwrap();
    assert!(row.ends_with("..."), "{text}");
    assert!(
        row.contains(&"y".repeat(45)) && !row.contains(&"y".repeat(46)),
        "{text}"
    );
}

#[test]
fn terminal_table_fits_the_width() {
    let table = Table {
        headers: vec!["id".into(), "description".into()],
        rows: vec![Row {
            id: "1".into(),
            cells: vec![Cell::id("short"), Cell::plain("z".repeat(40))],
        }],
    };
    let (mut context, streams) = captured(never_color(), tty(Some(30)), &[]);
    context
        .render_table(&table, &Value::Null, &ListView::default())
        .unwrap();
    let text = streams.stdout.text();
    assert!(
        text.lines().all(|line| line.chars().count() <= 30),
        "{text}"
    );
    assert!(
        text.contains("short"),
        "the widest column is cut first: {text}"
    );
    assert!(text.contains("..."), "{text}");
}

#[test]
fn empty_list_note_only_on_a_terminal() {
    let view = ListView {
        empty: "No offices yet.",
        total: None,
    };
    for (terminal, note) in [(tty(None), "No offices yet.\n"), (Terminal::piped(), "")] {
        let (mut context, streams) = captured(Modes::default(), terminal, &[]);
        context
            .render_list::<Office>(&[], &json!([]), &view)
            .unwrap();
        assert_eq!(streams.stdout.text(), "");
        assert_eq!(streams.stderr.text(), note);
    }
    let json_mode = Modes {
        json: true,
        ..Modes::default()
    };
    let (mut context, streams) = captured(json_mode, tty(None), &[]);
    context
        .render_list::<Office>(&[], &json!([]), &view)
        .unwrap();
    assert_eq!(streams.stdout.text(), "[]\n");
    assert_eq!(streams.stderr.text(), "");
}

#[test]
fn showing_n_of_total_on_a_terminal() {
    let view = ListView {
        empty: "",
        total: Some(10),
    };
    let streams = render(never_color(), tty(Some(80)), &view);
    assert_eq!(streams.stderr.text(), "Showing 2 of 10\n");
    let streams = render(never_color(), Terminal::piped(), &view);
    assert_eq!(streams.stderr.text(), "");
    let all = ListView {
        empty: "",
        total: Some(2),
    };
    let streams = render(never_color(), tty(Some(80)), &all);
    assert_eq!(streams.stderr.text(), "");
}

#[test]
fn glyphs() {
    assert_eq!(glyph(Style::Ok), "✓ ");
    assert_eq!(glyph(Style::Active), "● ");
    assert_eq!(glyph(Style::Pending), "◐ ");
    assert_eq!(glyph(Style::Warn), "! ");
    assert_eq!(glyph(Style::Error), "× ");
    assert_eq!(glyph(Style::Id), "");
    assert_eq!(Cell::dim("x").style, Style::Dim);
    assert_eq!(Cell::plain("x").render(true, true), "x");
}

#[test]
fn closed_stdout_is_reported() {
    let mut context = ExecutionContext::with_streams(
        Modes::default(),
        Terminal::piped(),
        &|_| None,
        Box::new(crate::context::testing::ClosedPipe),
        Box::new(io::sink()),
    );
    let result = context.render_list(&offices(), &Value::Null, &ListView::default());
    assert!(matches!(result, Err(ShellError::OutputClosed)));
}
