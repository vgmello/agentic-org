//! §4.2's rule switch and start check, table-driven (processes.md Appendix C "One branch"): each
//! case fails without its guard.

use std::collections::BTreeMap;

use super::*;
use crate::file::read;
use crate::{resolve, Input, Inputs, ProjectKey, TeamId, Work};

const MEMBER: &str = "01JD8Z3F0000000000000000CD";
const OTHER: &str = "01JD8Z3F0000000000000000EF";

fn team(id: &str) -> Place {
    Place::Team(TeamId::new(id).unwrap())
}

fn atlas() -> Place {
    Place::Project(ProjectKey::new("atlas").unwrap())
}

/// A file with `key` at `place`, version 1. `flags`: `off`, `sealed`, `v2`, `bad` (version 0, so
/// invalid), `nokey` (its key can't be read) and `as:<name>` (its file name, for a clash).
fn at(place: Place, key: &str, flags: &str) -> Input {
    let (mut status, mut overridable, mut version, mut process, mut path) =
        ("active", "true", "1", key, key);
    for flag in flags.split_whitespace() {
        match flag {
            "off" => status = "off\nreason: we work on two at once",
            "sealed" => overridable = "false",
            "bad" => version = "0",
            "v2" => version = "2",
            "nokey" => process = "Not a key",
            _ if flag.starts_with("as:") => path = &flag[3..],
            _ => panic!("unknown flag {flag}"),
        }
    }
    let scope = place
        .scope()
        .map(|s| format!("scope: {s}\n"))
        .unwrap_or_default();
    let text = format!(
        "---\nprocess: {process}\nname: The {key}\nkind: process\nlevel: {}\n{scope}\
         version: {version}\nowner: ada\nsummary: How we do it.\napplies_to: everyone\n\
         overridable: {overridable}\nstatus: {status}\n---\n# {key}\n",
        place.level().as_str()
    );
    Input {
        path: format!("{}/{path}.md", place.scope().unwrap_or("org")),
        read: read(&place, &text),
        place,
    }
}

/// Atlas's set, its teams `web` and `mobile` each linked on their own.
fn set(files: Vec<Input>) -> EffectiveSet {
    let teams = vec![
        vec![TeamId::new("web").unwrap()],
        vec![TeamId::new("mobile").unwrap()],
    ];
    resolved(
        Some(Work::new(ProjectKey::new("atlas").unwrap(), teams).unwrap()),
        files,
    )
}

fn resolved(work: Option<Work>, files: Vec<Input>) -> EffectiveSet {
    resolve(&Inputs {
        agent: MemberId::new(MEMBER).unwrap(),
        work,
        files,
        names: BTreeMap::new(),
    })
}

