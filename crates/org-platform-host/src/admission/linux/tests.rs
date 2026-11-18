//! Tests of the Linux adapter against the real kernel, with a compiled worker
//! (`examples/admission_fixture.rs`). They run as any user. Where a rule depends on
//! the user (root reads what an ordinary user cannot), the test says which part it
//! can check as which.

use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::fs::{symlink, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use super::sys::CallFailed;
use super::{image, spawn, sys};
use crate::admission::test_support::{
    child_peer, path_admission, scripted_admission, unowned_child, unreleased, Script, Step,
};
use crate::admission::{
    admission, native_peers, own_image, own_signing, peer, static_images, Admission,
    AdmissionError, Child, ChildEnv, ExpectedImage, ImageRefusal, NotTheChild, OwnImageError, Peer,
    SigningRule, SpawnRefused, Started,
};

/// Plain test data: Linux reads none of it.
const RULE: SigningRule = SigningRule {
    required_flags: 0,
    rejected_entitlements: &[],
    absent_entitlements: &[],
};

const PATIENCE: Duration = Duration::from_secs(20);

/// The fence of this test binary: held while it writes a file that is later executed
/// by its path, and by every start of a process.
///
/// A process copied while another thread holds such a file open for writing keeps
/// that descriptor until its own `exec`, and until then the kernel refuses to execute
/// the file by path (`ETXTBSY`). The sealed image is never executed by path, so only
/// the tests that start a program by path (the path adapter, the fixture as an
/// admitting process) could meet that; they did, about once in thirty runs, before
/// every writer and every start shared this lock.
static FENCE: Mutex<()> = Mutex::new(());

fn fenced() -> MutexGuard<'static, ()> {
    FENCE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn development(program: &Path) -> ExpectedImage {
    ExpectedImage {
        program: program.to_owned(),
        sha256: None,
        macos_cdhash: None,
        macos_requirement: None,
        macos_signing: RULE,
        development: true,
    }
}

fn release(program: &Path, sha256: [u8; 32]) -> ExpectedImage {
    ExpectedImage {
        sha256: Some(sha256),
        development: false,
        ..development(program)
    }
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn is_root() -> bool {
    sys::effective_uid() == 0
}

/// The compiled fixture, as `cargo test` built it beside this test binary.
fn built_fixture() -> PathBuf {
    let test_binary = std::env::current_exe().unwrap();
    let profile = test_binary.parent().and_then(Path::parent).unwrap();
    let fixture = profile.join("examples").join("admission_fixture");
    assert!(
        fixture.is_file(),
        "{} is not built: `cargo test -p org-platform-host` builds it; a run of one test \
         target alone needs `cargo build -p org-platform-host --example admission_fixture` \
         first",
        fixture.display()
    );
    fixture
}

/// A private directory with a copy of the fixture that only its owner can write, as a
/// pinned image must be. (A build directory may be writable by others.)
struct Workspace {
    dir: tempfile::TempDir,
    fixture: PathBuf,
}

impl Workspace {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        // Another user (the root variant of the non-dumpable test) must reach the copy.
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
        let fixture = dir.path().join("admission_fixture");
        let _no_process_is_started = fenced();
        fs::copy(built_fixture(), &fixture).unwrap();
        fs::set_permissions(&fixture, fs::Permissions::from_mode(0o755)).unwrap();
        Self { dir, fixture }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    /// An admission of the fixture as a development image.
    fn admission(&self) -> Admission {
        admission(development(&self.fixture)).unwrap()
    }

    /// An executable file `name` with `contents`.
    fn executable(&self, name: &str, contents: &[u8]) -> PathBuf {
        let path = self.path(name);
        let _no_process_is_started = fenced();
        fs::write(&path, contents).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }
}

fn spawn_with(
    admission: &Admission,
    commands: &[&str],
    env: &ChildEnv,
) -> std::io::Result<Started> {
    let commands: Vec<&OsStr> = commands.iter().map(OsStr::new).collect();
    admission.spawn(&commands, env, &FENCE)
}

fn start(admission: &Admission, commands: &[&str]) -> Started {
    spawn_with(admission, commands, &ChildEnv::new()).unwrap()
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let until = Instant::now() + PATIENCE;
    while !done() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn wait_exit(child: &mut Child) {
    wait_for("the child's exit", || child.try_wait().unwrap());
}

/// Whether `pid` names a process (running or a zombie).
fn exists(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

/// What the fixture's `report` command printed.
#[derive(Debug, Default)]
struct Report {
    args: Vec<String>,
    env: Vec<String>,
    /// Each open descriptor with the target of its link.
    fds: Vec<(String, String)>,
    values: Vec<(String, String)>,
}

impl Report {
    /// Reads lines up to `end`.
    fn read(lines: &mut impl Iterator<Item = std::io::Result<String>>) -> Self {
        let mut report = Self::default();
        for line in lines {
            let line = line.unwrap();
            if line == "end" {
                return report;
            }
            let (key, value) = line.split_once('=').expect("a key and a value");
            let pair = || {
                let (a, b) = value.split_once('=').expect("two values");
                (a.to_owned(), b.to_owned())
            };
            match key {
                "arg" => report.args.push(pair().1),
                "env" => report.env.push(value.to_owned()),
                "fd" => report.fds.push(pair()),
                _ => report.values.push((key.to_owned(), value.to_owned())),
            }
        }
        panic!("the report ended early: {report:?}");
    }

    fn value(&self, key: &str) -> &str {
        let found = self.values.iter().find(|(name, _)| name == key);
        &found.unwrap_or_else(|| panic!("no {key} in {self:?}")).1
    }

    /// The descriptors the process was started with: all but the one its own listing
    /// of `/proc/self/fd` holds open.
    fn inherited_fds(&self) -> Vec<(&str, &str)> {
        self.fds
            .iter()
            .filter(|(_, target)| !target.ends_with("/fd"))
            .map(|(fd, target)| (fd.as_str(), target.as_str()))
            .collect()
    }
}

/// Reads the child's report from its standard output, one byte at a time so that
/// nothing after it is taken from the pipe.
fn report_of(started: &mut Started) -> Report {
    let mut lines = std::iter::from_fn(|| Some(read_line(&mut started.stdout)));
    Report::read(&mut lines)
}

/// One line of `source`, without its end, reading nothing beyond it.
fn read_line(source: &mut File) -> std::io::Result<String> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        if source.read(&mut byte)? == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        if byte[0] == b'\n' {
            return Ok(String::from_utf8_lossy(&line).into_owned());
        }
        line.push(byte[0]);
    }
}

/// Accepts one connection, without waiting forever for a child that never connects.
fn accept(listener: &UnixListener) -> UnixStream {
    listener.set_nonblocking(true).unwrap();
    let until = Instant::now() + PATIENCE;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                return stream;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < until, "nothing connected");
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(error) => panic!("{error}"),
        }
    }
}

