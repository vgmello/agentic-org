#![cfg(target_os = "macos")]
//! Process groups, the stop sequence, reaping, and orphan cleanup, with real processes.

mod common;

use std::io::Write;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use common::*;
use org_runtime::{SignalTarget, Stdio, StopPlan, StopSignal, StopStep};
use org_supervise::{orphans, spawn, spawn_session, Command, Env};

fn our_sess() -> String {
    ps(std::process::id(), "sess")
}

#[test]
fn a_piped_child_leads_its_own_group_in_our_session() {
    let tmp = TempDir::new();
    let ws = tmp.dir("ws");
    let mark = mark("grp");
    let env = session_env(&tmp, &ws, &mark);
    let sleep = third_party_sleep(&tmp);
    let child = spawn_session(
        &plan(sleep.to_str().unwrap(), &["30"], &ws),
        &targets(&tmp.dir("rt")),
        &env,
    )
    .unwrap();
    let pid = child.pid();
    assert_eq!(ps(pid, "pgid"), pid.to_string());
    assert_eq!(ps(pid, "sess"), our_sess(), "same session as ours");
    assert!(!ps(pid, "stat").contains('s'), "not a session leader");
    assert_eq!(ps(pid, "tt"), "??", "no controlling terminal");
    let id = child.id().unwrap();
    assert_eq!(orphans::process_id(pid).unwrap(), Some(id));
    assert_eq!(orphans::find_by_mark(&mark).unwrap(), vec![id]);
    drop(child); // kills and reaps
    assert_clean(&[pid], Some(&mark));
}

/// A double-forked, `setsid`'ed grandchild (a third-party binary, so its environment is
/// readable) escapes the group kill, and isn't a recorded tool session either: only the mark
/// scan in `reap` finds and kills it.
#[test]
fn a_group_kill_misses_a_detached_grandchild_and_the_mark_scan_gets_it() {
    let tmp = TempDir::new();
    let ws = tmp.dir("ws");
    let mark = mark("tool");
    let env = session_env(&tmp, &ws, &mark);
    let sleep = third_party_sleep(&tmp);
    let script = format!(
        "/usr/bin/perl -MPOSIX -e '$|=1; exit if fork; POSIX::setsid() or die; print \"$$\\n\"; exec @ARGV' {} 61; \
         /bin/sleep 62 & echo $!; wait",
        sleep.display()
    );
    let mut child = spawn_session(
        &plan("/bin/sh", &["-c", &script], &ws),
        &targets(&tmp.dir("rt")),
        &env,
    )
    .unwrap();
    let leader = child.pid();
    let mut out = Reader::spawn(child.take_stdout().unwrap());
    let pids: Vec<u32> = out.lines(2).iter().map(|l| l.parse().unwrap()).collect();
    let (tool, member) = (pids[0], pids[1]);
    eventually("the tool runs", || {
        ps(tool, "command").contains("tsleep 61")
    });
    assert!(
        ps(tool, "stat").contains('s'),
        "the tool leads its own session"
    );
    assert_eq!(ps(member, "pgid"), leader.to_string());

    child.kill();
    assert!(child.wait_exit(Some(Duration::from_secs(5))).unwrap());
    assert!(
        child.tool_sessions().is_empty(),
        "not a direct child: never recorded"
    );
    eventually("the group member died", || !listed(member));
    thread::sleep(Duration::from_millis(100));
    assert!(
        listed(tool),
        "the group kill must not reach the detached tool"
    );
    let marked: Vec<u32> = orphans::find_by_mark(&mark)
        .unwrap()
        .iter()
        .map(|p| p.pid)
        .collect();
    assert_eq!(marked, vec![tool]);

    let exit = child.reap().unwrap();
    assert_eq!(exit.info.signal, Some(libc::SIGKILL));
    let report = exit.orphans.unwrap().unwrap();
    assert!(report.signalled.iter().any(|p| p.pid == tool), "{report:?}");
    assert!(report.survivors.is_empty(), "{report:?}");
    assert_clean(&[leader, member, tool], Some(&mark));
}

