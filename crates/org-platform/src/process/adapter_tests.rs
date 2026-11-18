//! Tests of the contract that need a real adapter (macOS or GNU/Linux).

use std::ffi::{CStr, CString};
use std::io::{self, Read, Write};
use std::process::ExitStatus;
use std::time::{Duration, Instant};

use super::{
    check_time, identity, native_mark, release_spawned, spawn_piped, try_exit, DeadlineElapsed,
    MarkEvidence, Watch,
};

const MARK: &[u8] = b"ORG_SESSION_MARK=";

/// A real child of the test process, killed and reaped when dropped.
pub(super) struct TestChild {
    pub(super) pid: libc::pid_t,
}

impl TestChild {
    /// Start `program` as [`spawn_piped`] leaves it, with the parent's pipe ends.
    pub(super) fn spawn_piped(
        program: &CStr,
        args: &[&CStr],
    ) -> (Self, std::fs::File, std::fs::File, std::fs::File) {
        let mut argv = vec![CString::from(program)];
        argv.extend(args.iter().map(|arg| CString::from(*arg)));
        let (pid, stdin, stdout, stderr) =
            spawn_piped(program, &argv, &[c"PATH=/usr/bin:/bin".into()], c"/").unwrap();
        (Self { pid }, stdin, stdout, stderr)
    }

    /// The exit status, without reaping, once the child has exited.
    fn exit(&self) -> Option<ExitStatus> {
        loop {
            match try_exit(self.pid) {
                Ok(status) => return status,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => panic!("the test's own child cannot be observed: {error}"),
            }
        }
    }
}

impl Drop for TestChild {
    fn drop(&mut self) {
        // SAFETY: this pid is the test's own unreaped child and cannot have been
        // reused. It is killed and reaped exactly once.
        unsafe {
            libc::kill(self.pid, libc::SIGKILL);
            while libc::waitpid(self.pid, std::ptr::null_mut(), 0) < 0
                && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted
            {}
        }
    }
}

#[test]
fn a_piped_child_leads_its_session_and_is_watched_without_being_reaped() {
    let (child, mut stdin, mut stdout, _stderr) = TestChild::spawn_piped(
        c"/bin/sh",
        &[c"-c", c"read line; echo \"$line from $PWD\"; exit 7"],
    );
    let (group, started, boot) = identity(child.pid).unwrap();
    assert_eq!(group, child.pid as u32, "the child leads its own group");
    assert!(!started.is_empty() && !boot.is_empty());
    let watch = Watch::new(child.pid).unwrap();
    assert!(child.exit().is_none());
    release_spawned(child.pid, || Ok(child.exit().is_some())).unwrap();
    stdin.write_all(b"hello\n").unwrap();

    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(status) = child.exit() {
            break status;
        }
        assert!(Instant::now() < deadline, "the child did not exit");
        watch.wait(deadline).unwrap();
    };
    assert_eq!(status.code(), Some(7));
    // Observation left the zombie in place: the same exit is still there to see.
    assert_eq!(child.exit().and_then(|status| status.code()), Some(7));
    let mut output = String::new();
    stdout.read_to_string(&mut output).unwrap();
    assert_eq!(output, "hello from /\n");
}

#[test]
fn native_mark_identical_duplicates_count_once_on_each_platform() {
    for bytes in [
        b"ORG_SESSION_MARK=a\0ORG_SESSION_MARK=a\0".as_slice(),
        b"ORG_SESSION_MARK=a\0OTHER=value\0ORG_SESSION_MARK=a\0ORG_SESSION_MARK=a\0",
    ] {
        assert_eq!(native_mark(bytes, MARK), MarkEvidence::Present("a".into()));
    }
    assert_eq!(
        native_mark(
            b"ORG_SESSION_MARK=a\0ORG_SESSION_MARK=a\0ORG_SESSION_MARK=b\0",
            MARK
        ),
        MarkEvidence::Unreadable
    );
}

#[test]
fn native_guards_quarantine_conflicting_duplicate_marks() {
    assert_eq!(
        native_mark(b"ORG_SESSION_MARK=a\0ORG_SESSION_MARK=b\0", MARK),
        MarkEvidence::Unreadable
    );
    assert_eq!(
        native_mark(b"ORG_SESSION_MARK=a\0", MARK),
        MarkEvidence::Present("a".into())
    );
}

#[test]
fn a_mark_is_read_whole_and_reported_without_judging_its_value() {
    assert_eq!(native_mark(b"", MARK), MarkEvidence::Absent);
    assert_eq!(native_mark(b"OTHER=a\0", MARK), MarkEvidence::Absent);
    // The consumer decides what a valid mark is.
    assert_eq!(
        native_mark(b"ORG_SESSION_MARK=not a mark!\0", MARK),
        MarkEvidence::Present("not a mark!".into())
    );
    assert_eq!(
        native_mark(b"ORG_SESSION_MARK=\0", MARK),
        MarkEvidence::Present(String::new())
    );
    // A block that does not end at an entry boundary, or a value that is not text.
    assert_eq!(
        native_mark(b"ORG_SESSION_MARK=a", MARK),
        MarkEvidence::Unreadable
    );
    assert_eq!(
        native_mark(b"ORG_SESSION_MARK=\xff\0", MARK),
        MarkEvidence::Unreadable
    );
}

#[test]
fn native_deadline_failure_classification_preserves_io() {
    check_time(Instant::now() + Duration::from_secs(3600)).unwrap();
    let error = check_time(Instant::now() - Duration::from_nanos(1)).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert_eq!(error.to_string(), "native recovery deadline elapsed");
    assert!(error
        .get_ref()
        .is_some_and(|source| source.is::<DeadlineElapsed>()));
}
