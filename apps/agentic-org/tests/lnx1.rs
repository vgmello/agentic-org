//! ADR-0017 LNX-1: the real host composition starts a real office worker, the worker serves
//! its local socket, and `agent-os` sends a message and reads it back. The same test runs on
//! every system that has an admission adapter.
//!
//! Everything here is a real executable built by cargo: `agentic-org serve --name default`
//! on a registered instance (setup's steps through A2's fixture), `agentic-org-office` as the
//! worker, and `agent-os` as the client. The host admits the worker through the platform's
//! admission (`org_platform_host::admission`); nothing in this file names a system.
//!
//! The worker and `agent-os` belong to other packages, so cargo gives this test no path for
//! them: they are taken from the folder cargo built `agentic-org` into. `cargo test
//! --workspace` (what CI runs) builds all three; a narrower run needs `cargo build -p
//! agentic-org-office -p agent-os` first, and a missing program fails the test saying so.

// The host keeps its keys in a development key folder, which only debug builds have.
#![cfg(debug_assertions)]

mod support;

use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use org_host::setup_flow::fixture::{registered_account, RegisteredAccount};
use org_office::rules::OrgCommand;
use org_proto::org::{AgentProfile, AgentRuntime, AgentStatus, MemberAddAgentParams};
use org_proto::OfficeSecretsKey;
use support::{eventually, private_program, sibling_program, Client};

const WAIT: Duration = Duration::from_secs(30);
/// How long the service may take to exit after `SIGTERM`, with its workers stopped: the
/// design's bound (Linux host design section 3.4), inside Docker Desktop's default grace of
/// 3 s.
const STOP_WITHIN: Duration = Duration::from_millis(2500);
const OFFICE: &str = "01OFFICE00000000000000HQ1";
const ORG: &str = "01ORG0000000000000000ACME1";
const PIXEL: &str = "AGENT00000000000000000PIXL";
const NOVA: &str = "AGENT00000000000000000NOVA";
/// The office's secrets key: the host's development key store holds it as hex.
const KEY: [u8; 32] = [0xB3; 32];

/// A host process that is killed and reaped when dropped.
struct HostProcess(Child);

impl Drop for HostProcess {
    fn drop(&mut self) {
        if let Ok(None) = self.0.try_wait() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

/// A registered instance under a short temporary base, with one office the office library
/// stored (as the host's provisioning leaves it) and its key in the development key store.
struct World {
    dir: tempfile::TempDir,
    account: RegisteredAccount,
    /// This user's own copies of the worker and of `agent-os`.
    worker: PathBuf,
    agent_os: PathBuf,
}

impl World {
    fn new() -> Self {
        let dir = support::short_tempdir("ol");
        let root = dir.path().canonicalize().unwrap();
        for name in ["a", "k", "u"] {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(root.join(name))
                .unwrap();
        }
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let account = registered_account(&root.join("a"), None, port).unwrap();
        let worker = private_program(&sibling_program("agentic-org-office"));
        let agent_os = private_program(&sibling_program("agent-os"));
        let world = Self {
            dir,
            account,
            worker,
            agent_os,
        };
        world.store_office();
        world
    }

    fn base(&self) -> PathBuf {
        self.dir.path().canonicalize().unwrap()
    }

    /// Stores the office with the office library and hires two agents into it, then puts its
    /// key where the host's development key store reads it.
    fn store_office(&self) {
        let office = org_office::Office::create(
            org_office_launch::office_dir(&self.account.office_home, OFFICE).unwrap(),
            &org_office::NewOffice {
                office_id: OFFICE.into(),
                office_name: "HQ".into(),
                role: org_office::OfficeRole::Hq,
                org_id: ORG.into(),
                org_name: "Acme".into(),
            },
            org_office::Key::from_office_secrets_key(OfficeSecretsKey::from_bytes(KEY)),
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
        drop(office);
        let keys = self.base().join("k").join(&self.account.instance_id);
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&keys)
            .unwrap();
        let hex: String = KEY.iter().map(|b| format!("{b:02x}")).collect();
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(keys.join(format!("{OFFICE}.key")))
            .unwrap()
            .write_all(hex.as_bytes())
            .unwrap();
    }

    /// Starts `agentic-org serve --name default` with the worker `program`, with only the
    /// environment the account's service gives it plus the development base and key folder.
    fn spawn_host(&self, program: &Path) -> HostProcess {
        let stderr = File::create(self.base().join("host.err")).unwrap();
        HostProcess(
            Command::new(env!("CARGO_BIN_EXE_agentic-org"))
                .args(["serve", "--name", "default", "--server-bin"])
                .arg(program)
                .env_clear()
                .env("HOME", self.base().join("u"))
                .env("ORG_DEV_ACCOUNT_BASE", self.base().join("a"))
                .env("ORG_OFFICE_DEV_KEY_DIR", self.base().join("k"))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(stderr)
                .spawn()
                .unwrap(),
        )
    }

    fn host_log(&self) -> String {
        fs::read_to_string(self.base().join("host.err")).unwrap_or_default()
    }

    fn office_socket(&self) -> PathBuf {
        org_proto::location::office_socket_in(&self.account.runtime, OFFICE).unwrap()
    }

    /// Waits until the worker the host started answers hello as this office on its socket.
    fn wait_until_ready(&self, host: &mut HostProcess) {
        let socket = self.office_socket();
        eventually(WAIT, "the worker to answer hello on its socket", || {
            assert!(
                host.0.try_wait().unwrap().is_none(),
                "serve exited early: {}",
                self.host_log()
            );
            if UnixStream::connect(&socket).is_err() {
                return false;
            }
            Client::connect(&socket)
                .call::<org_proto::Hello>(&org_proto::HelloParams::default())
                .is_ok_and(|hello| {
                    hello
                        .office
                        .is_some_and(|office| office.office.id == OFFICE)
                })
        });
    }
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

/// What one `agent-os` run printed and how it ended.
struct Run {
    code: i32,
    stdout: String,
    stderr: String,
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

    fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.stdout)
            .unwrap_or_else(|error| panic!("not JSON ({error}): {}", self.stdout))
    }
}

impl World {
    /// The office home `agent-os` is given, made once the worker serves its socket.
    ///
    /// `agent-os` finds an office by its folder under `<office home>/offices` and connects
    /// to `<office home>/run/<handle>.sock`, and it refuses a `run` that is a link. A
    /// registered instance keeps its sockets in the instance's runtime folder instead, which
    /// `agent-os` cannot name yet on any system (a managed agent's launch binding and a
    /// person's saved context are later increments; reported to the track's lead). Until
    /// then this test gives the agent an office home of its own: `offices` is a link to the
    /// instance's office folders, and `run` is a real folder that holds a second name (a hard
    /// link) of the worker's socket. Everything from the socket on is the product's route:
    /// the worker's peer check, `hello`, `msg.bind` as the agent, and the calls.
    fn agent_office_home(&self) -> PathBuf {
        let view = self.base().join("v");
        if !view.exists() {
            fs::DirBuilder::new().mode(0o700).create(&view).unwrap();
            std::os::unix::fs::symlink(
                self.account.office_home.join("offices"),
                view.join("offices"),
            )
            .unwrap();
            fs::DirBuilder::new()
                .mode(0o700)
                .create(view.join("run"))
                .unwrap();
            let socket = self.office_socket();
            fs::hard_link(&socket, view.join("run").join(socket.file_name().unwrap()))
                .expect("a second name for the worker's socket");
        }
        view
    }

