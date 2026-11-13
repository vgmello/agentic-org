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

fn work(chains: &[&[&str]]) -> Option<Work> {
    let chain = |c: &&[&str]| c.iter().map(|id| TeamId::new(id).unwrap()).collect();
    Some(Work {
        chains: chains.iter().map(chain).collect(),
    })
}

fn resolved(work: Option<Work>, files: Vec<Input>) -> EffectiveSet {
    resolve(&Inputs { work, files })
}

/// The set in words: each key's winner, each conflict and problem (sorted).
fn summary(set: &EffectiveSet) -> Vec<String> {
    let mut out = Vec::new();
    for entry in &set.entries {
        let winner = match &entry.winner {
            Some(w) => format!("{} v{} {:?}", label(&w.file.place), w.file.version, w.kind),
            None => "none".into(),
        };
        out.push(format!("{}: {winner}", entry.key.as_str()));
    }
    for c in &set.conflicts {
        let teams: Vec<String> = c.teams.iter().map(label).collect();
        let sealed: Vec<String> = c.sealed_by.iter().map(label).collect();
        let applies = c.applies.as_ref().map_or("none".into(), label);
        out.push(format!(
            "{} conflict {} sealed [{}] -> {applies}",
            c.key.as_str(),
            teams.join(","),
            sealed.join(",")
        ));
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
                Outcome::Refused { sealed_by } => format!("sealed by {}", label(sealed_by)),
                other => panic!("{other:?} isn't a problem"),
            };
            format!("{}: {kind}", p.path)
        })
        .collect();
    problems.sort();
    out.extend(problems);
    out
}

type Case = (&'static str, Option<Work>, Vec<Input>, Vec<&'static str>);

fn check(cases: Vec<Case>) {
    for (name, work, files, expected) in cases {
        assert_eq!(summary(&resolved(work, files)), expected, "{name}");
    }
}

#[test]
fn matching_level_order_and_the_nearer_team() {
    check(vec![
        (
            "matching is by key, never id",
            work(&[&["web"]]),
            vec![
                at(org(), "review", 1, "id:a"),
                at(team("web"), "checks", 1, "id:a"),
                at(team("web"), "review", 2, "id:b"),
            ],
            vec!["checks: web v1 Addition", "review: web v2 Override"],
        ),
        (
            "matching is by the front-matter key, never the file name",
            work(&[&["web"]]),
            vec![
                at(org(), "review", 1, ""),
                at(team("web"), "review", 2, "as:code-review"),
                at(team("web"), "checks", 2, "as:review"),
            ],
            vec!["checks: web v2 Addition", "review: web v2 Override"],
        ),
        (
            "the lower level wins, whatever the input order",
            work(&[&["web"]]),
            vec![
                at(team("web"), "review", 2, ""),
                at(team("web"), "checks", 2, ""),
                at(org(), "review", 1, ""),
                at(org(), "checks", 1, ""),
            ],
            vec!["checks: web v2 Override", "review: web v2 Override"],
        ),
        (
            "the nearer team wins, the outermost folded first",
            work(&[&["platform", "web"]]),
            vec![
                at(team("web"), "review", 2, ""),
                at(team("platform"), "review", 1, ""),
                at(team("platform"), "checks", 1, ""),
            ],
            vec!["checks: platform v1 Addition", "review: web v2 Override"],
        ),
        (
            "with no current work, the teams' files take no part",
            None,
            vec![
                at(org(), "review", 1, ""),
                at(team("web"), "review", 2, ""),
                at(team("web"), "checks", 2, ""),
                at(atlas(), "review", 3, ""),
                at(me(), "mine", 1, ""),
                at(Place::Agent(MemberId::new(OTHER).unwrap()), "theirs", 1, ""),
            ],
            vec!["review: org v1 Inherited"],
        ),
        (
            "an unlinked team's files take no part",
            work(&[&["web"]]),
            vec![
                at(org(), "review", 1, ""),
                at(team("mobile"), "review", 2, "sealed"),
                at(team("mobile"), "checks", 2, ""),
            ],
            vec!["review: org v1 Inherited"],
        ),
    ]);
}

