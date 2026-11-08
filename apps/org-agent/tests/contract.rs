//! The CLI contract other code already depends on: v1-old output shapes, the locked
//! `msg notify` contract (flags, exit codes, no bodies), the exact invocations of the inbox
//! adapters (agents-codex `mods/inbox/bridge/adapter.mjs`), wake tags, and concurrency.

mod common;

use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::os::unix::net::UnixListener;
use std::process::Stdio;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use common::TempOrgHome;
use serde_json::Value;

const ME: &str = "agy-senior-dev";
const PEER: &str = "ac-staff-engineer-ada";
const SECRET: &str = "SECRET-BODY-must-never-be-printed";

fn json(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or_else(|e| panic!("not JSON ({e}): {s}"))
}

fn setup(tag: &str) -> TempOrgHome {
    let env = TempOrgHome::new(tag);
    env.register(&[ME, PEER]);
    env
}

#[test]
fn v1_old_output_shapes_are_unchanged() {
    let env = TempOrgHome::new("compat");
    let reg = env.run(ME, &["msg", "register"]);
    assert_eq!((reg.code, reg.stdout.as_str()), (0, ME));
    let reg_json = env.run(PEER, &["msg", "register", "--json"]);
    assert_eq!(json(&reg_json.stdout)["registered"], PEER);

    let who = env.run(ME, &["whoami", "--json"]);
    assert_eq!(json(&who.stdout), serde_json::json!({ "agent": ME }));
    assert_eq!(env.run(ME, &["whoami"]).stdout, ME);

    // Body from stdin with `-`; --json prints {"id":...}.
    let sent = env.run_stdin(
        PEER,
        &["msg", "send", "--to", ME, "--subject", "hi", "--json", "-"],
        "from stdin",
    );
    assert_eq!(sent.code, 0, "{}", sent.stderr);
    let id = json(&sent.stdout)["id"].as_str().unwrap().to_string();
    let no_subject = env.send(PEER, ME, &["no subject here"]);
    env.deliver();

    // inbox text: id \t kind \t from \t created_at \t subject ("-" when absent).
    let rows = env.run(ME, &["msg", "inbox"]).stdout;
    let lines: Vec<Vec<&str>> = rows.lines().map(|l| l.split('\t').collect()).collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(
        (lines[0][0], lines[0][1], lines[0][2]),
        (id.as_str(), "msg", PEER)
    );
    assert_eq!(lines[0][4], "hi");
    assert_eq!((lines[1][0], lines[1][4]), (no_subject.as_str(), "-"));

    // inbox --json: the v1-old envelope fields (meta always present) plus triage fields.
    let list = json(&env.run(ME, &["msg", "inbox", "--json"]).stdout);
    let first = &list[0];
    for key in [
        "v",
        "id",
        "from",
        "to",
        "kind",
        "reply_to",
        "created_at",
        "expires_at",
        "subject",
        "body",
        "meta",
    ] {
        assert!(
            first.get(key).is_some(),
            "inbox --json lost '{key}': {first}"
        );
    }
    assert_eq!(first["body"], "from stdin");
    assert_eq!(first["meta"], serde_json::json!({}));
    assert_eq!(first["status"], "unread");
    assert_eq!(first["priority"], "normal");

    // A subject with tabs/newlines can't break the one-row-per-message TSV.
    let tricky = env.send(PEER, ME, &["--subject", "a\tb\nfake-id\tline", "x"]);
    let rows = env.run(ME, &["msg", "inbox"]).stdout;
    assert_eq!(rows.lines().count(), 3, "{rows}");
    env.run(ME, &["msg", "done", &tricky]);

    // read --next --json on an empty inbox prints null and exits 0.
    env.run(ME, &["msg", "read", &id]);
    env.run(ME, &["msg", "read", &no_subject]);
    let next = env.run(ME, &["msg", "read", "--next", "--json"]);
    assert_eq!((next.code, next.stdout.as_str()), (0, "null"));
}

