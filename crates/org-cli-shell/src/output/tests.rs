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

/// NB-04: under `-q` a row without an ID is left out, never printed as an empty line.
#[test]
fn quiet_skips_rows_without_an_id() {
    let quiet = Modes {
        quiet: true,
        ..Modes::default()
    };
    let row = |id: &str| Row {
        id: id.to_owned(),
        cells: vec![Cell::new("x", Style::Plain)],
    };
    let table = Table {
        headers: vec!["name".to_owned()],
        rows: vec![row(""), row("off_1"), row("")],
    };
    let (mut context, streams) = captured(quiet.clone(), Terminal::piped(), &[]);
    context
        .render_table(&table, &json!([]), &ListView::default())
        .unwrap();
    assert_eq!(streams.stdout.text(), "off_1\n");
    let only_empty = Table {
        headers: vec!["name".to_owned()],
        rows: vec![row("")],
    };
    let (mut context, streams) = captured(quiet, Terminal::piped(), &[]);
    context
        .render_table(&only_empty, &json!([]), &ListView::default())
        .unwrap();
    assert_eq!(streams.stdout.text(), "");
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

/// NB-05: a status on a line of its own has its glyph, its color only with color, and its
/// text sanitized.
#[test]
fn a_status_line_has_its_glyph_and_color() {
    let running = Cell::status("running (pid 42)", Style::Active);
    assert_eq!(running.line(false), "● running (pid 42)");
    let colored = running.line(true);
    assert!(colored.starts_with("\u{1b}[32m● running"), "{colored:?}");
    assert_eq!(
        Cell::status("x\u{1b}[2J", Style::Error).line(false),
        "× x\\u{1b}[2J"
    );
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

#[test]
fn eighty_column_tty_table_protects_distinguishing_handles_over_low_priority_columns() {
    let table = Table {
        headers: vec![
            "id".into(),
            "handle".into(),
            "name".into(),
            "role".into(),
            "status".into(),
            "office".into(),
            "team_id".into(),
            "updated".into(),
            "notes".into(),
        ],
        rows: vec![
            Row {
                id: "0193a5c0-0000-7000-8000-000000000001".into(),
                cells: vec![
                    Cell::id("0193a5c0-0000-7000-8000-000000000001"),
                    Cell::name("@backend"),
                    Cell::name("Backend Agent"),
                    Cell::plain("Backend Engineer"),
                    Cell::status("online", Style::Active),
                    Cell::dim("hq-macbook-pro"),
                    Cell::dim("0193a5c0-0000-7000-8000-0000000000aa"),
                    Cell::dim("2026-11-27 14:00 UTC"),
                    Cell::plain("API and database services"),
                ],
            },
            Row {
                id: "0193a5c0-0000-7000-8000-000000000002".into(),
                cells: vec![
                    Cell::id("0193a5c0-0000-7000-8000-000000000002"),
                    Cell::name("@backend-2"),
                    Cell::name("Backend Agent 2"),
                    Cell::plain("Backend Engineer"),
                    Cell::status("online", Style::Active),
                    Cell::dim("worker-eu-linux"),
                    Cell::dim("0193a5c0-0000-7000-8000-0000000000aa"),
                    Cell::dim("2026-11-27 14:01 UTC"),
                    Cell::plain("Replication and worker queue"),
                ],
            },
        ],
    };
    let (mut context, streams) = captured(never_color(), tty(Some(80)), &[]);
    context
        .render_table(&table, &Value::Null, &ListView::default())
        .unwrap();
    let text = streams.stdout.text();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "{text}");
    assert!(
        lines.iter().all(|line| line.chars().count() <= 80),
        "exceeded 80 columns:\n{text}"
    );
    assert!(
        lines[1].contains("@backend") && !lines[1].contains("@backend-2"),
        "row 1 must show full @backend:\n{text}"
    );
    assert!(
        lines[2].contains("@backend-2"),
        "row 2 must keep full @backend-2 distinguishable (never bac...):\n{text}"
    );
    assert!(
        lines[1].contains("Backend Agent") && lines[2].contains("Backend Agent 2"),
        "names must stay distinguishable:\n{text}"
    );
    assert!(
        !lines[0].contains("NOTES") && !lines[0].contains("UPDATED"),
        "low-priority columns should elide before truncating handles:\n{text}"
    );
}

#[test]
fn agent_harness_pty_emits_untruncated_json_while_pipe_preserves_tsv() {
    let table = Table {
        headers: vec!["id".into(), "handle".into(), "notes".into()],
        rows: vec![
            Row {
                id: "1".into(),
                cells: vec![
                    Cell::id("0193a5c0-0000-7000-8000-000000000001"),
                    Cell::name("@backend"),
                    Cell::plain("x".repeat(120)),
                ],
            },
            Row {
                id: "2".into(),
                cells: vec![
                    Cell::id("0193a5c0-0000-7000-8000-000000000002"),
                    Cell::name("@backend-2"),
                    Cell::plain("y".repeat(120)),
                ],
            },
        ],
    };
    let payload = json!([
        {"id": "0193a5c0-0000-7000-8000-000000000001", "handle": "@backend", "notes": "x".repeat(120)},
        {"id": "0193a5c0-0000-7000-8000-000000000002", "handle": "@backend-2", "notes": "y".repeat(120)}
    ]);

    // 80-col PTY under CLAUDECODE -> untruncated single-line JSON!
    let (mut ctx, streams) = captured(Modes::default(), tty(Some(80)), &[("CLAUDECODE", "1")]);
    ctx.render_table(&table, &payload, &ListView::default())
        .unwrap();
    let stdout = streams.stdout.text();
    assert_eq!(
        stdout.lines().count(),
        1,
        "compact JSON in harness PTY: {stdout}"
    );
    let parsed: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed, payload);

    // Non-TTY pipe under CLAUDECODE -> preserves untruncated TSV!
    let (mut ctx, streams) = captured(Modes::default(), Terminal::piped(), &[("CLAUDECODE", "1")]);
    ctx.render_table(&table, &payload, &ListView::default())
        .unwrap();
    let tsv = streams.stdout.text();
    assert!(
        tsv.contains("@backend\t") && tsv.contains("@backend-2\t"),
        "{tsv}"
    );
    assert!(tsv.contains(&"x".repeat(120)), "piped TSV is untruncated");

    // AGENT_OS_FORMAT=yaml on TTY renders valid escaped YAML!
    let (mut ctx, streams) = captured(
        Modes::default(),
        tty(Some(80)),
        &[(crate::context::OUTPUT_FORMAT_ENV, "yaml")],
    );
    ctx.render_table(&table, &payload, &ListView::default())
        .unwrap();
    let yaml = streams.stdout.text();
    assert!(
        yaml.contains("handle: \"@backend\"") && yaml.contains("handle: \"@backend-2\""),
        "{yaml}"
    );
}