/// Mirrors Claude's Bash tool exactly: a direct child of the runtime starts its own session
/// and runs an Apple binary, whose environment (and so the mark) macOS doesn't show. The
/// group kill misses it; the recorded tool session gets it.
#[test]
fn a_recorded_tool_session_is_killed_though_its_mark_is_unreadable() {
    let tmp = TempDir::new();
    let ws = tmp.dir("ws");
    let mark = mark("sess");
    let env = session_env(&tmp, &ws, &mark);
    let script = "/usr/bin/perl -MPOSIX -e 'POSIX::setsid() or die; exec q(/bin/sleep), q(64)' & \
                  echo $!; wait";
    let mut child = spawn_session(
        &plan("/bin/sh", &["-c", script], &ws),
        &targets(&tmp.dir("rt")),
        &env,
    )
    .unwrap();
    let leader = child.pid();
    let mut out = Reader::spawn(child.take_stdout().unwrap());
    let tool: u32 = out.lines(1)[0].parse().unwrap();
    eventually("the tool detached", || {
        ps(tool, "command") == "/bin/sleep 64"
    });
    assert!(
        ps(tool, "stat").contains('s'),
        "the tool leads its own session"
    );
    assert!(
        orphans::find_by_mark(&mark).unwrap().is_empty(),
        "an Apple binary's environment isn't readable"
    );

    // `kill` records the tool session before the group kill.
    child.kill();
    assert!(child.wait_exit(Some(Duration::from_secs(5))).unwrap());
    assert_eq!(
        child
            .tool_sessions()
            .iter()
            .map(|p| p.pid)
            .collect::<Vec<_>>(),
        vec![tool]
    );
    thread::sleep(Duration::from_millis(100));
    assert!(
        listed(tool),
        "the group kill must not reach the tool session"
    );

    let exit = child.reap().unwrap();
    let report = exit.orphans.unwrap().unwrap();
    assert!(report.signalled.iter().any(|p| p.pid == tool), "{report:?}");
    assert_clean(&[leader, tool], Some(&mark));
}

#[test]
fn closing_stdin_ends_a_child_that_waits_for_it() {
    let tmp = TempDir::new();
    let ws = tmp.dir("ws");
    let mark = mark("stdin");
    let env = session_env(&tmp, &ws, &mark);
    // Graceful steps come before any signal, and keep their full grace.
    let mut p = plan("/bin/sh", &["-c", "trap '' TERM; read x; exit 7"], &ws);
    p.stop = StopPlan {
        steps: vec![StopStep::CloseStdin { grace_ms: 5000 }],
    };
    let mut child = spawn_session(&p, &targets(&tmp.dir("rt")), &env).unwrap();
    let pid = child.pid();
    thread::sleep(Duration::from_millis(100)); // let the trap be set
    let started = Instant::now();
    child.stop(&p.stop).unwrap();
    assert!(started.elapsed() < Duration::from_secs(3));
    let exit = child.reap().unwrap();
    assert_eq!(exit.info.code, Some(7));
    assert_clean(&[pid], Some(&mark));
}

#[test]
fn the_pump_can_close_stdin_itself_and_the_child_exits() {
    let tmp = TempDir::new();
    let ws = tmp.dir("ws");
    let mark = mark("pump");
    let env = session_env(&tmp, &ws, &mark);
    let mut child = spawn_session(
        &plan("/bin/sh", &["-c", "read x; echo got:$x; exit 3"], &ws),
        &targets(&tmp.dir("rt")),
        &env,
    )
    .unwrap();
    let pid = child.pid();
    let mut stdin = child.take_stdin().unwrap();
    let mut out = Reader::spawn(child.take_stdout().unwrap());
    stdin.write_all(b"hi\n").unwrap();
    drop(stdin);
    assert!(child.wait_exit(Some(Duration::from_secs(5))).unwrap());
    assert_eq!(out.read_to_end(), "got:hi\n");
    assert_eq!(child.reap().unwrap().info.code, Some(3));
    assert_clean(&[pid], Some(&mark));
}

