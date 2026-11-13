//! The "Your processes" section (processes.md §3.4, Appendix C "the render cap and order"),
//! table-driven: each case fails without its guard.

use std::collections::BTreeMap;

use super::*;
use crate::file::{Moments, ProcessFile};
use crate::place::{MemberId, ProjectKey, TeamId};
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

fn key(key: &str) -> ProcessKey {
    ProcessKey::new(key).unwrap()
}

/// A valid file with `key` at `place`, named `key` in capitals, summary "<key> summary.".
/// Flags: `off` turns it off, `sealed` seals it, `start` and `wake` give it a prompt for that
/// moment ("<key> at start", "<key> at wake").
fn file(place: Place, k: &str, flags: &str) -> Input {
    let off = flags.contains("off");
    let prompt = |flag: &str| flags.contains(flag).then(|| format!("{k} at {flag}"));
    let file = ProcessFile {
        id: None,
        key: key(k),
        name: k.to_uppercase(),
        place: place.clone(),
        version: 2,
        owner: "ada".into(),
        summary: format!("{k} summary."),
        applies_to: "everyone".into(),
        overridable: !flags.contains("sealed"),
        status: if off { Status::Off } else { Status::Active },
        reason: off.then(|| "not here".into()),
        overrides: None,
        moments: Moments {
            starting_task: prompt("start"),
            waking_up: prompt("wake"),
            finishing_task: None,
        },
        checklist: Vec::new(),
    };
    Input {
        path: format!("{k}.md"),
        place,
        read: Ok(file),
    }
}

/// The set for work on project atlas with these chains.
fn set(chains: &[&[&str]], files: Vec<Input>) -> EffectiveSet {
    let chain = |c: &&[&str]| c.iter().map(|id| TeamId::new(id).unwrap()).collect();
    let work = Work::new(key_project(), chains.iter().map(chain).collect());
    resolve(&Inputs {
        agent: MemberId::new(MEMBER).unwrap(),
        work: Some(work.unwrap()),
        files,
        names: BTreeMap::new(),
    })
}

fn key_project() -> ProjectKey {
    ProjectKey::new("atlas").unwrap()
}

/// The office's knowledge: each `(key, blob)` of `texts` has the text "Text of <key>.\n" (a
/// blob starting with `long` has 300 bytes more), and `read` gives the blobs read per key.
fn input(texts: &[(&str, &str)], read: &[(&str, &str)]) -> RenderInput {
    let mut input = RenderInput::default();
    for (k, blob) in texts {
        let pad = if blob.starts_with("long") {
            "x".repeat(300)
        } else {
            String::new()
        };
        let text = format!("Text of {k}.{pad}\n");
        let blob = (*blob).into();
        input.texts.insert(key(k), FileText { blob, text });
    }
    for (k, blob) in read {
        input.read.entry(key(k)).or_default().insert((*blob).into());
    }
    input
}

fn delivered(rendered: &Rendered) -> Vec<String> {
    let each = |(k, blob): &(ProcessKey, String)| format!("{}@{blob}", k.as_str());
    rendered.delivered().iter().map(each).collect()
}

/// A case: its name, the set, the input, the cap, the text, and what was delivered.
type Case = (
    &'static str,
    EffectiveSet,
    RenderInput,
    usize,
    String,
    Vec<&'static str>,
);

fn check(cases: Vec<Case>) {
    for (name, set, input, cap, text, expected) in cases {
        let rendered = render_within(&set, &input, cap);
        assert_eq!(rendered.text(), text, "{name}");
        assert_eq!(delivered(&rendered), expected, "{name}");
        assert!(rendered.text().len() <= cap, "{name}");
    }
}

const HEAD: &str = "# Your processes\n\n";
const CHANGED: &str = "\n## Changed since you last read\n";

fn full(name: &str, place: &str, k: &str, pad: bool) -> String {
    let pad = if pad { "x".repeat(300) } else { String::new() };
    format!("\n### {name} ({place} v2)\n\nText of {k}.{pad}\n")
}

