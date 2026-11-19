//! The adapter against the running kernel. A confined process cannot be unconfined,
//! so each test starts its own child: this test executable again, with a role in its
//! environment. The role runs before the test harness starts (an `.init_array`
//! entry), on the main thread of a process that has no other thread and holds exactly
//! descriptors 0 to 2, as the web process is when it calls `confine_web`. The child
//! reports on its standard output and never reaches the harness.
//!
//! The filter must hold every guarantee by itself, so what a confined process can
//! and cannot do is tested four times ([`LAYERINGS`]): with Landlock under the
//! filter, and with the filter alone, Landlock refused at each of its three calls
//! by a profile as a container runtime's.
//!
//! The pure parts (the Landlock rights of each ABI, the filter program, the entry
//! and status parsers) are tested beside their code.

use std::ffi::CString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::linux::net::SocketAddrExt;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::{SocketAddr, UnixDatagram, UnixListener, UnixStream};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::panic;
use std::path::{Path, PathBuf};
use std::process::{ChildStderr, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use super::{landlock, profile, status, syscalls, Unavailable};
use crate::confinement::testing::RefusedCall;
use crate::confinement::{self, Enforcement, Layers, REFUSAL};

const WAIT: Duration = Duration::from_secs(10);

/// The environment variable that makes this executable a child, and names its role.
const ROLE: &str = "ORG_PLATFORM_CONFINEMENT_CHILD";
/// The child's rendezvous path, the fixture's file, and one more word for its role.
const RENDEZVOUS: &str = "ORG_PLATFORM_CONFINEMENT_RENDEZVOUS";
const FILE: &str = "ORG_PLATFORM_CONFINEMENT_FILE";
const WORD: &str = "ORG_PLATFORM_CONFINEMENT_WORD";
/// The address of the fixture's TCP listener, which nothing confined may reach.
const TARGET: &str = "ORG_PLATFORM_CONFINEMENT_TARGET";

#[used]
#[unsafe(link_section = ".init_array")]
static CHILD: extern "C" fn() = child;

/// Runs before `main`. Without a role it returns and the tests run; with one it is
/// the child, and ends the process when the role returns.
extern "C" fn child() {
    let Some(role) = std::env::var_os(ROLE) else {
        return;
    };
    // The loader has initialized the C library, and the test harness has yet to
    // start: this is the process's only thread. A panic is printed on standard
    // error by the default hook and ends the child with status 101.
    let code = panic::catch_unwind(|| run(role.to_str().unwrap(), &Report)).unwrap_or(101);
    // SAFETY: ends the child without returning into the harness.
    unsafe { libc::_exit(code) }
}

fn variable(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("{name} is not set")))
}

fn word() -> String {
    variable(WORD).into_os_string().into_string().unwrap()
}

/// What the child writes to its standard output.
struct Report;

impl Report {
    fn line(&self, text: &str) {
        let line = format!("{text}\n");
        // SAFETY: a live buffer of the given length; descriptor 1 is the child's
        // report pipe.
        unsafe { libc::write(1, line.as_ptr().cast(), line.len()) };
    }

    /// The outcome of an operation that should have failed or killed: its error
    /// number, or that it succeeded.
    fn outcome(&self, name: &str, result: libc::c_long) {
        if result < 0 {
            let errno = io::Error::last_os_error().raw_os_error().unwrap_or(0);
            self.line(&format!("{name}: error {errno}"));
        } else {
            self.line(&format!("{name}: succeeded"));
        }
    }
}

/// A started child, the reading end of its report, and its standard error.
struct Child {
    process: std::process::Child,
    report: ChildStdout,
    errors: ChildStderr,
}

/// How a child ended.
#[derive(Debug, PartialEq, Eq)]
enum Ended {
    Exited(i32),
    Signaled(i32),
}

impl Child {
    /// Starts this executable as the child `role`, with `word` for the role's own
    /// use. It gets standard input, output and error and no other descriptor,
    /// whatever this process holds, and no `RUST_BACKTRACE`, as the host starts the
    /// web process.
    fn start(role: &str, fixture: &Fixture, word: &str) -> Self {
        Self::start_under(None, role, fixture, word)
    }

    /// The same, under a profile that refuses `refused` where one is named.
    fn start_under(
        refused: Option<RefusedCall>,
        role: &str,
        fixture: &Fixture,
        word: &str,
    ) -> Self {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .env(ROLE, role)
            .env(RENDEZVOUS, &fixture.rendezvous)
            .env(FILE, &fixture.file)
            .env(WORD, word)
            .env(TARGET, fixture.target.local_addr().unwrap().to_string())
            .env_remove("RUST_BACKTRACE")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // SAFETY: the hook makes one system call between `fork` and `exec`: every
        // descriptor from 3 up is closed by the `exec`.
        unsafe {
            command.pre_exec(|| {
                match libc::close_range(3, libc::c_uint::MAX, libc::CLOSE_RANGE_CLOEXEC as i32) {
                    0 => Ok(()),
                    _ => Err(io::Error::last_os_error()),
                }
            });
        }
        if let Some(call) = refused {
            profile::refuse_call(&mut command, call).unwrap();
        }
        let mut process = command.spawn().unwrap();
        let report = process.stdout.take().unwrap();
        let errors = process.stderr.take().unwrap();
        Self {
            process,
            report,
            errors,
        }
    }

    fn id(&self) -> u32 {
        self.process.id()
    }

    fn pid(&self) -> libc::pid_t {
        self.process.id() as libc::pid_t
    }

    /// Whether the child has ended, without waiting.
    fn ended(&mut self) -> bool {
        self.process.try_wait().unwrap().is_some()
    }

    /// Reads exactly `expected` from the child's report: the child got that far.
    fn reported(&mut self, expected: &str) {
        let mut bytes = vec![0u8; expected.len()];
        self.report
            .read_exact(&mut bytes)
            .unwrap_or_else(|error| panic!("the child did not report {expected:?}: {error}"));
        assert_eq!(String::from_utf8_lossy(&bytes), expected);
    }

    /// Waits for the child and returns how it ended with the rest of its report.
    /// What it wrote to standard error, a panic's message for one, goes to this
    /// test's own output.
    fn finish(mut self) -> (Ended, String) {
        let (mut report, mut errors) = (String::new(), String::new());
        self.report.read_to_string(&mut report).unwrap();
        self.errors.read_to_string(&mut errors).unwrap();
        if !errors.is_empty() {
            eprintln!("the child's standard error:\n{errors}");
        }
        let status = self.process.wait().unwrap();
        let ended = match status.signal() {
            Some(signal) => Ended::Signaled(signal),
            None => Ended::Exited(status.code().unwrap()),
        };
        (ended, report)
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

/// A temporary directory with a rendezvous socket and a file, and what a confined
/// process must never reach: a TCP listener and a named Unix datagram socket.
struct Fixture {
    _directory: tempfile::TempDir,
    rendezvous: PathBuf,
    file: PathBuf,
    listener: UnixListener,
    target: TcpListener,
    datagrams: UnixDatagram,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::Builder::new()
            .prefix("oc")
            .tempdir_in("/tmp")
            .unwrap();
        let rendezvous = directory.path().join("r");
        let file = directory.path().join("file");
        std::fs::write(&file, b"retained").unwrap();
        let listener = UnixListener::bind(&rendezvous).unwrap();
        listener.set_nonblocking(true).unwrap();
        let target = TcpListener::bind("127.0.0.1:0").unwrap();
        target.set_nonblocking(true).unwrap();
        let datagrams = UnixDatagram::bind(directory.path().join("datagrams")).unwrap();
        datagrams.set_nonblocking(true).unwrap();
        Self {
            _directory: directory,
            rendezvous,
            file,
            listener,
            target,
            datagrams,
        }
    }

    /// What reached the TCP listener and the datagram socket: the bytes of the first
    /// connection, if one was made, and of the first datagram, if one arrived.
    fn reached(&self) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
        let connection = self.target.accept().ok().map(|(mut stream, _)| {
            stream.set_nonblocking(false).unwrap();
            stream.set_read_timeout(Some(WAIT)).unwrap();
            let mut bytes = [0u8; 16];
            let read = stream.read(&mut bytes).unwrap_or(0);
            bytes[..read].to_vec()
        });
        let mut bytes = [0u8; 16];
        let datagram = self
            .datagrams
            .recv(&mut bytes)
            .ok()
            .map(|read| bytes[..read].to_vec());
        (connection, datagram)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.file.with_file_name(name)
    }

    /// An address in the abstract namespace that only this fixture uses.
    fn abstract_address(&self, name: &str) -> SocketAddr {
        abstract_address(&self.path(name))
    }

    /// The child's connection, or `None` when it ended without connecting.
    fn accept(&self, child: &mut Child) -> Option<UnixStream> {
        let until = Instant::now() + WAIT;
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    stream.set_read_timeout(Some(WAIT)).unwrap();
                    return Some(stream);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    // Look once more after the child has ended: it may have
                    // connected just before.
                    if child.ended() {
                        return self.listener.accept().ok().map(|(stream, _)| stream);
                    }
                    assert!(Instant::now() < until, "the child did not connect");
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(error) => panic!("accept: {error}"),
            }
        }
    }
}

/// The abstract Unix address named by `path`'s bytes. No file is made.
fn abstract_address(path: &Path) -> SocketAddr {
    SocketAddr::from_abstract_name(path.as_os_str().as_bytes()).unwrap()
}

fn c_path(path: &Path) -> CString {
    CString::new(path.as_os_str().as_bytes()).unwrap()
}

/// A message of one data byte with room for one descriptor (`SCM_RIGHTS`), over
/// buffers the caller keeps alive.
fn descriptor_message(
    byte: &mut [u8; 1],
    iov: &mut libc::iovec,
    control: &mut [u64; 4],
) -> libc::msghdr {
    iov.iov_base = byte.as_mut_ptr().cast();
    iov.iov_len = 1;
    // SAFETY: every bit pattern is a valid msghdr; CMSG_SPACE is arithmetic.
    let (mut message, space): (libc::msghdr, _) =
        unsafe { (std::mem::zeroed(), libc::CMSG_SPACE(4) as usize) };
    assert!(space <= std::mem::size_of_val(control));
    message.msg_iov = iov;
    message.msg_iovlen = 1;
    message.msg_control = control.as_mut_ptr().cast();
    message.msg_controllen = space as _;
    message
}