#[test]
fn notify_exit_codes_0_10_11_1_and_4() {
    let env = setup("exit_codes");
    let notify = |extra: &[&str]| {
        let mut args = vec!["msg", "notify"];
        args.extend_from_slice(extra);
        env.run(ME, &args)
    };

    assert_eq!(notify(&[]).code, 0);
    env.send(PEER, ME, &["--subject", "normal", SECRET]);
    assert_eq!(
        notify(&[]).code,
        0,
        "young normal message: nothing mid-task"
    );
    assert_eq!(notify(&["--after-task"]).code, 10);
    assert_eq!(
        notify(&["--sweep-after", "0s"]).code,
        10,
        "configurable sweep"
    );
    env.send(
        PEER,
        ME,
        &["--priority", "urgent", "--subject", "urgent", SECRET],
    );
    assert_eq!(notify(&[]).code, 11);
    assert_eq!(notify(&["--urgent-only"]).code, 11);

    // Usage errors exit 1.
    for bad in [
        &["--format", "yaml"][..],
        &["--sweep-after", "soon"],
        &["--session", ""],
        &["--session"],
        &["--bogus"],
    ] {
        let r = notify(bad);
        assert_eq!(r.code, 1, "{bad:?}: {}", r.stderr);
        assert!(r.stdout.is_empty());
    }
    let mut no_identity = TempOrgHome::command(&env.root, ME, &["msg", "notify"]);
    no_identity.env_remove("ORG_AGENT");
    assert_eq!(no_identity.output().unwrap().status.code(), Some(1));

    // An unusable spool is an I/O error: exit 4.
    let file_home = env.root.join("not-a-dir");
    fs::write(&file_home, b"x").unwrap();
    let io = TempOrgHome::run_at(&file_home, ME, &["msg", "notify"]);
    assert_eq!(io.code, 4, "{}", io.stderr);
}

#[test]
fn notify_never_prints_bodies_in_any_mode() {
    let env = setup("no_bodies");
    env.send(PEER, ME, &["--subject", "plain", SECRET]);
    env.send(PEER, ME, &["--priority", "high", SECRET]);
    let ask_body = format!("{SECRET}-ask");
    let mut ask = TempOrgHome::command(
        &env.root,
        PEER,
        &["msg", "ask", "--to", ME, "--timeout", "5s", &ask_body],
    )
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .unwrap();
    // Wait until the ask has been enqueued and delivered (it runs concurrently).
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        env.deliver();
        let rows = json(&env.run(ME, &["msg", "inbox", "--summary", "--json"]).stdout);
        if rows.as_array().unwrap().len() == 3 {
            break;
        }
        assert!(Instant::now() < deadline, "the ask never arrived");
        thread::sleep(Duration::from_millis(20));
    }

    for args in [
        &["msg", "notify"][..],
        &["msg", "notify", "--after-task", "--format", "json"],
        &[
            "msg",
            "notify",
            "--after-task",
            "--format",
            "text",
            "--session",
            "s",
        ],
        &["msg", "notify", "--urgent-only"],
        &["msg", "due", "--after-task"],
        &["msg", "inbox", "--summary", "--json"],
    ] {
        let r = env.run(ME, args);
        assert!(
            !r.stdout.contains(SECRET) && !r.stderr.contains(SECRET),
            "{args:?} leaked a body: {}",
            r.stdout
        );
    }
    let _ = ask.kill();
    let _ = ask.wait();
}