#[test]
fn seals() {
    check(vec![
        (
            "an organization seal refuses lower overrides and offs, without a conflict",
            work(&[&["web"], &["mobile"]]),
            vec![
                at(org(), "review", 1, "sealed"),
                at(team("web"), "review", 2, ""),
                at(team("mobile"), "review", 2, "off"),
            ],
            vec![
                "review: org v1 Inherited",
                "mobile/review.md: sealed by org",
                "web/review.md: sealed by org",
            ],
        ),
        (
            "a team seal refuses its sub-teams' versions",
            work(&[&["platform", "web"]]),
            vec![
                at(org(), "review", 1, ""),
                at(team("platform"), "review", 2, "sealed"),
                at(team("web"), "review", 3, ""),
            ],
            vec![
                "review: platform v2 Override",
                "web/review.md: sealed by platform",
            ],
        ),
        (
            "a sibling's override or off of a team seal is a conflict; the sealed version applies",
            work(&[&["web"], &["mobile"]]),
            vec![
                at(org(), "review", 1, ""),
                at(team("web"), "review", 2, "sealed"),
                at(team("mobile"), "review", 3, ""),
                at(team("web"), "checks", 2, "sealed"),
                at(team("mobile"), "checks", 3, "off"),
            ],
            vec![
                "checks: web v2 Addition",
                "review: web v2 Override",
                "checks conflict mobile,web sealed [web] -> web",
                "review conflict mobile,web sealed [web] -> web",
            ],
        ),
        (
            "two team seals: the organization's version, else the first by team id not an off",
            work(&[&["web"], &["mobile"]]),
            vec![
                at(org(), "review", 1, ""),
                at(team("web"), "review", 2, "sealed"),
                at(team("mobile"), "review", 3, "sealed"),
                at(team("web"), "checks", 2, "sealed"),
                at(team("mobile"), "checks", 3, "sealed"),
                at(team("web"), "status", 2, "sealed"),
                at(team("mobile"), "status", 3, "sealed off"),
            ],
            vec![
                "checks: mobile v3 Addition",
                "review: org v1 Inherited",
                "status: web v2 Addition",
                "checks conflict mobile,web sealed [mobile,web] -> mobile",
                "review conflict mobile,web sealed [mobile,web] -> org",
                "status conflict mobile,web sealed [mobile,web] -> web",
            ],
        ),
        (
            "a clashed sealed key stays sealed, in the organization and in a team",
            work(&[&["platform", "web"]]),
            vec![
                at(org(), "review", 1, "sealed"),
                at(org(), "review", 2, "as:review-2"),
                at(team("web"), "review", 3, ""),
                at(team("platform"), "checks", 1, "sealed"),
                at(team("platform"), "checks", 1, "as:checks-2"),
                at(team("web"), "checks", 2, ""),
            ],
            vec![
                "checks: none",
                "review: none",
                "org/review-2.md: clash",
                "org/review.md: clash",
                "platform/checks-2.md: clash",
                "platform/checks.md: clash",
                "web/checks.md: sealed by platform",
                "web/review.md: sealed by org",
            ],
        ),
        (
            "an invalid file that is sealed, or can't be shown not to be, keeps its key sealed",
            work(&[&["web"]]),
            vec![
                at(org(), "review", 1, "sealed bad"),
                at(team("web"), "review", 2, ""),
                at(org(), "checks", 1, "maybe"),
                at(team("web"), "checks", 2, ""),
                at(org(), "plan", 1, "bad"),
                at(team("web"), "plan", 2, ""),
                at(org(), "notes", 1, "sealed bad draft"),
                at(team("web"), "notes", 2, ""),
                at(org(), "guide", 1, "sealed draft"),
                at(team("web"), "guide", 2, ""),
                at(org(), "steps", 1, "sealed retired"),
                at(team("web"), "steps", 2, ""),
            ],
            vec![
                "checks: none",
                "guide: web v2 Addition",
                "notes: web v2 Addition",
                "plan: web v2 Addition",
                "review: none",
                "steps: web v2 Addition",
                "org/checks.md: invalid overridable",
                "org/notes.md: invalid version",
                "org/plan.md: invalid version",
                "org/review.md: invalid version",
                "web/checks.md: sealed by org",
                "web/review.md: sealed by org",
            ],
        ),
        (
            "a file whose key can't be read seals every key below it",
            work(&[&["platform", "web"]]),
            vec![
                at(org(), "review", 1, ""),
                at(team("platform"), "broken", 1, "garbage"),
                at(team("platform"), "checks", 1, ""),
                at(team("web"), "review", 2, ""),
            ],
            vec![
                "checks: platform v1 Addition",
                "review: org v1 Inherited",
                "platform/broken.md: invalid NoFrontMatter",
                "web/review.md: sealed by platform",
            ],
        ),
    ]);
}