fn named(name: &str, place: &str, k: &str) -> String {
    format!("\n### {name} ({place} v2)\n\nRun `org-agent process show {k}` to read it.\n")
}

#[test]
fn the_set_by_level_then_key_with_turned_off_and_conflicts() {
    let mixed = set(
        &[&["web"]],
        vec![
            file(me(), "aaa", ""),
            file(atlas(), "bbb", ""),
            file(team("web"), "ccc", ""),
            file(org(), "zzz", ""),
            file(org(), "yyy", ""),
            file(org(), "off", ""),
            file(team("web"), "off", "off"),
        ],
    );
    let read = input(
        &[
            ("zzz", "1"),
            ("yyy", "1"),
            ("ccc", "1"),
            ("bbb", "1"),
            ("aaa", "1"),
        ],
        &[
            ("zzz", "1"),
            ("yyy", "1"),
            ("ccc", "1"),
            ("bbb", "1"),
            ("aaa", "1"),
        ],
    );
    let lines = [
        "- YYY (org v2): yyy summary.\n",
        "- ZZZ (org v2): zzz summary.\n",
        "- CCC (web v2): ccc summary.\n",
        "- BBB (atlas v2): bbb summary.\n",
        "- AAA (private v2): aaa summary.\n",
        "- off: Turned off for web: not here\n",
    ];
    let mut reversed = mixed.clone();
    reversed.entries.reverse();
    let mut not_active = set(&[], vec![file(org(), "yyy", ""), file(org(), "zzz", "")]);
    not_active.entries[0].winner.as_mut().unwrap().file.status = Status::Off;
    check(vec![
        (
            "by level, then key; turned off after",
            mixed,
            read.clone(),
            CAP,
            format!("{HEAD}{}", lines.concat()),
            vec![],
        ),
        (
            "the order holds whatever the order of the entries",
            reversed,
            read.clone(),
            CAP,
            format!("{HEAD}{}", lines.concat()),
            vec![],
        ),
        (
            "a winner that isn't active isn't shown",
            not_active,
            read,
            CAP,
            format!("{HEAD}{}", lines[1]),
            vec![],
        ),
        (
            "nothing to show: no text at all",
            set(&[], vec![]),
            RenderInput::default(),
            CAP,
            String::new(),
            vec![],
        ),
    ]);
}

/// A conflict on `review` among `teams`, with `applies`, `sealed_by` and `unknown`.
fn conflict_set(
    teams: &[&str],
    applies: Option<Place>,
    sealed: &[&str],
    unknown: &[&str],
) -> EffectiveSet {
    let mut set = EffectiveSet::default();
    set.conflicts.push(Conflict {
        key: key("review"),
        teams: teams.iter().map(|t| team(t)).collect(),
        sealed_by: sealed.iter().map(|t| team(t)).collect(),
        unknown: unknown.iter().map(|p| (*p).into()).collect(),
        applies,
    });
    set
}

