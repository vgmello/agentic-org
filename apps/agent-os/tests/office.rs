//! `agent-os` against an office on this Mac that is not running: a send to it is refused
//! (exit 5) without spooling, an office named by mistake is told at once, an office not named
//! is not used, and an office-run agent never falls back to the spool.
//!
//! What needs a running office is gone until agents can reach one: the office store binds no
//! agent before B2 places agents through HR, and `agent-os` addresses only plain office IDs
//! until A5 teaches it a selected (UUIDv7) office and its socket. `who` through the office and
//! the messaging cases return then (docs/implementation-plan.md, B2, B6 and A5).
//!
//! The office is its `office.json`, as the host's provisioning leaves it; nothing serves its
//! socket. Every home is a temporary folder: the office home (`ORG_OFFICE_HOME`) and the
//! agents' `HOME` and `ORG_HOME` (their spool), so nothing here can reach `~/.agency` or a
//! real office.
//!
//! Its own `main` (`harness = false`), as before.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;
use tempfile::TempDir;

const OFFICE: &str = "01OFFICE00000000000000HQ1";
const ORG: &str = "01ORG0000000000000000ACME1";

fn main() {
    let tests: &[(&str, fn())] = &[
        (
            "office_errors_keep_their_exit_codes_and_never_show_a_body",
            office_errors_keep_their_exit_codes_and_never_show_a_body,
        ),
        (
            "a_send_to_an_office_that_is_down_is_refused_until_ingest",
            a_send_to_an_office_that_is_down_is_refused_until_ingest,
        ),
        (
            "an_office_not_named_is_not_used",
            an_office_not_named_is_not_used,
        ),
        (
            "with_no_office_on_this_mac_the_spool_is_used_quietly",
            with_no_office_on_this_mac_the_spool_is_used_quietly,
        ),
        (
            "an_office_run_agent_never_falls_back_to_the_spool",
            an_office_run_agent_never_falls_back_to_the_spool,
        ),
    ];
    let args = TestArgs::parse(std::env::args().skip(1));
    if args.list {
        for (name, _) in tests {
            if args.selects(name) {
                println!("{name}: test");
            }
        }
        return;
    }
    let mut passed = 0;
    let mut failed = Vec::new();
    for (name, test) in tests {
        if !args.selects(name) {
            continue;
        }
        println!("test {name} ...");
        match catch_unwind(AssertUnwindSafe(test)) {
            Ok(()) => {
                println!("test {name} ... ok");
                passed += 1;
            }
            Err(_) => {
                println!("test {name} ... FAILED");
                failed.push(*name);
            }
        }
    }
    let verdict = if failed.is_empty() { "ok" } else { "FAILED" };
    println!(
        "\ntest result: {verdict}. {passed} passed; {} failed",
        failed.len()
    );
    if !failed.is_empty() {
        println!("failures: {}", failed.join(", "));
        std::process::exit(101);
    }
}

/// The libtest arguments this `main` honours: name filters, `--skip <name>`, `--exact` and
/// `--list`. Other flags are accepted and ignored, with the value of those that take one
/// (`--test-threads 2` is not a filter). `--ignored` runs nothing: no test here is ignored.
#[derive(Default)]
struct TestArgs {
    filters: Vec<String>,
    skips: Vec<String>,
    exact: bool,
    list: bool,
    ignored_only: bool,
}

impl TestArgs {
    fn parse(args: impl Iterator<Item = String>) -> Self {
        const WITH_VALUE: &[&str] = &[
            "--test-threads",
            "--format",
            "--color",
            "--logfile",
            "--shuffle-seed",
            "-Z",
        ];
        let mut parsed = TestArgs::default();
        let mut args = args;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--skip" => parsed.skips.extend(args.next()),
                skip if skip.starts_with("--skip=") => {
                    parsed.skips.push(skip["--skip=".len()..].to_owned());
                }
                "--exact" => parsed.exact = true,
                "--list" => parsed.list = true,
                "--ignored" => parsed.ignored_only = true,
                flag if WITH_VALUE.contains(&flag) => {
                    args.next();
                }
                flag if flag.starts_with('-') => {}
                filter => parsed.filters.push(filter.to_owned()),
            }
        }
        parsed
    }

    fn selects(&self, name: &str) -> bool {
        let matches = |pattern: &String| {
            if self.exact {
                name == pattern
            } else {
                name.contains(pattern.as_str())
            }
        };
        !self.ignored_only
            && (self.filters.is_empty() || self.filters.iter().any(matches))
            && !self.skips.iter().any(matches)
    }
}