#[test]
fn adapter_invocations_behave_as_the_adapters_expect() {
    let env = setup("adapter");
    let session = "claude:2f0c1c1e-0000-4000-8000-000000000001";
    let notify = |extra: &[&str]| {
        let mut args = vec![
            "msg",
            "notify",
            "--session",
            session,
            "--format",
            "text",
            "--sweep-after",
            "5m",
        ];
        args.extend_from_slice(extra);
        env.run(ME, &args)
    };

    // adapter.mjs notify(): Stop with --after-task, watcher with --urgent-only.
    assert_eq!(notify(&["--after-task"]).code, 0);
    let normal = env.send(PEER, ME, &["--subject", "fyi", SECRET]);
    let busy = notify(&["--urgent-only"]);
    assert_eq!(
        (busy.code, busy.stdout.as_str()),
        (0, ""),
        "busy agents aren't interrupted"
    );
    let stop = notify(&["--after-task"]);
    assert_eq!(stop.code, 10);
    assert!(stop.stdout.contains(&normal));
    assert_eq!(notify(&["--after-task"]).code, 0, "shared generation dedup");

    // adapter.mjs Stop grace: `msg wait --timeout <grace>ms` (output discarded).
    let grace = env.run(ME, &["msg", "wait", "--timeout", "300ms"]);
    assert_eq!(grace.code, 0, "an unread message is present");
    env.run(ME, &["msg", "done", &normal]);
    let started = Instant::now();
    let grace = env.run(ME, &["msg", "wait", "--timeout", "300ms"]);
    assert_eq!(grace.code, 2);
    assert!(started.elapsed() < Duration::from_secs(5));

    // adapter.mjs watcher: `msg wait --urgent-only --timeout 30s` (shorter here): a normal
    // message doesn't end it, an urgent one does.
    env.send(PEER, ME, &["--subject", "still normal", "b"]);
    let w = env.run(ME, &["msg", "wait", "--urgent-only", "--timeout", "300ms"]);
    assert_eq!(w.code, 2, "normal mail must not wake a busy watcher");
    let root = env.root.clone();
    let watcher = thread::spawn(move || {
        TempOrgHome::run_at(
            &root,
            ME,
            &["msg", "wait", "--urgent-only", "--timeout", "30s"],
        )
    });
    thread::sleep(Duration::from_millis(200));
    let urgent = env.send(PEER, ME, &["--priority", "high", "--subject", "now", "b"]);
    let w = watcher.join().unwrap();
    assert_eq!(w.code, 0, "{}", w.stderr);
    assert!(w.stdout.contains(&urgent));
    let watch_notify = notify(&["--urgent-only"]);
    assert_eq!(watch_notify.code, 11);
    assert!(watch_notify.stdout.contains("[URGENT]"));

    // adapter.mjs: `msg inbox --summary --json` -> rows with string ids and NO body key.
    let rows = json(&env.run(ME, &["msg", "inbox", "--summary", "--json"]).stdout);
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    for row in rows {
        assert!(row["id"].is_string());
        assert!(
            row.get("body").is_none() && row.get("meta").is_none(),
            "{row}"
        );
    }

    // ORG_INBOX_SWEEP_AFTER can be any duration the CLI parses.
    for sweep in ["0", "300s", "5m", "1h", "250ms"] {
        let r = env.run(
            ME,
            &["msg", "notify", "--sweep-after", sweep, "--format", "text"],
        );
        assert!([0, 10, 11].contains(&r.code), "{sweep}: {}", r.stderr);
    }
}

#[test]
fn after_task_deferred_during_a_break_does_not_loop() {
    // Regression: Stop#1 reports M; the agent defers M after-task; Stop#2 reported M again
    // (after_task, exit 10), and so on forever.
    let env = setup("break_loop");
    let stop = || {
        env.run(
            ME,
            &[
                "msg",
                "notify",
                "--session",
                "claude:s1",
                "--after-task",
                "--format",
                "json",
            ],
        )
    };
    let m = env.send(PEER, ME, &["--subject", "later", "b"]);

    let stop1 = stop();
    assert_eq!(stop1.code, 10, "{}", stop1.stderr);
    assert_eq!(json(&stop1.stdout)["items"][0]["reason"], "new");
    let d = env.run(ME, &["msg", "defer", &m, "--until", "after-task"]);
    assert_eq!(d.code, 0, "{}", d.stderr);

    // The next Stop of the same break: nothing to report.
    let stop2 = stop();
    assert_eq!(stop2.code, 0, "{}", stop2.stdout);
    assert_eq!(json(&stop2.stdout)["count"], 0);

    // The Stop after a later task: re-surfaced, as after_task, exactly once.
    let stop3 = stop();
    assert_eq!(stop3.code, 10, "{}", stop3.stdout);
    let report = json(&stop3.stdout);
    assert_eq!(report["count"], 1);
    assert_eq!(report["items"][0]["id"], m.as_str());
    assert_eq!(report["items"][0]["reason"], "after_task");
    assert_eq!(stop().code, 0, "reported once");

    // `msg due --after-task` (the stateless alias) follows the same rule.
    let due = || json(&env.run(ME, &["msg", "due", "--after-task"]).stdout);
    let n = env.send(PEER, ME, &["--subject", "another", "b"]);
    env.run(ME, &["msg", "done", &m]);
    assert_eq!(due()["items"][0]["id"], n.as_str());
    assert_eq!(
        env.run(ME, &["msg", "defer", &n, "--until", "after-task"])
            .code,
        0
    );
    assert_eq!(due()["count"], 0, "same break");
    let later = due();
    assert_eq!(later["items"][0]["id"], n.as_str(), "{later}");
    assert_eq!(later["items"][0]["reason"], "after_task");
}