#[test]
fn conflicts_in_the_spec_wording() {
    let mut names = RenderInput::default();
    names.names.insert(team("web"), "Web".into());
    let line = |text: &str| format!("{HEAD}- {text}\n");
    let resolved = set(
        &[&["web"], &["ios"]],
        vec![
            file(org(), "review", ""),
            file(team("web"), "review", ""),
            file(team("ios"), "review", ""),
        ],
    );
    let read = input(&[("review", "1")], &[("review", "1")]);
    check(vec![
        (
            "from resolve: led by the name of the version that applies",
            resolved,
            read,
            CAP,
            format!(
                "{HEAD}- REVIEW (org v2): review summary.\n- REVIEW: ios and web \
                 disagree. The organization's version applies until the project decides.\n"
            ),
            vec![],
        ),
        (
            "nothing applies: led by the key; place names from the input",
            conflict_set(&["web", "ios"], None, &[], &[]),
            names.clone(),
            CAP,
            line("review: Web and ios disagree. No version applies until the project decides."),
            vec![],
        ),
        (
            "a sealed team's version applies",
            conflict_set(&["web", "ios"], Some(team("web")), &["web"], &[]),
            names.clone(),
            CAP,
            line("review: Web and ios disagree. Sealed by Web; applies until the project decides."),
            vec![],
        ),
        (
            "two seals: the organization's version, and the other seals named",
            conflict_set(&["web", "ios", "mac"], Some(org()), &["web", "ios"], &[]),
            names.clone(),
            CAP,
            line(
                "review: Web, ios and mac disagree. The organization's version applies until \
                 the project decides. Sealed by Web. Sealed by ios.",
            ),
            vec![],
        ),
        (
            "a can't-tell seal applies: not called sealed by its team",
            conflict_set(
                &["web", "ios"],
                Some(team("ios")),
                &["web"],
                &["teams/x/processes/a.md"],
            ),
            names,
            CAP,
            line(
                "review: Web and ios disagree. ios's version applies until the project decides. \
                 Sealed by Web. Can't tell: teams/x/processes/a.md can't be read.",
            ),
            vec![],
        ),
    ]);
}

#[test]
fn changed_since_last_read_in_full_organization_first() {
    let three = || {
        set(
            &[&["web"]],
            vec![
                file(team("web"), "aaa", ""),
                file(org(), "bbb", ""),
                file(org(), "ccc", ""),
            ],
        )
    };
    let sums = "- BBB (org v2): bbb summary.\n- CCC (org v2): ccc summary.\n\
                - AAA (web v2): aaa summary.\n";
    let texts = [("aaa", "2"), ("bbb", "2"), ("ccc", "2")];
    let b = full("BBB", "org", "bbb", false);
    let c = full("CCC", "org", "ccc", false);
    let a = full("AAA", "web", "aaa", false);
    check(vec![
        (
            "never read, read an older blob, read this blob",
            three(),
            input(&texts, &[("bbb", "1"), ("ccc", "1"), ("ccc", "2")]),
            CAP,
            format!("{HEAD}{sums}{CHANGED}{b}{a}"),
            vec!["bbb@2", "aaa@2"],
        ),
        (
            "all changed: organization first, then by key",
            three(),
            input(&texts, &[]),
            CAP,
            format!("{HEAD}{sums}{CHANGED}{b}{c}{a}"),
            vec!["bbb@2", "ccc@2", "aaa@2"],
        ),
        (
            "a blob read for another key doesn't count",
            three(),
            input(
                &texts,
                &[("aaa", "1"), ("bbb", "1"), ("ccc", "1"), ("zzz", "2")],
            ),
            CAP,
            format!("{HEAD}{sums}{CHANGED}{b}{c}{a}"),
            vec!["bbb@2", "ccc@2", "aaa@2"],
        ),
        (
            "no text from the office: named, never delivered, even if something was read",
            three(),
            input(
                &[("aaa", "2"), ("ccc", "2")],
                &[("aaa", "2"), ("ccc", "2"), ("bbb", "2")],
            ),
            CAP,
            format!("{HEAD}{sums}{CHANGED}{}", named("BBB", "org", "bbb")),
            vec![],
        ),
    ]);
}

