use std::io;
use std::time::{Duration, Instant};

use tempfile::TempDir;

use super::*;
use crate::{Key, NewOffice, OfficeRole};

fn made() -> (TempDir, Office) {
    let dir = TempDir::new().unwrap();
    let new = NewOffice {
        office_id: "OFFICE1".into(),
        office_name: "HQ".into(),
        role: OfficeRole::Hq,
        org_id: "ORG1".into(),
        org_name: "Acme".into(),
    };
    let folder = dir.path().join("offices").join("OFFICE1");
    let office = Office::create(folder, &new, Key::from_bytes([5; 32])).unwrap();
    (dir, office)
}

fn leader(pid: u32) -> SavedLeader {
    SavedLeader {
        pid,
        pgid: pid,
        started: format!("s{pid}"),
        boot: "b".into(),
    }
}

#[test]
fn a_row_keeps_its_leader_and_hints_until_it_is_retired() {
    let (dir, office) = made();
    let key = office.record_owned_mark("m1").unwrap();
    office.set_owned_leader(key, &leader(9)).unwrap();
    office.set_owned_leader(key, &leader(9)).unwrap();
    assert!(matches!(
        office.set_owned_leader(key, &leader(8)),
        Err(OfficeError::Invalid(_))
    ));
    office.add_owned_tool_session(key, 40).unwrap();
    office.add_owned_tool_session(key, 40).unwrap();
    office.add_owned_tool_session(key, 41).unwrap();
    assert!(matches!(
        office.record_owned_mark("m1"),
        Err(OfficeError::AlreadyExists)
    ));
    drop(office);
    let folder = dir.path().join("offices").join("OFFICE1");
    let office = Office::open(folder, Key::from_bytes([5; 32])).unwrap();
    let expected = OwnedProcess {
        key,
        mark: "m1".into(),
        leader: Some(leader(9)),
        tool_sessions: vec![40, 41],
        inventory: vec![],
    };
    assert_eq!(
        office.load_owned_process(key).unwrap(),
        Some(expected.clone())
    );
    assert_eq!(office.pending_owned_processes().unwrap(), [expected]);
    assert!(office.retire_owned_process(key).unwrap());
    assert!(!office.retire_owned_process(key).unwrap());
    assert_eq!(office.load_owned_process(key).unwrap(), None);
    assert!(office.pending_owned_processes().unwrap().is_empty());
    // An id is never reused, so a retired key can't match a later row.
    assert!(office.record_owned_mark("m2").unwrap().id > key.id);
}

#[test]
fn a_stale_generation_or_id_touches_nothing() {
    let (_dir, office) = made();
    let key = office.record_owned_mark("m1").unwrap();
    let stale = OwnedProcessKey {
        generation: key.generation + 1,
        ..key
    };
    let missing = OwnedProcessKey {
        id: key.id + 1,
        ..key
    };
    for other in [stale, missing] {
        assert!(!office.retire_owned_process(other).unwrap());
        assert_eq!(office.load_owned_process(other).unwrap(), None);
        assert!(matches!(
            office.set_owned_leader(other, &leader(9)),
            Err(OfficeError::Invalid(_))
        ));
        assert!(matches!(
            office.add_owned_tool_session(other, 40),
            Err(OfficeError::Invalid(_))
        ));
    }
    let row = office.load_owned_process(key).unwrap().unwrap();
    assert_eq!((row.leader, row.tool_sessions), (None, vec![]));
}

