#![cfg(target_os = "macos")]
//! Probes: one-shot (stdin lines, the timeout, the output cap, cwd `/`, the group kill) and
//! conversations driven by a `ProbeSession` (lines, ticks, `Done`, stdout closing, timeout).

mod common;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use common::*;
use org_runtime::{
    encode_line, ProbeOutput, ProbePlan, ProbeResult, ProbeRun, ProbeSession, ProbeStep,
    Unavailable,
};
use org_supervise::{run_probe, Env, EnvBuilder, ProbeOutcome, PROBE_OUTPUT_CAP};

fn probe(program: &str, args: &[&str], timeout_ms: u64) -> ProbePlan {
    ProbePlan {
        program: PathBuf::from(program),
        args: args.iter().map(|s| s.to_string()).collect(),
        env_add: Vec::new(),
        stdin_lines: Vec::new(),
        timeout_ms,
    }
}

fn one_shot(plan: ProbePlan, env: &Env) -> ProbeOutput {
    match run_probe(ProbeRun::OneShot(plan), env).unwrap() {
        ProbeOutcome::OneShot(out) => out,
        other => panic!("{other:?}"),
    }
}

fn converse(plan: ProbePlan, session: impl ProbeSession + 'static, env: &Env) -> ProbeResult {
    let run = ProbeRun::Conversation {
        plan,
        session: Box::new(session),
    };
    match run_probe(run, env).unwrap() {
        ProbeOutcome::Conversation { result, .. } => result,
        other => panic!("{other:?}"),
    }
}

fn other(detail: &str) -> ProbeResult {
    ProbeResult::Unavailable {
        reason: Unavailable::Other {
            detail: detail.to_owned(),
        },
    }
}

#[test]
fn stdin_lines_are_written_then_stdin_is_closed() {
    let tmp = TempDir::new();
    let mut p = probe("/bin/cat", &[], 5000);
    p.stdin_lines = vec![encode_line("hello").unwrap(), encode_line("world").unwrap()];
    let out = one_shot(p, &probe_env(&tmp));
    assert_eq!(out.stdout, b"\"hello\"\n\"world\"\n");
    assert_eq!(out.exit_code, Some(0));
    assert!(!out.timed_out);
}

#[test]
fn a_probe_past_its_timeout_is_killed_and_reported() {
    let tmp = TempDir::new();
    let started = Instant::now();
    let out = one_shot(probe("/bin/sleep", &["30"], 300), &probe_env(&tmp));
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
    let out = one_shot(probe("/bin/sh", &["-c", script], 10_000), &probe_env(&tmp));
    assert_eq!(out.stdout.len(), PROBE_OUTPUT_CAP);
    assert_eq!(out.stderr.len(), PROBE_OUTPUT_CAP);
    assert_eq!(
        out.exit_code,
        Some(2),
        "the probe ran to its end: the rest was drained"
    );
    assert!(!out.timed_out);
}

/// A probe that floods its stdout still stops at its timeout.
#[test]
fn a_flooding_probe_still_times_out() {
    let tmp = TempDir::new();
    let started = Instant::now();
    let out = one_shot(probe("/usr/bin/yes", &[], 300), &probe_env(&tmp));
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
    let out = one_shot(probe("/usr/bin/true", &[], u64::MAX), &probe_env(&tmp));
    assert_eq!(out.exit_code, Some(0));
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
    let out = one_shot(p, &probe_env(&tmp));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "/\non\nnone\n");
}

/// A child starts with default signal dispositions: this process ignores `SIGPIPE` (Rust's
/// default), and a child that inherited that would see `yes` fail loudly with `EPIPE` instead
/// of dying quietly of `SIGPIPE`.
#[test]
fn a_child_doesnt_inherit_an_ignored_sigpipe() {
    let tmp = TempDir::new();
    let script = "/usr/bin/yes | /usr/bin/head -c1 >/dev/null";
    let out = one_shot(probe("/bin/sh", &["-c", script], 5000), &probe_env(&tmp));
    assert_eq!(out.exit_code, Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stderr), "", "yes saw EPIPE");
}

#[test]
fn what_the_probe_leaves_in_its_group_is_killed() {
    let tmp = TempDir::new();
    let out = one_shot(
        probe(
            "/bin/sh",
            &["-c", "/bin/sleep 30 >/dev/null 2>&1 & echo $!"],
            5000,
        ),
        &probe_env(&tmp),
    );
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
    let out = one_shot(probe("/bin/sh", &["-c", &script], 5000), &env);
    assert_eq!(out.stdout, b"done\n");
    assert_clean(&[], Some(&mark));
}

