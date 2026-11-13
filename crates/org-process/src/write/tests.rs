//! §5's write checks (processes.md Appendix C "Approval"), table-driven: each case fails without
//! its guard, and pairs of problems pin the order.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use super::*;
use crate::place::ProjectKey;

/// The writing agent, another agent, the owner and a human member.
const ME: &str = "01JD8Z3F0000000000000000CD";
const PEER: &str = "01JD8Z3F0000000000000000EF";
const BOSS: &str = "01JD8Z3F0000000000000000AA";
const HUMAN: &str = "01JD8Z3F0000000000000000BB";

const ORG: &str = "processes/review.md";
const WEB: &str = "teams/web/processes/review.md";
const MOBILE: &str = "teams/mobile/processes/review.md";
const ATLAS: &str = "docs/processes/review.md";

fn id(id: &str) -> MemberId {
    MemberId::new(id).unwrap()
}

fn agent() -> Writer {
    Writer::Agent(id(ME))
}

fn peer() -> Writer {
    Writer::Agent(id(PEER))
}

fn owner() -> Writer {
    Writer::Owner(id(BOSS))
}

fn human() -> Writer {
    Writer::Member(id(HUMAN))
}

/// A private file's path in `member`'s folder.
fn private(member: &str, name: &str) -> String {
    format!("agents/{member}/processes/{name}.md")
}

fn atlas() -> &'static ProjectKey {
    static KEY: OnceLock<ProjectKey> = OnceLock::new();
    KEY.get_or_init(|| ProjectKey::new("atlas").unwrap())
}

fn repo(path: &str) -> Repo<'static> {
    match path.starts_with("docs/") {
        true => Repo::Project(atlas()),
        false => Repo::Handbook,
    }
}

/// A file with `key` written for `path`'s place (the organization's for a path that has none).
/// `flags`: `sealed`, `off`, `draft`, `retired`, `over` (`overrides: <key>@1`), `bad` (an
/// invalid `name`, so its key can still be read), `garbage` (no front matter) and `web` (level
/// and scope of team `web`).
fn text(path: &str, key: &str, version: u32, flags: &str) -> String {
    let place = location(repo(path), path).map_or(Place::Organization, |at| at.place().clone());
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
    let mut set = |field: &'static str, value: &str| {
        lines.retain(|(f, _)| *f != field);
        lines.push((field, value.into()));
    };
    for flag in flags.split_whitespace() {
        match flag {
            "sealed" => set("overridable", "false"),
            "off" => {
                set("status", "off");
                set("reason", "we report daily");
            }
            "draft" | "retired" => set("status", flag),
            "over" => set("overrides", &format!("{key}@1")),
            "bad" => set("name", "[a list]"),
            "garbage" => return "no front matter".into(),
            "web" => {
                set("level", "team");
                set("scope", "web");
            }
            _ => panic!("unknown flag {flag}"),
        }
    }
    let front: Vec<String> = lines.iter().map(|(f, v)| format!("{f}: {v}")).collect();
    format!("---\n{}\n---\n# {key}\n", front.join("\n"))
}

/// A file at the base, version 1, read at `path`'s place.
fn base(path: &str, key: &str, flags: &str) -> Input {
    based(path, key, 1, flags)
}

/// A file at the base, version `version`, read at `path`'s place.
fn based(path: &str, key: &str, version: u32, flags: &str) -> Input {
    let at = location(repo(path), path).expect("a base file's place");
    Input {
        place: at.place().clone(),
        path: path.into(),
        read: at.read(&text(path, key, version, flags)),
    }
}

/// A branch for `path`, in its repository. `flags`: `foreign` (not office-made), `merged`,
/// `zeta` (in project zeta's repository) and `handbook` (in the handbook).
fn branch(path: &str, by: &str, flags: &str) -> Branch {
    let project = match repo(path) {
        _ if flags.contains("zeta") => ProjectKey::new("zeta"),
        _ if flags.contains("handbook") => None,
        Repo::Project(key) => Some(key.clone()),
        Repo::Handbook => None,
    };
    Branch {
        office_made: !flags.contains("foreign"),
        merged: flags.contains("merged"),
        project,
        path: path.into(),
        opened_by: id(by),
    }
}

