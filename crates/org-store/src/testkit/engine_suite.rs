//! The conformance suite for `Engine` and `CorePort` (ENG-3). Every engine invokes
//! [`engine_conformance_tests!`](crate::engine_conformance_tests) once with a closure
//! that opens a fresh, empty engine and returns it with whatever must outlive it (a
//! temporary directory, or `()`).

use crate::{
    change::{Ack, Actor, Outcome, Receipt, Sha256, Timestamp},
    engine::{CorePort, CoreRead, Cursor, Engine},
    error::StoreError,
    path::DurablePath,
    sequences::{FeedPosition, HighWaters},
    testkit::id,
};

/// A cursor at `ordinal` with distinct values in every field.
pub fn cursor(ordinal: u64) -> Cursor {
    Cursor {
        ordinal,
        sha256: Sha256::of(&ordinal.to_be_bytes()),
        feed: FeedPosition(ordinal + 100),
        high_waters: HighWaters {
            organization: ordinal + 1,
            work: ordinal + 2,
            message_content: ordinal + 3,
            message_state: ordinal + 4,
        },
    }
}

fn path(text: &str) -> DurablePath {
    DurablePath::parse(text).expect("a valid test path")
}

fn receipt(n: u64, reply: &[u8]) -> Receipt {
    Receipt {
        id: id(n),
        kind: "suite.operation".into(),
        actor: if n.is_multiple_of(2) {
            Actor::System
        } else {
            Actor::Employee(id(n + 1_000))
        },
        fingerprint: Sha256::of(&n.to_le_bytes()),
        outcome: if n.is_multiple_of(3) {
            Outcome::Rejected
        } else {
            Outcome::Completed
        },
        ack: if n.is_multiple_of(2) {
            Ack::Durable
        } else {
            Ack::Committed
        },
        ordinal: n,
        at: Timestamp(1_700_000_000_000 + n),
        reply: reply.to_vec().into(),
    }
}

pub fn cursor_round_trip<E: Engine, G>(open: impl Fn() -> (E, G)) {
    let (engine, _guard) = open();
    assert_eq!(engine.snapshot().unwrap().cursor().unwrap(), None);
    let tx = engine.begin().unwrap();
    assert_eq!(tx.cursor().unwrap(), None);
    engine.commit(tx, &cursor(4)).unwrap();
    assert_eq!(
        engine.snapshot().unwrap().cursor().unwrap(),
        Some(cursor(4))
    );
    let tx = engine.begin().unwrap();
    assert_eq!(tx.cursor().unwrap(), Some(cursor(4)));
    engine.commit(tx, &cursor(9)).unwrap();
    // The stream high-waters are committed with the cursor, so a read returns the
    // directory generation of its own snapshot.
    let read = engine.snapshot().unwrap().cursor().unwrap().unwrap();
    assert_eq!(read, cursor(9));
    assert_eq!(read.high_waters.organization, 10);
}

pub fn sources<E: Engine, G>(open: impl Fn() -> (E, G)) {
    let (engine, _guard) = open();
    let mut tx = engine.begin().unwrap();
    let names = [
        "state/teams/b/team.yaml",
        "state/teams/a/team.yaml",
        "state/teams-old/a",
        "state/organization.yaml",
        "local/offices/o/x",
    ];
    for name in names {
        tx.set_source(&path(name), Some(Sha256::of(name.as_bytes())))
            .unwrap();
    }
    let hash = tx.source_hash(&path("state/teams/a/team.yaml")).unwrap();
    assert_eq!(hash, Some(Sha256::of(b"state/teams/a/team.yaml")));
    assert_eq!(tx.source_hash(&path("state/none")).unwrap(), None);
    // Under a prefix by whole components, in byte order of path.
    assert_eq!(
        tx.sources_under(&path("state/teams")).unwrap(),
        [
            path("state/teams/a/team.yaml"),
            path("state/teams/b/team.yaml")
        ]
    );
    assert_eq!(tx.sources_under(&path("state")).unwrap().len(), 4);
    assert_eq!(
        tx.sources_under(&path("state/organization.yaml")).unwrap(),
        [path("state/organization.yaml")]
    );
    assert!(tx.sources_under(&path("budgets")).unwrap().is_empty());
    // A new hash replaces; `None` removes.
    tx.set_source(&path("state/teams/a/team.yaml"), Some(Sha256::of(b"v2")))
        .unwrap();
    tx.set_source(&path("state/teams/b/team.yaml"), None)
        .unwrap();
    tx.set_source(&path("state/never-there"), None).unwrap();
    assert_eq!(
        tx.sources_under(&path("state/teams")).unwrap(),
        [path("state/teams/a/team.yaml")]
    );
    engine.commit(tx, &cursor(1)).unwrap();
    let snapshot = engine.snapshot().unwrap();
    let hash = snapshot
        .source_hash(&path("state/teams/a/team.yaml"))
        .unwrap();
    assert_eq!(hash, Some(Sha256::of(b"v2")));
    assert_eq!(snapshot.sources_under(&path("state")).unwrap().len(), 3);
    assert_eq!(snapshot.sources_under(&path("local")).unwrap().len(), 1);
}