fn image_refusal<T>(result: Result<T, AdmissionError>) -> ImageRefusal {
    match result {
        Err(AdmissionError::Image(refusal)) => refusal,
        Err(other) => panic!("not an image refusal: {other}"),
        Ok(_) => panic!("the image was admitted"),
    }
}

fn failed_call<T>(result: std::io::Result<T>) -> (&'static str, Option<i32>) {
    let error = result.err().expect("the call succeeded");
    let failed = CallFailed::of(&error).unwrap_or_else(|| panic!("no call is named: {error}"));
    assert!(error.to_string().starts_with(failed.call), "{error}");
    (failed.call, failed.error.raw_os_error())
}

// ---------------------------------------------------------------------------------
// Decisions and error construction, without a child.

#[test]
fn memfd_create_is_retried_without_mfd_exec_only_when_the_kernel_does_not_know_the_flag() {
    assert!(sys::retry_without_exec(libc::EINVAL));
    // `vm.memfd_noexec = 2`, a profile's refusal, a missing call, exhaustion: final.
    for errno in [
        libc::EACCES,
        libc::EPERM,
        libc::ENOSYS,
        libc::EMFILE,
        libc::ENFILE,
        libc::ENOMEM,
        0,
    ] {
        assert!(!sys::retry_without_exec(errno), "{errno}");
    }
}

#[test]
fn a_failed_call_names_the_call_and_carries_the_systems_error() {
    for (call, errno) in [
        ("memfd_create", libc::EACCES),
        ("clone", libc::EPERM),
        ("close_range", libc::ENOSYS),
        ("execveat", libc::EPERM),
    ] {
        let error = sys::failed(call, errno);
        let system = std::io::Error::from_raw_os_error(errno);
        assert_eq!(error.kind(), system.kind());
        assert_eq!(error.to_string(), format!("{call} failed: {system}"));
        let failed = CallFailed::of(&error).unwrap();
        assert_eq!(
            (failed.call, failed.error.raw_os_error()),
            (call, Some(errno))
        );
        // It is neither of the contract's classified refusals.
        assert!(SpawnRefused::of(&error).is_none() && !NotTheChild::is(&error));
    }
    // An error that names no call is not taken for one.
    assert!(CallFailed::of(&std::io::Error::from_raw_os_error(libc::EPERM)).is_none());
}

/// The child reports a failed step as two integers; the host turns them into the
/// call's name and the system's error, and reads anything else as a failure too.
#[test]
fn a_childs_failed_step_is_reported_by_the_name_of_its_call() {
    let named: Vec<&str> = (1..=6).map(spawn::step_name).collect();
    assert_eq!(
        named,
        [
            "sigaction",
            "dup3",
            "chdir",
            "close_range",
            "sigprocmask",
            "execveat"
        ]
    );
    assert_eq!(spawn::step_name(0), "an unknown step of the child");
    assert_eq!(spawn::step_name(7), "an unknown step of the child");

    // A stand-in for the child: this process, never signalled and never waited for.
    let stand_in = || spawn::Child::unowned(std::process::id(), None).unwrap();
    let report = |bytes: &[u8]| {
        let (read, write) = sys::pipe().unwrap();
        File::from(write).write_all(bytes).unwrap();
        spawn::read_report(stand_in(), read)
    };
    let message = |step: i32, errno: i32| [step.to_ne_bytes(), errno.to_ne_bytes()].concat();
    // End of file with nothing written: the image was executed.
    assert!(report(&[]).is_ok());
    assert_eq!(
        failed_call(report(&message(4, libc::EPERM))),
        ("close_range", Some(libc::EPERM))
    );
    assert_eq!(
        failed_call(report(&message(6, libc::EACCES))),
        ("execveat", Some(libc::EACCES))
    );
    let cut = report(&message(6, libc::EACCES)[..5]).err().unwrap();
    assert!(cut.to_string().contains("cut short"), "{cut}");
}

#[test]
fn bytes_are_started_only_with_the_expected_digest_or_as_a_development_image() {
    let program = Path::new("/x/program");
    let read = [7; 32];
    assert_eq!(image::judge(&release(program, read), &read), Ok(()));
    let mut off_by_one_bit = read;
    off_by_one_bit[31] ^= 1;
    assert_eq!(
        image::judge(&release(program, off_by_one_bit), &read),
        Err(ImageRefusal::DigestMismatch)
    );
    // A development image with a digest is still held to it.
    let pinned_development = ExpectedImage {
        development: true,
        ..release(program, off_by_one_bit)
    };
    assert_eq!(
        image::judge(&pinned_development, &read),
        Err(ImageRefusal::DigestMismatch)
    );
    assert_eq!(image::judge(&development(program), &read), Ok(()));
    let unpinned_release = ExpectedImage {
        development: false,
        ..development(program)
    };
    assert_eq!(
        image::judge(&unpinned_release, &read),
        Err(ImageRefusal::Unpinned)
    );
    // The macOS fields are not read.
    let with_macos_data = ExpectedImage {
        macos_cdhash: Some([1; 20]),
        macos_requirement: Some("anchor apple".into()),
        ..release(program, read)
    };
    assert_eq!(image::judge(&with_macos_data, &read), Ok(()));
}

