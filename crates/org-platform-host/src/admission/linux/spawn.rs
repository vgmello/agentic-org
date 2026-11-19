//! Starting a child from a sealed image, and the handle that owns it: the one module
//! of the Linux admission adapter that creates a process (design section 3.1, steps 3
//! and 4).
//!
//! # The start
//!
//! `clone` with `CLONE_PIDFD` and `SIGCHLD`, without a shared address space: the
//! child is a copy of this process that runs [`exec_child`] and nothing else. It
//! places its three pipes on descriptors 0 to 2, moves to `/`, places the sealed image
//! on descriptor 3 and its failure report on descriptor 4 (both close-on-exec), closes
//! every other descriptor, and executes descriptor 3 with `execveat` and
//! `AT_EMPTY_PATH`. The kernel opens the image before it closes the close-on-exec
//! descriptors, so the program starts with exactly descriptors 0 to 2.
//!
//! `clone` rather than `clone3`: Docker's default seccomp profile answers `ENOSYS` to
//! `clone3` in a container without `CAP_SYS_ADMIN` (a filter cannot read the flags
//! inside its argument structure), and allows `clone` without namespace flags. The
//! raw call's argument order differs between architectures, so [`clone_and_exec`]
//! refuses an architecture whose order it does not know.
//!
//! # Why [`spawn_sealed`] returns only after the `exec`
//!
//! The report pipe is how this process learns what became of the child. Its write end
//! is close-on-exec, so a successful `execveat` closes it and this process reads end
//! of file: from then on the child's PID runs the sealed image and never ran anything
//! else of its own. (A child that something killed before its `execveat` reads the
//! same; it ran nothing, and its exit is seen on the pidfd like any other.) A step
//! that fails writes its number and the system's error there
//! and exits with 127; this process reaps the child and returns the error, which
//! names the call. Without the report a caller could not tell a refused `execveat`
//! (`vm.memfd_noexec = 2`, a container profile, a `#!` script) from a program that
//! ran and exited.
//!
//! # What the child may do
//!
//! Between `clone` and `execveat` the child shares nothing with this process but is a
//! copy of one thread of it: another thread may have held the allocator's lock, or any
//! other lock, at the moment of the copy, and in the child nobody will ever release
//! it. So [`exec_child`] calls only functions that are async-signal-safe (the system
//! call wrapper, `sigaction`, `sigprocmask`, `_exit`), reads only values prepared
//! before the copy, allocates nothing, takes no lock, and cannot panic: it has no
//! arithmetic, no indexing and no formatting. Every signal is blocked across the copy
//! and unblocked as the child's last step before `execveat`, when no descriptor of
//! this process is left in it: a handler this process installed can run in the child
//! at most in that last instant, with nothing of the host to reach.

use std::ffi::{CString, OsStr, OsString};
use std::fs::File;
use std::io::{self, Read};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::Mutex;

use super::sys::{self, failed, named};

/// Where the child places the sealed image before it executes it.
const IMAGE_FD: libc::c_long = 3;
/// Where the child places the write end of its failure report.
const REPORT_FD: libc::c_long = 4;
/// The first descriptor the child closes, and every one above it.
const FIRST_CLOSED: libc::c_long = sys::FIRST_FREE as libc::c_long;
// The image and the report sit directly below the first descriptor the child closes.
const _: () = assert!(IMAGE_FD == 3 && REPORT_FD == 4 && FIRST_CLOSED == 5);
/// The highest descriptor number `close_range` takes.
const LAST_DESCRIPTOR: libc::c_long = u32::MAX as libc::c_long;
/// Arguments of the child's system calls, as the wrapper takes them.
const NO_FLAGS: libc::c_long = 0;
const CLOSE_ON_EXEC: libc::c_long = libc::O_CLOEXEC as libc::c_long;
const EMPTY_PATH: libc::c_long = libc::AT_EMPTY_PATH as libc::c_long;
/// The report of a failed step: two integers, the step and the system's error.
const INTEGER_SIZE: usize = std::mem::size_of::<libc::c_int>();
const REPORT_SIZE: usize = 2 * INTEGER_SIZE;
const TWO_PARTS: libc::c_long = 2;

/// The exit status of a child that could not execute the image.
const EXIT_NOT_EXECUTED: libc::c_int = 127;

