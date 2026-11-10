//! Fake processes only. Every workspace, private home and mark is disposable and unique.
mod support;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use org_supervise::{Environment, PreparedSpawn, SearchPath, SessionMark, SpawnSpec, Spawned};

fn environment(root: &Path) -> Environment {
    Environment::new(
        SearchPath::new(
            Path::new("/usr/bin"),
            "/usr/bin:/bin".as_ref(),
            &[root.to_owned()],
        )
        .unwrap(),
        &[("HOME".into(), root.into()), ("TMPDIR".into(), root.into())],
    )
    .unwrap()
}

fn mark() -> SessionMark {
    let mut random = [0; 16];
    File::open("/dev/urandom")
        .unwrap()
        .read_exact(&mut random)
        .unwrap();
    SessionMark::new(format!("test-{:032x}", u128::from_ne_bytes(random))).unwrap()
}

fn prepare(root: &Path, program: PathBuf, args: Vec<OsString>, env: Environment) -> PreparedSpawn {
    PreparedSpawn::new(
        SpawnSpec {
            program,
            args,
            cwd: root.to_owned(),
            environment: env,
        },
        mark(),
    )
    .unwrap()
}

fn launch(root: &Path, case: &str) -> Spawned {
    let env = environment(root)
        .with_context(&[("ORG_TEST_CASE".into(), case.into())])
        .unwrap();
    prepare(
        root,
        std::env::current_exe().unwrap(),
        vec![
            "--exact".into(),
            "child_entry".into(),
            "--nocapture".into(),
            "--test-threads=1".into(),
        ],
        env,
    )
    .record_mark(|_| Ok(()))
    .unwrap()
    .spawn()
    .unwrap()
    .record_identity(|_| Ok(()))
    .unwrap()
}

fn output(spawned: Spawned) -> (std::process::ExitStatus, String) {
    let (status, text, errors) = support::collect(spawned, Duration::from_secs(10));
    assert!(status.success(), "stdout={text}\nstderr={errors}");
    (status, text)
}