#[test]
fn only_a_regular_file_that_the_service_user_or_root_alone_can_write_is_read() {
    const USER: libc::uid_t = 1000;
    let regular = |mode: libc::mode_t| libc::S_IFREG | mode;
    for mode in [0o755, 0o700, 0o555, 0o500, 0o644] {
        assert_eq!(image::fit(regular(mode), USER, USER), Ok(()), "{mode:o}");
        assert_eq!(
            image::fit(regular(mode), 0, USER),
            Ok(()),
            "{mode:o} of root"
        );
    }
    for mode in [0o775, 0o757, 0o777, 0o720, 0o702] {
        assert_eq!(
            image::fit(regular(mode), USER, USER),
            Err(ImageRefusal::UnsafeFile(
                "is writable by its group or by others"
            )),
            "{mode:o}"
        );
    }
    for owner in [1001, 65534] {
        assert_eq!(
            image::fit(regular(0o755), owner, USER),
            Err(ImageRefusal::UnsafeFile("is owned by another user")),
            "{owner}"
        );
    }
    // Root as the service user trusts no other owner either.
    assert!(image::fit(regular(0o755), USER, 0).is_err());
    for kind in [
        libc::S_IFDIR,
        libc::S_IFLNK,
        libc::S_IFIFO,
        libc::S_IFSOCK,
        libc::S_IFCHR,
        libc::S_IFBLK,
    ] {
        assert_eq!(
            image::fit(kind | 0o755, USER, USER),
            Err(ImageRefusal::UnsafeFile("is not a regular file")),
            "{kind:o}"
        );
    }
}

#[test]
fn a_child_environment_with_a_loader_variable_cannot_be_built() {
    let mut env = ChildEnv::new();
    for name in [
        "LD_PRELOAD",
        "LD_LIBRARY_PATH",
        "LD_AUDIT",
        "GLIBC_TUNABLES",
        "DYLD_INSERT_LIBRARIES",
    ] {
        let error = env.set(name, "/tmp/injected.so").unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput, "{name}");
    }
    // Nothing was added: there is no environment to start a child with that holds one.
    assert_eq!(env.pairs(), []);
}

// ---------------------------------------------------------------------------------
// The sealed image.