/// The steps of the child, as it reports the one that failed.
const STEP_SIGACTION: libc::c_int = 1;
const STEP_DUP3: libc::c_int = 2;
const STEP_CHDIR: libc::c_int = 3;
const STEP_CLOSE_RANGE: libc::c_int = 4;
const STEP_SIGPROCMASK: libc::c_int = 5;
const STEP_EXECVEAT: libc::c_int = 6;

/// The system call behind a step number of the child.
pub(super) fn step_name(step: libc::c_int) -> &'static str {
    match step {
        STEP_SIGACTION => "sigaction",
        STEP_DUP3 => "dup3",
        STEP_CHDIR => "chdir",
        STEP_CLOSE_RANGE => "close_range",
        STEP_SIGPROCMASK => "sigprocmask",
        STEP_EXECVEAT => "execveat",
        _ => "an unknown step of the child",
    }
}

/// A started child with the host's ends of its standard input, output and error.
pub(super) struct Spawned {
    pub(super) child: Child,
    pub(super) stdin: File,
    pub(super) stdout: File,
    pub(super) stderr: File,
}

/// The arguments and the environment of a child as C strings, with the pointer
/// arrays `execveat` and `posix_spawn` take. Built before any process is created.
pub(super) struct Strings {
    _owned: Vec<CString>,
    argv: Vec<*const libc::c_char>,
    envp: Vec<*const libc::c_char>,
}

impl Strings {
    /// `program` as the first argument, then `args`; `env` as `NAME=value` strings.
    pub(super) fn new(
        program: &Path,
        args: &[&OsStr],
        env: &[(OsString, OsString)],
    ) -> io::Result<Self> {
        let c = |bytes: &[u8]| {
            CString::new(bytes).map_err(|_| io::Error::other("an argument holds a NUL byte"))
        };
        let mut owned = vec![c(program.as_os_str().as_bytes())?];
        for arg in args {
            owned.push(c(arg.as_bytes())?);
        }
        let arguments = owned.len();
        for (name, value) in env {
            let mut pair = name.as_bytes().to_vec();
            pair.push(b'=');
            pair.extend_from_slice(value.as_bytes());
            owned.push(c(&pair)?);
        }
        // A `CString`'s bytes are on the heap: moving `owned` does not move them.
        let pointers = |strings: &[CString]| {
            let mut pointers: Vec<_> = strings.iter().map(|s| s.as_ptr()).collect();
            pointers.push(std::ptr::null());
            pointers
        };
        Ok(Self {
            argv: pointers(&owned[..arguments]),
            envp: pointers(&owned[arguments..]),
            _owned: owned,
        })
    }

    /// The NULL-terminated argument array.
    pub(super) fn argv(&self) -> *const *const libc::c_char {
        self.argv.as_ptr()
    }

    /// The NULL-terminated environment array.
    pub(super) fn envp(&self) -> *const *const libc::c_char {
        self.envp.as_ptr()
    }
}

/// The three pipes of a child: the child's ends, numbered at least
/// [`sys::FIRST_FREE`], and the host's.
pub(super) struct Pipes {
    pub(super) child: [OwnedFd; 3],
    pub(super) stdin: File,
    pub(super) stdout: File,
    pub(super) stderr: File,
}

impl Pipes {
    /// New pipes for a child's standard input, output and error, all ends
    /// close-on-exec.
    pub(super) fn new() -> io::Result<Self> {
        let (stdin_child, stdin) = sys::pipe()?;
        let (stdout, stdout_child) = sys::pipe()?;
        let (stderr, stderr_child) = sys::pipe()?;
        Ok(Self {
            child: [
                sys::above_the_childs_descriptors(stdin_child)?,
                sys::above_the_childs_descriptors(stdout_child)?,
                sys::above_the_childs_descriptors(stderr_child)?,
            ],
            stdin: File::from(stdin),
            stdout: File::from(stdout),
            stderr: File::from(stderr),
        })
    }
}

/// Everything [`exec_child`] reads, prepared before the process is copied. Plain
/// numbers and pointers: reading them calls nothing.
struct ChildPlan {
    stdin: libc::c_long,
    stdout: libc::c_long,
    stderr: libc::c_long,
    image: libc::c_long,
    report: libc::c_long,
    root: *const libc::c_char,
    empty: *const libc::c_char,
    argv: *const *const libc::c_char,
    envp: *const *const libc::c_char,
    default_action: libc::sigaction,
    no_signals: libc::sigset_t,
    /// A null pointer for the calls that take an optional out-value.
    no_old_action: *mut libc::sigaction,
    no_old_mask: *mut libc::sigset_t,
}