/// Sends one descriptor with one data byte.
fn send_descriptor(stream: &UnixStream, descriptor: RawFd) {
    let (mut byte, mut control) = ([7u8], [0u64; 4]);
    let mut iov = libc::iovec {
        iov_base: std::ptr::null_mut(),
        iov_len: 0,
    };
    let message = descriptor_message(&mut byte, &mut iov, &mut control);
    // SAFETY: the control buffer is aligned and holds one header and one
    // descriptor; the message and its buffers live through sendmsg.
    unsafe {
        let header = libc::CMSG_FIRSTHDR(&message);
        (*header).cmsg_level = libc::SOL_SOCKET;
        (*header).cmsg_type = libc::SCM_RIGHTS;
        (*header).cmsg_len = libc::CMSG_LEN(4) as _;
        libc::CMSG_DATA(header)
            .cast::<RawFd>()
            .write_unaligned(descriptor);
        assert_eq!(libc::sendmsg(stream.as_raw_fd(), &message, 0), 1);
    }
}

/// Receives the descriptor [`send_descriptor`] sent. `recvmsg` is on the list.
fn receive_descriptor(stream: &UnixStream) -> OwnedFd {
    let (mut byte, mut control) = ([0u8], [0u64; 4]);
    let mut iov = libc::iovec {
        iov_base: std::ptr::null_mut(),
        iov_len: 0,
    };
    let mut message = descriptor_message(&mut byte, &mut iov, &mut control);
    // SAFETY: the buffers live through recvmsg; the header is read only after the
    // kernel filled the control buffer, and carries one new descriptor.
    unsafe {
        assert_eq!(
            libc::recvmsg(stream.as_raw_fd(), &mut message, libc::MSG_CMSG_CLOEXEC),
            1
        );
        let header = libc::CMSG_FIRSTHDR(&message);
        assert!(!header.is_null());
        assert_eq!((*header).cmsg_type, libc::SCM_RIGHTS);
        OwnedFd::from_raw_fd(libc::CMSG_DATA(header).cast::<RawFd>().read_unaligned())
    }
}

/// What a process says of the confinement it applied: the layers, and why Landlock
/// is not among them.
fn confined_report(enforcement: &Enforcement, not_applied: Option<&str>) -> String {
    format!(
        "confined: landlock {:?} seccomp {} not applied {:?}",
        enforcement.landlock_abi, enforcement.seccomp, not_applied
    )
}

/// The child's half of every confined test: confine, then report what was enforced.
fn confine(report: &Report, rendezvous: &Path) -> UnixStream {
    let confined = confinement::confine_web(rendezvous).unwrap();
    report.line(&confined_report(
        &confined.enforcement,
        confined.landlock_not_applied.as_deref(),
    ));
    confined.stream
}

/// How the tests confine a process: with whatever this kernel offers, which is both
/// layers where it has Landlock, and under a profile that refuses Landlock at its
/// version query, at the creation of the ruleset and at the restriction, which
/// leaves the filter alone each time.
const LAYERINGS: [Option<RefusedCall>; 4] = [
    None,
    Some(RefusedCall::LandlockCreateRuleset),
    Some(RefusedCall::LandlockRuleset),
    Some(RefusedCall::LandlockRestrictSelf),
];

/// How a refused call's error reads in a reason.
const NOT_PERMITTED: &str = "EPERM (Operation not permitted (os error 1))";
const VERSION_QUERY_REFUSED: &str =
    "landlock_create_ruleset failed with EPERM: the Landlock version query was refused";

/// The Landlock ABI a process confines itself under on this kernel, or why it has
/// none. The tests run wherever the web process does, with or without Landlock.
fn kernel_landlock() -> Result<u32, String> {
    landlock::abi()
        .map(landlock::enforced_abi)
        .map_err(|reason| reason.to_string())
}

/// What a child that confined itself reports, here or under a profile.
fn confined_line_under(refused: Option<RefusedCall>) -> String {
    let landlock = match (refused, kernel_landlock()) {
        (None, kernel) => kernel,
        (Some(RefusedCall::LandlockCreateRuleset), _) => Err(VERSION_QUERY_REFUSED.to_owned()),
        // The version query is answered before the other two calls are made: a
        // kernel without Landlock says so itself.
        (Some(RefusedCall::LandlockRuleset | RefusedCall::LandlockRestrictSelf), Err(reason)) => {
            Err(reason)
        }
        (Some(RefusedCall::LandlockRuleset), Ok(_)) => Err(format!(
            "landlock_create_ruleset failed with {NOT_PERMITTED}"
        )),
        (Some(RefusedCall::LandlockRestrictSelf), Ok(_)) => Err(format!(
            "landlock_restrict_self failed with {NOT_PERMITTED}"
        )),
        (Some(other), _) => panic!("{other:?} leaves nothing confined"),
    };
    let enforcement = Enforcement {
        landlock_abi: landlock.as_ref().ok().copied(),
        seccomp: true,
    };
    format!(
        "{}\n",
        confined_report(&enforcement, landlock.as_ref().err().map(String::as_str))
    )
}

/// What a child that confined itself on this kernel reports.
fn confined_line() -> String {
    confined_line_under(None)
}

fn own_status() -> String {
    std::fs::read_to_string("/proc/self/status").unwrap()
}

/// The lines of a process's own status that confinement changes, and whether it can
/// still read a file.
fn freedom(file: &Path) -> String {
    let status = own_status();
    format!(
        "privileges {:?} seccomp {:?} file {:?}",
        status::field(&status, "NoNewPrivs"),
        status::field(&status, "Seccomp"),
        std::fs::read(file).map(|bytes| bytes.len()).ok(),
    )
}

/// What [`freedom`] says of a process that confined nothing, where the tests
/// themselves run without a filter.
const FREE: &str = "privileges Some(\"0\") seccomp Some(\"0\") file Some(8)";

/// Whether this test process runs without no-new-privileges and without a filter.
/// A child inherits both, so only then is "nothing was confined" an exact state.
fn runs_unfiltered() -> bool {
    let status = own_status();
    status::field(&status, "NoNewPrivs") == Some("0")
        && status::field(&status, "Seccomp") == Some("0")
}

#[test]
fn this_system_supports_the_web_process() {
    confinement::web_confinement_supported().unwrap();
    let arch = super::available().unwrap();
    assert_eq!(arch.audit, syscalls::native().unwrap().audit);
}

#[test]
fn support_is_decided_by_the_architecture_and_the_filter_and_never_by_landlock() {
    // `decide` takes no Landlock answer at all: what it is given is everything the
    // host's question depends on.
    let native = syscalls::native();
    assert!(super::decide(native, Ok(()), Ok(())).is_ok());
    let refusal = super::decide(None, Ok(()), Ok(())).unwrap_err();
    assert_eq!(refusal, Unavailable::Architecture);
    assert!(refusal
        .to_string()
        .starts_with("no system call list for the "));
    for (errno, name) in [
        (libc::EPERM, "EPERM"),
        (libc::ENOSYS, "ENOSYS"),
        (libc::EOPNOTSUPP, "EOPNOTSUPP"),
    ] {
        let refusal = super::decide(native, Err(errno), Ok(())).unwrap_err();
        assert_eq!(refusal, Unavailable::Seccomp(errno));
        assert_eq!(
            refusal.to_string(),
            format!(
                "seccomp failed with {name}: this kernel or its profile has no seccomp filter \
                 that kills the process"
            )
        );
    }
    for (errno, name) in [
        (libc::EPERM, "EPERM"),
        (libc::ENOSYS, "ENOSYS"),
        (libc::EINVAL, "EINVAL"),
        (0, "error 0"),
    ] {
        let refusal = super::decide(native, Ok(()), Err(errno)).unwrap_err();
        assert_eq!(refusal, Unavailable::Install(errno));
        assert_eq!(
            refusal.to_string(),
            format!(
                "seccomp failed with {name}: this kernel or its profile does not let a \
                 process install a seccomp filter on all its threads"
            )
        );
    }
    // The architecture is named first, then the kill action.
    assert_eq!(
        super::decide(None, Err(libc::ENOSYS), Err(libc::ENOSYS)).unwrap_err(),
        Unavailable::Architecture
    );
    assert_eq!(
        super::decide(native, Err(libc::ENOSYS), Err(libc::EPERM)).unwrap_err(),
        Unavailable::Seccomp(libc::ENOSYS)
    );
}

/// A report as the host builds it: the ready line has no seccomp field.
fn report_of(abi: u32) -> Enforcement {
    Enforcement {
        landlock_abi: Some(abi),
        seccomp: false,
    }
}

/// The host's report of a web process that says it applied no Landlock domain.
const FILTER_ALONE: Enforcement = Enforcement {
    landlock_abi: None,
    seccomp: false,
};

#[test]
fn a_report_is_accepted_with_or_without_landlock_and_ignores_seccomp() {
    for seccomp in [false, true] {
        for landlock_abi in [None, Some(1), Some(2), Some(3), Some(4), Some(7), Some(9)] {
            confinement::verify_ready_report(&Enforcement {
                landlock_abi,
                seccomp,
            })
            .unwrap();
        }
        let error = confinement::verify_ready_report(&Enforcement {
            landlock_abi: Some(0),
            seccomp,
        })
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("{REFUSAL}: the ready line reports Landlock ABI 0, which is no ABI")
        );
    }
}

#[test]
fn the_layers_are_the_filter_and_landlock_where_it_is_reported() {
    assert_eq!(
        confinement::layers(&FILTER_ALONE).unwrap(),
        Layers {
            description: "the seccomp filter alone".to_owned(),
            landlock_missing: true,
        }
    );
    for abi in [1, 3, 7] {
        assert_eq!(
            confinement::layers(&report_of(abi)).unwrap(),
            Layers {
                description: format!("the seccomp filter and Landlock ABI {abi}"),
                landlock_missing: false,
            }
        );
    }
    let error = confinement::layers(&report_of(0)).unwrap_err();
    assert_eq!(
        error.to_string(),
        format!("{REFUSAL}: the ready line reports Landlock ABI 0, which is no ABI")
    );
}

