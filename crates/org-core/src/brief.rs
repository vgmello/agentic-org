//! `projects/<key>/CLAUDE.md`: the project brief, with the team roster HR keeps
//! between the `hr:team` markers.

use crate::markdown::{
    find_tables, header_key, headings, marker_block, numbered_lines, section, Block,
};
use crate::model::{Issue, Parsed, ProblemKind, ProjectBrief, Roster, RosterMember, UnreadableRow};

/// The markers around the HR-managed roster (same as hr.py).
pub const TEAM_START: &str = "<!-- hr:team:start -->";
pub const TEAM_END: &str = "<!-- hr:team:end -->";

/// Parses a project brief. Never fails; a missing title or roster block is an issue.
pub fn parse_project_brief(text: &str) -> Parsed<ProjectBrief> {
    let lines = numbered_lines(text);
    let mut issues = Vec::new();

    let title = headings(&lines)
        .into_iter()
        .find(|(_, level, text)| *level == 1 && !text.is_empty())
        .map(|(_, _, text)| text);
    if title.is_none() {
        issues.push(Issue::warning(
            ProblemKind::InvalidField,
            "the brief has no `# <project name>` title",
        ));
    }

    let team = match marker_block(&lines, TEAM_START, TEAM_END) {
        Block::Found(block) => Some(parse_roster(&block, &mut issues)),
        Block::NoStart => {
            issues.push(Issue::warning(
                ProblemKind::Malformed,
                format!("no team roster block ({TEAM_START})"),
            ));
            None
        }
        Block::NoEnd => {
            issues.push(Issue::warning(
                ProblemKind::Malformed,
                format!("the team roster block has {TEAM_START} but no {TEAM_END}"),
            ));
            None
        }
    };

    let brief = ProjectBrief {
        title,
        goals: section(&lines, &["Goals"]).filter(|s| !s.is_empty()),
        context: section(&lines, &["Context"]).filter(|s| !s.is_empty()),
        team,
    };
    Parsed::new(brief, issues)
}

/// Column positions of a roster table.
struct Columns {
    agent: usize,
    name: Option<usize>,
    title: Option<usize>,
    focus: Option<usize>,
    reports_to: Option<usize>,
}

impl Columns {
    fn from_header(header: &[String]) -> Option<Self> {
        let keys: Vec<String> = header.iter().map(|h| header_key(h)).collect();
        let find = |names: &[&str]| keys.iter().position(|k| names.contains(&k.as_str()));
        let columns = Self {
            agent: find(&["agent", "agent id", "id"])?,
            name: find(&["name"]),
            title: find(&["role", "title"]),
            focus: find(&["focus"]),
            reports_to: find(&["reports to", "manager"]),
        };
        (columns.name.is_some() || columns.title.is_some()).then_some(columns)
    }
}

fn parse_roster(block: &[(usize, &str)], issues: &mut Vec<Issue>) -> Roster {
    let tables = find_tables(block, |cells| Columns::from_header(cells).is_some());
    let mut roster = Roster::default();
    for table in &tables {
        let Some(columns) = Columns::from_header(&table.header) else {
            continue;
        };
        for row in &table.rows {
            let cell = |i: Option<usize>| {
                i.and_then(|i| row.cells.get(i))
                    .map(|c| c.trim().to_string())
            };
            let agent = cell(Some(columns.agent)).map(|a| a.trim_matches('`').trim().to_string());
            if row.cells.len() > table.header.len() {
                // hr.py doesn't escape `|` in a focus or title, so the columns shifted.
                roster.unreadable.push(UnreadableRow {
                    line_no: row.line_no,
                    raw: row.raw.clone(),
                    reason: "the row has more cells than the header (a `|` in a cell?)".into(),
                });
                continue;
            }
            match agent {
                Some(agent) if !agent.is_empty() => roster.members.push(RosterMember {
                    line_no: row.line_no,
                    agent,
                    name: cell(columns.name).unwrap_or_default(),
                    title: cell(columns.title).unwrap_or_default(),
                    focus: cell(columns.focus).filter(|f| !f.is_empty() && f != "-"),
                    reports_to: cell(columns.reports_to).unwrap_or_default(),
                }),
                _ => roster.unreadable.push(UnreadableRow {
                    line_no: row.line_no,
                    raw: row.raw.clone(),
                    reason: "the agent cell is empty or missing".into(),
                }),
            }
        }
    }
    let has_table_rows = block.iter().any(|(_, l)| l.trim_start().starts_with('|'));
    if tables.is_empty() && has_table_rows {
        issues.push(Issue::warning(
            ProblemKind::Malformed,
            "the team roster block has a table without an Agent / Name / Role header",
        ));
    }
    if !roster.unreadable.is_empty() {
        issues.push(Issue::warning(
            ProblemKind::UnreadableRows,
            format!(
                "{} team roster row(s) can't be read",
                roster.unreadable.len()
            ),
        ));
    }
    roster
}