/// Starts the sealed `image` with `strings` as its arguments and environment and `/`
/// as its working directory, with its standard input, output and error on new pipes
/// and no other descriptor. Returns once the child executes the image (see the module
/// documentation); an error means no child is left.
///
/// `image` must be numbered at least [`sys::FIRST_FREE`]. `fence` is held while the
/// pipes and the process are made.
pub(super) fn spawn_sealed(
    image: BorrowedFd<'_>,
    strings: &Strings,
    fence: &Mutex<()>,
) -> io::Result<Spawned> {
    if image.as_raw_fd() < sys::FIRST_FREE {
        return Err(io::Error::other(
            "the sealed image is numbered below the child's own descriptors",
        ));
    }
    // The child is a copy of this process's memory until it executes the image:
    // closed to other processes of this user, as it was when the image was sealed.
    sys::close_to_other_processes()?;
    let (pipes, report, report_child, started) = {
        // No descriptor of the caller's may be half-made while a child is created.
        let _one_at_a_time = fence
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let pipes = Pipes::new()?;
        let (report, report_child) = sys::pipe()?;
        let report_child = sys::above_the_childs_descriptors(report_child)?;
        // SAFETY: all-zero is a valid `sigaction`; the handler is then the default
        // action and the mask is emptied by the call made for that.
        let default_action = unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = libc::SIG_DFL;
            libc::sigemptyset(&mut action.sa_mask);
            action
        };
        // SAFETY: all-zero is a valid `sigset_t` for `sigemptyset` and `sigfillset`
        // to initialize.
        let (no_signals, every_signal) = unsafe {
            let (mut none, mut every): (libc::sigset_t, libc::sigset_t) =
                (std::mem::zeroed(), std::mem::zeroed());
            libc::sigemptyset(&mut none);
            libc::sigfillset(&mut every);
            (none, every)
        };
        let plan = ChildPlan {
            stdin: libc::c_long::from(pipes.child[0].as_raw_fd()),
            stdout: libc::c_long::from(pipes.child[1].as_raw_fd()),
            stderr: libc::c_long::from(pipes.child[2].as_raw_fd()),
            image: libc::c_long::from(image.as_raw_fd()),
            report: libc::c_long::from(report_child.as_raw_fd()),
            root: c"/".as_ptr(),
            empty: c"".as_ptr(),
            argv: strings.argv(),
            envp: strings.envp(),
            default_action,
            no_signals,
            no_old_action: std::ptr::null_mut(),
            no_old_mask: std::ptr::null_mut(),
        };
        let started = clone_and_exec(&plan, &every_signal);
        (pipes, report, report_child, started)
    };
    let child = started?;
    // The child holds its own copies of its ends; this process's copies close here,
    // so that the report reads end of file when the child's copy closes.
    drop((pipes.child, report_child));
    read_report(child, report).map(|child| Spawned {
        child,
        stdin: pipes.stdin,
        stdout: pipes.stdout,
        stderr: pipes.stderr,
    })
}