#[test]
fn child_entry() {
    let Ok(case) = std::env::var("ORG_TEST_CASE") else {
        return;
    };
    match case.as_str() {
        "core-limit-parent" => {
            let mut limits = std::mem::MaybeUninit::<libc::rlimit>::uninit();
            // SAFETY: correctly sized output slot, querying this isolated helper only.
            assert_eq!(
                unsafe { libc::getrlimit(libc::RLIMIT_CORE, limits.as_mut_ptr()) },
                0
            );
            // SAFETY: successful query initialized both fields.
            let limits = unsafe { limits.assume_init() };
            assert_eq!((limits.rlim_cur, limits.rlim_max), (0, 0));
            // The normal prepare/spawn path rechecks the actual inherited zero limits.
            let root = std::env::current_dir().unwrap();
            output(launch(&root, "inspect"));
            println!("CORE-ZERO-ACCEPTED");
        }
        "drop-group-parent" => {
            assert_drop_kills_group();
            println!("GROUP-REAPED");
        }
        "inspect" => {
            // No open calls before this scan. The test runner has no external descriptor.
            let extra: Vec<_> = (3..1024)
                .filter(|fd| {
                    // SAFETY: F_GETFD only queries whether this numeric descriptor is open.
                    unsafe { libc::fcntl(*fd, libc::F_GETFD) >= 0 }
                })
                .collect();
            assert!(extra.is_empty(), "inherited descriptors: {extra:?}");
            assert_eq!(
                std::env::vars_os()
                    .map(|(k, _)| k)
                    .filter(|k| k == "ORG_SESSION_MARK")
                    .count(),
                1
            );
            assert!(std::env::var_os("ORG_SESSION_MARK").is_some());
            for name in [
                "SSH_AUTH_SOCK",
                "GH_TOKEN",
                "LD_PRELOAD",
                "NODE_OPTIONS",
                "CLAUDE_CODE_MESSAGING_TOKEN",
                "CARGO_MANIFEST_DIR",
            ] {
                assert!(std::env::var_os(name).is_none(), "unexpected {name}");
            }
            // SAFETY: process/session queries for this child only.
            unsafe {
                assert_eq!(libc::getsid(0), libc::getpid());
                assert_eq!(libc::getpgrp(), libc::getpid());
            }
            println!("INSPECTED");
        }
        "echo" => {
            let mut text = String::new();
            io::stdin().read_to_string(&mut text).unwrap();
            print!("ECHO:{text}");
            eprintln!("FINAL-STDERR");
        }
        "wait" => loop {
            std::thread::park();
        },
        "signal-parent" => {
            // Isolated helper process: never change the main test runner's dispositions.
            // SAFETY: only this helper process; descendants must reset these states.
            unsafe {
                libc::signal(libc::SIGTERM, libc::SIG_IGN);
                libc::signal(libc::SIGPIPE, libc::SIG_IGN);
                let mut blocked = std::mem::zeroed();
                libc::sigemptyset(&mut blocked);
                libc::sigaddset(&mut blocked, libc::SIGTERM);
                assert_eq!(
                    libc::pthread_sigmask(libc::SIG_BLOCK, &blocked, std::ptr::null_mut()),
                    0
                );
            }
            let root = std::env::current_dir().unwrap();
            for signal in ["TERM", "PIPE"] {
                let mut child = prepare(
                    &root,
                    "/bin/sh".into(),
                    vec!["-c".into(), format!("kill -{signal} $$; exit 99").into()],
                    environment(&root),
                )
                .record_mark(|_| Ok(()))
                .unwrap()
                .spawn()
                .unwrap()
                .record_identity(|_| Ok(()))
                .unwrap();
                let status = child
                    .child
                    .wait_until(Instant::now() + Duration::from_secs(5))
                    .unwrap()
                    .unwrap();
                use std::os::unix::process::ExitStatusExt;
                assert_eq!(
                    status.signal(),
                    Some(if signal == "TERM" {
                        libc::SIGTERM
                    } else {
                        libc::SIGPIPE
                    })
                );
            }
            println!("SIGNALS-RESET");
        }
        "autowait-parent" => {
            // SAFETY: isolated helper only; parent test runner's SIGCHLD is unaffected.
            unsafe {
                libc::signal(libc::SIGCHLD, libc::SIG_IGN);
            }
            let root = std::env::current_dir().unwrap();
            assert!(PreparedSpawn::new(
                SpawnSpec {
                    program: "/bin/sh".into(),
                    args: vec![],
                    cwd: root.clone(),
                    environment: environment(&root)
                },
                mark()
            )
            .is_err());
            println!("AUTOREAP-REFUSED");
        }
        other => panic!("unknown fake case {other}"),
    }
}