#[test]
fn stop_sends_sigterm_to_the_leader_first() {
    let tmp = TempDir::new();
    let ws = tmp.dir("ws");
    let mark = mark("term");
    let env = session_env(&tmp, &ws, &mark);
    let script = "trap 'exit 5' TERM; echo ready; while :; do /bin/sleep 0.05; done";
    let mut child = spawn_session(
        &plan("/bin/sh", &["-c", script], &ws),
        &targets(&tmp.dir("rt")),
        &env,
    )
    .unwrap();
    let pid = child.pid();
    // The trap is set once it says so.
    let mut out = Reader::spawn(child.take_stdout().unwrap());
    out.until("ready\n");
    // The plan's first signal is SIGINT to the group; the leader's SIGTERM comes first, with
    // that step's grace.
    let stop = StopPlan {
        steps: vec![StopStep::Signal {
            signal: StopSignal::Interrupt,
            target: SignalTarget::Group,
            grace_ms: 5000,
        }],
    };
    child.stop(&stop).unwrap();
    assert_eq!(child.reap().unwrap().info.code, Some(5));
    assert_clean(&[pid], Some(&mark));
}

#[test]
fn a_child_that_ignores_every_step_gets_sigkill_after_the_graces() {
    let tmp = TempDir::new();
    let ws = tmp.dir("ws");
    let mark = mark("kill");
    let env = session_env(&tmp, &ws, &mark);
    let script = "trap '' TERM INT; echo ready; while :; do /bin/sleep 0.05; done";
    let mut child = spawn_session(
        &plan("/bin/sh", &["-c", script], &ws),
        &targets(&tmp.dir("rt")),
        &env,
    )
    .unwrap();
    let pid = child.pid();
    // The trap is set once it says so.
    let mut out = Reader::spawn(child.take_stdout().unwrap());
    out.until("ready\n");
    let stop = StopPlan {
        steps: vec![
            StopStep::CloseStdin { grace_ms: 150 },
            StopStep::Signal {
                signal: StopSignal::Interrupt,
                target: SignalTarget::Leader,
                grace_ms: 150,
            },
            StopStep::Signal {
                signal: StopSignal::Terminate,
                target: SignalTarget::Group,
                grace_ms: 150,
            },
        ],
    };
    let started = Instant::now();
    child.stop(&stop).unwrap();
    let took = started.elapsed();
    assert!(
        took >= Duration::from_millis(600),
        "every grace was waited: {took:?}"
    );
    assert!(took < Duration::from_secs(3), "{took:?}");
    // `stop` doesn't reap: the child is a zombie until `reap`, so the pump can drain first.
    eventually("a zombie", || ps(pid, "stat").starts_with('Z'));
    assert_eq!(child.reap().unwrap().info.signal, Some(libc::SIGKILL));
    assert_clean(&[pid], Some(&mark));
}

#[test]
fn dropping_a_child_kills_and_reaps_it() {
    let env = Env::from_vars([(name("A"), "1".to_owned())]).unwrap();
    let child = spawn(&Command {
        program: PathBuf::from("/bin/sleep"),
        args: vec!["30".into()],
        cwd: PathBuf::from("/"),
        env,
        stdio: Stdio::Piped,
    })
    .unwrap();
    let pid = child.pid();
    assert!(listed(pid));
    drop(child);
    assert_clean(&[pid], None);
}

