//! End-to-end integration tests for `org-agent` and `org-relay` binaries over a temp `$ORG_HOME`.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use org_msg::{generate_ulid, Envelope, MessageKind, Spool};

fn org_agent_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_org-agent"))
}

fn org_relay_bin() -> PathBuf {
    let mut dir = org_agent_bin();
    dir.pop();
    let candidate = dir.join("org-relay");
    if candidate.is_file() {
        return candidate;
    }
    // Build org-relay if not yet built in this profile
    let workspace_manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("Cargo.toml");
    let status = Command::new("cargo")
        .args([
            "build",
            "--offline",
            "--manifest-path",
            workspace_manifest.to_str().unwrap(),
            "-p",
            "org-relay",
        ])
        .status()
        .expect("build org-relay");
    assert!(status.success());
    candidate
}

struct TempOrgHome {
    root: PathBuf,
    relay: Option<Child>,
}

impl TempOrgHome {
    fn new(tag: &str) -> Self {
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

    fn start_relay(&mut self) {
        let child = Command::new(org_relay_bin())
            .arg("run")
            .env("ORG_HOME", &self.root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn org-relay");
        self.relay = Some(child);
        // Wait briefly for relay.sock to appear
        for _ in 0..40 {
            if self.root.join("spool/relay.sock").exists() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn stop_relay(&mut self) {
        if let Some(mut child) = self.relay.take() {
            let _ = Command::new("kill")
                .args(["-TERM", &child.id().to_string()])
                .status();
            let _ = child.wait();
        }
    }

    fn run_agent(&self, as_agent: &str, args: &[&str]) -> (i32, String, String) {
        Self::run_agent_at(&self.root, as_agent, args)
    }

    fn run_agent_at(root: &Path, as_agent: &str, args: &[&str]) -> (i32, String, String) {
        let out = Command::new(org_agent_bin())
            .args(args)
            .env("ORG_HOME", root)
            .env("ORG_AGENT", as_agent)
            .output()
            .expect("run org-agent");
        let code = out.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        (code, stdout, stderr)
    }
}

impl Drop for TempOrgHome {
    fn drop(&mut self) {
        self.stop_relay();
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn send_to_read_and_fire_and_forget_while_relay_down() {
    let mut env = TempOrgHome::new("send_read");

    // Register both agents while relay is still DOWN
    let (c1, _, _) = env.run_agent("ac-staff-engineer-ada", &["msg", "register"]);
    let (c2, _, _) = env.run_agent("agy-senior-dev", &["msg", "register"]);
    assert_eq!(c1, 0);
    assert_eq!(c2, 0);

    // Fire-and-forget send while relay is DOWN: must succeed and queue in outbox/new
    let (code, msg_id, _) = env.run_agent(
        "agy-senior-dev",
        &[
            "msg",
            "send",
            "--to",
            "ac-staff-engineer-ada",
            "--subject",
            "offline test",
            "queued while relay down",
        ],
    );
    assert_eq!(code, 0);
    assert_eq!(msg_id.len(), 26);
    assert!(
        env.root
            .join(format!("spool/outbox/new/{msg_id}.json"))
            .is_file(),
        "message should sit in outbox/new while relay is down"
    );

    // Now start the relay: it drains outbox/new on startup
    env.start_relay();
    for _ in 0..40 {
        if env
            .root
            .join(format!("spool/inbox/ac-staff-engineer-ada/new/{msg_id}.json"))
            .is_file()
        {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }

    // Check ada's inbox and read the message (which moves new -> cur)
    let (code_in, inbox_out, _) =
        env.run_agent("ac-staff-engineer-ada", &["msg", "inbox", "--json"]);
    assert_eq!(code_in, 0);
    assert!(inbox_out.contains(&msg_id));

    let (code_rd, read_out, _) =
        env.run_agent("ac-staff-engineer-ada", &["msg", "read", &msg_id]);
    assert_eq!(code_rd, 0);
    assert!(read_out.contains("queued while relay down"));

    // Subsequent unread inbox is empty, and cur/ holds the message
    let (_, inbox_after, _) = env.run_agent("ac-staff-engineer-ada", &["msg", "inbox"]);
    assert!(inbox_after.is_empty());
    assert!(
        env.root
            .join(format!("spool/inbox/ac-staff-engineer-ada/cur/{msg_id}.json"))
            .is_file()
    );
}

#[test]
fn ask_and_reply_round_trip_with_live_relay() {
    let mut env = TempOrgHome::new("ask_reply");
    env.run_agent("ac-staff-engineer-ada", &["msg", "register"]);
    env.run_agent("agy-senior-dev", &["msg", "register"]);
    env.start_relay();

    let root_clone = env.root.clone();
    let responder = thread::spawn(move || {
        // Wait for incoming request on ac-staff-engineer-ada
        let (w_code, w_json, w_err) = TempOrgHome::run_agent_at(
            &root_clone,
            "ac-staff-engineer-ada",
            &["msg", "wait", "--timeout", "5s", "--json"],
        );
        assert_eq!(w_code, 0, "wait failed: {w_err}");
        let req: Envelope = Envelope::from_json_bytes(w_json.as_bytes()).expect("parse waited request");
        assert_eq!(req.kind, MessageKind::Request);

        // Claim it and send reply
        let (r_code, _, _) = TempOrgHome::run_agent_at(
            &root_clone,
            "ac-staff-engineer-ada",
            &["msg", "read", &req.id],
        );
        assert_eq!(r_code, 0);

        let (rep_code, _, rep_err) = TempOrgHome::run_agent_at(
            &root_clone,
            "ac-staff-engineer-ada",
            &["msg", "reply", &req.id, "go for merge into v1"],
        );
        assert_eq!(rep_code, 0, "reply failed: {rep_err}");
    });

    let (ask_code, ask_out, ask_err) = env.run_agent(
        "agy-senior-dev",
        &[
            "msg",
            "ask",
            "--to",
            "ac-staff-engineer-ada",
            "--timeout",
            "5s",
            "ready for review?",
        ],
    );
    responder.join().unwrap();
    assert_eq!(ask_code, 0, "ask failed: {ask_err}");
    assert_eq!(ask_out, "go for merge into v1");
}

#[test]
fn unknown_recipient_exits_3_and_deregistered_or_expired_goes_to_dead() {
    let env = TempOrgHome::new("dead_and_exit3");
    env.run_agent("agy-senior-dev", &["msg", "register"]);

    // (a) send --to nobody exits 3 and outbox/new is empty (0004 §3a)
    let (code, _, stderr) = env.run_agent(
        "agy-senior-dev",
        &["msg", "send", "--to", "nobody", "hello?"],
    );
    assert_eq!(code, 3, "expected exit 3, stderr={stderr}");
    assert!(stderr.contains("unknown recipient nobody"));
    let outbox_files: Vec<_> = fs::read_dir(env.root.join("spool/outbox/new"))
        .unwrap()
        .collect();
    assert!(outbox_files.is_empty(), "outbox/new must remain empty");

    // (b) agent deregistered after send, before relay runs -> dead/ with reason (0004 §3b)
    env.run_agent("ephemeral-agent", &["msg", "register"]);
    let (s_code, msg_id, _) = env.run_agent(
        "agy-senior-dev",
        &["msg", "send", "--to", "ephemeral-agent", "before deregister"],
    );
    assert_eq!(s_code, 0);
    env.run_agent("agy-senior-dev", &["msg", "deregister", "ephemeral-agent"]);

    // (c) expired message in outbox -> dead/ with reason (0004 §3c)
    let spool = Spool::open(&env.root).unwrap();
    let mut expired =
        Envelope::new_msg("agy-senior-dev", "agy-senior-dev", None, "too late", BTreeMap::new())
            .unwrap();
    expired.expires_at = Some("1999-12-31T23:59:59.000Z".into());
    spool.enqueue(&expired).unwrap();

    // Run org-relay --once to process both
    let relay_out = Command::new(org_relay_bin())
        .args(["run", "--once"])
        .env("ORG_HOME", &env.root)
        .output()
        .unwrap();
    assert!(relay_out.status.success());

    let reason_b =
        fs::read_to_string(env.root.join(format!("spool/dead/{msg_id}.reason"))).unwrap();
    assert!(reason_b.contains("unknown_recipient: ephemeral-agent"));

    let reason_c =
        fs::read_to_string(env.root.join(format!("spool/dead/{}.reason", expired.id))).unwrap();
    assert!(reason_c.contains("expired"));
}

#[test]
fn ask_timeout_exits_with_code_2() {
    let mut env = TempOrgHome::new("ask_timeout");
    env.run_agent("ac-staff-engineer-ada", &["msg", "register"]);
    env.run_agent("agy-senior-dev", &["msg", "register"]);
    env.start_relay();

    let (code, _, stderr) = env.run_agent(
        "agy-senior-dev",
        &[
            "msg",
            "ask",
            "--to",
            "ac-staff-engineer-ada",
            "--timeout",
            "80ms",
            "nobody will answer",
        ],
    );
    assert_eq!(code, 2, "expected exit code 2 on timeout, stderr={stderr}");
}
