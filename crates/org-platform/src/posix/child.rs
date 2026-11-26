//! Forking a process that has one thread, and running a job in such a child whose only
//! output is a bounded answer read within a deadline.

use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, IntoRawFd, OwnedFd, RawFd};
use std::time::{Duration, Instant};

use super::check;

/// The descriptors a forked child looks over when it lets go of its parent's: below this,
/// or below `sysconf(_SC_OPEN_MAX)` when that is smaller.
const MOST_DESCRIPTORS: usize = 65_536;

/// Which side of a [`fork_single_threaded`] the caller is on.
#[derive(Debug, PartialEq, Eq)]
pub enum Forked {
    /// The new process: a copy of everything, with only the calling thread.
    Child(ForkChild),
    /// This process, and the new child's process ID.
    Parent(libc::pid_t),
}

/// The new process of a [`fork_single_threaded`], and what only such a process may do.
#[derive(Debug, PartialEq, Eq)]
pub struct ForkChild {
    _only_after_a_fork: (),
}

/// Fork this process (`fork`), only while it has one thread.
///
/// After a fork the child has only the thread that called it, so a lock another thread
/// held (the allocator's, a standard stream's) would stay locked in the child for good,
/// and a child that runs ordinary code (allocates, formats) could hang or corrupt memory.
/// With one thread there is no other holder. This refuses
/// ([`io::ErrorKind::WouldBlock`]) unless the process has exactly one thread now, which
/// no other thread can change in between, because there is none.
///
/// The child runs on with a copy of everything. It ends with
/// [`super::exit_immediately`], not by returning into its callers, whose cleanup is the
/// parent's.
pub fn fork_single_threaded() -> io::Result<Forked> {
    let threads = crate::process::thread_count()?;
    if threads != 1 {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            format!("cannot fork a process with {threads} threads"),
        ));
    }
    // SAFETY: the process has exactly one thread (checked just above, and there is no
    // other thread to start one), so the child inherits no lock held by another thread and
    // may run ordinary code.
    match check(unsafe { libc::fork() })? {
        0 => Ok(Forked::Child(ForkChild {
            _only_after_a_fork: (),
        })),
        pid => Ok(Forked::Parent(pid)),
    }
}

impl ForkChild {
    /// Let go of everything the parent held open: the three standard descriptors and every
    /// other open one but `keep` are pointed at `/dev/null` (`dup2`, retried when
    /// interrupted), so this process holds none of its parent's files, sockets, terminals
    /// or locks. Each number stays open (on the null device), so nothing the copied memory
    /// still owns is ever closed twice or reused under it. A standard descriptor the parent
    /// had closed is open on the null device too, and a program this child starts inherits
    /// the three; every other released number is close-on-exec.
    pub fn release_inherited_descriptors(&self, keep: &[RawFd]) -> io::Result<()> {
        let null = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/null")?;
        let null = OwnedFd::from(null);
        let most = super::open_max()
            .unwrap_or(MOST_DESCRIPTORS)
            .min(MOST_DESCRIPTORS);
        for fd in 0..most as RawFd {
            if fd == null.as_raw_fd() || keep.contains(&fd) {
                continue;
            }
            if fd > libc::STDERR_FILENO && !super::is_open(fd) {
                continue;
            }
            point_at(&null, fd)?;
        }
        if null.as_raw_fd() <= libc::STDERR_FILENO {
            // A standard descriptor was closed in the parent, so the null device took its
            // number: keep that number open there (inherited, as the other two are), and
            // give up ownership of it so that it is not closed when `null` goes.
            super::set_close_on_exec(&null, false)?;
            let _ = null.into_raw_fd();
        }
        Ok(())
    }
}

/// Point `fd` at what `null` refers to (`dup2`), retried when interrupted. A standard
/// number is left inherited by a program this child starts; any other is close-on-exec,
/// which `dup2` clears on its copy.
fn point_at(null: &OwnedFd, fd: RawFd) -> io::Result<()> {
    loop {
        // SAFETY: `null` is open and borrowed for the call; `fd` is a standard number or
        // one open in this single-threaded child, and `dup2` replaces its referent
        // atomically without freeing the number, so a copied value that owns it still
        // holds an open descriptor.
        match check(unsafe { libc::dup2(null.as_raw_fd(), fd) }) {
            Ok(_) => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    if fd > libc::STDERR_FILENO {
        // SAFETY: `fd` was just made a copy of `null` above, so it is open, and F_SETFD
        // with FD_CLOEXEC (its only descriptor flag) changes nothing but that copy.
        check(unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) })?;
    }
    Ok(())
}

/// Why [`answer_from_child`] has no answer.
#[derive(Debug)]
pub enum ChildAnswerError {
    /// No child was started (this process has other threads, or a call failed).
    NotStarted(io::Error),
    /// The child did not end its answer within the time limit; it was killed.
    TimedOut,
    /// The answer was longer than allowed; the child was killed.
    TooLarge,
    /// Reading the answer failed; the child was killed.
    Read(io::Error),
}

/// Run `job` in a forked child (only while this process has one thread:
/// [`fork_single_threaded`]) and return what it wrote, at most `most` bytes, within
/// `limit`.
///
/// The child first lets go of every descriptor but its end of the answer pipe
/// ([`ForkChild::release_inherited_descriptors`]), runs `job`, and ends at once without
/// running this process's cleanup, also when `job` panics: it never returns into the
/// caller. This process reads until the child closes the pipe. A child that is too slow
/// or says too much is killed. The child is reaped within a second; one that the kill
/// cannot end at once (stuck in a filesystem that does not answer) is left to the system.
pub fn answer_from_child(
    job: impl FnOnce(&mut dyn Write),
    limit: Duration,
    most: usize,
) -> Result<Vec<u8>, ChildAnswerError> {
    let (reader, writer) = io::pipe().map_err(ChildAnswerError::NotStarted)?;
    let pid = match fork_single_threaded().map_err(ChildAnswerError::NotStarted)? {
        Forked::Child(child) => {
            drop(reader);
            if child
                .release_inherited_descriptors(&[writer.as_raw_fd()])
                .is_ok()
            {
                let mut answer = std::fs::File::from(OwnedFd::from(writer));
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    job(&mut answer);
                }));
            }
            super::exit_immediately(0);
        }
        Forked::Parent(pid) => pid,
    };
    drop(writer);
    let answer = read_bounded(std::fs::File::from(OwnedFd::from(reader)), limit, most);
    if answer.is_err() {
        let _ = super::kill(pid, libc::SIGKILL);
    }
    let reap_by = Instant::now() + Duration::from_secs(1);
    while matches!(super::wait_pid(pid, libc::WNOHANG), Ok(None)) && Instant::now() < reap_by {
        std::thread::sleep(Duration::from_millis(10));
    }
    answer
}

fn read_bounded(
    mut file: std::fs::File,
    limit: Duration,
    most: usize,
) -> Result<Vec<u8>, ChildAnswerError> {
    let deadline = Instant::now() + limit;
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(ChildAnswerError::TimedOut);
        }
        let mut slot = [super::poll_slot(file.as_raw_fd(), libc::POLLIN)];
        match super::poll(&mut slot, Some(left)) {
            Ok(0) => return Err(ChildAnswerError::TimedOut),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(ChildAnswerError::Read(error)),
        }
        match file.read(&mut chunk) {
            Ok(0) => return Ok(bytes),
            Ok(n) if n <= most.saturating_sub(bytes.len()) => bytes.extend_from_slice(&chunk[..n]),
            Ok(_) => return Err(ChildAnswerError::TooLarge),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(ChildAnswerError::Read(error)),
        }
    }
}
