//! The checklist merge (processes.md §3.4 Checklists, PROC-12, Appendix C), table-driven: each
//! case fails without its guard.

use std::collections::BTreeMap;

use super::*;
use crate::file::{Moments, ProcessFile};
use crate::place::{MemberId, Place, ProcessKey, ProjectKey, TeamId};
use crate::resolve::{resolve, Input, Inputs, Work};

const MEMBER: &str = "01JD8Z3F0000000000000000CD";

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

/// Steps from `"text"` (optional) and `"text!"` (required).
fn steps(texts: &[&str]) -> Vec<Step> {
    let step = |t: &&str| match t.strip_suffix('!') {
        Some(text) => Step {
            text: text.into(),
            required: true,
        },
        None => Step {
            text: (*t).into(),
            required: false,
        },
    };
    texts.iter().map(step).collect()
}

/// A valid file with `key` at `place` whose checklist is `texts` (see [`steps`]); `sealed`
/// seals it, `off` turns it off.
fn file(place: Place, key: &str, texts: &[&str], flags: &str) -> Input {
    let off = flags.contains("off");
    let file = ProcessFile {
        id: None,
        key: ProcessKey::new(key).unwrap(),
        name: key.into(),
        place: place.clone(),
        version: 1,
        owner: "ada".into(),
        summary: "How we do it.".into(),
        applies_to: "everyone".into(),
        overridable: !flags.contains("sealed"),
        status: if off { Status::Off } else { Status::Active },
        reason: off.then(|| "not here".into()),
        overrides: None,
        moments: Moments::default(),
        checklist: steps(texts),
    };
    Input {
        path: format!("{key}.md"),
        place,
        read: Ok(file),
    }
}

/// The set for the work with these chains on project atlas.
fn set(chains: &[&[&str]], files: Vec<Input>) -> EffectiveSet {
    let chain = |c: &&[&str]| c.iter().map(|id| TeamId::new(id).unwrap()).collect();
    let work = Work::new(
        ProjectKey::new("atlas").unwrap(),
        chains.iter().map(chain).collect(),
    );
    resolve(&Inputs {
        agent: MemberId::new(MEMBER).unwrap(),
        work: Some(work.unwrap()),
        files,
        names: BTreeMap::new(),
    })
}

/// A checklist in words: `"text"` or `"text!"` for a required step.
fn words(steps: &[Step]) -> Vec<String> {
    let word = |s: &Step| format!("{}{}", s.text, if s.required { "!" } else { "" });
    steps.iter().map(word).collect()
}

type Case = (
    &'static str,
    &'static [&'static str],
    EffectiveSet,
    &'static [&'static str],
);

fn check(cases: Vec<Case>) {
    for (name, default, set, expected) in cases {
        assert_eq!(words(&checklist(&steps(default), &set)), expected, "{name}");
    }
}

#[test]
fn order_default_first_then_by_level_then_key() {
    check(vec![
        (
            "the default checklist first, then the processes' steps",
            &["Plan", "Test!"],
            set(&[], vec![file(org(), "review", &["Review!"], "")]),
            &["Plan", "Test!", "Review!"],
        ),
        (
            "an empty default: the processes' steps only",
            &[],
            set(&[], vec![file(org(), "review", &["Review"], "")]),
            &["Review"],
        ),
        (
            "by level (organization, team, project) before key",
            &[],
            set(
                &[&["web"]],
                vec![
                    file(atlas(), "aaa", &["Project step"], ""),
                    file(team("web"), "bbb", &["Team step"], ""),
                    file(org(), "zzz", &["Org step"], ""),
                ],
            ),
            &["Org step", "Team step", "Project step"],
        ),
        (
            "within a level, by key; each file's steps in its own order",
            &[],
            set(
                &[],
                vec![
                    file(org(), "merge", &["M1", "M2"], ""),
                    file(org(), "build", &["B2", "B1"], ""),
                ],
            ),
            &["B2", "B1", "M1", "M2"],
        ),
    ]);
}

