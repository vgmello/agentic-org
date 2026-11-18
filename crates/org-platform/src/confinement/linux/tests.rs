//! The adapter against the running kernel. A confined process cannot be unconfined,
//! so each test starts its own child: this test executable again, with a role in its
//! environment. The role runs before the test harness starts (an `.init_array`
//! entry), on the main thread of a process that has no other thread and holds exactly
//! descriptors 0 to 2, as the web process is when it calls `confine_web`. The child
//! reports on its standard output and never reaches the harness.
//!
//! The pure parts (the Landlock rights of each ABI, the filter program, the entry
//! and status parsers) are tested beside their code.

use std::ffi::CString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::panic;
use std::path::{Path, PathBuf};
use std::process::{ChildStderr, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use super::{landlock, profile, status, syscalls, Unavailable};
use crate::confinement::{self, Enforcement, REFUSAL};

const WAIT: Duration = Duration::from_secs(10);

/// The environment variable that makes this executable a child, and names its role.
const ROLE: &str = "ORG_PLATFORM_CONFINEMENT_CHILD";
/// The child's rendezvous path, the fixture's file, and one more word for its role.
const RENDEZVOUS: &str = "ORG_PLATFORM_CONFINEMENT_RENDEZVOUS";
const FILE: &str = "ORG_PLATFORM_CONFINEMENT_FILE";
const WORD: &str = "ORG_PLATFORM_CONFINEMENT_WORD";

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
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .env(ROLE, role)
            .env(RENDEZVOUS, &fixture.rendezvous)
            .env(FILE, &fixture.file)
            .env(WORD, word)
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

/// A temporary directory with a rendezvous socket and a file.
struct Fixture {
    _directory: tempfile::TempDir,
    rendezvous: PathBuf,
    file: PathBuf,
    listener: UnixListener,
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
        Self {
            _directory: directory,
            rendezvous,
            file,
            listener,
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.file.with_file_name(name)
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

/// The child's half of every confined test: confine, then report what was enforced.
fn confine(report: &Report, rendezvous: &Path) -> UnixStream {
    let (stream, enforcement) = confinement::confine_web(rendezvous).unwrap();
    report.line(&format!(
        "confined: landlock {:?} seccomp {}",
        enforcement.landlock_abi, enforcement.seccomp
    ));
    stream
}

/// What a child that confined itself on this kernel reports.
fn confined_line() -> String {
    let abi = landlock::enforced_abi(landlock::abi().unwrap());
    format!("confined: landlock Some({abi}) seccomp true\n")
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
    assert_eq!(confinement::web_confinement_supported(), Ok(()));
    let (arch, abi) = super::available().unwrap();
    assert!(abi >= 3);
    assert_eq!(arch.audit, syscalls::native().unwrap().audit);
}

#[test]
fn support_is_decided_by_the_architecture_then_landlock_then_seccomp() {
    let native = syscalls::native();
    assert!(super::decide(native, Ok(3), Ok(())).is_ok());
    assert_eq!(
        super::decide(None, Ok(7), Ok(())).unwrap_err(),
        Unavailable::Architecture
    );
    for old in [0, 1, 2] {
        let refusal = super::decide(native, landlock::decide(Ok(old)), Ok(())).unwrap_err();
        assert_eq!(
            refusal,
            Unavailable::Landlock(landlock::Unavailable::TooOld(old))
        );
        assert!(refusal
            .to_string()
            .contains(&format!("Landlock ABI is {old}, below 3")));
        assert!(refusal
            .refused()
            .to_string()
            .contains("Landlock ABI is below 3; Linux 6.2 or later is required"));
    }
    for (errno, typed, fixed) in [
        (
            libc::ENOSYS,
            "ENOSYS: this kernel has no Landlock",
            "ENOSYS",
        ),
        (
            libc::EOPNOTSUPP,
            "EOPNOTSUPP: Landlock is built into this kernel but is not an enabled",
            "EOPNOTSUPP",
        ),
        (
            libc::EPERM,
            "landlock_create_ruleset failed with EPERM",
            "EPERM",
        ),
        (
            libc::EINVAL,
            "landlock_create_ruleset failed with EINVAL",
            "failed",
        ),
    ] {
        let refusal = super::decide(native, landlock::decide(Err(errno)), Ok(())).unwrap_err();
        assert!(refusal.to_string().contains(typed), "{refusal}");
        let refused = refusal.refused().to_string();
        assert!(refused.contains("landlock_create_ruleset"), "{refused}");
        assert!(refused.contains(fixed), "{refused}");
        assert!(
            refused.starts_with("web process confinement ("),
            "{refused}"
        );
        assert!(refused.contains("is not available on linux"), "{refused}");
    }
    for (errno, typed) in [
        (libc::EPERM, "seccomp failed with EPERM"),
        (libc::ENOSYS, "seccomp failed with ENOSYS"),
        (libc::EOPNOTSUPP, "seccomp failed with EOPNOTSUPP"),
    ] {
        let refusal = super::decide(native, Ok(7), Err(errno)).unwrap_err();
        assert_eq!(refusal, Unavailable::Seccomp(errno));
        assert!(refusal.to_string().contains(typed), "{refusal}");
        assert!(refusal.refused().to_string().contains("seccomp failed"));
    }
    // Landlock is named before seccomp when both are missing.
    assert_eq!(
        super::decide(
            native,
            landlock::decide(Err(libc::ENOSYS)),
            Err(libc::ENOSYS)
        )
        .unwrap_err(),
        Unavailable::Landlock(landlock::Unavailable::Missing)
    );
    let text = Unavailable::Architecture.refused().to_string();
    assert!(text.contains("no system call list for this processor architecture"));
}

#[test]
fn a_report_needs_a_landlock_abi_of_three_or_more_and_ignores_seccomp() {
    for abi in [3, 4, 7, 9] {
        for seccomp in [false, true] {
            confinement::verify_ready_report(&Enforcement {
                landlock_abi: Some(abi),
                seccomp,
            })
            .unwrap();
        }
    }
    for abi in [0, 1, 2] {
        let error = confinement::verify_ready_report(&Enforcement {
            landlock_abi: Some(abi),
            seccomp: true,
        })
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("{REFUSAL}: the ready line reports Landlock ABI {abi}, below 3")
        );
    }
    let error = confinement::verify_ready_report(&Enforcement {
        landlock_abi: None,
        seccomp: true,
    })
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        format!("{REFUSAL}: the ready line reports no Landlock domain")
    );
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
            let stream = confine(report, &rendezvous);
            let socket = receive_descriptor(&stream);
            let name = word();
            report.outcome(
                &name,
                attempt(&name, &Paths::new(&file), socket.as_raw_fd()),
            );
        }
        "unconfined" => {
            // The same attempt with nothing confined.
            // SAFETY: a plain socket call; the result is checked and owned at once.
            let socket = unsafe {
                let socket = libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0);
                assert!(socket >= 0);
                OwnedFd::from_raw_fd(socket)
            };
            let name = word();
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
            let (call, errno) = match word().as_str() {
                "create EPERM" => (libc::SYS_landlock_create_ruleset, libc::EPERM),
                "create ENOSYS" => (libc::SYS_landlock_create_ruleset, libc::ENOSYS),
                "create EOPNOTSUPP" => (libc::SYS_landlock_create_ruleset, libc::EOPNOTSUPP),
                "restrict EPERM" => (libc::SYS_landlock_restrict_self, libc::EPERM),
                "seccomp EPERM" => (libc::SYS_seccomp, libc::EPERM),
                other => panic!("an unknown profile: {other}"),
            };
            profile::enter(&profile::refusing(&[call], errno)).unwrap();
            report.line(&format!(
                "supported: {:?}",
                super::supported().map_err(|refused| refused.to_string())
            ));
            let error = confinement::confine_web(&rendezvous).unwrap_err();
            report.line(&format!("{error}"));
            report.line(&format!("os error: {:?}", error.raw_os_error()));
            report.line(&format!(
                "file: {:?}",
                std::fs::read(&file)
                    .map(|bytes| bytes.len())
                    .map_err(|error| error.raw_os_error())
            ));
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
        "host under a profile" => host_under_a_profile(report, &rendezvous),
        other => panic!("an unknown role: {other}"),
    }
    0
}