#[test]
fn the_cap_summaries_first_then_changed_texts_then_names() {
    let two = || set(&[], vec![file(org(), "aaa", ""), file(org(), "bbb", "")]);
    let sums = "- AAA (org v2): aaa summary.\n- BBB (org v2): bbb summary.\n";
    let sum_a = "- AAA (org v2): aaa summary.\n";
    let a = full("AAA", "org", "aaa", false);
    let a_long = full("AAA", "org", "aaa", true);
    let b = full("BBB", "org", "bbb", false);
    let na = named("AAA", "org", "aaa");
    let nb = named("BBB", "org", "bbb");
    let more = "and 1 more: run `org-agent process list`\n";
    let more_changed = "\nand 1 more changed: run `org-agent process list`\n";
    let at = |parts: &[&str]| parts.concat().len();
    let three = || {
        set(
            &[],
            vec![
                file(org(), "aaa", ""),
                file(org(), "bbb", ""),
                file(org(), "ccc", ""),
            ],
        )
    };
    let three_read = [("aaa", "1"), ("bbb", "1"), ("ccc", "1")];
    let more2 = more.replace('1', "2");
    let more3 = more.replace('1', "3");
    let ccc_changed = input(
        &[("aaa", "1"), ("bbb", "1"), ("ccc", "long")],
        &three_read[..2],
    );
    let mut moment_on = input(&three_read, &three_read);
    moment_on
        .moments
        .insert(Moment::WakingUp, "Wake well. ".repeat(10));
    let more_moments =
        "\nand 1 more moments: process context limit reached (`org-agent process list`)\n";
    let mut long_first = three();
    long_first.entries[0].winner.as_mut().unwrap().file.summary = "x".repeat(150);
    check(vec![
        (
            "summaries past the cap keep room to say something changed",
            three(),
            ccc_changed,
            at(&[HEAD, sum_a, &more2, CHANGED, more_changed]),
            format!("{HEAD}{sum_a}{more2}{CHANGED}{more_changed}"),
            vec![],
        ),
        (
            "summaries past the cap keep room to say a moment is On",
            three(),
            moment_on,
            at(&[HEAD, sum_a, &more2, more_moments]),
            format!("{HEAD}{sum_a}{more2}{more_moments}"),
            vec![],
        ),
        (
            "summaries are a prefix: a long first one leaves out the rest, though they'd fit",
            long_first,
            input(&three_read, &three_read),
            at(&[
                HEAD,
                "- BBB (org v2): bbb summary.\n- CCC (org v2): ccc summary.\n",
                more,
            ]),
            format!("{HEAD}{more3}"),
            vec![],
        ),
        (
            "summaries past the cap: the count line fits in their place",
            set(
                &[],
                vec![
                    file(org(), "aaa", ""),
                    file(org(), "bbb", ""),
                    file(org(), "ccc", ""),
                ],
            ),
            input(&three_read, &three_read),
            at(&[HEAD, sum_a, more]),
            format!("{HEAD}{sum_a}{}", more.replace('1', "2")),
            vec![],
        ),
        (
            "everything fits exactly",
            two(),
            input(&[("aaa", "2"), ("bbb", "2")], &[]),
            at(&[HEAD, sums, CHANGED, &a, &b]),
            format!("{HEAD}{sums}{CHANGED}{a}{b}"),
            vec!["aaa@2", "bbb@2"],
        ),
        (
            "a text that doesn't fit is named; the rest after it too, though one would fit",
            two(),
            input(&[("aaa", "long"), ("bbb", "2")], &[]),
            at(&[HEAD, sums, CHANGED, &na, &nb]),
            format!("{HEAD}{sums}{CHANGED}{na}{nb}"),
            vec![],
        ),
        (
            "a text is delivered only with room left to name every later one",
            two(),
            input(&[("aaa", "long"), ("bbb", "2")], &[]),
            at(&[HEAD, sums, CHANGED, &a_long]),
            format!("{HEAD}{sums}{CHANGED}{na}{nb}"),
            vec![],
        ),
        (
            "the names don't all fit: those that do, then the count",
            two(),
            input(&[("aaa", "long1"), ("bbb", "long2")], &[]),
            at(&[HEAD, sums, CHANGED, &na, more_changed]),
            format!("{HEAD}{sums}{CHANGED}{na}{more_changed}"),
            vec![],
        ),
        (
            "too small for every count line: the earlier part's wins",
            two(),
            input(&[("aaa", "2"), ("bbb", "2")], &[]),
            at(&[HEAD, sums, CHANGED]) + 10,
            format!("{HEAD}{}", more.replace('1', "2")),
            vec![],
        ),
        (
            "a heading that doesn't fit leaves out its section, though a part of it would fit",
            set(&[], vec![]),
            RenderInput {
                moments: BTreeMap::from([(Moment::WakingUp, String::new())]),
                ..RenderInput::default()
            },
            HEAD.len() - 1,
            String::new(),
            vec![],
        ),
    ]);
}

