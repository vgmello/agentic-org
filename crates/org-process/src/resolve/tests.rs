//! §3.2's rules, table-driven (processes.md Appendix C): each case fails without its rule.

use super::*;
use crate::{read, Location, MemberId, ProjectKey, TeamId};

const MEMBER: &str = "01JD8Z3F0000000000000000CD";
const OTHER: &str = "01JD8Z3F0000000000000000EF";

fn org() -> Place {
    Place::Organization
}

fn team(id: &str) -> Place {
    Place::Team(TeamId::new(id).unwrap())
}

fn atlas() -> Place {
    Place::Project(ProjectKey::new("atlas").unwrap())
}

fn me() -> Place {
    Place::Agent(MemberId::new(MEMBER).unwrap())
}

fn label(place: &Place) -> String {
    match place {
        Place::Organization => "org".into(),
        Place::Agent(id) if id.as_str() == MEMBER => "me".into(),
        other => other.scope().unwrap_or_default().to_owned(),
    }
}

/// A file with `key` at `place`, read from its text. `flags`: `sealed`, `maybe` (an unreadable
/// `overridable`), `off`, `draft`, `retired`, `bad` (version 0), `garbage` (no front matter),
/// `badname` (a refused file name), `over<N>` (`overrides: <key>@<N>`), `id:<a|b>`, `as:<name>`
/// (the file name) and `at:<team>` (written for and read at that team's place).
fn at(place: Place, key: &str, version: u32, flags: &str) -> Input {
    let mut lines: Vec<(&str, String)> = vec![
        ("process", key.into()),
        ("name", format!("The {key}")),
        ("kind", "process".into()),
        ("level", place.level().as_str().into()),
        ("version", version.to_string()),
        ("owner", "ada".into()),
        ("summary", "How we do it.".into()),
        ("applies_to", "everyone".into()),
        ("overridable", "true".into()),
        ("status", "active".into()),
    ];
    if let Some(scope) = place.scope() {
        lines.push(("scope", scope.into()));
    }
    let mut set = |field: &'static str, value: String| {
        lines.retain(|(f, _)| *f != field);
        lines.push((field, value));
    };
    let (mut path, mut read_at, mut garbage, mut refused) =
        (key.to_owned(), place.clone(), false, false);
    for flag in flags.split_whitespace() {
        match flag {
            "sealed" => set("overridable", "false".into()),
            "maybe" => set("overridable", "maybe".into()),
            "off" => {
                set("status", "off".into());
                set("reason", "we report daily".into());
            }
            "draft" | "retired" => set("status", flag.into()),
            "bad" => set("version", "0".into()),
            "garbage" => garbage = true,
            "badname" => refused = true,
            _ if flag.starts_with("over") => set("overrides", format!("{key}@{}", &flag[4..])),
            _ if flag.starts_with("id:") => {
                set("id", format!("doc-01jd8z3f00000000000000000{}", &flag[3..]))
            }
            _ if flag.starts_with("as:") => path = flag[3..].to_owned(),
            _ if flag.starts_with("at:") => {
                read_at = team(&flag[3..]);
                set("level", "team".into());
                set("scope", flag[3..].into());
            }
            _ => panic!("unknown flag {flag}"),
        }
    }
    let front: Vec<String> = lines.iter().map(|(f, v)| format!("{f}: {v}")).collect();
    let text = match garbage {
        true => "no front matter".to_owned(),
        false => format!("---\n{}\n---\n# {key}\n", front.join("\n")),
    };
    let read = match refused {
        true => Location {
            place: read_at.clone(),
            name_refused: true,
        }
        .read(&text),
        false => read(&read_at, &text),
    };
    Input {
        path: format!("{}/{path}.md", label(&place)),
        place,
        read,
    }
}

fn resolved(files: Vec<Input>) -> EffectiveSet {
    resolve(&Inputs { files })
}

