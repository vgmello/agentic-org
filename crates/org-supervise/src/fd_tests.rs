//! The child holds only fds 0-2, even while other threads open descriptors **without**
//! close-on-exec. A unit test, because making such descriptors takes `unsafe` (in `sys`).

use std::collections::BTreeSet;
use std::fs::File;
use std::os::fd::AsFd;
use std::path::PathBuf;
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use org_runtime::{EnvName, Stdio};

use crate::{spawn, sys, Command, Env};

/// The numeric fds `lsof` lists for `pid`.
fn open_fds(pid: u32) -> BTreeSet<u32> {
    let out = process::Command::new("/usr/sbin/lsof")
        .args([
            "-n",
            "-P",
            "-a",
            "-p",
            &pid.to_string(),
            "-d",
            "0-65535",
            "-F",
            "f",
        ])
        .output()
        .expect("run lsof");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.strip_prefix('f'))
        .filter_map(|f| f.parse().ok())
        .collect()
}

#[test]
fn a_child_holds_only_fds_0_to_2_while_other_threads_open_inheritable_fds() {
    // A descriptor this process holds open without close-on-exec for the whole test: any
    // spawn without POSIX_SPAWN_CLOEXEC_DEFAULT would leak it.
    let file = File::open("/dev/null").unwrap();
    let held = sys::dup_inheritable(file.as_fd()).unwrap();

    let stop = Arc::new(AtomicBool::new(false));
    let workers: Vec<_> = (0..4)
        .map(|i| {
            let stop = stop.clone();
            thread::spawn(move || {
                let file = File::open("/dev/null").unwrap();
                while !stop.load(Ordering::Relaxed) {
                    let mut fds = Vec::new();
                    for _ in 0..16 {
                        if i % 2 == 0 {
                            fds.push(sys::dup_inheritable(file.as_fd()).unwrap());
                        } else {
                            fds.push(sys::socket_inheritable().unwrap());
                        }
                    }
                    drop(fds);
                }
            })
        })
        .collect();

    let env = Env::from_vars([(EnvName::try_from("A").unwrap(), "1".to_owned())]).unwrap();
    let mut leaks = Vec::new();
    for _ in 0..20 {
        let mut child = spawn(&Command {
            program: PathBuf::from("/bin/sleep"),
            args: vec!["30".into()],
            cwd: PathBuf::from("/"),
            env: env.clone(),
            stdio: Stdio::Piped,
        })
        .unwrap();
        // Let the exec finish.
        thread::sleep(Duration::from_millis(30));
        let fds = open_fds(child.pid());
        if fds != BTreeSet::from([0, 1, 2]) {
            leaks.push(fds);
        }
        child.kill();
        let exit = child.reap().unwrap();
        assert_eq!(exit.info.signal, Some(libc::SIGKILL));
    }
    stop.store(true, Ordering::Relaxed);
    for w in workers {
        w.join().unwrap();
    }
    drop(held);
    assert!(leaks.is_empty(), "children held other fds: {leaks:?}");
}

/// The child starts with an empty signal mask, whatever the spawning thread blocks
/// (`POSIX_SPAWN_SETSIGMASK`).
#[test]
fn a_child_starts_with_no_blocked_signals() {
    sys::block_signal(libc::SIGUSR1, true).unwrap();
    let env = Env::from_vars([(EnvName::try_from("A").unwrap(), "1".to_owned())]).unwrap();
    let spawned = spawn(&Command {
        program: PathBuf::from("/bin/sleep"),
        args: vec!["30".into()],
        cwd: PathBuf::from("/"),
        env,
        stdio: Stdio::Piped,
    });
    sys::block_signal(libc::SIGUSR1, false).unwrap();
    let mut child = spawned.unwrap();
    let out = process::Command::new("/bin/ps")
        .args(["-o", "blocked=", "-p", &child.pid().to_string()])
        .output()
        .unwrap();
    let blocked = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    child.kill();
    child.reap().unwrap();
    assert_eq!(blocked, "0", "the child's blocked mask");
}