#[cfg(test)]
mod tests {
    use super::*;

    const BRIEF: &str = "# Payments\n\n<!-- Project created by HR. -->\n\n## Goals\n\nShip v1.\n\n## Context\n\n_Background._\n\n## Team\n\n<!-- hr:team:start -->\n| Agent | Name | Role | Focus | Reports to |\n| --- | --- | --- | --- | --- |\n| `pay-dev-ivo` | Ivo | Backend Engineer | - | user |\n| `pay-ux-noa` | Noa | Designer | Checkout | pay-dev-ivo |\n| | | | | |\n| | x | y | z | w |\n<!-- hr:team:end -->\n\n## Working together\n";

    #[test]
    fn reads_title_sections_and_roster() {
        let parsed = parse_project_brief(BRIEF);
        let brief = parsed.value;
        assert_eq!(brief.title.as_deref(), Some("Payments"));
        assert_eq!(brief.goals.as_deref(), Some("Ship v1."));
        assert_eq!(brief.context.as_deref(), Some("_Background._"));
        let team = brief.team.unwrap();
        assert_eq!(team.members.len(), 2);
        assert_eq!(team.members[0].agent, "pay-dev-ivo");
        assert_eq!(team.members[0].focus, None);
        assert_eq!(team.members[1].focus.as_deref(), Some("Checkout"));
        assert_eq!(team.members[1].reports_to, "pay-dev-ivo");
        assert_eq!(team.unreadable.len(), 1);
        assert_eq!(team.unreadable[0].line_no, 21);
        assert_eq!(parsed.issues.len(), 1);
    }

    #[test]
    fn a_pipe_in_a_roster_cell_makes_the_row_unreadable() {
        let text = "# P\n<!-- hr:team:start -->\n| Agent | Name | Role | Focus | Reports to |\n| --- | --- | --- | --- | --- |\n| `p-dev-a` | A | Dev | Rust | Tauri | user |\n<!-- hr:team:end -->\n";
        let team = parse_project_brief(text).value.team.unwrap();
        assert!(team.members.is_empty());
        assert_eq!(team.unreadable.len(), 1);
    }

    #[test]
    fn empty_team_note_has_no_members() {
        let text =
            "# Web\n\n<!-- hr:team:start -->\n_No agents allocated yet._\n<!-- hr:team:end -->\n";
        let parsed = parse_project_brief(text);
        assert!(parsed.issues.is_empty());
        assert_eq!(parsed.value.team, Some(Roster::default()));
    }

    #[test]
    fn missing_markers_are_issues() {
        let parsed = parse_project_brief("no title\n");
        assert_eq!(parsed.value.team, None);
        assert_eq!(parsed.issues.len(), 2);
        let parsed = parse_project_brief("# T\n<!-- hr:team:start -->\n| a |\n");
        assert_eq!(parsed.value.team, None);
        assert_eq!(parsed.issues.len(), 1);
    }
}