#[test]
fn a_failed_allocator_limit_refuses_naming_the_call() {
    // glibc's mallopt returns 1 for this parameter whatever the process's state, so
    // the refusal is tested on the decision alone: nothing here provokes the call
    // itself to fail.
    super::one_arena(1).unwrap();
    for failed in [0, -1, 2] {
        let error = super::one_arena(failed).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("{REFUSAL}: mallopt(M_ARENA_MAX, 1) failed")
        );
        assert_eq!(error.raw_os_error(), None);
    }
}

#[test]
fn a_relative_rendezvous_is_refused_before_anything_changes() {
    let before = own_status();
    let error = confinement::confine_web(Path::new("relative/r")).unwrap_err();
    assert_eq!(
        error.to_string(),
        format!("{REFUSAL}: the rendezvous path is not absolute")
    );
    assert_eq!(error.raw_os_error(), None);
    assert_eq!(
        status::field(&before, "NoNewPrivs"),
        status::field(&own_status(), "NoNewPrivs")
    );
}

/// Grows a vector and a string to megabytes by appending, never asking for the
/// room first.
fn grow() {
    let mut bytes = Vec::new();
    for value in 0..(3u32 << 20) {
        bytes.push(value as u8);
    }
    let mut text = String::new();
    while text.len() < 2 << 20 {
        text.push_str("grown by appending ");
    }
    assert_eq!(bytes.len(), 3 << 20);
    assert_eq!(bytes[(1 << 20) + 5], 5);
    assert!(text.ends_with("appending "));
}

/// Four senders and four receivers on one bounded channel and one that hands each
/// value over directly, as a pool of workers behind a queue does.
fn contend() {
    for capacity in [0, 1] {
        let (send, receive) = std::sync::mpsc::sync_channel::<u32>(capacity);
        let receive = std::sync::Arc::new(std::sync::Mutex::new(receive));
        let receivers: Vec<_> = (0..4)
            .map(|_| {
                let receive = std::sync::Arc::clone(&receive);
                std::thread::spawn(move || {
                    let mut sum = 0u64;
                    loop {
                        let next = receive.lock().unwrap().recv();
                        match next {
                            Ok(value) => sum += u64::from(value),
                            Err(_) => return sum,
                        }
                    }
                })
            })
            .collect();
        let senders: Vec<_> = (0..4)
            .map(|_| {
                let send = send.clone();
                std::thread::spawn(move || (0..20_000).for_each(|value| send.send(value).unwrap()))
            })
            .collect();
        drop(send);
        senders
            .into_iter()
            .for_each(|sender| sender.join().unwrap());
        let sum: u64 = receivers.into_iter().map(|r| r.join().unwrap()).sum();
        assert_eq!(sum, 4 * (0..20_000u64).sum::<u64>());
    }
}

