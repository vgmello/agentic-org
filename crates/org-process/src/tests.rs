use super::*;

const ULID: &str = "01jd8z3f0000000000000000ab";
const MEMBER: &str = "01JD8Z3F0000000000000000CD";

fn project() -> ProjectKey {
    ProjectKey::new("atlas").unwrap()
}

fn team() -> Place {
    Place::Team(TeamId::new("web").unwrap())
}

/// A valid team-level file; `edit` changes its front-matter lines before it is joined.
fn file(edit: impl FnOnce(&mut Vec<String>)) -> String {
    let mut lines: Vec<String> = [
        format!("id: doc-{ULID}"),
        "process: review-and-merge".into(),
        "name: Review and merge".into(),
        "kind: process".into(),
        "level: team".into(),
        "scope: web".into(),
        "version: 3".into(),
        "owner: ac-staff-engineer-ada".into(),
        "summary: Own review; the reviewer's READY; the lead's go; merge.".into(),
        "applies_to: every agent that changes a repository".into(),
        "overridable: true".into(),
        "status: active".into(),
        "overrides: review-and-merge@2".into(),
        "moments:".into(),
        "  waking_up: \"Check on your branch first.\"".into(),
        "checklist:".into(),
        "  - { text: \"Own fresh review\", required: true }".into(),
        "  - { text: \"The lead's go\" }".into(),
    ]
    .into();
    edit(&mut lines);
    format!("---\n{}\n---\n# Review and merge\n", lines.join("\n"))
}

type Edit = Box<dyn FnOnce(&mut Vec<String>)>;

/// Removes `key`'s line and its indented block; returns where it was.
fn take_block(lines: &mut Vec<String>, key: &str) -> usize {
    let at = lines
        .iter()
        .position(|l| l.starts_with(&format!("{key}:")))
        .unwrap();
    let end = (at + 1..lines.len())
        .find(|&n| !lines[n].starts_with(' '))
        .unwrap_or(lines.len());
    lines.drain(at..end);
    at
}

fn set(key: &str, value: &str) -> Edit {
    let (key, value) = (key.to_owned(), value.to_owned());
    Box::new(move |lines| {
        let at = take_block(lines, &key);
        lines.insert(at, format!("{key}: {value}"));
    })
}

fn drop_key(key: &str) -> Edit {
    let key = key.to_owned();
    Box::new(move |lines| {
        take_block(lines, &key);
    })
}

#[test]
fn paths_map_to_their_place_and_nothing_else_does() {
    let key = project();
    let member = MemberId::new(MEMBER).unwrap();
    let cases = [
        (
            Repo::Handbook,
            "processes/review.md",
            Some(Place::Organization),
        ),
        (
            Repo::Handbook,
            "teams/web/processes/review.md",
            Some(team()),
        ),
        (
            Repo::Handbook,
            &format!("agents/{MEMBER}/processes/mine.md"),
            Some(Place::Agent(member)),
        ),
        (
            Repo::Project(&key),
            "docs/processes/review.md",
            Some(Place::Project(key.clone())),
        ),
        (Repo::Handbook, "processes/sub/review.md", None),
        (Repo::Handbook, "processes/review.txt", None),
        (Repo::Handbook, "processes/.md", None),
        (Repo::Handbook, "processes/.hidden.md", None),
        (Repo::Handbook, "processes/../processes/review.md", None),
        (Repo::Handbook, "/processes/review.md", None),
        (Repo::Handbook, "processes//review.md", None),
        (Repo::Handbook, "teams/w eb/processes/review.md", None),
        (Repo::Handbook, "agents/short/processes/mine.md", None),
        (Repo::Handbook, "docs/processes/review.md", None),
        (Repo::Project(&key), "processes/review.md", None),
        (Repo::Project(&key), "docs/processes/x/review.md", None),
    ];
    for (repo, path, expected) in cases {
        assert_eq!(location(repo, path), expected, "{path}");
    }
}

