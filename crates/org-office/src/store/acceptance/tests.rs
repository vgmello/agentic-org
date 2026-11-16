use super::*;
use crate::{office::OFFICE_SCHEMA, store::Opening, Key};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tempfile::TempDir;

fn config(dir: &Path) -> AcceptanceConfig {
    AcceptanceConfig {
        authority: None,
        identity: AcceptanceIdentity {
            instance_id: "instance".into(),
            organization_id: "org".into(),
            office_id: "office".into(),
            journal_incarnation: "journal-one".into(),
            writer_epoch: 1,
            machine_id: "machine".into(),
        },
        protected_root: dir.join("recovery"),
        public_root: dir.join("public"),
    }
}

#[derive(Default)]
struct ProtectedCheckpoint {
    position: std::sync::Mutex<Option<JournalPosition>>,
    refuse: std::sync::atomic::AtomicBool,
}
impl WriterAuthority for ProtectedCheckpoint {
    fn checkpoint(&self, _: &AcceptanceIdentity) -> Result<Option<JournalPosition>, OfficeError> {
        if self.refuse.load(Ordering::SeqCst) {
            return Err(invalid());
        }
        Ok(self.position.lock().unwrap().clone())
    }
    fn confirm(
        &self,
        _: &AcceptanceIdentity,
        before: Option<&JournalPosition>,
        after: &JournalPosition,
    ) -> Result<(), OfficeError> {
        if self.refuse.load(Ordering::SeqCst) {
            return Err(invalid());
        }
        let mut current = self.position.lock().unwrap();
        if current.as_ref() != before
            || !after.valid()
            || before.is_some_and(|p| p != after && !after.follows(p))
        {
            return Err(invalid());
        }
        *current = Some(after.clone());
        Ok(())
    }
}