/// The child: one role, chosen by the environment. Returns its exit status.
fn run(role: &str, report: &Report) -> i32 {
    let (rendezvous, file) = (variable(RENDEZVOUS), variable(FILE));
    match role {
        "use" => {
            let mut stream = confine(report, &rendezvous);
            let mut ping = [0u8; 4];
            stream.read_exact(&mut ping).unwrap();
            assert_eq!(&ping, b"ping");
            // A thread: glibc tries clone3, is answered ENOSYS, and calls clone.
            let started = Instant::now();
            let sum = std::thread::Builder::new()
                .name("confined".into())
                .spawn(|| {
                    let block = vec![1u8; 16 << 20];
                    block.iter().map(|byte| u64::from(*byte)).sum::<u64>()
                })
                .unwrap()
                .join()
                .unwrap();
            assert_eq!(sum, 16 << 20);
            assert!(started.elapsed() < WAIT);
            // The clock as a system call, not only through the vDSO.
            let mut time = libc::timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            // SAFETY: a valid out structure for the raw clock_gettime call.
            let read =
                unsafe { libc::syscall(libc::SYS_clock_gettime, libc::CLOCK_MONOTONIC, &mut time) };
            assert_eq!(read, 0);
            let mut random = [0u8; 64];
            confinement::fill_entropy(&mut random).unwrap();
            assert_ne!(random, [0u8; 64]);
            // Buffers that grow a push at a time, on this thread and on another:
            // past about 128 KiB the C library moves the block with mremap.
            grow();
            std::thread::spawn(grow).join().unwrap();
            report.line("grew");
            // Channels under contention, as each of the web process's requests
            // uses them: a waiting side yields its time slice (sched_yield).
            contend();
            report.line("contended");
            // A caught panic: the message goes to standard error, and unwinding
            // needs no forbidden call.
            assert!(panic::catch_unwind(|| panic!("a confined process caught this")).is_err());
            stream.write_all(b"pong").unwrap();
            report.line("done");
        }
        "serve" => {
            let stream = confine(report, &rendezvous);
            let listener = TcpListener::from(receive_descriptor(&stream));
            let (mut client, _) = listener.accept().unwrap();
            let mut request = [0u8; 5];
            client.read_exact(&mut request).unwrap();
            assert_eq!(&request, b"GET /");
            client.write_all(b"200 OK").unwrap();
            client.shutdown(std::net::Shutdown::Both).unwrap();
            // A second connection on the same listener, made non-blocking as the
            // runtime makes it.
            let (mut client, _) = listener.accept().unwrap();
            client.set_nonblocking(true).unwrap();
            client.write_all(b"again").unwrap();
            report.line("served");
        }
        "forbidden" => {
            // What the attempt needs is asked before confinement: asking for the
            // parent's id is itself not on the list, and each attempt must be
            // stopped at the call it names.
            let paths = Paths::new(&file);
            let stream = confine(report, &rendezvous);
            let socket = receive_descriptor(&stream);
            let name = word();
            report.outcome(&name, attempt(&name, &paths, socket.as_raw_fd()));
        }
        "candidates" => {
            // SAFETY: asks for the parent's id, while asking is still allowed.
            let parent = unsafe { libc::getppid() };
            let stream = match word().as_str() {
                "confined" => confine(report, &rendezvous),
                "landlock alone" => {
                    let stream = UnixStream::connect(&rendezvous).unwrap();
                    super::no_new_privileges().unwrap();
                    landlock::apply().unwrap();
                    report.line("landlock alone");
                    stream
                }
                "unconfined" => {
                    let stream = UnixStream::connect(&rendezvous).unwrap();
                    report.line("unconfined");
                    stream
                }
                other => panic!("an unknown way to run the candidates: {other}"),
            };
            candidates(report, &stream, parent);
        }
        "unconfined" => {
            // The same attempt with nothing confined.
            let name = word();
            let (family, kind) = if wants_a_datagram_socket(&name) {
                (libc::AF_UNIX, libc::SOCK_DGRAM)
            } else {
                (libc::AF_INET, libc::SOCK_STREAM)
            };
            // SAFETY: a plain socket call; the result is checked and owned at once.
            let socket = unsafe {
                let socket = libc::socket(family, kind, 0);
                assert!(socket >= 0);
                OwnedFd::from_raw_fd(socket)
            };
            report.outcome(
                &name,
                attempt(&name, &Paths::new(&file), socket.as_raw_fd()),
            );
        }
        "forbidden on a thread" => {
            let _stream = confine(report, &rendezvous);
            let read = std::thread::spawn(move || std::fs::read(file).map(drop)).join();
            report.line(&format!("the thread returned: {read:?}"));
        }
        "abort" => {
            let _stream = confine(report, &rendezvous);
            std::process::abort();
        }
        "timed wait" => {
            let mut stream = confine(report, &rendezvous);
            stream.write_all(b"w").unwrap();
            let mut slot = libc::pollfd {
                fd: stream.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: one valid poll slot and a finite timeout.
            let ready = unsafe { libc::poll(&mut slot, 1, 20_000) };
            report.line(&format!("poll returned {ready}"));
        }
        "second thread" => {
            let (release, parked) = std::sync::mpsc::channel::<()>();
            let second = std::thread::spawn(move || parked.recv());
            refused_at_entry(report, &rendezvous, &file);
            drop(release);
            let _ = second.join();
        }
        "another descriptor" => {
            let held = File::open(&file).unwrap();
            assert_eq!(held.as_raw_fd(), 3);
            refused_at_entry(report, &rendezvous, &file);
        }
        "no standard input" => {
            // SAFETY: closes this child's own standard input.
            unsafe { libc::close(0) };
            refused_at_entry(report, &rendezvous, &file);
        }
        "missing rendezvous" => {
            let error =
                confinement::confine_web(&rendezvous.with_file_name("missing")).unwrap_err();
            report.line(&format!("os error: {:?}", error.raw_os_error()));
            report.line(&freedom(&file));
        }
        "landlock alone" => landlock_alone(report, &file),
        "under a profile" => {
            // The word names the refused call and its error number.
            let landlock = |errno| profile::refusing(&[libc::SYS_landlock_create_ruleset], errno);
            let program = match word().as_str() {
                "create EPERM" => landlock(libc::EPERM),
                "create ENOSYS" => landlock(libc::ENOSYS),
                "create EOPNOTSUPP" => landlock(libc::EOPNOTSUPP),
                "ruleset EPERM" => profile::profile(RefusedCall::LandlockRuleset),
                "restrict EPERM" => profile::profile(RefusedCall::LandlockRestrictSelf),
                "seccomp EPERM" => profile::profile(RefusedCall::Seccomp),
                "filter EPERM" => profile::refusing_when(
                    &[(
                        libc::SYS_seccomp,
                        profile::When::Argument(0, libc::SECCOMP_SET_MODE_FILTER),
                    )],
                    libc::EPERM,
                ),
                "install ENOMEM" => profile::profile(RefusedCall::SeccompInstall),
                "landlock and seccomp EPERM" => profile::refusing(
                    &[
                        libc::SYS_landlock_create_ruleset,
                        libc::SYS_seccomp,
                        libc::SYS_prctl,
                    ],
                    libc::EPERM,
                ),
                other => panic!("an unknown profile: {other}"),
            };
            profile::enter(&program).unwrap();
            report.line(&format!(
                "supported: {:?}",
                super::supported().map_err(|refused| (refused.to_string(), refused.raw_os_error()))
            ));
            match confinement::confine_web(&rendezvous) {
                Ok(confined) => report.line(&confined_report(
                    &confined.enforcement,
                    confined.landlock_not_applied.as_deref(),
                )),
                Err(error) => {
                    report.line(&format!("{error}"));
                    report.line(&format!("os error: {:?}", error.raw_os_error()));
                    report.line(&format!(
                        "file: {:?}",
                        std::fs::read(&file)
                            .map(|bytes| bytes.len())
                            .map_err(|error| error.raw_os_error())
                    ));
                }
            }
        }
        "stay confined" => {
            let mut stream = confine(report, &rendezvous);
            // Confined and alive until the test has looked.
            let _ = stream.read(&mut [0u8]);
        }
        "privileges only" => {
            super::no_new_privileges().unwrap();
            report.line("ready");
            loop {
                // SAFETY: suspends this child until the test ends it.
                unsafe { libc::pause() };
            }
        }
        "exit" => (),
        "host under a profile" => host_under_a_profile(report, &rendezvous, &word()),
        other => panic!("an unknown role: {other}"),
    }
    0
}

#[test]
fn a_confined_process_uses_its_stream_threads_memory_clock_and_randomness() {
    for refused in LAYERINGS {
        let fixture = Fixture::new();
        let mut child = Child::start_under(refused, "use", &fixture, "");
        let mut stream = fixture.accept(&mut child).unwrap();
        stream.write_all(b"ping").unwrap();
        // The child answers before it ends, so the answer is read once it has ended:
        // its work takes longer than the stream's read timeout on a loaded machine.
        let (ended, report) = child.finish();
        let mut pong = [0u8; 4];
        let answered = stream.read_exact(&mut pong);
        // The report first: it says how far a child that died had come.
        assert_eq!(
            report,
            format!("{}grew\ncontended\ndone\n", confined_line_under(refused)),
            "{refused:?}"
        );
        assert_eq!(ended, Ended::Exited(0), "{refused:?}");
        answered.unwrap();
        assert_eq!(&pong, b"pong");
    }
}

#[test]
fn a_confined_process_serves_a_listener_it_was_handed() {
    for refused in LAYERINGS {
        let fixture = Fixture::new();
        let mut child = Child::start_under(refused, "serve", &fixture, "");
        let stream = fixture.accept(&mut child).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        send_descriptor(&stream, listener.as_raw_fd());
        drop(listener);
        let mut browser = TcpStream::connect(address).unwrap();
        browser.set_read_timeout(Some(WAIT)).unwrap();
        browser.write_all(b"GET /").unwrap();
        let mut reply = Vec::new();
        browser.read_to_end(&mut reply).unwrap();
        assert_eq!(reply, b"200 OK");
        let mut browser = TcpStream::connect(address).unwrap();
        browser.set_read_timeout(Some(WAIT)).unwrap();
        let mut reply = Vec::new();
        browser.read_to_end(&mut reply).unwrap();
        assert_eq!(reply, b"again");
        let (ended, report) = child.finish();
        assert_eq!(
            report,
            format!("{}served\n", confined_line_under(refused)),
            "{refused:?}"
        );
        assert_eq!(ended, Ended::Exited(0), "{refused:?}");
    }
}

/// What the forbidden calls act on.
struct Paths {
    file: CString,
    created: CString,
    address: libc::sockaddr_in,
    datagrams: libc::sockaddr_un,
    parent: libc::pid_t,
}

impl Paths {
    /// The fixture's file, a name beside it that does not exist, the address of its
    /// TCP listener, the name of its datagram socket and the parent's process id.
    /// Made before confinement: asking for the parent is not on the list.
    fn new(file: &Path) -> Self {
        let std::net::SocketAddr::V4(target) = variable(TARGET).to_str().unwrap().parse().unwrap()
        else {
            panic!("the target is an IPv4 address");
        };
        // SAFETY: every bit pattern is a valid sockaddr_un.
        let mut datagrams: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        datagrams.sun_family = libc::AF_UNIX as libc::sa_family_t;
        let name = file.with_file_name("datagrams");
        let name = name.as_os_str().as_bytes();
        assert!(name.len() < datagrams.sun_path.len());
        for (to, from) in datagrams.sun_path.iter_mut().zip(name) {
            *to = *from as libc::c_char;
        }
        Self {
            file: c_path(file),
            created: c_path(&file.with_file_name("created")),
            address: libc::sockaddr_in {
                sin_family: libc::AF_INET as libc::sa_family_t,
                sin_port: target.port().to_be(),
                sin_addr: libc::in_addr {
                    s_addr: u32::from(*target.ip()).to_be(),
                },
                sin_zero: [0; 8],
            },
            datagrams,
            // SAFETY: takes no argument and cannot fail.
            parent: unsafe { libc::getppid() },
        }
    }
}

/// The forbidden calls that are made on a Unix datagram socket instead of a TCP one.
fn wants_a_datagram_socket(name: &str) -> bool {
    name.contains("Unix datagram socket")
}

/// What a confined process must not do. Each is one system call that is not on the
/// list, or is on it with other arguments.
const FORBIDDEN: &[&str] = &[
    "open a file for reading",
    "create a file",
    "truncate a file by path",
    "rename a file",
    "remove a file",
    "change a file's mode",
    "read a file's metadata",
    "read its own environment",
    "create a Unix socket",
    "create a TCP socket",
    "create a socket pair",
    "connect a socket it was handed",
    "bind a socket it was handed",
    "listen on a socket it was handed",
    "send to a destination on a TCP socket it was handed",
    "send with fast open to a destination on a TCP socket it was handed",
    "send with a flag other than the two it may carry",
    "send to a destination on a Unix datagram socket it was handed",
    "call sendmsg",
    "start a program",
    "fork",
    "trace a process",
    "signal its parent",
    "signal a thread of its parent",
    "become dumpable again",
    "ask for its seccomp mode",
    "an ioctl other than the two socket requests",
    "make a descriptor signal its parent",
    "set a descriptor's flags with fcntl",
    "map executable memory",
    "make memory executable",
    "call seccomp again",
    "ask for the Landlock version",
    "create a memory file",
    "create a pipe",
    "duplicate a descriptor with dup",
    "leave its namespaces",
    "set up io_uring",
    "call bpf",
    "mount",
    "read another process's memory",
    "sleep by a call that is not listed",
];

/// Makes the forbidden call `name`. `socket` is an unconnected TCP socket the child
/// was handed. Under the filter this does not return.
fn attempt(name: &str, paths: &Paths, socket: RawFd) -> libc::c_long {
    let address = (&paths.address as *const libc::sockaddr_in).cast();
    let length = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
    let program = [c"/bin/sh".as_ptr(), std::ptr::null()];
    // SAFETY: raw calls with live NUL-terminated paths, live out structures or
    // plain integers. Each is expected to kill this child before it returns; where
    // it returns instead, its result is only reported.
    unsafe {
        match name {
            "open a file for reading" => libc::open(paths.file.as_ptr(), libc::O_RDONLY).into(),
            "create a file" => libc::open(
                paths.created.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT,
                0o600,
            )
            .into(),
            "truncate a file by path" => libc::truncate(paths.file.as_ptr(), 0).into(),
            "rename a file" => libc::rename(paths.file.as_ptr(), paths.created.as_ptr()).into(),
            "remove a file" => libc::unlink(paths.file.as_ptr()).into(),
            "change a file's mode" => libc::chmod(paths.file.as_ptr(), 0o777).into(),
            "read a file's metadata" => {
                let mut metadata: libc::stat = std::mem::zeroed();
                libc::stat(paths.file.as_ptr(), &mut metadata).into()
            }
            "read its own environment" => {
                libc::open(c"/proc/self/environ".as_ptr(), libc::O_RDONLY).into()
            }
            "create a Unix socket" => libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0).into(),
            "create a TCP socket" => libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0).into(),
            "create a socket pair" => {
                let mut pair = [-1; 2];
                libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, pair.as_mut_ptr()).into()
            }
            "connect a socket it was handed" => libc::connect(socket, address, length).into(),
            "bind a socket it was handed" => libc::bind(socket, address, length).into(),
            "listen on a socket it was handed" => libc::listen(socket, 1).into(),
            "send to a destination on a TCP socket it was handed" => libc::sendto(
                socket,
                c"x".as_ptr().cast(),
                1,
                libc::MSG_NOSIGNAL,
                address,
                length,
            ) as libc::c_long,
            "send with fast open to a destination on a TCP socket it was handed" => libc::sendto(
                socket,
                c"x".as_ptr().cast(),
                1,
                libc::MSG_FASTOPEN,
                address,
                length,
            )
                as libc::c_long,
            "send with a flag other than the two it may carry" => libc::sendto(
                socket,
                c"x".as_ptr().cast(),
                1,
                libc::MSG_MORE,
                std::ptr::null(),
                0,
            ) as libc::c_long,
            "send to a destination on a Unix datagram socket it was handed" => libc::sendto(
                socket,
                c"x".as_ptr().cast(),
                1,
                libc::MSG_NOSIGNAL,
                (&paths.datagrams as *const libc::sockaddr_un).cast(),
                std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t,
            )
                as libc::c_long,
            "call sendmsg" => {
                let message: libc::msghdr = std::mem::zeroed();
                libc::sendmsg(socket, &message, libc::MSG_NOSIGNAL) as libc::c_long
            }
            "start a program" => {
                libc::execve(program[0], program.as_ptr(), program[1..].as_ptr()).into()
            }
            "fork" => libc::fork().into(),
            "trace a process" => libc::ptrace(libc::PTRACE_TRACEME, 0, 0, 0),
            "signal its parent" => libc::kill(paths.parent, 0).into(),
            "signal a thread of its parent" => {
                libc::syscall(libc::SYS_tgkill, paths.parent, paths.parent, 0)
            }
            "become dumpable again" => libc::prctl(libc::PR_SET_DUMPABLE, 1, 0, 0, 0).into(),
            "ask for its seccomp mode" => libc::prctl(libc::PR_GET_SECCOMP, 0, 0, 0, 0).into(),
            "an ioctl other than the two socket requests" => {
                let mut size: libc::winsize = std::mem::zeroed();
                libc::ioctl(0, libc::TIOCGWINSZ, &mut size).into()
            }
            "make a descriptor signal its parent" => {
                libc::fcntl(socket, libc::F_SETOWN, paths.parent).into()
            }
            "set a descriptor's flags with fcntl" => {
                libc::fcntl(socket, libc::F_SETFL, libc::O_ASYNC | libc::O_NONBLOCK).into()
            }
            "map executable memory" => libc::mmap(
                std::ptr::null_mut(),
                4096,
                libc::PROT_READ | libc::PROT_EXEC,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            ) as libc::c_long,
            "make memory executable" => {
                let page = libc::mmap(
                    std::ptr::null_mut(),
                    4096,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                    -1,
                    0,
                );
                assert_ne!(page, libc::MAP_FAILED);
                libc::mprotect(page, 4096, libc::PROT_READ | libc::PROT_EXEC).into()
            }
            "call seccomp again" => libc::syscall(libc::SYS_seccomp, 2, 0, std::ptr::null::<u8>()),
            "ask for the Landlock version" => {
                libc::syscall(libc::SYS_landlock_create_ruleset, 0, 0, 1)
            }
            "create a memory file" => libc::memfd_create(c"x".as_ptr(), 0).into(),
            "create a pipe" => {
                let mut pipe = [-1; 2];
                libc::pipe2(pipe.as_mut_ptr(), 0).into()
            }
            "duplicate a descriptor with dup" => libc::dup(0).into(),
            "leave its namespaces" => libc::unshare(libc::CLONE_NEWUSER).into(),
            "set up io_uring" => libc::syscall(libc::SYS_io_uring_setup, 1, 0),
            "call bpf" => libc::syscall(libc::SYS_bpf, 0, 0, 0),
            "mount" => libc::mount(
                paths.file.as_ptr(),
                paths.file.as_ptr(),
                std::ptr::null(),
                0,
                std::ptr::null(),
            )
            .into(),
            "read another process's memory" => {
                libc::process_vm_readv(paths.parent, std::ptr::null(), 0, std::ptr::null(), 0, 0)
                    as libc::c_long
            }
            "sleep by a call that is not listed" => {
                let moment = libc::timespec {
                    tv_sec: 0,
                    tv_nsec: 1,
                };
                libc::nanosleep(&moment, std::ptr::null_mut()).into()
            }
            other => panic!("an unknown forbidden call: {other}"),
        }
    }
}

