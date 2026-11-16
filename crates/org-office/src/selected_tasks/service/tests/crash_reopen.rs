//! SIGKILL at explicit unit-only publication boundaries, then a fresh process
//! reopens real SQLCipher/journal/canonical files. Not physical power-loss proof
//! or production host/custody/authentication evidence.
use super::*;
use crate::store::acceptance::crash::{self, Point};
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};
const CHILD: &str = "selected_tasks::service::tests::crash_reopen::acceptance_crash_child";
fn config(root: &std::path::Path) -> AcceptanceConfig {
    AcceptanceConfig {
        authority: None,
        identity: AcceptanceIdentity {
            instance_id: "instance".into(),
            organization_id: "org".into(),
            office_id: "office".into(),
            journal_incarnation: "journal".into(),
            writer_epoch: 1,
            machine_id: "machine".into(),
        },
        protected_root: root.join("recovery"),
        public_root: root.join("public"),
    }
}
fn reopen(root: &std::path::Path) -> Store {
    let store = Store::open(
        &root.join("office.db"),
        &Key::from_bytes([31; 32]),
        Opening::Existing,
        &OFFICE_SCHEMA,
    )
    .unwrap();
    store.install_acceptance(config(root)).unwrap();
    store
}
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
        _ => panic!("unknown private crash point"),
    }
}
#[test]
fn acceptance_crash_child() {
    let Some(root) = std::env::var_os("ORG_TEST_ACCEPTANCE_CRASH_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    assert!(root.is_absolute());
    let operation = std::env::var("ORG_TEST_ACCEPTANCE_CRASH_OPERATION").unwrap();
    let selected = std::env::var("ORG_TEST_ACCEPTANCE_CRASH_POINT")
        .unwrap()
        .parse()
        .unwrap();
    let store = reopen(&root);
    crash::arm(point(selected));
    let mut change = request(1, Some("doing-team"), Some("employee"));
    change.operation_id = operation;
    let _ = assign(&store, authority(), change);
    panic!("armed boundary was never reached");
}
struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
fn sigkill_reopen_preserves_frozen_assignment_across_all_ten_publication_boundaries() {
    for selected in 1..=10 {
        let source = format!("{SOURCE}{}\n", "original body content ".repeat(4000));
        let (root, store, config) = fixture_with_source(1, "office", &source);
        let mut change = request(1, Some("doing-team"), Some("employee"));
        let operation = change.operation_id.clone();
        drop(store);
        let mut child = Kill(
            Command::new(std::env::current_exe().unwrap())
                .args([CHILD, "--exact", "--nocapture"])
                .env("ORG_TEST_ACCEPTANCE_CRASH_ROOT", root.path())
                .env("ORG_TEST_ACCEPTANCE_CRASH_OPERATION", &operation)
                .env("ORG_TEST_ACCEPTANCE_CRASH_POINT", selected.to_string())
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
            use std::io::Read;
            let mut text = String::new();
            BufReader::new(errors)
                .take(16 * 1024)
                .read_to_string(&mut text)
                .unwrap();
            text
        });
        if received.recv_timeout(Duration::from_secs(10)).is_err() {
            child.0.kill().ok();
            child.0.wait().ok();
            reader.join().unwrap();
            let stderr = stderr.join().unwrap();
            panic!("boundary {selected} not reached: {stderr}");
        }
        child.0.kill().unwrap();
        assert!(!child.0.wait().unwrap().success());
        reader.join().unwrap();
        let stderr = stderr.join().unwrap();
        assert!(
            stderr.is_empty(),
            "unexpected child error at {selected}: {stderr}"
        );
        let store = reopen(root.path());
        let receipt = lookup(&store, authority(), operation.clone()).unwrap();
        let snapshot = config
            .public_root
            .join("teams/accountable/work/tasks/opaque-task.md");
        let event = config
            .public_root
            .join("work-history/tasks/opaque-task/2.json");
        if selected <= 4 {
            assert!(
                receipt.is_none(),
                "unpublished transaction admitted at {selected}"
            );
            assert_eq!(std::fs::read_to_string(&snapshot).unwrap(), source);
            assert!(!event.exists());
            change.operation_id = operation.clone();
            assert!(matches!(
                assign(&store, authority(), change).unwrap(),
                AssignmentOutcome::Completed { revision: 2, .. }
            ));
        } else {
            let receipt = receipt.expect("published frozen acceptance must replay");
            assert!(matches!(
                receipt,
                AssignmentOutcome::Completed { revision: 2, .. }
            ));
            let bytes = std::fs::read(&event).unwrap();
            history::verify(
                &bytes,
                history::ExpectedIdentity {
                    organization_id: "org",
                    task_id: "opaque-task",
                    filename: "2.json",
                },
                Admission::New,
            )
            .unwrap();
            let after = std::fs::read_to_string(&snapshot).unwrap();
            assert!(after.ends_with(source.split_once("---\nBody").unwrap().1));
            assert!(after.contains("1.234567890123456789E+012"));
            // Later directory/state changes cannot rerun the frozen decision or
            // replace its original receipt with a target/revision rejection.
            super::super::super::directory::admit(
                &store,
                authority(),
                Some(1),
                directory_source(false),
            )
            .unwrap();
            assert!(matches!(
                assign(&store, authority(), request(2, None, None)).unwrap(),
                AssignmentOutcome::Completed { revision: 3, .. }
            ));
            assert_eq!(assign(&store, authority(), change).unwrap(), receipt);
            assert_eq!(
                lookup(&store, authority(), operation).unwrap(),
                Some(receipt)
            );
        }
        drop(store);
        let store = reopen(root.path());
        assert!(std::fs::read_to_string(snapshot)
            .unwrap()
            .contains("lexical: 1.234567890123456789E+012"));
        store
            .selected_read(|db, _| {
                let events: i64 = db
                    .query_row(
                        "SELECT COUNT(*) FROM selected_events WHERE task_id='opaque-task'",
                        [],
                        |r| r.get(0),
                    )
                    .map_err(storage)?;
                assert!((2..=3).contains(&events));
                Ok(())
            })
            .unwrap();
    }
}
