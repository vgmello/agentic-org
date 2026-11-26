use super::pidfd::poll_until;
use super::procfs::{boot_bind_ok, validate_mountinfo};
use super::records::{
    auxv_ordinary, environment_unchanged, ordinary_exec_with, parse_stat, read_mark_with,
    Credentials,
};
use super::*;
use crate::process::adapter_tests::TestChild;
use std::cell::Cell;
use std::fs::File;
use std::time::{Duration, Instant};

const MARK: &[u8] = b"ORG_SESSION_MARK=";

/// What a consumer asks for a target that carries the mark itself: the exact value.
fn exact_mark(mark: &MarkEvidence) -> bool {
    *mark == MarkEvidence::Present("guard-mark".into())
}

#[test]
fn native_guards_require_the_exact_pid_namespace() {
    assert!(pidfd_namespace_matches("Pid:\t42\nNSpid:\t42\n", 42).unwrap());
    for info in [
        "Pid:\t43\nNSpid:\t42\n",
        "Pid:\t42\nNSpid:\t43\n",
        "Pid:\t42\nNSpid:\t42 1\n",
        "Pid:\t42\nNSpid:\t\n",
        "Pid:\t42\nNSpid:\tbad\n",
        "Pid:\t42\n",
    ] {
        assert!(
            !pidfd_namespace_matches(info, 42).unwrap_or(false),
            "{info}"
        );
    }
}

#[test]
fn native_guards_require_the_boot_bind_root_and_device() {
    assert!(boot_bind_ok(b"/sys", 21, 21));
    assert!(!boot_bind_ok(b"/", 21, 21));
    assert!(!boot_bind_ok(b"/sys", 22, 21));
}

#[test]
fn descriptor_exhaustion_explains_the_recovery_limit() {
    for code in [libc::EMFILE, libc::ENFILE] {
        let error = descriptor_error(io::Error::from_raw_os_error(code));
        assert!(error.to_string().contains("doctor:"));
        assert!(error.to_string().contains("RLIMIT_NOFILE"));
    }
    let error = descriptor_error(io::Error::from_raw_os_error(libc::EPERM));
    assert_eq!(error.raw_os_error(), Some(libc::EPERM));
}

#[test]
fn native_evidence_wiring_preserves_resource_failures() {
    let mark = b"ORG_SESSION_MARK=guard-mark\0";
    let stat = Stat {
        environment: (4096, 4096 + mark.len() as u64),
        ..ordinary_stat()
    };
    assert!(ordinary_exec_with(|| Ok(auxv(&[(libc::AT_SECURE as usize, 0), (0, 0)]))).unwrap());
    assert_eq!(
        read_mark_with(stat, MARK, |_| Ok(mark.to_vec()), || Ok(stat)).unwrap(),
        MarkEvidence::Present("guard-mark".into())
    );
    for code in [libc::EMFILE, libc::ENFILE] {
        let fault = || descriptor_error(io::Error::from_raw_os_error(code));
        for error in [
            ordinary_exec_with(|| Err(fault())).unwrap_err(),
            read_mark_with(
                stat,
                MARK,
                |_| Err(fault()),
                || panic!("no stat read after environment failure"),
            )
            .unwrap_err(),
            read_mark_with(stat, MARK, |_| Ok(mark.to_vec()), || Err(fault())).unwrap_err(),
        ] {
            assert!(error.to_string().contains("doctor:"), "{code}: {error}");
            assert!(
                error.to_string().contains("RLIMIT_NOFILE"),
                "{code}: {error}"
            );
        }
    }
    assert!(!ordinary_exec_with(|| Err(io::Error::from_raw_os_error(libc::EPERM))).unwrap());
    assert_eq!(
        read_mark_with(
            stat,
            MARK,
            |_| Err(io::Error::from_raw_os_error(libc::EPERM)),
            || panic!("no stat read")
        )
        .unwrap(),
        MarkEvidence::Unreadable
    );
    assert_eq!(
        ordinary_exec_with(|| Err(io::Error::from(io::ErrorKind::TimedOut)))
            .unwrap_err()
            .kind(),
        io::ErrorKind::TimedOut
    );
}