/// Every forbidden call, the five sends among them, in each of the four layerings:
/// the filter stops each one by itself, whether or not Landlock is under it.
#[test]
fn every_forbidden_call_kills_the_process_with_sigsys() {
    assert!(Path::new("/bin/sh").exists());
    assert_eq!(
        FORBIDDEN
            .iter()
            .filter(|name| name.starts_with("send") || **name == "call sendmsg")
            .count(),
        5
    );
    for refused in LAYERINGS {
        let fixture = Fixture::new();
        for name in FORBIDDEN {
            let mut child = Child::start_under(refused, "forbidden", &fixture, name);
            let stream = fixture.accept(&mut child).unwrap();
            let listening = TcpListener::bind("127.0.0.1:0").unwrap();
            // An unconnected, unbound TCP socket is made by hand: std has none.
            // SAFETY: a plain socket call; the result is checked and owned at once.
            let unconnected = unsafe {
                let socket = libc::socket(libc::AF_INET, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0);
                assert!(socket >= 0);
                OwnedFd::from_raw_fd(socket)
            };
            let datagram = UnixDatagram::unbound().unwrap();
            // "listen" needs a bound socket to succeed without the filter.
            let handed = if name.starts_with("listen") {
                listening.as_raw_fd()
            } else if wants_a_datagram_socket(name) {
                datagram.as_raw_fd()
            } else {
                unconnected.as_raw_fd()
            };
            send_descriptor(&stream, handed);
            let (ended, report) = child.finish();
            // The call never returned: nothing follows the report of confinement.
            assert_eq!(report, confined_line_under(refused), "{name} {refused:?}");
            assert_eq!(ended, Ended::Signaled(libc::SIGSYS), "{name} {refused:?}");
        }
        assert_eq!(std::fs::read(&fixture.file).unwrap(), b"retained");
        assert!(!fixture.path("created").exists());
        assert_eq!(
            fixture.reached(),
            (None, None),
            "a confined process sent out ({refused:?})"
        );
    }
}

/// Why a send may name no destination: without the filter, a send with one reaches
/// a socket the process was never connected to. On a TCP socket that is the
/// send-time connect of `MSG_FASTOPEN`, which is not a `connect` call.
#[test]
fn without_the_filter_a_send_with_a_destination_reaches_another_socket() {
    let fixture = Fixture::new();
    let name = "send to a destination on a Unix datagram socket it was handed";
    let (ended, report) = Child::start("unconfined", &fixture, name).finish();
    assert_eq!(
        (ended, report),
        (Ended::Exited(0), format!("{name}: succeeded\n"))
    );
    assert_eq!(fixture.reached(), (None, Some(b"x".to_vec())));

    let name = "send with fast open to a destination on a TCP socket it was handed";
    let (ended, report) = Child::start("unconfined", &fixture, name).finish();
    assert_eq!(ended, Ended::Exited(0), "{report}");
    let mut connected = None;
    let until = Instant::now() + Duration::from_secs(2);
    while connected.is_none() && Instant::now() < until {
        connected = fixture.reached().0;
        std::thread::sleep(Duration::from_millis(5));
    }
    if report == format!("{name}: succeeded\n") {
        // The kernel connected the socket and delivered the byte.
        assert_eq!(connected, Some(b"x".to_vec()));
    } else {
        // A system with TCP fast open turned off for clients refuses the send.
        assert_eq!(report, format!("{name}: error {}\n", libc::EOPNOTSUPP));
        assert_eq!(connected, None);
    }
}

#[test]
fn the_forbidden_calls_work_without_the_filter() {
    // The same attempts by an unconfined child, so that a kill above is the
    // filter's doing and not a broken attempt. The others change or need something
    // (a file, a tracer, a privilege) and are left to the filter's own table test.
    let fixture = Fixture::new();
    for name in [
        "open a file for reading",
        "read a file's metadata",
        "read its own environment",
        "create a Unix socket",
        "create a TCP socket",
        "create a socket pair",
        "signal its parent",
        "ask for its seccomp mode",
        "make a descriptor signal its parent",
        "set a descriptor's flags with fcntl",
        "map executable memory",
        "make memory executable",
        "ask for the Landlock version",
        "create a memory file",
        "create a pipe",
        "duplicate a descriptor with dup",
        "sleep by a call that is not listed",
    ] {
        assert!(FORBIDDEN.contains(&name));
        let (ended, report) = Child::start("unconfined", &fixture, name).finish();
        assert_eq!(report, format!("{name}: succeeded\n"));
        assert_eq!(ended, Ended::Exited(0), "{name}");
    }
}

#[test]
fn a_forbidden_call_on_a_second_thread_kills_the_whole_process() {
    for refused in LAYERINGS {
        let fixture = Fixture::new();
        let mut child = Child::start_under(refused, "forbidden on a thread", &fixture, "");
        let _stream = fixture.accept(&mut child).unwrap();
        let (ended, report) = child.finish();
        assert_eq!(report, confined_line_under(refused), "{refused:?}");
        assert_eq!(ended, Ended::Signaled(libc::SIGSYS), "{refused:?}");
    }
}

#[test]
fn an_abort_ends_a_confined_process_with_its_own_signal() {
    // glibc's abort blocks signals, asks for the thread and process ids and sends
    // SIGABRT to its own thread: rt_sigprocmask, gettid, getpid, tgkill.
    let fixture = Fixture::new();
    let mut child = Child::start("abort", &fixture, "");
    let _stream = fixture.accept(&mut child).unwrap();
    let (ended, report) = child.finish();
    assert_eq!(report, confined_line());
    assert_eq!(ended, Ended::Signaled(libc::SIGABRT));
}

#[test]
fn a_stopped_and_continued_process_resumes_its_timed_wait() {
    // The kernel resumes an interrupted poll with the restart_syscall call, which
    // passes the filter like any other.
    let fixture = Fixture::new();
    let mut child = Child::start("timed wait", &fixture, "");
    let mut stream = fixture.accept(&mut child).unwrap();
    let mut waiting = [0u8];
    stream.read_exact(&mut waiting).unwrap();
    // Give the child time to enter poll, then stop it and see it stopped.
    std::thread::sleep(Duration::from_millis(100));
    let mut status = 0;
    // SAFETY: signals to this test's own child, and a wait that reports its stop
    // without reaping it.
    unsafe {
        assert_eq!(libc::kill(child.pid(), libc::SIGSTOP), 0);
        assert_eq!(
            libc::waitpid(child.pid(), &mut status, libc::WUNTRACED),
            child.pid()
        );
        assert!(libc::WIFSTOPPED(status));
        assert_eq!(libc::kill(child.pid(), libc::SIGCONT), 0);
    }
    std::thread::sleep(Duration::from_millis(100));
    stream.write_all(b"go").unwrap();
    let (ended, report) = child.finish();
    assert_eq!(report, format!("{}poll returned 1\n", confined_line()));
    assert_eq!(ended, Ended::Exited(0));
}

/// The child's half of the precondition tests: `confine_web` must refuse, and the
/// process must be as free as before.
fn refused_at_entry(report: &Report, rendezvous: &Path, file: &Path) {
    let error = confinement::confine_web(rendezvous).unwrap_err();
    report.line(&format!("{error}"));
    report.line(&format!("os error: {:?}", error.raw_os_error()));
    report.line(&freedom(file));
}

/// Starts the child `role`, which must be refused at entry for `reason` with
/// nothing confined and nothing connected.
fn assert_refused_at_entry(role: &str, reason: &str) {
    let fixture = Fixture::new();
    let mut child = Child::start(role, &fixture, "");
    assert!(fixture.accept(&mut child).is_none(), "nothing connects");
    let (ended, report) = child.finish();
    assert_eq!(ended, Ended::Exited(0), "{report}");
    let refusal = format!("{REFUSAL}: {reason}\nos error: None\n");
    if runs_unfiltered() {
        assert_eq!(report, format!("{refusal}{FREE}\n"));
    } else {
        assert!(report.starts_with(&refusal), "{report}");
        assert!(report.ends_with("file Some(8)\n"), "{report}");
    }
}

