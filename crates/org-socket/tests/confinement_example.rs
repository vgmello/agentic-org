//! The confinement example process, run as a test: `examples/gateway_confinement.rs`
//! proves on the running kernel what a confined web process can and cannot do, and
//! ADR-0017's LNX-3 with it: where the layer that must hold (on GNU/Linux the
//! seccomp filter) is refused, the process refuses to start and reports the reason.
//! A layer the adapter applies only where the system offers it (Landlock) is not
//! such a reason: under a profile that refuses it, the whole example passes with
//! the filter alone and says so.
//!
//! Every test here holds one more descriptor open, and inheritable, while it runs the
//! example: a process that starts a confining process must give it exactly descriptors 0
//! to 2, whatever it holds itself, and a CI runner leaves descriptors of its own open in
//! the processes it starts. The example closes them when it starts each child; without
//! that, every adapter's entry check refuses the child, as it must.
//!
//! `cargo test` builds this package's examples before it runs its tests.

use org_platform::confinement::{
    self,
    testing::{self, RefusedCall},
};
use std::{
    fs,
    io::{self, Read},
    os::fd::AsRawFd,
    os::unix::{fs::DirBuilderExt, net::UnixListener},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const WAIT: Duration = Duration::from_secs(10);

/// The example beside this test's own executable:
/// `target/<profile>/examples/gateway_confinement`.
fn example() -> PathBuf {
    let test = std::env::current_exe().unwrap();
    let path = test
        .parent()
        .and_then(Path::parent)
        .expect("a test executable in target/<profile>/deps")
        .join("examples/gateway_confinement");
    assert!(
        path.is_file(),
        "{} is missing: build this package's examples (`cargo test -p org-socket` does)",
        path.display()
    );
    path
}

/// A descriptor of this test process that every process it starts inherits: open, and not
/// close-on-exec, for as long as this is held.
struct Inherited(#[allow(dead_code)] fs::File);

fn inherited() -> Inherited {
    let file = fs::File::open("/").unwrap();
    assert!(file.as_raw_fd() > 2);
    // SAFETY: clears close-on-exec on a descriptor this test owns.
    assert_eq!(
        unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, 0) },
        0
    );
    Inherited(file)
}

#[test]
fn the_example_passes_where_the_web_process_can_be_confined() {
    let _held = inherited();
    let output = Command::new(example())
        .env_remove("RUST_BACKTRACE")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    match confinement::web_confinement_supported() {
        Ok(()) => {
            assert!(
                output.status.success(),
                "{}\n{stdout}\n{stderr}",
                output.status
            );
            let last = stdout.lines().last().unwrap_or_default();
            // The line names the layers that were enforced.
            assert!(last.starts_with("PASS: confined by "), "{stdout}\n{stderr}");
        }
        // No adapter, no run: the vector never reports a pass without enforcement.
        Err(refused) => {
            assert!(!output.status.success());
            assert!(!stdout.contains("PASS"), "{stdout}");
            assert!(
                stderr.contains("needs a web confinement adapter"),
                "{stderr}"
            );
            assert!(stderr.contains(&refused.to_string()), "{stderr}");
        }
    }
}

struct Root(PathBuf);
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Starts the example's confining child under a profile that refuses `call`, and
/// returns what it printed, how it ended, and whether its connection to the
/// rendezvous closed without a byte. `None` where this system's adapter does not
/// confine with such calls: there is no profile to start it under.
fn refused_under(call: RefusedCall, name: &str) -> Option<(Output, bool)> {
    let _held = inherited();
    let root = Root(fs::canonicalize("/tmp").unwrap().join(format!(
        "agx-{}-{name}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )));
    fs::DirBuilder::new().mode(0o700).create(&root.0).unwrap();
    fs::write(root.0.join("fixture"), b"retained").unwrap();
    let rendezvous = root.0.join("r");
    let listener = UnixListener::bind(&rendezvous).unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut command = Command::new(example());
    command
        // `--start`: the example starts the role with exactly its three standard
        // descriptors, as it starts every child, and passes on its output and status.
        .args([
            "--start",
            "--child",
            root.0.to_str().unwrap(),
            rendezvous.to_str().unwrap(),
            "127.0.0.1:9",
        ])
        .env_remove("RUST_BACKTRACE")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Err(error) = testing::refuse_call(&mut command, call) {
        assert_eq!(error.kind(), io::ErrorKind::Unsupported, "{error}");
        return None;
    }
    let mut child = command.spawn().unwrap();
    let until = Instant::now() + WAIT;
    let dropped = loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                stream.set_read_timeout(Some(WAIT)).unwrap();
                let mut bytes = Vec::new();
                break stream.read_to_end(&mut bytes).is_ok() && bytes.is_empty();
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if child.try_wait().unwrap().is_some() {
                    break false;
                }
                assert!(Instant::now() < until, "the example did not connect");
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("accept: {error}"),
        }
    };
    let output = child.wait_with_output().unwrap();
    assert_eq!(fs::read(root.0.join("fixture")).unwrap(), b"retained");
    Some((output, dropped))
}

/// The process refused to start: it printed the reason and nothing else, exited
/// with the example's refusal status, and never used its stream.
fn assert_refused(outcome: Option<(Output, bool)>, reason: &str) {
    let Some((output, dropped)) = outcome else {
        return;
    };
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("refused: gateway confinement unavailable: {reason}\n"),
        "{stderr}"
    );
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(dropped, "the connected stream was dropped unused");
}

