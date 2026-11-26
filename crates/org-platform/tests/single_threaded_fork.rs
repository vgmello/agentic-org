//! `posix::answer_from_child` and `posix::fork_single_threaded`, in a process with one
//! thread: this test is its own `main` (`harness = false`), because the standard harness
//! runs every test on a thread of its own, and a fork is refused while other threads run.
//! The same holds for the checks of a process whose standard descriptors are closed:
//! each closes them in a forked child of its own.

use std::io::{Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use org_platform::posix::{self, ChildAnswerError, ForkChild, Forked, Standard};

/// A check in a forked child whose standard descriptors it closes: 0, or which step failed.
type StandardCheck = fn(&ForkChild, &UnixStream) -> i32;

/// Whether `fd` is open on the null device.
fn is_null(fd: RawFd) -> bool {
    let Ok(stat) = posix::status_of_number(fd) else {
        return false;
    };
    let null = std::fs::metadata("/dev/null").unwrap();
    #[allow(
        clippy::unnecessary_cast,
        reason = "st_rdev is u64 on GNU/Linux and i32 on macOS"
    )]
    let device = stat.st_rdev as u64;
    stat.st_mode & libc::S_IFMT == libc::S_IFCHR
        && device == std::os::unix::fs::MetadataExt::rdev(&null)
}

/// What `fd` is: `closed`, `null` (the null device), `pipe` or `other`.
fn kind(fd: RawFd) -> &'static str {
    match posix::status_of_number(fd) {
        Err(_) => "closed",
        Ok(_) if is_null(fd) => "null",
        Ok(stat) if stat.st_mode & libc::S_IFMT == libc::S_IFIFO => "pipe",
        Ok(_) => "other",
    }
}

/// Whether `fd` is open and inherited by a program this process starts.
fn inherited(fd: RawFd) -> bool {
    matches!(posix::is_close_on_exec(fd), Ok(false))
}

/// Close the numbers `fds` of this forked child, as a parent started without them would.
fn close(fds: &[RawFd]) {
    for &fd in fds {
        // SAFETY: (test) this single-threaded forked child holds no Rust value that owns a
        // standard number; closing one only leaves the number free.
        unsafe { libc::close(fd) };
    }
}

/// The standard descriptors were closed: releasing points all three at the null device and
/// keeps them open (the null device took number 0, which must not be closed again) and
/// inherited, and makes every other released number (`held`) close-on-exec.
fn released_with_standard_closed(child: &ForkChild, held: &UnixStream) -> i32 {
    if posix::set_close_on_exec(held, false).is_err() {
        return 1;
    }
    close(&[0, 1, 2]);
    if child.release_inherited_descriptors(&[]).is_err() {
        return 2;
    }
    for fd in 0..=2 {
        if !is_null(fd) {
            return 10 + fd;
        }
        if !inherited(fd) {
            return 20 + fd;
        }
    }
    if !is_null(held.as_raw_fd()) {
        return 30;
    }
    if !matches!(posix::is_close_on_exec(held.as_raw_fd()), Ok(true)) {
        return 31;
    }
    0
}

/// The standard descriptors were closed: in the job's child each is open, two on the null
/// device and one the answer pipe, which took one of the free numbers.
fn answered_with_standard_closed(_child: &ForkChild, _held: &UnixStream) -> i32 {
    close(&[0, 1, 2]);
    let answer = posix::answer_from_child(
        |out| {
            let kinds = (0..=2).map(kind).collect::<Vec<_>>().join(" ");
            out.write_all(kinds.as_bytes()).unwrap();
        },
        Duration::from_secs(10),
        64,
    );
    let Ok(answer) = answer else {
        return 1;
    };
    let Ok(answer) = String::from_utf8(answer) else {
        return 2;
    };
    let mut kinds: Vec<&str> = answer.split(' ').collect();
    kinds.sort_unstable();
    if kinds != ["null", "null", "pipe"] {
        return 3;
    }
    0
}

/// Standard input was closed: reopening only standard output leaves it closed, and
/// reopening it too keeps the null device open at its number and inherited.
fn reopened_with_input_closed(_child: &ForkChild, _held: &UnixStream) -> i32 {
    close(&[0]);
    if posix::reopen_standard_on_null(&[Standard::Output]).is_err() {
        return 1;
    }
    if posix::is_open(0) {
        return 2;
    }
    if !is_null(1) || !inherited(1) {
        return 3;
    }
    if posix::reopen_standard_on_null(&[Standard::Input, Standard::Error]).is_err() {
        return 4;
    }
    for fd in [0, 2] {
        if !is_null(fd) || !inherited(fd) {
            return 10 + fd;
        }
    }
    0
}