#[test]
fn a_process_with_a_second_thread_is_refused_at_entry_and_nothing_is_confined() {
    assert_refused_at_entry(
        "second thread",
        "the process has 2 threads; it must confine itself before it starts any",
    );
}

#[test]
fn a_process_with_another_descriptor_is_refused_at_entry_and_nothing_is_confined() {
    assert_refused_at_entry(
        "another descriptor",
        "the process must hold exactly descriptors 0, 1 and 2; it holds [0, 1, 2, 3]",
    );
}

#[test]
fn a_missing_standard_descriptor_is_not_made_up_by_the_checks_own() {
    // With descriptor 0 closed, the check's own open files take number 0. They are
    // left out of what it finds, so the process holds 1 and 2 only and is refused.
    assert_refused_at_entry(
        "no standard input",
        "the process must hold exactly descriptors 0, 1 and 2; it holds [1, 2]",
    );
}

#[test]
fn a_missing_rendezvous_fails_at_the_connection_with_nothing_confined() {
    let fixture = Fixture::new();
    let (ended, report) = Child::start("missing rendezvous", &fixture, "").finish();
    assert_eq!(ended, Ended::Exited(0), "{report}");
    let connection = format!("os error: Some({})\n", libc::ENOENT);
    if runs_unfiltered() {
        assert_eq!(report, format!("{connection}{FREE}\n"));
    } else {
        assert!(report.starts_with(&connection), "{report}");
    }
}

/// The child of the next test: steps 3 and 4 without the filter.
fn landlock_alone(report: &Report, file: &Path) {
    super::no_new_privileges().unwrap();
    let abi = landlock::apply().unwrap();
    report.line(&format!("abi {abi}"));
    let (file_c, created_c) = (c_path(file), c_path(&file.with_file_name("created")));
    let program = [c"/bin/sh".as_ptr(), std::ptr::null()];
    // SAFETY: raw calls with live NUL-terminated paths; results are reported.
    unsafe {
        report.outcome("open", libc::open(file_c.as_ptr(), libc::O_RDONLY).into());
        report.outcome(
            "create",
            libc::open(created_c.as_ptr(), libc::O_WRONLY | libc::O_CREAT, 0o600).into(),
        );
        report.outcome("truncate", libc::truncate(file_c.as_ptr(), 0).into());
        report.outcome(
            "rename",
            libc::rename(file_c.as_ptr(), created_c.as_ptr()).into(),
        );
        report.outcome("unlink", libc::unlink(file_c.as_ptr()).into());
        report.outcome("mkdir", libc::mkdir(created_c.as_ptr(), 0o700).into());
        report.outcome(
            "environ",
            libc::open(c"/proc/self/environ".as_ptr(), libc::O_RDONLY).into(),
        );
        report.outcome(
            "execve",
            libc::execve(program[0], program.as_ptr(), program[1..].as_ptr()).into(),
        );
    }
    if abi >= 4 {
        let connect = TcpStream::connect(word()).map_err(|error| error.raw_os_error());
        report.line(&format!("tcp connect: {:?}", connect.map(drop)));
        let bind = TcpListener::bind("127.0.0.1:0").map_err(|error| error.raw_os_error());
        report.line(&format!("tcp bind: {:?}", bind.map(drop)));
    }
    if abi >= 6 {
        // SAFETY: a null signal to the parent: a permission check only.
        report.outcome("signal the parent", unsafe {
            libc::kill(libc::getppid(), 0).into()
        });
        let abstract_name = abstract_address(&file.with_file_name("abstract"));
        let connect = UnixStream::connect_addr(&abstract_name);
        report.line(&format!(
            "abstract unix connect: {:?}",
            connect.map(drop).map_err(|error| error.raw_os_error())
        ));
        let datagram = UnixDatagram::unbound().unwrap();
        let sent = datagram.send_to_addr(b"x", &abstract_address(&file.with_file_name("heard")));
        report.line(&format!(
            "datagram to an abstract name: {:?}",
            sent.map_err(|error| error.raw_os_error())
        ));
    }
    // Not controlled by Landlock: these succeed, and the filter kills them.
    let named = UnixStream::connect(file.with_file_name("named"));
    report.line(&format!(
        "named unix connect: {:?}",
        named.map(drop).map_err(|error| error.raw_os_error())
    ));
    // SAFETY: raw calls with a live NUL-terminated path; results are reported.
    unsafe {
        report.outcome("chmod", libc::chmod(file_c.as_ptr(), 0o640).into());
        report.outcome(
            "socket",
            libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0).into(),
        );
    }
}

/// Landlock alone, where the kernel has it. What it denies returns an error, and
/// each of those is a call the filter kills: a path operation, a TCP `connect` or
/// `bind`, a signal to another process, a `connect` or a send with a destination to
/// an abstract Unix name. What it does not control is what the filter is for.
#[test]
fn landlock_alone_denies_every_path_and_leaves_the_rest_to_the_filter() {
    let Ok(abi) = kernel_landlock() else {
        eprintln!("this kernel has no Landlock: nothing to measure");
        return;
    };
    let fixture = Fixture::new();
    let named = UnixListener::bind(fixture.path("named")).unwrap();
    let abstract_listener = UnixListener::bind_addr(&fixture.abstract_address("abstract")).unwrap();
    let heard = UnixDatagram::bind_addr(&fixture.abstract_address("heard")).unwrap();
    heard.set_nonblocking(true).unwrap();
    let outbound = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = outbound.local_addr().unwrap().to_string();
    let (ended, report) = Child::start("landlock alone", &fixture, &address).finish();
    assert_eq!(ended, Ended::Exited(0), "{report}");
    let denied = libc::EACCES;
    let mut expected = format!("abi {abi}\nopen: error {denied}\ncreate: error {denied}\n");
    // Truncation by path is a right of its own from ABI 3. Below it the truncation
    // succeeds under Landlock alone, and is one more thing only the filter stops.
    expected += &match abi {
        3.. => format!("truncate: error {denied}\n"),
        _ => "truncate: succeeded\n".to_owned(),
    };
    expected += &format!(
        "rename: error {denied}\nunlink: error {denied}\nmkdir: error {denied}\n\
         environ: error {denied}\nexecve: error {denied}\n"
    );
    if abi >= 4 {
        expected += &format!("tcp connect: Err(Some({denied}))\ntcp bind: Err(Some({denied}))\n");
    }
    if abi >= 6 {
        let scoped = libc::EPERM;
        expected += &format!(
            "signal the parent: error {scoped}\nabstract unix connect: Err(Some({scoped}))\n\
             datagram to an abstract name: Err(Some({scoped}))\n"
        );
    }
    expected += "named unix connect: Ok(())\nchmod: succeeded\nsocket: succeeded\n";
    assert_eq!(report, expected);
    if abi >= 3 {
        assert_eq!(std::fs::read(&fixture.file).unwrap(), b"retained");
    }
    assert!(!fixture.path("created").exists());
    assert!(
        heard.recv(&mut [0u8; 4]).is_err(),
        "a scoped datagram arrived"
    );
    drop((named, abstract_listener, outbound));
}

/// What the list lets a confined process do that comes closest to anything Landlock
/// controls: files, devices and sockets it was handed, abstract Unix names behind
/// them, signals to itself. Each line is one allowed call and its outcome.
fn candidates(report: &Report, stream: &UnixStream, parent: libc::pid_t) {
    let file = receive_descriptor(stream);
    let null = receive_descriptor(stream);
    let random = receive_descriptor(stream);
    let datagram = receive_descriptor(stream);
    let abstract_stream = receive_descriptor(stream);
    let tcp = receive_descriptor(stream);
    let unconnected = receive_descriptor(stream);
    let (file, null, random) = (file.as_raw_fd(), null.as_raw_fd(), random.as_raw_fd());
    let (datagram, abstract_stream) = (datagram.as_raw_fd(), abstract_stream.as_raw_fd());
    let (tcp, unconnected) = (tcp.as_raw_fd(), unconnected.as_raw_fd());
    let send = |socket, byte: &std::ffi::CStr| {
        // SAFETY: one live byte, no destination, a flag the list allows.
        unsafe {
            libc::sendto(
                socket,
                byte.as_ptr().cast(),
                1,
                libc::MSG_NOSIGNAL,
                std::ptr::null(),
                0,
            ) as libc::c_long
        }
    };
    let mut bytes = [0u8; 8];
    let mut left: libc::c_int = -1;
    // SAFETY: raw calls on descriptors this process owns, with live buffers and out
    // values; each result is only reported.
    unsafe {
        let read = libc::read(file, bytes.as_mut_ptr().cast(), 6);
        report.line(&format!(
            "read a file it was handed: {read} {:?}",
            String::from_utf8_lossy(&bytes[..6])
        ));
        report.outcome(
            "ask how much of the file is left",
            libc::ioctl(file, libc::FIONREAD, &mut left).into(),
        );
        report.line(&format!("left: {left}"));
        report.outcome(
            "write to the file",
            libc::write(file, c"!".as_ptr().cast(), 1) as libc::c_long,
        );
        let mapped = libc::mmap(
            std::ptr::null_mut(),
            11,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            file,
            0,
        );
        if mapped == libc::MAP_FAILED {
            report.outcome("write to the file through a shared mapping", -1);
        } else {
            mapped.cast::<u8>().write(b'H');
            report.outcome(
                "write to the file through a shared mapping",
                libc::munmap(mapped, 11).into(),
            );
        }
        report.outcome(
            "duplicate the file's descriptor",
            libc::fcntl(file, libc::F_DUPFD_CLOEXEC, 0).into(),
        );
        report.outcome(
            "ask how much of the null device is left",
            libc::ioctl(null, libc::FIONREAD, &mut left).into(),
        );
        let mut blocking: libc::c_int = 0;
        report.outcome(
            "make the null device blocking",
            libc::ioctl(null, libc::FIONBIO, &mut blocking).into(),
        );
        report.outcome(
            "write to the null device",
            libc::write(null, c"x".as_ptr().cast(), 1) as libc::c_long,
        );
        report.outcome(
            "read the random device",
            libc::read(random, bytes.as_mut_ptr().cast(), 8) as libc::c_long,
        );
        report.outcome(
            "ask how much of the random device is left",
            libc::ioctl(random, libc::FIONREAD, &mut left).into(),
        );
        report.outcome(
            "send on a datagram socket connected to an abstract name",
            send(datagram, c"d"),
        );
        report.outcome(
            "write on a stream connected to an abstract name",
            libc::write(abstract_stream, c"s".as_ptr().cast(), 1) as libc::c_long,
        );
        report.outcome(
            "write on a TCP connection",
            libc::write(tcp, c"t".as_ptr().cast(), 1) as libc::c_long,
        );
        let mut peer: libc::sockaddr_storage = std::mem::zeroed();
        let mut length = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        report.outcome(
            "ask a TCP connection's peer",
            libc::getpeername(
                tcp,
                (&mut peer as *mut libc::sockaddr_storage).cast(),
                &mut length,
            )
            .into(),
        );
        let one: libc::c_int = 1;
        report.outcome(
            "set an option on a TCP connection",
            libc::setsockopt(
                tcp,
                libc::IPPROTO_TCP,
                libc::TCP_NODELAY,
                (&one as *const libc::c_int).cast(),
                4,
            )
            .into(),
        );
        report.outcome(
            "shut down a TCP connection",
            libc::shutdown(tcp, libc::SHUT_WR).into(),
        );
        report.outcome(
            "send on an unconnected datagram socket",
            send(unconnected, c"u"),
        );
        let own = libc::getpid();
        report.outcome(
            "signal its own thread",
            libc::syscall(libc::SYS_tgkill, own, libc::syscall(libc::SYS_gettid), 0),
        );
        report.outcome(
            "signal its parent as a thread of its own",
            libc::syscall(libc::SYS_tgkill, own, parent, 0),
        );
        let mut processors: libc::cpu_set_t = std::mem::zeroed();
        let asked = libc::syscall(
            libc::SYS_sched_getaffinity,
            parent,
            std::mem::size_of::<libc::cpu_set_t>(),
            &mut processors as *mut libc::cpu_set_t,
        );
        report.outcome("ask which processors its parent may use", asked);
        let epoll = libc::epoll_create1(libc::EPOLL_CLOEXEC);
        let event = libc::eventfd(1, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK);
        let mut interest = libc::epoll_event {
            events: libc::EPOLLIN as u32,
            u64: 7,
        };
        let added = libc::epoll_ctl(epoll, libc::EPOLL_CTL_ADD, event, &mut interest);
        let ready = libc::epoll_wait(epoll, &mut interest, 1, 0);
        report.line(&format!(
            "wait on an event descriptor it made: {}",
            if epoll >= 0 && event >= 0 && added == 0 && ready == 1 {
                "succeeded".to_owned()
            } else {
                format!("epoll {epoll} event {event} added {added} ready {ready}")
            }
        ));
    }
}

