//! Fuzz-ish robustness: every parser, and the loader, on garbage and on mutated
//! real-looking files. The only assertion that matters is "no panic", plus a few
//! invariants on what comes back.

mod common;

use std::fs;

use common::{fixture, TempDir};
use org_core::{
    classify, parse_agent_profile, parse_backlog, parse_project_brief, parse_project_profile,
    parse_role, parse_staff_function, parse_status, BacklogItem, Workspace,
};

/// xorshift64*: deterministic, so a failure is reproducible from the seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Pieces that exercise the parsers' edge cases: table syntax, escapes, code
/// spans and fences, headings, markers, frontmatter, JSON/TOML syntax, CRLF,
/// multi-byte and odd Unicode.
const FRAGMENTS: &[&str] = &[
    "|",
    "||",
    " | ",
    "\\|",
    "\\",
    "`",
    "``",
    "```",
    "~~~",
    "\n",
    "\r\n",
    "\r",
    " ",
    "\t",
    "-",
    "---",
    ":---:",
    "#",
    "# ",
    "## ",
    "###### ",
    "*",
    "**",
    "_",
    "<!--",
    "-->",
    "<!-- hr:team:start -->",
    "<!-- hr:team:end -->",
    "| # | Task | Status | Requested by | Notes |",
    "| --- | --- | --- | --- | --- |",
    "| Agent | Name | Role |",
    "todo",
    "in-progress",
    "in progress (",
    "blocked:",
    "done",
    "Done ✅",
    "x",
    "1",
    "#3",
    "4.",
    "99999999999999",
    "{",
    "}",
    "[",
    "]",
    "\"",
    "'",
    "''",
    ":",
    ",",
    "=",
    "\"\"\"",
    "[[",
    "]]",
    "null",
    "true",
    "\u{feff}",
    "é",
    "·",
    "—",
    "İ",
    "ß",
    "🦀",
    "\u{200b}",
    "\u{0}",
    "\u{7f}",
    "Backlog · ",
    "name: ",
    "description: ",
    "tools: ",
    "Goals",
    "Context",
];

fn garbage(rng: &mut Rng) -> String {
    let len = rng.below(120);
    (0..len)
        .map(|_| FRAGMENTS[rng.below(FRAGMENTS.len())])
        .collect()
}

/// Random edits of a real-looking text: deletions, duplications, insertions and
/// truncation, always at char boundaries.
fn mutate(rng: &mut Rng, text: &str) -> String {
    let mut chars: Vec<char> = text.chars().collect();
    for _ in 0..1 + rng.below(6) {
        if chars.is_empty() {
            break;
        }
        let at = rng.below(chars.len());
        let span = 1 + rng.below(20.min(chars.len() - at));
        match rng.below(4) {
            0 => {
                chars.drain(at..at + span);
            }
            1 => {
                let copy: Vec<char> = chars[at..at + span].to_vec();
                chars.splice(at..at, copy);
            }
            2 => {
                let piece: Vec<char> = FRAGMENTS[rng.below(FRAGMENTS.len())].chars().collect();
                chars.splice(at..at, piece);
            }
            _ => chars.truncate(at),
        }
    }
    chars.into_iter().collect()
}

fn parse_everything(text: &str) {
    let backlog = parse_backlog(text);
    let line_count = text.split('\n').count();
    for item in &backlog.value.items {
        let line_no = match item {
            BacklogItem::Task(task) => {
                assert!(!task.task.is_empty() && !task.status_raw.is_empty());
                task.line_no
            }
            BacklogItem::Unreadable {
                line_no, reason, ..
            } => {
                assert!(!reason.is_empty());
                *line_no
            }
        };
        assert!(
            (1..=line_count).contains(&line_no),
            "line {line_no} of {line_count}"
        );
    }
    assert_eq!(
        parse_backlog(text).value,
        backlog.value,
        "parsing is deterministic"
    );

    let brief = parse_project_brief(text);
    if let Some(team) = &brief.value.team {
        assert!(team.members.iter().all(|m| !m.agent.is_empty()));
    }
    parse_staff_function("hr.md", text);
    parse_agent_profile(text);
    parse_project_profile(text);
    parse_role("role", text);
    parse_status(text);
    classify(text);
    serde_json::to_string(&backlog.value).unwrap();
    serde_json::to_string(&brief.value).unwrap();
}

#[test]
fn parsers_survive_random_garbage() {
    let mut rng = Rng(0x005e_ed0f_0c0e);
    for _ in 0..3000 {
        parse_everything(&garbage(&mut rng));
    }
}

#[test]
fn parsers_survive_mutated_fixture_files() {
    let files = [
        "projects/shop/agents/shop-backend-engineer-ivo/tasks/BACKLOG.md",
        "projects/shop/CLAUDE.md",
        "projects/shop/agents/shop-designer-noa/agent.json",
        "projects/shop/project.json",
        "hr/roles/backend-engineer.toml",
        "hr/roles/designer.toml",
        ".claude/agents/hr.md",
    ];
    let mut rng = Rng(0xdead_beef_cafe);
    for file in files {
        let text = fs::read_to_string(fixture().join(file)).unwrap();
        parse_everything(&text);
        for _ in 0..400 {
            parse_everything(&mutate(&mut rng, &text));
        }
    }
}

#[test]
fn pathological_inputs() {
    for text in [
        String::new(),
        "|".repeat(10_000),
        "`".repeat(10_000),
        "| # | Task | Status |\n".repeat(2_000),
        format!(
            "| # | Task | Status |\n{}",
            "| 1 | t | todo |\n".repeat(5_000)
        ),
        "\n".repeat(10_000),
        "---\n".repeat(1_000),
        "<!-- hr:team:start -->\n".repeat(500),
        "#".repeat(10_000),
        "\u{feff}\u{feff}| # | Task | Status |\r\n|-|-|-|\r\n| 1 | t | todo |\r\n".into(),
    ] {
        parse_everything(&text);
    }
}

#[test]
fn the_loader_survives_garbage_files_and_bytes() {
    let mut rng = Rng(0x0bad_f00d);
    for round in 0..15 {
        let ws = TempDir::with_fixture("garbage");
        for file in [
            "projects/shop/agents/shop-backend-engineer-ivo/tasks/BACKLOG.md",
            "projects/shop/agents/shop-backend-engineer-ivo/agent.json",
            "projects/shop/CLAUDE.md",
            "projects/shop/project.json",
            "hr/roles/designer.toml",
            ".claude/agents/hr.md",
        ] {
            let content: Vec<u8> = if round % 3 == 0 {
                // Raw bytes, including invalid UTF-8.
                (0..rng.below(400)).map(|_| rng.next() as u8).collect()
            } else {
                garbage(&mut rng).into_bytes()
            };
            ws.write(file, content);
        }
        // A folder where a file should be.
        fs::remove_file(ws.path().join("projects/blog/project.json")).unwrap();
        fs::create_dir(ws.path().join("projects/blog/project.json")).unwrap();

        let model =
            Workspace::load(ws.path()).expect("garbage inside the workspace is never fatal");
        assert_eq!(model.projects.len(), 2);
        assert!(model
            .problems_for("projects/blog/project.json")
            .next()
            .is_some());
        serde_json::to_string(&model).unwrap();
    }
}