/// A write, built up for one case.
struct Write {
    caller: Writer,
    path: String,
    text: String,
    files: Vec<Input>,
    base_version: Option<u32>,
    branch: Option<Branch>,
}

/// `caller` writes `key` at `path`, version `version` with `flags` (see [`text`]).
fn write(caller: Writer, path: &str, key: &str, version: u32, flags: &str) -> Write {
    Write {
        caller,
        path: path.into(),
        text: text(path, key, version, flags),
        files: Vec::new(),
        base_version: None,
        branch: None,
    }
}

impl Write {
    fn with(mut self, file: Input) -> Self {
        self.files.push(file);
        self
    }

    fn after(mut self, version: u32) -> Self {
        self.base_version = Some(version);
        self
    }

    fn on(mut self, branch: Branch) -> Self {
        self.branch = Some(branch);
        self
    }

    /// The answer in words: `ok`, or the code, `/reason` and ` (field)`.
    fn answer(self) -> String {
        let input = WriteInput {
            repo: repo(&self.path),
            path: self.path,
            text: self.text,
            caller: self.caller,
            base: Inputs {
                agent: id(ME),
                work: None,
                files: self.files,
                names: BTreeMap::new(),
            },
            base_version: self.base_version,
            branch: self.branch,
        };
        match check_write(&input) {
            Ok(_) => "ok".into(),
            Err(refusal) => {
                assert!(!refusal.message().is_empty());
                let reason = refusal.reason().map(|r| format!("/{}", r.as_str()));
                let field = refusal.field().map(|f| format!(" ({f})"));
                let (reason, field) = (reason.unwrap_or_default(), field.unwrap_or_default());
                format!("{}{reason}{field}", refusal.code().as_str())
            }
        }
    }
}

fn check(cases: Vec<(&str, Write, &str)>) {
    let failed: Vec<String> = cases
        .into_iter()
        .filter_map(|(name, write, want)| {
            let got = write.answer();
            (got != want).then(|| format!("{name}: got {got}, want {want}"))
        })
        .collect();
    assert!(failed.is_empty(), "{failed:#?}");
}

const POA: &str = "invalid_request/private_only_adds";

#[test]
fn check_1_location() {
    check(vec![
        (
            "an organization file",
            write(owner(), ORG, "review", 1, ""),
            "ok",
        ),
        (
            "no process folder",
            write(owner(), "notes/review.md", "review", 1, ""),
            "invalid_request",
        ),
        (
            "nested",
            write(owner(), "processes/a/review.md", "review", 1, ""),
            "invalid_request",
        ),
        (
            "not markdown",
            write(owner(), "processes/review.txt", "review", 1, ""),
            "invalid_request",
        ),
        (
            "a refused project file name, before its front matter",
            write(agent(), "docs/processes/-review.md", "review", 1, "garbage"),
            "invalid_request (file name)",
        ),
    ]);
}

#[test]
fn check_2_front_matter() {
    check(vec![
        (
            "no front matter",
            write(owner(), ORG, "review", 1, "garbage"),
            "invalid_request",
        ),
        (
            "an invalid field",
            write(owner(), ORG, "review", 1, "bad"),
            "invalid_request (name)",
        ),
        (
            "another place's level",
            write(owner(), ORG, "review", 1, "web"),
            "invalid_request (level)",
        ),
        (
            "another team's scope",
            write(owner(), MOBILE, "review", 1, "web"),
            "invalid_request (scope)",
        ),
    ]);
}

#[test]
fn check_3_own_private_folder() {
    check(vec![
        (
            "the agent's own",
            write(agent(), &private(ME, "mine"), "mine", 1, ""),
            "ok",
        ),
        (
            "another agent's",
            write(peer(), &private(ME, "mine"), "mine", 1, ""),
            "not_authorized",
        ),
        (
            "the owner, in an agent's",
            write(owner(), &private(ME, "mine"), "mine", 1, ""),
            "not_authorized",
        ),
        (
            "the owner, in its own id's",
            write(owner(), &private(BOSS, "mine"), "mine", 1, ""),
            "not_authorized",
        ),
        (
            "a human, in its own id's",
            write(human(), &private(HUMAN, "mine"), "mine", 1, ""),
            "not_authorized",
        ),
        (
            "an agent elsewhere",
            write(peer(), ATLAS, "review", 1, ""),
            "ok",
        ),
    ]);
}

