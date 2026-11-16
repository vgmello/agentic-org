//! Real journal/projection SIGKILL recovery, independent of later task services.
//! These process tests do not establish physical power-loss or host authority.
use super::*;
use crate::store::acceptance::crash::{self, Point};
use std::{
    io::{BufRead, BufReader, Read},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

const CHILD: &str = "store::acceptance::tests::crash_reopen::storage_crash_child";
const SNAPSHOT: &str = "teams/accountable/work/tasks/opaque-task.md";
const EVENT: &str = "work-history/tasks/opaque-task/2.json";
const RECEIPT: &str = "offices/office/receipts/crash-original.json";
const ORIGINAL: &[u8] = b"---\nrevision: 1\n---\nOriginal lexical body: 1.000e+02\n";

fn point(n: u8) -> Point {
    match n {
        1 => Point::Prepared,
        2 => Point::BlobFlushed,
        3 => Point::BlobPublished,
        4 => Point::ManifestFlushed,
        5 => Point::ManifestPublished,
        6 => Point::SnapshotPublished,
        7 => Point::EventPublished,
        8 => Point::ReceiptPublished,
        9 => Point::SqlCommitted,
        10 => Point::BeforeReply,
        11 => Point::AnchorInstalled,
        12 => Point::ManifestInstalled,
        13 => Point::RootDirectoryCreated,
        14 => Point::NestedDirectoryCreated,
        _ => panic!("unknown storage crash hook"),
    }
}

fn result_bytes(n: usize) -> Vec<u8> {
    format!(
        "{{\"event_count\":{n},\"padding\":\"{}\"}}",
        "x".repeat(80 * 1024)
    )
    .into_bytes()
}

fn snapshot() -> Vec<u8> {
    format!(
        "---\nrevision: 2\n---\n{}\nLexical: -0.0e+002\n",
        "original body ".repeat(6000)
    )
    .into_bytes()
}

fn commit(store: &Store, n: usize) {
    store.selected_write(move |db| {
        if n != 0 {
            events(db, n, 3)?;
            db.execute("UPDATE selected_artifacts SET content=?1 WHERE namespace='public' AND path=?2", params![snapshot(),SNAPSHOT]).map_err(storage)?;
            artifact(db, EVENT, b"{\"extension\":-0.0e+002}", true)?;
        }
        let result = result_bytes(n);
        db.execute("INSERT INTO selected_receipts VALUES('org','crash-original','instance','account','owner',NULL,'task.set_assignment',1,'original-fingerprint',?1,3000)",[&result]).map_err(storage)?;
        db.execute("INSERT INTO selected_artifacts VALUES('protected',?1,?2,1)",params![RECEIPT,result]).map_err(storage)?;
        Ok(())
    }).unwrap();
}

fn reopen(root: &Path) -> Store {
    let store = open(root, Opening::Existing);
    store.install_acceptance(config(root)).unwrap();
    store
}

#[test]
fn storage_crash_child() {
    let Some(root) = std::env::var_os("ORG_TEST_STORAGE_CRASH_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    assert!(root.is_absolute());
    let selected = std::env::var("ORG_TEST_STORAGE_CRASH_POINT")
        .unwrap()
        .parse()
        .unwrap();
    let n = std::env::var("ORG_TEST_STORAGE_CRASH_EVENTS")
        .unwrap()
        .parse()
        .unwrap();
    if matches!(selected, 11 | 13 | 14) {
        let store = open(&root, Opening::Existing);
        crash::arm(point(selected));
        store.install_acceptance(config(&root)).unwrap();
        panic!("armed startup boundary was never reached");
    }
    let store = reopen(&root);
    crash::arm(point(selected));
    commit(&store, n);
    panic!("armed storage boundary was never reached");
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn kill_at(root: &Path, selected: u8, n: usize) {
    let mut child = Kill(
        Command::new(std::env::current_exe().unwrap())
            .args([CHILD, "--exact", "--nocapture"])
            .env("ORG_TEST_STORAGE_CRASH_ROOT", root)
            .env("ORG_TEST_STORAGE_CRASH_POINT", selected.to_string())
            .env("ORG_TEST_STORAGE_CRASH_EVENTS", n.to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let output = child.0.stdout.take().unwrap();
    let errors = child.0.stderr.take().unwrap();
    let (sent, received) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            if line.unwrap().ends_with("ACCEPTANCE_CRASH_RENDEZVOUS") {
                sent.send(()).unwrap();
                break;
            }
        }
    });
    let stderr = std::thread::spawn(move || {
        let mut text = String::new();
        BufReader::new(errors)
            .take(16 * 1024)
            .read_to_string(&mut text)
            .unwrap();
        text
    });
    let rendezvous = received.recv_timeout(Duration::from_secs(10));
    child.0.kill().unwrap();
    let status = child.0.wait().unwrap();
    reader.join().unwrap();
    let stderr = stderr.join().unwrap();
    assert!(
        rendezvous.is_ok(),
        "events={n} hook={selected}: not reached: {stderr}"
    );
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(status.signal(), Some(libc::SIGKILL));
    assert!(stderr.is_empty(), "events={n} hook={selected}: {stderr}");
}

fn original_receipt(store: &Store) -> Option<Vec<u8>> {
    store.selected_read(|db,_| {
        db.query_row("SELECT result FROM selected_receipts WHERE operation_id='crash-original' AND instance_id='instance' AND organization_id='org' AND account_id='account' AND actor_kind='owner' AND actor_id IS NULL AND method='task.set_assignment' AND command_version=1 AND fingerprint='original-fingerprint' AND expires_at_ms=3000",[],|r|r.get(0))
            .optional().map_err(storage)
    }).unwrap()
}

fn verify_frozen(store: &Store, c: &AcceptanceConfig, n: usize) {
    assert_eq!(cursor(store), (2, 2 + n as i64));
    assert_eq!(original_receipt(store), Some(result_bytes(n)));
    assert_eq!(
        std::fs::read(c.protected_root.join(RECEIPT)).unwrap(),
        result_bytes(n)
    );
    let bytes = std::fs::read(c.public_root.join(SNAPSHOT)).unwrap();
    assert_eq!(
        bytes,
        if n == 0 {
            ORIGINAL.to_vec()
        } else {
            snapshot()
        }
    );
    assert_eq!(c.public_root.join(EVENT).exists(), n != 0);
    let entries = store
        .selected_read(|db, _| {
            let incarnation = origin::stored_incarnation(db)?.unwrap();
            origin::entries_after(db, &incarnation, 2, 1024)
        })
        .unwrap();
    assert_eq!(entries.len(), n);
    assert_eq!(
        entries.iter().map(|e| e.seq).collect::<Vec<_>>(),
        (3..3 + n as u64).collect::<Vec<_>>()
    );
    assert_eq!(
        entries.iter().map(|e| e.target.version).collect::<Vec<_>>(),
        (3..3 + n as u64).collect::<Vec<_>>()
    );
}

#[test]
fn sigkill_reopen_frozen_storage_at_all_ten_hooks_with_zero_one_multi_origin_mappings() {
    for n in [0, 1, 3] {
        for selected in 1..=10 {
            // Receipt-only commits publish no snapshot/event: those hooks cannot occur.
            if n == 0 && matches!(selected, 6 | 7) {
                continue;
            }
            let root = TempDir::new().unwrap();
            let c = config(root.path());
            let store = fresh(root.path());
            store
                .write(|db| {
                    events(db, 2, 1)?;
                    Ok(((), None))
                })
                .unwrap();
            store.install_acceptance(c.clone()).unwrap();
            store
                .selected_write(|db| artifact(db, SNAPSHOT, ORIGINAL, false))
                .unwrap();
            assert_eq!(cursor(&store), (1, 2));
            drop(store);
            kill_at(root.path(), selected, n);
            let store = reopen(root.path());
            if selected <= 4 {
                assert_eq!(cursor(&store), (1, 2));
                assert!(original_receipt(&store).is_none());
                assert_eq!(
                    std::fs::read(c.public_root.join(SNAPSHOT)).unwrap(),
                    ORIGINAL
                );
                assert!(!c.public_root.join(EVENT).exists());
                assert!(!c.protected_root.join(RECEIPT).exists());
                // The operation was never published. A fresh decision may now commit it.
                commit(&store, n);
            }
            verify_frozen(&store, &c, n);
            let frozen =
                std::fs::read(c.protected_root.join(
                    "offices/office/journals/work-acceptance/journal-one/transactions/2.json",
                ))
                .unwrap();
            let envelope = codec::parse(&frozen).unwrap();
            assert!(matches!(envelope.get("origin_entries"),Some(Json::Array(a)) if a.len()==n));
            // An exact original lookup is stable after an unrelated later acceptance;
            // no callback or event allocation is involved in returning this receipt.
            store
                .selected_write(|db| receipt(db, "later-transaction"))
                .unwrap();
            assert_eq!(cursor(&store), (3, 2 + n as i64));
            assert_eq!(original_receipt(&store), Some(result_bytes(n)));
            drop(store);
            let store = reopen(root.path());
            assert_eq!(cursor(&store), (3, 2 + n as i64));
            assert_eq!(original_receipt(&store), Some(result_bytes(n)));
            assert_eq!(
                std::fs::read(c.protected_root.join(
                    "offices/office/journals/work-acceptance/journal-one/transactions/2.json"
                ))
                .unwrap(),
                frozen
            );
        }
    }
}

#[test]
fn sigkill_after_anchor_hardlink_before_parent_flush_recovers_identical_source() {
    let root = TempDir::new().unwrap();
    let c = config(root.path());
    let store = fresh(root.path());
    store
        .write(|db| {
            events(db, 2, 1)?;
            Ok(((), None))
        })
        .unwrap();
    drop(store);
    kill_at(root.path(), 11, 0);
    let path = c
        .protected_root
        .join("offices/office/journals/work-acceptance/journal-one/anchor.json");
    let bytes = std::fs::read(&path).unwrap();
    let store = reopen(root.path());
    assert_eq!(cursor(&store), (0, 2));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    store
        .selected_write(|db| receipt(db, "after-anchor-recovery"))
        .unwrap();
    drop(store);
    let store = reopen(root.path());
    assert_eq!(cursor(&store), (1, 2));
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
fn sigkill_after_manifest_hardlink_before_parent_flush_recovers_zero_one_multi_origins() {
    for n in [0, 1, 3] {
        let root = TempDir::new().unwrap();
        let c = config(root.path());
        let store = fresh(root.path());
        store
            .write(|db| {
                events(db, 2, 1)?;
                Ok(((), None))
            })
            .unwrap();
        store.install_acceptance(c.clone()).unwrap();
        store
            .selected_write(|db| artifact(db, SNAPSHOT, ORIGINAL, false))
            .unwrap();
        drop(store);
        kill_at(root.path(), 12, n);
        let path = c
            .protected_root
            .join("offices/office/journals/work-acceptance/journal-one/transactions/2.json");
        let bytes = std::fs::read(&path).unwrap();
        let store = reopen(root.path());
        verify_frozen(&store, &c, n);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        drop(store);
        let store = reopen(root.path());
        verify_frozen(&store, &c, n);
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
}

#[test]
fn sigkill_after_mkdir_before_parent_flush_reestablishes_root_and_nested_links() {
    for selected in [13, 14] {
        let root = TempDir::new().unwrap();
        let c = config(root.path());
        drop(fresh(root.path()));
        kill_at(root.path(), selected, 0);
        assert!(c.protected_root.is_dir());
        assert_eq!(c.protected_root.join("offices").exists(), selected == 14);
        assert!(!c
            .protected_root
            .join("offices/office/journals/work-acceptance/journal-one/anchor.json")
            .exists());

        let store = open(root.path(), Opening::Existing);
        store
            .control(|_| {
                durable::observe_directory_syncs(None);
                Ok(())
            })
            .unwrap();
        store.install_acceptance(c.clone()).unwrap();
        let syncs = store
            .control(|_| Ok(durable::take_directory_syncs()))
            .unwrap();
        assert_eq!(syncs.first().unwrap(), root.path());
        if selected == 14 {
            assert!(syncs.contains(&c.protected_root));
        }
        assert_eq!(cursor(&store), (0, 0));
        store
            .selected_write(|db| receipt(db, "after-directory-recovery"))
            .unwrap();
        drop(store);
        let store = reopen(root.path());
        assert_eq!(cursor(&store), (1, 0));
        assert_eq!(store.selected_read(|db, _| {
            db.query_row("SELECT result FROM selected_receipts WHERE operation_id='after-directory-recovery'", [], |r| r.get::<_, Vec<u8>>(0)).map_err(storage)
        }).unwrap(), b"{}");
    }
}
