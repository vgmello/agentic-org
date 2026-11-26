//! `posix::answer_from_child` and `posix::fork_single_threaded`, in a process with one
//! thread: this test is its own `main` (`harness = false`), because the standard harness
//! runs every test on a thread of its own, and a fork is refused while other threads run.

use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use org_platform::posix::{self, ChildAnswerError, Forked};

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

    // The answer is what the child wrote, and the child is another process.
    let answer = posix::answer_from_child(
        |out| write!(out, "{}", std::process::id()).unwrap(),
        Duration::from_secs(10),
        64,
    )
    .unwrap();
    let child: u32 = String::from_utf8(answer).unwrap().parse().unwrap();
    assert_ne!(child, parent, "the job ran in this process");
    mark("answered");

    // The child holds none of the parent's descriptors: the inherited socket end and the
    // standard descriptors are the null device there, so the peer hears nothing.
    let (inherited, mut peer) = UnixStream::pair().unwrap();
    let held = inherited.as_raw_fd();
    let answer = posix::answer_from_child(
        move |out| {
            let mine = posix::status_of_number(held).unwrap();
            let input = posix::status_of_number(0).unwrap();
            let null = std::fs::metadata("/dev/null").unwrap();
            #[allow(
                clippy::unnecessary_cast,
                reason = "st_rdev is u64 on GNU/Linux and i32 on macOS"
            )]
            let is_null = |stat: &libc::stat| {
                stat.st_mode & libc::S_IFMT == libc::S_IFCHR
                    && stat.st_rdev as u64 == std::os::unix::fs::MetadataExt::rdev(&null)
            };
            write!(out, "{} {}", is_null(&mine), is_null(&input)).unwrap();
        },
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

    let lines = std::fs::read_to_string(ran.path()).unwrap();
    let expected = ["answered", "released", "bounded", "panicked", "forked"]
        .map(|what| format!("{parent} {what}"))
        .join("\n");
    assert_eq!(lines.trim_end(), expected, "only this process went on");
    println!("single_threaded_fork: ok");
}