#[test]
fn check_4_organization_seal() {
    let resave = write(agent(), ORG, "review", 2, "sealed").with(base(ORG, "review", "sealed"));
    // `caller` saves version 2, overridable, over the base's version 1 with `flags`.
    let edit = |caller: Writer, flags: &str| {
        write(caller, ORG, "review", 2, "")
            .with(base(ORG, "review", flags))
            .after(1)
    };
    check(vec![
        (
            "the owner seals",
            write(owner(), ORG, "review", 1, "sealed"),
            "ok",
        ),
        (
            "an agent seals",
            write(agent(), ORG, "review", 1, "sealed"),
            "not_authorized",
        ),
        (
            "a human seals",
            write(human(), ORG, "review", 1, "sealed"),
            "not_authorized",
        ),
        (
            "ruling (a): sealed stays sealed",
            resave.after(1),
            "not_authorized",
        ),
        (
            "a sealed draft",
            write(agent(), ORG, "review", 1, "sealed draft"),
            "not_authorized",
        ),
        (
            "an agent, overridable",
            write(agent(), ORG, "review", 1, ""),
            "ok",
        ),
        (
            "an agent seals a team's",
            write(agent(), WEB, "review", 1, "sealed"),
            "ok",
        ),
        (
            "an agent unseals",
            edit(agent(), "sealed"),
            "not_authorized",
        ),
        ("a human unseals", edit(human(), "sealed"), "not_authorized"),
        (
            "one that can't be read",
            edit(agent(), "garbage"),
            "not_authorized",
        ),
        (
            "an invalid sealed one",
            edit(agent(), "sealed bad"),
            "not_authorized",
        ),
        (
            "a sealed draft at the base",
            edit(agent(), "sealed draft"),
            "ok",
        ),
        (
            "a sealed retired one",
            edit(agent(), "sealed retired"),
            "ok",
        ),
        ("an overridable one", edit(agent(), ""), "ok"),
        ("the owner unseals", edit(owner(), "sealed"), "ok"),
        (
            "another file's seal",
            write(agent(), ORG, "review", 1, "").with(base("processes/b.md", "other", "sealed")),
            "ok",
        ),
    ]);
}

#[test]
fn check_5_private_only_adds() {
    let mine = private(ME, "review");
    let add = || write(agent(), &mine, "review", 1, "");
    check(vec![
        ("a new key", add(), "ok"),
        ("overrides", write(agent(), &mine, "review", 1, "over"), POA),
        ("off", write(agent(), &mine, "review", 1, "off"), POA),
        (
            "an organization key",
            add().with(base(ORG, "review", "")),
            POA,
        ),
        (
            "a draft's key",
            add().with(base(ORG, "review", "draft")),
            POA,
        ),
        (
            "a retired file's key",
            add().with(base(ORG, "review", "retired")),
            POA,
        ),
        (
            "an invalid file's key",
            add().with(base(ORG, "review", "bad")),
            POA,
        ),
        (
            "can't tell",
            add().with(base(ORG, "review", "garbage")),
            POA,
        ),
        (
            "an invalid file's other key",
            add().with(base(ORG, "other", "bad")),
            "ok",
        ),
        (
            "another organization key",
            add().with(base("processes/a.md", "other", "")),
            "ok",
        ),
        ("a team's key", add().with(base(WEB, "review", "")), "ok"),
        (
            "a project file overrides",
            write(agent(), ATLAS, "review", 1, "over"),
            "ok",
        ),
    ]);
}

#[test]
fn check_6_unique_key() {
    let twin = |path: &str, flags: &str| base(path, "review", flags);
    let new = |path: &str| write(owner(), path, "review", 1, "");
    let mine = || write(agent(), &private(ME, "review"), "review", 1, "");
    check(vec![
        (
            "another file",
            new(ORG).with(twin("processes/b.md", "")),
            "already_exists",
        ),
        (
            "an invalid one",
            new(ORG).with(twin("processes/b.md", "bad")),
            "already_exists",
        ),
        (
            "a draft",
            new(ORG).with(twin("processes/b.md", "draft")),
            "already_exists",
        ),
        (
            "one that can't be read",
            new(ORG).with(twin("processes/b.md", "garbage")),
            "ok",
        ),
        (
            "a save, version 2",
            write(owner(), ORG, "review", 2, "")
                .with(twin(ORG, ""))
                .after(1),
            "ok",
        ),
        ("another team", new(WEB).with(twin(MOBILE, "")), "ok"),
        ("another level", new(ATLAS).with(twin(WEB, "")), "ok"),
        (
            "a private twin",
            mine().with(twin(&private(ME, "b"), "")),
            "already_exists",
        ),
    ]);
}

