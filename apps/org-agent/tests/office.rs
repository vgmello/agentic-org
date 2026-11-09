//! `org-agent` against a real office server: `msg send | inbox | read | wait` and `who` go
//! through the office, and `send` falls back to the local spool when the office is down.
//!
//! The server is the real one: this test binary, started with `ORG_AGENT_TEST_OFFICE_SERVER`
//! set, runs exactly what `org-office-server`'s `main` runs (its library, built with `dev-keys`, so
//! keys are files in a temporary folder and no test touches a real Keychain). It is driven the
//! way the host drives it (`apps/org-office-server/tests/launch.rs`). Every home is a temporary
//! folder: the office home (`ORG_OFFICE_HOME`), the key folder (`ORG_OFFICE_DEV_KEY_DIR`) and the
//! agents' `HOME` and `ORG_HOME` (their spool), so nothing here can reach `~/.agency` or a real
//! office.
//!
//! Its own `main` (`harness = false`), because the server writes control lines on stdout.
//! Run it with `cargo test -p org-agent --features office-tests`.

use std::io::BufReader;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Output, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use org_client::Client;
use org_office_launch::{
    read_server_event, write_host_message, CreateOffice, HostMessage, ServerEvent,
};
use org_proto::location::office_socket_path;
use org_proto::org::{
    AgentProfile, AgentRuntime, AgentStatus, MemberAddAgent, MemberAddAgentParams, MemberOffboard,
    MemberOffboardParams,
};
use org_proto::{AppToken, OfficeRole};
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
            "send_falls_back_to_the_spool_when_the_office_is_down",
            send_falls_back_to_the_spool_when_the_office_is_down,
        ),
        (
            "with_no_office_on_this_mac_the_spool_is_used_quietly",
            with_no_office_on_this_mac_the_spool_is_used_quietly,
        ),
    ];
    let filter: Option<String> = std::env::args().skip(1).find(|a| !a.starts_with('-'));
    let mut passed = 0;
    let mut failed = Vec::new();
    for (name, test) in tests {
        if filter.as_deref().is_some_and(|f| !name.contains(f)) {
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

/// The office server, exactly as `org-office-server`'s `main` runs it.
fn serve() -> ! {
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

/// A temporary office home, key folder and agents' home, under /tmp (socket paths are short).
struct World {
    _dir: TempDir,
    home: PathBuf,
    keys: PathBuf,
    user: PathBuf,
}

impl World {
    fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("oa")
            .tempdir_in("/tmp")
            .unwrap();
        let home = dir.path().join("h");
        let keys = dir.path().join("k");
        let user = dir.path().join("u");
        for folder in [home.join("offices"), home.join("run"), user.clone()] {
            std::fs::create_dir_all(folder).unwrap();
        }
        Self {
            _dir: dir,
            home,
            keys,
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
            .env("ORG_OFFICE_DEV_KEY_DIR", &self.keys)
            .env("HOME", &self.user)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
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
        write_host_message(
            server.stdin.as_mut().unwrap(),
            &HostMessage::Create(CreateOffice {
                org_id: ORG.into(),
                org_name: "Acme".into(),
                office_id: OFFICE.into(),
                office_name: "HQ".into(),
                role: OfficeRole::Hq,
                app_token: token(),
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

    /// `org-agent <args>` as `agent`, with every home in this world and no office named.
    fn agent(&self, agent: &str, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_org-agent"));
        command
            .args(args)
            .env("ORG_OFFICE_HOME", &self.home)
            .env("HOME", &self.user)
            .env("ORG_HOME", &self.user)
            .env("ORG_AGENT", agent)
            .env_remove("ORG_OFFICE")
            .env_remove("ORG_SESSION_TOKEN")
            .stdin(Stdio::null());
        command
    }

    fn run(&self, agent: &str, args: &[&str]) -> Run {
        self.agent(agent, args).output().unwrap().into()
    }

    /// The files waiting in the spool's outbox.
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
    assert_eq!(rows[0]["from"], PIXEL);
    assert_eq!(rows[0]["to"], NOVA);
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
    assert_eq!(message["from"], PIXEL);
    assert_eq!(message["kind"], "msg");
    assert_eq!(message["subject"], "hi");
    let by_id = world.run("nova", &["msg", "read", &second]).ok();
    assert!(
        by_id
            .stdout
            .starts_with(&format!("{second} [msg] {PIXEL} -> {NOVA}")),
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
    assert_eq!(woken.json()["from"], PIXEL);
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

fn send_falls_back_to_the_spool_when_the_office_is_down() {
    let world = World::new();
    let mut server = world.office();
    // The spool needs nova registered to accept a send for it (Phase A's rule).
    world.run("nova", &["msg", "register", "nova"]).ok();
    server.stop();

    let secret = "kept for later";
    let started = Instant::now();
    let sent = world
        .run("pixel", &["msg", "send", "--to", "nova", "--json", secret])
        .ok();
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    let answer = sent.json();
    assert_eq!(answer["queued"], true);
    let id = answer["id"].as_str().unwrap();
    assert!(
        sent.stderr
            .contains(&format!("office {OFFICE} is not reachable")),
        "{}",
        sent.stderr
    );
    assert!(
        sent.stderr.contains("queued in the local spool"),
        "{}",
        sent.stderr
    );
    assert!(!sent.stderr.contains(secret));
    let spooled = world.spooled();
    assert_eq!(spooled.len(), 1);
    assert!(spooled[0].ends_with(format!("{id}.json")), "{spooled:?}");

    // A recipient the spool doesn't know: the spool refuses it (exit 3), and nothing claims it
    // was queued.
    let refused = world.run("pixel", &["msg", "send", "--to", "stranger", secret]);
    assert_eq!(refused.code, 3, "{}", refused.stderr);
    assert!(
        refused.stderr.contains("did not take the message"),
        "{}",
        refused.stderr
    );
    assert!(!refused.stderr.contains("queued in"), "{}", refused.stderr);
    assert!(!refused.stderr.contains(secret));
    assert_eq!(world.spooled().len(), 1);

    // The read side says the office isn't reachable and shows the local spool, as before.
    world.run("nova", &["relay", "run", "--once"]).ok();
    let inbox = world.run("nova", &["msg", "inbox", "--json"]).ok();
    assert!(
        inbox.stderr.contains("showing the local spool"),
        "{}",
        inbox.stderr
    );
    let rows = inbox.json();
    assert_eq!(rows[0]["id"], id);
    assert_eq!(rows[0]["body"], secret);
    let read = world.run("nova", &["msg", "read", "--next"]).ok();
    assert!(read.stdout.ends_with(secret));
    assert!(read.stderr.contains("not reachable"));
}

fn with_no_office_on_this_mac_the_spool_is_used_quietly() {
    let world = World::new();
    world.run("nova", &["msg", "register", "nova"]).ok();
    let sent = world
        .run("pixel", &["msg", "send", "--to", "nova", "--json", "hi"])
        .ok();
    assert_eq!(sent.stderr, "");
    assert!(sent.json().get("queued").is_none());
    assert_eq!(world.spooled().len(), 1);
    assert_eq!(world.run("nova", &["msg", "inbox"]).ok().stderr, "");
    // Named but missing, the office is unreachable rather than absent.
    let named = world
        .agent("pixel", &["msg", "send", "--to", "nova", "hi"])
        .env("ORG_OFFICE", OFFICE)
        .output()
        .unwrap();
    let named: Run = named.into();
    let named = named.ok();
    assert!(named.stderr.contains("not reachable"), "{}", named.stderr);
}