/// The copy is what was read: its digest is the file's, a write, a truncation and a
/// further seal change are refused, and the user cannot read it back by path.
#[test]
fn the_image_is_sealed_and_cannot_be_read_back_by_this_user() {
    let work = Workspace::new();
    let bytes = fs::read(&work.fixture).unwrap();
    let sealed = image::SealedImage::of(&development(&work.fixture)).unwrap();
    assert_eq!(sealed.sha256(), sha256(&bytes));
    sealed.intact().unwrap();
    let fd = sealed.file().as_raw_fd();
    assert!(fd >= sys::FIRST_FREE);
    let errno = || std::io::Error::last_os_error().raw_os_error();
    // SAFETY: a valid descriptor; these calls read its flags, try to change the
    // sealed file and are expected to fail.
    unsafe {
        assert_eq!(
            libc::fcntl(fd, libc::F_GETFD) & libc::FD_CLOEXEC,
            libc::FD_CLOEXEC
        );
        assert_eq!(libc::fcntl(fd, libc::F_GET_SEALS), image::SEALS);
        assert_eq!(libc::pwrite(fd, b"x".as_ptr().cast(), 1, 0), -1);
        assert_eq!(errno(), Some(libc::EPERM), "a write after sealing");
        assert_eq!(libc::write(fd, b"x".as_ptr().cast(), 1), -1);
        assert_eq!(errno(), Some(libc::EPERM), "an append after sealing");
        assert_eq!(libc::ftruncate(fd, 0), -1);
        assert_eq!(errno(), Some(libc::EPERM), "a truncation after sealing");
        assert_eq!(libc::fcntl(fd, libc::F_ADD_SEALS, libc::F_SEAL_WRITE), -1);
        assert_eq!(errno(), Some(libc::EPERM), "a seal change after sealing");
    }
    let stat = sys::status(sealed.file()).unwrap();
    assert_eq!(stat.st_mode, libc::S_IFREG | image::EXECUTE_ONLY);
    assert_eq!(stat.st_size as usize, bytes.len());
    // By path the file is execute-only. Root reads any file, so only another user
    // shows it.
    let by_path = File::open(format!("/proc/self/fd/{fd}"));
    if is_root() {
        assert!(by_path.is_ok());
    } else {
        assert_eq!(
            by_path.unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        let writable = fs::OpenOptions::new()
            .write(true)
            .open(format!("/proc/self/fd/{fd}"));
        assert_eq!(
            writable.unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
    }
    // Still the bytes that were read, after the pinned file is replaced.
    fs::write(&work.fixture, b"replaced").unwrap();
    sealed.intact().unwrap();
    assert_eq!(sealed.sha256(), sha256(&bytes));
}

#[test]
fn an_image_one_byte_off_its_expected_digest_is_refused_and_nothing_is_started() {
    let work = Workspace::new();
    let original = fs::read(&work.fixture).unwrap();
    let expected = release(&work.fixture, sha256(&original));
    // The expected bytes are admitted, and the admission holds their digest.
    let admitted = admission(expected.clone()).unwrap();
    assert_eq!(admitted.image_sha256(), expected.sha256);
    drop(admitted);
    // One byte of the file changed: no admission exists, so nothing can be started.
    let mut changed = original.clone();
    let last = changed.len() - 1;
    changed[last] ^= 1;
    fs::write(&work.fixture, &changed).unwrap();
    assert_eq!(
        image_refusal(admission(expected.clone())),
        ImageRefusal::DigestMismatch
    );
    // The same as an `io::Error`, as the host takes it.
    let error = std::io::Error::from(admission(expected.clone()).err().unwrap());
    assert_eq!(
        error
            .get_ref()
            .and_then(|source| source.downcast_ref::<ImageRefusal>()),
        Some(&ImageRefusal::DigestMismatch)
    );
    // One byte fewer, and one more.
    for other in [&original[..last], &[&original[..], b"\0"].concat()[..]] {
        fs::write(&work.fixture, other).unwrap();
        assert_eq!(
            image_refusal(admission(expected.clone())),
            ImageRefusal::DigestMismatch
        );
    }
    // The right file under a digest that is one bit off.
    fs::write(&work.fixture, &original).unwrap();
    let mut digest = sha256(&original);
    digest[0] ^= 0x80;
    assert_eq!(
        image_refusal(admission(release(&work.fixture, digest))),
        ImageRefusal::DigestMismatch
    );
    assert!(admission(expected).is_ok());
}

#[test]
fn a_release_image_needs_a_digest_and_a_development_image_records_the_one_it_read() {
    let work = Workspace::new();
    let unpinned_release = ExpectedImage {
        development: false,
        ..development(&work.fixture)
    };
    assert_eq!(
        image_refusal(admission(unpinned_release)),
        ImageRefusal::Unpinned
    );
    // Refused before anything is read: not even a missing file is noticed.
    let nothing_there = ExpectedImage {
        development: false,
        ..development(&work.path("missing"))
    };
    assert_eq!(
        image_refusal(admission(nothing_there)),
        ImageRefusal::Unpinned
    );
    let recorded = work.admission().image_sha256();
    assert_eq!(recorded, Some(sha256(&fs::read(&work.fixture).unwrap())));
}

/// What stands at the pinned path must be a regular file that only the service user
/// or root can write; a symbolic link is not followed.
#[test]
fn the_pinned_path_must_be_a_regular_file_of_this_user_or_root_that_no_one_else_can_write() {
    let work = Workspace::new();
    let link = work.path("link");
    symlink(&work.fixture, &link).unwrap();
    match admission(development(&link)) {
        Err(AdmissionError::System(error)) => {
            assert_eq!(failed_call::<()>(Err(error)), ("open", Some(libc::ELOOP)));
        }
        _ => panic!("a symbolic link was followed"),
    }
    match admission(development(&work.path("missing"))) {
        Err(AdmissionError::System(error)) => {
            assert_eq!(failed_call::<()>(Err(error)), ("open", Some(libc::ENOENT)));
        }
        _ => panic!("a missing file was admitted"),
    }

    let not_regular = ImageRefusal::UnsafeFile("is not a regular file");
    assert_eq!(
        image_refusal(admission(development(work.dir.path()))),
        not_regular
    );
    // A FIFO nobody writes to: refused, not waited for.
    let fifo = work.path("fifo");
    let fifo_c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
    // SAFETY: a NUL-terminated path in this test's own directory.
    assert_eq!(unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o600) }, 0);
    assert_eq!(image_refusal(admission(development(&fifo))), not_regular);

    let writable = ImageRefusal::UnsafeFile("is writable by its group or by others");
    for mode in [0o775, 0o757, 0o777] {
        fs::set_permissions(&work.fixture, fs::Permissions::from_mode(mode)).unwrap();
        assert_eq!(
            image_refusal(admission(development(&work.fixture))),
            writable,
            "{mode:o}"
        );
    }
    fs::set_permissions(&work.fixture, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(admission(development(&work.fixture)).is_ok());

    // A file of another user. Only root can make one: as any other user this test
    // cannot create the file, and the rule itself is tested in
    // `only_a_regular_file_that_the_service_user_or_root_alone_can_write_is_read`.
    if is_root() {
        std::os::unix::fs::chown(&work.fixture, Some(65534), Some(65534)).unwrap();
        assert_eq!(
            image_refusal(admission(development(&work.fixture))),
            ImageRefusal::UnsafeFile("is owned by another user")
        );
    }
}

/// A root-owned system program is admitted by any user and runs to its end.
#[test]
fn a_program_that_root_owns_is_admitted() {
    let program = Path::new("/usr/bin/true");
    let owned_by_root = admission(development(program)).unwrap();
    assert_eq!(
        owned_by_root.image_sha256(),
        Some(sha256(&fs::read(program).unwrap()))
    );
    let mut started = start(&owned_by_root, &[]);
    wait_exit(&mut started.child);
    assert_eq!(started.child.ended().as_deref(), Some("exit code 0"));
}

// ---------------------------------------------------------------------------------
// The start.

/// The child runs the sealed image with its pipes on 0 to 2, the given arguments,
/// exactly the given environment, `/` as its working directory and no other
/// descriptor, whatever this process holds and whatever its own environment is.
#[test]
fn a_child_gets_its_pipes_its_environment_and_nothing_else() {
    let work = Workspace::new();
    // A descriptor of this process that is not close-on-exec: only the child's own
    // closing keeps it out.
    let leaky = File::open("/dev/null").unwrap();
    // SAFETY: a valid descriptor of this test; clears only its close-on-exec flag.
    assert_eq!(
        unsafe { libc::fcntl(leaky.as_raw_fd(), libc::F_SETFD, 0) },
        0
    );
    assert!(
        std::env::vars_os().count() > 0,
        "this test needs an environment of its own"
    );

    let mut env = ChildEnv::new();
    env.set("ONLY", "this").unwrap();
    env.set("SECOND", "two words = kept").unwrap();
    let admission = work.admission();
    let mut started = spawn_with(&admission, &["report", "echo"], &env).unwrap();
    // The start returned, so the child executes the image already: its command line
    // is the image's, not this test binary's.
    let command_line = fs::read(format!("/proc/{}/cmdline", started.child.id())).unwrap();
    let expected_line = [work.fixture.to_str().unwrap(), "report", "echo", ""].join("\0");
    assert_eq!(String::from_utf8_lossy(&command_line), expected_line);

    let report = report_of(&mut started);
    assert_eq!(
        report.args,
        [work.fixture.to_str().unwrap(), "report", "echo"]
    );
    assert_eq!(report.env, ["ONLY=this", "SECOND=two words = kept"]);
    assert_eq!(report.value("cwd"), "/");
    let inherited = report.inherited_fds();
    let numbers: Vec<&str> = inherited.iter().map(|(fd, _)| *fd).collect();
    assert_eq!(numbers, ["0", "1", "2"], "{report:?}");
    for (fd, target) in &inherited {
        assert!(target.starts_with("pipe:"), "fd {fd}: {report:?}");
    }
    assert_eq!(
        report.value("euid").parse::<u32>().unwrap(),
        sys::effective_uid()
    );
    // Its executable is the memory file, which has no path: it cannot find programs
    // beside itself.
    for link in ["exe", "current_exe"] {
        assert_eq!(report.value(link), "/memfd:admission_fixture (deleted)");
    }

    // The three pipes are wired as standard input, output and error.
    started.stdin.write_all(b"through the pipes\n").unwrap();
    let mut err = String::new();
    assert_eq!(
        read_line(&mut started.stdout).unwrap(),
        "echo=through the pipes"
    );
    started.stderr.read_to_string(&mut err).unwrap();
    assert_eq!(err, "stderr=through the pipes\n");
    wait_exit(&mut started.child);
    assert_eq!(started.child.ended().as_deref(), Some("exit code 0"));
    drop(leaky);

    // An empty environment is empty, and a poisoned fence is taken all the same.
    let fence = Arc::new(Mutex::new(()));
    let poison = Arc::clone(&fence);
    let _ = std::thread::spawn(move || {
        let _held = poison.lock().unwrap();
        panic!("poisoning the fence (expected)");
    })
    .join();
    assert!(fence.is_poisoned());
    let mut bare = {
        let _no_file_is_written = fenced();
        admission
            .spawn(&[OsStr::new("report")], &ChildEnv::new(), &fence)
            .unwrap()
    };
    let report = report_of(&mut bare);
    assert_eq!(report.env, [] as [&str; 0]);
    assert_eq!(report.args.len(), 2);
}

/// Design section 3.1: an execute-only image starts a child of an unprivileged user
/// non-dumpable, so no process of that user can read its memory or follow its
/// executable link. The property does not hold for root, which reads any file.
///
/// The adapter runs here in a fixture process (`admit`), unchanged. As root that
/// process is started as user 65534, so the test checks the same thing whoever runs
/// it; as any other user it runs as that user, and the test also looks itself.
/// Sealing an image and starting a child both need a process that others of its user
/// cannot open: the copy is writable until it is sealed, and a child is a copy of this
/// process until it executes the image. The admission makes it so itself.
#[test]
fn building_an_admission_closes_this_process_to_other_processes() {
    let work = Workspace::new();
    // SAFETY: `prctl` with scalar arguments; it sets and reads one attribute.
    let dumpable = || unsafe { libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) };
    let _sealed = image::SealedImage::of(&development(&work.fixture)).unwrap();
    assert_eq!(dumpable(), 0, "after an image was sealed");
    // Even when something opened it again in between, a start closes it first.
    let admission = work.admission();
    // SAFETY: as above.
    assert_eq!(unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 1, 0, 0, 0) }, 0);
    let mut started = start(&admission, &["exit", "0"]);
    assert_eq!(dumpable(), 0, "after a child was started");
    wait_exit(&mut started.child);
}

