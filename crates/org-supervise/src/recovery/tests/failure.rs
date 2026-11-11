use super::*;

fn classified(error: io::Error) -> ScanFailureKind {
    ScanFailure {
        error,
        report: RecoveryReport::default(),
    }
    .kind()
}

#[test]
fn failure_classification_deadline_boundary_keeps_error_kind_and_message() {
    let mut ops = Processes::new(vec![]);
    let now = ops.now;
    check_deadline(&mut ops, now + Duration::from_nanos(1)).unwrap();
    for deadline in [now, now - Duration::from_nanos(1)] {
        let error = check_deadline(&mut ops, deadline).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(error.to_string(), "recovery deadline elapsed");
        assert_eq!(classified(error), ScanFailureKind::DeadlineElapsed);
    }
    let batch = recover_startup(&mut records(false), &mut ops, now).unwrap();
    assert_eq!(batch.unresolved.len(), 1);
    assert_eq!(
        batch.unresolved[0].1.kind(),
        ScanFailureKind::DeadlineElapsed
    );
    assert!(ops.signals().is_empty());
}

#[test]
fn failure_classification_inventory_boundary_and_ancestry_disjunct() {
    validate_target_count(256).unwrap();
    let error = validate_target_count(257).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(
        error.to_string(),
        "recovery inventory exceeds 256 identities"
    );
    assert_eq!(classified(error), ScanFailureKind::InventoryOverflow);
    let target = |ancestry: usize| CheckpointTarget {
        identity: marked(2000).identity.unwrap(),
        ancestry: (1000..1000 + ancestry as u32)
            .map(|pid| marked(pid).identity.unwrap())
            .collect(),
    };
    validate_inventory(&[target(255)]).unwrap();
    let error = validate_inventory(&[target(256)]).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(
        error.to_string(),
        "invalid or duplicate saved process identity"
    );
    assert_eq!(classified(error), ScanFailureKind::InventoryOverflow);
    let mut invalid = target(0);
    invalid.identity.pid = 0;
    assert_eq!(
        classified(validate_inventory(&[invalid]).unwrap_err()),
        ScanFailureKind::Other
    );
    let duplicate = target(0);
    assert_eq!(
        classified(validate_inventory(&[duplicate.clone(), duplicate]).unwrap_err()),
        ScanFailureKind::Other
    );
}

#[test]
fn failure_classification_never_infers_a_tag_from_text_or_io_kind() {
    assert_eq!(
        classified(io::Error::from(io::ErrorKind::TimedOut)),
        ScanFailureKind::Other
    );
    assert_eq!(
        classified(io::Error::from_raw_os_error(libc::EPERM)),
        ScanFailureKind::Other
    );
    for kind in [
        io::ErrorKind::TimedOut,
        io::ErrorKind::InvalidData,
        io::ErrorKind::Other,
    ] {
        for message in [
            "recovery deadline elapsed",
            "native recovery deadline elapsed",
            "recovery inventory exceeds 256 identities",
            "invalid or duplicate saved process identity",
        ] {
            assert_eq!(
                classified(io::Error::new(kind, message)),
                ScanFailureKind::Other
            );
        }
    }
    let mut ops = Processes::new(vec![]);
    ops.snapshot_failure = true;
    let result = run(false, &mut ops);
    assert_eq!(result.unresolved.len(), 1);
    assert_eq!(result.unresolved[0].1.kind(), ScanFailureKind::Other);
}

#[test]
fn failure_classification_real_adapter_snapshot_and_optional_job_deadlines() {
    let error = NativeRecovery::new()
        .unwrap()
        .snapshot(Instant::now() - Duration::from_nanos(1))
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert_eq!(error.to_string(), "native recovery deadline elapsed");
    assert_eq!(classified(error), ScanFailureKind::DeadlineElapsed);
    let mut ops = Processes::new(vec![]);
    ops.snapshot_deadline = true;
    let result = run(false, &mut ops);
    assert_eq!(result.unresolved[0].1.error.kind(), io::ErrorKind::TimedOut);
    assert_eq!(
        result.unresolved[0].1.error.to_string(),
        "native recovery deadline elapsed"
    );
    assert_eq!(result.unresolved.len(), 1);
    assert_eq!(
        result.unresolved[0].1.kind(),
        ScanFailureKind::DeadlineElapsed
    );
    assert!(ops.signals().is_empty());
    let mut ops = Processes::new(vec![marked(100), unreadable_job_member(200, 100, 100)]);
    ops.job_error = true;
    ops.job_late = true;
    let result = run(true, &mut ops);
    assert_eq!(result.unresolved[0].1.error.kind(), io::ErrorKind::TimedOut);
    assert_eq!(
        result.unresolved[0].1.error.to_string(),
        "recovery deadline elapsed"
    );
    assert_eq!(result.unresolved.len(), 1);
    assert_eq!(
        result.unresolved[0].1.kind(),
        ScanFailureKind::DeadlineElapsed
    );
    assert!(ops.signals().is_empty());
}

#[test]
fn failure_classification_preserves_inventory_guard_order_and_other_failures() {
    let mut oversized = CheckpointTarget {
        identity: marked(2000).identity.unwrap(),
        ancestry: (1000..1256)
            .map(|pid| marked(pid).identity.unwrap())
            .collect(),
    };
    oversized.identity.pid = 0;
    assert_eq!(
        classified(validate_inventory(&[oversized.clone()]).unwrap_err()),
        ScanFailureKind::Other
    );
    oversized.identity = marked(2000).identity.unwrap();
    let duplicate = CheckpointTarget {
        identity: oversized.identity.clone(),
        ancestry: vec![],
    };
    assert_eq!(
        classified(validate_inventory(&[duplicate, oversized]).unwrap_err()),
        ScanFailureKind::InventoryOverflow
    );
    for stage in ["duplicate", "signal", "live-leader", "live-target"] {
        let mut ops = Processes::new(vec![marked(100), marked(200)]);
        match stage {
            "duplicate" => ops.live.push(marked(200)),
            "signal" => ops.signal_failure = Some(100),
            _ => ops.kill_exits = false,
        }
        let result = run(stage != "live-target", &mut ops);
        assert_eq!(result.unresolved.len(), 1, "{stage}");
        assert_eq!(
            result.unresolved[0].1.kind(),
            ScanFailureKind::Other,
            "{stage}"
        );
    }
}

#[test]
fn failure_classification_capture_overflow_reaches_the_caller_without_signals() {
    let mut ops = Processes::new((200..457).map(marked).collect());
    let result = run(false, &mut ops);
    assert_eq!(result.unresolved.len(), 1);
    assert_eq!(
        result.unresolved[0].1.kind(),
        ScanFailureKind::InventoryOverflow
    );
    assert_eq!(
        result.unresolved[0].1.error.kind(),
        io::ErrorKind::InvalidData
    );
    assert_eq!(
        result.unresolved[0].1.error.to_string(),
        "recovery inventory exceeds 256 identities"
    );
    assert!(ops.signals().is_empty());
    let mut ops = Processes::new((200..456).map(marked).collect());
    let result = run(false, &mut ops);
    assert!(result.unresolved.is_empty());
    assert_eq!(result.gone.len(), 1);
    assert_eq!(ops.signals().len(), 256);
}
