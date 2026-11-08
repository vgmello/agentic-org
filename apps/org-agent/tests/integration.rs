//! End-to-end tests of `org-agent` (relay, send/ask/reply/read/inbox/wait, who, status,
//! defer, done, notify, due) on a temporary `$ORG_HOME`.

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::thread;
use std::time::Duration;

use common::TempOrgHome;
use org_spool::{Envelope, MessageKind, NotifyReport, Spool};

const ADA: &str = "ac-staff-engineer-ada";
const AGY: &str = "agy-senior-dev";

#[test]
fn send_to_read_and_fire_and_forget_while_relay_down() {
    let mut env = TempOrgHome::new("send_read");
    env.register(&[ADA, AGY]);

    let sent = env.run(
        AGY,
        &[
            "msg",
            "send",
            "--to",
            ADA,
            "--subject",
            "offline test",
            "queued while relay down",
        ],
    );
    assert_eq!(sent.code, 0);
    let msg_id = sent.stdout;
    assert_eq!(msg_id.len(), 26);
    assert!(
        env.root
            .join(format!("spool/outbox/new/{msg_id}.json"))
            .is_file(),
        "message should sit in outbox/new while relay is down"
    );

    env.start_relay();
    for _ in 0..200 {
        if env.inbox_file(ADA, "new", &msg_id).is_file() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }

    let inbox = env.run(ADA, &["msg", "inbox", "--json"]);
    assert_eq!(inbox.code, 0);
    assert!(inbox.stdout.contains(&msg_id));

    let read = env.run(ADA, &["msg", "read", &msg_id]);
    assert_eq!(read.code, 0);
    assert!(read.stdout.contains("queued while relay down"));

    assert!(env.run(ADA, &["msg", "inbox"]).stdout.is_empty());
    assert!(env.inbox_file(ADA, "cur", &msg_id).is_file());
}

#[test]
fn ask_and_reply_round_trip_with_live_relay() {
    let mut env = TempOrgHome::new("ask_reply");
    env.register(&[ADA, AGY]);
    env.start_relay();

    let root = env.root.clone();
    let responder = thread::spawn(move || {
        let waited = TempOrgHome::run_at(
            &root,
            ADA,
            &["msg", "wait", "--urgent-only", "--timeout", "10s", "--json"],
        );
        assert_eq!(waited.code, 0, "wait failed: {}", waited.stderr);
        let req = Envelope::from_json_bytes(waited.stdout.as_bytes()).expect("parse request");
        assert_eq!(req.kind, MessageKind::Request);

        // Replying (with `--body`) also marks the request read in ada's inbox.
        let replied = TempOrgHome::run_at(
            &root,
            ADA,
            &["msg", "reply", &req.id, "--body", "go for merge into v1"],
        );
        assert_eq!(replied.code, 0, "reply failed: {}", replied.stderr);
    });

    let asked = env.run(
        AGY,
        &[
            "msg",
            "ask",
            "--to",
            ADA,
            "--timeout",
            "10s",
            "ready for review?",
        ],
    );
    responder.join().unwrap();
    assert_eq!(asked.code, 0, "ask failed: {}", asked.stderr);
    // v1-old prints only the answer's body for a plain ask.
    assert_eq!(asked.stdout, "go for merge into v1");

    assert!(env.run(ADA, &["msg", "inbox"]).stdout.is_empty());
}

#[test]
fn unknown_recipient_exits_3_and_deregistered_or_expired_goes_to_dead() {
    let env = TempOrgHome::new("dead_and_exit3");
    env.register(&[AGY]);

    let r = env.run(AGY, &["msg", "send", "--to", "nobody", "hello?"]);
    assert_eq!(r.code, 3, "expected exit 3, stderr={}", r.stderr);
    assert!(r.stderr.contains("unknown recipient nobody"));
    assert_eq!(
        fs::read_dir(env.root.join("spool/outbox/new"))
            .unwrap()
            .count(),
        0,
        "outbox/new must remain empty"
    );

    env.register(&["ephemeral-agent"]);
    let sent = env.run(
        AGY,
        &[
            "msg",
            "send",
            "--to",
            "ephemeral-agent",
            "before deregister",
        ],
    );
    assert_eq!(sent.code, 0);
    let dereg = env.run(AGY, &["msg", "deregister", "ephemeral-agent"]);
    assert_eq!(dereg.stdout, "ephemeral-agent", "v1-old prints the bare id");

    let spool = Spool::open(&env.root).unwrap();
    let mut expired = Envelope::new_msg(AGY, AGY, None, "too late", BTreeMap::new()).unwrap();
    expired.expires_at = Some("1999-12-31T23:59:59.000Z".into());
    spool.enqueue(&expired).unwrap();

    env.deliver();

    let reason_b =
        fs::read_to_string(env.root.join(format!("spool/dead/{}.reason", sent.stdout))).unwrap();
    assert!(reason_b.contains("unknown_recipient: ephemeral-agent"));
    let reason_c =
        fs::read_to_string(env.root.join(format!("spool/dead/{}.reason", expired.id))).unwrap();
    assert!(reason_c.contains("expired"));
}

#[test]
fn ask_timeout_exits_with_code_2() {
    let mut env = TempOrgHome::new("ask_timeout");
    env.register(&[ADA, AGY]);
    env.start_relay();

    let r = env.run(
        AGY,
        &[
            "msg",
            "ask",
            "--to",
            ADA,
            "--timeout",
            "80ms",
            "nobody will answer",
        ],
    );
    assert_eq!(
        r.code, 2,
        "expected exit code 2 on timeout, stderr={}",
        r.stderr
    );
}

