//! §3.2's rules, table-driven (processes.md Appendix C): each case fails without its rule.

use super::*;
use crate::file::read;
use crate::{location, Repo};

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
/// `nokey` (an invalid `process`, so its key can't be read), `badname` (a refused file name;
/// only a project file can have one), `over<N>` (`overrides: <key>@<N>`), `id:<a|b>`,
/// `as:<name>` (the file name) and `at:<team|atlas>` (written for and read at that team's or the
/// project's place).
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
            "nokey" => set("process", "Not a key".into()),
            "garbage" => garbage = true,
            "badname" => refused = true,
            _ if flag.starts_with("over") => set("overrides", format!("{key}@{}", &flag[4..])),
            _ if flag.starts_with("id:") => {
                set("id", format!("doc-01jd8z3f00000000000000000{}", &flag[3..]))
            }
            _ if flag.starts_with("as:") => path = flag[3..].to_owned(),
            _ if flag.starts_with("at:") => {
                read_at = match &flag[3..] {
                    "atlas" => atlas(),
                    id => team(id),
                };
                set("level", read_at.level().as_str().into());
                set("scope", read_at.scope().unwrap_or_default().into());
            }
            _ => panic!("unknown flag {flag}"),
        }
    }
    let front: Vec<String> = lines.iter().map(|(f, v)| format!("{f}: {v}")).collect();
    let text = match garbage {
        true => "no front matter".to_owned(),
        false => format!("---\n{}\n---\n# {key}\n", front.join("\n")),
    };
    let read = match (refused, &read_at) {
        (true, Place::Project(key)) => {
            let found = location(Repo::Project(key), &format!("docs/processes/-{path}.md"));
            found.expect("a refused project file name").read(&text)
        }
        (true, _) => panic!("only a project file can have a refused name"),
        (false, _) => read(&read_at, &text),
    };
    Input {
        path: format!("{}/{path}.md", label(&place)),
        place,
        read,
    }
}

fn chains(chains: &[&[&str]]) -> Vec<Vec<TeamId>> {
    let chain = |c: &&[&str]| c.iter().map(|id| TeamId::new(id).unwrap()).collect();
    chains.iter().map(chain).collect()
}

fn work(teams: &[&[&str]]) -> Option<Work> {
    Some(Work::new(ProjectKey::new("atlas").unwrap(), chains(teams)).unwrap())
}

fn resolved(work: Option<Work>, files: Vec<Input>) -> EffectiveSet {
    let mut names: BTreeMap<Place, String> = ["web", "mobile", "platform"]
        .into_iter()
        .map(|id| (team(id), id[..1].to_uppercase() + &id[1..]))
        .collect();
    names.insert(atlas(), "Atlas".into());
    resolve(&Inputs {
        agent: MemberId::new(MEMBER).unwrap(),
        work,
        files,
        names,
    })
}

