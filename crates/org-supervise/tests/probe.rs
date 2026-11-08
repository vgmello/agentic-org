//! Probes: stdin lines, a conversation, the timeout, the output cap, cwd `/` and the group
//! kill at the end.

mod common;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use common::*;
use org_runtime::{encode_line, Frame, ProbePlan};
use org_supervise::{
    run_probe, run_probe_with, EnvBuilder, ProbeConversation, ProbeReply, PROBE_OUTPUT_CAP,
};

fn probe(program: &str, args: &[&str], timeout_ms: u64) -> ProbePlan {
    ProbePlan {
        program: PathBuf::from(program),
        args: args.iter().map(|s| s.to_string()).collect(),
        env_add: Vec::new(),
        stdin_lines: Vec::new(),
        keep_stdin_open_until_exit: false,
        timeout_ms,
    }
}

#[test]
fn stdin_lines_are_written_then_stdin_is_closed() {
    let tmp = TempDir::new();
    let mut p = probe("/bin/cat", &[], 5000);
    p.stdin_lines = vec![encode_line("hello").unwrap(), encode_line("world").unwrap()];
    let out = run_probe(&p, &probe_env(&tmp)).unwrap();
    assert_eq!(out.stdout, b"\"hello\"\n\"world\"\n");
    assert_eq!(out.exit_code, Some(0));
    assert!(!out.timed_out);
}

#[test]
fn stdin_stays_open_when_asked_until_the_timeout() {
    let tmp = TempDir::new();
    let mut p = probe("/bin/cat", &[], 400);
    p.stdin_lines = vec![encode_line("ping").unwrap()];
    p.keep_stdin_open_until_exit = true;
    let started = Instant::now();
    let out = run_probe(&p, &probe_env(&tmp)).unwrap();
    assert!(out.timed_out, "cat never saw EOF");
    assert!(started.elapsed() >= Duration::from_millis(400));
    assert_eq!(out.stdout, b"\"ping\"\n");
    assert_eq!(out.exit_code, None);
}

#[test]
fn a_probe_past_its_timeout_is_killed_and_reported() {
    let tmp = TempDir::new();
    let started = Instant::now();
    let out = run_probe(&probe("/bin/sleep", &["30"], 300), &probe_env(&tmp)).unwrap();
    let took = started.elapsed();
    assert!(out.timed_out);
    assert_eq!(out.exit_code, None);
    assert!(
        took >= Duration::from_millis(300) && took < Duration::from_secs(3),
        "{took:?}"
    );
}

#[test]
fn output_is_capped_per_stream() {
    let tmp = TempDir::new();
    let script =
        "/usr/bin/head -c 3000000 /dev/zero; /usr/bin/head -c 2000000 /dev/zero >&2; exit 2";
    let out = run_probe(&probe("/bin/sh", &["-c", script], 10_000), &probe_env(&tmp)).unwrap();
    assert_eq!(out.stdout.len(), PROBE_OUTPUT_CAP);
    assert_eq!(out.stderr.len(), PROBE_OUTPUT_CAP);
    assert_eq!(
        out.exit_code,
        Some(2),
        "the probe ran to its end: the rest was drained"
    );
    assert!(!out.timed_out);
}

#[test]
fn a_probe_runs_in_root_with_the_office_env_plus_its_additions() {
    let tmp = TempDir::new();
    let mut p = probe(
        "/bin/sh",
        &["-c", "pwd; echo $X_PROBE; echo ${ORG_SESSION_TOKEN:-none}"],
        5000,
    );
    p.env_add = vec![(name("X_PROBE"), "on".into())];
    let out = run_probe(&p, &probe_env(&tmp)).unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "/\non\nnone\n");
}

#[test]
fn what_the_probe_leaves_in_its_group_is_killed() {
    let tmp = TempDir::new();
    let out = run_probe(
        &probe(
            "/bin/sh",
            &["-c", "/bin/sleep 30 >/dev/null 2>&1 & echo $!"],
            5000,
        ),
        &probe_env(&tmp),
    )
    .unwrap();
    assert_eq!(out.exit_code, Some(0));
    let left: u32 = String::from_utf8_lossy(&out.stdout).trim().parse().unwrap();
    assert_clean(&[left], None);
}