    /// `agent-os <args>` as `agent`, by hand: no session token, the office named, and every
    /// home inside this world (the route the `agent-os` suite's own office tests use).
    fn agent(&self, agent: &str, args: &[&str]) -> Run {
        let out = Command::new(&self.agent_os)
            .args(args)
            .env_clear()
            .env("ORG_OFFICE_HOME", self.agent_office_home())
            .env("HOME", self.base().join("u"))
            .env("ORG_HOME", self.base().join("u"))
            .env("ORG_AGENT", agent)
            .env("ORG_OFFICE", OFFICE)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        Run {
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).trim().to_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        }
    }
}

/// LNX-1. The real host admits the real worker through the platform's admission, the worker
/// serves its local socket as this office, and `agent-os` sends a message through it and
/// reads it back; then the host stops cleanly on `SIGTERM` and takes its worker with it.
#[test]
fn the_real_host_starts_the_real_worker_and_agent_os_sends_and_reads() {
    if !support::may_run_the_service(
        "the_real_host_starts_the_real_worker_and_agent_os_sends_and_reads",
    ) {
        return;
    }
    let world = World::new();
    let mut host = world.spawn_host(&world.worker);
    world.wait_until_ready(&mut host);
    let log = world.host_log();
    assert!(!log.contains("couldn't start the server"), "{log}");
    assert!(!log.contains("refused office server"), "{log}");
    assert!(!log.contains("no office server can be started"), "{log}");

    // pixel sends to nova by handle; the office answers with the message's id.
    let sent = world
        .agent(
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
    // Nothing fell back to a local spool: the send went through the office.
    assert!(!world.base().join("u/spool/outbox/new").exists());

    // nova's inbox shows it, and reading it gives the body back.
    let inbox = world.agent("nova", &["msg", "inbox", "--json"]).ok().json();
    let rows = inbox.as_array().unwrap();
    assert_eq!(rows.len(), 1, "{inbox}");
    assert_eq!(rows[0]["id"], id.as_str());
    assert_eq!(rows[0]["from"], "pixel");
    assert_eq!(rows[0]["status"], "unread");
    let read = world
        .agent("nova", &["msg", "read", "--next", "--json"])
        .ok()
        .json();
    assert_eq!(read["id"], id.as_str());
    assert_eq!(read["body"], "hello nova");
    assert_eq!(read["from"], "pixel");
    assert_eq!(read["subject"], "hi");
    assert_eq!(
        world.agent("nova", &["msg", "inbox", "--json"]).ok().stdout,
        "[]"
    );

    // The service stops on SIGTERM, exits 0 and leaves no worker serving the socket.
    // SAFETY: sends a signal to this test's own child.
    assert_eq!(
        unsafe { libc::kill(host.0.id() as libc::pid_t, libc::SIGTERM) },
        0
    );
    let stopped = std::time::Instant::now();
    let status = loop {
        if let Some(status) = host.0.try_wait().unwrap() {
            break status;
        }
        assert!(stopped.elapsed() < WAIT, "the host didn't stop");
        std::thread::sleep(Duration::from_millis(20));
    };
    let took = stopped.elapsed();
    assert!(status.success(), "{status}: {}", world.host_log());
    assert!(
        took < STOP_WITHIN,
        "the service took {took:?} to stop: {}",
        world.host_log()
    );
    println!("the service stopped {took:?} after SIGTERM");
    // It took its worker with it: nothing serves the socket once the service has exited.
    eventually(
        Duration::from_secs(1),
        "the worker's socket to stop answering",
        || UnixStream::connect(world.office_socket()).is_err(),
    );
}