/// Copies this process with every signal blocked. The copy runs [`exec_child`]; this
/// process gets the child's handle, with its own signal mask restored.
fn clone_and_exec(plan: &ChildPlan, every_signal: &libc::sigset_t) -> io::Result<Child> {
    // The raw `clone` takes (flags, stack, parent_tid, ...) on these architectures;
    // the two arguments after them differ in order, and both are zero here. Another
    // architecture may order the first three differently (s390 does).
    if !cfg!(any(target_arch = "x86_64", target_arch = "aarch64")) {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "clone's argument order is not known for this architecture",
        ));
    }
    let flags = (libc::CLONE_PIDFD | libc::SIGCHLD) as libc::c_ulong;
    let no_stack: *mut libc::c_void = std::ptr::null_mut();
    let unused: libc::c_ulong = 0;
    let mut pidfd: libc::c_int = -1;
    // SAFETY: all-zero is a valid `sigset_t` for the call to overwrite.
    let mut mask_before: libc::sigset_t = unsafe { std::mem::zeroed() };
    // SAFETY: two valid signal sets. Only this thread's mask changes, and it is put
    // back below on every path.
    let blocked =
        unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, every_signal, &mut mask_before) };
    if blocked != 0 {
        return Err(failed("pthread_sigmask", blocked));
    }
    // SAFETY: `pidfd` is a valid out-value that outlives the call (with `CLONE_PIDFD`
    // the kernel stores the new pidfd where `parent_tid` points). With no stack and
    // no `CLONE_VM` the call returns twice, as `fork` does: here with the child's PID,
    // and in a copy of this process with 0. The copy calls `exec_child` at once and
    // never returns into Rust code that could unwind, allocate or lock: `exec_child`
    // upholds that.
    let pid = unsafe {
        let pid = libc::syscall(
            libc::SYS_clone,
            flags,
            no_stack,
            std::ptr::from_mut(&mut pidfd),
            unused,
            unused,
        );
        if pid == 0 {
            exec_child(plan)
        }
        pid
    };
    // Read before anything else can change this thread's `errno`.
    let clone_error = io::Error::last_os_error();
    // SAFETY: the mask saved above. This cannot fail with a valid `how`.
    unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &mask_before, std::ptr::null_mut()) };
    if pid < 0 {
        return Err(named("clone", clone_error));
    }
    if pidfd < 0 {
        // The kernel wrote no pidfd, so the child cannot be held as the design
        // requires. It is this process's unreaped child, so its PID is still its own:
        // end it by PID, reap it and refuse.
        // SAFETY: signals and reaps exactly the child `clone` just returned.
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGKILL);
            libc::waitpid(pid as libc::pid_t, std::ptr::null_mut(), 0);
        }
        return Err(io::Error::other("clone returned no pidfd"));
    }
    // SAFETY: the new pidfd the kernel stored for this call; nothing else owns it.
    let pidfd = unsafe { OwnedFd::from_raw_fd(pidfd) };
    Ok(Child::started(pid as libc::pid_t, pidfd))
}

/// The child's side of a start, in a copy of the host between `clone` and `execveat`.
/// Never returns: it becomes the image, or reports the failed step and exits.
///
/// # Safety
///
/// Called only in the single-threaded copy `clone` made, with every signal blocked.
/// Every descriptor in `plan` is open and numbered at least [`sys::FIRST_FREE`], and
/// every pointer in it is valid in the copy (it points into memory the parent
/// prepared before the copy and has not freed).
///
/// The body must stay async-signal-safe (see the module documentation): system calls
/// through `libc::syscall`, `libc::sigaction`, `libc::sigprocmask`,
/// `libc::__errno_location` and `libc::_exit`, and nothing else. No allocation, no
/// lock, no formatting, no arithmetic or indexing that could panic, and no call into
/// Rust's standard library: every value is a constant or a field of `plan`.
#[inline(never)]
unsafe fn exec_child(plan: &ChildPlan) -> ! {
    // Where a failure is reported: the parent's descriptor until it is placed on
    // `REPORT_FD`, which is the only copy `close_range` leaves.
    let mut report = plan.report;
    // SAFETY: the function's contract. Each call takes plain numbers and pointers
    // that are valid in this process and keeps none of them.
    let step = unsafe {
        'steps: {
            // `SIGPIPE` at its default: Rust's runtime ignores it in the host, and an
            // ignored signal stays ignored across `execve`.
            if libc::sigaction(libc::SIGPIPE, &plan.default_action, plan.no_old_action) != 0 {
                break 'steps STEP_SIGACTION;
            }
            // The three pipes on descriptors 0 to 2. `dup3` with no flag is `dup2`
            // (which not every architecture has) for two different descriptors, and
            // the sources are numbered above every target.
            if libc::syscall(libc::SYS_dup3, plan.stdin, 0 as libc::c_long, NO_FLAGS) < 0 {
                break 'steps STEP_DUP3;
            }
            if libc::syscall(libc::SYS_dup3, plan.stdout, 1 as libc::c_long, NO_FLAGS) < 0 {
                break 'steps STEP_DUP3;
            }
            if libc::syscall(libc::SYS_dup3, plan.stderr, 2 as libc::c_long, NO_FLAGS) < 0 {
                break 'steps STEP_DUP3;
            }
            if libc::syscall(libc::SYS_chdir, plan.root) < 0 {
                break 'steps STEP_CHDIR;
            }
            // The sealed image and the report, close-on-exec, directly above the
            // pipes.
            if libc::syscall(libc::SYS_dup3, plan.image, IMAGE_FD, CLOSE_ON_EXEC) < 0 {
                break 'steps STEP_DUP3;
            }
            if libc::syscall(libc::SYS_dup3, plan.report, REPORT_FD, CLOSE_ON_EXEC) < 0 {
                break 'steps STEP_DUP3;
            }
            report = REPORT_FD;
            // Everything else of the host's is closed: no descriptor reaches the
            // image, close-on-exec or not.
            if libc::syscall(
                libc::SYS_close_range,
                FIRST_CLOSED,
                LAST_DESCRIPTOR,
                NO_FLAGS,
            ) < 0
            {
                break 'steps STEP_CLOSE_RANGE;
            }
            if libc::sigprocmask(libc::SIG_SETMASK, &plan.no_signals, plan.no_old_mask) != 0 {
                break 'steps STEP_SIGPROCMASK;
            }
            // The image itself, not a path: what is executed is the sealed object.
            libc::syscall(
                libc::SYS_execveat,
                IMAGE_FD,
                plan.empty,
                plan.argv,
                plan.envp,
                EMPTY_PATH,
            );
            STEP_EXECVEAT
        }
    };
    // SAFETY: `__errno_location` returns the address of this thread's `errno`, which the
    // failed call set and nothing has changed since. The report is the step and that
    // `errno`, written by the kernel from where they are: this code does not read
    // through a pointer, so not even a debug build's pointer checks are in the child.
    // A failed write changes nothing (the parent then reads a short report and
    // refuses all the same). `_exit` ends the process without running any of the
    // host's exit handlers.
    unsafe {
        let errno = libc::__errno_location();
        let message = [
            libc::iovec {
                iov_base: &raw const step as *mut libc::c_void,
                iov_len: INTEGER_SIZE,
            },
            libc::iovec {
                iov_base: errno as *mut libc::c_void,
                iov_len: INTEGER_SIZE,
            },
        ];
        libc::syscall(libc::SYS_writev, report, &raw const message, TWO_PARTS);
        libc::_exit(EXIT_NOT_EXECUTED)
    }
}

