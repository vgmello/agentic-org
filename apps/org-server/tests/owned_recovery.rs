//! B1-12 end to end: a real marked child, recorded in an office the way a session spawner
//! records it, is stopped by org-supervise's native recovery reading the office's rows.
//! Its own test binary: org-supervise needs core dumps off before the harness starts, and the
//! native scan should not share a process with unrelated tests.

#[path = "../../../crates/org-supervise/tests/support/core_limit.rs"]
mod core_limit;

use std::cell::Cell;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::time::{Duration, Instant};

use org_office::{Key, NewOffice, Office, OfficeRole, OwnedProcessKey};
use org_server::owned_recovery::{record_leader, record_mark, OfficeRecoveryRecords};
use org_supervise::recovery::{recover_record, NativeRecovery};
use org_supervise::{
    Environment, OwnedChild, PreparedSpawn, SearchPath, SessionMark, SpawnSpec, Spawned,
};

const HOLD: &str = "ORG_B112_HOLD";

/// The child: this test binary run again with `HOLD` set, so the same program works on every
/// platform (a system binary may hide its environment from the mark scan). It says it is
/// ready, then waits to be stopped. Without `HOLD` it is an empty test.
#[test]
fn hold() {
    if std::env::var_os(HOLD).is_some() {
        println!("READY");
        std::thread::sleep(Duration::from_secs(30));
    }
}

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
    let key = Key::from_db_key(org_proto::DbKey::from_bytes([5; 32]));
    (dir, Office::create(folder, &new, key).unwrap())
}

fn unique_mark(what: &str) -> String {
    let mut bytes = [0; 16];
    File::open("/dev/urandom")
        .unwrap()
        .read_exact(&mut bytes)
        .unwrap();
    format!("b112-{what}-{:032x}", u128::from_ne_bytes(bytes))
}

/// Spawns the holding child with `mark`; with an office, records it the way a session spawner
/// does: the row before the spawn, the leader before the first byte. Returns once it is ready.
fn spawn(
    root: &Path,
    mark: &str,
    office: Option<&Office>,
) -> (OwnedChild, Option<OwnedProcessKey>) {
    let search = SearchPath::new(
        Path::new("/usr/bin"),
        "/usr/bin:/bin".as_ref(),
        &[root.to_owned()],
    )
    .unwrap();
    let spec = SpawnSpec {
        program: std::env::current_exe().unwrap(),
        args: vec!["--exact".into(), "hold".into(), "--nocapture".into()],
        cwd: root.to_owned(),
        environment: Environment::new(search, &[])
            .unwrap()
            .with_context(&[(HOLD.into(), "1".into())])
            .unwrap(),
    };
    let key = Cell::new(None);
    let Spawned { child, stdout, .. } =
        PreparedSpawn::new(spec, SessionMark::new(mark.to_owned()).unwrap())
            .unwrap()
            .record_mark(|mark| {
                if let Some(office) = office {
                    key.set(Some(record_mark(office, mark)?));
                }
                Ok(())
            })
            .unwrap()
            .spawn()
            .unwrap()
            .record_identity(|leader| match (office, key.get()) {
                (Some(office), Some(key)) => record_leader(office, key, leader),
                _ => Ok(()),
            })
            .unwrap();
    let mut lines = BufReader::new(stdout).lines();
    let ready = lines.find(|line| line.as_ref().is_ok_and(|l| l.trim() == "READY"));
    assert!(ready.is_some(), "the child never got ready");
    std::thread::spawn(move || lines.for_each(drop));
    (child, key.get())
}

#[test]
fn recovery_stops_a_recorded_child_through_the_office_rows_and_leaves_others_alone() {
    let (dir, office) = made();
    let mark = unique_mark("marked");
    let (mut marked, key) = spawn(dir.path(), &mark, Some(&office));
    let (mut unrelated, _) = spawn(dir.path(), &unique_mark("unrelated"), None);
    let key = key.unwrap();
    let mut native = NativeRecovery::new().unwrap();
    let mut records = OfficeRecoveryRecords(&office);
    let gone = recover_record(
        &mut records,
        &key,
        &mut native,
        Instant::now() + Duration::from_secs(10),
    )
    .unwrap();
    assert_eq!(gone.mark(), mark);
    // Recovery checkpointed the leader into the office's row (its write-ahead inventory).
    let saved = office.load_owned_process(key).unwrap().unwrap().inventory;
    assert!(
        saved
            .iter()
            .any(|t| t.identity.pid == marked.identity().pid),
        "{saved:?}"
    );
    assert!(office.retire_owned_process(key).unwrap());
    let deadline = Instant::now() + Duration::from_secs(5);
    while marked.try_exit().unwrap().is_none() {
        assert!(Instant::now() < deadline, "the recorded child must exit");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        unrelated.try_exit().unwrap().is_none(),
        "an unrecorded child is never signalled"
    );
}