#[test]
fn kill_orphans_from_a_record_terminates_the_leader_then_its_group() {
    // The host's watchdog case: it isn't the parent, it has only the record.
    let tmp = TempDir::new();
    let ws = tmp.dir("ws");
    let mark = mark("host");
    let env = session_env(&tmp, &ws, &mark);
    let script = "trap '' TERM; /bin/sleep 63 & echo $!; wait";
    let mut child = spawn_session(
        &plan("/bin/sh", &["-c", script], &ws),
        &targets(&tmp.dir("rt")),
        &env,
    )
    .unwrap();
    let leader = child.id().unwrap();
    let mut out = Reader::spawn(child.take_stdout().unwrap());
    let member: u32 = out.until("\n").trim().parse().unwrap();
    let report = orphans::kill_orphans(&orphans::OrphanQuery {
        mark: &mark,
        group_leader: Some(leader),
        tool_sessions: &[],
        grace: Duration::from_millis(200),
    })
    .unwrap();
    assert!(report.leader_signalled);
    assert!(report.survivors.is_empty(), "{report:?}");
    assert!(child.wait_exit(Some(Duration::from_secs(5))).unwrap());
    assert_eq!(child.reap().unwrap().info.signal, Some(libc::SIGKILL));
    assert_clean(&[leader.pid, member], Some(&mark));
}

#[test]
fn a_stale_record_never_signals_a_stranger() {
    // A pid reused since the record: its start time doesn't match, so nothing happens.
    let env = Env::from_vars([(name("A"), "1".to_owned())]).unwrap();
    let mut child = spawn(&Command {
        program: PathBuf::from("/bin/sleep"),
        args: vec!["30".into()],
        cwd: PathBuf::from("/"),
        env,
        stdio: Stdio::Piped,
    })
    .unwrap();
    let mut id = child.id().unwrap();
    id.start_us -= 1;
    let report = orphans::kill_orphans(&orphans::OrphanQuery {
        mark: "osv-no-such-mark",
        group_leader: Some(id),
        tool_sessions: &[],
        grace: Duration::from_millis(50),
    })
    .unwrap();
    assert!(!report.leader_signalled);
    assert!(!child.has_exited().unwrap(), "the unmarked stranger lives");
    let pid = child.pid();
    drop(child);
    assert_clean(&[pid], None);
}

/// The watchdog case where the leader goes on `SIGTERM` (as Claude does): its group's
/// members, Apple binaries whose mark macOS doesn't show, still get `SIGKILL`.
#[test]
fn kill_orphans_from_a_record_kills_the_group_even_when_the_leader_exits_on_sigterm() {
    let tmp = TempDir::new();
    let ws = tmp.dir("ws");
    let mark = mark("host2");
    let env = session_env(&tmp, &ws, &mark);
    // The member starts with SIGTERM ignored; the leader takes it again before `ready`.
    let script = "trap '' TERM; /bin/sleep 67 & echo $!; trap - TERM; echo ready; wait";
    let mut child = spawn_session(
        &plan("/bin/sh", &["-c", script], &ws),
        &targets(&tmp.dir("rt")),
        &env,
    )
    .unwrap();
    let leader = child.id().unwrap();
    let mut out = Reader::spawn(child.take_stdout().unwrap());
    let member: u32 = out.lines(2)[0].trim().parse().unwrap();
    let report = orphans::kill_orphans(&orphans::OrphanQuery {
        mark: &mark,
        group_leader: Some(leader),
        tool_sessions: &[],
        grace: Duration::from_secs(2),
    })
    .unwrap();
    assert!(report.leader_signalled);
    assert!(child.wait_exit(Some(Duration::from_secs(5))).unwrap());
    assert_eq!(
        child.reap().unwrap().info.signal,
        Some(libc::SIGTERM),
        "it went on SIGTERM"
    );
    assert!(
        report.signalled.iter().any(|p| p.pid == member),
        "{report:?}"
    );
    assert!(report.survivors.is_empty(), "{report:?}");
    assert_clean(&[leader.pid, member], Some(&mark));
}