#[test]
fn native_guards_reject_privileged_credentials() {
    let ordinary = || Credentials {
        uid: [501; 4],
        gid: [20; 4],
        capabilities: 0,
    };
    assert!(ordinary().safe(501));
    assert!(!ordinary().safe(0));
    assert!(!Credentials {
        uid: [0; 4],
        gid: [20; 4],
        capabilities: 0
    }
    .safe(0));
    for index in 0..4 {
        let mut changed = ordinary();
        changed.uid[index] = 0;
        assert!(!changed.safe(501), "uid index {index}");
        let mut changed = ordinary();
        changed.gid[index] = 0;
        assert!(!changed.safe(501), "gid index {index}");
    }
    let mut changed = ordinary();
    changed.capabilities = 1;
    assert!(!changed.safe(501));
}

fn ordinary_stat() -> Stat {
    parse_stat(&stat_row(b"42 (tool) S 13 41 44"), 42).unwrap()
}

fn target() -> ProcessIdentity {
    ProcessIdentity {
        pid: 42,
        started: "789".into(),
        boot: "boot".into(),
    }
}

#[test]
fn native_guards_quarantine_unsafe_or_zombie_samples() {
    let stat = ordinary_stat();
    assert!(sample_mark_allowed(stat, true, 42, 54));
    assert!(!sample_mark_allowed(stat, false, 42, 54));
    assert!(!sample_mark_allowed(
        Stat {
            zombie: true,
            ..stat
        },
        true,
        42,
        54
    ));
    assert!(!sample_mark_allowed(stat, true, 42, 42));
}

#[test]
fn native_guards_refuse_reused_or_zombie_signal_targets() {
    let stat = ordinary_stat();
    assert!(verify_signal_stat(stat, &target()).is_ok());
    assert!(verify_signal_stat(
        Stat {
            started: 790,
            ..stat
        },
        &target()
    )
    .is_err());
    assert_eq!(
        verify_signal_stat(
            Stat {
                zombie: true,
                ..stat
            },
            &target()
        )
        .unwrap_err()
        .kind(),
        io::ErrorKind::PermissionDenied
    );
}

#[test]
fn native_guards_require_an_unchanged_environment_capture() {
    let before = ordinary_stat();
    assert!(environment_unchanged(before, before));
    for after in [
        Stat {
            started: 790,
            ..before
        },
        Stat {
            zombie: true,
            ..before
        },
        Stat {
            environment: (4095, 4128),
            ..before
        },
        Stat {
            environment: (4096, 4129),
            ..before
        },
    ] {
        assert!(!environment_unchanged(before, after), "{after:?}");
    }
}

fn auxv(entries: &[(usize, usize)]) -> Vec<u8> {
    entries
        .iter()
        .flat_map(|(kind, value)| [kind.to_ne_bytes(), value.to_ne_bytes()].concat())
        .collect()
}

#[test]
fn native_guards_require_one_nonsecure_auxv_entry_and_a_complete_terminator() {
    let secure = libc::AT_SECURE as usize;
    let valid = auxv(&[(secure, 0), (0, 0)]);
    assert!(auxv_ordinary(&valid));
    for bytes in [
        auxv(&[(secure, 1), (0, 0)]),
        auxv(&[(0, 0)]),
        auxv(&[(secure, 0), (secure, 0), (0, 0)]),
        auxv(&[(secure, 0)]),
        auxv(&[(secure, 0), (0, 1)]),
        auxv(&[(secure, 0), (0, 0), (0, 0)]),
        valid[..valid.len() - 1].to_vec(),
    ] {
        assert!(!auxv_ordinary(&bytes), "{bytes:?}");
    }
}

struct ProcessFixture {
    root: tempfile::TempDir,
    child: TestChild,
}