fn main() {
    let parent = std::process::id();
    let ran = tempfile::NamedTempFile::new().unwrap();
    let mark = |what: &str| {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(ran.path())
            .unwrap();
        writeln!(file, "{} {what}", std::process::id()).unwrap();
    };

    // The answer is what the child wrote, and the child is another process. Where the
    // process adapter refuses, no thread count is known and nothing forks.
    let answer = match posix::answer_from_child(
        |out| write!(out, "{}", std::process::id()).unwrap(),
        Duration::from_secs(10),
        64,
    ) {
        Err(ChildAnswerError::NotStarted(error))
            if error.kind() == std::io::ErrorKind::Unsupported =>
        {
            println!("single_threaded_fork: skipped, {error}");
            return;
        }
        answer => answer.unwrap(),
    };
    let child: u32 = String::from_utf8(answer).unwrap().parse().unwrap();
    assert_ne!(child, parent, "the job ran in this process");
    mark("answered");

    // The child holds none of the parent's descriptors: the inherited socket end and the
    // standard descriptors are the null device there, so the peer hears nothing.
    let (inherited, mut peer) = UnixStream::pair().unwrap();
    let held = inherited.as_raw_fd();
    let answer = posix::answer_from_child(
        move |out| write!(out, "{} {}", is_null(held), is_null(0)).unwrap(),
        Duration::from_secs(10),
        64,
    )
    .unwrap();
    assert_eq!(String::from_utf8(answer).unwrap(), "true true");
    peer.set_nonblocking(true).unwrap();
    assert_eq!(
        peer.read(&mut [0; 1]).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock,
        "the parent's socket end is still open here and silent"
    );
    drop(inherited);
    mark("released");

    // A job that does not finish within the limit is killed; one that says too much too.
    let started = Instant::now();
    let slow = posix::answer_from_child(
        |_| std::thread::sleep(Duration::from_secs(30)),
        Duration::from_millis(300),
        64,
    );
    assert!(matches!(slow, Err(ChildAnswerError::TimedOut)), "{slow:?}");
    assert!(started.elapsed() < Duration::from_secs(5));
    let loud = posix::answer_from_child(
        |out| out.write_all(&[b'x'; 100]).unwrap(),
        Duration::from_secs(10),
        10,
    );
    assert!(matches!(loud, Err(ChildAnswerError::TooLarge)), "{loud:?}");
    mark("bounded");

    // A job that panics ends its child, which never returns into this code.
    let panicked = posix::answer_from_child(
        |out| {
            out.write_all(b"partial").unwrap();
            panic!("the job failed");
        },
        Duration::from_secs(10),
        64,
    )
    .unwrap();
    assert_eq!(panicked, b"partial");
    mark("panicked");

    // The bare fork: the child exits with its own status, which the parent reaps.
    match posix::fork_single_threaded().unwrap() {
        Forked::Child(_) => posix::exit_immediately(7),
        Forked::Parent(pid) => {
            let status = posix::reap(pid).unwrap();
            assert!(libc::WIFEXITED(status));
            assert_eq!(libc::WEXITSTATUS(status), 7);
        }
    }
    mark("forked");

    // A child whose parent had closed its standard descriptors: each check runs in a child
    // of its own, which closes them, and exits with 0 or the number of the check that
    // failed. A panic there is silent (its standard error may be another file) and 99.
    let (held, _peer) = UnixStream::pair().unwrap();
    let checks: [(&str, StandardCheck); 3] = [
        ("released", released_with_standard_closed),
        ("answered", answered_with_standard_closed),
        ("reopened", reopened_with_input_closed),
    ];
    for (name, check) in checks {
        match posix::fork_single_threaded().unwrap() {
            Forked::Child(child) => {
                std::panic::set_hook(Box::new(|_| {}));
                let failed = std::panic::catch_unwind(|| check(&child, &held)).unwrap_or(99);
                posix::exit_immediately(failed);
            }
            Forked::Parent(pid) => {
                let status = posix::reap(pid).unwrap();
                assert!(libc::WIFEXITED(status), "{name}: {status:#x}");
                assert_eq!(
                    libc::WEXITSTATUS(status),
                    0,
                    "{name}: the check that failed"
                );
            }
        }
    }
    mark("standard");

    let lines = std::fs::read_to_string(ran.path()).unwrap();
    let expected = [
        "answered", "released", "bounded", "panicked", "forked", "standard",
    ]
    .map(|what| format!("{parent} {what}"))
    .join("\n");
    assert_eq!(lines.trim_end(), expected, "only this process went on");
    println!("single_threaded_fork: ok");
}
