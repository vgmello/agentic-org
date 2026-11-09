//! `org-agent` against a real office server: `msg send | inbox | read | wait` and `who` go
//! through the office, and `send` falls back to the local spool when the office is down.
//!
//! The server is the real one: this test binary, started with `ORG_AGENT_TEST_OFFICE_SERVER`
//! set, runs exactly what `org-office-server`'s `main` runs (its library). It is driven the way
//! the host drives it (`apps/org-office-server/tests/launch.rs`): the office's database key and the
//! app token are a fixed test key and token handed over a one-shot key socket (the key hand-off),
//! so no Keychain is involved. Every home is a temporary
//! folder: the office home (`ORG_OFFICE_HOME`) and the agents' `HOME` and `ORG_HOME` (their
//! spool), so nothing here can reach `~/.agency` or a real office.
//!
//! Its own `main` (`harness = false`), because the server writes control lines on stdout.

use std::io::BufReader;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Output, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use org_client::Client;
use org_office_launch::{
    key_socket_path, read_server_event, write_hand_off, write_host_message, CreateOffice,
    HostMessage, KeySocketId, ServerEvent,
};
use org_proto::location::office_socket_path;
use org_proto::org::{
    AgentProfile, AgentRuntime, AgentStatus, MemberAddAgent, MemberAddAgentParams, MemberOffboard,
    MemberOffboardParams,
};
use org_proto::{AppToken, DbKey, OfficeRole};
use serde_json::Value;
use tempfile::TempDir;

/// Set on the child that is the office server.
const SERVER_ENV: &str = "ORG_AGENT_TEST_OFFICE_SERVER";
const OFFICE: &str = "01OFFICE00000000000000HQ1";
const ORG: &str = "01ORG0000000000000000ACME1";
const PIXEL: &str = "AGENT00000000000000000PIXL";
const NOVA: &str = "AGENT00000000000000000NOVA";
const WAIT: Duration = Duration::from_secs(30);