#[test]
fn the_child_of_an_unprivileged_user_is_not_dumpable() {
    use std::os::unix::process::CommandExt;
    let work = Workspace::new();
    let mut admitter = std::process::Command::new(&work.fixture);
    admitter
        .args(["admit", work.fixture.to_str().unwrap(), "report", "hold"])
        .env_clear()
        .current_dir("/");
    if is_root() {
        admitter.uid(65534).gid(65534);
    }
    admitter
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let running = {
        let _no_file_is_written = fenced();
        admitter.spawn().unwrap()
    };
    let output = running.wait_with_output().unwrap();
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        output.status.success(),
        "{text}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut lines = text.lines().map(|line| Ok(line.to_owned()));
    let report = Report::read(&mut lines);
    let rest: Vec<String> = lines.map(Result::unwrap).collect();
    let user: u32 = report.value("euid").parse().unwrap();
    assert_ne!(user, 0, "the child must run as an unprivileged user");
    assert_eq!(
        user,
        if is_root() {
            65534
        } else {
            sys::effective_uid()
        }
    );
    // Root owns the memory of a process that is not dumpable.
    assert_eq!(report.value("mem_uid"), "0", "{report:?}");
    // The child reads its own executable link. The process that started it can
    // neither read that link nor open the child's memory, though both are one user.
    assert_eq!(report.value("exe"), "/memfd:admission_fixture (deleted)");
    assert_eq!(
        rest,
        ["probe_mem=PermissionDenied", "probe_exe=PermissionDenied"]
    );

    if !is_root() {
        // The same from this process, the child's own parent.
        let mut started = start(&work.admission(), &["report", "hold"]);
        let report = report_of(&mut started);
        assert_eq!(report.value("mem_uid"), "0");
        let pid = started.child.id();
        for refused in [
            File::open(format!("/proc/{pid}/mem")).map(drop),
            fs::read_link(format!("/proc/{pid}/exe")).map(drop),
            fs::read_dir(format!("/proc/{pid}/fd")).map(drop),
        ] {
            assert_eq!(
                refused.unwrap_err().kind(),
                std::io::ErrorKind::PermissionDenied
            );
        }
        // What the design says stays readable: so no secret is ever an argument.
        assert!(fs::read(format!("/proc/{pid}/cmdline")).is_ok());
        assert!(fs::read_to_string(format!("/proc/{pid}/status")).is_ok());
    }
}