#[test]
fn ids_and_keys_have_their_shapes() {
    assert!(ProcessKey::new("review-and-merge").is_some() && ProcessKey::new("1x").is_some());
    for bad in ["", "-x", "Review", "a_b", &"a".repeat(65)] {
        assert!(ProcessKey::new(bad).is_none(), "{bad}");
    }
    assert!(ProcessKey::new(&"a".repeat(64)).is_some());
    assert!(
        TeamId::new("Web_team-1").is_some()
            && TeamId::new("").is_none()
            && TeamId::new("a/b").is_none()
    );
    assert!(
        ProjectKey::new("ab").is_some()
            && ProjectKey::new("a").is_none()
            && ProjectKey::new("Ab").is_none()
    );
    assert!(MemberId::new(MEMBER).is_some() && MemberId::new(&MEMBER[1..]).is_none());
    for good in ["ab", "a1", "abcdefghij"] {
        assert!(ProjectKey::new(good).is_some(), "{good}");
    }
    for bad in ["a", "Ab", "1a", "abcdefghijk", "a-b"] {
        assert!(ProjectKey::new(bad).is_none(), "{bad}");
    }
    assert!(TeamId::new(&"t".repeat(64)).is_some() && TeamId::new(&"t".repeat(65)).is_none());
    assert!(MemberId::new(&format!("{}-", &MEMBER[1..])).is_none());
    for level in [
        Level::Organization,
        Level::Team,
        Level::Project,
        Level::Agent,
    ] {
        assert_eq!(Level::parse(level.as_str()), Some(level));
    }
    assert_eq!(Level::parse("Team"), None);
}

#[test]
fn a_valid_file_reads_with_every_field() {
    let read = read(&team(), &file(|_| {})).unwrap();
    assert_eq!(read.id, Some(format!("doc-{ULID}")));
    // A doc written with git has no id until its first save through the tool.
    assert_eq!(
        super::read(&team(), &file(drop_key("id"))).unwrap().id,
        None
    );
    assert_eq!(read.key.as_str(), "review-and-merge");
    assert_eq!(
        (read.place.clone(), read.version, read.overridable),
        (team(), 3, true)
    );
    assert_eq!(read.status, Status::Active);
    assert_eq!(
        read.overrides,
        Some((ProcessKey::new("review-and-merge").unwrap(), 2))
    );
    assert_eq!(
        read.moments.waking_up.as_deref(),
        Some("Check on your branch first.")
    );
    assert_eq!(
        (read.moments.starting_task, read.moments.finishing_task),
        (None, None)
    );
    let steps = [("Own fresh review", true), ("The lead's go", false)];
    let got: Vec<_> = read
        .checklist
        .iter()
        .map(|s| (s.text.as_str(), s.required))
        .collect();
    assert_eq!(got, steps);
    // The organization's level has no scope; null and absent are the same.
    for edit in [set("scope", "null"), drop_key("scope")] {
        let org = file(|l| {
            edit(l);
            set("level", "organization")(l);
        });
        assert_eq!(
            super::read(&Place::Organization, &org).unwrap().place,
            Place::Organization
        );
    }
    for (status, expected) in [("draft", Status::Draft), ("retired", Status::Retired)] {
        assert_eq!(
            super::read(&team(), &file(set("status", status)))
                .unwrap()
                .status,
            expected
        );
    }
}