/// What [`candidates`] reports, however the process is confined.
fn candidate_outcomes() -> String {
    format!(
        "read a file it was handed: 6 \"handed\"\n\
         ask how much of the file is left: succeeded\n\
         left: 5\n\
         write to the file: succeeded\n\
         write to the file through a shared mapping: succeeded\n\
         duplicate the file's descriptor: succeeded\n\
         ask how much of the null device is left: error {not_a_terminal}\n\
         make the null device blocking: succeeded\n\
         write to the null device: succeeded\n\
         read the random device: succeeded\n\
         ask how much of the random device is left: error {invalid}\n\
         send on a datagram socket connected to an abstract name: succeeded\n\
         write on a stream connected to an abstract name: succeeded\n\
         write on a TCP connection: succeeded\n\
         ask a TCP connection's peer: succeeded\n\
         set an option on a TCP connection: succeeded\n\
         shut down a TCP connection: succeeded\n\
         send on an unconnected datagram socket: error {not_connected}\n\
         signal its own thread: succeeded\n\
         signal its parent as a thread of its own: error {no_such_thread}\n\
         ask which processors its parent may use: succeeded\n\
         wait on an event descriptor it made: succeeded\n",
        not_a_terminal = libc::ENOTTY,
        invalid = libc::EINVAL,
        not_connected = libc::ENOTCONN,
        no_such_thread = libc::ESRCH,
    )
}

/// The other direction of the layering: is there anything the list allows that only
/// Landlock stopped? The candidates are run unconfined, under Landlock alone, under
/// both layers and under the filter alone (Landlock refused at each of its three
/// calls). Every run has the same outcomes, and what was sent arrived every time:
/// Landlock denies none of them, so taking it away opens nothing.
#[test]
fn nothing_the_list_allows_is_stopped_by_landlock_alone() {
    let mut runs = vec![("unconfined", None)];
    if kernel_landlock().is_ok() {
        runs.push(("landlock alone", None));
    }
    runs.extend(LAYERINGS.map(|refused| ("confined", refused)));
    for (way, refused) in runs {
        let fixture = Fixture::new();
        let handed = fixture.path("handed");
        std::fs::write(&handed, b"handed over").unwrap();
        let heard = UnixDatagram::bind_addr(&fixture.abstract_address("heard")).unwrap();
        heard.set_read_timeout(Some(WAIT)).unwrap();
        let abstract_listener =
            UnixListener::bind_addr(&fixture.abstract_address("abstract")).unwrap();
        let tcp_listener = TcpListener::bind("127.0.0.1:0").unwrap();

        let mut child = Child::start_under(refused, "candidates", &fixture, way);
        let stream = fixture.accept(&mut child).unwrap();
        let file = File::options()
            .read(true)
            .write(true)
            .open(&handed)
            .unwrap();
        let null = File::options()
            .read(true)
            .write(true)
            .open("/dev/null")
            .unwrap();
        let random = File::open("/dev/urandom").unwrap();
        let datagram = UnixDatagram::unbound().unwrap();
        datagram
            .connect_addr(&fixture.abstract_address("heard"))
            .unwrap();
        let abstract_stream =
            UnixStream::connect_addr(&fixture.abstract_address("abstract")).unwrap();
        let (mut abstract_peer, _) = abstract_listener.accept().unwrap();
        let tcp = TcpStream::connect(tcp_listener.local_addr().unwrap()).unwrap();
        let (mut tcp_peer, _) = tcp_listener.accept().unwrap();
        let unconnected = UnixDatagram::unbound().unwrap();
        for descriptor in [
            file.as_raw_fd(),
            null.as_raw_fd(),
            random.as_raw_fd(),
            datagram.as_raw_fd(),
            abstract_stream.as_raw_fd(),
            tcp.as_raw_fd(),
            unconnected.as_raw_fd(),
        ] {
            send_descriptor(&stream, descriptor);
        }
        let (ended, report) = child.finish();
        // For whoever reads this test's output: what this run measured.
        println!("{way} {refused:?}:\n{report}");
        let (first, outcomes) = report.split_once('\n').unwrap_or((&report, ""));
        let confined = confined_line_under(refused);
        assert_eq!(
            first,
            match way {
                "confined" => confined.trim_end(),
                way => way,
            },
            "{way} {refused:?}"
        );
        assert_eq!(outcomes, candidate_outcomes(), "{way} {refused:?}");
        assert_eq!(ended, Ended::Exited(0), "{way} {refused:?}");
        // What was sent arrived, and the file was changed both ways.
        let mut byte = [0u8; 4];
        assert_eq!(heard.recv(&mut byte).unwrap(), 1, "{way} {refused:?}");
        assert_eq!(byte[0], b'd');
        abstract_peer.set_read_timeout(Some(WAIT)).unwrap();
        tcp_peer.set_read_timeout(Some(WAIT)).unwrap();
        for (peer, sent) in [
            (&mut abstract_peer as &mut dyn Read, b's'),
            (&mut tcp_peer as &mut dyn Read, b't'),
        ] {
            peer.read_exact(&mut byte[..1]).unwrap();
            assert_eq!(byte[0], sent, "{way} {refused:?}");
        }
        assert_eq!(std::fs::read(&handed).unwrap(), b"Handed!over");
    }
}

/// A child under a profile that refuses one call asks whether the web process is
/// supported and tries to confine itself. Returns its report, and whether the
/// rendezvous saw a connection that closed without a byte.
fn confine_under_profile(refused: &str) -> (String, bool) {
    let fixture = Fixture::new();
    let mut child = Child::start("under a profile", &fixture, refused);
    let dropped = fixture.accept(&mut child).is_some_and(|mut stream| {
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).is_ok() && bytes.is_empty()
    });
    let (ended, report) = child.finish();
    assert_eq!(ended, Ended::Exited(0), "{report}");
    (report, dropped)
}

/// Landlock is never a reason to refuse: whatever its version query, the creation
/// of the ruleset or the restriction answers, the host's question passes and the
/// process confines itself with the filter alone and says why.
#[test]
fn a_kernel_or_profile_without_landlock_leaves_the_filter_alone_and_says_why() {
    let on_this_kernel = |reason: String| kernel_landlock().err().unwrap_or(reason);
    for (refused, reason) in [
        ("create EPERM", VERSION_QUERY_REFUSED.to_owned()),
        (
            "create ENOSYS",
            "landlock_create_ruleset failed with ENOSYS: this kernel has no Landlock".to_owned(),
        ),
        (
            "create EOPNOTSUPP",
            "landlock_create_ruleset failed with EOPNOTSUPP: Landlock is built into this kernel \
             but is not an enabled security module"
                .to_owned(),
        ),
        (
            "ruleset EPERM",
            on_this_kernel(format!(
                "landlock_create_ruleset failed with {NOT_PERMITTED}"
            )),
        ),
        (
            "restrict EPERM",
            on_this_kernel(format!(
                "landlock_restrict_self failed with {NOT_PERMITTED}"
            )),
        ),
    ] {
        let (report, connected) = confine_under_profile(refused);
        assert_eq!(
            report,
            format!(
                "supported: Ok(())\nconfined: landlock None seccomp true not applied \
                 Some({reason:?})\n"
            ),
            "{refused}"
        );
        assert!(connected, "{refused}");
    }
}