#[test]
fn one_undecodable_row_fails_the_whole_pending_read() {
    let leader = "leader_pgid=1, leader_started='s', leader_boot='b', leader_pid";
    let bad = [
        ("inventory", "not json"),
        ("inventory", r#"{"v":2,"targets":[]}"#),
        ("inventory", r#"{"targets":[]}"#),
        ("inventory", r#"{"v":1,"targets":[],"x":0}"#),
        (
            "inventory",
            r#"{"v":1,"targets":[{"identity":{"pid":-1,"started":"s","boot":"b"},"ancestry":[]}]}"#,
        ),
        (
            "inventory",
            r#"{"v":1,"targets":[{"identity":{"pid":2,"started":"s","boot":"b","x":0},"ancestry":[]}]}"#,
        ),
        (
            "inventory",
            r#"{"v":1,"targets":[{"identity":{"pid":2,"started":"s","boot":"b"},"ancestry":[],"x":0}]}"#,
        ),
        ("tool_sessions", "[-1]"),
        (leader, "-1"),
        (leader, "4294967296"),
    ];
    for (column, text) in bad {
        let (_dir, office) = made();
        office.record_owned_mark("good").unwrap();
        let key = office.record_owned_mark("bad").unwrap();
        office
            .local_store()
            .write(move |db| {
                let sql = format!("UPDATE owned_processes SET {column}=?2 WHERE id=?1");
                let value: rusqlite::types::Value = match text.parse::<i64>() {
                    Ok(number) => number.into(),
                    Err(_) => text.to_owned().into(),
                };
                db.execute(&sql, (key.id, value))
                    .map_err(OfficeError::storage)?;
                Ok(((), None))
            })
            .unwrap();
        let read = office.pending_owned_processes();
        assert!(
            matches!(read, Err(OfficeError::Storage(_))),
            "{column} {text}: {read:?}"
        );
        assert!(office.load_owned_process(key).is_err(), "{column} {text}");
    }
}

#[test]
fn owned_rows_are_written_through_a_synchronous_full_connection() {
    let (_dir, office) = made();
    let (sync, full): (i64, i64) = office
        .local_store()
        .write(|db| {
            let get = |name| db.pragma_query_value(None, name, |r| r.get(0)).unwrap();
            Ok(((get("synchronous"), get("fullfsync")), None))
        })
        .unwrap();
    assert_eq!((sync, full), (2, 1), "synchronous=FULL, fullfsync=ON");
}

/// The mark whose checkpoint commits late: after its write, the writer waits past the deadline.
static DELAYED: std::sync::Mutex<Option<&str>> = std::sync::Mutex::new(None);

pub(crate) fn pause_before_commit(mark: &str, deadline: Instant) {
    if *DELAYED.lock().unwrap_or_else(|p| p.into_inner()) == Some(mark) {
        while Instant::now() <= deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn target(pid: u32) -> SavedTarget {
    let identity = |pid| SavedIdentity {
        pid,
        started: format!("s{pid}"),
        boot: "b".into(),
    };
    SavedTarget {
        identity: identity(pid),
        ancestry: vec![identity(1)],
    }
}

fn later() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

fn reopen(dir: &TempDir) -> Office {
    let folder = dir.path().join("offices").join("OFFICE1");
    Office::open(folder, Key::from_bytes([5; 32])).unwrap()
}

fn timed_out(result: Result<(), OfficeError>) -> bool {
    matches!(result, Err(OfficeError::Storage(cause))
        if cause.downcast_ref::<io::Error>().map(io::Error::kind) == Some(io::ErrorKind::TimedOut))
}

#[test]
fn a_checkpoint_keeps_the_union_across_a_restart() {
    let (dir, office) = made();
    let key = office.record_owned_mark("m1").unwrap();
    office
        .checkpoint_owned_inventory(key, "m1", &[target(2)], later())
        .unwrap();
    office
        .checkpoint_owned_inventory(key, "m1", &[target(2), target(3)], later())
        .unwrap();
    // The same entries again, reordered, are still the union.
    office
        .checkpoint_owned_inventory(key, "m1", &[target(3), target(2)], later())
        .unwrap();
    drop(office);
    let office = reopen(&dir);
    let rows = office.pending_owned_processes().unwrap();
    assert_eq!((rows.len(), rows[0].key), (1, key));
    assert_eq!(rows[0].inventory, [target(3), target(2)]);
}

#[test]
fn a_checkpoint_refuses_omissions_stale_keys_and_marks_and_a_passed_deadline() {
    let (_dir, office) = made();
    let key = office.record_owned_mark("m1").unwrap();
    office
        .checkpoint_owned_inventory(key, "m1", &[target(2), target(3)], later())
        .unwrap();
    let stale = OwnedProcessKey {
        generation: key.generation + 1,
        ..key
    };
    let missing = OwnedProcessKey {
        id: key.id + 1,
        ..key
    };
    let all = [target(2), target(3), target(4)];
    let refusals = [
        office.checkpoint_owned_inventory(key, "m1", &[target(3), target(4)], later()),
        // A saved entry re-sent with other ancestry is a changed entry, not a union.
        office.checkpoint_owned_inventory(
            key,
            "m1",
            &[
                target(2),
                SavedTarget {
                    ancestry: vec![],
                    ..target(3)
                },
                target(4),
            ],
            later(),
        ),
        office.checkpoint_owned_inventory(key, "m2", &all, later()),
        office.checkpoint_owned_inventory(stale, "m1", &all, later()),
        office.checkpoint_owned_inventory(missing, "m1", &all, later()),
    ];
    for (n, refusal) in refusals.into_iter().enumerate() {
        assert!(
            matches!(refusal, Err(OfficeError::Invalid(_))),
            "{n}: {refusal:?}"
        );
    }
    // A deadline already passed when the write is about to be made: nothing is written.
    assert!(timed_out(office.checkpoint_owned_inventory(
        key,
        "m1",
        &all,
        Instant::now()
    )));
    let row = office.load_owned_process(key).unwrap().unwrap();
    assert_eq!((row.key, row.inventory), (key, vec![target(2), target(3)]));
}

#[test]
fn a_commit_that_finishes_late_is_timed_out_but_its_union_is_durable() {
    let (dir, office) = made();
    let key = office.record_owned_mark("late").unwrap();
    office
        .checkpoint_owned_inventory(key, "late", &[target(2)], later())
        .unwrap();
    *DELAYED.lock().unwrap() = Some("late");
    let deadline = Instant::now() + Duration::from_secs(1);
    let late = office.checkpoint_owned_inventory(key, "late", &[target(2), target(3)], deadline);
    *DELAYED.lock().unwrap() = None;
    assert!(timed_out(late), "ambiguous: TimedOut although it commits");
    drop(office);
    let row = reopen(&dir).load_owned_process(key).unwrap().unwrap();
    assert_eq!(
        row.inventory,
        [target(2), target(3)],
        "the caller reloads and finds the union"
    );
}