impl ProcessFixture {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().unwrap(),
            // Linux starts the child running. Its pipes close here, as the
            // recovery fixtures never use them.
            child: TestChild::spawn_piped(c"/bin/sleep", &[c"20"]).0,
        }
    }

    fn pid(&self) -> u32 {
        self.child.pid as u32
    }

    fn directory(&self) -> std::path::PathBuf {
        self.root.path().join(self.pid().to_string())
    }

    fn reset(&self) {
        let directory = self.directory();
        std::fs::create_dir_all(&directory).unwrap();
        self.stat(b'S', 789);
        self.status(0);
        let environment = b"ORG_SESSION_MARK=guard-mark\0";
        std::fs::write(directory.join("environ"), environment).unwrap();
        std::fs::write(
            directory.join("auxv"),
            auxv(&[(libc::AT_SECURE as usize, 0), (0, 0)]),
        )
        .unwrap();
    }

    fn stat(&self, state: u8, started: u64) {
        let prefix = format!("{} (tool) {} 1 41 44", self.pid(), char::from(state));
        let bytes = String::from_utf8(stat_row(prefix.as_bytes()))
            .unwrap()
            .replace(" 789 ", &format!(" {started} "))
            .replace("4096 4128", "4096 4124");
        std::fs::write(self.directory().join("stat"), bytes).unwrap();
    }

    fn status(&self, caps: u64) {
        // SAFETY: getuid has no pointer arguments or side effects.
        let uid = unsafe { libc::getuid() };
        std::fs::write(
            self.directory().join("status"),
            format!(
                "Uid:\t{uid} {uid} {uid} {uid}\nGid:\t20 20 20 20\nCapPrm:\t{caps:x}\nCapEff:\t0\n"
            ),
        )
        .unwrap();
    }

    fn native(&self) -> RecoveryScan {
        // SAFETY: getuid has no pointer arguments or side effects.
        let uid = unsafe { libc::getuid() };
        RecoveryScan {
            procfs: File::open(self.root.path()).unwrap(),
            procfs_mount_id: 0,
            mark_prefix: MARK.to_vec(),
            context: RecoveryContext {
                uid,
                pid: std::process::id(),
                boot: "boot".into(),
            },
            pins: BTreeMap::new(),
        }
    }
}

#[test]
fn native_sample_wiring_quarantines_unsafe_or_zombie_processes() {
    let fixture = ProcessFixture::new();
    fixture.reset();
    let native = fixture.native();
    let deadline = Instant::now() + Duration::from_secs(2);
    let row = native.sample(fixture.pid(), deadline).unwrap().unwrap().0;
    assert_eq!(row.mark, MarkEvidence::Present("guard-mark".into()));
    fixture.status(1);
    let row = native.sample(fixture.pid(), deadline).unwrap().unwrap().0;
    assert!(!row.credentials_safe);
    assert_eq!(row.mark, MarkEvidence::Unreadable);
    fixture.reset();
    fixture.stat(b'Z', 789);
    let row = native.sample(fixture.pid(), deadline).unwrap().unwrap().0;
    assert_eq!(row.mark, MarkEvidence::Unreadable);
    fixture.reset();
    std::fs::write(
        fixture.directory().join("auxv"),
        auxv(&[(libc::AT_SECURE as usize, 1), (0, 0)]),
    )
    .unwrap();
    let row = native.sample(fixture.pid(), deadline).unwrap().unwrap().0;
    assert!(!row.credentials_safe);
    assert_eq!(row.mark, MarkEvidence::Unreadable);
}

#[test]
fn native_signal_wiring_refuses_changes_after_the_snapshot() {
    let fixture = ProcessFixture::new();
    let mut target = target();
    target.pid = fixture.pid();
    for mutation in ["mark", "capabilities", "secure-exec", "zombie"] {
        fixture.reset();
        let mut native = fixture.native();
        let deadline = Instant::now() + Duration::from_secs(2);
        let (_, pin) = native.sample(fixture.pid(), deadline).unwrap().unwrap();
        native.pins.insert(fixture.pid(), pin);
        match mutation {
            "mark" => {
                std::fs::write(
                    fixture.directory().join("environ"),
                    b"ORG_SESSION_MARK=wrong-mark\0",
                )
                .unwrap();
            }
            "capabilities" => fixture.status(1),
            "secure-exec" => {
                std::fs::write(
                    fixture.directory().join("auxv"),
                    auxv(&[(libc::AT_SECURE as usize, 1), (0, 0)]),
                )
                .unwrap();
            }
            _ => fixture.stat(b'Z', 789),
        }
        let error = native
            .signal_checked(&target, &exact_mark, RecoverySignal::Terminate, deadline)
            .unwrap_err();
        assert_eq!(
            error.kind(),
            io::ErrorKind::PermissionDenied,
            "{mutation}: {error}"
        );
        if mutation == "zombie" {
            assert!(error.to_string().contains("exited thread-group leader"));
        }
        assert!(!native.wait_exited(&target, Instant::now()).unwrap());
    }
}