/// The `off` files approved by a non-author, each by its place, path and version.
type Approved = Vec<(Place, &'static str, u32)>;

fn approves(approved: &Approved, layer: &Layer) -> bool {
    let file = (&layer.place, layer.path.as_str(), layer.version);
    approved
        .iter()
        .any(|(place, path, v)| file == (place, *path, Some(*v)))
}

#[test]
fn the_rule_is_off_only_for_a_settled_approved_off_of_its_key() {
    let key = RULE_KEY;
    let path = "atlas/branch-and-merge.md";
    let team_path = "web/branch-and-merge.md";
    let team_off = || at(team("web"), key, "off");
    let atlas_off = || set(vec![at(atlas(), key, "off")]);
    let atlas_ok: Approved = vec![(atlas(), path, 1)];
    let team_ok: Approved = vec![(team("web"), team_path, 1)];
    let both_ok: Approved = vec![(atlas(), path, 1), (team("web"), team_path, 1)];
    // Hand-made sets (resolution makes one turned-off layer per key, listed in `turned_off`), to
    // pin that every turned-off layer must be approved and that each part must be present.
    let mut twice = atlas_off();
    let mut second = twice.entries[0].chain[0].clone();
    (second.place, second.path) = (team("web"), team_path.into());
    twice.entries[0].chain.push(second);
    let mut no_layer = atlas_off();
    no_layer.entries.clear();
    let mut not_listed = atlas_off();
    not_listed.turned_off.clear();
    let mut other_listed = set(vec![at(atlas(), key, "off"), at(atlas(), "review", "off")]);
    other_listed
        .turned_off
        .retain(|off| off.key.as_str() != key);
    let review_ok: Approved = vec![(atlas(), path, 1), (atlas(), "atlas/review.md", 1)];
    type Case = (&'static str, Option<EffectiveSet>, Approved, bool);
    let cases: Vec<Case> = vec![
        ("no set: can't compute", None, both_ok.clone(), true),
        (
            "an approved project off",
            Some(atlas_off()),
            atlas_ok.clone(),
            false,
        ),
        (
            "an approved team off",
            Some(set(vec![team_off()])),
            team_ok.clone(),
            false,
        ),
        (
            "an earlier version of the off approved",
            Some(set(vec![at(atlas(), key, "off v2")])),
            atlas_ok.clone(),
            true,
        ),
        (
            "an approval of another file at its place",
            Some(atlas_off()),
            vec![(atlas(), "atlas/other.md", 1)],
            true,
        ),
        (
            "an overridden version above the off needn't be approved",
            Some(set(vec![at(team("web"), key, ""), at(atlas(), key, "off")])),
            atlas_ok.clone(),
            false,
        ),
        (
            "no project in the set (O + G): the organization's approved off",
            Some(resolved(None, vec![at(Place::Organization, key, "off")])),
            vec![(Place::Organization, "org/branch-and-merge.md", 1)],
            true,
        ),
        (
            "the key is missing",
            Some(set(vec![])),
            both_ok.clone(),
            true,
        ),
        (
            "the key applies",
            Some(set(vec![at(atlas(), key, "")])),
            both_ok.clone(),
            true,
        ),
        ("an unapproved off", Some(atlas_off()), vec![], true),
        (
            "an approved off of another key",
            Some(set(vec![at(atlas(), "review", "off")])),
            vec![(atlas(), "atlas/review.md", 1)],
            true,
        ),
        (
            "another key's unapproved off doesn't count",
            Some(set(vec![team_off(), at(atlas(), "review", "off")])),
            team_ok.clone(),
            false,
        ),
        (
            "one of two offs unapproved",
            Some(twice.clone()),
            atlas_ok.clone(),
            true,
        ),
        ("both offs approved", Some(twice), both_ok.clone(), false),
        (
            "no turned-off layer in the chain",
            Some(no_layer),
            both_ok.clone(),
            true,
        ),
        (
            "a turned-off layer not in turned_off",
            Some(not_listed),
            both_ok.clone(),
            true,
        ),
        (
            "turned_off lists only another key",
            Some(other_listed),
            review_ok,
            true,
        ),
        (
            "the project's files clash; the team's off survives",
            Some(set(vec![
                team_off(),
                at(atlas(), key, ""),
                at(atlas(), key, "off as:other"),
            ])),
            team_ok.clone(),
            true,
        ),
        (
            "the project's file is invalid; the team's off survives",
            Some(set(vec![team_off(), at(atlas(), key, "bad")])),
            team_ok.clone(),
            true,
        ),
        (
            "a project file's key can't be read; the team's off survives",
            Some(set(vec![team_off(), at(atlas(), key, "nokey")])),
            team_ok.clone(),
            true,
        ),
        (
            "another key's invalid file doesn't count",
            Some(set(vec![team_off(), at(atlas(), "review", "bad")])),
            team_ok.clone(),
            false,
        ),
        (
            "ruling (b): a conflict, with a sealed team's approved off applying meanwhile",
            Some(set(vec![
                at(team("web"), key, "sealed off"),
                at(team("mobile"), key, ""),
            ])),
            team_ok.clone(),
            true,
        ),
        (
            "a conflict on another key doesn't count",
            Some(set(vec![
                team_off(),
                at(team("web"), "review", "sealed off"),
                at(team("mobile"), "review", ""),
            ])),
            team_ok,
            false,
        ),
    ];
    for (name, set, approved, on) in cases {
        let got = rule_on(set.as_ref(), |layer: &Layer| approves(&approved, layer));
        assert_eq!(got, on, "{name}");
    }
}

#[test]
fn ruling_b_the_conflicted_key_is_turned_off_in_the_set() {
    // What ruling (b) guards against: resolution lists the sealed team's `off` as turned off.
    let set = set(vec![
        at(team("web"), RULE_KEY, "sealed off"),
        at(team("mobile"), RULE_KEY, ""),
    ]);
    assert_eq!(set.turned_off.len(), 1);
    assert_eq!(set.conflicts.len(), 1);
}

fn task(name: &str) -> TaskRef {
    TaskRef(name.into())
}

fn record(name: &str, state: State) -> Record {
    Record {
        task: task(name),
        branch: format!("task/{name}-work"),
        state,
        stage: None,
    }
}

fn target() -> Target {
    Target {
        task: task("chk-T-18"),
        assignee: MemberId::new(MEMBER).unwrap(),
    }
}

fn exception(name: &str, assignee: &str) -> Exception {
    Exception {
        task: task(name),
        assignee: MemberId::new(assignee).unwrap(),
    }
}

/// Allowed, reusing a record or not and using the exception or not; or refused, naming a branch.
#[derive(Debug, PartialEq)]
enum Answer {
    Allowed { reuse: bool, exception_used: bool },
    Refused(String),
}

fn answer(result: Result<Start, OneBranchOpen>) -> Answer {
    match result {
        Ok(start) => Answer::Allowed {
            reuse: start.reuses_record(),
            exception_used: start.exception_used(),
        },
        Err(open) => Answer::Refused(open.branch().to_owned()),
    }
}

fn refused(name: &str) -> Answer {
    Answer::Refused(format!("task/{name}-work"))
}

#[test]
fn start_check() {
    use Answer::Allowed;
    use State::{Archived, Closing, Merged, Open, Reserved};
    let free = || Allowed {
        reuse: false,
        exception_used: false,
    };
    let used = || Allowed {
        reuse: false,
        exception_used: true,
    };
    let reuse = || Allowed {
        reuse: true,
        exception_used: false,
    };
    let mine = exception("chk-T-18", MEMBER);
    let named_like_docs = Record {
        branch: "docs/handbook".into(),
        ..record("chk-T-17", Open)
    };
    type Case = (&'static str, Vec<Record>, Option<Exception>, bool, Answer);
    let cases: Vec<Case> = vec![
        ("no records", vec![], None, true, free()),
        (
            "another task's reserved record",
            vec![record("chk-T-17", Reserved)],
            None,
            true,
            refused("chk-T-17"),
        ),
        (
            "another task's open record",
            vec![record("chk-T-17", Open)],
            None,
            true,
            refused("chk-T-17"),
        ),
        (
            "ruling (a): another task's closing record",
            vec![record("chk-T-17", Closing)],
            None,
            true,
            refused("chk-T-17"),
        ),
        (
            "another task's merged record",
            vec![record("chk-T-17", Merged)],
            None,
            true,
            free(),
        ),
        (
            "another task's archived record",
            vec![record("chk-T-17", Archived)],
            None,
            true,
            free(),
        ),
        (
            "its own reserved record: a reuse",
            vec![record("chk-T-18", Reserved)],
            None,
            true,
            reuse(),
        ),
        (
            "its own open record: a reuse",
            vec![record("chk-T-18", Open)],
            None,
            true,
            reuse(),
        ),
        (
            "its own closing record never blocks, but isn't a reuse",
            vec![record("chk-T-18", Closing)],
            None,
            true,
            free(),
        ),
        (
            "point 2 after an exception was used at point 1: the reservation is reused",
            vec![record("chk-T-17", Open), record("chk-T-18", Reserved)],
            None,
            true,
            reuse(),
        ),
        (
            "a reuse leaves a held exception unused",
            vec![record("chk-T-17", Open), record("chk-T-18", Reserved)],
            Some(mine.clone()),
            true,
            reuse(),
        ),
        (
            "its own closing record with another open: no reuse",
            vec![record("chk-T-17", Open), record("chk-T-18", Closing)],
            None,
            true,
            refused("chk-T-17"),
        ),
        (
            "its own merged record with another open: no reuse",
            vec![record("chk-T-17", Open), record("chk-T-18", Merged)],
            None,
            true,
            refused("chk-T-17"),
        ),
        (
            "its own archived record with another open: no reuse",
            vec![record("chk-T-17", Open), record("chk-T-18", Archived)],
            None,
            true,
            refused("chk-T-17"),
        ),
        // A55: docs, process and retro branches have no record, so with one open the agent's
        // records hold only its task branches. A record blocks by its task, whatever the
        // branch is called.
        (
            "an open docs branch: no record, only its own and merged task records",
            vec![record("chk-T-18", Closing), record("chk-T-17", Merged)],
            None,
            true,
            free(),
        ),
        (
            "a record blocks whatever its branch is called",
            vec![named_like_docs],
            None,
            true,
            Answer::Refused("docs/handbook".into()),
        ),
        (
            "the rule is off",
            vec![record("chk-T-17", Open)],
            None,
            false,
            free(),
        ),
        (
            "an exception for it and its assignee",
            vec![record("chk-T-17", Open)],
            Some(mine.clone()),
            true,
            used(),
        ),
        (
            "an exception is used up even when not needed",
            vec![],
            Some(mine.clone()),
            true,
            used(),
        ),
        (
            "an exception is used up with the rule off",
            vec![],
            Some(mine),
            false,
            used(),
        ),
        (
            "an exception for another task",
            vec![record("chk-T-17", Open)],
            Some(exception("chk-T-17", MEMBER)),
            true,
            refused("chk-T-17"),
        ),
        (
            "an exception for another assignee",
            vec![record("chk-T-17", Open)],
            Some(exception("chk-T-18", OTHER)),
            true,
            refused("chk-T-17"),
        ),
        (
            "an exception for another task isn't used with the rule off",
            vec![],
            Some(exception("chk-T-17", MEMBER)),
            false,
            free(),
        ),
    ];
    for (name, records, exception, rule_on, expected) in cases {
        let got = answer(check_start(
            &records,
            &target(),
            exception.as_ref(),
            rule_on,
        ));
        assert_eq!(got, expected, "{name}");
    }
}

#[test]
fn the_first_blocking_record_is_open_then_closing_then_reserved_then_by_branch_and_task() {
    use State::{Closing, Merged, Open, Reserved};
    let on = |branch: &str, name: &str| Record {
        branch: branch.into(),
        ..record(name, Open)
    };
    let cases: Vec<(&str, Vec<Record>, &str)> = vec![
        (
            "open before closing and reserved, in any input order",
            vec![
                record("chk-T-15", Reserved),
                record("chk-T-16", Closing),
                record("chk-T-17", Open),
            ],
            "chk-T-17",
        ),
        (
            "closing before reserved",
            vec![record("chk-T-15", Reserved), record("chk-T-17", Closing)],
            "chk-T-17",
        ),
        (
            "by branch within a state, not by task",
            vec![on("task/b", "chk-T-15"), on("task/a", "chk-T-17")],
            "chk-T-17",
        ),
        (
            "by task within a branch",
            vec![on("task/a", "chk-T-17"), on("task/a", "chk-T-15")],
            "chk-T-15",
        ),
        (
            "merged and its own closing records are skipped",
            vec![
                record("chk-T-15", Merged),
                record("chk-T-18", Closing),
                record("chk-T-17", Reserved),
            ],
            "chk-T-17",
        ),
    ];
    for (name, records, task) in cases {
        let open = check_start(&records, &target(), None, true).expect_err(name);
        assert_eq!(open.task().0, task, "{name}");
    }
}

#[test]
fn the_refusal_names_the_branch_task_and_stage() {
    let staged = Record {
        stage: Some("waiting for review".into()),
        ..record("chk-T-17", State::Open)
    };
    let open = check_start(&[staged], &target(), None, true).unwrap_err();
    assert_eq!(
        (open.branch(), open.task(), open.stage()),
        (
            "task/chk-T-17-work",
            &task("chk-T-17"),
            Some("waiting for review")
        )
    );
    assert_eq!(
        open.message(),
        "Your branch task/chk-T-17-work isn't merged yet (waiting for review)."
    );
    let unstaged = check_start(&[record("chk-T-17", State::Open)], &target(), None, true);
    let message = unstaged.unwrap_err().message();
    assert_eq!(message, "Your branch task/chk-T-17-work isn't merged yet.");
}