#[test]
fn every_problem_names_its_field() {
    use ProblemKind::*;
    let long = |n| format!("\"{}\"", "x".repeat(n));
    let steps = |n| format!("[{}]", vec!["{text: s}"; n].join(", "));
    let cases: Vec<(String, &str, ProblemKind)> = vec![
        (
            file(|l| l.push("sealled: true".into())),
            "sealled",
            Unrecognized,
        ),
        (
            file(|l| l.insert(15, "  sometime: x".into())),
            "moments.sometime",
            Unrecognized,
        ),
        (
            file(set("checklist", "[{text: a}, {text: b, extra: 1}]")),
            "checklist[1].extra",
            Unrecognized,
        ),
        (file(drop_key("name")), "name", Missing),
        (file(set("name", "null")), "name", Missing),
        (
            file(set("id", "doc-01JD8Z3F0000000000000000AB")),
            "id",
            Invalid,
        ),
        (file(set("id", "ADR-0001")), "id", Invalid),
        (file(set("kind", "adr")), "kind", Invalid),
        (file(set("level", "project")), "level", WrongPlace),
        (file(set("level", "3")), "level", Invalid),
        (file(set("scope", "mobile")), "scope", WrongPlace),
        (file(drop_key("scope")), "scope", WrongPlace),
        (file(set("scope", "[web]")), "scope", Invalid),
        (file(set("version", "0")), "version", Invalid),
        (file(set("version", "\"3\"")), "version", Invalid),
        (file(set("version", "4294967296")), "version", Invalid),
        (file(set("status", "paused")), "status", Invalid),
        (file(set("overridable", "\"yes\"")), "overridable", Invalid),
        (file(set("overridable", "yes")), "overridable", Invalid),
        (
            file(set("overrides", "review-and-merge")),
            "overrides",
            Invalid,
        ),
        (
            file(set("overrides", "review-and-merge@0")),
            "overrides",
            Invalid,
        ),
        (file(set("overrides", "Review@1")), "overrides", Invalid),
        (file(set("process", "Review_Merge")), "process", Invalid),
        (file(set("summary", &long(201))), "summary", Invalid),
        (file(set("summary", "\"two\\nlines\"")), "summary", Invalid),
        (file(set("summary", "\" \"")), "summary", Invalid),
        (
            file(set(
                "moments",
                &("{waking_up: ".to_owned() + &long(501) + "}"),
            )),
            "moments.waking_up",
            Invalid,
        ),
        (file(set("moments", "[x]")), "moments", Invalid),
        (file(set("checklist", &steps(51))), "checklist", Invalid),
        (
            file(set("checklist", "[{required: true}]")),
            "checklist[0].text",
            Missing,
        ),
        (
            file(set("checklist", "[{text: a, required: maybe}]")),
            "checklist[0].required",
            Invalid,
        ),
        (file(set("checklist", "[x]")), "checklist[0]", Invalid),
        (
            file(|l| l.push("name: again".into())),
            "name",
            FrontMatter(org_docs::Reason::DuplicateKey),
        ),
        (
            file(set("id", "doc-81jd8z3f0000000000000000ab")),
            "id",
            Invalid,
        ),
        (
            file(set("id", "doc-01jd8z3f0000000000000000il")),
            "id",
            Invalid,
        ),
        (
            file(set("id", "doc-01jd8z3f0000000000000000a")),
            "id",
            Invalid,
        ),
        (file(set("level", "bogus")), "level", Invalid),
        (file(set("level", "Team")), "level", Invalid),
        (file(set("overrides", "other-key@1")), "overrides", Invalid),
        (
            file(set("overrides", "review-and-merge@01")),
            "overrides",
            Invalid,
        ),
        (
            file(set("overrides", "review-and-merge@+1")),
            "overrides",
            Invalid,
        ),
        (
            file(set("overrides", "review-and-merge@1@2")),
            "overrides",
            Invalid,
        ),
        (file(set("summary", "\"a\\u2028b\"")), "summary", Invalid),
        (file(set("summary", "\"a\\tb\"")), "summary", Invalid),
        (file(set("summary", "\"a\\rb\"")), "summary", Invalid),
        (file(set("summary", "\"trailing \"")), "summary", Invalid),
        (
            file(set("summary", &format!("\"{}\"", "é".repeat(201)))),
            "summary",
            Invalid,
        ),
        (
            file(set("checklist", "[{text: null}]")),
            "checklist[0].text",
            Missing,
        ),
        (file(set("status", "off")), "reason", Missing),
        (file(set("summary", "\"a\\u2029b\"")), "summary", Invalid),
        (file(set("summary", "\"a\\u202eb\"")), "summary", Invalid),
        (file(set("summary", "\"a\\u200bb\"")), "summary", Invalid),
        (file(set("summary", "\"a\\u2066b\"")), "summary", Invalid),
        (
            file(|l| {
                l.push("zz: 1".into());
                l.push("aa: 1".into());
            }),
            "aa",
            Unrecognized,
        ),
        ("# No front matter\n".into(), "", NoFrontMatter),
    ];
    for (text, field, kind) in cases {
        let problem = read(&team(), &text).unwrap_err();
        assert_eq!(
            (problem.field.as_str(), problem.kind),
            (field, kind),
            "{field}"
        );
    }
    // The limits themselves are allowed.
    assert!(read(&team(), &file(set("summary", &long(200)))).is_ok());
    let at_limit = "{waking_up: ".to_owned() + &long(500) + "}";
    assert!(read(&team(), &file(set("moments", &at_limit))).is_ok());
    assert!(read(&team(), &file(set("checklist", &steps(50)))).is_ok());
}

