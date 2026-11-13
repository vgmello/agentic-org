use std::time::Duration;

use org_office::{Key, NewOffice, OfficeRole};

use super::*;

fn made() -> (tempfile::TempDir, Office) {
    let dir = tempfile::tempdir().unwrap();
    let new = NewOffice {
        office_id: "OFFICE1".into(),
        office_name: "HQ".into(),
        role: OfficeRole::Hq,
        org_id: "ORG1".into(),
        org_name: "Acme".into(),
    };
    let folder = dir.path().join("offices").join("OFFICE1");
    let office = Office::create(
        folder,
        &new,
        Key::from_db_key(org_proto::DbKey::from_bytes([5; 32])),
    )
    .unwrap();
    (dir, office)
}

fn target(pid: u32) -> CheckpointTarget {
    let id = |pid| ProcessIdentity {
        pid,
        started: format!("s{pid}"),
        boot: "b".into(),
    };
    CheckpointTarget {
        identity: id(pid),
        ancestry: vec![id(2)],
    }
}

fn later() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

#[test]
fn rows_read_back_as_validated_records_with_their_hints() {
    let (_dir, office) = made();
    let key = office.record_owned_mark("b112-roundtrip").unwrap();
    let leader = SavedLeader {
        pid: 9,
        pgid: 9,
        started: "s9".into(),
        boot: "b".into(),
    };
    office.set_owned_leader(key, &leader).unwrap();
    office.add_owned_tool_session(key, 40).unwrap();
    office.add_owned_tool_session(key, 41).unwrap();
    let mut records = OfficeRecoveryRecords(&office);
    records
        .checkpoint(&key, "b112-roundtrip", &[target(3), target(4)], later())
        .unwrap();
    let pending = records.pending().unwrap();
    let loaded = records.load(&key, later()).unwrap().unwrap();
    for (record, from) in [(&pending[0].1, "pending"), (&loaded, "load")] {
        assert_eq!(record.mark(), "b112-roundtrip", "{from}");
        let expected = ChildIdentity {
            pid: 9,
            pgid: 9,
            started: "s9".into(),
            boot: "b".into(),
            mark: "b112-roundtrip".into(),
        };
        assert_eq!(record.leader(), Some(&expected), "{from}");
        assert_eq!(record.tool_sessions(), [40, 41], "{from}");
        assert_eq!(record.inventory(), [target(3), target(4)], "{from}");
    }
    assert_eq!((pending.len(), pending[0].0), (1, key));
    // The office's own row holds the same values, field by field.
    let raw = office.load_owned_process(key).unwrap().unwrap().inventory;
    let saved = |pid| SavedIdentity {
        pid,
        started: format!("s{pid}"),
        boot: "b".into(),
    };
    let expected = |pid| SavedTarget {
        identity: saved(pid),
        ancestry: vec![saved(2)],
    };
    assert_eq!(raw, [expected(3), expected(4)]);
    assert!(office.retire_owned_process(key).unwrap());
    assert!(records.load(&key, later()).unwrap().is_none());
}

#[test]
fn refusals_reach_recovery_as_io_errors_and_write_nothing() {
    let (_dir, office) = made();
    let key = office.record_owned_mark("b112-refusals").unwrap();
    let mut records = OfficeRecoveryRecords(&office);
    records
        .checkpoint(&key, "b112-refusals", &[target(3)], later())
        .unwrap();
    let too_many: Vec<_> = (3..300).map(target).collect();
    // Fewer than 256 targets, but past 256 identities once their distinct ancestors count.
    let by_ancestry: Vec<_> = (3..203)
        .map(|pid| CheckpointTarget {
            ancestry: vec![target(pid + 1000).identity],
            ..target(pid)
        })
        .collect();
    let refusals = [
        (
            records.checkpoint(&key, "b112-refusals", &[target(4)], later()),
            io::ErrorKind::InvalidInput,
        ),
        (
            records.checkpoint(&key, "b112-other", &[target(3)], later()),
            io::ErrorKind::InvalidInput,
        ),
        (
            records.checkpoint(&key, "b112-refusals", &too_many, later()),
            io::ErrorKind::InvalidInput,
        ),
        (
            records.checkpoint(&key, "b112-refusals", &by_ancestry, later()),
            io::ErrorKind::InvalidInput,
        ),
        (
            records.checkpoint(
                &key,
                "b112-refusals",
                &[target(3), target(4)],
                Instant::now(),
            ),
            io::ErrorKind::TimedOut,
        ),
        (
            records.load(&key, Instant::now()).map(|_| ()),
            io::ErrorKind::TimedOut,
        ),
    ];
    for (n, (result, kind)) in refusals.into_iter().enumerate() {
        assert_eq!(result.map_err(|e| e.kind()), Err(kind), "{n}");
    }
    assert_eq!(
        records.load(&key, later()).unwrap().unwrap().inventory(),
        [target(3)]
    );
}

#[test]
fn a_row_recovery_would_refuse_fails_the_whole_read() {
    let (_dir, office) = made();
    office.record_owned_mark("b112-good").unwrap();
    let key = office.record_owned_mark("b112-bad").unwrap();
    // A leader whose group isn't its own: RecoveryRecord::new refuses it.
    let leader = SavedLeader {
        pid: 9,
        pgid: 8,
        started: "s9".into(),
        boot: "b".into(),
    };
    office.set_owned_leader(key, &leader).unwrap();
    let mut records = OfficeRecoveryRecords(&office);
    assert_eq!(
        records.pending().map_err(|e| e.kind()).err(),
        Some(io::ErrorKind::InvalidInput)
    );
    assert!(records.load(&key, later()).is_err());
}

#[test]
fn the_write_helpers_refuse_what_recovery_would_refuse() {
    let (_dir, office) = made();
    assert_eq!(
        record_mark(&office, "not a mark!")
            .map_err(|e| e.kind())
            .err(),
        Some(io::ErrorKind::InvalidInput)
    );
    let key = record_mark(&office, "b112-writes").unwrap();
    let leader = ChildIdentity {
        pid: 9,
        pgid: 9,
        started: "s9".into(),
        boot: "b".into(),
        mark: "b112-writes".into(),
    };
    let refused = [
        ChildIdentity {
            pgid: 8,
            ..leader.clone()
        },
        ChildIdentity {
            mark: "b112-other".into(),
            ..leader.clone()
        },
    ];
    for bad in refused {
        assert_eq!(
            record_leader(&office, key, &bad)
                .map_err(|e| e.kind())
                .err(),
            Some(io::ErrorKind::InvalidInput)
        );
    }
    record_leader(&office, key, &leader).unwrap();
    let mut records = OfficeRecoveryRecords(&office);
    assert_eq!(records.pending().unwrap()[0].1.leader(), Some(&leader));
}