fn assert_drop_kills_group() {
    #[cfg(target_os = "linux")]
    // SAFETY: this is an isolated helper, not the parallel test runner. Adopt/reap its
    // grandchild so even container PID1 behavior cannot leave a fixture zombie.
    assert_eq!(
        unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) },
        0
    );
    let root = std::env::current_dir().unwrap();
    let spawned = prepare(
        &root,
        "/bin/sh".into(),
        vec!["-c".into(), "trap '' TERM; sleep 8 & echo $!; wait".into()],
        environment(&root),
    )
    .record_mark(|_| Ok(()))
    .unwrap()
    .spawn()
    .unwrap()
    .record_identity(|_| Ok(()))
    .unwrap();
    let Spawned {
        child,
        stdin,
        stdout,
        stderr,
    } = spawned;
    stdin.close().unwrap();
    drop(stderr);
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let mut line = String::new();
        std::io::BufReader::new(stdout)
            .read_line(&mut line)
            .unwrap();
        let _ = sender.send(line);
    });
    let pid: i32 = receiver
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    // SAFETY: queries only the freshly reported private fixture process.
    assert_eq!(unsafe { libc::getpgid(pid) }, child.identity().pgid as i32);
    drop(child);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        #[cfg(target_os = "linux")]
        {
            let mut status = 0;
            // SAFETY: the isolated subreaper owns this adopted, unreaped grandchild.
            let rc = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
            if rc == pid {
                assert!(libc::WIFSIGNALED(status));
                assert_eq!(libc::WTERMSIG(status), libc::SIGKILL);
                break; // reaping ends ownership; never probe or signal a reused PID
            }
            if rc == -1 {
                assert_eq!(
                    io::Error::last_os_error().kind(),
                    io::ErrorKind::Interrupted
                );
                continue;
            }
        }
        // SAFETY: signal0 only probes the fixture pid; it cannot signal a reused identity.
        if unsafe { libc::kill(pid, 0) } == -1 {
            assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
            break;
        }
        if Instant::now() >= deadline {
            #[cfg(target_os = "linux")]
            // SAFETY: clean up the isolated subreaper's own still-unreaped grandchild.
            unsafe {
                libc::kill(pid, libc::SIGKILL);
                libc::waitpid(pid, std::ptr::null_mut(), 0);
            }
            // On macOS a deliberately broken group-kill mutation has at most three
            // seconds left on sleep8; do not signal a nonchild by numeric PID.
            panic!("owned group member survived owner Drop");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn owner_drop_kills_and_reaps_its_process_group() {
    let root = tempfile::tempdir().unwrap();
    assert!(output(launch(
        &root.path().canonicalize().unwrap(),
        "drop-group-parent"
    ))
    .1
    .contains("GROUP-REAPED"));
}

#[test]
fn supplies_only_explicit_environment_and_private_session() {
    let root = tempfile::tempdir().unwrap();
    let (_, text) = output(launch(&root.path().canonicalize().unwrap(), "inspect"));
    assert!(text.contains("INSPECTED"));
}

#[cfg(target_os = "macos")]
#[test]
fn macos_holds_even_a_fast_child_until_its_identity_is_recorded() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let pending = prepare(
        &root,
        "/bin/sh".into(),
        vec!["-c".into(), "printf ready > startup-marker".into()],
        environment(&root),
    )
    .record_mark(|_| Ok(()))
    .unwrap()
    .spawn()
    .unwrap();
    let pid = pending.identity().pid;
    let mut child = pending
        .record_identity(|identity| {
            assert_eq!(identity.pid, pid);
            let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
            let size = std::mem::size_of::<libc::proc_bsdinfo>();
            // SAFETY: querying only our child with a correctly sized output record.
            assert_eq!(
                unsafe {
                    libc::proc_pidinfo(
                        pid as i32,
                        libc::PROC_PIDTBSDINFO,
                        0,
                        info.as_mut_ptr().cast(),
                        size as i32,
                    )
                },
                size as i32
            );
            // SAFETY: complete proc_bsdinfo returned above.
            assert_eq!(unsafe { info.assume_init() }.pbi_status, libc::SSTOP);
            assert!(!root.join("startup-marker").exists());
            Ok(())
        })
        .unwrap();
    child.stdin.close().unwrap();
    assert!(child
        .child
        .wait_until(Instant::now() + Duration::from_secs(5))
        .unwrap()
        .unwrap()
        .success());
    assert_eq!(
        std::fs::read(root.join("startup-marker")).unwrap(),
        b"ready"
    );
}

#[test]
fn mark_and_identity_must_be_persisted_before_stdin_is_exposed() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().canonicalize().unwrap();
    let spec = || prepare(&path, "/bin/cat".into(), vec![], environment(&path));
    assert!(spec()
        .record_mark(|_| Err(io::Error::other("persistence failed")))
        .is_err());
    let pending = spec()
        .record_mark(|mark| {
            assert!(mark.starts_with("test-"));
            Ok(())
        })
        .unwrap()
        .spawn()
        .unwrap();
    let pid = pending.identity().pid;
    let identity = pending.identity().clone();
    assert_eq!(identity.pid, identity.pgid);
    assert!(!identity.boot.is_empty());
    assert!(!identity.started.is_empty());
    assert!(pending
        .record_identity(|_| Err(io::Error::other("record failed")))
        .is_err());
    let mut status = 0;
    // SAFETY: observing only the test child; its owner must already have reaped it.
    assert_eq!(
        unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) },
        -1
    );
    assert_eq!(
        io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
}