pub fn receipts<E: Engine, G>(open: impl Fn() -> (E, G)) {
    let (engine, _guard) = open();
    let mut tx = engine.begin().unwrap();
    let text = receipt(1, b"{\"ok\":true}");
    let binary = receipt(6, &[0, 255, 128, 10]);
    let empty = receipt(3, b"");
    for receipt in [&text, &binary, &empty] {
        tx.put_receipt(receipt).unwrap();
    }
    // Visible to later operations of the same transaction, with every field.
    assert_eq!(tx.receipt(&id(1)).unwrap(), Some(text.clone()));
    assert_eq!(tx.receipt(&id(2)).unwrap(), None);
    // Never replaced: not by the same receipt, not by another actor or fingerprint.
    assert!(tx.put_receipt(&text).is_err());
    let mut other = receipt(1, b"other");
    other.fingerprint = Sha256::of(b"another request");
    assert!(tx.put_receipt(&other).is_err());
    engine.commit(tx, &cursor(6)).unwrap();
    let snapshot = engine.snapshot().unwrap();
    assert_eq!(snapshot.receipt(&id(1)).unwrap(), Some(text));
    assert_eq!(snapshot.receipt(&id(6)).unwrap(), Some(binary));
    assert_eq!(snapshot.receipt(&id(3)).unwrap(), Some(empty));
    let mut tx = engine.begin().unwrap();
    assert!(tx.put_receipt(&other).is_err());
}

pub fn mark_and_undo<E: Engine, G>(open: impl Fn() -> (E, G)) {
    let (engine, _guard) = open();
    let mut tx = engine.begin().unwrap();
    tx.set_source(&path("state/a"), Some(Sha256::of(b"a")))
        .unwrap();
    tx.put_receipt(&receipt(1, b"one")).unwrap();
    let mark = tx.mark().unwrap();
    tx.set_source(&path("state/a"), Some(Sha256::of(b"a2")))
        .unwrap();
    tx.set_source(&path("state/b"), Some(Sha256::of(b"b")))
        .unwrap();
    tx.put_receipt(&receipt(2, b"two")).unwrap();
    tx.undo_to(mark).unwrap();
    assert_eq!(
        tx.source_hash(&path("state/a")).unwrap(),
        Some(Sha256::of(b"a"))
    );
    assert_eq!(tx.source_hash(&path("state/b")).unwrap(), None);
    assert_eq!(tx.receipt(&id(2)).unwrap(), None);
    assert!(tx.receipt(&id(1)).unwrap().is_some());
    // The undone operation leaves the transaction usable: the next one marks again.
    let mark = tx.mark().unwrap();
    tx.put_receipt(&receipt(2, b"two again")).unwrap();
    tx.set_source(&path("state/c"), Some(Sha256::of(b"c")))
        .unwrap();
    let _ = mark;
    tx.check().unwrap();
    engine.commit(tx, &cursor(2)).unwrap();
    let snapshot = engine.snapshot().unwrap();
    assert_eq!(
        snapshot.receipt(&id(2)).unwrap().unwrap().reply.as_ref(),
        b"two again"
    );
    assert_eq!(
        snapshot.sources_under(&path("state")).unwrap(),
        [path("state/a"), path("state/c")]
    );
}