#[test]
fn defer_done_due_and_snooze_aware_inbox() {
    let env = setup("triage_cli");
    let a = env.send(PEER, ME, &["--subject", "a", "body a"]);
    let b = env.send(PEER, ME, &["--subject", "b", "body b"]);
    let reply_to_a = env.run(ME, &["msg", "reply", &a, "--body", "re a"]);
    assert_eq!(reply_to_a.code, 0);
    env.deliver();

    let d = env.run(
        ME,
        &[
            "msg",
            "defer",
            &b,
            "--until",
            "2h",
            "--note",
            "after the PR",
            "--json",
        ],
    );
    assert_eq!(d.code, 0, "{}", d.stderr);
    let d = json(&d.stdout);
    assert_eq!(d["status"], "snoozed");
    assert_eq!(d["snooze_note"], "after the PR");

    // Hidden by default, shown with --all (with due time and note).
    assert!(!env.run(ME, &["msg", "inbox"]).stdout.contains(&b));
    let all = json(&env.run(ME, &["msg", "inbox", "--all", "--json"]).stdout);
    let row = all
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == b.as_str())
        .unwrap()
        .clone();
    assert_eq!(row["status"], "snoozed");
    assert_eq!(row["snooze_note"], "after the PR");
    assert!(row["snoozed_until"].as_str().unwrap().ends_with('Z'));

    // due: stateless JSON, exit 0, nothing due yet.
    let due = env.run(ME, &["msg", "due"]);
    assert_eq!(due.code, 0);
    assert_eq!(json(&due.stdout)["count"], 0);
    assert_eq!(env.run(ME, &["msg", "due", "--session", "x"]).code, 1);
    assert_eq!(env.run(ME, &["msg", "due", "--format", "text"]).code, 1);

    // Re-defer to the past -> due now, reason "due"; `due` reports it every time (stateless).
    env.run(ME, &["msg", "defer", &b, "--until", "2000-01-01T00:00:00Z"]);
    for _ in 0..2 {
        let due = json(&env.run(ME, &["msg", "due", "--json"]).stdout);
        assert_eq!(due["items"][0]["id"], b.as_str());
        assert_eq!(due["items"][0]["reason"], "due");
    }

    // --thread: the message and replies to it (in my inbox, my own reply isn't).
    let thread_rows = env.run(PEER, &["msg", "inbox", "--thread", &a]).stdout;
    assert_eq!(thread_rows.lines().count(), 1, "{thread_rows}");
    assert_eq!(env.run(ME, &["msg", "inbox", "--cursor", "5"]).code, 1);

    // done: never resurfaces, even at the break.
    let done = env.run(ME, &["msg", "done", &b, "--note", "handled", "--json"]);
    assert_eq!(json(&done.stdout)["status"], "done");
    let r = env.run(ME, &["msg", "notify", "--after-task", "--sweep-after", "0"]);
    assert!(!r.stdout.contains(&b));

    // Usage errors.
    for bad in [
        &["msg", "defer", &b][..],
        &["msg", "defer", &b, "--until", "whenever"],
        &["msg", "defer", "not-a-ulid", "--until", "30m"],
        &["msg", "done"],
    ] {
        assert_eq!(env.run(ME, bad).code, 1, "{bad:?}");
    }
    // Far-future or overflowing snoozes are rejected rather than coming due at once.
    let c = env.send(PEER, ME, &["--subject", "c", "body c"]);
    for until in ["3000000d", "99999999999999999h"] {
        assert_eq!(
            env.run(ME, &["msg", "defer", &c, "--until", until]).code,
            1,
            "{until}"
        );
    }
    assert!(env.run(ME, &["msg", "inbox"]).stdout.contains(&c));
    let unknown = org_spool::generate_ulid();
    assert_eq!(env.run(ME, &["msg", "done", &unknown]).code, 1);
}