#[test]
fn a_marked_probe_has_its_detached_leftovers_killed_too() {
    let tmp = TempDir::new();
    let mark = mark("probe");
    let env = EnvBuilder::new(base_env(&tmp))
        .mark(mark.clone())
        .build()
        .unwrap();
    let sleep = third_party_sleep(&tmp);
    let script = format!(
        "/usr/bin/perl -MPOSIX -e '$|=1; exit if fork; POSIX::setsid(); open(STDOUT, \">/dev/null\"); exec @ARGV' {} 30; echo done",
        sleep.display()
    );
    let out = run_probe(&probe("/bin/sh", &["-c", &script], 5000), &env).unwrap();
    assert_eq!(out.stdout, b"done\n");
    assert_clean(&[], Some(&mark));
}

/// Answers each `qN` with `aN`, and is done at `end`.
struct Chat {
    seen: Vec<String>,
}

impl ProbeConversation for Chat {
    fn on_frame(&mut self, frame: &Frame) -> ProbeReply {
        let Frame::Line(line) = frame else {
            return ProbeReply::default();
        };
        let line = String::from_utf8_lossy(line).into_owned();
        self.seen.push(line.clone());
        match line.as_str() {
            "q1" => ProbeReply {
                write: vec![encode_line("a1").unwrap()],
                done: false,
            },
            "q2" => ProbeReply {
                write: vec![encode_line("a2").unwrap()],
                done: false,
            },
            l if l.starts_with("end") => ProbeReply {
                write: Vec::new(),
                done: true,
            },
            _ => ProbeReply::default(),
        }
    }
}

#[test]
fn a_conversation_drives_stdin_from_stdout() {
    let tmp = TempDir::new();
    // Ends only on EOF: the conversation being done closes stdin.
    let script = "echo q1; read a; echo q2; read b; echo end:$a:$b; cat >/dev/null; exit 6";
    let mut p = probe("/bin/sh", &["-c", script], 5000);
    p.keep_stdin_open_until_exit = true;
    let mut chat = Chat { seen: Vec::new() };
    let out = run_probe_with(&p, &probe_env(&tmp), &mut chat).unwrap();
    assert_eq!(chat.seen, ["q1", "q2", "end:\"a1\":\"a2\""]);
    assert_eq!(out.exit_code, Some(6));
    assert!(!out.timed_out);
}

#[test]
fn a_done_conversation_kills_a_probe_that_doesnt_exit() {
    let tmp = TempDir::new();
    let script = "trap '' TERM; echo end; while :; do /bin/sleep 0.05; done";
    let mut p = probe("/bin/sh", &["-c", script], 10_000);
    p.keep_stdin_open_until_exit = true;
    let started = Instant::now();
    let out = run_probe_with(&p, &probe_env(&tmp), &mut Chat { seen: Vec::new() }).unwrap();
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(!out.timed_out);
    assert_eq!(out.exit_code, None);
}

/// A child starts with default signal dispositions: this process ignores `SIGPIPE` (Rust's
/// default), and a child that inherited that would see `yes` fail loudly with `EPIPE` instead
/// of dying quietly of `SIGPIPE`.
#[test]
fn a_child_doesnt_inherit_an_ignored_sigpipe() {
    let tmp = TempDir::new();
    let script = "/usr/bin/yes | /usr/bin/head -c1 >/dev/null";
    let out = run_probe(&probe("/bin/sh", &["-c", script], 5000), &probe_env(&tmp)).unwrap();
    assert_eq!(out.exit_code, Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stderr), "", "yes saw EPIPE");
}

/// A probe that floods its stdout still stops at its timeout.
#[test]
fn a_flooding_probe_still_times_out() {
    let tmp = TempDir::new();
    let started = Instant::now();
    let out = run_probe(&probe("/usr/bin/yes", &[], 300), &probe_env(&tmp)).unwrap();
    assert!(out.timed_out);
    assert!(!out.stdout.is_empty() && out.stdout.len() <= PROBE_OUTPUT_CAP);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn a_huge_timeout_doesnt_overflow() {
    let tmp = TempDir::new();
    let out = run_probe(&probe("/usr/bin/true", &[], u64::MAX), &probe_env(&tmp)).unwrap();
    assert_eq!(out.exit_code, Some(0));
}