/// The whole example, parent and children, under a profile that refuses `call`.
/// `None` where this system has no such profile.
fn example_under(call: RefusedCall) -> Option<Output> {
    let _held = inherited();
    let mut command = Command::new(example());
    command.env_remove("RUST_BACKTRACE").stdin(Stdio::null());
    if let Err(error) = testing::refuse_call(&mut command, call) {
        assert_eq!(error.kind(), io::ErrorKind::Unsupported, "{error}");
        return None;
    }
    Some(command.output().unwrap())
}

/// Landlock refused at its version query, at the creation of the ruleset and at the
/// restriction: each time every part of the example holds with the filter alone
/// (the forbidden operations kill, the approved descriptors work, the parent's
/// observation accepts the child), and the PASS line says why Landlock is absent.
#[test]
fn under_a_profile_that_refuses_landlock_the_example_passes_with_the_filter_alone() {
    let not_permitted = "EPERM (Operation not permitted (os error 1))";
    for (call, reason) in [
        (
            RefusedCall::LandlockCreateRuleset,
            "landlock_create_ruleset failed with EPERM: the Landlock version query was refused"
                .to_owned(),
        ),
        (
            RefusedCall::LandlockRuleset,
            format!("landlock_create_ruleset failed with {not_permitted}"),
        ),
        (
            RefusedCall::LandlockRestrictSelf,
            format!("landlock_restrict_self failed with {not_permitted}"),
        ),
    ] {
        let Some(output) = example_under(call) else {
            return;
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "{call:?}: {}\n{stdout}\n{stderr}",
            output.status
        );
        assert!(
            stdout.contains("forbidden operations each killed their confined process with SIGSYS"),
            "{call:?}: {stdout}"
        );
        let last = stdout.lines().last().unwrap_or_default();
        // A kernel without Landlock gives its own reason, whatever the profile.
        if last.contains("failed with ENOSYS") || last.contains("failed with EOPNOTSUPP") {
            assert!(
                last.starts_with("PASS: confined by the seccomp filter alone (Landlock was not "),
                "{call:?}: {stdout}"
            );
            continue;
        }
        assert!(
            last.starts_with(&format!(
                "PASS: confined by the seccomp filter alone (Landlock was not applied: \
                 {reason}); "
            )),
            "{call:?}: {stdout}\n{stderr}"
        );
    }
}

/// LNX-3 for the whole example: under a profile that refuses `seccomp` the parent
/// is told before it starts a child, and nothing passes.
#[test]
fn lnx_3_under_a_profile_that_refuses_seccomp_the_example_starts_nothing() {
    let Some(output) = example_under(RefusedCall::Seccomp) else {
        return;
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(!stdout.contains("PASS"), "{stdout}");
    assert!(
        stderr.contains(
            "needs a web confinement adapter: gateway confinement unavailable: seccomp failed \
             with EPERM: this kernel or its profile has no seccomp filter that kills the process"
        ),
        "{stderr}"
    );
}

/// LNX-3 for the process itself: one that cannot install its filter refuses to
/// start and says why, whether the kernel refuses `seccomp` outright or only the
/// installation fails.
#[test]
fn lnx_3_a_process_that_cannot_install_its_filter_refuses_with_the_reason() {
    assert_refused(
        refused_under(RefusedCall::Seccomp, "seccomp"),
        "seccomp failed with EPERM (Operation not permitted (os error 1))",
    );
    assert_refused(
        refused_under(RefusedCall::SeccompInstall, "install"),
        "seccomp failed with ENOMEM (Cannot allocate memory (os error 12))",
    );
}