/// Waits for the child's `exec`: end of file on `report` means the child executes
/// the image (or was killed before it could: it ran nothing). Anything else is a
/// failed step: the child is reaped and the error names it.
pub(super) fn read_report(mut child: Child, report: OwnedFd) -> io::Result<Child> {
    const SIZE: usize = REPORT_SIZE;
    let mut report = File::from(report);
    let mut message = [0u8; SIZE];
    let mut read = 0;
    let outcome = loop {
        match report.read(&mut message[read..]) {
            Ok(0) => break Ok(read),
            Ok(count) => read += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => break Err(named("read", error)),
        }
        if read == SIZE {
            break Ok(read);
        }
    };
    match outcome {
        Ok(0) => Ok(child),
        Ok(SIZE) => {
            child.kill();
            let (step, errno) = message.split_at(SIZE / 2);
            let number = |bytes: &[u8]| bytes.try_into().map_or(0, libc::c_int::from_ne_bytes);
            Err(failed(step_name(number(step)), number(errno)))
        }
        Ok(_) => {
            child.kill();
            Err(io::Error::other(
                "the child's report of its failed step was cut short",
            ))
        }
        Err(error) => {
            child.kill();
            Err(error)
        }
    }
}

/// How a reaped child ended: `si_code` and `si_status` of its `SIGCHLD` information.
type Exit = (libc::c_int, libc::c_int);

/// A child process the host started and holds by its pidfd: killed and reaped when
/// dropped, if it has not been reaped.
///
/// The pidfd names this one process for as long as it is held. The PID is the
/// child's own until the child is reaped here; nothing else may reap it (a host that
/// ignores `SIGCHLD` or waits for any child would free the PID for reuse).
pub(in crate::admission) struct Child {
    pid: libc::pid_t,
    pidfd: OwnedFd,
    /// Whether nothing is left to signal or wait for: reaped here or elsewhere, or
    /// never this code's to reap (a test's stand-in).
    reaped: bool,
    /// How it ended, once reaped here.
    exit: Option<Exit>,
    /// Tests only: the identity a scripted check admitted it with.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) admitted: Option<crate::admission::Peer>,
}