#[test]
fn the_scope_must_match_and_an_invalid_file_keeps_what_could_be_read() {
    // The organization has no scope, so a scope there is out of place.
    let org = file(|l| {
        set("level", "organization")(l);
        set("scope", "web")(l);
    });
    let problem = read(&Place::Organization, &org).unwrap_err();
    assert_eq!(
        (problem.field.as_str(), problem.kind),
        ("scope", ProblemKind::WrongPlace)
    );
    // An invalid file still reports its key and whether it is sealed (§3.2 rule 2).
    let sealed_but_bad = file(|l| {
        set("overridable", "false")(l);
        set("version", "0")(l);
    });
    let problem = read(&team(), &sealed_but_bad).unwrap_err();
    assert_eq!(
        problem.key.as_ref().map(ProcessKey::as_str),
        Some("review-and-merge")
    );
    assert_eq!(problem.sealed, Some(true));
    let unreadable = read(&team(), "---\nprocess: [\n---\n").unwrap_err();
    assert_eq!((unreadable.key, unreadable.sealed), (None, None));
    // Multi-byte text is limited by characters, not bytes; a null `required` is false.
    let ok = file(set("summary", &format!("\"{}\"", "é".repeat(200))));
    assert!(read(&team(), &ok).is_ok());
    let step = read(
        &team(),
        &file(set("checklist", "[{text: a, required: null}]")),
    )
    .unwrap();
    assert!(!step.checklist[0].required);
    let moments = read(&team(), &file(set("moments", "{waking_up: null}"))).unwrap();
    assert_eq!(moments.moments.waking_up, None);
    // A turned-off process says why (processes-ux.md decision 4).
    let off = file(|l| {
        set("status", "off")(l);
        l.push("reason: we report daily".into());
    });
    let off = read(&team(), &off).unwrap();
    assert_eq!(
        (off.status, off.reason.as_deref()),
        (Status::Off, Some("we report daily"))
    );
    // A draft or retired file never seals, even when it is invalid (§3.2 rule 2).
    for (status, sealed) in [("draft", false), ("retired", false), ("active", true)] {
        let bad = file(|l| {
            set("status", status)(l);
            set("overridable", "false")(l);
            set("name", "\" x\"")(l);
        });
        assert_eq!(
            read(&team(), &bad).unwrap_err().sealed,
            Some(sealed),
            "{status}"
        );
    }
}

#[test]
fn hidden_text_is_refused_but_joiners_and_direction_marks_are_kept() {
    let summary = |text: &str| file(set("summary", &format!("\"{text}\"")));
    for hidden in [
        "a\\u200bb",
        "a\\u202eb",
        "a\\u2066b",
        "a\\ufeffb",
        "a\\u2064b",
        "ok\\U000E0049",
        "x\\u3164y",
    ] {
        let problem = read(&team(), &summary(hidden)).unwrap_err();
        assert_eq!(
            (problem.field.as_str(), problem.kind),
            ("summary", ProblemKind::Invalid),
            "{hidden}"
        );
    }
    // A family emoji (ZWJ), a Persian word (ZWNJ), a flag (ZWJ + VS16) and direction marks.
    for kept in [
        "Family \u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}",
        "\u{0645}\u{06CC}\u{200C}\u{062E}",
        "\u{1F3F3}\u{FE0F}\u{200D}\u{1F308}",
        "a\u{200E}b\u{200F}c",
    ] {
        assert!(read(&team(), &summary(kept)).is_ok(), "{kept}");
    }
    // A draft never seals, even when `overridable` can't be read.
    let draft = file(|l| {
        set("status", "draft")(l);
        set("overridable", "\"maybe\"")(l);
    });
    assert_eq!(read(&team(), &draft).unwrap_err().sealed, Some(false));
}