pub fn snapshot_isolation<E: Engine, G>(open: impl Fn() -> (E, G)) {
    let (engine, _guard) = open();
    let mut tx = engine.begin().unwrap();
    tx.set_source(&path("state/a"), Some(Sha256::of(b"1")))
        .unwrap();
    engine.commit(tx, &cursor(1)).unwrap();

    let before = engine.snapshot().unwrap();
    let mut tx = engine.begin().unwrap();
    tx.set_source(&path("state/a"), Some(Sha256::of(b"2")))
        .unwrap();
    tx.set_source(&path("state/b"), Some(Sha256::of(b"b")))
        .unwrap();
    tx.put_receipt(&receipt(5, b"five")).unwrap();
    // A reader never sees part of a batch, and reads while the writer holds its transaction.
    let during = engine.snapshot().unwrap();
    assert_eq!(
        during.source_hash(&path("state/a")).unwrap(),
        Some(Sha256::of(b"1"))
    );
    assert_eq!(during.receipt(&id(5)).unwrap(), None);
    assert_eq!(during.cursor().unwrap(), Some(cursor(1)));
    engine.commit(tx, &cursor(2)).unwrap();
    let after = engine.snapshot().unwrap();
    assert_eq!(
        after.source_hash(&path("state/a")).unwrap(),
        Some(Sha256::of(b"2"))
    );
    assert_eq!(after.cursor().unwrap(), Some(cursor(2)));
    assert!(after.receipt(&id(5)).unwrap().is_some());
    drop((before, during));

    // A transaction dropped without commit leaves nothing.
    let mut tx = engine.begin().unwrap();
    tx.set_source(&path("state/c"), Some(Sha256::of(b"c")))
        .unwrap();
    drop(tx);
    let snapshot = engine.snapshot().unwrap();
    assert_eq!(snapshot.source_hash(&path("state/c")).unwrap(), None);
    assert_eq!(snapshot.cursor().unwrap(), Some(cursor(2)));
    let tx = engine.begin().unwrap();
    assert_eq!(tx.source_hash(&path("state/c")).unwrap(), None);
}

pub fn atomic_replace<E: Engine, G>(open: impl Fn() -> (E, G)) {
    let (engine, _guard) = open();
    let mut tx = engine.begin().unwrap();
    tx.set_source(&path("state/old"), Some(Sha256::of(b"old")))
        .unwrap();
    tx.put_receipt(&receipt(1, b"old")).unwrap();
    engine.commit(tx, &cursor(3)).unwrap();

    // A build that fails leaves the current projection in place.
    let failed = engine.replace(&mut |tx| {
        tx.set_source(&path("state/half"), Some(Sha256::of(b"half")))?;
        Err(StoreError::Internal("the build failed".into()))
    });
    assert_eq!(failed, Err(StoreError::Internal("the build failed".into())));
    let snapshot = engine.snapshot().unwrap();
    assert_eq!(snapshot.cursor().unwrap(), Some(cursor(3)));
    assert!(snapshot.source_hash(&path("state/old")).unwrap().is_some());
    assert_eq!(snapshot.source_hash(&path("state/half")).unwrap(), None);
    drop(snapshot);

    // A build starts from an empty projection and replaces everything at once.
    engine
        .replace(&mut |tx| {
            assert_eq!(tx.cursor()?, None);
            assert_eq!(tx.receipt(&id(1))?, None);
            assert!(tx.sources_under(&path("state"))?.is_empty());
            tx.set_source(&path("state/new"), Some(Sha256::of(b"new")))?;
            tx.put_receipt(&receipt(2, b"new"))?;
            tx.check()?;
            Ok(cursor(7))
        })
        .unwrap();
    let snapshot = engine.snapshot().unwrap();
    assert_eq!(snapshot.cursor().unwrap(), Some(cursor(7)));
    assert_eq!(
        snapshot.sources_under(&path("state")).unwrap(),
        [path("state/new")]
    );
    assert_eq!(snapshot.receipt(&id(1)).unwrap(), None);
    assert!(snapshot.receipt(&id(2)).unwrap().is_some());
    drop(snapshot);
    // The writer goes on from the replaced projection.
    let mut tx = engine.begin().unwrap();
    assert_eq!(tx.cursor().unwrap(), Some(cursor(7)));
    tx.set_source(&path("state/next"), Some(Sha256::of(b"n")))
        .unwrap();
    engine.commit(tx, &cursor(8)).unwrap();
    assert_eq!(
        engine
            .snapshot()
            .unwrap()
            .sources_under(&path("state"))
            .unwrap()
            .len(),
        2
    );
}

/// Declares one test per case of the engine suite.
#[macro_export]
macro_rules! engine_conformance_tests {
    ($open:expr) => {
        #[test]
        fn engine_cursor_round_trip() {
            $crate::testkit::engine_suite::cursor_round_trip($open);
        }
        #[test]
        fn engine_sources() {
            $crate::testkit::engine_suite::sources($open);
        }
        #[test]
        fn engine_receipts() {
            $crate::testkit::engine_suite::receipts($open);
        }
        #[test]
        fn engine_mark_and_undo() {
            $crate::testkit::engine_suite::mark_and_undo($open);
        }
        #[test]
        fn engine_snapshot_isolation() {
            $crate::testkit::engine_suite::snapshot_isolation($open);
        }
        #[test]
        fn engine_atomic_replace() {
            $crate::testkit::engine_suite::atomic_replace($open);
        }
    };
}