#[test]
fn check_8_version() {
    let at = |version: u32| write(owner(), ORG, "review", version, "");
    check(vec![
        ("new, 1", at(1), "ok"),
        ("new, 2", at(2), "invalid_request (version)"),
        ("base + 1", at(4).after(3), "ok"),
        ("base", at(3).after(3), "invalid_request (version)"),
        ("base + 2", at(5).after(3), "invalid_request (version)"),
        (
            "the base file agrees",
            at(4).after(3).with(based(ORG, "review", 3, "")),
            "ok",
        ),
        (
            "new, but the base has the file",
            at(1).with(based(ORG, "review", 3, "")),
            "invalid_request (version)",
        ),
        (
            "the base file disagrees",
            at(3).after(2).with(based(ORG, "review", 3, "")),
            "invalid_request (version)",
        ),
        (
            "a base file whose version can't be read",
            at(1).with(based(ORG, "review", 3, "bad")),
            "ok",
        ),
        (
            "another path's version",
            at(1).with(based("processes/b.md", "other", 3, "")),
            "ok",
        ),
        (
            "no version follows",
            at(u32::MAX).after(u32::MAX),
            "invalid_request (version)",
        ),
    ]);
}

#[test]
fn check_9_branch() {
    let on = |flags: &str, path: &str, by: &str| {
        write(agent(), ATLAS, "review", 1, "").on(branch(path, by, flags))
    };
    check(vec![
        ("its own open branch", on("", ATLAS, ME), "ok"),
        (
            "not office-made",
            on("foreign", ATLAS, ME),
            "invalid_request",
        ),
        (
            "ruling (c): merged",
            on("merged", ATLAS, ME),
            "invalid_request",
        ),
        (
            "for another file",
            on("", "docs/processes/b.md", ME),
            "invalid_request",
        ),
        ("another member's", on("", ATLAS, PEER), "not_authorized"),
        (
            "another project's",
            on("zeta", ATLAS, ME),
            "invalid_request",
        ),
        (
            "the handbook's",
            on("handbook", ATLAS, ME),
            "invalid_request",
        ),
        (
            "a project's, for a handbook file",
            write(owner(), ORG, "review", 1, "").on(branch(ORG, BOSS, "zeta")),
            "invalid_request",
        ),
        (
            "the handbook's, for a handbook file",
            write(owner(), ORG, "review", 1, "").on(branch(ORG, BOSS, "")),
            "ok",
        ),
        (
            "ruling (c): merged and another's",
            on("merged", ATLAS, PEER),
            "invalid_request",
        ),
    ]);
}

/// Two problems at once: the earlier check's answer.
#[test]
fn the_first_problem_is_reported() {
    let peers = private(PEER, "review");
    let mine = private(ME, "review");
    check(vec![
        (
            "2 before 3",
            write(agent(), &peers, "review", 1, "web"),
            "invalid_request (level)",
        ),
        (
            "3 before 5",
            write(agent(), &peers, "review", 1, "over"),
            "not_authorized",
        ),
        (
            "4 before 6",
            write(agent(), ORG, "review", 1, "sealed").with(base("processes/b.md", "review", "")),
            "not_authorized",
        ),
        (
            "5 before 6",
            write(agent(), &mine, "review", 1, "over").with(base(&private(ME, "b"), "review", "")),
            POA,
        ),
        (
            "6 before 8",
            write(owner(), ORG, "review", 2, "").with(base("processes/b.md", "review", "")),
            "already_exists",
        ),
        (
            "8 before 9",
            write(agent(), ATLAS, "review", 2, "").on(branch(ATLAS, PEER, "")),
            "invalid_request (version)",
        ),
    ]);
}
