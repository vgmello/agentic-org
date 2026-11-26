use super::procargs::{code_signing_allows, CS_RESTRICT};
use super::signal::recheck_after_mark;
use super::{
    proc_list_pids, procargs_mark, wait_after_registration, KINFO_PROC_PID_OFFSET, KINFO_PROC_SIZE,
};
use crate::process::MarkEvidence;
use std::cell::Cell;
use std::time::{Duration, Instant};

const MARK: &[u8] = b"ORG_SESSION_MARK=";

/// What a consumer asks for a target that carries the mark itself: the exact value.
fn exact_mark(mark: &MarkEvidence) -> bool {
    *mark == MarkEvidence::Present("guard-mark".into())
}

fn ordinary_info() -> libc::proc_bsdinfo {
    // SAFETY: proc_bsdinfo is a C record of integer fields and arrays.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    info.pbi_pid = 42;
    info.pbi_start_tvsec = 12;
    info.pbi_start_tvusec = 34;
    info.pbi_uid = 501;
    info.pbi_ruid = 501;
    info.pbi_svuid = 501;
    info.pbi_gid = 20;
    info.pbi_rgid = 20;
    info.pbi_svgid = 20;
    info
}

#[test]
fn native_job_observation_rechecks_membership_identity_and_credentials() {
    use crate::process::{ProcessIdentity, RecoveryContext};
    let info = libc::proc_bsdinfo {
        pbi_pgid: 40,
        ..ordinary_info()
    };
    let identity = ProcessIdentity {
        pid: info.pbi_pid,
        started: super::start(&info),
        boot: "boot".into(),
    };
    let mut native = super::RecoveryScan {
        context: RecoveryContext {
            uid: 501,
            pid: 999,
            boot: "boot".into(),
        },
        arg_max: 4096,
        captured: [(identity.pid, identity.started.clone())].into(),
        mark_prefix: MARK.to_vec(),
    };
    for variant in 0..20 {
        let mut before = info;
        let mut after = info;
        match variant {
            1 => after.pbi_start_tvsec += 1,
            2 => after.pbi_start_tvusec += 1,
            3 => after.pbi_pid += 1,
            4 => before.pbi_status = libc::SZOMB,
            5 => after.pbi_status = libc::SZOMB,
            6 => before.pbi_uid = 0,
            7 => after.pbi_uid = 0,
            8 => before.pbi_ruid = 502,
            9 => after.pbi_ruid = 502,
            10 => before.pbi_svuid = 0,
            11 => after.pbi_svuid = 0,
            12 => before.pbi_gid = 0,
            13 => after.pbi_gid = 0,
            14 => before.pbi_rgid = 0,
            15 => after.pbi_rgid = 0,
            16 => before.pbi_svgid = 0,
            17 => after.pbi_svgid = 0,
            18 => after.pbi_flags = super::PROC_FLAG_PSUGID,
            19 => before.pbi_flags = super::PROC_FLAG_PSUGID,
            _ => {}
        }
        let calls = std::cell::RefCell::new(vec![]);
        let mut count = 0;
        let result = native
            .job_observation_with(
                &identity,
                Instant::now() + Duration::from_secs(1),
                |_| {
                    count += 1;
                    calls.borrow_mut().push("identity");
                    Ok(Some(if count == 1 { before } else { after }))
                },
                |_| {
                    calls.borrow_mut().push("session");
                    41
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(*calls.borrow(), ["identity", "session", "identity"]);
        assert_eq!(result.safe, variant == 0, "variant {variant}");
        assert_eq!(result.group, 40);
        assert_eq!(result.session, 41);
        assert_eq!(
            result.order,
            (after.pbi_start_tvsec, after.pbi_start_tvusec)
        );
    }
    for absent in [1, 2] {
        let mut count = 0;
        assert!(native
            .job_observation_with(
                &identity,
                Instant::now() + Duration::from_secs(1),
                |_| {
                    count += 1;
                    Ok((count != absent).then_some(info))
                },
                |_| 41
            )
            .unwrap()
            .is_none());
    }
    for fail_at in [1, 2] {
        let mut count = 0;
        let error = native
            .job_observation_with(
                &identity,
                Instant::now() + Duration::from_secs(1),
                |_| {
                    count += 1;
                    if count == fail_at {
                        Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
                    } else {
                        Ok(Some(info))
                    }
                },
                |_| 41,
            )
            .err()
            .unwrap();
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    }
    let error = native
        .job_observation_with(&identity, Instant::now(), |_| Ok(Some(info)), |_| 41)
        .err()
        .unwrap();
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    let error = native
        .job_observation_with(
            &identity,
            Instant::now() + Duration::from_secs(1),
            |_| Ok(Some(info)),
            |_| {
                // SAFETY: __error returns this thread's writable errno slot.
                unsafe {
                    *libc::__error() = libc::EPERM;
                }
                -1
            },
        )
        .err()
        .unwrap();
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    native.captured.clear();
    assert!(native
        .job_observation_with(
            &identity,
            Instant::now() + Duration::from_secs(1),
            |_| panic!("no observation without captured identity"),
            |_| panic!("no session without captured identity")
        )
        .unwrap()
        .is_none());
}

#[test]
fn native_guards_recheck_identity_credentials_and_mark_after_reading_it() {
    use crate::process::ProcessIdentity;
    let info = ordinary_info();
    let target = ProcessIdentity {
        pid: 42,
        started: super::start(&info),
        boot: "boot".into(),
    };
    let mark = MarkEvidence::Present("guard-mark".into());
    assert!(recheck_after_mark(&target, &exact_mark, Some(&info), &mark, 501).unwrap());
    assert!(!recheck_after_mark(&target, &exact_mark, None, &mark, 501).unwrap());
    for changed in [
        libc::proc_bsdinfo {
            pbi_pid: 43,
            ..info
        },
        libc::proc_bsdinfo {
            pbi_start_tvsec: 13,
            ..info
        },
        libc::proc_bsdinfo {
            pbi_start_tvusec: 35,
            ..info
        },
        libc::proc_bsdinfo {
            pbi_status: libc::SZOMB,
            ..info
        },
    ] {
        assert!(!recheck_after_mark(&target, &exact_mark, Some(&changed), &mark, 501).unwrap());
    }
    for mark in [
        MarkEvidence::Absent,
        MarkEvidence::Unreadable,
        MarkEvidence::Present("other".into()),
    ] {
        assert!(recheck_after_mark(&target, &exact_mark, Some(&info), &mark, 501).is_err());
    }
    for changed in [
        libc::proc_bsdinfo { pbi_uid: 0, ..info },
        libc::proc_bsdinfo {
            pbi_ruid: 0,
            ..info
        },
        libc::proc_bsdinfo {
            pbi_svuid: 0,
            ..info
        },
        libc::proc_bsdinfo { pbi_gid: 0, ..info },
        libc::proc_bsdinfo {
            pbi_rgid: 0,
            ..info
        },
        libc::proc_bsdinfo {
            pbi_svgid: 0,
            ..info
        },
        libc::proc_bsdinfo {
            pbi_flags: super::PROC_FLAG_PSUGID,
            ..info
        },
    ] {
        assert_eq!(
            recheck_after_mark(&target, &exact_mark, Some(&changed), &mark, 501)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::PermissionDenied
        );
    }
}

#[test]
fn native_guards_reject_restricted_or_unknown_code_signing_status() {
    assert!(code_signing_allows(0, 0));
    assert!(!code_signing_allows(-1, 0));
    assert!(!code_signing_allows(0, CS_RESTRICT));
}

#[test]
fn native_signal_wiring_refuses_changes_during_the_mark_read() {
    use crate::process::{ProcessIdentity, SignalOutcome};
    let before = ordinary_info();
    let target = ProcessIdentity {
        pid: 42,
        started: super::start(&before),
        boot: "boot".into(),
    };
    let exact = MarkEvidence::Present("guard-mark".into());
    let cases = [
        (Some(before), exact.clone(), "signal"),
        (None, exact.clone(), "gone"),
        (
            Some(libc::proc_bsdinfo {
                pbi_start_tvsec: 13,
                ..before
            }),
            exact.clone(),
            "gone",
        ),
        (
            Some(libc::proc_bsdinfo {
                pbi_uid: 0,
                ..before
            }),
            exact.clone(),
            "refuse",
        ),
        (
            Some(libc::proc_bsdinfo {
                pbi_flags: super::PROC_FLAG_PSUGID,
                ..before
            }),
            exact,
            "refuse",
        ),
        (
            Some(before),
            MarkEvidence::Present("changed".into()),
            "refuse",
        ),
        (Some(before), MarkEvidence::Unreadable, "refuse"),
    ];
    for (after, mark, expected) in cases {
        let queries = Cell::new(0);
        let marked = Cell::new(false);
        let sent = Cell::new(false);
        let outcome = super::signal_with(
            &target,
            &exact_mark,
            Instant::now() + Duration::from_secs(1),
            501,
            || {
                queries.set(queries.get() + 1);
                if queries.get() == 1 {
                    assert!(!marked.get());
                    Ok(Some(before))
                } else {
                    assert!(marked.get());
                    Ok(after)
                }
            },
            |_| {
                assert_eq!(queries.get(), 1);
                marked.set(true);
                Ok(mark)
            },
            || {
                assert_eq!(queries.get(), 2);
                sent.set(true);
                Ok(SignalOutcome::Signalled)
            },
        );
        assert_eq!(queries.get(), 2);
        assert!(marked.get());
        match expected {
            "signal" => assert_eq!(outcome.unwrap(), SignalOutcome::Signalled),
            "gone" => assert_eq!(outcome.unwrap(), SignalOutcome::Gone),
            _ => assert_eq!(
                outcome.unwrap_err().kind(),
                std::io::ErrorKind::PermissionDenied
            ),
        }
        assert_eq!(sent.get(), expected == "signal");
    }
}

#[test]
fn denied_libproc_query_preserves_an_unreadable_same_uid_row() {
    let mut rows = Vec::new();
    let observed = super::enumerated_info(54, 501, &mut rows, |pid| {
        assert_eq!(pid, 54);
        Err(std::io::Error::from_raw_os_error(libc::EPERM))
    })
    .unwrap();
    assert!(observed.is_none());
    let [row] = rows.as_slice() else {
        panic!("expected one S0 row");
    };
    assert_eq!((row.pid, row.uid), (54, 501));
    assert_eq!(row.mark, MarkEvidence::Unreadable);
    assert!(row.identity.is_none() && row.parent.is_none());
    assert!(!row.credentials_safe && !row.zombie);

    let absent = super::enumerated_info(56, 501, &mut rows, |_| Ok(None)).unwrap();
    assert!(absent.is_none());
    assert_eq!(rows.len(), 1);
}

#[test]
fn debugger_parent_is_not_process_ancestry() {
    // SAFETY: proc_bsdinfo is a plain C record containing integer fields and arrays.
    let mut child: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    child.pbi_pid = 56;
    child.pbi_ppid = 54;
    child.pbi_start_tvsec = 12;
    let mut traced = child;
    traced.pbi_flags = super::PROC_FLAG_TRACED;
    assert_eq!(super::observed_parent_pid(&child, &child), Some(54));
    assert_eq!(super::observed_parent_pid(&child, &traced), None);
    assert_eq!(super::observed_parent_pid(&traced, &child), None);
}

#[test]
fn process_list_rejects_partial_records_and_missing_supervisor() {
    let mut bytes = vec![0; 2 * KINFO_PROC_SIZE];
    for (index, pid) in [54i32, 56].into_iter().enumerate() {
        let field = index * KINFO_PROC_SIZE + KINFO_PROC_PID_OFFSET;
        bytes[field..field + 4].copy_from_slice(&pid.to_ne_bytes());
    }
    assert_eq!(proc_list_pids(&bytes, 54).unwrap(), [54, 56]);
    assert!(proc_list_pids(&bytes[..bytes.len() - 1], 54).is_err());
    assert!(proc_list_pids(&bytes, 55).is_err());
}

#[test]
fn an_empty_exit_queue_does_not_wait_out_the_deadline() {
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut observations = 0;
    let gone = wait_after_registration(
        deadline,
        Instant::now,
        || {
            observations += 1;
            Ok(observations == 2)
        },
        |until| {
            assert!(
                until < deadline,
                "an empty kqueue consumed the exit deadline"
            );
            Ok(())
        },
    )
    .unwrap();
    assert!(gone);
    assert_eq!(observations, 2);
}

#[test]
fn exit_wait_never_starts_an_observation_after_its_deadline() {
    let start = Instant::now();
    let deadline = start + Duration::from_millis(30);
    let clock = Cell::new(start);
    let observations = Cell::new(0);
    let gone = wait_after_registration(
        deadline,
        || clock.get(),
        || {
            observations.set(observations.get() + 1);
            Ok(clock.get() >= deadline)
        },
        |_| {
            clock.set(deadline);
            Ok(())
        },
    )
    .unwrap();
    assert!(!gone, "an observation after the deadline was accepted");
    assert_eq!(observations.get(), 1);
}

fn capture(argc: i32, entries: &[u8]) -> Vec<u8> {
    let mut bytes = argc.to_ne_bytes().to_vec();
    let path = b"/tmp/tool\0";
    bytes.extend_from_slice(path);
    bytes.resize(4 + path.len().next_multiple_of(8), 0);
    bytes.extend_from_slice(entries);
    bytes.resize(4 + (bytes.len() - 4).next_multiple_of(8), 0);
    bytes
}

#[test]
fn procargs_mark_ignores_argv_and_counts_later_empty_arguments() {
    let bytes = capture(
        3,
        b"tool\0\0ORG_SESSION_MARK=argument\0ORG_SESSION_MARK=environment\0",
    );
    assert_eq!(
        procargs_mark(&bytes, bytes.len(), MARK),
        MarkEvidence::Present("environment".into())
    );
    let unrelated = capture(1, b"tool\0PREFIX_ORG_SESSION_MARK=prefix\0");
    assert_eq!(
        procargs_mark(&unrelated, unrelated.len(), MARK),
        MarkEvidence::Absent
    );
}

#[test]
fn procargs_mark_rejects_an_aligned_prefix_shorter_than_the_kernel_size() {
    let prefix = capture(1, b"tool\0ORG_SESSION_MARK=mark\0");
    let full = capture(
        1,
        b"tool\0ORG_SESSION_MARK=mark\0ORG_SESSION_MARK=conflict\0",
    );
    assert_eq!(
        procargs_mark(&prefix, full.len(), MARK),
        MarkEvidence::Unreadable
    );
}

#[test]
fn procargs_mark_quarantines_incomplete_or_conflicting_captures() {
    let empty_argv_zero = capture(1, b"\0ORG_SESSION_MARK=mark\0");
    let invalid_argc = capture(100, b"tool\0ORG_SESSION_MARK=mark\0");
    let conflicting = capture(1, b"tool\0ORG_SESSION_MARK=a\0ORG_SESSION_MARK=b\0");
    let valid = capture(1, b"tool\0ORG_SESSION_MARK=mark\0");
    for bad in [
        &empty_argv_zero,
        &invalid_argc,
        &conflicting,
        &valid[..valid.len() - 1],
    ] {
        assert_eq!(
            procargs_mark(bad, bad.len(), MARK),
            MarkEvidence::Unreadable
        );
    }
}
