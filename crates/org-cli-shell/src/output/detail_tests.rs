//! The detail card and the dry-run preview in every mode.

use serde::Serialize;
use serde_json::json;

use super::*;
use crate::context::testing::{captured, tty, Captured};
use crate::context::{ColorMode, Modes};
use crate::terminal::Terminal;

fn never_color() -> Modes {
    Modes {
        color: ColorMode::Never,
        ..Modes::default()
    }
}

fn detail_fields() -> Vec<(&'static str, Cell)> {
    vec![
        ("id", Cell::id("off_1")),
        ("name", Cell::name("Head\noffice")),
        ("state", Cell::status("running", Style::Ok)),
    ]
}

fn render_detail(modes: Modes, terminal: Terminal) -> Captured {
    let (mut context, streams) = captured(modes, terminal, &[]);
    context
        .render_detail("off_1", &detail_fields(), &json!({"id": "off_1"}))
        .unwrap();
    streams
}

#[test]
fn detail_modes() {
    let json_mode = Modes {
        json: true,
        ..Modes::default()
    };
    let quiet = Modes {
        quiet: true,
        ..Modes::default()
    };
    assert_eq!(
        render_detail(json_mode, Terminal::piped()).stdout.text(),
        "{\"id\":\"off_1\"}\n"
    );
    assert_eq!(
        render_detail(quiet.clone(), tty(None)).stdout.text(),
        "off_1\n"
    );
    // NB-04: a record without an ID prints nothing under `-q`, never an empty line.
    let (mut context, streams) = captured(quiet, Terminal::piped(), &[]);
    context
        .render_detail("", &detail_fields(), &json!({}))
        .unwrap();
    assert_eq!(streams.stdout.text(), "");
    assert_eq!(
        render_detail(Modes::default(), Terminal::piped())
            .stdout
            .text(),
        "id\toff_1\nname\tHead\\u{a}office\nstate\trunning\n"
    );
    assert_eq!(
        render_detail(never_color(), tty(None)).stdout.text(),
        "ID     off_1\nNAME   Head\\u{a}office\nSTATE  ✓ running\n"
    );
    let colored = render_detail(Modes::default(), tty(None)).stdout.text();
    assert!(colored.contains("\u{1b}[") && colored.contains("Head\\u{a}office"));
}

#[derive(Serialize)]
struct Plan {
    office: &'static str,
    action: &'static str,
    count: u32,
}

const PLAN: Plan = Plan {
    office: "hq",
    action: "remove",
    count: 2,
};

#[test]
fn dry_run_json_keeps_field_order() {
    let json_mode = Modes {
        json: true,
        dry_run: true,
        ..Modes::default()
    };
    let (mut context, streams) = captured(json_mode, Terminal::piped(), &[]);
    context.render_dry_run("remove office hq", &PLAN).unwrap();
    assert_eq!(
        streams.stdout.text(),
        "{\"dry_run\":true,\"office\":\"hq\",\"action\":\"remove\",\"count\":2}\n"
    );
    assert_eq!(streams.stderr.text(), "");
}

#[test]
fn dry_run_for_people_and_quiet() {
    let (mut context, streams) = captured(never_color(), tty(None), &[]);
    context
        .render_dry_run("remove office \x1b[1mhq", &PLAN)
        .unwrap();
    assert_eq!(streams.stdout.text(), "");
    assert_eq!(
        streams.stderr.text(),
        "[DRY-RUN] remove office \\u{1b}[1mhq\n"
    );
    let (mut context, streams) = captured(Modes::default(), tty(None), &[]);
    context.render_dry_run("x", &PLAN).unwrap();
    assert!(
        streams.stderr.text().contains("\u{1b}["),
        "bold magenta tag"
    );
    let quiet = Modes {
        quiet: true,
        ..Modes::default()
    };
    let (mut context, streams) = captured(quiet, tty(None), &[]);
    context.render_dry_run("x", &PLAN).unwrap();
    assert_eq!(streams.stdout.text(), "");
    assert_eq!(streams.stderr.text(), "");
}

#[test]
fn dry_run_of_a_non_map_plan_is_an_error() {
    let json_mode = Modes {
        json: true,
        ..Modes::default()
    };
    let (mut context, _) = captured(json_mode, Terminal::piped(), &[]);
    assert!(matches!(
        context.render_dry_run("x", &5),
        Err(ShellError::Io { .. })
    ));
}
