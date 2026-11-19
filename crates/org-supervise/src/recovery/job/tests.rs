use super::*;
use std::cell::{Cell, RefCell};
use std::time::Duration;

fn fixture() -> (
    ProcessIdentity,
    ChildIdentity,
    RecoveryContext,
    JobObservation,
    JobObservation,
) {
    let target = ProcessIdentity {
        pid: 200,
        started: "target-start".into(),
        boot: "boot".into(),
    };
    let leader = ChildIdentity {
        pid: 100,
        pgid: 100,
        started: "leader-start".into(),
        boot: "boot".into(),
        mark: "job-mark".into(),
    };
    let context = RecoveryContext {
        pid: 999,
        uid: 501,
        boot: "boot".into(),
    };
    let child = JobObservation {
        identity: target.clone(),
        order: (20, 0),
        group: 100,
        session: 100,
        safe: true,
    };
    let anchor = JobObservation {
        identity: ProcessIdentity {
            pid: 100,
            started: leader.started.clone(),
            boot: leader.boot.clone(),
        },
        order: (10, 0),
        group: 100,
        session: 100,
        safe: true,
    };
    (target, leader, context, child, anchor)
}

#[test]
fn job_proof_rechecks_exact_live_leader_after_target_membership() {
    for case in [
        "match",
        "session-only",
        "group-only",
        "equal-age",
        "older",
        "member-mismatch",
        "target-reused",
        "target-unsafe",
        "leader-reused",
        "leader-unsafe",
        "leader-not-group-leader",
    ] {
        let (target, leader, context, mut child, mut anchor) = fixture();
        let expected = matches!(case, "match" | "session-only" | "group-only" | "equal-age");
        match case {
            "session-only" => child.group = 201,
            "group-only" => child.session = 0,
            "equal-age" => child.order = anchor.order,
            "older" => child.order = (9, 999_999),
            "member-mismatch" => {
                child.group = 201;
                child.session = 201;
            }
            "target-reused" => child.identity.started = "replacement".into(),
            "target-unsafe" => child.safe = false,
            "leader-reused" => anchor.identity.started = "replacement".into(),
            "leader-unsafe" => anchor.safe = false,
            "leader-not-group-leader" => anchor.group = 99,
            _ => {}
        }
        let order = RefCell::new(vec![]);
        let mut child = Some(child);
        let mut anchor = Some(anchor);
        let matched = verify(
            &target,
            &leader,
            &context,
            Instant::now() + Duration::from_secs(1),
            Instant::now,
            |identity| {
                order.borrow_mut().push(identity.pid);
                if identity.pid == target.pid {
                    Ok(child.take())
                } else {
                    Ok(anchor.take())
                }
            },
        )
        .unwrap();
        assert_eq!(matched, expected, "{case}");
        assert_eq!(order.borrow()[0], target.pid);
        if expected || case.starts_with("leader-") || case == "older" {
            assert_eq!(*order.borrow(), [target.pid, leader.pid], "{case}");
        } else {
            assert_eq!(*order.borrow(), [target.pid], "{case}");
        }
    }
}

#[test]
fn job_proof_refuses_wrong_boot_self_root_or_uncreated_group_before_reads() {
    for case in [
        "target-boot",
        "leader-boot",
        "self-target",
        "self-leader",
        "root",
        "uncreated-group",
        "target-pid",
        "leader-pid",
        "target-start",
        "leader-start",
    ] {
        let (mut target, mut leader, mut context, _, _) = fixture();
        match case {
            "target-boot" => target.boot = "other-boot".into(),
            "leader-boot" => leader.boot = "other-boot".into(),
            "self-target" => context.pid = target.pid,
            "self-leader" => context.pid = leader.pid,
            "root" => context.uid = 0,
            "uncreated-group" => leader.pgid = 99,
            "target-pid" => target.pid = 1,
            "leader-pid" => {
                leader.pid = 1;
                leader.pgid = 1;
            }
            "target-start" => target.started.clear(),
            _ => leader.started.clear(),
        }
        assert!(
            !verify(
                &target,
                &leader,
                &context,
                Instant::now() + Duration::from_secs(1),
                Instant::now,
                |_| panic!("invalid job must not query processes: {case}")
            )
            .unwrap(),
            "{case}"
        );
    }
}

#[test]
fn job_proof_errors_absence_and_expired_observations_are_s0() {
    for case in [
        "target-error",
        "target-gone",
        "leader-error",
        "leader-gone",
        "deadline-before",
        "deadline-after-target",
        "deadline-after-leader",
    ] {
        let (target, leader, context, child, anchor) = fixture();
        let start = Instant::now();
        let deadline = start + Duration::from_secs(1);
        let now = Cell::new(if case == "deadline-before" {
            deadline
        } else {
            start
        });
        let reads = Cell::new(0);
        let (mut child, mut anchor) = (Some(child), Some(anchor));
        let matched = verify(
            &target,
            &leader,
            &context,
            deadline,
            || now.get(),
            |identity| {
                reads.set(reads.get() + 1);
                if identity.pid == target.pid {
                    if case == "target-error" {
                        return Err(io::Error::from(io::ErrorKind::PermissionDenied));
                    }
                    if case == "target-gone" {
                        return Ok(None);
                    }
                    if case == "deadline-after-target" {
                        now.set(deadline);
                    }
                    Ok(child.take())
                } else {
                    assert_eq!(reads.get(), 2);
                    if case == "leader-error" {
                        return Err(io::Error::from(io::ErrorKind::PermissionDenied));
                    }
                    if case == "leader-gone" {
                        return Ok(None);
                    }
                    if case == "deadline-after-leader" {
                        now.set(deadline);
                    }
                    Ok(anchor.take())
                }
            },
        )
        .unwrap();
        assert!(!matched, "{case}");
        let expected_reads = match case {
            "deadline-before" => 0,
            "target-error" | "target-gone" | "deadline-after-target" => 1,
            _ => 2,
        };
        assert_eq!(reads.get(), expected_reads, "{case}");
    }
}

#[test]
fn job_deadline_failure_classification_preserves_io() {
    use crate::recovery::{RecoveryReport, ScanFailure, ScanFailureKind};
    let now = Instant::now();
    super::check_budget(now, now + Duration::from_nanos(1)).unwrap();
    for deadline in [now, now - Duration::from_nanos(1)] {
        let error = super::check_budget(now, deadline).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(
            error.to_string(),
            io::Error::from(io::ErrorKind::TimedOut).to_string()
        );
        assert_eq!(
            ScanFailure {
                error,
                report: RecoveryReport::default()
            }
            .kind(),
            ScanFailureKind::DeadlineElapsed
        );
    }
}