#[test]
fn conflicts() {
    check(vec![
        (
            "two team overrides give the organization's version, or none",
            work(&[&["web"], &["mobile"]]),
            vec![
                at(org(), "review", 1, ""),
                at(team("web"), "review", 2, ""),
                at(team("mobile"), "review", 3, ""),
                at(team("web"), "checks", 2, ""),
                at(team("mobile"), "checks", 3, "off"),
            ],
            vec![
                "checks: none",
                "review: org v1 Inherited",
                "checks conflict mobile,web sealed [] -> none",
                "review conflict mobile,web sealed [] -> org",
            ],
        ),
        (
            "one team's version applies",
            work(&[&["web"], &["mobile"]]),
            vec![
                at(org(), "review", 1, ""),
                at(team("web"), "review", 2, ""),
                at(team("mobile"), "checks", 2, ""),
            ],
            vec!["checks: mobile v2 Addition", "review: web v2 Override"],
        ),
        (
            "a parent-and-sub-team link isn't a conflict",
            work(&[&["platform"], &["platform", "web"]]),
            vec![
                at(org(), "review", 1, ""),
                at(team("platform"), "review", 2, ""),
                at(team("web"), "review", 3, ""),
            ],
            vec!["review: web v3 Override"],
        ),
        (
            "a diamond or a repeated chain isn't a conflict",
            work(&[
                &["platform", "web"],
                &["platform", "mobile"],
                &["platform", "web"],
            ]),
            vec![
                at(org(), "review", 1, ""),
                at(team("platform"), "review", 2, ""),
                at(team("platform"), "checks", 2, "sealed"),
            ],
            vec![
                "checks: platform v2 Addition",
                "review: platform v2 Override",
            ],
        ),
        (
            "a team that sealed the version it inherits conflicts with a sibling's override",
            work(&[&["web"], &["mobile"]]),
            vec![
                at(org(), "review", 1, ""),
                at(team("web"), "review", 2, "sealed bad"),
                at(team("mobile"), "review", 3, ""),
            ],
            vec![
                "review: org v1 Inherited",
                "review conflict mobile,web sealed [web] -> org",
                "web/review.md: invalid version",
            ],
        ),
        (
            "a team seal that kept what it inherited conflicts with a sibling's override",
            work(&[&["platform", "web"], &["mobile"]]),
            vec![
                at(org(), "review", 1, ""),
                at(team("platform"), "review", 2, ""),
                at(team("web"), "review", 3, "sealed bad"),
                at(team("mobile"), "review", 4, ""),
            ],
            vec![
                "review: platform v2 Override",
                "review conflict mobile,platform sealed [web] -> platform",
                "web/review.md: invalid version",
            ],
        ),
    ]);
}