#[test]
fn a_plan_without_signal_steps_still_gets_sigterm_to_the_leader() {
    let tmp = TempDir::new();
    let ws = tmp.dir("ws");
    let mark = mark("empty");
    let script = "trap 'exit 9' TERM; echo ready; while :; do /bin/sleep 0.05; done";
    let mut child = spawn_session(
        &plan("/bin/sh", &["-c", script], &ws),
        &targets(&tmp.dir("rt")),
        &session_env(&tmp, &ws, &mark),
    )
    .unwrap();
    let pid = child.pid();
    // The trap is set once it says so.
    let mut out = Reader::spawn(child.take_stdout().unwrap());
    out.until("ready\n");
    let started = Instant::now();
    child.stop(&StopPlan { steps: Vec::new() }).unwrap();
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(child.reap().unwrap().info.code, Some(9));
    assert_clean(&[pid], Some(&mark));
}

#[test]
fn a_first_step_of_sigterm_to_the_leader_isnt_sent_twice() {
    let tmp = TempDir::new();
    let ws = tmp.dir("ws");
    let mark = mark("twice");
    // Counts the SIGTERMs it gets; SIGINT (the second step) then ends it.
    let script =
        "n=0; trap 'n=$((n+1)); echo term$n' TERM; echo ready; while :; do /bin/sleep 0.05; done";
    let mut child = spawn_session(
        &plan("/bin/sh", &["-c", script], &ws),
        &targets(&tmp.dir("rt")),
        &session_env(&tmp, &ws, &mark),
    )
    .unwrap();
    let pid = child.pid();
    let mut out = Reader::spawn(child.take_stdout().unwrap());
    out.until("ready\n");
    let stop = StopPlan {
        steps: vec![
            StopStep::Signal {
                signal: StopSignal::Terminate,
                target: SignalTarget::Leader,
                grace_ms: 300,
            },
            // To the group: bash exits on SIGINT only if its foreground child died of it.
            StopStep::Signal {
                signal: StopSignal::Interrupt,
                target: SignalTarget::Group,
                grace_ms: 2000,
            },
        ],
    };
    child.stop(&stop).unwrap();
    assert_eq!(child.reap().unwrap().info.signal, Some(libc::SIGINT));
    assert_eq!(out.read_to_end(), "ready\nterm1\n");
    assert_clean(&[pid], Some(&mark));
}

/// A tool session (an Apple binary: no readable mark) that survives SIGTERM and keeps forking
/// during the grace: nothing it forks escapes, and the report says so truthfully.
#[test]
fn a_tool_session_forking_during_the_grace_leaves_nothing_behind() {
    let tmp = TempDir::new();
    let ws = tmp.dir("ws");
    let mark = mark("fork");
    let tool = "trap true TERM; while :; do /bin/sleep 101 & wait; done";
    let script = format!(
        "/usr/bin/perl -MPOSIX -e 'POSIX::setsid() or die; exec q(/bin/sh), q(-c), q({tool})' & \\
         echo $!; wait"
    );
    let mut child = spawn_session(
        &plan("/bin/sh", &["-c", &script], &ws),
        &targets(&tmp.dir("rt")),
        &session_env(&tmp, &ws, &mark),
    )
    .unwrap();
    child.set_orphan_grace(Duration::from_millis(300));
    let leader = child.pid();
    let mut out = Reader::spawn(child.take_stdout().unwrap());
    let tool_pid: u32 = out.lines(1)[0].parse().unwrap();
    eventually("the tool forks", || sleeps_101() > 0);
    child.kill();
    let exit = child.reap().unwrap();
    let report = exit.orphans.unwrap().unwrap();
    assert!(report.survivors.is_empty(), "{report:?}");
    assert!(
        report.signalled.iter().any(|p| p.pid == tool_pid),
        "{report:?}"
    );
    eventually("no sleep 101 left", || sleeps_101() == 0);
    assert_clean(&[leader, tool_pid], Some(&mark));
}

/// How many `/bin/sleep 101` processes exist.
fn sleeps_101() -> usize {
    let out = std::process::Command::new("/bin/ps")
        .args(["-A", "-o", "command="])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.trim() == "/bin/sleep 101")
        .count()
}