#[test]
fn priority_flag_and_wake_tags_u_and_n() {
    let env = setup("wake_tags");
    let wake_dir = env.root.join(format!("spool/inbox/{ME}/wake"));
    let listener = UnixListener::bind(wake_dir.join("1-tagprobe.sock")).unwrap();
    listener.set_nonblocking(false).unwrap();
    let tag = |args: &[&str]| {
        env.send(PEER, ME, args);
        let (mut stream, _) = listener.accept().unwrap();
        let mut b = [0u8; 1];
        stream.read_exact(&mut b).unwrap();
        b[0]
    };
    assert_eq!(tag(&["plain"]), b'N');
    assert_eq!(tag(&["--priority", "normal", "x"]), b'N');
    assert_eq!(tag(&["--priority", "high", "x"]), b'U');
    assert_eq!(tag(&["--priority", "urgent", "x"]), b'U');
    assert_eq!(
        env.run(PEER, &["msg", "send", "--to", ME, "--priority", "low", "x"])
            .code,
        1
    );
    assert_eq!(
        env.run(
            PEER,
            &["msg", "send", "--to", ME, "--meta", "priority=low", "x"]
        )
        .code,
        1
    );

    let rows = json(&env.run(ME, &["msg", "inbox", "--json"]).stdout);
    let priorities: Vec<&str> = rows
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["priority"].as_str().unwrap())
        .collect();
    assert_eq!(priorities, ["normal", "normal", "high", "urgent"]);
}

#[test]
fn status_set_show_and_clear() {
    let env = setup("status_cli");
    let set = env.run(ME, &["status", "reviewing", "PR", "9", "--json"]);
    assert_eq!(json(&set.stdout)["status"], "reviewing PR 9");
    // Others read it from the directory card.
    let card = env.run(PEER, &["who", ME, "--json"]);
    assert_eq!(json(&card.stdout)["status"], "reviewing PR 9");
    assert_eq!(
        env.run(ME, &["status"]).stdout,
        format!("{ME}: reviewing PR 9")
    );
    // No sub-keywords: every word is status text.
    env.run(ME, &["status", "show"]);
    assert_eq!(env.run(ME, &["status"]).stdout, format!("{ME}: show"));
    assert_eq!(env.run(ME, &["status", "--clear"]).code, 0);
    assert_eq!(
        env.run(ME, &["status"]).stdout,
        format!("{ME}: (no status)")
    );
    assert_eq!(env.run(ME, &["status", "two\nlines"]).code, 1);
    assert_eq!(env.run(ME, &["status", "x", "--ttl", "25h"]).code, 1);
}