/// Only compiled executables are admitted: the kernel cannot start an interpreter
/// script from the sealed descriptor, because the interpreter would have to open a
/// descriptor that is closed by then.
#[test]
fn a_script_cannot_be_started_from_a_sealed_image() {
    let work = Workspace::new();
    let script = work.executable("script", b"#!/bin/sh\necho ran\n");
    // Its bytes are sealed like any others; the start is what the kernel refuses.
    let sealed_script = admission(development(&script)).unwrap();
    let refused = spawn_with(&sealed_script, &[], &ChildEnv::new());
    assert_eq!(failed_call(refused), ("execveat", Some(libc::ENOENT)));
    // Neither is a file that is no executable at all.
    let text = work.executable("text", b"not a program\n");
    let sealed_text = admission(development(&text)).unwrap();
    let refused = spawn_with(&sealed_text, &[], &ChildEnv::new());
    assert_eq!(failed_call(refused), ("execveat", Some(libc::ENOEXEC)));
}

/// An argument with a NUL byte is refused before any process exists.
#[test]
fn an_argument_with_a_nul_byte_starts_nothing() {
    let work = Workspace::new();
    let error = spawn_with(&work.admission(), &["a\0b"], &ChildEnv::new())
        .err()
        .unwrap();
    assert_eq!(error.to_string(), "an argument holds a NUL byte");
}

// ---------------------------------------------------------------------------------
// The child's handle.

#[test]
fn a_child_is_reaped_once_and_reports_how_it_ended() {
    let work = Workspace::new();
    let admission = work.admission();
    let mut exiting = start(&admission, &["exit", "7"]);
    let pid = exiting.child.id();
    wait_exit(&mut exiting.child);
    assert_eq!(exiting.child.ended().as_deref(), Some("exit code 7"));
    assert!(!exists(pid), "the child was not reaped");
    // Asking again reaps nothing more and answers the same; killing it does nothing.
    assert!(exiting.child.try_wait().unwrap());
    exiting.child.kill();
    assert_eq!(exiting.child.ended().as_deref(), Some("exit code 7"));

    let mut running = start(&admission, &["hold"]);
    let pid = running.child.id();
    assert!(!running.child.try_wait().unwrap());
    assert_eq!(running.child.ended(), None);
    assert!(running.child.0.alive().unwrap());
    running.child.kill();
    assert_eq!(running.child.ended().as_deref(), Some("signal 9"));
    assert!(running.child.try_wait().unwrap());
    assert!(!exists(pid), "the killed child was not reaped");
    assert!(!running.child.0.alive().unwrap());
}

#[test]
fn dropping_a_child_kills_and_reaps_it() {
    let work = Workspace::new();
    let admission = work.admission();
    let started = start(&admission, &["hold"]);
    let pid = started.child.id();
    assert!(exists(pid));
    drop(started);
    assert!(!exists(pid), "a dropped child was left behind");

    // The same for a child that is held and never released.
    let held = admission
        .start(&[OsStr::new("hold")], &ChildEnv::new(), &FENCE)
        .unwrap();
    let pid = held.child().id();
    assert!(exists(pid));
    drop(held);
    assert!(!exists(pid), "a dropped held child was left behind");
}

/// There is nothing to resume on Linux: a held child was started from the verified
/// bytes and runs already, and releasing it changes nothing.
#[test]
fn a_held_child_already_runs_the_verified_image() {
    let work = Workspace::new();
    let admission = work.admission();
    let commands = [OsStr::new("report"), OsStr::new("hold")];
    let held = admission
        .start(&commands, &ChildEnv::new(), &FENCE)
        .unwrap();
    admission.recheck(held.child()).unwrap();
    // Not released, and it has reported all the same.
    let mut running = unreleased(held);
    assert_eq!(report_of(&mut running).args.len(), 3);

    let held = admission
        .start(&commands, &ChildEnv::new(), &FENCE)
        .unwrap();
    let pid = held.child().id();
    let mut released = held.release().unwrap();
    assert_eq!(released.child.id(), pid);
    assert_eq!(report_of(&mut released).args.len(), 3);
}

// ---------------------------------------------------------------------------------
// The connection's peer.