#[test]
fn moment_prompts_follow_their_guidance_only_while_on_and_last() {
    let files = || {
        set(
            &[&["web"]],
            vec![
                file(team("web"), "aaa", "start wake"),
                file(org(), "bbb", "start"),
                file(org(), "ccc", ""),
            ],
        )
    };
    let sums = "- BBB (org v2): bbb summary.\n- CCC (org v2): ccc summary.\n\
                - AAA (web v2): aaa summary.\n";
    let all = [("aaa", "1"), ("bbb", "1"), ("ccc", "1")];
    let on = |moments: &[(Moment, &str)], read: &[(&str, &str)]| {
        let mut input = input(&all, read);
        for (moment, guidance) in moments {
            input.moments.insert(*moment, format!("{guidance}\n\n"));
        }
        input
    };
    let start = "\n## Starting a task\n\nStart well.\n\n- BBB: bbb at start\n- AAA: aaa at start\n";
    let wake = "\n## Waking up\n\nWake well.\n\n- AAA: aaa at wake\n";
    let finish = "\n## Finishing a task\n\nFinish well.\n";
    let more = "\nand 1 more moments: process context limit reached (`org-agent process list`)\n";
    let b_long = full("BBB", "org", "bbb", true);
    let long_start = "Start well. ".repeat(10);
    let at = |parts: &[&str]| parts.concat().len();
    let read = [("aaa", "1"), ("ccc", "1")];
    let mut long = on(&[(Moment::StartingTask, &long_start)], &read);
    long.texts.get_mut(&key("bbb")).unwrap().blob = "long".into();
    long.texts.get_mut(&key("bbb")).unwrap().text = format!("Text of bbb.{}\n", "x".repeat(300));
    check(vec![
        (
            "each On moment: its guidance, then the prompts for it",
            files(),
            on(
                &[
                    (Moment::WakingUp, "Wake well."),
                    (Moment::StartingTask, "Start well."),
                ],
                &all,
            ),
            CAP,
            format!("{HEAD}{sums}{start}{wake}"),
            vec![],
        ),
        (
            "an Off moment shows nothing; one with no prompts shows its guidance",
            files(),
            on(&[(Moment::FinishingTask, "Finish well.")], &all),
            CAP,
            format!("{HEAD}{sums}{finish}"),
            vec![],
        ),
        (
            "changed texts come before moments in the cap",
            files(),
            long.clone(),
            at(&[HEAD, sums, CHANGED, &b_long, more]),
            format!("{HEAD}{sums}{CHANGED}{b_long}{more}"),
            vec!["bbb@long"],
        ),
        (
            "changed texts don't take the room kept for the moments' count line",
            files(),
            long.clone(),
            at(&[HEAD, sums, CHANGED, &b_long, more]) - 1,
            format!(
                "{HEAD}{sums}{CHANGED}{}\n## Starting a task\n\n{}\n\n- BBB: bbb at start\n\
                 - AAA: aaa at start\n",
                named("BBB", "org", "bbb"),
                long_start.trim_end()
            ),
            vec![],
        ),
    ]);
}

