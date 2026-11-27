//! Real exec inheritance and pending signals, without libtest's pre-existing threads.
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use org_host::signals::ShutdownSignals;
use org_platform::posix;

fn child(signal: i32) {
    // Retain an unrelated blocked/pending signal across the host's setup. This process has
    // one thread, so a signal it sends itself is pending for that thread.
    let unrelated = posix::SignalSet::of(&[libc::SIGUSR1]).unwrap();
    posix::block_signals(&unrelated).unwrap();
    posix::kill(std::process::id() as libc::pid_t, libc::SIGUSR1).unwrap();
    let signals = ShutdownSignals::block().unwrap();
    // Queue the shutdown signal before wait, as can happen while the host starts.
    let mask = posix::block_signals(&posix::SignalSet::empty().unwrap()).unwrap();
    for blocked in [libc::SIGUSR1, libc::SIGINT, libc::SIGTERM] {
        assert!(mask.contains(blocked));
    }
    for untouched in [libc::SIGHUP, libc::SIGPIPE, libc::SIGQUIT] {
        assert!(posix::is_ignored(untouched).unwrap());
    }
    posix::kill(std::process::id() as libc::pid_t, signal).unwrap();
    assert!(posix::pending_signals().unwrap().contains(libc::SIGUSR1));
    assert_eq!(signals.wait().unwrap(), signal);
    println!("consumed {signal}");
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|arg| arg == "--child") {
        child(args[2].parse().unwrap());
        return;
    }
    let mut failures = Vec::new();
    for ignored in [false, true] {
        for signal in [libc::SIGINT, libc::SIGTERM] {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args(["--child", &signal.to_string()])
                .stdout(Stdio::piped());
            // Only signal dispositions change, before exec.
            let shutdown = [libc::SIGINT, libc::SIGTERM];
            let untouched = [libc::SIGHUP, libc::SIGPIPE, libc::SIGQUIT];
            if ignored {
                let all: Vec<_> = shutdown.iter().chain(&untouched).copied().collect();
                posix::testing::with_dispositions(&mut command, &all, &[]);
            } else {
                posix::testing::with_dispositions(&mut command, &untouched, &shutdown);
            }
            let mut process = command.spawn().unwrap();
            let deadline = Instant::now() + Duration::from_secs(3);
            let status = loop {
                if let Some(status) = process.try_wait().unwrap() {
                    break Some(status);
                }
                if Instant::now() >= deadline {
                    process.kill().unwrap();
                    process.wait().unwrap();
                    break None;
                }
                thread::sleep(Duration::from_millis(10));
            };
            let output = process.wait_with_output().unwrap();
            if !status.is_some_and(|status| status.success())
                || String::from_utf8(output.stdout).unwrap().trim() != format!("consumed {signal}")
            {
                failures.push(format!("ignored={ignored}, signal={signal}: {status:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    println!("test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured");
}