/// A connection is the child's when the child itself made it: not another child of
/// the same admission, not this process, and not once the child has exited.
#[test]
fn only_the_childs_own_connection_while_it_lives_is_its_reader() {
    let work = Workspace::new();
    let admission = work.admission();
    let socket = work.path("rendezvous");
    let listener = UnixListener::bind(&socket).unwrap();
    let commands = ["connect", socket.to_str().unwrap(), "hold"];

    let mut child = start(&admission, &commands);
    let from_child = accept(&listener);
    admission.verify_reader(&child.child, &from_child).unwrap();
    // Twice: the check consumes nothing.
    admission.verify_reader(&child.child, &from_child).unwrap();
    let seen = peer(&from_child).unwrap();
    assert_eq!(
        seen,
        Peer::from_linux_credentials(sys::effective_uid(), child.child.id())
    );
    assert_eq!(child_peer(child.child.id()).unwrap(), seen);

    // Another process of the same user, started from the same verified image.
    let other = start(&admission, &commands);
    let from_other = accept(&listener);
    let mismatch = admission
        .verify_reader(&child.child, &from_other)
        .unwrap_err();
    assert!(NotTheChild::is(&mismatch), "{mismatch}");
    admission.verify_reader(&other.child, &from_other).unwrap();
    let mismatch = admission
        .verify_reader(&other.child, &from_child)
        .unwrap_err();
    assert!(NotTheChild::is(&mismatch), "{mismatch}");

    // This process itself.
    let _own = UnixStream::connect(&socket).unwrap();
    let from_self = accept(&listener);
    let mismatch = admission
        .verify_reader(&child.child, &from_self)
        .unwrap_err();
    assert!(NotTheChild::is(&mismatch), "{mismatch}");
    assert_eq!(peer(&from_self).unwrap().pid, Some(std::process::id()));

    // The child exits and is not reaped yet: its PID is still its own, and its
    // connection is no longer admitted.
    let pid = child.child.id();
    drop(child.stdin);
    wait_for("the child's exit", || !child.child.0.alive().unwrap());
    assert!(exists(pid), "the exited child must still be unreaped here");
    let gone = admission
        .verify_reader(&child.child, &from_child)
        .unwrap_err();
    assert!(NotTheChild::is(&gone), "{gone}");
    assert!(admission.recheck(&child.child).is_err());
    // Reaped: the same.
    assert!(child.child.try_wait().unwrap());
    assert!(!exists(pid));
    let gone = admission
        .verify_reader(&child.child, &from_child)
        .unwrap_err();
    assert!(NotTheChild::is(&gone), "{gone}");
    // The connection still names the PID it was made with: a PID proves nothing
    // without the held process.
    assert_eq!(peer(&from_child).unwrap().pid, Some(pid));
}

#[test]
fn a_recheck_passes_while_the_child_lives_and_the_image_stands() {
    let work = Workspace::new();
    let admission = work.admission();
    let mut started = start(&admission, &["hold"]);
    admission.recheck(&started.child).unwrap();
    admission.readmit(&started.child).unwrap();
    // The pinned file is replaced after the start: the child runs the sealed bytes,
    // and so does every later child of this admission.
    fs::write(&work.fixture, b"#!/bin/sh\n").unwrap();
    admission.recheck(&started.child).unwrap();
    let mut later = start(&admission, &["report"]);
    assert_eq!(report_of(&mut later).args.len(), 2);

    started.child.kill();
    for refusal in [
        admission.recheck(&started.child),
        admission.readmit(&started.child),
    ] {
        assert_eq!(
            refusal,
            Err(ImageRefusal::Identity("the child has exited".into()))
        );
    }
}

#[test]
fn a_socket_pair_reports_this_process_and_no_code_signature_exists() {
    let (a, _b) = UnixStream::pair().unwrap();
    let own = peer(&a).unwrap();
    assert_eq!(
        own,
        Peer::from_linux_credentials(sys::effective_uid(), std::process::id())
    );
    assert_eq!(own.macos_audit_token, [0; 32]);
    native_peers().unwrap();
    // No requirement is satisfied, no signing information is read, and no static
    // image check exists.
    assert!(!own.satisfies("anchor apple", &RULE));
    assert!(own_signing().is_none());
    assert!(static_images().is_err());
    #[cfg(debug_assertions)]
    assert_eq!(
        crate::admission::peer_executable(&a).unwrap_err().kind(),
        std::io::ErrorKind::Unsupported
    );
}

#[test]
fn the_hosts_own_image_is_the_expected_one_only_by_its_digest() {
    let own = sha256(&fs::read("/proc/self/exe").unwrap());
    let program = std::env::current_exe().unwrap();
    own_image(&release(&program, own)).unwrap();
    // The path is not what is read: the running executable is.
    own_image(&release(Path::new("/nonexistent"), own)).unwrap();
    let mut other = own;
    other[0] ^= 1;
    for expected in [
        release(&program, other),
        development(&program),
        ExpectedImage {
            development: false,
            ..development(&program)
        },
        ExpectedImage {
            macos_cdhash: Some([0; 20]),
            ..development(&program)
        },
    ] {
        assert!(
            matches!(own_image(&expected), Err(OwnImageError::Mismatch)),
            "{expected:?}"
        );
    }
}

// ---------------------------------------------------------------------------------
// Test support.

fn script(
    answer: impl Fn(Step, u32) -> Result<Option<Peer>, String> + Send + Sync + 'static,
) -> Script {
    Arc::new(answer)
}

/// A scripted check stands in for a check of the started child. The child is still
/// started from the sealed image, and a refused one is killed and reaped.
#[test]
fn a_scripted_check_answers_for_a_real_child_of_the_sealed_image() {
    let work = Workspace::new();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&asked);
    let recording = scripted_admission(
        development(&work.fixture),
        script(move |step, pid| {
            seen.lock().unwrap().push((step, pid));
            Ok(None)
        }),
    )
    .unwrap();
    assert!(asked.lock().unwrap().is_empty(), "asked before any child");
    let mut started = start(&recording, &["report", "hold"]);
    let pid = started.child.id();
    assert_eq!(*asked.lock().unwrap(), [(Step::Admit, pid)]);
    assert_eq!(report_of(&mut started).args.len(), 3);
    // Admitted without an identity: a re-check asks the script nothing.
    recording.recheck(&started.child).unwrap();
    assert_eq!(asked.lock().unwrap().len(), 1);
    recording.readmit(&started.child).unwrap();
    assert_eq!(asked.lock().unwrap().last(), Some(&(Step::Admit, pid)));
    assert_eq!(asked.lock().unwrap().len(), 2);

    let refusing = scripted_admission(
        development(&work.fixture),
        script(|_, _| Err("refused by the test".into())),
    )
    .unwrap();
    let error = spawn_with(&refusing, &["hold"], &ChildEnv::new())
        .err()
        .unwrap();
    let refused = SpawnRefused::of(&error).expect("an image refusal");
    assert_eq!(
        refused.refusal,
        ImageRefusal::Scripted("refused by the test".into())
    );
    assert!(!exists(refused.pid), "a refused child was left behind");

    // Admitted with an identity: a re-check compares it, a re-admission too.
    let identity = Peer::from_macos_audit_token([7; 32]);
    let changing = scripted_admission(
        development(&work.fixture),
        script(move |step, _| match step {
            Step::Admit => Ok(Some(identity)),
            Step::Still => Ok(Some(Peer::from_macos_audit_token([8; 32]))),
        }),
    )
    .unwrap();
    let started = start(&changing, &["hold"]);
    assert_eq!(changing.recheck(&started.child), Err(ImageRefusal::Changed));
    changing.readmit(&started.child).unwrap();
    let another = scripted_admission(development(&work.fixture), script(|_, _| Ok(None))).unwrap();
    assert_eq!(
        another.readmit(&started.child),
        Err(ImageRefusal::Readmitted)
    );
}