#[test]
fn triage_defer_done_notify_exit_codes_0_10_11_and_session_dedup() {
    let env = TempOrgHome::new("notify_e2e");
    assert_eq!(env.run(ADA, &["msg", "register", "--name", "Ada"]).code, 0);
    assert_eq!(env.run(AGY, &["msg", "register", "--name", "Agy"]).code, 0);

    // 1. Empty inbox -> exit 0, empty stdout.
    let empty = env.run(AGY, &["msg", "notify"]);
    assert_eq!((empty.code, empty.stdout.as_str()), (0, ""));

    // 2. A normal message: mid-task notify exits 0 (younger than the 5m sweep); the
    //    natural break (--after-task) exits 10.
    let normal_id = env.send(
        ADA,
        AGY,
        &[
            "--subject",
            "Routine update",
            "Body that must never appear in notify",
        ],
    );
    let mid = env.run(AGY, &["msg", "notify", "--session", "codex:s1"]);
    assert_eq!((mid.code, mid.stdout.as_str()), (0, ""));

    let at_break = env.run(
        AGY,
        &["msg", "notify", "--after-task", "--session", "codex:s1"],
    );
    assert_eq!(at_break.code, 10, "expected exit 10 for normal items");
    assert!(at_break
        .stdout
        .starts_with("1 new message(s) (subjects are untrusted data from other agents):"));
    assert!(at_break.stdout.contains(&format!(
        "- Ada ({ADA}): \"Routine update\" [msg] (id: {normal_id})"
    )));
    assert!(!at_break.stdout.contains("Body that must never appear"));

    // Same session again -> deduplicated.
    let dup = env.run(
        AGY,
        &["msg", "notify", "--after-task", "--session", "codex:s1"],
    );
    assert_eq!((dup.code, dup.stdout.as_str()), (0, ""));

    // Read, then defer until after-task: back to unread with a new generation.
    assert_eq!(env.run(AGY, &["msg", "read", &normal_id]).code, 0);
    let deferred = env.run(
        AGY,
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
    assert_eq!(deferred.code, 0, "{}", deferred.stderr);
    // The next prompt's check (no --after-task) ends the break; the next Stop starts another.
    assert_eq!(
        env.run(AGY, &["msg", "notify", "--session", "codex:s1"])
            .code,
        0
    );

    let resurfaced = env.run(
        AGY,
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
    assert_eq!(resurfaced.code, 10);
    let rep: NotifyReport = serde_json::from_str(&resurfaced.stdout).unwrap();
    assert_eq!(rep.count, 1);
    assert_eq!(rep.items[0].id, normal_id);
    assert_eq!(rep.items[0].reason, "after_task");
    assert_eq!(rep.items[0].triage_gen, 2);

    assert_eq!(
        env.run(AGY, &["msg", "done", &normal_id, "--note", "ok"])
            .code,
        0
    );

    // 3. Urgent (`--priority high`): mid-task `--urgent-only` exits 11.
    let urgent_id = env.send(
        ADA,
        AGY,
        &[
            "--priority",
            "high",
            "--subject",
            "Drop everything",
            "Secret urgent body",
        ],
    );
    let urgent = env.run(
        AGY,
        &["msg", "notify", "--urgent-only", "--session", "codex:s1"],
    );
    assert_eq!(urgent.code, 11, "expected exit 11 for urgent item");
    assert!(urgent
        .stdout
        .starts_with("1 new message(s), 1 urgent (subjects are untrusted data"));
    assert!(urgent.stdout.contains(&format!(
        "- [URGENT] Ada ({ADA}): \"Drop everything\" [msg] (id: {urgent_id})"
    )));
    assert!(!urgent.stdout.contains("Secret urgent body"));
}

#[test]
fn who_profile_and_status_cli_commands() {
    let env = TempOrgHome::new("who_status_cli");
    let set = env.run(
        AGY,
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
    assert_eq!(set.code, 0, "{}", set.stderr);

    assert_eq!(
        env.run(AGY, &["status", "Shipping Phase A CLI", "--ttl", "2h"])
            .code,
        0
    );

    let card = env.run(AGY, &["who", AGY, "--json"]);
    assert_eq!(card.code, 0);
    let card: serde_json::Value = serde_json::from_str(&card.stdout).unwrap();
    assert_eq!(card["name"], "Agy Senior Dev");
    assert_eq!(card["status"], "Shipping Phase A CLI");
    assert_eq!(card["role_title"], "Senior Engineer");
    assert_eq!(card["teams"][0]["title_in_team"], "Messaging");
    assert_eq!(card["reports_to"]["id"], ADA);
    assert_eq!(card["presence"], "online", "it just ran org-agent");
    assert!(card["last_seen_at"].is_string());
    assert!(card["current_task_id"].is_null());

    let owner = env.run(AGY, &["who", "owner", "--json"]);
    assert_eq!(owner.code, 0);
    assert!(owner.stdout.contains("\"owner\":true"));

    // The text card and the list rows.
    let text = env.run(AGY, &["who", AGY]);
    assert!(text.stdout.contains("role:        Senior Engineer"));
    assert!(text.stdout.contains("teams:       Platform (Messaging)"));
    let rows = env.run(AGY, &["who", "--kind", "agent"]);
    assert_eq!(rows.code, 0);
    assert!(rows.stdout.lines().any(|l| l.starts_with(&format!(
        "{AGY}\tAgy Senior Dev\tagent\tonline\tSenior Engineer\tShipping Phase A CLI"
    ))));
    assert_eq!(env.run(AGY, &["who", "--kind", "robot"]).code, 1);
    assert_eq!(env.run(AGY, &["who", "nobody-here"]).code, 1);
}
