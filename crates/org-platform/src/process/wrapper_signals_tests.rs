//! The wrapper's signal guard: recorded, never fatal while held, not inherited by a child,
//! restored when dropped.

use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::*;

/// The guard is process-wide: these tests take turns.
static SERIAL: Mutex<()> = Mutex::new(());

fn disposition(signal: libc::c_int) -> libc::sighandler_t {
    // SAFETY: querying a disposition with a valid, zeroed output record; nothing changes.
    unsafe {
        let mut action = std::mem::zeroed::<libc::sigaction>();
        assert_eq!(libc::sigaction(signal, std::ptr::null(), &mut action), 0);
        action.sa_sigaction
    }
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn signals_are_recorded_while_held_and_the_old_dispositions_come_back() {
    let _turn = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let before: Vec<_> = [libc::SIGINT, libc::SIGQUIT, libc::SIGTERM, libc::SIGHUP]
        .into_iter()
        .map(disposition)
        .collect();
    let guard = WrapperSignals::install().unwrap();
    let second = WrapperSignals::install().unwrap_err();
    assert_eq!(second.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(guard.termination(), None);
    assert!(!guard.interrupted());

    // Ctrl-C reaches the wrapper too: recorded, and the process lives on.
    forward_signal(std::process::id(), libc::SIGINT).unwrap();
    wait_for("the interrupt", || guard.interrupted());
    assert_eq!(guard.termination(), None);
    // A termination request is recorded, the first one kept.
    forward_signal(std::process::id(), libc::SIGTERM).unwrap();
    wait_for("the termination", || guard.termination().is_some());
    forward_signal(std::process::id(), libc::SIGHUP).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(guard.termination(), Some(libc::SIGTERM));

    drop(guard);
    let after: Vec<_> = [libc::SIGINT, libc::SIGQUIT, libc::SIGTERM, libc::SIGHUP]
        .into_iter()
        .map(disposition)
        .collect();
    assert_eq!(before, after, "the replaced dispositions are restored");
    // Released: a new guard starts with nothing recorded.
    let again = WrapperSignals::install().unwrap();
    assert_eq!(again.termination(), None);
    assert!(!again.interrupted());
}

#[test]
fn a_child_started_while_held_keeps_the_default_ctrl_c_and_reports_like_a_shell() {
    let _turn = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let _guard = WrapperSignals::install().unwrap();
    let mut child = Command::new("sleep")
        .arg("30")
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    forward_signal(child.id(), libc::SIGINT).unwrap();
    let status = child.wait().unwrap();
    assert_eq!(shell_exit_code(&status), 130, "{status:?}");
    let mut child = Command::new("sleep").arg("30").spawn().unwrap();
    forward_signal(child.id(), SIGTERM).unwrap();
    assert_eq!(shell_exit_code(&child.wait().unwrap()), 143);
    let status = Command::new("sh").args(["-c", "exit 7"]).status().unwrap();
    assert_eq!(shell_exit_code(&status), 7);
    assert_eq!(signal_exit_code(SIGINT), 130);
}

#[test]
fn only_a_child_and_a_termination_or_interrupt_signal_are_forwarded() {
    for pid in [0, 1, u32::MAX] {
        let refused = forward_signal(pid, libc::SIGTERM).unwrap_err();
        assert_eq!(refused.kind(), io::ErrorKind::InvalidInput, "{pid}");
    }
    let refused = forward_signal(std::process::id(), libc::SIGSTOP).unwrap_err();
    assert_eq!(refused.kind(), io::ErrorKind::InvalidInput);
}