fn main() {
    if std::env::var_os(SERVER_ENV).is_some() {
        serve();
    }
    let tests: &[(&str, fn())] = &[
        (
            "send_inbox_read_and_who_go_through_the_office",
            send_inbox_read_and_who_go_through_the_office,
        ),
        (
            "office_errors_keep_their_exit_codes_and_never_show_a_body",
            office_errors_keep_their_exit_codes_and_never_show_a_body,
        ),
        ("wait_wakes_on_a_send", wait_wakes_on_a_send),
        (
            "wait_times_out_and_a_person_can_interrupt_it",
            wait_times_out_and_a_person_can_interrupt_it,
        ),
        (
            "an_agent_offboarded_during_a_wait_is_told_and_not_left_waiting",
            an_agent_offboarded_during_a_wait_is_told_and_not_left_waiting,
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

/// The office server, exactly as `org-office-server`'s `main` runs it.
fn serve() -> ! {
    // First of all, as `main` does.
    if let Err(refusal) = org_office_server::hygiene::run() {
        org_office_server::hygiene::refuse(refusal);
    }
    let control = match org_office_server::fdswap::take_control_pipe() {
        Ok(control) => control,
        Err(error) => {
            eprintln!("org-office-server: cannot set up its descriptors: {error}");
            std::process::exit(1);
        }
    };
    org_office_server::install_panic_hook();
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(org_office_server::run(&args, control));
}

/// Binds the key socket `id` (owner-only, as the host does), and serves its one connection: the
/// hand-off line with the test token and key, then the socket is unlinked and the connection
/// closed. The thread gives up after a while if nobody connects.
fn serve_key_socket(home: &std::path::Path, id: &KeySocketId) {
    let path = key_socket_path(home, id);
    let listener = UnixListener::bind(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    listener.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        let until = Instant::now() + Duration::from_secs(30);
        while Instant::now() < until {
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = stream.set_nonblocking(false);
                let _ = write_hand_off(&mut stream, &token(), Some(&DbKey::from_bytes([0xB3; 32])));
                // As the host does: stop writing, keep the connection until the server closes.
                let _ = stream.shutdown(std::net::Shutdown::Write);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let mut sink = [0u8; 64];
                while matches!(std::io::Read::read(&mut stream, &mut sink), Ok(n) if n > 0) {}
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let _ = std::fs::remove_file(&path);
    });
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

    fn socket(&self) -> PathBuf {
        office_socket_path(&self.home, OFFICE).unwrap()
    }

    /// Creates the office, hires pixel and nova into it, and returns the running server.
    fn office(&self) -> Server {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg("--create")
            .env(SERVER_ENV, "1")
            .env("ORG_OFFICE_HOME", &self.home)
            .env("HOME", &self.user)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // The server's log must be a pipe, as the host gives it (a fd-hygiene rule).
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut log = child.stderr.take().unwrap();
        std::thread::spawn(move || {
            let _ = std::io::copy(&mut log, &mut std::io::sink());
        });
        let (tx, events) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            while let Ok(Some(event)) = read_server_event(&mut reader) {
                if tx.send(event).is_err() {
                    break;
                }
            }
        });
        let mut server = Server {
            stdin: child.stdin.take(),
            child,
            events,
        };
        // The host's part of the key hand-off: a one-shot socket, bound before the first line,
        // that writes the app token and the key to the one connection.
        let key_socket_id = KeySocketId::generate_from([0xA5; 8]);
        serve_key_socket(&self.home, &key_socket_id);
        write_host_message(
            server.stdin.as_mut().unwrap(),
            &HostMessage::Create(CreateOffice {
                org_id: ORG.into(),
                org_name: "Acme".into(),
                office_id: OFFICE.into(),
                office_name: "HQ".into(),
                role: OfficeRole::Hq,
                key_socket_id,
            }),
        )
        .unwrap();
        assert!(matches!(server.event(), ServerEvent::Created { .. }));
        assert!(matches!(server.event(), ServerEvent::Ready { .. }));

        let app = Client::connect(self.socket()).unwrap();
        app.bind_app(token()).unwrap();
        for (n, id, handle) in [(1, PIXEL, "pixel"), (2, NOVA, "nova")] {
            app.call::<MemberAddAgent>(&MemberAddAgentParams {
                command_id: command_id(n),
                member_id: id.into(),
                name: handle.to_uppercase(),
                office_id: OFFICE.into(),
                profile: AgentProfile {
                    role: "engineer".into(),
                    title: "Engineer".into(),
                    focus: None,
                    runtime: AgentRuntime::ClaudeCode,
                    model: None,
                    run_mode: None,
                    schedule: None,
                    home: None,
                    messaging_name: handle.into(),
                    status: AgentStatus::Active,
                    offboarded_at: None,
                },
            })
            .unwrap();
        }
        server
    }

    /// `org-agent <args>` as `agent`, with every home in this world and the office named
    /// (`ORG_OFFICE`: during the cutover the office is used only when named).
    fn agent(&self, agent: &str, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_org-agent"));
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

    /// `org-agent <args>` as `agent` with no office named.
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

struct Server {
    child: Child,
    stdin: Option<ChildStdin>,
    events: Receiver<ServerEvent>,
}

impl Server {
    fn event(&self) -> ServerEvent {
        self.events.recv_timeout(WAIT).expect("no control line")
    }

    /// Closes stdin, as a host that goes away does, and waits for the server to exit.
    fn stop(&mut self) {
        self.stdin = None;
        let started = Instant::now();
        while self.child.try_wait().unwrap().is_none() {
            assert!(started.elapsed() < WAIT, "the server did not exit");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
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

fn token() -> AppToken {
    AppToken::from_bytes([0xA7; 32])
}

/// A ULID made now (the command ids the office accepts), distinct for each `n`.
fn command_id(n: u8) -> String {
    const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let mut value = (ms << 80) | u128::from(n);
    let mut out = [0u8; 26];
    for slot in out.iter_mut().rev() {
        *slot = CROCKFORD[(value & 0x1F) as usize];
        value >>= 5;
    }
    String::from_utf8(out.to_vec()).unwrap()
}

fn send_inbox_read_and_who_go_through_the_office() {
    let world = World::new();
    let _server = world.office();

    // pixel sends by handle; the office answers with the message's id. No spool, no notice.
    let sent = world
        .run(
            "pixel",
            &[
                "msg",
                "send",
                "--to",
                "nova",
                "--subject",
                "hi",
                "--json",
                "hello nova",
            ],
        )
        .ok();
    assert_eq!(sent.stderr, "");
    let id = sent.json()["id"].as_str().unwrap().to_owned();
    assert!(world.spooled().is_empty(), "the send was spooled");

    // nova's inbox, through the office: the spool's row shape, headers only.
    let inbox = world.run("nova", &["msg", "inbox", "--json"]).ok();
    assert_eq!(inbox.stderr, "");
    let rows = inbox.json();
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], id.as_str());
    // Members by handle, as the spool showed names (the member ids are in who).
    assert_eq!(rows[0]["from"], "pixel");
    assert_eq!(rows[0]["to"], "nova");
    assert_eq!(rows[0]["subject"], "hi");
    assert_eq!(rows[0]["status"], "unread");
    assert_eq!(rows[0]["kind"], "msg");
    assert!(rows[0].get("body").is_none());
    let summary = world
        .run("nova", &["msg", "inbox", "--summary", "--json"])
        .ok();
    assert!(summary.json()[0].get("body").is_none());
    let text = world.run("nova", &["msg", "inbox"]).ok();
    let columns: Vec<&str> = text.stdout.split('\t').collect();
    assert_eq!(columns.len(), 5, "{}", text.stdout);
    assert_eq!(columns[0], id);
    assert_eq!(columns[4], "hi");

    // A second message, so --limit and the order can be seen: oldest first, like the spool.
    let second = world
        .run("pixel", &["msg", "send", "--to", "nova", "second"])
        .ok()
        .stdout;
    let both = world.run("nova", &["msg", "inbox", "--json"]).ok().json();
    let ids: Vec<&str> = both
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec![id.as_str(), second.as_str()]);
    let limited = world
        .run("nova", &["msg", "inbox", "--limit", "1", "--json"])
        .ok();
    assert_eq!(limited.json().as_array().unwrap().len(), 1);
    assert_eq!(limited.json()[0]["id"], id.as_str());

    // read --next takes the oldest and marks it read; read <id> takes the one named.
    let read = world.run("nova", &["msg", "read", "--next", "--json"]).ok();
    let message = read.json();
    assert_eq!(message["id"], id.as_str());
    assert_eq!(message["body"], "hello nova");
    assert_eq!(message["from"], "pixel");
    assert_eq!(message["kind"], "msg");
    assert_eq!(message["subject"], "hi");
    let by_id = world.run("nova", &["msg", "read", &second]).ok();
    assert!(
        by_id
            .stdout
            .starts_with(&format!("{second} [msg] pixel -> nova")),
        "{}",
        by_id.stdout
    );
    assert!(by_id.stdout.ends_with("\nsecond"), "{}", by_id.stdout);
    assert_eq!(
        world.run("nova", &["msg", "inbox", "--json"]).ok().stdout,
        "[]"
    );
    assert_eq!(
        world
            .run("nova", &["msg", "read", "--next", "--json"])
            .ok()
            .stdout,
        "null"
    );
    let all = world.run("nova", &["msg", "inbox", "--all"]).ok();
    let lines: Vec<&str> = all.stdout.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(
        lines.iter().all(|l| l.split('\t').nth(5) == Some("read")),
        "{}",
        all.stdout
    );

    // who, through the office, in the spool's card shape.
    let everyone = world.run("pixel", &["who", "--json"]).ok().json();
    let cards = everyone.as_array().unwrap();
    for member in [PIXEL, NOVA] {
        assert!(cards.iter().any(|c| c["id"] == member), "{everyone}");
    }
    assert!(cards.iter().any(|c| c["owner"] == true), "{everyone}");
    let nova = world.run("pixel", &["who", "nova", "--json"]).ok().json();
    assert_eq!(nova["id"], NOVA);
    assert_eq!(nova["kind"], "agent");
    let owner = world
        .run("pixel", &["who", "--kind", "owner", "--json"])
        .ok()
        .json();
    assert_eq!(owner.as_array().unwrap().len(), 1);
    assert_eq!(owner[0]["owner"], true);
    let row = world.run("pixel", &["who", "nova"]).ok();
    assert!(
        row.stdout.contains(&format!("id:          {NOVA}")),
        "{}",
        row.stdout
    );
}

fn office_errors_keep_their_exit_codes_and_never_show_a_body() {
    let world = World::new();
    let _server = world.office();
    let secret = "a secret body";

    // Nobody by that name: unknown recipient, exit 3, and the body is in no output.
    let unknown = world.run("pixel", &["msg", "send", "--to", "nobody", secret]);
    assert_eq!(unknown.code, 3, "{}", unknown.stderr);
    assert!(!unknown.stderr.contains(secret) && !unknown.stdout.contains(secret));
    assert!(world.spooled().is_empty());

    // An agent the office doesn't know can't bind: exit 1, naming the office and the agent,
    // and no fall back to the spool.
    let stranger = world.run("ghost", &["msg", "send", "--to", "nova", secret]);
    assert_eq!(stranger.code, 1, "{}", stranger.stderr);
    assert!(stranger.stderr.contains(OFFICE), "{}", stranger.stderr);
    assert!(stranger.stderr.contains("'ghost'"), "{}", stranger.stderr);
    assert!(!stranger.stderr.contains(secret));
    assert!(world.spooled().is_empty());
    assert_eq!(world.run("ghost", &["msg", "inbox"]).code, 1);

    // A message id that isn't nova's: not found, exit 1.
    let missing = world.run("nova", &["msg", "read", "01NOSUCHMESSAGE00000000000"]);
    assert_eq!(missing.code, 1, "{}", missing.stderr);

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

fn wait_wakes_on_a_send() {
    let world = World::new();
    let _server = world.office();

    let mut waiting = world
        .agent("nova", &["msg", "wait", "--timeout", "30s", "--json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Held: nothing has been sent, so it must still be waiting.
    std::thread::sleep(Duration::from_millis(800));
    assert!(
        waiting.try_wait().unwrap().is_none(),
        "wait ended with nothing sent"
    );

    world
        .run("pixel", &["msg", "send", "--to", "nova", "wake up"])
        .ok();
    let started = Instant::now();
    while waiting.try_wait().unwrap().is_none() {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the send didn't wake the wait"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let woken: Run = waiting.wait_with_output().unwrap().into();
    let woken = woken.ok();
    assert_eq!(woken.json()["body"], "wake up");
    assert_eq!(woken.json()["from"], "pixel");
    // wait doesn't mark it read.
    let inbox = world.run("nova", &["msg", "inbox", "--json"]).ok().json();
    assert_eq!(inbox.as_array().unwrap().len(), 1);

    // --from and --urgent-only reach the office: a normal message from pixel doesn't end an
    // urgent-only wait, and an urgent one does.
    let mut urgent = world
        .agent(
            "nova",
            &[
                "msg",
                "wait",
                "--urgent-only",
                "--from",
                "pixel",
                "--timeout",
                "30s",
            ],
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(800));
    assert!(
        urgent.try_wait().unwrap().is_none(),
        "an urgent-only wait took a normal message"
    );
    world
        .run(
            "pixel",
            &["msg", "send", "--to", "nova", "--priority", "urgent", "now"],
        )
        .ok();
    let started = Instant::now();
    while urgent.try_wait().unwrap().is_none() {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "an urgent send didn't wake it"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let woken: Run = urgent.wait_with_output().unwrap().into();
    assert!(woken.ok().stdout.ends_with("\nnow"));
}

fn wait_times_out_and_a_person_can_interrupt_it() {
    let world = World::new();
    let _server = world.office();

    let started = Instant::now();
    let timed_out = world.run("nova", &["msg", "wait", "--timeout", "1s"]);
    assert_eq!(timed_out.code, 2, "{}", timed_out.stderr);
    assert!(
        timed_out.stderr.contains("timed out"),
        "{}",
        timed_out.stderr
    );
    assert!(started.elapsed() >= Duration::from_millis(900));

    let waiting = world
        .agent("nova", &["msg", "wait", "--timeout", "30s"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(800));
    let status = Command::new("kill")
        .args(["-INT", &waiting.id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());
    let started = Instant::now();
    let ended = waiting.wait_with_output().unwrap();
    assert_eq!(ended.status.code(), Some(130));
    assert!(started.elapsed() < Duration::from_secs(5));
}

fn an_agent_offboarded_during_a_wait_is_told_and_not_left_waiting() {
    let world = World::new();
    let _server = world.office();
    let waiting = world
        .agent("nova", &["msg", "wait", "--timeout", "3s"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(800));
    let app = Client::connect(world.socket()).unwrap();
    app.bind_app(token()).unwrap();
    app.call::<MemberOffboard>(&MemberOffboardParams {
        command_id: command_id(9),
        member_id: NOVA.into(),
        expected_version: 1,
    })
    .unwrap();

    // The held wait ends no later than its timeout. Today the office re-checks a binding only
    // when the next call arrives, so it times out (2); if it ends held waits on offboarding, it
    // answers not_bound, org-agent binds again, is refused, and says so (1).
    let started = Instant::now();
    let ended = waiting.wait_with_output().unwrap();
    assert!(started.elapsed() < Duration::from_secs(10));
    let ended: Run = ended.into();
    match ended.code {
        2 => assert!(ended.stderr.contains("timed out"), "{}", ended.stderr),
        1 => assert!(
            ended
                .stderr
                .contains(&format!("office {OFFICE} unbound 'nova'"))
                && ended.stderr.contains("won't bind it again"),
            "{}",
            ended.stderr
        ),
        other => panic!("exit {other}: {}", ended.stderr),
    }

    // Its next command is refused at the bind, clearly, and nothing falls back to the spool.
    let next = world.run("nova", &["msg", "inbox"]);
    assert_eq!(next.code, 1, "{}", next.stderr);
    assert!(
        next.stderr.contains("did not let 'nova' in"),
        "{}",
        next.stderr
    );
    assert!(next.stdout.is_empty());
}

fn a_send_to_an_office_that_is_down_is_refused_until_ingest() {
    let world = World::new();
    let mut server = world.office();
    server.stop();

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
    let _server = world.office();
    // Running, with its office.json, but not named: the local spool, quietly, as before.
    world.run_unnamed("nova", &["msg", "register", "nova"]).ok();
    let sent = world
        .run_unnamed("pixel", &["msg", "send", "--to", "nova", "--json", "hi"])
        .ok();
    assert_eq!(sent.stderr, "");
    assert!(sent.json().get("queued").is_none());
    assert_eq!(world.spooled().len(), 1);
    let inbox = world.run_unnamed("nova", &["msg", "inbox", "--json"]).ok();
    assert_eq!(inbox.stderr, "");
    // Named, the office has nothing for nova: the send didn't go there.
    assert_eq!(
        world.run("nova", &["msg", "inbox", "--json"]).ok().stdout,
        "[]"
    );
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
    let mut server = world.office();
    server.stop();
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