/// The set in words: each key's winner, each turned-off key, conflict and problem (sorted).
fn summary(set: &EffectiveSet) -> Vec<String> {
    let mut out = Vec::new();
    for entry in &set.entries {
        let winner = match &entry.winner {
            Some(w) => format!("{} v{} {:?}", label(&w.file.place), w.file.version, w.kind),
            None => "none".into(),
        };
        out.push(format!("{}: {winner}", entry.key.as_str()));
    }
    for off in &set.turned_off {
        out.push(format!("{}: {}", off.key.as_str(), off.message()));
    }
    for c in &set.conflicts {
        let teams: Vec<String> = c.teams.iter().map(label).collect();
        let sealed: Vec<String> = c.sealed_by.iter().map(label).collect();
        let applies = c.applies.as_ref().map_or("none".into(), label);
        let unknown = c
            .unknown
            .iter()
            .map(|path| format!(" can't tell: {path} can't be read"));
        out.push(format!(
            "{} conflict {} sealed [{}]{} -> {applies}",
            c.key.as_str(),
            teams.join(","),
            sealed.join(","),
            unknown.collect::<String>()
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
                Outcome::RefusedUnknown { path } => format!("can't tell: {path} can't be read"),
                Outcome::PrivateOnlyAdds(reason) => format!("private {reason:?}"),
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
            "the lowest level wins, whatever the input order",
            work(&[&["web"]]),
            vec![
                at(atlas(), "review", 3, ""),
                at(team("web"), "review", 2, ""),
                at(team("web"), "checks", 2, ""),
                at(org(), "review", 1, ""),
                at(org(), "checks", 1, ""),
            ],
            vec!["checks: web v2 Override", "review: atlas v3 Override"],
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
            "no current work gives O ⊕ G",
            None,
            vec![
                at(org(), "review", 1, ""),
                at(team("web"), "review", 2, ""),
                at(team("web"), "checks", 2, ""),
                at(atlas(), "review", 3, ""),
                at(me(), "mine", 1, ""),
                at(Place::Agent(MemberId::new(OTHER).unwrap()), "theirs", 1, ""),
            ],
            vec!["mine: me v1 Addition", "review: org v1 Inherited"],
        ),
        (
            "with no current work, a private key a team uses only adds",
            None,
            vec![at(team("web"), "checks", 1, ""), at(me(), "checks", 1, "")],
            vec!["checks: me v1 Addition"],
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
                at(atlas(), "review", 3, ""),
            ],
            vec![
                "review: org v1 Inherited",
                "atlas/review.md: sealed by org",
                "mobile/review.md: sealed by org",
                "web/review.md: sealed by org",
            ],
        ),
        (
            "a team seal refuses its sub-teams' and projects' versions",
            work(&[&["platform", "web"]]),
            vec![
                at(org(), "review", 1, ""),
                at(team("platform"), "review", 2, "sealed"),
                at(team("web"), "review", 3, ""),
                at(atlas(), "review", 4, "off"),
            ],
            vec![
                "review: platform v2 Override",
                "atlas/review.md: sealed by platform",
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
            "a project version settles a conflict with a seal",
            work(&[&["web"], &["mobile"]]),
            vec![
                at(org(), "review", 1, ""),
                at(team("web"), "review", 2, "sealed"),
                at(team("mobile"), "review", 3, ""),
                at(atlas(), "review", 4, ""),
            ],
            vec!["review: atlas v4 Override"],
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
                at(atlas(), "checks", 3, ""),
            ],
            vec![
                "checks: none",
                "review: none",
                "atlas/checks.md: sealed by platform",
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
                at(atlas(), "plan", 1, ""),
            ],
            vec![
                "checks: platform v1 Addition",
                "plan: none",
                "review: org v1 Inherited",
                "atlas/plan.md: can't tell: platform/broken.md can't be read",
                "platform/broken.md: invalid NoFrontMatter",
                "web/review.md: can't tell: platform/broken.md can't be read",
            ],
        ),
        (
            "an organization file that can't be read: can't tell, never sealed by the organization",
            work(&[&["web"]]),
            vec![
                at(org(), "broken", 1, "garbage"),
                at(team("web"), "review", 2, "over1"),
                at(atlas(), "plan", 1, ""),
                at(me(), "mine", 1, ""),
            ],
            vec![
                "mine: none",
                "plan: none",
                "review: none",
                "atlas/plan.md: can't tell: org/broken.md can't be read",
                "me/mine.md: can't tell: org/broken.md can't be read",
                "org/broken.md: invalid NoFrontMatter",
                "web/review.md: can't tell: org/broken.md can't be read",
            ],
        ),
        (
            "a file that can't be read beside a sealed one: sealed by that place",
            work(&[&["web"]]),
            vec![
                at(org(), "broken", 1, "garbage"),
                at(org(), "review", 1, "sealed"),
                at(team("web"), "review", 2, ""),
            ],
            vec![
                "review: org v1 Inherited",
                "org/broken.md: invalid NoFrontMatter",
                "web/review.md: sealed by org",
            ],
        ),
        (
            "a draft team file whose key can't be read seals nothing below it (rule 2)",
            work(&[&["platform", "web"]]),
            vec![
                at(team("platform"), "broken", 1, "draft nokey"),
                at(team("web"), "review", 2, ""),
                at(atlas(), "plan", 1, ""),
            ],
            vec![
                "plan: atlas v1 Addition",
                "review: web v2 Addition",
                "platform/broken.md: invalid process",
            ],
        ),
        (
            "a chain's outermost seal is its seal (A71 takes the first seal by team id)",
            work(&[&["a", "z"], &["m"]]),
            vec![
                at(team("a"), "review", 1, "sealed"),
                at(team("z"), "review", 2, "sealed"),
                at(team("m"), "review", 3, "sealed"),
            ],
            vec![
                "review: a v1 Addition",
                "review conflict a,m sealed [a,m] -> a",
                "z/review.md: sealed by a",
            ],
        ),
        (
            "a project file is refused by the smallest sealing team, chains in this order",
            work(&[&["web"], &["mobile"]]),
            vec![
                at(team("web"), "review", 1, "sealed bad"),
                at(team("mobile"), "review", 1, "sealed bad"),
                at(atlas(), "review", 2, ""),
            ],
            vec![
                "review: none",
                "atlas/review.md: sealed by mobile",
                "mobile/review.md: invalid version",
                "web/review.md: invalid version",
            ],
        ),
        (
            "a project file is refused by the smallest sealing team, chains in the other order",
            work(&[&["mobile"], &["web"]]),
            vec![
                at(team("web"), "review", 1, "sealed bad"),
                at(team("mobile"), "review", 1, "sealed bad"),
                at(atlas(), "review", 2, ""),
            ],
            vec![
                "review: none",
                "atlas/review.md: sealed by mobile",
                "mobile/review.md: invalid version",
                "web/review.md: invalid version",
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
            "one team's version applies, an off included",
            work(&[&["web"], &["mobile"]]),
            vec![
                at(org(), "review", 1, ""),
                at(team("web"), "review", 2, ""),
                at(org(), "checks", 1, ""),
                at(team("mobile"), "checks", 2, "off"),
            ],
            vec![
                "checks: none",
                "review: web v2 Override",
                "checks: Turned off for Mobile: we report daily",
            ],
        ),
        (
            "a project version settles a conflict",
            work(&[&["web"], &["mobile"]]),
            vec![
                at(org(), "review", 1, ""),
                at(team("web"), "review", 2, ""),
                at(team("mobile"), "review", 3, ""),
                at(atlas(), "review", 4, ""),
                at(team("web"), "checks", 2, ""),
                at(team("mobile"), "checks", 3, ""),
                at(atlas(), "checks", 4, ""),
            ],
            vec!["checks: atlas v4 Override", "review: atlas v4 Override"],
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
            "a sub-team's override in a diamond is a conflict (R1): the org's version, or none",
            work(&[&["platform", "web"], &["platform", "mobile"]]),
            vec![
                at(org(), "review", 1, ""),
                at(team("platform"), "review", 2, ""),
                at(team("web"), "review", 3, ""),
                at(team("platform"), "checks", 2, ""),
                at(team("web"), "checks", 3, ""),
            ],
            vec![
                "checks: none",
                "review: org v1 Inherited",
                "checks conflict platform,web sealed [] -> none",
                "review conflict platform,web sealed [] -> org",
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
        (
            "a sibling's file that can't be read is a can't tell, never sealed by its team",
            work(&[&["web"], &["mobile"]]),
            vec![
                at(org(), "review", 1, ""),
                at(team("web"), "broken", 1, "garbage"),
                at(team("mobile"), "review", 2, ""),
            ],
            vec![
                "review: org v1 Inherited",
                "review conflict mobile,web sealed [] can't tell: web/broken.md can't be read -> org",
                "web/broken.md: invalid NoFrontMatter",
            ],
        ),
        (
            "a can't tell counts as a seal for A71: the team's own version applies meanwhile",
            work(&[&["web"], &["mobile"]]),
            vec![
                at(org(), "review", 1, ""),
                at(team("web"), "broken", 1, "garbage"),
                at(team("web"), "review", 2, ""),
                at(team("mobile"), "review", 3, ""),
            ],
            vec![
                "review: web v2 Override",
                "review conflict mobile,web sealed [] can't tell: web/broken.md can't be read -> web",
                "web/broken.md: invalid NoFrontMatter",
            ],
        ),
        (
            "a seal and a can't tell are two seals for A71: the organization's version",
            work(&[&["web"], &["mobile"]]),
            vec![
                at(org(), "review", 1, ""),
                at(team("web"), "review", 2, "sealed"),
                at(team("mobile"), "broken", 1, "garbage"),
                at(team("mobile"), "review", 3, ""),
            ],
            vec![
                "review: org v1 Inherited",
                "review conflict mobile,web sealed [web] can't tell: mobile/broken.md can't be read -> org",
                "mobile/broken.md: invalid NoFrontMatter",
            ],
        ),
    ]);
}

/// The P3 review corner (processes.md §3.2 rules 2, 3 and 5; ruled 2026-11-13: ada): an
/// outermost team's file whose key can't be read, above a real seal lower in the same chain. The
/// current behaviour fails closed: the unreadable file might seal the key, so it is the chain's
/// seal and the lower seal is refused ("can't tell") before it can count. The conflict with a
/// sibling chain names both: the can't-tell file (not its team), and the team whose own file
/// seals the key, the outermost one below it (Tomas's LOW). A71 has one seal, the can't tell, with no version of its own, so
/// the organization's version applies, or nothing.
#[test]
fn p3_corner_an_unreadable_outermost_file_above_a_real_seal_fails_closed() {
    let files = |org_version: bool| {
        let mut files = vec![
            at(team("platform"), "broken", 1, "garbage"),
            at(team("web"), "review", 2, "sealed"),
            at(team("mobile"), "review", 3, ""),
        ];
        if org_version {
            files.push(at(org(), "review", 1, ""));
        }
        files
    };
    check(vec![
        (
            "the can't tell is the chain's seal; the organization's version applies",
            work(&[&["platform", "web"], &["mobile"]]),
            files(true),
            vec![
                "review: org v1 Inherited",
                "review conflict mobile,platform sealed [web] can't tell: platform/broken.md can't be read -> org",
                "platform/broken.md: invalid NoFrontMatter",
                "web/review.md: can't tell: platform/broken.md can't be read",
            ],
        ),
        (
            "with no organization version, nothing applies (never the refused seal's)",
            work(&[&["platform", "web"], &["mobile"]]),
            files(false),
            vec![
                "review: none",
                "review conflict mobile,platform sealed [web] can't tell: platform/broken.md can't be read -> none",
                "platform/broken.md: invalid NoFrontMatter",
                "web/review.md: can't tell: platform/broken.md can't be read",
            ],
        ),
        (
            "two real seals below the can't tell: only the outermost one is named",
            work(&[&["platform", "web", "ios"], &["mobile"]]),
            {
                let mut files = files(true);
                files.push(at(team("ios"), "review", 4, "sealed"));
                files
            },
            vec![
                "review: org v1 Inherited",
                "review conflict mobile,platform sealed [web] can't tell: platform/broken.md can't be read -> org",
                "ios/review.md: can't tell: platform/broken.md can't be read",
                "platform/broken.md: invalid NoFrontMatter",
                "web/review.md: can't tell: platform/broken.md can't be read",
            ],
        ),
    ]);
}

#[test]
fn r2b_an_unreadable_key_with_overridable_true_does_not_seal() {
    check(vec![(
        "rule 2 can tell: a file whose key can't be read, but whose overridable: true can",
        work(&[&["web"]]),
        vec![
            at(org(), "broken", 1, "nokey"),
            at(org(), "review", 1, ""),
            at(team("web"), "review", 2, ""),
            at(atlas(), "plan", 1, ""),
        ],
        vec![
            "plan: atlas v1 Addition",
            "review: web v2 Override",
            "org/broken.md: invalid process",
        ],
    )]);
}

#[test]
fn work_takes_only_chains_that_are_parent_paths() {
    let new = |teams: &[&[&str]]| Work::new(ProjectKey::new("atlas").unwrap(), chains(teams));
    let id = |id: &str| TeamId::new(id).unwrap();
    let refused = [
        (&[&["web"][..], &[]][..], ChainError::Empty),
        (&[&["web", "web"]], ChainError::Repeated(id("web"))),
        (
            &[&["platform", "web", "platform"]],
            ChainError::Repeated(id("platform")),
        ),
        (
            &[&["web"], &["platform", "web"]],
            ChainError::Ancestors(id("web")),
        ),
        (
            &[&["platform", "web"], &["mobile", "web"]],
            ChainError::Ancestors(id("web")),
        ),
        (
            &[&["platform", "web"], &["web", "mobile"]],
            ChainError::Ancestors(id("web")),
        ),
    ];
    for (teams, error) in refused {
        assert_eq!(new(teams), Err(error), "{teams:?}");
    }
    let accepted: [&[&[&str]]; 5] = [
        &[],
        &[&["web"]],
        &[&["platform"], &["platform", "web"]],
        &[&["platform", "web"], &["platform", "mobile"]],
        &[&["platform", "web"], &["platform", "web"], &["design"]],
    ];
    for teams in accepted {
        let work = new(teams).unwrap_or_else(|e| panic!("{teams:?}: {e:?}"));
        assert_eq!(
            (work.project().as_str(), work.chains()),
            ("atlas", &chains(teams)[..])
        );
    }
}

#[test]
fn turned_off_private_and_validity() {
    check(vec![
        (
            "a surviving off removes the key, named for its team or project",
            work(&[&["platform", "web"]]),
            vec![
                at(org(), "review", 1, ""),
                at(team("web"), "review", 2, "off"),
                at(org(), "checks", 1, ""),
                at(atlas(), "checks", 2, "off"),
                at(org(), "plan", 1, ""),
                at(team("platform"), "plan", 2, "off"),
                at(team("web"), "plan", 3, ""),
            ],
            vec![
                "checks: none",
                "plan: web v3 Override",
                "review: none",
                "checks: Turned off for Atlas: we report daily",
                "review: Turned off for Web: we report daily",
            ],
        ),
        (
            "a place with no name is named by its team id, or as the organization",
            work(&[&["design"]]),
            vec![
                at(org(), "review", 1, "off"),
                at(org(), "checks", 1, ""),
                at(team("design"), "checks", 2, "off"),
            ],
            vec![
                "checks: none",
                "review: none",
                "checks: Turned off for design: we report daily",
                "review: Turned off for the organization: we report daily",
            ],
        ),
        (
            "private files only add",
            work(&[&["web"]]),
            vec![
                at(me(), "mine", 1, ""),
                at(org(), "review", 1, ""),
                at(me(), "review", 1, ""),
                at(me(), "plan", 1, "over1"),
                at(me(), "quiet", 1, "off"),
                at(team("web"), "drafted", 1, "draft"),
                at(me(), "drafted", 1, ""),
                at(atlas(), "old", 1, "retired"),
                at(me(), "old", 1, ""),
                at(org(), "broken", 1, "bad"),
                at(me(), "broken", 1, ""),
                at(team("web"), "twice", 1, ""),
                at(team("web"), "twice", 1, "as:twice-2"),
                at(me(), "twice", 1, ""),
            ],
            vec![
                "broken: none",
                "drafted: none",
                "mine: me v1 Addition",
                "old: none",
                "plan: none",
                "quiet: none",
                "review: org v1 Inherited",
                "twice: none",
                "me/broken.md: private Inherited",
                "me/drafted.md: private Inherited",
                "me/old.md: private Inherited",
                "me/plan.md: private Overrides",
                "me/quiet.md: private TurnsOff",
                "me/review.md: private Inherited",
                "me/twice.md: private Inherited",
                "org/broken.md: invalid version",
                "web/twice-2.md: clash",
                "web/twice.md: clash",
            ],
        ),
        (
            "a private key might be an unreadable file's",
            work(&[&["web"]]),
            vec![at(atlas(), "broken", 1, "garbage"), at(me(), "mine", 1, "")],
            vec![
                "mine: none",
                "atlas/broken.md: invalid NoFrontMatter",
                "me/mine.md: can't tell: atlas/broken.md can't be read",
            ],
        ),
        (
            "valid files only: invalid, misplaced and refused names out; drafts silently",
            work(&[&["web"]]),
            vec![
                at(org(), "review", 1, ""),
                at(team("web"), "review", 2, "bad"),
                at(org(), "checks", 1, ""),
                at(team("web"), "checks", 2, "draft"),
                at(atlas(), "plan", 1, "retired"),
                at(org(), "steps", 1, ""),
                at(team("web"), "steps", 2, "at:mobile"),
                at(org(), "notes", 1, ""),
                at(atlas(), "notes", 2, "badname"),
                at(org(), "guide", 1, ""),
                at(team("web"), "guide", 2, "at:atlas"),
            ],
            vec![
                "checks: org v1 Inherited",
                "guide: org v1 Inherited",
                "notes: org v1 Inherited",
                "review: org v1 Inherited",
                "steps: org v1 Inherited",
                "atlas/notes.md: invalid file name",
                "web/guide.md: invalid level",
                "web/review.md: invalid version",
                "web/steps.md: invalid scope",
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
        (
            "an invalid draft or retired file doesn't clash either",
            work(&[&["web"]]),
            vec![
                at(team("web"), "review", 2, ""),
                at(team("web"), "review", 3, "bad draft as:review-2"),
                at(atlas(), "checks", 2, ""),
                at(atlas(), "checks", 3, "badname retired as:checks-2"),
                at(team("web"), "plan", 2, ""),
                at(team("web"), "plan", 3, "draft at:mobile as:plan-2"),
            ],
            vec![
                "checks: atlas v2 Addition",
                "plan: web v2 Addition",
                "review: web v2 Addition",
                "atlas/checks-2.md: invalid file name",
                "web/plan-2.md: invalid scope",
                "web/review-2.md: invalid version",
            ],
        ),
    ]);
}

#[test]
fn drift_compares_an_override_with_its_parent() {
    let drift = |work, files: Vec<Input>| -> Vec<(String, Option<Drift>)> {
        let set = resolved(work, files);
        let winners = set.entries.into_iter().filter_map(|e| e.winner);
        winners
            .map(|w| (w.file.key.as_str().to_owned(), w.drift))
            .collect()
    };
    let changed = |place, version| {
        Some(Drift::ParentChanged {
            parent: place,
            version,
        })
    };
    let one = drift(
        work(&[&["web"]]),
        vec![
            at(org(), "review", 2, ""),
            at(team("web"), "review", 3, "over1"),
            at(org(), "checks", 2, ""),
            at(team("web"), "checks", 3, "over2"),
            at(org(), "plan", 2, ""),
            at(team("web"), "plan", 3, ""),
            at(org(), "notes", 1, ""),
            at(team("web"), "extra", 1, "over1"),
            at(team("web"), "steps", 4, ""),
            at(atlas(), "steps", 5, "over3"),
            at(org(), "guide", 2, ""),
            at(team("web"), "guide", 3, "over5"),
        ],
    );
    let expected = vec![
        ("checks".to_owned(), Some(Drift::Current)),
        ("extra".to_owned(), None),
        ("guide".to_owned(), Some(Drift::Unknown)),
        ("notes".to_owned(), None),
        ("plan".to_owned(), Some(Drift::Unknown)),
        ("review".to_owned(), changed(org(), 2)),
        ("steps".to_owned(), changed(team("web"), 4)),
    ];
    assert_eq!(one, expected);
    // A project version that settles a conflict with nothing applying has no parent version.
    let two = drift(
        work(&[&["web"], &["mobile"]]),
        vec![
            at(team("web"), "review", 2, ""),
            at(team("mobile"), "review", 3, ""),
            at(atlas(), "review", 4, "over3"),
        ],
    );
    assert_eq!(two, vec![("review".to_owned(), Some(Drift::Unknown))]);
}

#[test]
fn why_shows_every_layer_and_what_happened_to_it() {
    let invalid = |field: &str| {
        Outcome::Invalid(Problem {
            field: field.into(),
            kind: ProblemKind::Invalid,
            key: ProcessKey::new("review"),
            sealed: Some(false),
            status: Some(Status::Active),
        })
    };
    let set = resolved(
        work(&[&["platform", "web"], &["mobile"]]),
        vec![
            at(me(), "review", 1, ""),
            at(team("mobile"), "review", 4, ""),
            at(team("web"), "review", 3, ""),
            at(team("platform"), "review", 2, "sealed"),
            at(org(), "review", 1, ""),
            at(team("web"), "review", 3, "bad as:review-bad"),
            at(team("platform"), "checks", 1, "off"),
            at(org(), "checks", 1, ""),
            at(team("web"), "checks", 1, "draft"),
        ],
    );
    assert_eq!(
        set.inputs,
        vec![
            org(),
            team("platform"),
            team("web"),
            team("mobile"),
            atlas(),
            me()
        ]
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
            (
                "me/review.md".into(),
                Some(1),
                Outcome::PrivateOnlyAdds(PrivateReason::Inherited)
            ),
        ]
    );
    assert_eq!(
        why("checks"),
        vec![
            ("org/checks.md".into(), Some(1), Outcome::Overridden),
            ("platform/checks.md".into(), Some(1), Outcome::TurnedOff),
        ]
    );
    // A refused version, and a conflict the project settled.
    let set = resolved(
        work(&[&["platform", "web"], &["mobile"]]),
        vec![
            at(team("platform"), "review", 2, "sealed"),
            at(team("web"), "review", 3, ""),
            at(team("mobile"), "review", 4, ""),
            at(atlas(), "review", 5, ""),
        ],
    );
    let chain = &set.why(&ProcessKey::new("review").unwrap()).unwrap().chain;
    let outcomes: Vec<&Outcome> = chain.iter().map(|l| &l.outcome).collect();
    assert_eq!(
        outcomes,
        vec![
            &Outcome::Overridden,
            &refused,
            &Outcome::Overridden,
            &Outcome::Applied
        ]
    );
    assert!(set.conflicts.is_empty() && set.why(&ProcessKey::new("x").unwrap()).is_none());
}
