#![cfg(target_os = "macos")]
//! Take-over terminals: the child leads a session whose controlling terminal is the PTY.

mod common;

use std::io::Write;
use std::time::Duration;

use common::*;
use org_runtime::Stdio;
use org_supervise::spawn_session;

#[test]
fn the_child_gets_the_terminal_as_its_controlling_tty_and_sees_resizes() {
    let tmp = TempDir::new();
    let ws = tmp.dir("ws");
    let mark = mark("pty");
    let script = "tty; stty size; echo TERM=$TERM; echo via-dev-tty > /dev/tty; \
                  read x; stty size; exec /bin/sleep 30";
    let mut p = plan("/bin/sh", &["-c", script], &ws);
    p.stdio = Stdio::Pty { cols: 80, rows: 24 };
    let mut child =
        spawn_session(&p, &targets(&tmp.dir("rt")), &session_env(&tmp, &ws, &mark)).unwrap();
    let pid = child.pid();
    assert!(child.take_stdout().is_none() && child.take_stdin().is_none());
    let pty = child.take_pty().unwrap();
    assert_eq!(pty.size().unwrap(), (80, 24));
    let mut out = Reader::spawn(pty.master().try_clone().unwrap());

    let text = out.until("via-dev-tty");
    let tty = text.lines().next().unwrap().trim().to_owned();
    assert!(tty.starts_with("/dev/ttys"), "{text:?}");
    assert!(text.contains("24 80"), "{text:?}");
    assert!(text.contains("TERM=xterm-256color"), "{text:?}");

    // The session leader, with this terminal as its controlling tty (`ps` shows it by name).
    let stat = ps(pid, "stat");
    assert!(stat.contains('s'), "a session leader: {stat}");
    assert!(
        stat.contains('+'),
        "in the terminal's foreground group: {stat}"
    );
    assert_eq!(ps(pid, "pgid"), pid.to_string());
    assert_eq!(format!("/dev/tty{}", ps(pid, "tt")), tty);

    pty.resize(100, 30).unwrap();
    assert_eq!(pty.size().unwrap(), (100, 30));
    pty.master().write_all(b"go\n").unwrap();
    out.until("30 100");
    eventually("sleep runs", || ps(pid, "command") == "/bin/sleep 30");
    // ^C goes through the line discipline to the foreground group: the child.
    pty.master().write_all(b"\x03").unwrap();

    assert!(child.wait_exit(Some(Duration::from_secs(5))).unwrap());
    assert_eq!(child.reap().unwrap().info.signal, Some(libc::SIGINT));
    drop(pty);
    assert_clean(&[pid], Some(&mark));
}