#[test]
fn a_confined_process_uses_its_stream_threads_memory_clock_and_randomness() {
    let fixture = Fixture::new();
    let mut child = Child::start("use", &fixture, "");
    let mut stream = fixture.accept(&mut child).unwrap();
    stream.write_all(b"ping").unwrap();
    let mut pong = [0u8; 4];
    stream.read_exact(&mut pong).unwrap();
    assert_eq!(&pong, b"pong");
    let (ended, report) = child.finish();
    assert_eq!(report, format!("{}done\n", confined_line()));
    assert_eq!(ended, Ended::Exited(0));
}

#[test]
fn a_confined_process_serves_a_listener_it_was_handed() {
    let fixture = Fixture::new();
    let mut child = Child::start("serve", &fixture, "");
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
    assert_eq!(report, format!("{}served\n", confined_line()));
    assert_eq!(ended, Ended::Exited(0));
}

/// What the forbidden calls act on.
struct Paths {
    file: CString,
    created: CString,
    address: libc::sockaddr_in,
}

impl Paths {
    /// The fixture's file, a name beside it that does not exist, and the loopback
    /// discard port, which nothing here listens on.
    fn new(file: &Path) -> Self {
        Self {
            file: c_path(file),
            created: c_path(&file.with_file_name("created")),
            address: libc::sockaddr_in {
                sin_family: libc::AF_INET as libc::sa_family_t,
                sin_port: 9u16.to_be(),
                sin_addr: libc::in_addr {
                    s_addr: u32::from(std::net::Ipv4Addr::LOCALHOST).to_be(),
                },
                sin_zero: [0; 8],
            },
        }
    }
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
    "start a program",
    "fork",
    "trace a process",
    "signal its parent",
    "signal a thread of its parent",
    "become dumpable again",
    "ask for its seccomp mode",
    "an ioctl other than the two socket requests",
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
            "start a program" => {
                libc::execve(program[0], program.as_ptr(), program[1..].as_ptr()).into()
            }
            "fork" => libc::fork().into(),
            "trace a process" => libc::ptrace(libc::PTRACE_TRACEME, 0, 0, 0),
            "signal its parent" => libc::kill(libc::getppid(), 0).into(),
            "signal a thread of its parent" => {
                libc::syscall(libc::SYS_tgkill, libc::getppid(), libc::getppid(), 0)
            }
            "become dumpable again" => libc::prctl(libc::PR_SET_DUMPABLE, 1, 0, 0, 0).into(),
            "ask for its seccomp mode" => libc::prctl(libc::PR_GET_SECCOMP, 0, 0, 0, 0).into(),
            "an ioctl other than the two socket requests" => {
                let mut size: libc::winsize = std::mem::zeroed();
                libc::ioctl(0, libc::TIOCGWINSZ, &mut size).into()
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
                libc::process_vm_readv(libc::getppid(), std::ptr::null(), 0, std::ptr::null(), 0, 0)
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

#[test]
fn every_forbidden_call_kills_the_process_with_sigsys() {
    let fixture = Fixture::new();
    assert!(Path::new("/bin/sh").exists());
    for name in FORBIDDEN {
        let mut child = Child::start("forbidden", &fixture, name);
        let stream = fixture.accept(&mut child).unwrap();
        let listening = TcpListener::bind("127.0.0.1:0").unwrap();
        // An unconnected, unbound TCP socket is made by hand: std has none.
        // SAFETY: a plain socket call; the result is checked and owned at once.
        let unconnected = unsafe {
            let socket = libc::socket(libc::AF_INET, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0);
            assert!(socket >= 0);
            OwnedFd::from_raw_fd(socket)
        };
        // "listen" needs a bound socket to succeed without the filter.
        let handed = if name.starts_with("listen") {
            listening.as_raw_fd()
        } else {
            unconnected.as_raw_fd()
        };
        send_descriptor(&stream, handed);
        let (ended, report) = child.finish();
        // The call never returned: nothing follows the report of confinement.
        assert_eq!(report, confined_line(), "{name}");
        assert_eq!(ended, Ended::Signaled(libc::SIGSYS), "{name}");
    }
    assert_eq!(std::fs::read(&fixture.file).unwrap(), b"retained");
    assert!(!fixture.path("created").exists());
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
    let fixture = Fixture::new();
    let mut child = Child::start("forbidden on a thread", &fixture, "");
    let _stream = fixture.accept(&mut child).unwrap();
    let (ended, report) = child.finish();
    assert_eq!(report, confined_line());
    assert_eq!(ended, Ended::Signaled(libc::SIGSYS));
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
    let abi = landlock::restrict(landlock::abi().unwrap()).unwrap();
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

/// Landlock alone. What it denies returns an error, and what it does not control
/// is what the filter is for.
#[test]
fn landlock_alone_denies_every_path_and_leaves_the_rest_to_the_filter() {
    let fixture = Fixture::new();
    let named = UnixListener::bind(fixture.path("named")).unwrap();
    let outbound = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = outbound.local_addr().unwrap().to_string();
    let (ended, report) = Child::start("landlock alone", &fixture, &address).finish();
    assert_eq!(ended, Ended::Exited(0), "{report}");
    let abi = landlock::enforced_abi(landlock::abi().unwrap());
    let denied = libc::EACCES;
    let mut expected = format!(
        "abi {abi}\nopen: error {denied}\ncreate: error {denied}\ntruncate: error {denied}\n\
         rename: error {denied}\nunlink: error {denied}\nmkdir: error {denied}\n\
         environ: error {denied}\nexecve: error {denied}\n"
    );
    if abi >= 4 {
        expected += &format!("tcp connect: Err(Some({denied}))\ntcp bind: Err(Some({denied}))\n");
    }
    if abi >= 6 {
        expected += &format!("signal the parent: error {}\n", libc::EPERM);
    }
    expected += "named unix connect: Ok(())\nchmod: succeeded\nsocket: succeeded\n";
    assert_eq!(report, expected);
    assert_eq!(std::fs::read(&fixture.file).unwrap(), b"retained");
    assert!(!fixture.path("created").exists());
    drop((named, outbound));
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

#[test]
fn a_profile_that_refuses_landlock_makes_the_process_refuse_with_the_reason() {
    for (refused, reason, cause) in [
        (
            "create EPERM",
            "landlock_create_ruleset failed with EPERM, as under a profile that refuses it",
            "landlock_create_ruleset failed with EPERM: the Landlock version query was refused",
        ),
        (
            "create ENOSYS",
            "landlock_create_ruleset failed with ENOSYS: this kernel has no Landlock",
            "landlock_create_ruleset failed with ENOSYS: this kernel has no Landlock (Linux 6.2 \
             or later with Landlock is required)",
        ),
        (
            "create EOPNOTSUPP",
            "landlock_create_ruleset failed with EOPNOTSUPP",
            "landlock_create_ruleset failed with EOPNOTSUPP: Landlock is built into this kernel \
             but is not an enabled security module",
        ),
    ] {
        let (report, dropped) = confine_under_profile(refused);
        let mut lines = report.lines();
        let supported = lines.next().unwrap();
        assert!(supported.starts_with("supported: Err("), "{report}");
        assert!(supported.contains(reason), "{report}");
        assert_eq!(lines.next(), Some(format!("{REFUSAL}: {cause}").as_str()));
        assert_eq!(lines.next(), Some("os error: None"));
        // Nothing was restricted: the file is still readable.
        assert_eq!(lines.next(), Some("file: Ok(8)"));
        assert!(dropped, "the connected stream was dropped unused");
    }
}

#[test]
fn a_profile_that_refuses_restriction_makes_the_process_refuse_with_the_reason() {
    let (report, dropped) = confine_under_profile("restrict EPERM");
    assert_eq!(
        report,
        format!(
            "supported: Ok(())\n{REFUSAL}: landlock_restrict_self failed with EPERM (Operation \
             not permitted (os error 1))\nos error: None\nfile: Ok(8)\n"
        )
    );
    assert!(dropped);
}

#[test]
fn a_profile_that_refuses_seccomp_leaves_a_half_confined_process_that_refuses() {
    let (report, dropped) = confine_under_profile("seccomp EPERM");
    let mut lines = report.lines();
    let supported = lines.next().unwrap();
    assert!(
        supported.contains("seccomp failed with EPERM, as under a profile that refuses it"),
        "{report}"
    );
    assert_eq!(
        lines.next(),
        Some(
            format!("{REFUSAL}: seccomp failed with EPERM (Operation not permitted (os error 1))")
                .as_str()
        )
    );
    assert_eq!(lines.next(), Some("os error: None"));
    // Landlock already holds: the process is half confined, and it refused.
    assert_eq!(
        lines.next(),
        Some(format!("file: Err(Some({}))", libc::EACCES).as_str())
    );
    assert!(dropped, "the connected stream was dropped unused");
}

/// A report as the host builds it: the ready line has no seccomp field.
fn report_of(abi: u32) -> Enforcement {
    Enforcement {
        landlock_abi: Some(abi),
        seccomp: false,
    }
}

#[test]
fn the_host_accepts_a_confined_process_and_refuses_a_low_report_of_it() {
    let fixture = Fixture::new();
    let mut child = Child::start("stay confined", &fixture, "");
    let stream = fixture.accept(&mut child).unwrap();
    // The connection is made before the filter: wait for the report of confinement.
    child.reported(&confined_line());
    let abi = landlock::enforced_abi(landlock::abi().unwrap());
    confinement::verify_web(child.id(), &report_of(abi), &[]).unwrap();
    confinement::verify_web(child.id(), &report_of(3), &[&fixture.file]).unwrap();
    confinement::verify_web(
        child.id(),
        &Enforcement {
            landlock_abi: Some(abi),
            seccomp: true,
        },
        &[],
    )
    .unwrap();
    for low in [0, 1, 2] {
        let error = confinement::verify_web(child.id(), &report_of(low), &[]).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("{REFUSAL}: the ready line reports Landlock ABI {low}, below 3")
        );
    }
    let error = confinement::verify_web(
        child.id(),
        &Enforcement {
            landlock_abi: None,
            seccomp: true,
        },
        &[],
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        format!("{REFUSAL}: the ready line reports no Landlock domain")
    );
    drop(stream);
    assert_eq!(child.finish(), (Ended::Exited(0), String::new()));
}

#[test]
fn the_host_refuses_a_process_that_is_not_confined() {
    // This test process itself: it installed no filter, whatever it inherited.
    let text = confinement::verify_web(std::process::id(), &report_of(7), &[])
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

#[test]
fn the_host_refuses_a_process_with_no_new_privileges_but_no_filter() {
    let fixture = Fixture::new();
    let mut child = Child::start("privileges only", &fixture, "");
    child.reported("ready\n");
    let text = confinement::verify_web(child.id(), &report_of(7), &[])
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
fn host_under_a_profile(report: &Report, rendezvous: &Path) {
    profile::enter(&profile::refusing(&[], 0)).unwrap();
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
            Ok((mut stream, _)) => {
                let _ = stream.read(&mut [0u8]);
                0
            }
            Err(_) => 9,
        }
    });
    let (mut stream, _) = listener.accept().unwrap();
    // The connection is made before the filter: wait until the filter holds.
    let until = Instant::now() + WAIT;
    let accepted = loop {
        match confinement::verify_web(web as u32, &report_of(3), &[]) {
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
/// accepts only a child with one more than its own.
#[test]
fn under_a_profile_the_host_tells_an_inherited_filter_from_the_web_processs_own() {
    let fixture = Fixture::new();
    let (ended, report) = Child::start("host under a profile", &fixture, "").finish();
    assert_eq!(ended, Ended::Exited(0), "{report}");
    let mut lines = report.lines();
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
