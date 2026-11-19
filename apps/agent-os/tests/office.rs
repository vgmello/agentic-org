//! `agent-os` against a real office server: `who` goes through the office; `msg send | inbox |
//! read | wait` reach it and are refused, because the office does not serve messaging until B6
//! rebuilds it on the office store; and a `send` to an office that is down is refused (exit 5)
//! without spooling.
//!
//! The server is the real one: this test binary, started with `ORG_AGENT_TEST_OFFICE_SERVER`
//! set, runs exactly what `agentic-org-office`'s `main` runs (its library). It is driven the way
//! the host drives it (`apps/agentic-org-office/tests/launch.rs`): the office's database key is a
//! fixed test key on the launch line, so no Keychain is involved. Every home is a temporary
//! folder: the office home (`ORG_OFFICE_HOME`) and the agents' `HOME` and `ORG_HOME` (their
//! spool), so nothing here can reach `~/.agency` or a real office.
//!
//! Its own `main` (`harness = false`), because the server writes control lines on stdout.

use std::io::{BufReader, Write};
use std::os::fd::AsFd;
use std::os::unix::net::{UnixDatagram, UnixListener, UnixStream};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Output, Stdio};
use std::time::{Duration, Instant};

use agentic_org_office::test_support::{grant_key, signed_grant, ChannelCaller};
use org_office::rules::OrgCommand;
use org_office_launch::ServerEvent;
use org_office_launch::{
    self, ChannelKind, Handoff, HandoffFd, HostFirstLine, OfficeLine, RendezvousEvent,
    TransferRecord,
};
use org_proto::org::{
    AgentProfile, AgentRuntime, AgentStatus, MemberAddAgentParams, MemberOffboardParams,
};
use org_proto::OfficeSecretsKey;
use serde_json::Value;
use tempfile::TempDir;

/// Set on the child that is the office server.
const SERVER_ENV: &str = "ORG_AGENT_TEST_OFFICE_SERVER";
const OFFICE: &str = "01OFFICE00000000000000HQ1";
const ORG: &str = "01ORG0000000000000000ACME1";
const PIXEL: &str = "AGENT00000000000000000PIXL";
const NOVA: &str = "AGENT00000000000000000NOVA";
const WAIT: Duration = Duration::from_secs(30);
/// The instance the host starts the office as (the worker adopts the library-stored office into it).
const INSTANCE: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607182";