#[test]
fn concurrent_writers_lose_nothing() {
    let env = Arc::new(setup("two_writers"));
    const PER_WRITER: usize = 15;

    // Two senders at once, then the relay: every message is delivered exactly once.
    let gate = Arc::new(Barrier::new(2));
    let senders: Vec<_> = (0..2)
        .map(|w| {
            let (env, gate) = (Arc::clone(&env), Arc::clone(&gate));
            thread::spawn(move || {
                gate.wait();
                (0..PER_WRITER)
                    .map(|i| {
                        let r = env.run(
                            PEER,
                            &[
                                "msg",
                                "send",
                                "--to",
                                ME,
                                "--subject",
                                &format!("w{w}-{i}"),
                                "b",
                            ],
                        );
                        assert_eq!(r.code, 0, "{}", r.stderr);
                        r.stdout
                    })
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    let ids: Vec<String> = senders
        .into_iter()
        .flat_map(|h| h.join().unwrap())
        .collect();
    env.deliver();
    let unread = json(
        &env.run(
            ME,
            &["msg", "inbox", "--summary", "--limit", "100", "--json"],
        )
        .stdout,
    );
    let got: BTreeSet<String> = unread
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(got, ids.iter().cloned().collect::<BTreeSet<_>>());

    // Two triage writers at once on the same triage.json: one defers, one marks done.
    let gate = Arc::new(Barrier::new(2));
    let (defer_ids, done_ids) = ids.split_at(PER_WRITER);
    let writers: Vec<_> = [(defer_ids.to_vec(), true), (done_ids.to_vec(), false)]
        .into_iter()
        .map(|(batch, defer)| {
            let (env, gate) = (Arc::clone(&env), Arc::clone(&gate));
            thread::spawn(move || {
                gate.wait();
                for id in batch {
                    let args: Vec<&str> = if defer {
                        vec!["msg", "defer", &id, "--until", "1h"]
                    } else {
                        vec!["msg", "done", &id]
                    };
                    let r = env.run(ME, &args);
                    assert_eq!(r.code, 0, "{}", r.stderr);
                }
            })
        })
        .collect();
    writers.into_iter().for_each(|h| h.join().unwrap());

    let all = json(
        &env.run(
            ME,
            &[
                "msg",
                "inbox",
                "--all",
                "--summary",
                "--limit",
                "100",
                "--json",
            ],
        )
        .stdout,
    );
    let count = |status: &str| {
        all.as_array()
            .unwrap()
            .iter()
            .filter(|r| r["status"] == status)
            .count()
    };
    assert_eq!(count("snoozed"), PER_WRITER, "every concurrent defer kept");
    assert_eq!(count("done"), PER_WRITER, "every concurrent done kept");
}

#[test]
fn defer_racing_notify_never_loses_a_snooze() {
    let env = Arc::new(setup("defer_vs_notify"));
    let ids: Vec<String> = (0..12)
        .map(|i| {
            let r = env.run(
                PEER,
                &[
                    "msg",
                    "send",
                    "--to",
                    ME,
                    "--priority",
                    "urgent",
                    &format!("m{i}"),
                ],
            );
            r.stdout
        })
        .collect();
    env.deliver();

    let gate = Arc::new(Barrier::new(3));
    let deferrer = {
        let (env, gate, ids) = (Arc::clone(&env), Arc::clone(&gate), ids.clone());
        thread::spawn(move || {
            gate.wait();
            for id in &ids {
                let r = env.run(ME, &["msg", "defer", id, "--until", "1h"]);
                assert_eq!(r.code, 0, "{}", r.stderr);
            }
        })
    };
    let notifiers: Vec<_> = (0..2)
        .map(|n| {
            let (env, gate) = (Arc::clone(&env), Arc::clone(&gate));
            thread::spawn(move || {
                gate.wait();
                for _ in 0..8 {
                    let session = format!("racer-{n}");
                    let r = env.run(
                        ME,
                        &["msg", "notify", "--after-task", "--session", &session],
                    );
                    assert!([0, 10, 11].contains(&r.code), "{}", r.stderr);
                }
            })
        })
        .collect();
    deferrer.join().unwrap();
    notifiers.into_iter().for_each(|h| h.join().unwrap());

    // Every defer survived the concurrent notify writes: nothing is reportable now.
    let fresh = env.run(
        ME,
        &[
            "msg",
            "notify",
            "--after-task",
            "--session",
            "fresh",
            "--format",
            "json",
        ],
    );
    assert_eq!(fresh.code, 0, "{}", fresh.stdout);
    let all = json(
        &env.run(ME, &["msg", "inbox", "--all", "--summary", "--json"])
            .stdout,
    );
    assert!(all
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["status"] == "snoozed"));
    assert_eq!(all.as_array().unwrap().len(), ids.len());
}

#[test]
fn deregister_requires_an_agent_and_never_falls_back_to_the_caller() {
    let env = setup("deregister");
    let id = env.send(PEER, ME, &["keep me"]);
    for args in [
        &["msg", "deregister"][..],
        &["msg", "deregister", "--json"],
        &["msg", "deregister", PEER, ME],
    ] {
        let r = env.run(ME, args);
        assert_eq!(r.code, 1, "{args:?}: {}", r.stderr);
        assert!(r.stdout.is_empty(), "{args:?}: {}", r.stdout);
    }
    assert!(
        env.inbox_file(ME, "new", &id).is_file(),
        "the caller's inbox survives"
    );
    assert!(env.root.join(format!("spool/inbox/{PEER}")).is_dir());
}

#[test]
fn an_invalid_recipient_id_is_an_unknown_recipient_like_v1_old() {
    let env = setup("bad_to");
    for to in ["bad.name", "../etc", "has space"] {
        let r = env.run(ME, &["msg", "send", "--to", to, "hi"]);
        assert_eq!(r.code, 3, "{to}: {}", r.stderr);
        let r = env.run(ME, &["msg", "ask", "--to", to, "--timeout", "50ms", "hi"]);
        assert_eq!(r.code, 3, "{to}: {}", r.stderr);
    }
}

#[test]
fn profiles_are_edited_by_their_member_or_the_owner() {
    let env = setup("profile_auth");
    let other = env.run(ME, &["profile", "set", PEER, "--name", "Owner (vgmello)"]);
    assert_eq!(other.code, 1, "{}", other.stderr);
    let reg = env.run(ME, &["msg", "register", PEER, "--name", "x"]);
    assert_eq!(reg.code, 1);
    assert_eq!(
        env.run(ME, &["msg", "register", PEER]).code,
        0,
        "plain register as in v1-old"
    );
    assert_eq!(env.run(ME, &["profile", "set", "--name", "Agy"]).code, 0);
    assert_eq!(
        env.run(ME, &["profile", "set", ME, "--role", "Dev"]).code,
        0
    );
    let by_owner = env.run("vgmello", &["profile", "set", PEER, "--name", "Ada"]);
    assert_eq!(by_owner.code, 0, "{}", by_owner.stderr);
    assert_eq!(
        json(&env.run(ME, &["who", PEER, "--json"]).stdout)["name"],
        "Ada"
    );
    assert_eq!(
        env.run(ME, &["profile", "set", "--owner"]).code,
        1,
        "only vgmello is the owner"
    );
}

#[test]
fn inbox_text_strips_bidi_characters_from_subjects() {
    let env = setup("inbox_bidi");
    env.send(PEER, ME, &["--subject", "ok\u{202E}evil\u{200B}", "x"]);
    let rows = env.run(ME, &["msg", "inbox"]).stdout;
    assert!(
        !rows.contains('\u{202E}') && !rows.contains('\u{200B}'),
        "{rows}"
    );
}

#[test]
fn any_msg_command_marks_the_caller_online_in_who() {
    let env = setup("presence_cli");
    // Registered on its behalf by someone else: never seen yet.
    const FRESH: &str = "fresh-agent";
    assert_eq!(env.run(PEER, &["msg", "register", FRESH]).code, 0);
    let before = json(&env.run(PEER, &["who", FRESH, "--json"]).stdout);
    assert_eq!(before["presence"], "offline");
    assert!(before["last_seen_at"].is_null());
    assert_eq!(env.run(FRESH, &["msg", "inbox"]).code, 0);
    let after = json(&env.run(PEER, &["who", FRESH, "--json"]).stdout);
    assert_eq!(after["presence"], "online");
    assert!(after["last_seen_at"].is_string());
    // An unregistered --as never gets an inbox just by running commands.
    env.run("nobody-yet", &["msg", "inbox"]);
    assert!(!env.root.join("spool/inbox/nobody-yet").exists());
}