/// What a process that could not install its filter reports: the refusal, no OS
/// error number, and whether it can still read a file. Landlock was applied before
/// the filter was refused, where this kernel has it: the process is half confined,
/// and it refused.
fn filter_refused(cause: &str) -> String {
    let file = match kernel_landlock() {
        Ok(_) => format!("Err(Some({}))", libc::EACCES),
        Err(_) => "Ok(8)".to_owned(),
    };
    format!("{REFUSAL}: {cause}\nos error: None\nfile: {file}\n")
}

/// LNX-3: a profile that refuses `seccomp` stops the web process, and the host is
/// told so before it starts one.
#[test]
fn a_profile_that_refuses_seccomp_makes_the_host_and_the_process_refuse_with_the_reason() {
    for (refused, question) in [
        (
            "seccomp EPERM",
            "seccomp failed with EPERM: this kernel or its profile has no seccomp filter that \
             kills the process",
        ),
        (
            "filter EPERM",
            "seccomp failed with EPERM: this kernel or its profile does not let a process \
             install a seccomp filter on all its threads",
        ),
    ] {
        let (report, dropped) = confine_under_profile(refused);
        assert_eq!(
            report,
            format!(
                "supported: Err(({:?}, None))\n{}",
                format!("{REFUSAL}: {question}"),
                filter_refused(&format!("seccomp failed with {NOT_PERMITTED}"))
            ),
            "{refused}"
        );
        assert!(dropped, "the connected stream was dropped unused");
    }
}

/// The host's question cannot foresee every failure of the installation. One that
/// fails afterwards still refuses, in the web process itself.
#[test]
fn a_filter_that_cannot_be_installed_after_all_makes_the_process_refuse() {
    let (report, dropped) = confine_under_profile("install ENOMEM");
    assert_eq!(
        report,
        format!(
            "supported: Ok(())\n{}",
            filter_refused("seccomp failed with ENOMEM (Cannot allocate memory (os error 12))")
        )
    );
    assert!(dropped, "the connected stream was dropped unused");
}

/// Without Landlock the filter is the only layer, and it is still required: where
/// both are refused, nothing is restricted and the process refuses.
#[test]
fn without_landlock_a_refused_filter_still_refuses() {
    let (report, dropped) = confine_under_profile("landlock and seccomp EPERM");
    assert_eq!(
        report,
        format!(
            "supported: Err(({:?}, None))\n{REFUSAL}: seccomp failed with {NOT_PERMITTED}\n\
             os error: None\nfile: Ok(8)\n",
            format!(
                "{REFUSAL}: seccomp failed with EPERM: this kernel or its profile has no \
                 seccomp filter that kills the process"
            )
        )
    );
    assert!(dropped, "the connected stream was dropped unused");
}

/// The host observes the filter, with or without Landlock under it: the report says
/// which, and the host cannot see more.
#[test]
fn the_host_accepts_a_confined_process_with_or_without_landlock() {
    for refused in LAYERINGS {
        let fixture = Fixture::new();
        let mut child = Child::start_under(refused, "stay confined", &fixture, "");
        let stream = fixture.accept(&mut child).unwrap();
        // The connection is made before the filter: wait for the report of
        // confinement.
        child.reported(&confined_line_under(refused));
        for report in [FILTER_ALONE, report_of(1), report_of(3), report_of(7)] {
            confinement::verify_web(child.id(), &report, &[]).unwrap();
            confinement::verify_web(child.id(), &report, &[&fixture.file]).unwrap();
            let seccomp = Enforcement {
                seccomp: true,
                ..report
            };
            confinement::verify_web(child.id(), &seccomp, &[]).unwrap();
        }
        let error = confinement::verify_web(child.id(), &report_of(0), &[]).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("{REFUSAL}: the ready line reports Landlock ABI 0, which is no ABI")
        );
        drop(stream);
        assert_eq!(child.finish(), (Ended::Exited(0), String::new()));
    }
}

#[test]
fn the_host_refuses_a_process_that_is_not_confined() {
    // This test process itself: it installed no filter, whatever it inherited and
    // whatever is reported of it.
    for report in [report_of(7), FILTER_ALONE] {
        let text = confinement::verify_web(std::process::id(), &report, &[])
            .unwrap_err()
            .to_string();
        if runs_unfiltered() {
            assert_eq!(
                text,
                format!("{REFUSAL}: the web process has not set no-new-privileges (NoNewPrivs: 0)")
            );
        } else {
            assert!(
                text.starts_with(&format!("{REFUSAL}: the web process")),
                "{text}"
            );
        }
    }
}

#[test]
fn the_host_refuses_a_process_with_no_new_privileges_but_no_filter() {
    let fixture = Fixture::new();
    let mut child = Child::start("privileges only", &fixture, "");
    child.reported("ready\n");
    for report in [report_of(7), FILTER_ALONE] {
        let text = confinement::verify_web(child.id(), &report, &[])
            .unwrap_err()
            .to_string();
        if runs_unfiltered() {
            assert_eq!(
                text,
                format!("{REFUSAL}: the web process runs without a seccomp filter (Seccomp: 0)")
            );
        } else {
            assert!(
                text.starts_with(&format!(
                    "{REFUSAL}: the web process installed no seccomp filter of its own"
                )),
                "{text}"
            );
        }
    }
    assert!(!child.ended());
}

#[test]
fn the_host_refuses_a_process_that_no_longer_exists() {
    let fixture = Fixture::new();
    let child = Child::start("exit", &fixture, "");
    let pid = child.id();
    assert_eq!(child.finish().0, Ended::Exited(0));
    let error = confinement::verify_web(pid, &report_of(7), &[]).unwrap_err();
    assert_eq!(
        error.to_string(),
        format!("{REFUSAL}: /proc/{pid}/status cannot be read (NotFound)")
    );
    for invalid in [0, u32::MAX] {
        let error = confinement::verify_web(invalid, &report_of(7), &[]).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("{REFUSAL}: the web process has no valid process id")
        );
    }
}

/// The child of the next test, as the "host": a process under a profile that
/// allows everything, which starts one child that only inherits the profile and one
/// that confines itself, and checks both as the host does.
fn host_under_a_profile(report: &Report, rendezvous: &Path, profile: &str) {
    let (program, expected) = match profile {
        "that allows everything" => (profile::refusing(&[], 0), report_of(3)),
        "that refuses Landlock" => (
            profile::profile(RefusedCall::LandlockCreateRuleset),
            FILTER_ALONE,
        ),
        other => panic!("an unknown profile: {other}"),
    };
    profile::enter(&program).unwrap();
    // The question is asked under the profile, as the host asks it.
    report.line(&format!("supported: {:?}", super::supported().is_ok()));
    let inner = rendezvous.with_file_name("inner");
    let listener = UnixListener::bind(&inner).unwrap();
    let (mut wake, waiting) = UnixStream::pair().unwrap();
    let plain = fork_then(move || {
        let mut waiting = waiting;
        let _ = waiting.read(&mut [0u8]);
        0
    });
    let refused = confinement::verify_web(plain as u32, &report_of(7), &[]).unwrap_err();
    report.line(&refused.to_string());
    let web = fork_then(|| {
        // SAFETY: closes every descriptor but the three standard ones.
        unsafe { libc::close_range(3, libc::c_uint::MAX, 0) };
        match confinement::confine_web(&inner) {
            // Its exit status says whether it applied the layers this profile leaves.
            Ok(mut confined) => {
                let _ = confined.stream.read(&mut [0u8]);
                let landlock = confined.enforcement.landlock_abi.is_some();
                let expected = expected.landlock_abi.is_some() && kernel_landlock().is_ok();
                i32::from(landlock != expected)
            }
            Err(_) => 9,
        }
    });
    let (mut stream, _) = listener.accept().unwrap();
    // The connection is made before the filter: wait until the filter holds.
    let until = Instant::now() + WAIT;
    let accepted = loop {
        match confinement::verify_web(web as u32, &expected, &[]) {
            Ok(()) => break "accepted".to_owned(),
            Err(error) if Instant::now() > until => break error.to_string(),
            Err(_) => std::thread::sleep(Duration::from_millis(2)),
        }
    };
    report.line(&accepted);
    let filters = |pid: u32| {
        let text = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
        status::filters(&text).unwrap()
    };
    let own = filters(std::process::id());
    report.line(&format!(
        "plain {} web {}",
        filters(plain as u32) - own,
        filters(web as u32) - own
    ));
    let _ = stream.write_all(b"x");
    let _ = wake.write_all(b"x");
    let mut status = 0;
    // SAFETY: waits for this process's own two children.
    unsafe {
        libc::waitpid(plain, &mut status, 0);
        libc::waitpid(web, &mut status, 0);
    }
    report.line(&format!("web exited {}", libc::WEXITSTATUS(status)));
}

/// Forks the calling child, which has no other thread. The new process runs `body`
/// and exits with its value.
fn fork_then(body: impl FnOnce() -> i32) -> libc::pid_t {
    // SAFETY: a single-threaded process forks; the new one ends in `_exit`.
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0);
    if pid == 0 {
        // SAFETY: the alarm ends a process that a failed test left waiting.
        unsafe { libc::alarm(60) };
        let code = body();
        // SAFETY: ends the process without unwinding into its parent's frames.
        unsafe { libc::_exit(code) }
    }
    pid
}

/// Under a container's own profile every process has a filter. The host then
/// accepts only a child with one more than its own, whether the profile lets the
/// child use Landlock or not.
#[test]
fn under_a_profile_the_host_tells_an_inherited_filter_from_the_web_processs_own() {
    for profile in ["that allows everything", "that refuses Landlock"] {
        let fixture = Fixture::new();
        let (ended, report) = Child::start("host under a profile", &fixture, profile).finish();
        assert_eq!(ended, Ended::Exited(0), "{report}");
        let mut lines = report.lines();
        assert_eq!(lines.next(), Some("supported: true"), "{report}");
        let refused = lines.next().unwrap();
        assert!(
            refused.starts_with(&format!(
                "{REFUSAL}: the web process installed no seccomp filter of its own (it has "
            )),
            "{report}"
        );
        assert_eq!(lines.next(), Some("accepted"), "{report}");
        assert_eq!(lines.next(), Some("plain 0 web 1"), "{report}");
        assert_eq!(lines.next(), Some("web exited 0"), "{report}");
    }
}