fn main() {
    if std::env::var_os(SERVER_ENV).is_some() {
        serve();
    }
    let tests: &[(&str, fn())] = &[
        ("who_goes_through_the_office", who_goes_through_the_office),
        (
            "messages_through_the_office_are_refused_unsupported_and_show_no_body",
            messages_through_the_office_are_refused_unsupported_and_show_no_body,
        ),
        (
            "office_errors_keep_their_exit_codes_and_never_show_a_body",
            office_errors_keep_their_exit_codes_and_never_show_a_body,
        ),
        (
            "an_offboarded_agent_is_refused_at_the_bind",
            an_offboarded_agent_is_refused_at_the_bind,
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

/// The office server, exactly as `agentic-org-office`'s `main` runs it.
fn serve() -> ! {
    let control = match agentic_org_office::fdswap::take_control_pipe() {
        Ok(control) => control,
        Err(error) => {
            eprintln!("agentic-org-office: cannot set up its descriptors: {error}");
            std::process::exit(1);
        }
    };
    agentic_org_office::install_panic_hook();
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(agentic_org_office::run(&args, control));
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

    /// Stores the office with the library and hires pixel and nova into it there (the worker
    /// creates only through HQ creation-v1, and no app or person binds on the office socket),
    /// then starts it the way the host does (the first line on stdin, the keys and the
    /// transfer socket over the rendezvous) and returns the running server.
    fn office(&self) -> Server {
        let office = org_office::Office::create(
            org_office_launch::office_dir(&self.home, OFFICE).unwrap(),
            &org_office::NewOffice {
                office_id: OFFICE.into(),
                office_name: "HQ".into(),
                role: org_office::OfficeRole::Hq,
                org_id: ORG.into(),
                org_name: "Acme".into(),
            },
            org_office::Key::from_office_secrets_key(office_secrets_key()),
        )
        .unwrap();
        for (n, id, handle) in [(1, PIXEL, "pixel"), (2, NOVA, "nova")] {
            office
                .submit_org(OrgCommand::MemberAddAgent(MemberAddAgentParams {
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
                }))
                .unwrap()
                .unwrap();
        }
        let owner = office
            .org_snapshot()
            .unwrap()
            .members
            .into_iter()
            .find(|m| m.owner)
            .unwrap()
            .id;
        drop(office);

        let rendezvous = self.home.parent().unwrap().join("r");
        let _ = std::fs::remove_file(&rendezvous);
        let listener = UnixListener::bind(&rendezvous).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--office", OFFICE])
            .env(SERVER_ENV, "1")
            .env("ORG_OFFICE_HOME", &self.home)
            .env("HOME", &self.user)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take();
        org_office_launch::write_first_line(
            stdin.as_mut().unwrap(),
            &HostFirstLine::Start(OfficeLine {
                id_profile: None,
                selected_writer_required: false,
                instance_id: INSTANCE.into(),
                office_id: OFFICE.into(),
                rendezvous: rendezvous.to_str().unwrap().into(),
            }),
        )
        .unwrap();
        let stream = accept(&listener, &mut child);
        let mut server = Server {
            reader: BufReader::with_capacity(1, stream.try_clone().unwrap()),
            stream,
            transfer: None,
            stdin,
            child,
            owner,
            channels: 0,
        };
        let ready_for_secrets: Value = serde_json::from_slice(
            &org_proto::frame::read_line(&mut server.reader)
                .unwrap()
                .expect("the rendezvous closed"),
        )
        .unwrap();
        assert_eq!(ready_for_secrets["event"], "ready_for_secrets");
        let mut handoff = org_office_launch::encode_handoff(&Handoff::Start {
            office_secrets_key: Some(office_secrets_key()),
            grant_key: grant_key(),
            fds: vec![HandoffFd::Transfer],
        })
        .unwrap();
        server.stream.write_all(&handoff).unwrap();
        handoff.fill(0);
        let (transfer, host_end) = UnixDatagram::pair().unwrap();
        transfer.shutdown(std::net::Shutdown::Write).unwrap();
        send_fds(&server.stream, &[org_office_launch::FD_MARKER], &transfer);
        server.expect_received("handoff");
        drop(transfer);
        server.transfer = Some(host_end);
        assert!(matches!(
            server.event(),
            RendezvousEvent::Server(ServerEvent::Ready { .. })
        ));
        server
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

/// Waits for the worker to connect to the rendezvous, failing if it exits first.
fn accept(listener: &UnixListener, child: &mut Child) -> UnixStream {
    listener.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + WAIT;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                stream.set_read_timeout(Some(WAIT)).unwrap();
                return stream;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => panic!("rendezvous accept: {e}"),
        }
        assert!(
            child.try_wait().unwrap().is_none(),
            "the worker exited before the rendezvous"
        );
        assert!(Instant::now() < deadline, "the worker never connected");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// One `sendmsg` of `payload` carrying `fd`, as the host sends descriptors.
fn send_fds(socket: &impl AsFd, payload: &[u8], fd: &impl AsFd) {
    org_socket::rights::send(
        socket.as_fd(),
        payload,
        &[fd.as_fd()],
        Instant::now() + Duration::from_secs(5),
    )
    .unwrap();
}

/// The running office: its process, the rendezvous it reports on and the host end of its
/// transfer socket.
struct Server {
    child: Child,
    stdin: Option<ChildStdin>,
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    transfer: Option<UnixDatagram>,
    owner: String,
    channels: u32,
}

impl Server {
    fn event(&mut self) -> RendezvousEvent {
        org_office_launch::read_server_event(&mut self.reader)
            .unwrap()
            .expect("the rendezvous closed")
    }

    fn expect_received(&mut self, of: &str) {
        match self.event() {
            RendezvousEvent::FdsReceived { of: got } => assert_eq!(got, of),
            other => panic!("expected fds_received for {of}, got {other:?}"),
        }
    }

    /// A new host channel to the office, acting as the owner with a signed grant (persons and
    /// the owner act only through grants on host channels).
    fn owner(&mut self) -> ChannelCaller {
        self.channels += 1;
        let id = format!("owner-{}", self.channels);
        let (source, peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(WAIT)).unwrap();
        let record = org_office_launch::encode_transfer_record(&TransferRecord::Channel {
            kind: ChannelKind::Local,
            office_id: OFFICE.into(),
            channel_id: id.clone(),
        })
        .unwrap();
        send_fds(self.transfer.as_ref().unwrap(), &record, &source);
        self.expect_received(&id);
        drop(source);
        let mut caller = ChannelCaller::new(peer);
        assert_eq!(caller.hello()["result"]["api_version"], 2);
        caller.with_grant(signed_grant(&self.owner, ORG, OFFICE, &id, "owner-session"))
    }

    /// Ends the rendezvous, as a host that goes away does, and waits for the server to exit.
    fn stop(&mut self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
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

/// The database key the host would hold for the office.
fn office_secrets_key() -> OfficeSecretsKey {
    OfficeSecretsKey::from_bytes([0xB3; 32])
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

fn who_goes_through_the_office() {
    let world = World::new();
    let _server = world.office();

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

/// The office answers every messaging call `unsupported` until B6 rebuilds messaging on the
/// office store. `agent-os` reports the refusal with its exit code for an office error it has
/// no class for (4, `exit_code`'s default), prints nothing on stdout, never shows the body,
/// and never falls back to a spool: the office was reached and answered.
fn messages_through_the_office_are_refused_unsupported_and_show_no_body() {
    let world = World::new();
    let _server = world.office();
    let secret = "a secret body";
    let refused = |run: Run, what: &str| {
        assert_eq!(run.code, 4, "{what}: {}", run.stderr);
        assert_eq!(run.stdout, "", "{what}");
        assert!(
            run.stderr.contains("does not serve messaging yet"),
            "{what}: {}",
            run.stderr
        );
        assert!(!run.stderr.contains(secret), "{what}");
    };
    refused(
        world.run(
            "pixel",
            &[
                "msg",
                "send",
                "--to",
                "nova",
                "--subject",
                "hi",
                "--json",
                secret,
            ],
        ),
        "send",
    );
    refused(world.run("nova", &["msg", "inbox", "--json"]), "inbox");
    refused(
        world.run("nova", &["msg", "read", "--next", "--json"]),
        "read",
    );
    // Answered at once: the office does not hold it.
    let started = Instant::now();
    refused(
        world.run("nova", &["msg", "wait", "--timeout", "30s"]),
        "wait",
    );
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(world.spooled().is_empty(), "a refused send was spooled");
    assert!(world.office_spooled().is_empty());
}

fn office_errors_keep_their_exit_codes_and_never_show_a_body() {
    let world = World::new();
    let _server = world.office();
    let secret = "a secret body";

    // An agent the office doesn't know can't bind: exit 1, naming the office and the agent,
    // and no fall back to the spool.
    let stranger = world.run("ghost", &["msg", "send", "--to", "nova", secret]);
    assert_eq!(stranger.code, 1, "{}", stranger.stderr);
    assert!(stranger.stderr.contains(OFFICE), "{}", stranger.stderr);
    assert!(stranger.stderr.contains("'ghost'"), "{}", stranger.stderr);
    assert!(!stranger.stderr.contains(secret));
    assert!(world.spooled().is_empty());
    assert_eq!(world.run("ghost", &["msg", "inbox"]).code, 1);

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

fn an_offboarded_agent_is_refused_at_the_bind() {
    let world = World::new();
    let mut server = world.office();
    // Bound before: the office knows nova.
    world.run("nova", &["who", "nova", "--json"]).ok();
    let mut owner = server.owner();
    let offboarded = owner.call(
        "member.offboard",
        serde_json::to_value(MemberOffboardParams {
            command_id: command_id(9),
            member_id: NOVA.into(),
            expected_version: 1,
        })
        .unwrap(),
    );
    assert!(offboarded.get("result").is_some(), "{offboarded}");

    // Its next command is refused at the bind, clearly, and nothing falls back to the spool.
    let next = world.run("nova", &["msg", "inbox"]);
    assert_eq!(next.code, 1, "{}", next.stderr);
    assert!(
        next.stderr.contains("did not let 'nova' in"),
        "{}",
        next.stderr
    );
    assert!(next.stdout.is_empty());
    assert!(world.spooled().is_empty());
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
    // Named, the office is asked, and refuses: it does not serve messaging yet.
    let named = world.run("nova", &["msg", "inbox", "--json"]);
    assert_eq!(named.code, 4, "{}", named.stderr);
    assert_eq!(named.stdout, "");
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