#[test]
fn each_place_has_its_level_and_scope() {
    let member = MemberId::new(MEMBER).unwrap();
    let cases = [
        (Place::Organization, Level::Organization, None),
        (team(), Level::Team, Some("web")),
        (Place::Project(project()), Level::Project, Some("atlas")),
        (Place::Agent(member), Level::Agent, Some(MEMBER)),
    ];
    for (place, level, scope) in cases {
        assert_eq!((place.level(), place.scope()), (level, scope), "{place:?}");
    }
}

#[test]
fn file_names_a_doc_may_not_have_are_never_process_files() {
    for name in [
        "a b",
        "a\nb",
        "a\tb",
        "a\u{202e}b",
        "-x",
        "a\\b",
        "a\u{200b}b",
        "a\u{feff}",
        "a\u{7}b",
    ] {
        let path = format!("processes/{name}.md");
        assert_eq!(location(Repo::Handbook, &path), None, "{name:?}");
    }
    assert_eq!(
        location(Repo::Handbook, "processes/a-b_c.1.md"),
        Some(Place::Organization)
    );
}

#[test]
fn project_and_agent_files_read_only_at_their_own_level() {
    let member = MemberId::new(MEMBER).unwrap();
    let at = |level: &str, scope: &str| {
        let (level, scope) = (level.to_owned(), scope.to_owned());
        file(move |l| {
            set("level", &level)(l);
            set("scope", &scope)(l);
        })
    };
    let project_place = Place::Project(project());
    let agent_place = Place::Agent(member);
    assert_eq!(
        read(&project_place, &at("project", "atlas")).unwrap().place,
        project_place
    );
    assert_eq!(
        read(&agent_place, &at("agent", MEMBER)).unwrap().place,
        agent_place
    );
    for (place, text) in [
        (&agent_place, at("project", MEMBER)),
        (&project_place, at("agent", "atlas")),
    ] {
        let problem = read(place, &text).unwrap_err();
        assert_eq!(
            (problem.field.as_str(), problem.kind),
            ("level", ProblemKind::WrongPlace)
        );
    }
}

#[test]
fn empty_text_missing_applies_to_and_multi_line_moments_are_refused() {
    let cases = [
        (
            file(set("summary", "\"\"")),
            "summary",
            ProblemKind::Invalid,
        ),
        (file(set("name", "\"\"")), "name", ProblemKind::Invalid),
        (
            file(set("checklist", "[{text: \"\"}]")),
            "checklist[0].text",
            ProblemKind::Invalid,
        ),
        (
            file(drop_key("applies_to")),
            "applies_to",
            ProblemKind::Missing,
        ),
        (
            file(set("moments", "{waking_up: \"a\\nb\"}")),
            "moments.waking_up",
            ProblemKind::Invalid,
        ),
    ];
    for (text, field, kind) in cases {
        let problem = read(&team(), &text).unwrap_err();
        assert_eq!(
            (problem.field.as_str(), problem.kind),
            (field, kind),
            "{field}"
        );
    }
}

#[test]
fn every_character_org_office_refuses_in_status_lines_is_refused_here() {
    // The same cases as org-office's status-line test (organization/status.rs).
    for bad in [
        "two\nlines",
        "tab\there",
        "bell\u{7}",
        "nul\0",
        "esc\u{1b}[0m",
        "line\u{2028}break",
        "para\u{2029}graph",
        "hidden\u{200b}gap",
        "flip\u{202e}ed",
        "bom\u{feff}",
        "joiner\u{2060}",
        "invisible\u{2064}times",
        "isolate\u{2066}x",
        "isolate\u{2069}",
        "deprecated\u{206a}",
        "deprecated\u{206f}",
        "embed\u{202a}",
        "tag\u{e0041}",
        "tag\u{e007f}",
        "selector\u{e0100}",
        "selector\u{e01ef}",
        "mongolian\u{180e}",
        "annotation\u{fff9}",
        "annotation\u{fffb}",
        "filler\u{115f}",
        "filler\u{1160}",
        "filler\u{3164}",
        "filler\u{ffa0}",
    ] {
        let quoted = serde_json::to_string(bad).unwrap();
        let problem = read(&team(), &file(set("summary", &quoted))).unwrap_err();
        assert_eq!(
            (problem.field.as_str(), problem.kind),
            ("summary", ProblemKind::Invalid),
            "{bad:?}"
        );
    }
}
