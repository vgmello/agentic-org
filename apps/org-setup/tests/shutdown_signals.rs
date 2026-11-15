//! Real exec inheritance and pending signals, without libtest's pre-existing threads.
use std::io;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use org_office_host::signals::ShutdownSignals;

fn child(signal: i32) {
    // Retain an unrelated blocked/pending signal across the host's setup.
    // SAFETY: valid initialized sets and signal numbers; this process has one thread.
    unsafe {
        let mut unrelated = std::mem::zeroed();
        assert_eq!(libc::sigemptyset(&mut unrelated), 0);
        assert_eq!(libc::sigaddset(&mut unrelated, libc::SIGUSR1), 0);
        assert_eq!(
            libc::pthread_sigmask(libc::SIG_BLOCK, &unrelated, std::ptr::null_mut()),
            0
        );
        assert_eq!(libc::raise(libc::SIGUSR1), 0);
    }
    let signals = ShutdownSignals::block().unwrap();
    // Queue the shutdown signal before wait, as can happen while the host starts.
    // SAFETY: querying this process's mask/pending set and signalling only ourselves.
    unsafe {
        let mut mask = std::mem::zeroed();
        assert_eq!(libc::pthread_sigmask(0, std::ptr::null(), &mut mask), 0);
        for blocked in [libc::SIGUSR1, libc::SIGINT, libc::SIGTERM] {
            assert_eq!(libc::sigismember(&mask, blocked), 1);
        }
        for untouched in [libc::SIGHUP, libc::SIGPIPE, libc::SIGQUIT] {
            let mut action = std::mem::zeroed();
            assert_eq!(libc::sigaction(untouched, std::ptr::null(), &mut action), 0);
            assert_eq!(action.sa_sigaction, libc::SIG_IGN);
        }
        assert_eq!(libc::kill(libc::getpid(), signal), 0);
        let mut pending = std::mem::zeroed();
        assert_eq!(libc::sigpending(&mut pending), 0);
        assert_eq!(libc::sigismember(&pending, libc::SIGUSR1), 1);
    }
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
            // SAFETY: only async-signal-safe signal disposition changes before exec.
            unsafe {
                command.pre_exec(move || {
                    for shutdown in [libc::SIGINT, libc::SIGTERM] {
                        let disposition = if ignored {
                            libc::SIG_IGN
                        } else {
                            libc::SIG_DFL
                        };
                        if libc::signal(shutdown, disposition) == libc::SIG_ERR {
                            return Err(io::Error::last_os_error());
                        }
                    }
                    for untouched in [libc::SIGHUP, libc::SIGPIPE, libc::SIGQUIT] {
                        if libc::signal(untouched, libc::SIG_IGN) == libc::SIG_ERR {
                            return Err(io::Error::last_os_error());
                        }
                    }
                    Ok(())
                });
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