#[test]
fn validity_and_clashes() {
    check(vec![
        (
            "valid files only: invalid and misplaced out; drafts and retired silently",
            work(&[&["web"]]),
            vec![
                at(org(), "review", 1, ""),
                at(team("web"), "review", 2, "bad"),
                at(org(), "checks", 1, ""),
                at(team("web"), "checks", 2, "draft"),
                at(team("web"), "plan", 1, "retired"),
                at(org(), "steps", 1, ""),
                at(team("web"), "steps", 2, "at:mobile"),
            ],
            vec![
                "checks: org v1 Inherited",
                "review: org v1 Inherited",
                "steps: org v1 Inherited",
                "web/review.md: invalid version",
                "web/steps.md: invalid level",
            ],
        ),
        (
            "two files with one key at one place are both out; a draft doesn't clash",
            work(&[&["web"]]),
            vec![
                at(org(), "review", 1, ""),
                at(team("web"), "review", 2, "as:a"),
                at(team("web"), "review", 3, "as:b"),
                at(team("web"), "checks", 2, ""),
                at(team("web"), "checks", 2, "bad as:checks-2"),
                at(team("web"), "plan", 2, ""),
                at(team("web"), "plan", 3, "draft as:plan-2"),
            ],
            vec![
                "checks: none",
                "plan: web v2 Addition",
                "review: org v1 Inherited",
                "web/a.md: clash",
                "web/b.md: clash",
                "web/checks-2.md: invalid version",
                "web/checks.md: clash",
            ],
        ),
    ]);
}

#[test]
fn why_shows_every_layer_and_what_happened_to_it() {
    let invalid = |field: &str| {
        Outcome::Invalid(Problem {
            field: field.into(),
            kind: ProblemKind::Invalid,
            key: ProcessKey::new("review"),
            sealed: Some(false),
        })
    };
    let set = resolved(
        work(&[&["platform", "web"], &["mobile"]]),
        vec![
            at(team("mobile"), "review", 4, ""),
            at(team("web"), "review", 3, ""),
            at(team("platform"), "review", 2, "sealed"),
            at(org(), "review", 1, ""),
            at(team("web"), "review", 3, "bad as:review-bad"),
            at(team("platform"), "checks", 1, ""),
            at(org(), "checks", 1, ""),
            at(team("web"), "checks", 1, "draft"),
        ],
    );
    assert_eq!(
        set.inputs,
        vec![org(), team("platform"), team("web"), team("mobile"),]
    );
    let why = |key: &str| {
        let entry = set.why(&ProcessKey::new(key).unwrap()).unwrap();
        let layers = entry.chain.iter();
        layers
            .map(|l| (l.path.clone(), l.version, l.outcome.clone()))
            .collect::<Vec<_>>()
    };
    let refused = Outcome::Refused {
        sealed_by: team("platform"),
    };
    assert_eq!(
        why("review"),
        vec![
            ("org/review.md".into(), Some(1), Outcome::Overridden),
            ("platform/review.md".into(), Some(2), Outcome::Applied),
            ("web/review-bad.md".into(), None, invalid("version")),
            ("web/review.md".into(), Some(3), Outcome::Clashed),
            ("mobile/review.md".into(), Some(4), Outcome::Conflict),
        ]
    );
    assert_eq!(
        why("checks"),
        vec![
            ("org/checks.md".into(), Some(1), Outcome::Overridden),
            ("platform/checks.md".into(), Some(1), Outcome::Applied),
        ]
    );
    // A refused version, and the version a sibling's conflicts with.
    let set = resolved(
        work(&[&["platform", "web"], &["mobile"]]),
        vec![
            at(team("platform"), "review", 2, "sealed"),
            at(team("web"), "review", 3, ""),
            at(team("mobile"), "review", 4, ""),
        ],
    );
    let chain = &set.why(&ProcessKey::new("review").unwrap()).unwrap().chain;
    let outcomes: Vec<&Outcome> = chain.iter().map(|l| &l.outcome).collect();
    assert_eq!(
        outcomes,
        vec![&Outcome::Applied, &refused, &Outcome::Conflict]
    );
    assert_eq!(set.conflicts.len(), 1);
    assert!(set.why(&ProcessKey::new("x").unwrap()).is_none());
}