#[test]
fn native_mark_wiring_rechecks_the_environment_identity() {
    let fixture = ProcessFixture::new();
    fixture.reset();
    let directory = File::open(fixture.directory()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let before = read_stat(&directory, fixture.pid(), Some(deadline)).unwrap();
    assert_eq!(
        read_mark(&directory, fixture.pid(), before, MARK, deadline).unwrap(),
        MarkEvidence::Present("guard-mark".into())
    );
    fixture.stat(b'S', 790);
    assert_eq!(
        read_mark(&directory, fixture.pid(), before, MARK, deadline).unwrap(),
        MarkEvidence::Unreadable
    );
}

#[test]
fn reopened_proc_directory_must_still_match_the_live_pidfd() {
    let fixture = ProcessFixture::new();
    fixture.reset();
    let native = fixture.native();
    let deadline = Instant::now() + Duration::from_secs(2);
    let (pin, _) = native.pin(fixture.pid(), Some(deadline)).unwrap().unwrap();
    assert!(native
        .reopen(fixture.pid(), &pin, deadline)
        .unwrap()
        .is_some());
    fixture.stat(b'S', 790);
    let error = native.reopen(fixture.pid(), &pin, deadline).err().unwrap();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    // SAFETY: the fixture still owns this unreaped child and its individual PID.
    let rc = unsafe { libc::kill(fixture.pid() as libc::pid_t, libc::SIGKILL) };
    assert_eq!(rc, 0);
    assert!(exited(&pin.fd, Some(deadline)).unwrap());
    assert!(native
        .reopen(fixture.pid(), &pin, deadline)
        .unwrap()
        .is_none());
}

#[test]
fn native_job_observation_requires_current_pin_credentials_owner_and_safe_exec() {
    let fixture = ProcessFixture::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    for variant in 0..9 {
        fixture.reset();
        let mut native = fixture.native();
        let (pin, _directory) = native.pin(fixture.pid(), Some(deadline)).unwrap().unwrap();
        let identity = native.identity(fixture.pid(), pin.stat);
        if variant != 1 {
            native.pins.insert(identity.pid, pin);
        }
        match variant {
            2 => {
                native.pins.get_mut(&identity.pid).unwrap().stat.started += 1;
            }
            3 => fixture.stat(b'Z', 789),
            4 => fixture.status(1),
            5 => {
                std::fs::write(
                    fixture.directory().join("auxv"),
                    auxv(&[(libc::AT_SECURE as usize, 1), (0, 0)]),
                )
                .unwrap();
            }
            6 => native.context.uid += 1,
            7 => fixture.stat(b'S', 790),
            8 => {
                std::fs::remove_file(fixture.directory().join("status")).unwrap();
            }
            _ => {}
        }
        let result = native.job_observation(&identity, deadline);
        match variant {
            0 => {
                let value = result.unwrap().unwrap();
                assert!(value.safe);
                assert_eq!(value.identity, identity);
                assert_eq!(value.order, (789, 0));
                assert_eq!((value.group, value.session), (41, 44));
            }
            1 | 2 => assert!(result.unwrap().is_none(), "variant {variant}"),
            3..=6 => assert!(!result.unwrap().unwrap().safe, "variant {variant}"),
            7 => assert_eq!(result.err().unwrap().kind(), io::ErrorKind::InvalidData),
            8 => assert!(result.is_err()),
            _ => unreachable!(),
        }
    }
    fixture.reset();
    let mut native = fixture.native();
    let (pin, _) = native.pin(fixture.pid(), Some(deadline)).unwrap().unwrap();
    let identity = native.identity(fixture.pid(), pin.stat);
    native.pins.insert(identity.pid, pin);
    assert!(
        !native
            .job_observation_with(&identity, deadline, |_| Ok(native.context.uid + 1))
            .unwrap()
            .unwrap()
            .safe
    );
    assert!(native
        .job_observation_with(&identity, deadline, |_| Err(io::Error::from(
            io::ErrorKind::PermissionDenied
        )))
        .is_err());
    assert!(native.job_observation(&identity, Instant::now()).is_err());
}

#[test]
fn proc_status_accepts_the_kernel_group_limit() {
    let root = tempfile::tempdir().unwrap();
    let groups = "4294967294 ".repeat(65_536);
    let status = format!(
        "Uid:\t501 501 501 501\nGid:\t20 20 20 20\nGroups:\t{groups}\nCapPrm:\t0\nCapEff:\t0\n"
    );
    std::fs::write(root.path().join("status"), status).unwrap();
    let directory = File::open(root.path()).unwrap();
    let identity = credentials(&directory, Instant::now() + Duration::from_secs(5)).unwrap();
    assert!(identity.safe(501));
}

#[test]
fn process_record_open_rejects_crossing_a_mount() {
    let root = File::open("/").unwrap();
    let err = open_at(&root, c"proc", libc::O_DIRECTORY, libc::RESOLVE_NO_XDEV).unwrap_err();
    assert_eq!(err.raw_os_error(), Some(libc::EXDEV));
}

#[test]
fn pidfd_readiness_after_the_deadline_is_not_completion() {
    let start = Instant::now();
    let deadline = start + Duration::from_millis(30);
    let clock = Cell::new(start);
    let gone = poll_until(
        Some(deadline),
        || clock.get(),
        |_| {
            clock.set(deadline);
            Ok(true)
        },
    )
    .unwrap();
    assert!(!gone);
    assert!(poll_until(
        None,
        || clock.get(),
        |remaining| {
            assert!(remaining.is_zero());
            Ok(true)
        }
    )
    .unwrap());
}

#[test]
fn checks_the_held_procfs_mount_for_filtering() {
    let unfiltered = b"42 20 0:21 / /proc rw - proc proc rw\n";
    assert!(validate_mountinfo(unfiltered, 42).is_ok());
    assert!(validate_mountinfo(unfiltered, 44).is_err());
    let another_mount = b"41 20 0:22 / /proc-alt rw - proc proc rw,hidepid=off\n";
    let filtered = b"42 20 0:21 / /proc rw master:2 - proc proc rw,hidepid=ptraceable\n";
    let mountinfo = [another_mount.as_slice(), filtered.as_slice()].concat();
    assert!(validate_mountinfo(&mountinfo, 41).is_ok());
    assert!(validate_mountinfo(&mountinfo, 42).is_err());
    let bind = b"42 20 0:21 /2 /proc rw - proc proc rw\n";
    assert!(validate_mountinfo(bind, 42).is_err());
}

fn stat_row(prefix: &[u8]) -> Vec<u8> {
    let mut bytes = prefix.to_vec();
    bytes.extend_from_slice(b" 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 789");
    for _ in 23..50 {
        bytes.extend_from_slice(b" 0");
    }
    bytes.extend_from_slice(b" 4096 4128 0\n");
    bytes
}

#[test]
fn stat_accepts_missing_group_and_session_hints_during_reaping() {
    let stat = parse_stat(&stat_row(b"42 (exiting) Z 0 -1 -1"), 42).unwrap();
    assert_eq!((stat.group, stat.session), (0, 0));
    assert!(stat.zombie);
}

#[test]
fn stat_uses_the_last_parenthesis_and_only_decodes_numeric_fields() {
    let bytes = stat_row(b"42 (\xff) a process) S 13 41 44");
    assert_eq!(
        parse_stat(&bytes, 42).unwrap(),
        Stat {
            parent: 13,
            group: 41,
            session: 44,
            started: 789,
            environment: (4096, 4128),
            zombie: false,
        }
    );
    assert!(parse_stat(&bytes, 43).is_err());
}
