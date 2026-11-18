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
use org_proto::DbKey;
use support::{eventually, private_program, sibling_program, Client};

const WAIT: Duration = Duration::from_secs(30);
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
            org_office::Key::from_db_key(DbKey::from_bytes(KEY)),
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

/// The early production path: the real host admits the real worker through the platform's
/// admission and the worker reaches `ready` (it serves its socket as this office).
#[test]
fn the_real_host_starts_the_real_worker() {
    let world = World::new();
    let mut host = world.spawn_host(&world.worker);
    world.wait_until_ready(&mut host);
    let log = world.host_log();
    assert!(!log.contains("couldn't start the server"), "{log}");
    assert!(!log.contains("refused office server"), "{log}");
    let _ = &world.agent_os;
}
