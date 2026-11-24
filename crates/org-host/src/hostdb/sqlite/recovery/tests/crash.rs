//! Real process death and plain-SQLite/developer-custody reopen. Generated fixture
//! secrets only; neither an OS confirmation nor a release Keychain claim.
use super::*;
use crate::host_secrets::dev::DevSecrets;
use std::{
    cell::Cell,
    io::{BufRead, BufReader, Read, Write},
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

thread_local! {static STOP: Cell<u8> = const { Cell::new(0) };}
const CHILD: &str = "hostdb::sqlite::recovery::tests::crash::recovery_crash_child";
pub(in crate::hostdb::sqlite::recovery) fn checkpoint(step: u8) {
    if STOP.with(Cell::get) == step {
        println!("RECOVERY_CRASH_RENDEZVOUS");
        std::io::stdout().flush().unwrap();
        loop {
            std::thread::park();
        }
    }
}
struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn custody(base: &Path, home: &Path, instance: &str) -> DevSecrets {
    DevSecrets::open(base, home, instance).unwrap()
}
#[test]
fn recovery_crash_child() {
    let Some(home) = std::env::var_os("ORG_TEST_RECOVERY_CRASH_HOME") else {
        return;
    };
    let home = std::path::PathBuf::from(home);
    let base = std::path::PathBuf::from(std::env::var_os("ORG_TEST_RECOVERY_CRASH_BASE").unwrap());
    assert!(home.is_absolute() && base.is_absolute());
    let instance = std::env::var("ORG_TEST_RECOVERY_CRASH_INSTANCE").unwrap();
    let operation = std::env::var("ORG_TEST_RECOVERY_CRASH_OPERATION").unwrap();
    let step = std::env::var("ORG_TEST_RECOVERY_CRASH_STEP")
        .unwrap()
        .parse::<u8>()
        .unwrap();
    assert!((1..=8).contains(&step));
    let custody = custody(&base, &home, &instance);
    let db = HostDb::open(&home, &custody).unwrap();
    let pending = db.recovery_observed(&operation).unwrap();
    let source = db
        .recovery_source(&pending.binding.organization_id)
        .unwrap();
    STOP.with(|n| n.set(step));
    let shown = db
        .recovery_delivered(&pending, &source.account, false, 100)
        .unwrap();
    db.recovery_ack_intent(&shown, &source.account, 200)
        .unwrap();
    db.recovery_reconcile(&custody).unwrap();
    panic!("armed recovery boundary was never reached");
}
#[test]
fn sigkill_reopens_all_eight_delivery_ack_and_custody_boundaries_without_new_actor_or_time() {
    for step in 1..=8 {
        let Fixture {
            _dir: home,
            db,
            custody: memory,
            r,
            public,
        } = fixture();
        let base = tempfile::tempdir().unwrap();
        std::fs::set_permissions(base.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let stored = custody(base.path(), home.path(), &public.instance_id);
        // Copy only this test's generated custody through its actual provider API.
        // No real user home/keychain or secret command argument is involved.
        for name in memory.list().unwrap() {
            stored
                .put(&name, &memory.load(&name).unwrap().unwrap())
                .unwrap();
        }
        let pending = db.recovery_prepare(&public).unwrap();
        let frozen_clock: i64 = db
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM meta WHERE key='clock_high_ms'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        drop(db);
        drop(memory);
        drop(stored);
        let mut child = Kill(
            Command::new(std::env::current_exe().unwrap())
                .args([CHILD, "--exact", "--nocapture"])
                .env("ORG_TEST_RECOVERY_CRASH_HOME", home.path())
                .env("ORG_TEST_RECOVERY_CRASH_BASE", base.path())
                .env("ORG_TEST_RECOVERY_CRASH_INSTANCE", &public.instance_id)
                .env("ORG_TEST_RECOVERY_CRASH_OPERATION", &r.operation_id)
                .env("ORG_TEST_RECOVERY_CRASH_STEP", step.to_string())
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
                if line.unwrap().ends_with("RECOVERY_CRASH_RENDEZVOUS") {
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
        if received.recv_timeout(Duration::from_secs(10)).is_err() {
            child.0.kill().ok();
            child.0.wait().ok();
            reader.join().unwrap();
            panic!(
                "recovery boundary {step} not reached: {}",
                stderr.join().unwrap()
            );
        }
        child.0.kill().unwrap();
        assert!(!child.0.wait().unwrap().success());
        reader.join().unwrap();
        assert!(stderr.join().unwrap().is_empty());
        let stored = custody(base.path(), home.path(), &public.instance_id);
        let db = HostDb::open(home.path(), &stored).unwrap();
        let observed = db.recovery_prepare(&public).unwrap();
        let name = Name::parse(&r.recovery_handle).unwrap();
        match step {
            1 => assert_eq!(observed, pending),
            2 | 3 => {
                assert_eq!(observed.state, State::Delivered);
                assert_eq!(observed.ordinal, 1);
                assert!(observed.acknowledged_by.is_none());
            }
            4..=7 => assert_eq!(observed.state, State::AckPending),
            8 => assert_eq!(observed.state, State::Acknowledged),
            _ => unreachable!(),
        }
        assert_eq!(stored.load(&name).unwrap().is_some(), step <= 5);
        db.recovery_reconcile(&stored).unwrap();
        let recovered = db.recovery_prepare(&public).unwrap();
        if step <= 3 {
            assert_eq!(
                recovered, observed,
                "restart cannot acknowledge an unconfirmed delivery"
            );
            assert!(crate::provisioning::pending_recovery(&stored, &r).is_ok());
        } else {
            assert_eq!(recovered.state, State::Acknowledged);
            assert_eq!(recovered.ordinal, 1);
            assert_eq!(recovered.redelivery_count, 0);
            assert_eq!(
                recovered.acknowledged_by.as_deref(),
                Some(r.account.as_str())
            );
            assert_eq!(recovered.acknowledged_ms, Some(frozen_clock));
            assert!(stored.load(&name).unwrap().is_none());
            let audit = db.recovery_status(&recovered, 0).unwrap();
            assert_eq!(audit.audit.len(), 3);
            assert_eq!(
                audit
                    .audit
                    .iter()
                    .map(|a| a.action.as_str())
                    .collect::<Vec<_>>(),
                ["show", "ack_pending", "acknowledged"]
            );
            assert!(audit
                .audit
                .iter()
                .all(|a| a.account == r.account && a.at_ms == frozen_clock));
            assert!(db
                .recovery_delivered(&recovered, &r.account, true, i64::MAX)
                .is_err());
        }
        drop(db);
        drop(stored);
        let stored = custody(base.path(), home.path(), &public.instance_id);
        let reopened = HostDb::open(home.path(), &stored).unwrap();
        reopened.recovery_reconcile(&stored).unwrap();
        assert_eq!(reopened.recovery_prepare(&public).unwrap(), recovered);
    }
}