#[test]
fn shared_stdin_closes_once_and_last_output_survives_nonreaping_wait() {
    let root = tempfile::tempdir().unwrap();
    let mut spawned = launch(&root.path().canonicalize().unwrap(), "echo");
    let close = spawned.stdin.clone();
    spawned.stdin.write_all(b"the final input").unwrap();
    close.close().unwrap();
    close.close().unwrap();
    assert_eq!(
        spawned.stdin.write(b"late").unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
    let identity = spawned.child.identity().clone();
    let (_, text) = output(spawned);
    assert!(text.contains("ECHO:the final input"));
    assert!(identity.pid > 0);
}

#[test]
fn exit_wait_retains_child_and_drop_reaps_it() {
    let root = tempfile::tempdir().unwrap();
    let mut child = prepare(
        root.path(),
        "/bin/sh".into(),
        vec!["-c".into(), "exit 17".into()],
        environment(root.path()),
    )
    .record_mark(|_| Ok(()))
    .unwrap()
    .spawn()
    .unwrap()
    .record_identity(|_| Ok(()))
    .unwrap();
    let pid = child.child.identity().pid;
    let status = child
        .child
        .wait_until(Instant::now() + Duration::from_secs(5))
        .unwrap()
        .unwrap();
    assert_eq!(status.code(), Some(17));
    assert_eq!(child.child.try_exit().unwrap(), Some(status));
    drop(child);
    // SAFETY: only our test child; Drop must have reaped it.
    assert_eq!(
        unsafe { libc::waitpid(pid as i32, std::ptr::null_mut(), libc::WNOHANG) },
        -1
    );
    assert_eq!(
        io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
}

#[test]
fn deadline_does_not_reap_and_owner_drop_kills_a_live_child() {
    let root = tempfile::tempdir().unwrap();
    let mut child = launch(&root.path().canonicalize().unwrap(), "wait");
    let pid = child.child.identity().pid;
    assert!(child
        .child
        .wait_until(Instant::now() + Duration::from_millis(25))
        .unwrap()
        .is_none());
    drop(child);
    // SAFETY: observing only the pid from the test's owned child.
    assert_eq!(
        unsafe { libc::waitpid(pid as i32, std::ptr::null_mut(), libc::WNOHANG) },
        -1
    );
    assert_eq!(
        io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
}

#[test]
fn resets_inherited_signal_mask_and_ignored_dispositions() {
    let root = tempfile::tempdir().unwrap();
    assert!(output(launch(
        &root.path().canonicalize().unwrap(),
        "signal-parent"
    ))
    .1
    .contains("SIGNALS-RESET"));
}

#[test]
fn accepts_the_hardened_core_limit_in_an_isolated_helper() {
    let root = tempfile::tempdir().unwrap();
    assert!(output(launch(
        &root.path().canonicalize().unwrap(),
        "core-limit-parent"
    ))
    .1
    .contains("CORE-ZERO-ACCEPTED"));
}

#[test]
fn refuses_an_automatic_child_reaper() {
    let root = tempfile::tempdir().unwrap();
    assert!(output(launch(
        &root.path().canonicalize().unwrap(),
        "autowait-parent"
    ))
    .1
    .contains("AUTOREAP-REFUSED"));
}

#[test]
fn rejects_setid_binaries_and_environment_injection_or_collisions() {
    let root = tempfile::tempdir().unwrap();
    let binary = root.path().join("setid");
    std::fs::copy("/bin/sh", &binary).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o4755)).unwrap();
    assert!(PreparedSpawn::new(
        SpawnSpec {
            program: binary,
            args: vec![],
            cwd: root.path().into(),
            environment: environment(root.path())
        },
        mark()
    )
    .is_err());
    for name in [
        "HOME",
        "ORG_SESSION_TOKEN",
        "GIT_CONFIG_COUNT",
        "DYLD_INSERT_LIBRARIES",
        "LD_PRELOAD",
        "NODE_OPTIONS",
        "BASH_ENV",
        "ENV",
        "PYTHONSTARTUP",
        "CLAUDE_CODE_BAD",
    ] {
        assert!(environment(root.path())
            .with_runtime_additions(&[(name.into(), "secret".into())])
            .is_err());
    }
    assert!(environment(root.path())
        .with_runtime_additions(&[
            ("ENABLE_TOOL_SEARCH".into(), "false".into()),
            ("ENABLE_TOOL_SEARCH".into(), "true".into())
        ])
        .is_err());
    let env = environment(root.path())
        .with_context(&[("ORG_SESSION_TOKEN".into(), "very-secret".into())])
        .unwrap();
    assert!(!format!("{env:?}").contains("very-secret"));
}

#[test]
fn search_path_drops_relative_workspace_home_and_symlink_aliases() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let alias = root.path().join("alias");
    std::os::unix::fs::symlink(&home, &alias).unwrap();
    let captured = std::env::join_paths([
        Path::new("."),
        Path::new("relative"),
        &home,
        &alias,
        Path::new("/bin"),
    ])
    .unwrap();
    let path = SearchPath::new(Path::new("/usr/bin"), &captured, &[home]).unwrap();
    let text = format!("{path:?}");
    assert!(!text.contains("home"));
    assert!(!text.contains("alias"));
    assert!(!text.contains("relative"));
}