/// A temporary office home and agents' home, under /tmp (socket paths are short).
struct World {
    _dir: TempDir,
    home: PathBuf,
    user: PathBuf,
}

impl World {
    fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("oa")
            .tempdir_in("/tmp")
            .unwrap();
        let home = dir.path().join("h");
        let user = dir.path().join("u");
        for folder in [home.join("offices"), home.join("run"), user.clone()] {
            std::fs::create_dir_all(folder).unwrap();
        }
        Self {
            _dir: dir,
            home,
            user,
        }
    }

    /// The office on this Mac, as the host's provisioning leaves it (its `office.json`),
    /// with nothing serving its socket.
    fn stored_office(&self) {
        let folder = self.home.join("offices").join(OFFICE);
        std::fs::create_dir_all(&folder).unwrap();
        let office = org_office_launch::OfficeFile::new(
            OFFICE,
            "HQ",
            org_proto::OfficeRole::Hq,
            ORG,
            "Acme",
            "2026-11-09T10:00:00.000Z",
        );
        std::fs::write(folder.join("office.json"), office.to_vec().unwrap()).unwrap();
    }

    /// `agent-os <args>` as `agent`, with every home in this world and the office named
    /// (`ORG_OFFICE`: during the cutover the office is used only when named).
    fn agent(&self, agent: &str, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_agent-os"));
        command
            .args(args)
            .env("ORG_OFFICE_HOME", &self.home)
            .env("HOME", &self.user)
            .env("ORG_HOME", &self.user)
            .env("ORG_AGENT", agent)
            .env("ORG_OFFICE", OFFICE)
            .env_remove("ORG_SESSION_TOKEN")
            .stdin(Stdio::null());
        command
    }

    fn run(&self, agent: &str, args: &[&str]) -> Run {
        self.agent(agent, args).output().unwrap().into()
    }

    /// `agent-os <args>` as `agent` with no office named.
    fn run_unnamed(&self, agent: &str, args: &[&str]) -> Run {
        self.agent(agent, args)
            .env_remove("ORG_OFFICE")
            .output()
            .unwrap()
            .into()
    }

    /// The sends waiting in the office's own spool (ADR-0003 §3.5).
    fn office_spooled(&self) -> Vec<PathBuf> {
        match std::fs::read_dir(self.home.join("offices").join(OFFICE).join("spool/new")) {
            Ok(entries) => entries.map(|e| e.unwrap().path()).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// The files waiting in the local spool's outbox.
    fn spooled(&self) -> Vec<PathBuf> {
        match std::fs::read_dir(self.user.join("spool/outbox/new")) {
            Ok(entries) => entries.map(|e| e.unwrap().path()).collect(),
            Err(_) => Vec::new(),
        }
    }
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

impl From<Output> for Run {
    fn from(out: Output) -> Self {
        Self {
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).trim().to_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        }
    }
}

impl Run {
    fn ok(self) -> Self {
        assert_eq!(
            self.code, 0,
            "stdout: {}\nstderr: {}",
            self.stdout, self.stderr
        );
        self
    }

    fn json(&self) -> Value {
        serde_json::from_str(&self.stdout)
            .unwrap_or_else(|e| panic!("not JSON ({e}): {}", self.stdout))
    }
}

/// (An agent the office doesn't know being refused at its bind needs a running office that
/// `agent-os` can address: it returns with A5.)
fn office_errors_keep_their_exit_codes_and_never_show_a_body() {
    let world = World::new();
    world.stored_office();
    let secret = "a secret body";

    // An office asked for that isn't a plain id is a usage error, before anything is sent.
    let bad = world.run(
        "pixel",
        &["msg", "send", "--office", "../x", "--to", "nova", secret],
    );
    assert_eq!(bad.code, 1);
    assert!(world.spooled().is_empty());

    // An office id with no office.json here (a typo) is told at once (exit 1), before any
    // connection: it isn't "unreachable", and nothing is sent or spooled.
    for args in [
        &[
            "msg",
            "send",
            "--office",
            "01OFFICE00000000000000XX9",
            "--to",
            "nova",
            secret,
        ][..],
        &["msg", "inbox", "--office", "01OFFICE00000000000000XX9"][..],
    ] {
        let typo = world.run("pixel", args);
        assert_eq!(typo.code, 1, "{args:?}: {}", typo.stderr);
        assert!(
            typo.stderr
                .contains("office 01OFFICE00000000000000XX9 isn't on this Mac"),
            "{}",
            typo.stderr
        );
        assert!(!typo.stderr.contains("not reachable"), "{}", typo.stderr);
        assert!(!typo.stderr.contains(secret));
    }
    assert!(world.spooled().is_empty());
    assert!(world.office_spooled().is_empty());
}

fn a_send_to_an_office_that_is_down_is_refused_until_ingest() {
    let world = World::new();
    world.stored_office();

    // Until the office ingests its spool, a send to it while it's down is refused (exit 5):
    // nothing is written anywhere, and the body isn't shown. (The queued path is unit-tested
    // with the switch on.)
    let secret = "kept for later";
    let started = Instant::now();
    let sent = world.run("pixel", &["msg", "send", "--to", "nova", "--json", secret]);
    assert_eq!(sent.code, 5, "{}", sent.stderr);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(sent.stdout, "");
    assert!(
        sent.stderr
            .contains(&format!("office {OFFICE} is not reachable")),
        "{}",
        sent.stderr
    );
    assert!(
        sent.stderr
            .contains("can't take queued sends yet; nothing was sent"),
        "{}",
        sent.stderr
    );
    assert!(!sent.stderr.contains(secret));
    assert!(world.office_spooled().is_empty());
    assert!(world.spooled().is_empty(), "it went to the local spool");

    // The read side says the office isn't reachable and shows the local spool, as before.
    let inbox = world.run("nova", &["msg", "inbox", "--json"]).ok();
    assert!(
        inbox.stderr.contains("showing the local spool"),
        "{}",
        inbox.stderr
    );
    assert_eq!(inbox.stdout, "[]");
    let read = world.run("nova", &["msg", "read", "--next", "--json"]).ok();
    assert!(read.stderr.contains("not reachable"), "{}", read.stderr);
    assert_eq!(read.stdout, "null");
}

fn an_office_not_named_is_not_used() {
    let world = World::new();
    world.stored_office();
    // On this Mac, with its office.json, but not named: the local spool, quietly, as before.
    world.run_unnamed("nova", &["msg", "register", "nova"]).ok();
    let sent = world
        .run_unnamed("pixel", &["msg", "send", "--to", "nova", "--json", "hi"])
        .ok();
    assert_eq!(sent.stderr, "");
    assert!(sent.json().get("queued").is_none());
    assert_eq!(world.spooled().len(), 1);
    let inbox = world.run_unnamed("nova", &["msg", "inbox", "--json"]).ok();
    assert_eq!(inbox.stderr, "");
    // A hand-run agent the office doesn't know isn't refused when it names no office.
    let stranger = world.run_unnamed("ghost", &["msg", "inbox"]).ok();
    assert_eq!(stranger.stderr, "");
}

fn with_no_office_on_this_mac_the_spool_is_used_quietly() {
    let world = World::new();
    world.run_unnamed("nova", &["msg", "register", "nova"]).ok();
    let sent = world
        .run_unnamed("pixel", &["msg", "send", "--to", "nova", "--json", "hi"])
        .ok();
    assert_eq!(sent.stderr, "");
    assert!(sent.json().get("queued").is_none());
    assert_eq!(world.spooled().len(), 1);
    // Without a session token, who with no identity reads the spool as before.
    let who: Run = world
        .agent("pixel", &["who", "--json"])
        .env_remove("ORG_AGENT")
        .env_remove("ORG_OFFICE")
        .output()
        .unwrap()
        .into();
    let who = who.ok();
    assert_eq!(who.stderr, "");
    assert!(who.json().is_array(), "{}", who.stdout);
    assert_eq!(world.run_unnamed("nova", &["msg", "inbox"]).ok().stderr, "");
    // Named but not on this Mac: told so (exit 1), and no office folder is made.
    let named = world.run("pixel", &["msg", "send", "--to", "nova", "hi"]);
    assert_eq!(named.code, 1, "{}", named.stderr);
    assert!(
        named.stderr.contains("isn't on this Mac"),
        "{}",
        named.stderr
    );
    assert!(!world.home.join("offices").join(OFFICE).exists());
}

fn an_office_run_agent_never_falls_back_to_the_spool() {
    let world = World::new();
    let spool = world.user.join("spool");

    // who with no identity can't ask the office, and an office-run agent never uses the spool.
    let who: Run = world
        .agent("nova", &["who"])
        .env_remove("ORG_AGENT")
        .env("ORG_SESSION_TOKEN", "t")
        .output()
        .unwrap()
        .into();
    assert_eq!(who.code, 5, "{}", who.stderr);
    assert!(
        who.stderr.contains("without a valid identity") && who.stderr.contains("identity not set"),
        "{}",
        who.stderr
    );
    // An invalid identity: the same refusal, naming that error.
    let invalid: Run = world
        .agent("a b", &["who"])
        .env("ORG_SESSION_TOKEN", "t")
        .output()
        .unwrap()
        .into();
    assert_eq!(invalid.code, 5, "{}", invalid.stderr);
    assert!(
        invalid.stderr.contains("may only contain"),
        "{}",
        invalid.stderr
    );
    assert!(!spool.exists(), "the spool was opened");

    // Its office named but not running: exit 5, nothing spooled.
    world.stored_office();
    let sent: Run = world
        .agent("pixel", &["msg", "send", "--to", "nova", "hi"])
        .env("ORG_OFFICE", OFFICE)
        .env("ORG_SESSION_TOKEN", "t")
        .output()
        .unwrap()
        .into();
    assert_eq!(sent.code, 5, "{}", sent.stderr);
    assert!(world.spooled().is_empty());
    assert!(!spool.exists(), "the spool was opened");

    // An office asked for with no absolute office home is a usage error, not the spool.
    let homeless: Run = world
        .agent("nova", &["msg", "inbox"])
        .env_remove("ORG_OFFICE_HOME")
        .env("HOME", "relative")
        .env("ORG_OFFICE", OFFICE)
        .output()
        .unwrap()
        .into();
    assert_eq!(homeless.code, 1, "{}", homeless.stderr);
    assert!(
        homeless.stderr.contains("absolute path"),
        "{}",
        homeless.stderr
    );
    assert!(!spool.exists(), "the spool was opened");

    // An office-run agent with no office named: exit 5, no spool.
    let lost: Run = world
        .agent("nova", &["msg", "inbox"])
        .env_remove("ORG_OFFICE_HOME")
        .env("HOME", "relative")
        .env_remove("ORG_OFFICE")
        .env("ORG_SESSION_TOKEN", "t")
        .output()
        .unwrap()
        .into();
    assert_eq!(lost.code, 5, "{}", lost.stderr);
    assert!(lost.stderr.contains("set ORG_OFFICE"), "{}", lost.stderr);
    assert!(!spool.exists(), "the spool was opened");
}