#[test]
fn low_priority_columns_elide_before_shrinking_critical_columns_to_24() {
    let title = "Migrate workspace lease renewal flow";
    assert_eq!(title.len(), 36);
    let table = Table {
        headers: vec![
            "handle".into(),
            "title".into(),
            "notes".into(),
            "updated".into(),
        ],
        rows: vec![Row {
            id: "1".into(),
            cells: vec![
                Cell::name("@backend-2"),
                Cell::name(title),
                Cell::plain("Secondary notes text here"),
                Cell::dim("2026-11-27 14:00 UTC"),
            ],
        }],
    };
    let (mut ctx, streams) = captured(never_color(), tty(Some(60)), &[]);
    ctx.render_table(&table, &Value::Null, &ListView::default())
        .unwrap();
    let text = streams.stdout.text();
    assert!(
        text.lines()
            .all(|line| console::measure_text_width(line) <= 60),
        "exceeded 60 cols:\n{text}"
    );
    assert!(
        text.contains(title),
        "Critical title must keep full 36 chars when Low columns can elide:\n{text}"
    );
    assert!(
        !text.contains("NOTES") && !text.contains("UPDATED"),
        "Low columns should elide before shrinking Critical title:\n{text}"
    );
}

#[test]
fn terminal_table_handles_zero_tiny_width_empty_rows_and_wide_unicode() {
    let sample = Table {
        headers: vec!["handle".into(), "title".into()],
        rows: vec![Row {
            id: "1".into(),
            cells: vec![Cell::name("@ada"), Cell::name("Hello world")],
        }],
    };
    for tiny in [0usize, 2] {
        let rendered = table::render_tty(&sample, false, Some(tiny));
        for line in rendered.lines() {
            assert!(
                console::measure_text_width(line) <= tiny,
                "width={tiny}: line {line:?} exceeded width"
            );
        }
    }

    // Empty rows directly through `render_tty` and through `render_table`.
    let empty_rows = Table {
        headers: vec!["handle".into(), "status".into()],
        rows: Vec::new(),
    };
    let header_only = table::render_tty(&empty_rows, false, Some(40));
    assert_eq!(header_only.lines().count(), 1, "{header_only:?}");
    let (mut ctx, streams) = captured(never_color(), tty(Some(40)), &[]);
    ctx.render_table(&empty_rows, &Value::Null, &ListView::default())
        .unwrap();
    assert!(streams.stdout.text().is_empty());

    // Multi-width CJK and emoji cells truncate cleanly at display-width boundaries without panic.
    let unicode = Table {
        headers: vec!["handle".into(), "title".into()],
        rows: vec![
            Row {
                id: "1".into(),
                cells: vec![Cell::name("@ada"), Cell::name("日本語テストタイトル確認")],
            },
            Row {
                id: "2".into(),
                cells: vec![Cell::name("@ben"), Cell::name("🚀🔥✨🎉🎯📦")],
            },
        ],
    };
    for width in [10usize, 11, 15] {
        let rendered = table::render_tty(&unicode, false, Some(width));
        assert_eq!(rendered.lines().count(), 3, "width={width}:\n{rendered}");
        for line in rendered.lines() {
            assert!(
                console::measure_text_width(line) <= width,
                "width={width}: line {line:?} width {} > {width}",
                console::measure_text_width(line)
            );
        }
    }
}