#[test]
fn host_confirmation_loss_replays_one_frozen_successor_without_rerunning_decision() {
    let dir = TempDir::new().unwrap();
    let store = fresh(dir.path());
    let authority = Arc::new(ProtectedCheckpoint::default());
    let mut config = config(dir.path());
    config.authority = Some(authority.clone());
    store.install_acceptance(config.clone()).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&calls);
    let failing = authority.clone();
    assert!(store
        .selected_write(move |db| {
            count.fetch_add(1, Ordering::SeqCst);
            receipt(db, "confirmed-later")?;
            // Model channel loss after the live check and effect-free preparation.
            failing.refuse.store(true, Ordering::SeqCst);
            Ok(())
        })
        .is_err());
    assert_eq!(
        authority.position.lock().unwrap().as_ref().unwrap().ordinal,
        0
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    drop(store);
    authority.refuse.store(false, Ordering::SeqCst);
    let reopened = open(dir.path(), Opening::Existing);
    reopened.install_acceptance(config).unwrap();
    assert_eq!(
        authority.position.lock().unwrap().as_ref().unwrap().ordinal,
        1
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let value: String = reopened
        .selected_read(|db, _| {
            db.query_row(
                "SELECT CAST(result AS TEXT) FROM selected_receipts WHERE operation_id='confirmed-later'",
                [],
                |r| r.get(0),
            )
            .map_err(storage)
        })
        .unwrap();
    assert_eq!(value, "{}");
}

#[test]
fn protected_checkpoint_detects_in_place_rollback_before_projection_or_decision() {
    let dir = TempDir::new().unwrap();
    let store = fresh(dir.path());
    let authority = Arc::new(ProtectedCheckpoint::default());
    let mut config = config(dir.path());
    config.authority = Some(authority.clone());
    store.install_acceptance(config.clone()).unwrap();
    store
        .selected_write(|db| {
            receipt(db, "acknowledged")?;
            Ok(())
        })
        .unwrap();
    drop(store);
    let journal = config
        .protected_root
        .join("offices/office/journals/work-acceptance/journal-one/transactions/1.json");
    let bytes = std::fs::read(&journal).unwrap();
    std::fs::remove_file(&journal).unwrap();
    let reopened = open(dir.path(), Opening::Existing);
    assert!(reopened.install_acceptance(config.clone()).is_err());
    assert!(reopened
        .selected_write::<()>(|_| panic!("a restored writer cannot decide"))
        .is_err());
    // Refusal retained the acknowledged projection; it did not replay the anchor.
    let retained: String = reopened
        .control(|state| {
            state
                .db
                .query_row("SELECT CAST(result AS TEXT) FROM selected_receipts WHERE operation_id='acknowledged'", [], |r| {
                    r.get(0)
                })
                .map_err(storage)
        })
        .unwrap();
    assert_eq!(retained, "{}");
    drop(reopened);
    std::fs::write(journal, bytes).unwrap();
    let reopened = open(dir.path(), Opening::Existing);
    reopened.install_acceptance(config).unwrap();
    assert_eq!(
        reopened
            .selected_read(|db, _| db
                .query_row("SELECT CAST(result AS TEXT) FROM selected_receipts WHERE operation_id='acknowledged'", [], |r| r
                    .get::<_, String>(0))
                .map_err(storage))
            .unwrap(),
        "{}"
    );
}
fn open(dir: &Path, opening: Opening) -> Store {
    Store::open(
        &dir.join("office.db"),
        &Key::from_bytes([19; 32]),
        opening,
        &OFFICE_SCHEMA,
    )
    .unwrap()
}
fn fresh(dir: &Path) -> Store {
    let store = open(dir, Opening::New);
    store
        .write(|db| {
            db.execute_batch(
                "INSERT INTO meta VALUES('office_id','office'),('instance_id','instance')",
            )
            .map_err(storage)?;
            Ok(((), None))
        })
        .unwrap();
    store
}
fn events(db: &Connection, n: usize, version: u64) -> Result<(), OfficeError> {
    let target = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    db.execute("INSERT INTO work_feed(targets) VALUES('[]')", [])
        .map_err(storage)?;
    let feed = db.last_insert_rowid() as u64;
    let refs = (0..n)
        .map(|i| origin::OriginRef {
            target_id: target.into(),
            generation: 1,
            version: version + i as u64,
        })
        .collect::<Vec<_>>();
    origin::append(db, org_proto::Position { epoch: 1, seq: 1 }, &refs, feed, 1)?;
    Ok(())
}
fn receipt(db: &Connection, id: &str) -> Result<(), OfficeError> {
    db.execute("INSERT INTO selected_receipts VALUES('org',?1,'instance','account','owner',NULL,'task.set_assignment',1,'fingerprint',X'7B7D',3000)",[id]).map_err(storage)?;
    Ok(())
}
fn cursor(store: &Store) -> (i64, i64) {
    store
        .read(|db| {
            db.query_row(
                "SELECT ordinal,origin_high_water FROM selected_journal_cursor",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(storage)
        })
        .unwrap()
}
fn artifact(db: &Connection, path: &str, bytes: &[u8], immutable: bool) -> Result<(), OfficeError> {
    db.execute(
        "INSERT INTO selected_artifacts VALUES('public',?1,?2,?3)",
        params![path, bytes, immutable],
    )
    .map_err(storage)?;
    Ok(())
}

#[test]
fn zero_one_multi_mapping_and_reopen_preserve_one_event_allocator() {
    let dir = TempDir::new().unwrap();
    let c = config(dir.path());
    let store = fresh(dir.path());
    // Existing legacy positions belong to the anchor, never a restarted stream.
    store
        .write(|db| {
            events(db, 2, 1)?;
            Ok(((), None))
        })
        .unwrap();
    store.install_acceptance(c.clone()).unwrap();
    assert_eq!(cursor(&store), (0, 2));
    store
        .selected_write(|db| {
            events(db, 1, 3)?;
            receipt(db, "success-one")
        })
        .unwrap();
    assert_eq!(cursor(&store), (1, 3));
    store
        .selected_write(|db| receipt(db, "rejected-receipt-only"))
        .unwrap();
    assert_eq!(cursor(&store), (2, 3));
    store
        .selected_write(|db| {
            events(db, 2, 4)?;
            receipt(db, "legacy-multi")
        })
        .unwrap();
    assert_eq!(cursor(&store), (3, 5));
    drop(store);
    let store = open(dir.path(), Opening::Existing);
    store.install_acceptance(c.clone()).unwrap();
    assert_eq!(cursor(&store), (3, 5));
    let first = std::fs::read(
        c.protected_root
            .join("offices/office/journals/work-acceptance/journal-one/transactions/2.json"),
    )
    .unwrap();
    let json = codec::parse(&first).unwrap();
    assert_eq!(json.get("origin_before").unwrap().positive_i64(), Some(3));
    assert!(matches!(json.get("origin_entries"),Some(Json::Array(a)) if a.is_empty()));
}

#[test]
fn projection_failure_replays_frozen_bytes_without_decision_rerun() {
    let dir = TempDir::new().unwrap();
    let c = config(dir.path());
    let store = fresh(dir.path());
    store.install_acceptance(c.clone()).unwrap();
    std::fs::write(c.public_root.join("task.md"), b"outside writer").unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let result = store.selected_write(move |db| {
        observed.fetch_add(1, Ordering::SeqCst);
        events(db, 1, 1)?;
        receipt(db, "original")?;
        artifact(db, "task.md", b"frozen snapshot", false)
    });
    assert!(result.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(cursor(&store), (0, 0));
    assert!(c
        .protected_root
        .join("offices/office/journals/work-acceptance/journal-one/transactions/1.json")
        .is_file());
    assert!(store
        .selected_write(|db| receipt(db, "must-not-run"))
        .is_err());
    drop(store);
    std::fs::remove_file(c.public_root.join("task.md")).unwrap();
    let store = open(dir.path(), Opening::Existing);
    store.install_acceptance(c.clone()).unwrap();
    assert_eq!(cursor(&store), (1, 1));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        std::fs::read(c.public_root.join("task.md")).unwrap(),
        b"frozen snapshot"
    );
    let count: i64 = store
        .read(|db| {
            db.query_row("SELECT count(*) FROM selected_receipts", [], |r| r.get(0))
                .map_err(storage)
        })
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn blobs_multiple_edits_and_lost_projection_repair_are_exact() {
    let dir = TempDir::new().unwrap();
    let c = config(dir.path());
    let store = fresh(dir.path());
    store.install_acceptance(c.clone()).unwrap();
    let bytes = vec![0xf1; 100_000];
    let expected = bytes.clone();
    store
        .selected_write(move |db| {
            artifact(db, "event.bin", &bytes, true)?;
            artifact(db, "task.md", b"intermediate", false)?;
            db.execute(
                "UPDATE selected_artifacts SET content=X'66696E616C' WHERE path='task.md'",
                [],
            )
            .map_err(storage)?;
            Ok(())
        })
        .unwrap();
    assert_eq!(cursor(&store), (1, 0));
    let manifest = std::fs::read(
        c.protected_root
            .join("offices/office/journals/work-acceptance/journal-one/transactions/1.json"),
    )
    .unwrap();
    assert!(manifest.len() < 10_000);
    assert!(String::from_utf8(manifest)
        .unwrap()
        .contains("$journal_blob"));
    drop(store);
    std::fs::remove_file(c.public_root.join("event.bin")).unwrap();
    let store = open(dir.path(), Opening::Existing);
    store.install_acceptance(c.clone()).unwrap();
    assert_eq!(
        std::fs::read(c.public_root.join("event.bin")).unwrap(),
        expected
    );
    assert_eq!(
        std::fs::read(c.public_root.join("task.md")).unwrap(),
        b"final"
    );
}

#[test]
fn raw_work_bypass_and_identity_changes_are_fenced() {
    let dir = TempDir::new().unwrap();
    let c = config(dir.path());
    let store = fresh(dir.path());
    store.install_acceptance(c.clone()).unwrap();
    assert!(store
        .write(|db| {
            receipt(db, "bypass")?;
            Ok(((), None))
        })
        .is_err());
    store.selected_write(|db| receipt(db, "good")).unwrap();
    assert_eq!(cursor(&store), (1, 0));
    drop(store);
    let mut wrong = c;
    wrong.identity.writer_epoch = 2;
    let store = open(dir.path(), Opening::Existing);
    assert!(store.install_acceptance(wrong).is_err());
}

#[test]
fn altered_projection_and_missing_or_conflicting_journal_fail_closed() {
    let dir = TempDir::new().unwrap();
    let c = config(dir.path());
    let store = fresh(dir.path());
    store.install_acceptance(c.clone()).unwrap();
    store.selected_write(|db| receipt(db, "original")).unwrap();
    drop(store);
    let store = open(dir.path(), Opening::Existing);
    // A projection is not an alternate source of truth when it diverges.
    store
        .write(|db| {
            receipt(db, "outside-journal")?;
            Ok(((), None))
        })
        .unwrap();
    assert!(store.install_acceptance(c).is_err());
}

#[test]
fn feed_high_water_and_message_watermark_survive_initial_adoption() {
    let dir = TempDir::new().unwrap();
    let c = config(dir.path());
    let store = fresh(dir.path());
    store.write(|db|{events(db,1,1)?;db.execute_batch("UPDATE sqlite_sequence SET seq=40 WHERE name='work_feed'; INSERT INTO meta VALUES('message_sequence','999')").map_err(storage)?;Ok(((),None))}).unwrap();
    store.install_acceptance(c.clone()).unwrap();
    store.selected_write(|db| events(db, 1, 2)).unwrap();
    let seq = store.read(rows::feed_high_water).unwrap();
    assert_eq!(seq, Some(41));
    let messages: String = store
        .read(|db| {
            db.query_row(
                "SELECT value FROM meta WHERE key='message_sequence'",
                [],
                |r| r.get(0),
            )
            .map_err(storage)
        })
        .unwrap();
    assert_eq!(messages, "999");
    drop(store);
    let store = open(dir.path(), Opening::Existing);
    store.install_acceptance(c).unwrap();
    assert_eq!(cursor(&store), (1, 2));
}

#[test]
fn failed_preparation_rolls_back_and_does_not_consume_any_position() {
    let dir = TempDir::new().unwrap();
    let store = fresh(dir.path());
    store.install_acceptance(config(dir.path())).unwrap();
    assert!(store
        .selected_write::<()>(|db| {
            events(db, 1, 1)?;
            Err(OfficeError::Invalid("rejected before freeze"))
        })
        .is_err());
    assert_eq!(cursor(&store), (0, 0));
    store.selected_write(|db| events(db, 1, 1)).unwrap();
    assert_eq!(cursor(&store), (1, 1));
}

#[test]
fn filesystem_read_is_not_a_write_and_symlink_escape_is_rejected() {
    let dir = TempDir::new().unwrap();
    let c = config(dir.path());
    assert!(durable::read(&c.protected_root, "absent/path", 10).is_err());
    assert!(!c.protected_root.exists());
    std::fs::create_dir(&c.public_root).unwrap();
    std::os::unix::fs::symlink(dir.path(), c.public_root.join("escape")).unwrap();
    assert!(durable::publish(&c.public_root, "escape/data", b"no", true, None).is_err());
    assert!(!dir.path().join("data").exists());
}

#[test]
fn actual_platform_publication_flush_evidence() {
    let root = TempDir::new().unwrap();
    let before = durable::calls();
    durable::publish(
        root.path(),
        "proof/immutable.bin",
        b"publication evidence",
        true,
        None,
    )
    .unwrap();
    let after = durable::calls();
    let calls = (after.0 - before.0, after.1 - before.1, after.2 - before.2);
    assert!(calls.0 >= 3 && calls.2 >= 3);
    #[cfg(target_os = "macos")]
    assert_eq!(calls.0, calls.1);
    eprintln!(
        "PLATFORM_FLUSH_EVIDENCE {}",
        serde_json::json!({"platform":std::env::consts::OS,"file_sync_all_successes":calls.0,"f_fullfsync_successes":calls.1,"directory_sync_all_successes":calls.2,"immutable_install":"hard_link","power_loss_proof":false})
    );
}

#[test]
fn corrupt_blob_missing_predecessor_and_unknown_codec_stop_recovery() {
    for corruption in ["blob", "gap", "codec"] {
        let root = TempDir::new().unwrap();
        let config = config(root.path());
        let store = fresh(root.path());
        store.install_acceptance(config.clone()).unwrap();
        store
            .selected_write(|db| artifact(db, "event.bin", &vec![31; 100_000], true))
            .unwrap();
        store.selected_write(|db| receipt(db, "receipt")).unwrap();
        drop(store);
        let prefix = config
            .protected_root
            .join("offices/office/journals/work-acceptance/journal-one/transactions");
        match corruption {
            "blob" => std::fs::write(
                config.protected_root.join(format!(
                    "blobs/sha256/{}",
                    codec::sha256(&vec![31; 100_000])
                )),
                b"corrupt",
            )
            .unwrap(),
            "gap" => std::fs::remove_file(prefix.join("1.json")).unwrap(),
            _ => {
                let file = prefix.join("2.json");
                let mut json = codec::parse(&std::fs::read(&file).unwrap()).unwrap();
                if let Json::Object(ref mut map) = json {
                    map.insert(
                        "json_codec".into(),
                        Json::Number(NumberToken::new("99").unwrap()),
                    );
                }
                let digest = codec::payload_sha256(&json).unwrap();
                if let Json::Object(ref mut map) = json {
                    map.insert("payload_sha256".into(), Json::String(digest));
                }
                std::fs::write(file, codec::canonical(&json).unwrap()).unwrap();
            }
        }
        let store = open(root.path(), Opening::Existing);
        assert!(store.install_acceptance(config).is_err(), "{corruption}");
    }
}

#[test]
fn immutable_receipts_reject_replace_and_projection_cursor_cannot_be_forged() {
    let root = TempDir::new().unwrap();
    let store = fresh(root.path());
    store.install_acceptance(config(root.path())).unwrap();
    store.selected_write(|db| receipt(db, "immutable")).unwrap();
    assert!(store.selected_write(|db|{
        db.execute("INSERT OR REPLACE INTO selected_receipts VALUES('org','immutable','instance','other-account','owner',NULL,'task.set_assignment',1,'changed',X'7B7D',9000)",[]).map_err(storage)?;Ok(())
    }).is_err());
    assert!(store
        .write(|db| {
            db.execute("UPDATE selected_journal_cursor SET ordinal=9", [])
                .map_err(storage)?;
            Ok(((), None))
        })
        .is_err());
    assert_eq!(cursor(&store), (1, 0));
}

#[test]
fn journal_and_origin_arithmetic_never_wrap_or_restart_incarnations() {
    let identity = config(TempDir::new().unwrap().path()).identity;
    let entry = origin::OriginEntry {
        origin: origin::LocalOrigin {
            office_id: "office".into(),
            incarnation: "incarnation".into(),
        },
        seq: i64::MAX as u64,
        feed_seq: 1,
        target: origin::OriginRef {
            target_id: "task".into(),
            generation: 1,
            version: 2,
        },
        org_position: org_proto::Position::default(),
        committed_at_ms: 1,
    };
    validate_origins(
        &identity,
        "incarnation",
        i64::MAX - 1,
        i64::MAX,
        std::slice::from_ref(&entry),
    )
    .unwrap();
    assert!(validate_origins(
        &identity,
        "incarnation",
        i64::MAX - 1,
        i64::MAX,
        &[entry.clone(), entry]
    )
    .is_err());
    validate_origins(&identity, "incarnation", i64::MAX, i64::MAX, &[]).unwrap();
    let root = TempDir::new().unwrap();
    let store = fresh(root.path());
    store.install_acceptance(config(root.path())).unwrap();
    store
        .control(|state| {
            let journal = state.acceptance.as_mut().unwrap();
            journal.ordinal = i64::MAX;
            let (mut job, _) = Store::job_with_batch_abort::<()>(
                |_| panic!("exhausted ordinal must precede decision"),
                |_| false,
            );
            assert!(prepare_and_accept(&state.db, journal, &mut job).is_err());
            assert_eq!(journal.ordinal, i64::MAX);
            Ok(())
        })
        .unwrap();
}

#[test]
fn rebuild_sql_without_rewinding_newer_files_and_repair_verified_old_files() {
    let root = TempDir::new().unwrap();
    let c = config(root.path());
    let store = fresh(root.path());
    store.install_acceptance(c.clone()).unwrap();
    store
        .selected_write(|db| {
            events(db, 1, 1)?;
            artifact(db, "task.md", b"revision one", false)
        })
        .unwrap();
    store
        .selected_write(|db| {
            events(db, 1, 2)?;
            db.execute(
                "UPDATE selected_artifacts SET content=?1 WHERE path='task.md'",
                [b"revision two".as_slice()],
            )
            .map_err(storage)?;
            receipt(db, "accepted-two")
        })
        .unwrap();
    drop(store);
    std::fs::remove_file(root.path().join("office.db")).unwrap();
    let store = fresh(root.path());
    store.install_acceptance(c.clone()).unwrap();
    assert_eq!(cursor(&store), (2, 2));
    assert_eq!(
        std::fs::read(c.public_root.join("task.md")).unwrap(),
        b"revision two"
    );
    drop(store);
    std::fs::write(c.public_root.join("task.md"), b"revision one").unwrap();
    let store = open(root.path(), Opening::Existing);
    store.install_acceptance(c.clone()).unwrap();
    assert_eq!(
        std::fs::read(c.public_root.join("task.md")).unwrap(),
        b"revision two"
    );
    assert_eq!(cursor(&store), (2, 2));
}

#[test]
fn failed_startup_never_falls_back_to_an_unjournaled_legacy_writer() {
    let root = TempDir::new().unwrap();
    let c = config(root.path());
    let store = fresh(root.path());
    store.install_acceptance(c.clone()).unwrap();
    drop(store);
    std::fs::write(
        c.protected_root
            .join("offices/office/journals/work-acceptance/journal-one/anchor.json"),
        b"corrupt source",
    )
    .unwrap();
    let store = open(root.path(), Opening::Existing);
    assert!(store.install_acceptance(c).is_err());
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let (job, reply) = Store::job_with_batch_abort(
        move |db| {
            observed.fetch_add(1, Ordering::SeqCst);
            events(db, 1, 1)?;
            Ok(((), None))
        },
        |_| false,
    );
    store.send_work_job(job).unwrap();
    assert!(reply.recv().unwrap().is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(store
        .write(|db| {
            receipt(db, "generic-bypass")?;
            Ok(((), None))
        })
        .is_err());
}

#[test]
fn exhausted_run_job_preserves_coherent_original_receipt_read() {
    let root = TempDir::new().unwrap();
    let store = fresh(root.path());
    store.install_acceptance(config(root.path())).unwrap();
    store.selected_write(|db| receipt(db, "original")).unwrap();
    struct Owner;
    impl crate::selected_tasks::OwnerAuthority for Owner {
        fn verify_live(
            &self,
            _: &AcceptanceIdentity,
        ) -> Result<crate::selected_tasks::OwnerAccount, OfficeError> {
            Ok(crate::selected_tasks::OwnerAccount {
                account_id: "account".into(),
                member_id: "member".into(),
            })
        }
    }
    let authority: Arc<dyn crate::selected_tasks::OwnerAuthority> = Arc::new(Owner);
    crate::selected_tasks::directory::bind_owner(&store, Arc::clone(&authority)).unwrap();
    let before = cursor(&store);
    crate::selected_tasks::directory::bind_owner(&store, Arc::clone(&authority)).unwrap();
    assert_eq!(
        cursor(&store),
        before,
        "healthy rebind is not a new acceptance"
    );
    // Inject a verified exhausted prefix position, including its SQL cursor.
    store
        .control(|state| {
            let journal = state.acceptance.as_mut().unwrap();
            rows::mode(&state.db, 2)?;
            state
                .db
                .execute("UPDATE selected_journal_cursor SET ordinal=?1", [i64::MAX])
                .map_err(storage)?;
            rows::mode(&state.db, 0)?;
            journal.ordinal = i64::MAX;
            Ok(())
        })
        .unwrap();
    assert!(store
        .selected_transaction::<()>(|_, _| panic!("must refuse before decision"))
        .is_err());
    // Exercise the actual startup owner binding used by the worker adapter,
    // not just selected_read directly after run_job refuses allocation.
    crate::selected_tasks::directory::bind_owner(&store, authority).unwrap();
    let result: Vec<u8> = store
        .selected_read(|db, _| {
            db.query_row(
                "SELECT result FROM selected_receipts WHERE operation_id='original'",
                [],
                |r| r.get(0),
            )
            .map_err(storage)
        })
        .unwrap();
    assert_eq!(result, b"{}");
    assert_eq!(cursor(&store), (i64::MAX, 0));
}

#[test]
fn duplicate_preexisting_feed_allocator_is_rejected_before_anchor() {
    let root = TempDir::new().unwrap();
    let c = config(root.path());
    let store = fresh(root.path());
    store
        .write(|db| {
            db.execute_batch(
                "INSERT INTO sqlite_sequence(name,seq) VALUES('work_feed',40),('work_feed',41)",
            )
            .map_err(storage)?;
            Ok(((), None))
        })
        .unwrap();
    assert!(store.install_acceptance(c.clone()).is_err());
    assert!(!c
        .protected_root
        .join("offices/office/journals/work-acceptance/journal-one/anchor.json")
        .exists());
}

#[test]
fn empty_feed_preserves_retained_high_water_through_rebuild() {
    let root = TempDir::new().unwrap();
    let c = config(root.path());
    let store = fresh(root.path());
    store
        .write(|db| {
            db.execute(
                "INSERT INTO sqlite_sequence(name,seq) VALUES('work_feed',87)",
                [],
            )
            .map_err(storage)?;
            Ok(((), None))
        })
        .unwrap();
    store.install_acceptance(c.clone()).unwrap();
    drop(store);
    std::fs::remove_file(root.path().join("office.db")).unwrap();
    let store = fresh(root.path());
    store.install_acceptance(c).unwrap();
    store.selected_write(|db| events(db, 1, 1)).unwrap();
    assert_eq!(store.read(rows::feed_high_water).unwrap(), Some(88));
}
