mod core_limit;

use std::io::Read;
use std::process::ExitStatus;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use org_supervise::Spawned;

/// Bound both exit observation and pipe draining, even if a broken wait reports a stop
/// as exit or a descendant keeps a pipe open. Drop the owner before awaiting pipe EOF.
pub fn collect(mut spawned: Spawned, timeout: Duration) -> (ExitStatus, String, String) {
    let deadline = Instant::now() + timeout;
    spawned.stdin.close().unwrap();
    let (sender, receiver) = mpsc::channel();
    for (stderr, mut file) in [(false, spawned.stdout), (true, spawned.stderr)] {
        let sender = sender.clone();
        std::thread::spawn(move || {
            let mut text = String::new();
            let result = file.read_to_string(&mut text).map(|_| text);
            let _ = sender.send((stderr, result));
        });
    }
    drop(sender);
    let status = spawned.child.wait_until(deadline);
    drop(spawned.child); // kill/reap even on timeout/error or a falsely reported exit
    let mut stdout = String::new();
    let mut stderr = String::new();
    for _ in 0..2 {
        let (is_error, text) = receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("pipe drain deadline");
        if is_error {
            stderr = text.unwrap();
        } else {
            stdout = text.unwrap();
        }
    }
    (
        status.unwrap().expect("child exit deadline"),
        stdout,
        stderr,
    )
}