impl Child {
    /// The handle of the child `pid` that `pidfd` names: this process's own, unreaped.
    pub(super) fn started(pid: libc::pid_t, pidfd: OwnedFd) -> Self {
        Self {
            pid,
            pidfd,
            reaped: false,
            exit: None,
            #[cfg(any(test, feature = "test-support"))]
            admitted: None,
        }
    }

    /// A handle for a process this code does not own: it counts as reaped, so it is
    /// never signalled and never waited for. The pidfd is opened for the process
    /// `pid` names now.
    #[cfg(any(test, feature = "test-support"))]
    pub(in crate::admission) fn unowned(
        pid: u32,
        admitted: Option<crate::admission::Peer>,
    ) -> io::Result<Self> {
        let pid = libc::pid_t::try_from(pid).map_err(|_| io::Error::other("pid out of range"))?;
        let mut child = Self::started(pid, sys::pidfd_open(pid)?);
        child.reaped = true;
        child.admitted = admitted;
        Ok(child)
    }

    pub(in crate::admission) fn id(&self) -> u32 {
        self.pid as u32
    }

    /// Nothing to do: the child was started from the verified bytes and runs already.
    pub(in crate::admission) fn release(&self) -> io::Result<()> {
        Ok(())
    }

    /// Whether the process has not exited. A process that has exited and is not
    /// reaped yet (it still has its PID) is not alive.
    pub(super) fn alive(&self) -> io::Result<bool> {
        if sys::exited(self.pidfd.as_fd())? {
            return Ok(false);
        }
        match sys::pidfd_send_signal(self.pidfd.as_fd(), 0) {
            Ok(()) => Ok(true),
            Err(error) if error.raw_os_error() == Some(libc::ESRCH) => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// Whether it has exited (it is reaped if so). Doesn't wait.
    pub(in crate::admission) fn try_wait(&mut self) -> io::Result<bool> {
        if !self.reaped && sys::exited(self.pidfd.as_fd())? {
            self.reaped = self.wait(libc::WNOHANG)?;
        }
        Ok(self.reaped)
    }

    /// How it ended once reaped here, as fixed text: its exit code or signal.
    /// `None` while it runs, or when it was reaped elsewhere.
    pub(in crate::admission) fn ended(&self) -> Option<String> {
        let (code, status) = self.exit?;
        Some(match code {
            libc::CLD_EXITED => format!("exit code {status}"),
            libc::CLD_KILLED | libc::CLD_DUMPED => format!("signal {status}"),
            _ => format!("wait status {code}/{status}"),
        })
    }

    /// SIGKILLs it and reaps it.
    pub(in crate::admission) fn kill(&mut self) {
        if self.reaped {
            return;
        }
        // An error means it is gone already (reaped elsewhere); the wait says so.
        let _ = sys::pidfd_send_signal(self.pidfd.as_fd(), libc::SIGKILL);
        while !self.reaped {
            match self.wait(0) {
                Ok(reaped) => self.reaped = reaped,
                Err(_) => break,
            }
        }
    }

    /// `waitid` on the pidfd with `options`, retried on `EINTR`: whether nothing is
    /// left to wait for (the child was reaped here, or is not this process's to reap
    /// any more).
    fn wait(&mut self, options: libc::c_int) -> io::Result<bool> {
        loop {
            // SAFETY: all-zero is a valid `siginfo_t` for the call to overwrite.
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            // SAFETY: the pidfd of this process's own child and a valid out-value.
            let rc = unsafe {
                libc::waitid(
                    libc::P_PIDFD,
                    self.pidfd.as_raw_fd() as libc::id_t,
                    &mut info,
                    libc::WEXITED | options,
                )
            };
            if rc == 0 {
                // SAFETY: `waitid` succeeded, so `info` is the `SIGCHLD` variant; a
                // zero PID in it means no child had exited (`WNOHANG`).
                let (pid, status) = unsafe { (info.si_pid(), info.si_status()) };
                if pid == 0 {
                    return Ok(false);
                }
                self.exit = Some((info.si_code, status));
                return Ok(true);
            }
            let error = io::Error::last_os_error();
            match error.raw_os_error() {
                Some(libc::EINTR) => {}
                // Reaped elsewhere: gone, and never to be signalled again.
                Some(libc::ECHILD) => return Ok(true),
                _ => return Err(named("waitid", error)),
            }
        }
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        self.kill();
    }
}
