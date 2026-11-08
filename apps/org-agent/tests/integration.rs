//! End-to-end integration tests for `org-agent` (including `relay`, `who`, `status`,
//! `defer`, `done`, `notify`, `wait --urgent-only`) over temporary `$ORG_HOME` directories.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use org_spool::{generate_ulid, Envelope, MessageKind, NotifyReport, Spool};

fn org_agent_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_org-agent"))
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
        let child = Command::new(org_agent_bin())
            .args(["relay", "run"])
            .env("ORG_HOME", &self.root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn org-agent relay run");
        self.relay = Some(child);
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

    let (c1, _, _) = env.run_agent("ac-staff-engineer-ada", &["msg", "register"]);
    let (c2, _, _) = env.run_agent("agy-senior-dev", &["msg", "register"]);
    assert_eq!(c1, 0);
    assert_eq!(c2, 0);

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

    env.start_relay();
    for _ in 0..40 {
        if env
            .root
            .join(format!(
                "spool/inbox/ac-staff-engineer-ada/new/{msg_id}.json"
            ))
            .is_file()
        {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }

    let (code_in, inbox_out, _) =
        env.run_agent("ac-staff-engineer-ada", &["msg", "inbox", "--json"]);
    assert_eq!(code_in, 0);
    assert!(inbox_out.contains(&msg_id));

    let (code_rd, read_out, _) = env.run_agent("ac-staff-engineer-ada", &["msg", "read", &msg_id]);
    assert_eq!(code_rd, 0);
    assert!(read_out.contains("queued while relay down"));

    let (_, inbox_after, _) = env.run_agent("ac-staff-engineer-ada", &["msg", "inbox"]);
    assert!(inbox_after.is_empty());
    assert!(env
        .root
        .join(format!(
            "spool/inbox/ac-staff-engineer-ada/cur/{msg_id}.json"
        ))
        .is_file());
}

#[test]
fn ask_and_reply_round_trip_with_live_relay() {
    let mut env = TempOrgHome::new("ask_reply");
    env.run_agent("ac-staff-engineer-ada", &["msg", "register"]);
    env.run_agent("agy-senior-dev", &["msg", "register"]);
    env.start_relay();

    let root_clone = env.root.clone();
    let responder = thread::spawn(move || {
        let (w_code, w_json, w_err) = TempOrgHome::run_agent_at(
            &root_clone,
            "ac-staff-engineer-ada",
            &["msg", "wait", "--urgent-only", "--timeout", "5s", "--json"],
        );
        assert_eq!(w_code, 0, "wait failed: {w_err}");
        let req: Envelope =
            Envelope::from_json_bytes(w_json.as_bytes()).expect("parse waited request");
        assert_eq!(req.kind, MessageKind::Request);

        // Replying directly (with `--body`) also marks `req.id` read in ada's inbox!
        let (rep_code, _, rep_err) = TempOrgHome::run_agent_at(
            &root_clone,
            "ac-staff-engineer-ada",
            &["msg", "reply", &req.id, "--body", "go for merge into v1"],
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
    assert!(ask_out.contains("go for merge into v1"));

    // Verify `reply` marked `req` read in ac-staff-engineer-ada's inbox
    let (_, ada_inbox, _) = env.run_agent("ac-staff-engineer-ada", &["msg", "inbox"]);
    assert!(ada_inbox.is_empty());
}

#[test]
fn unknown_recipient_exits_3_and_deregistered_or_expired_goes_to_dead() {
    let env = TempOrgHome::new("dead_and_exit3");
    env.run_agent("agy-senior-dev", &["msg", "register"]);

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

    env.run_agent("ephemeral-agent", &["msg", "register"]);
    let (s_code, msg_id, _) = env.run_agent(
        "agy-senior-dev",
        &[
            "msg",
            "send",
            "--to",
            "ephemeral-agent",
            "before deregister",
        ],
    );
    assert_eq!(s_code, 0);
    env.run_agent("agy-senior-dev", &["msg", "deregister", "ephemeral-agent"]);

    let spool = Spool::open(&env.root).unwrap();
    let mut expired = Envelope::new_msg(
        "agy-senior-dev",
        "agy-senior-dev",
        None,
        "too late",
        BTreeMap::new(),
    )
    .unwrap();
    expired.expires_at = Some("1999-12-31T23:59:59.000Z".into());
    spool.enqueue(&expired).unwrap();

    let (r_code, _, _) = env.run_agent("agy-senior-dev", &["relay", "run", "--once"]);
    assert_eq!(r_code, 0);

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

#[test]
fn triage_defer_done_notify_exit_codes_0_10_11_and_session_dedup() {
    let env = TempOrgHome::new("notify_e2e");
    env.run_agent(
        "ac-staff-engineer-ada",
        &["msg", "register", "--name", "Ada"],
    );
    env.run_agent("agy-senior-dev", &["msg", "register", "--name", "Agy"]);

    // 1. Empty inbox -> notify exits 0 with empty stdout
    let (c0, out0, _) = env.run_agent("agy-senior-dev", &["msg", "notify"]);
    assert_eq!(c0, 0);
    assert!(out0.is_empty());

    // 2. Normal message arrives -> mid-work notify exits 0 (not urgent, younger than sweep-after 5m),
    //    while --after-task notify exits 10!
    let (_, normal_id, _) = env.run_agent(
        "ac-staff-engineer-ada",
        &[
            "msg",
            "send",
            "--to",
            "agy-senior-dev",
            "--subject",
            "Routine update",
            "Body that must never appear in notify",
        ],
    );
    env.run_agent("agy-senior-dev", &["relay", "run", "--once"]);

    let (c_mid, out_mid, _) = env.run_agent(
        "agy-senior-dev",
        &["msg", "notify", "--session", "codex:s1"],
    );
    assert_eq!(c_mid, 0);
    assert!(out_mid.is_empty());

    let (c_break, out_break, _) = env.run_agent(
        "agy-senior-dev",
        &["msg", "notify", "--after-task", "--session", "codex:s1"],
    );
    assert_eq!(c_break, 10, "expected exit 10 for normal items");
    assert!(out_break.contains("1 new message(s):"));
    assert!(out_break.contains("Ada (ac-staff-engineer-ada): Routine update [msg]"));
    assert!(!out_break.contains("Body that must never appear"));

    // Repeat with same session -> deduplicated (exit 0)
    let (c_dup, out_dup, _) = env.run_agent(
        "agy-senior-dev",
        &["msg", "notify", "--after-task", "--session", "codex:s1"],
    );
    assert_eq!(c_dup, 0);
    assert!(out_dup.is_empty());

    // Read then defer --until after-task -> restores to unread and bumps triage_gen
    let (r_code, _, _) = env.run_agent("agy-senior-dev", &["msg", "read", &normal_id]);
    assert_eq!(r_code, 0);
    let (d_code, _, _) = env.run_agent(
        "agy-senior-dev",
        &[
            "msg",
            "defer",
            &normal_id,
            "--until",
            "after-task",
            "--note",
            "later",
        ],
    );
    assert_eq!(d_code, 0);

    // Next --after-task resurfaces it once in the same session!
    let (c_resurf, json_resurf, _) = env.run_agent(
        "agy-senior-dev",
        &[
            "msg",
            "notify",
            "--after-task",
            "--session",
            "codex:s1",
            "--format",
            "json",
        ],
    );
    assert_eq!(c_resurf, 10);
    let rep: NotifyReport = serde_json::from_str(&json_resurf).unwrap();
    assert_eq!(rep.count, 1);
    assert_eq!(rep.items[0].id, normal_id);
    assert_eq!(rep.items[0].reason, "after_task");
    assert_eq!(rep.items[0].triage_gen, 2);

    // Mark normal_id done
    let (done_code, _, _) = env.run_agent(
        "agy-senior-dev",
        &["msg", "done", &normal_id, "--note", "ok"],
    );
    assert_eq!(done_code, 0);

    // 3. Urgent message (`--priority high`) -> mid-work `--urgent-only` notify exits 11!
    let (_, urgent_id, _) = env.run_agent(
        "ac-staff-engineer-ada",
        &[
            "msg",
            "send",
            "--to",
            "agy-senior-dev",
            "--priority",
            "high",
            "--subject",
            "Drop everything",
            "Secret urgent body",
        ],
    );
    env.run_agent("agy-senior-dev", &["relay", "run", "--once"]);

    let (c_urg, out_urg, _) = env.run_agent(
        "agy-senior-dev",
        &["msg", "notify", "--urgent-only", "--session", "codex:s1"],
    );
    assert_eq!(c_urg, 11, "expected exit 11 for urgent item");
    assert!(out_urg.contains("1 new message(s), 1 urgent:"));
    assert!(out_urg.contains("- [URGENT] Ada (ac-staff-engineer-ada): Drop everything [msg]"));
    assert!(out_urg.contains(&urgent_id));
    assert!(!out_urg.contains("Secret urgent body"));
}

#[test]
fn who_profile_and_status_cli_commands() {
    let env = TempOrgHome::new("who_status_cli");
    let (c1, _, _) = env.run_agent(
        "agy-senior-dev",
        &[
            "profile",
            "set",
            "--name",
            "Agy Senior Dev",
            "--role",
            "Senior Engineer",
            "--team",
            "platform:Platform:Messaging",
            "--reports-to",
            "ac-staff-engineer-ada:Ada",
        ],
    );
    assert_eq!(c1, 0);

    let (c2, _, _) = env.run_agent(
        "agy-senior-dev",
        &["status", "Shipping Phase A CLI", "--ttl", "2h"],
    );
    assert_eq!(c2, 0);

    let (c3, who_json, _) = env.run_agent("agy-senior-dev", &["who", "agy-senior-dev", "--json"]);
    assert_eq!(c3, 0);
    assert!(who_json.contains("\"name\":\"Agy Senior Dev\""));
    assert!(who_json.contains("\"status\":\"Shipping Phase A CLI\""));

    let (c4, owner_json, _) = env.run_agent("agy-senior-dev", &["who", "owner", "--json"]);
    assert_eq!(c4, 0);
    assert!(owner_json.contains("\"owner\":true"));
}