#[test]
fn the_cap_holds_for_every_cap_and_only_whole_texts_are_delivered() {
    let files = set(
        &[&["web"]],
        vec![
            file(team("web"), "aaa", "start"),
            file(org(), "bbb", "start"),
            file(org(), "off", ""),
            file(team("web"), "off", "off"),
        ],
    );
    let mut input = input(&[("aaa", "long"), ("bbb", "2")], &[]);
    input
        .moments
        .insert(Moment::StartingTask, "Ünïcode guidance.".into());
    let whole = render(&files, &input).text().len();
    for cap in 0..=whole + 1 {
        let rendered = render_within(&files, &input, cap);
        let text = rendered.text();
        assert!(text.len() <= cap, "cap {cap}");
        assert!(
            text.is_empty() || text.starts_with(HEAD),
            "cap {cap}: {text:?}"
        );
        for (k, blob) in rendered.delivered() {
            let body = &input.texts[k];
            assert_eq!(&body.blob, blob, "cap {cap}");
            assert!(
                text.contains(&body.text),
                "cap {cap}: {} delivered, not shown",
                k.as_str()
            );
        }
        let shown = |k: &str| text.contains(&input.texts[&key(k)].text);
        let listed = |k: &str| rendered.delivered().iter().any(|(d, _)| d.as_str() == k);
        assert_eq!(shown("aaa"), listed("aaa"), "cap {cap}");
        assert_eq!(shown("bbb"), listed("bbb"), "cap {cap}");
    }
    assert_eq!(render_within(&files, &input, whole).text().len(), whole);
}

#[test]
fn the_real_cap_is_24_kib_of_utf8() {
    let files = (0..200)
        .map(|n| file(org(), &format!("p{n:03}"), ""))
        .collect();
    let mut many = set(&[], files);
    for entry in &mut many.entries {
        entry.winner.as_mut().unwrap().file.summary = "é".repeat(100);
    }
    // Every text read, so the summaries alone pass the cap.
    let keys: Vec<String> = (0..200).map(|n| format!("p{n:03}")).collect();
    let all: Vec<(&str, &str)> = keys.iter().map(|k| (k.as_str(), "1")).collect();
    let rendered = render(&many, &input(&all, &all));
    let text = rendered.text();
    assert!(text.len() <= 24 * 1024, "{}", text.len());
    assert!(text.len() > 24 * 1024 - 300, "{}", text.len());
    assert!(
        text.ends_with(" more: run `org-agent process list`\n"),
        "{text}"
    );
}

#[test]
fn each_process_is_labelled_by_its_place() {
    let files = || {
        set(
            &[&["web"], &["ios"]],
            vec![
                file(org(), "aaa", ""),
                file(team("web"), "bbb", ""),
                file(team("ios"), "ccc", ""),
                file(atlas(), "ddd", ""),
                file(me(), "eee", ""),
            ],
        )
    };
    let all = ["aaa", "bbb", "ccc", "ddd", "eee"].map(|k| (k, "1"));
    let mut named = input(&all, &all);
    named.names.insert(team("web"), "Web".into());
    named.names.insert(atlas(), "Atlas".into());
    named.names.insert(me(), "Noor Haddad".into());
    let mut changed = named.clone();
    changed.read.clear();
    let lines = |web: &str, project: &str| {
        format!(
            "{HEAD}- AAA (org v2): aaa summary.\n- BBB ({web} v2): bbb summary.\n\
             - CCC (ios v2): ccc summary.\n- DDD ({project} v2): ddd summary.\n\
             - EEE (private v2): eee summary.\n"
        )
    };
    let texts = [
        full("AAA", "org", "aaa", false),
        full("BBB", "Web", "bbb", false),
        full("CCC", "ios", "ccc", false),
        full("DDD", "Atlas", "ddd", false),
        full("EEE", "private", "eee", false),
    ];
    let delivered = vec!["aaa@1", "bbb@1", "ccc@1", "ddd@1", "eee@1"];
    check(vec![
        (
            "org; a named team or project by its name, an unnamed team by its id; private \
             never by the agent's name or id",
            files(),
            named,
            CAP,
            lines("Web", "Atlas"),
            vec![],
        ),
        (
            "no names: a team by its id, the project by its key",
            files(),
            input(&all, &all),
            CAP,
            lines("web", "atlas"),
            vec![],
        ),
        (
            "changed texts carry the same labels",
            files(),
            changed,
            CAP,
            format!("{}{CHANGED}{}", lines("Web", "Atlas"), texts.concat()),
            delivered,
        ),
    ]);
}