/// The test adapter that starts by path: it starts a script, which the adapter itself
/// cannot, and still admits only the child's own connection.
#[test]
fn a_path_admission_starts_a_script_and_checks_its_reader_by_pid() {
    let work = Workspace::new();
    let socket = work.path("rendezvous");
    let listener = UnixListener::bind(&socket).unwrap();
    let connect = format!(
        "'{}' connect '{}' hold",
        work.fixture.display(),
        socket.display()
    );
    // The script becomes the fixture: the connection is made by the child's PID.
    let becoming = work.executable(
        "becoming",
        format!("#!/bin/sh\necho \"script=$ONLY $1 $(pwd)\"\nexec {connect}\n").as_bytes(),
    );
    let by_path = path_admission(development(&becoming)).unwrap();
    assert_eq!(by_path.image_sha256(), None);
    let mut env = ChildEnv::new();
    env.set("ONLY", "this").unwrap();
    let mut started = spawn_with(&by_path, &["argument"], &env).unwrap();
    assert_eq!(
        read_line(&mut started.stdout).unwrap(),
        "script=this argument /"
    );
    let stream = accept(&listener);
    by_path.verify_reader(&started.child, &stream).unwrap();
    by_path.recheck(&started.child).unwrap();
    assert_eq!(peer(&stream).unwrap().pid, Some(started.child.id()));
    let pid = started.child.id();
    started.child.kill();
    assert_eq!(started.child.ended().as_deref(), Some("signal 9"));
    assert!(!exists(pid));
    assert!(NotTheChild::is(
        &by_path.verify_reader(&started.child, &stream).unwrap_err()
    ));

    // The script starts the fixture as a process of its own (which ends at once, on
    // the empty standard input of a background job) and stays: the connection is not
    // the child's, and is refused.
    let delegating = work.executable(
        "delegating",
        format!("#!/bin/sh\n{connect} &\nwait\nread line\n").as_bytes(),
    );
    let by_path = path_admission(development(&delegating)).unwrap();
    let started = start(&by_path, &[]);
    let stream = accept(&listener);
    let job = peer(&stream).unwrap().pid.unwrap();
    assert_ne!(job, started.child.id());
    assert!(NotTheChild::is(
        &by_path.verify_reader(&started.child, &stream).unwrap_err()
    ));
    // The script reaps its own job before it is ended, so no orphan is left.
    wait_for("the script's job to be reaped", || !exists(job));
    assert!(started.child.0.alive().unwrap());
    drop(started);

    // It gives the child nothing but its pipes either.
    let reporting = path_admission(development(&work.fixture)).unwrap();
    let leaky = File::open("/dev/null").unwrap();
    // SAFETY: a valid descriptor of this test; clears only its close-on-exec flag.
    assert_eq!(
        unsafe { libc::fcntl(leaky.as_raw_fd(), libc::F_SETFD, 0) },
        0
    );
    let mut started = start(&reporting, &["report"]);
    let report = report_of(&mut started);
    let numbers: Vec<&str> = report.inherited_fds().iter().map(|(fd, _)| *fd).collect();
    assert_eq!(numbers, ["0", "1", "2"], "{report:?}");
    assert_eq!(report.env, [] as [&str; 0]);
    assert_eq!(report.value("cwd"), "/");
    // By path the child is an ordinary process of its file.
    assert_eq!(report.value("exe"), work.fixture.to_str().unwrap());
}

/// A stand-in for a child the test does not own: checked like a child, and never
/// signalled or waited for.
#[test]
fn an_unowned_child_is_checked_and_never_signalled() {
    let work = Workspace::new();
    let admission = work.admission();
    let me = std::process::id();
    let mut stand_in = unowned_child(me, None).unwrap();
    assert_eq!(stand_in.id(), me);
    let (a, _b) = UnixStream::pair().unwrap();
    admission.verify_reader(&stand_in, &a).unwrap();
    admission.recheck(&stand_in).unwrap();
    // It counts as reaped: killing and dropping it leaves this process alone.
    assert!(stand_in.try_wait().unwrap());
    assert_eq!(stand_in.ended(), None);
    stand_in.kill();
    drop(stand_in);

    // A handle that outlives its process: nothing about it passes any more.
    let mut started = start(&admission, &["hold"]);
    let handle = unowned_child(started.child.id(), None).unwrap();
    admission.recheck(&handle).unwrap();
    assert_eq!(
        child_peer(started.child.id()).unwrap().pid,
        Some(started.child.id())
    );
    started.child.kill();
    assert!(admission.recheck(&handle).is_err());
    assert!(admission.readmit(&handle).is_err());
    // No pidfd for a process that does not exist, and no identity for a process
    // that is not this one's child.
    assert!(unowned_child(started.child.id(), None).is_err());
    assert!(child_peer(started.child.id()).is_err());
    assert!(child_peer(1).is_err());
    // A pidfd of another live process answers that it has not exited.
    let init = sys::pidfd_open(1).unwrap();
    assert!(!sys::exited(init.as_fd()).unwrap());
}
