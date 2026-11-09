//! Shared harness: runs the real `org-agent` binary against a temporary `$ORG_HOME` under
//! `/tmp` (short enough for macOS socket paths). `HOME` is pointed there too, so nothing a test
//! runs can ever fall back to the live `~/.agency` spool.
#![allow(dead_code)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::Duration;

use org_spool::generate_ulid;

pub fn org_agent_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_org-agent"))
}

pub struct Run {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl From<Output> for Run {
    fn from(out: Output) -> Self {
        Self {
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).trim().to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        }
    }
}

pub struct TempOrgHome {
    pub root: PathBuf,
    relay: Option<Child>,
}

impl TempOrgHome {
    pub fn new(tag: &str) -> Self {
        let root = PathBuf::from("/tmp").join(format!(
            "oh_{}_{}_{}",
            tag,
            std::process::id(),
            &generate_ulid()[20..]
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Self { root, relay: None }
    }

    pub fn command(root: &Path, as_agent: &str, args: &[&str]) -> Command {
        let mut cmd = Command::new(org_agent_bin());
        // No office: these tests are about the spool, and must never reach a real office
        // through an inherited ORG_OFFICE_HOME or ORG_OFFICE (HOME is the temp root, so the
        // default office home is empty).
        cmd.args(args)
            .env("ORG_HOME", root)
            .env("HOME", root)
            .env_remove("ORG_OFFICE_HOME")
            .env_remove("ORG_OFFICE")
            .env_remove("ORG_SESSION_TOKEN")
            .env("ORG_AGENT", as_agent);
        cmd
    }

    pub fn start_relay(&mut self) {
        let child = Command::new(org_agent_bin())
            .args(["relay", "run"])
            .env("ORG_HOME", &self.root)
            .env("HOME", &self.root)
            .env_remove("ORG_OFFICE_HOME")
            .env_remove("ORG_OFFICE")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn org-agent relay run");
        self.relay = Some(child);
        for _ in 0..200 {
            if self.root.join("spool/relay.sock").exists() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn stop_relay(&mut self) {
        if let Some(mut child) = self.relay.take() {
            let _ = Command::new("kill")
                .args(["-TERM", &child.id().to_string()])
                .status();
            let _ = child.wait();
        }
    }

    /// Deliver everything in the outbox (the relay's job) once.
    pub fn deliver(&self) {
        let r = self.run("relay-runner", &["relay", "run", "--once"]);
        assert_eq!(r.code, 0, "relay --once: {}", r.stderr);
    }

    pub fn run(&self, as_agent: &str, args: &[&str]) -> Run {
        Self::run_at(&self.root, as_agent, args)
    }

    pub fn run_at(root: &Path, as_agent: &str, args: &[&str]) -> Run {
        Self::command(root, as_agent, args)
            .output()
            .expect("run org-agent")
            .into()
    }

    pub fn run_stdin(&self, as_agent: &str, args: &[&str], stdin: &str) -> Run {
        let mut child = Self::command(&self.root, as_agent, args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap().into()
    }

    /// Register agents (exit 0 each).
    pub fn register(&self, agents: &[&str]) {
        for a in agents {
            let r = self.run(a, &["msg", "register"]);
            assert_eq!(r.code, 0, "register {a}: {}", r.stderr);
        }
    }

    /// `msg send` as `from` and deliver it; returns the id.
    pub fn send(&self, from: &str, to: &str, extra: &[&str]) -> String {
        let mut args = vec!["msg", "send", "--to", to];
        args.extend_from_slice(extra);
        let r = self.run(from, &args);
        assert_eq!(r.code, 0, "send: {}", r.stderr);
        self.deliver();
        r.stdout
    }

    pub fn inbox_file(&self, agent: &str, sub: &str, id: &str) -> PathBuf {
        self.root
            .join(format!("spool/inbox/{agent}/{sub}/{id}.json"))
    }
}

impl Drop for TempOrgHome {
    fn drop(&mut self) {
        self.stop_relay();
        let _ = fs::remove_dir_all(&self.root);
    }
}