#[test]
fn only_the_projects_applying_processes_add_steps() {
    check(vec![
        (
            "a private (agent-level) process adds nothing (never G)",
            &["Plan"],
            set(
                &[],
                vec![
                    file(org(), "review", &["Review"], ""),
                    file(me(), "mine", &["My step!"], ""),
                ],
            ),
            &["Plan", "Review"],
        ),
        (
            "a turned-off process adds nothing",
            &[],
            set(
                &[&["web"]],
                vec![
                    file(org(), "review", &["Review!"], ""),
                    file(team("web"), "review", &[], "off"),
                ],
            ),
            &[],
        ),
        (
            "an overridden version adds nothing; the override's steps do",
            &[],
            set(
                &[&["web"]],
                vec![
                    file(org(), "review", &["Org review"], ""),
                    file(team("web"), "review", &["Web review"], ""),
                ],
            ),
            &["Web review"],
        ),
        (
            "a conflict adds only the version that applies (here a sealed team's)",
            &[],
            set(
                &[&["web"], &["mobile"]],
                vec![
                    file(org(), "review", &["Org review"], ""),
                    file(team("web"), "review", &["Web review"], "sealed"),
                    file(team("mobile"), "review", &["Mobile review"], ""),
                ],
            ),
            &["Web review"],
        ),
        (
            "a conflict with nothing applying adds nothing",
            &[],
            set(
                &[&["web"], &["mobile"]],
                vec![
                    file(team("web"), "review", &["Web review"], ""),
                    file(team("mobile"), "review", &["Mobile review"], ""),
                ],
            ),
            &[],
        ),
    ]);
}

#[test]
fn a_winner_that_is_not_active_adds_nothing() {
    // A set from `resolve` never has one; the set's fields are public, so `checklist` holds for
    // any set.
    let mut off = set(&[], vec![file(org(), "review", &["Review"], "")]);
    let winner = off.entries[0].winner.as_mut().unwrap();
    winner.file.status = Status::Off;
    check(vec![("an off winner", &["Plan"], off, &["Plan"])]);
}

#[test]
fn the_order_is_by_key_whatever_the_order_of_the_entries() {
    // `resolve` gives entries by key; the set's fields are public, so the order mustn't hang on
    // that.
    let mut set = set(
        &[],
        vec![
            file(org(), "build", &["Build"], ""),
            file(org(), "merge", &["Merge"], ""),
        ],
    );
    set.entries.reverse();
    check(vec![("entries reversed", &[], set, &["Build", "Merge"])]);
}

#[test]
fn duplicates_merge_into_the_earlier_step_and_required_wins() {
    let two = |a: &'static [&'static str], b: &'static [&'static str]| {
        set(
            &[],
            vec![file(org(), "aaa", a, ""), file(org(), "bbb", b, "")],
        )
    };
    check(vec![
        (
            "a process step repeating a default step keeps the default's place",
            &["Test", "Plan"],
            two(&["Review", "Test"], &[]),
            &["Test", "Plan", "Review"],
        ),
        (
            "required wins when the later copy is required",
            &["Test"],
            two(&["Test!"], &[]),
            &["Test!"],
        ),
        (
            "required wins when the earlier copy is required",
            &["Test!"],
            two(&["Test"], &[]),
            &["Test!"],
        ),
        (
            "two processes' copies merge into the first",
            &[],
            two(&["Review", "Merge"], &["Lint", "Review!"]),
            &["Review!", "Merge", "Lint"],
        ),
        (
            "copies inside the default checklist merge too",
            &["Plan", "Plan!"],
            two(&[], &[]),
            &["Plan!"],
        ),
        (
            "texts are compared trimmed; the earlier text is kept",
            &[" Plan "],
            two(&["Plan!"], &[]),
            &[" Plan !"],
        ),
        (
            "letter case and inner spaces count: no merge",
            &["Plan it"],
            two(&["plan it", "Plan  it"], &[]),
            &["Plan it", "plan it", "Plan  it"],
        ),
    ]);
}