/// The set in words: each key's winner, then each problem (sorted).
fn summary(set: &EffectiveSet) -> Vec<String> {
    let mut out = Vec::new();
    for entry in &set.entries {
        let winner = match &entry.winner {
            Some(w) => format!("{} v{} {:?}", label(&w.file.place), w.file.version, w.kind),
            None => "none".into(),
        };
        out.push(format!("{}: {winner}", entry.key.as_str()));
    }
    let mut problems: Vec<String> = set
        .problems
        .iter()
        .map(|p| {
            let kind = match &p.outcome {
                Outcome::Invalid(problem) if problem.field.is_empty() => {
                    format!("invalid {:?}", problem.kind)
                }
                Outcome::Invalid(problem) => format!("invalid {}", problem.field),
                Outcome::Clashed => "clash".into(),
                other => panic!("{other:?} isn't a problem"),
            };
            format!("{}: {kind}", p.path)
        })
        .collect();
    problems.sort();
    out.extend(problems);
    out
}

type Case = (&'static str, Vec<Input>, Vec<&'static str>);

fn check(cases: Vec<Case>) {
    for (name, files, expected) in cases {
        assert_eq!(summary(&resolved(files)), expected, "{name}");
    }
}

#[test]
fn the_organizations_valid_files_apply_matched_by_key() {
    check(vec![
        (
            "only the organization's files take part, for now",
            vec![
                at(org(), "review", 1, ""),
                at(team("web"), "review", 2, ""),
                at(team("web"), "checks", 2, ""),
                at(atlas(), "plan", 3, ""),
                at(me(), "mine", 1, ""),
                at(Place::Agent(MemberId::new(OTHER).unwrap()), "theirs", 1, ""),
            ],
            vec!["review: org v1 Inherited"],
        ),
        (
            "matching is by the front-matter key, never the id or the file name",
            vec![
                at(org(), "review", 1, "id:a as:code-review"),
                at(org(), "checks", 2, "id:a as:review"),
            ],
            vec!["checks: org v2 Inherited", "review: org v1 Inherited"],
        ),
        (
            "valid files only: invalid and misplaced out; drafts and retired silently",
            vec![
                at(org(), "review", 1, "bad"),
                at(org(), "checks", 1, "draft"),
                at(org(), "plan", 1, "retired"),
                at(org(), "steps", 1, "at:web"),
                at(org(), "broken", 1, "garbage"),
                at(org(), "notes", 1, ""),
            ],
            vec![
                "notes: org v1 Inherited",
                "review: none",
                "steps: none",
                "org/broken.md: invalid NoFrontMatter",
                "org/review.md: invalid version",
                "org/steps.md: invalid level",
            ],
        ),
        (
            "two files with one key at one place are both out; a draft doesn't clash",
            vec![
                at(org(), "review", 2, "as:a"),
                at(org(), "review", 3, "as:b"),
                at(org(), "checks", 2, ""),
                at(org(), "checks", 2, "bad as:checks-2"),
                at(org(), "plan", 2, ""),
                at(org(), "plan", 3, "draft as:plan-2"),
            ],
            vec![
                "checks: none",
                "plan: org v2 Inherited",
                "review: none",
                "org/a.md: clash",
                "org/b.md: clash",
                "org/checks-2.md: invalid version",
                "org/checks.md: clash",
            ],
        ),
    ]);
}

#[test]
fn why_shows_every_layer_and_what_happened_to_it() {
    let set = resolved(vec![
        at(org(), "review", 2, "as:b"),
        at(org(), "review", 1, "bad as:a"),
        at(org(), "checks", 1, ""),
        at(org(), "checks", 2, "draft as:checks-2"),
    ]);
    assert_eq!(set.inputs, vec![org()]);
    let why = |key: &str| {
        let entry = set.why(&ProcessKey::new(key).unwrap()).unwrap();
        let layers = entry.chain.iter();
        layers
            .map(|l| (l.path.clone(), l.version, l.outcome.clone()))
            .collect::<Vec<_>>()
    };
    let invalid = Outcome::Invalid(Problem {
        field: "version".into(),
        kind: ProblemKind::Invalid,
        key: ProcessKey::new("review"),
        sealed: Some(false),
    });
    assert_eq!(
        why("review"),
        vec![
            ("org/a.md".into(), None, invalid),
            ("org/b.md".into(), Some(2), Outcome::Clashed),
        ]
    );
    assert_eq!(
        why("checks"),
        vec![("org/checks.md".into(), Some(1), Outcome::Applied)]
    );
    assert!(set.why(&ProcessKey::new("x").unwrap()).is_none());
}
