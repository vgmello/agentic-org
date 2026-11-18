//! The confinement example process, run as a test: `examples/gateway_confinement.rs`
//! proves on the running kernel what a confined web process can and cannot do, and
//! ADR-0017's LNX-3 with it: where the kernel feature is refused, the process
//! refuses to start and reports the reason.
//!
//! `cargo test` builds this package's examples before it runs its tests.

use org_platform::confinement::{
    self,
    testing::{self, RefusedCall},
};
use std::{
    fs,
    io::{self, Read},
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

#[test]
fn the_example_passes_where_the_web_process_can_be_confined() {
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
            assert!(last.starts_with("PASS: "), "{stdout}\n{stderr}");
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
        .args([
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

#[test]
fn lnx_3_a_profile_that_refuses_landlock_makes_the_process_refuse_with_the_reason() {
    assert_refused(
        refused_under(RefusedCall::LandlockCreateRuleset, "create"),
        "landlock_create_ruleset failed with EPERM: the Landlock version query was refused",
    );
    assert_refused(
        refused_under(RefusedCall::LandlockRestrictSelf, "restrict"),
        "landlock_restrict_self failed with EPERM (Operation not permitted (os error 1))",
    );
}

#[test]
fn lnx_3_a_profile_that_refuses_seccomp_makes_the_process_refuse_with_the_reason() {
    assert_refused(
        refused_under(RefusedCall::Seccomp, "seccomp"),
        "seccomp failed with EPERM (Operation not permitted (os error 1))",
    );
}