/// Answers `q1` with `a1` and `q2` with `a2`; `end...` is its result. If stdout closes first
/// it concludes "closed"; it may also finish on a tick.
struct Chat {
    seen: Vec<String>,
    tick_at: Option<Instant>,
}

impl Chat {
    fn new() -> Chat {
        Chat {
            seen: Vec::new(),
            tick_at: None,
        }
    }
}

impl ProbeSession for Chat {
    fn start(&mut self, _now: Instant) -> Vec<ProbeStep> {
        Vec::new()
    }

    fn on_line(&mut self, line: &[u8], _now: Instant) -> Vec<ProbeStep> {
        let line = String::from_utf8_lossy(line).into_owned();
        self.seen.push(line.clone());
        match line.as_str() {
            "q1" => vec![ProbeStep::Write(encode_line("a1").unwrap())],
            "q2" => vec![ProbeStep::Write(encode_line("a2").unwrap())],
            l if l.starts_with("end") => vec![ProbeStep::Done(ProbeResult::Version {
                raw: l.to_owned(),
                in_tested_range: None,
            })],
            _ => Vec::new(),
        }
    }

    fn on_tick(&mut self, _now: Instant) -> Vec<ProbeStep> {
        self.tick_at = None;
        vec![ProbeStep::Done(ProbeResult::Auth {
            signed_in: true,
            method: Some("tick".into()),
        })]
    }

    fn on_stdout_closed(&mut self, _now: Instant) -> ProbeResult {
        other("closed")
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.tick_at
    }
}

#[test]
fn a_conversation_drives_stdin_from_stdout_until_done() {
    let tmp = TempDir::new();
    // Ends only on EOF: stdin is closed once the session is done.
    let script = "echo q1; read a; echo q2; read b; echo end:$a:$b; cat >/dev/null; exit 6";
    let result = converse(
        probe("/bin/sh", &["-c", script], 5000),
        Chat::new(),
        &probe_env(&tmp),
    );
    assert_eq!(
        result,
        ProbeResult::Version {
            raw: "end:\"a1\":\"a2\"".into(),
            in_tested_range: None,
        }
    );
}

#[test]
fn a_done_conversation_stops_a_probe_that_doesnt_exit_and_isnt_timed_out() {
    let tmp = TempDir::new();
    let script = "trap '' TERM; echo end; while :; do /bin/sleep 0.05; done";
    let started = Instant::now();
    let result = converse(
        probe("/bin/sh", &["-c", script], 10_000),
        Chat::new(),
        &probe_env(&tmp),
    );
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(matches!(result, ProbeResult::Version { .. }), "{result:?}");
}

#[test]
fn a_conversation_concludes_when_stdout_closes_first() {
    let tmp = TempDir::new();
    let result = converse(
        probe("/bin/sh", &["-c", "echo hi"], 5000),
        Chat::new(),
        &probe_env(&tmp),
    );
    assert_eq!(result, other("closed"));
}

#[test]
fn a_conversation_without_done_times_out() {
    let tmp = TempDir::new();
    // cat waits for EOF, which never comes: stdin stays open for the conversation.
    let started = Instant::now();
    let result = converse(probe("/bin/cat", &[], 300), Chat::new(), &probe_env(&tmp));
    assert_eq!(result, other("timed out"));
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn a_conversation_gets_its_ticks() {
    let tmp = TempDir::new();
    let mut chat = Chat::new();
    chat.tick_at = Some(Instant::now() + Duration::from_millis(150));
    let started = Instant::now();
    let result = converse(probe("/bin/sleep", &["30"], 10_000), chat, &probe_env(&tmp));
    assert_eq!(
        result,
        ProbeResult::Auth {
            signed_in: true,
            method: Some("tick".into())
        }
    );
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn a_conversation_ends_when_stdout_closes_before_the_exit() {
    let tmp = TempDir::new();
    let started = Instant::now();
    let result = converse(
        probe(
            "/bin/sh",
            &["-c", "echo hi; exec 1>&-; /bin/sleep 30"],
            5000,
        ),
        Chat::new(),
        &probe_env(&tmp),
    );
    assert_eq!(result, other("closed"));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
}