#[test]
fn only_three_fds_even_during_concurrent_inheritable_fd_receipt() {
    use std::os::unix::net::UnixStream;
    let root = tempfile::tempdir().unwrap();
    let (tx, rx) = UnixStream::pair().unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let done = Arc::new(AtomicBool::new(false));
    let received = Arc::new(AtomicUsize::new(0));
    let receiver = {
        let barrier = barrier.clone();
        let done = done.clone();
        let received = received.clone();
        std::thread::spawn(move || {
            loop {
                // SAFETY: SCM_RIGHTS buffers use aligned storage, sufficient for one fd;
                // receiver owns the returned fd and intentionally leaves it inheritable.
                let fd = unsafe {
                    let mut byte = [0u8];
                    let mut iov = libc::iovec {
                        iov_base: byte.as_mut_ptr().cast(),
                        iov_len: 1,
                    };
                    let mut control = [0usize; 8];
                    let mut msg = std::mem::zeroed::<libc::msghdr>();
                    msg.msg_iov = &mut iov;
                    msg.msg_iovlen = 1;
                    msg.msg_control = control.as_mut_ptr().cast();
                    msg.msg_controllen = std::mem::size_of_val(&control) as _;
                    assert_eq!(libc::recvmsg(rx.as_raw_fd(), &mut msg, 0), 1);
                    let header = libc::CMSG_FIRSTHDR(&msg);
                    assert!(!header.is_null());
                    let fd = libc::CMSG_DATA(header).cast::<i32>().read_unaligned();
                    assert_eq!(libc::fcntl(fd, libc::F_SETFD, 0), 0);
                    fd
                };
                received.fetch_add(1, Ordering::SeqCst);
                barrier.wait(); // main spawns while this newly received fd is inheritable
                barrier.wait();
                // SAFETY: receiver uniquely owns the received descriptor.
                unsafe {
                    libc::close(fd);
                }
                if done.load(Ordering::SeqCst) {
                    break;
                }
            }
        })
    };
    let file = File::open("/dev/null").unwrap();
    for round in 0..16 {
        // SAFETY: aligned, sized SCM_RIGHTS payload with one valid borrowed fd.
        unsafe {
            let mut byte = [1u8];
            let mut iov = libc::iovec {
                iov_base: byte.as_mut_ptr().cast(),
                iov_len: 1,
            };
            let mut control = [0usize; 8];
            let mut msg = std::mem::zeroed::<libc::msghdr>();
            msg.msg_iov = &mut iov;
            msg.msg_iovlen = 1;
            msg.msg_control = control.as_mut_ptr().cast();
            msg.msg_controllen = libc::CMSG_SPACE(std::mem::size_of::<i32>() as _) as _;
            let header = libc::CMSG_FIRSTHDR(&msg);
            (*header).cmsg_level = libc::SOL_SOCKET;
            (*header).cmsg_type = libc::SCM_RIGHTS;
            (*header).cmsg_len = libc::CMSG_LEN(std::mem::size_of::<i32>() as _) as _;
            libc::CMSG_DATA(header)
                .cast::<i32>()
                .write_unaligned(file.as_raw_fd());
            assert_eq!(libc::sendmsg(tx.as_raw_fd(), &msg, 0), 1);
        }
        barrier.wait();
        let (_, text) = output(launch(&root.path().canonicalize().unwrap(), "inspect"));
        assert!(text.contains("INSPECTED"));
        if round == 15 {
            done.store(true, Ordering::SeqCst);
        }
        barrier.wait();
    }
    receiver.join().unwrap();
    assert_eq!(received.load(Ordering::SeqCst), 16);
}
