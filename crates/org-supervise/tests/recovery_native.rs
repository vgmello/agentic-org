#![cfg(target_os = "linux")]

#[path = "support/core_limit.rs"]
mod core_limit;

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use std::time::{Duration, Instant};

use org_supervise::recovery::{
    recover_record, CheckpointTarget, NativeRecovery, RecoveryCheckpoint, RecoveryRecord,
    RecoveryRecords,
};
use org_supervise::{Environment, PreparedSpawn, SearchPath, SessionMark, SpawnSpec, Spawned};

struct Row(RecoveryRecord);
impl RecoveryRecords for Row {
    type Key = ();
    fn pending(&mut self) -> io::Result<Vec<((), RecoveryRecord)>> {
        unreachable!("live recovery loads only its keyed row")
    }
    fn load(&mut self, _: &(), _: Instant) -> io::Result<Option<RecoveryRecord>> {
        Ok(Some(self.0.clone()))
    }
}
impl RecoveryCheckpoint for Row {
    fn checkpoint(
        &mut self,
        _: &(),
        mark: &str,
        inventory: &[CheckpointTarget],
        _: Instant,
    ) -> io::Result<()> {
        assert_eq!(self.0.mark(), mark);
        assert!(self
            .0
            .inventory()
            .iter()
            .all(|saved| inventory.contains(saved)));
        self.0 = self.0.clone().with_inventory(inventory.to_vec())?;
        Ok(())
    }
}

fn sleep_fixture(root: &Path) -> Spawned {
    let mut bytes = [0; 16];
    File::open("/dev/urandom")
        .unwrap()
        .read_exact(&mut bytes)
        .unwrap();
    let mark = SessionMark::new(format!(
        "native-recovery-{:032x}",
        u128::from_ne_bytes(bytes)
    ))
    .unwrap();
    let search = SearchPath::new(
        Path::new("/usr/bin"),
        "/usr/bin:/bin".as_ref(),
        &[root.to_owned()],
    )
    .unwrap();
    PreparedSpawn::new(
        SpawnSpec {
            program: "/bin/sleep".into(),
            args: vec!["30".into()],
            cwd: root.to_owned(),
            environment: Environment::new(search, &[]).unwrap(),
        },
        mark,
    )
    .unwrap()
    .record_mark(|_| Ok(()))
    .unwrap()
    .spawn()
    .unwrap()
    .record_identity(|_| Ok(()))
    .unwrap()
}

#[test]
fn native_recovery_signals_the_marked_identity_without_reaping_it() {
    let root = tempfile::tempdir().unwrap();
    let mut marked = sleep_fixture(root.path());
    let mut unrelated = sleep_fixture(root.path());
    let identity = marked.child.identity().clone();
    let mut row =
        Row(RecoveryRecord::new(identity.mark.clone(), Some(identity.clone()), vec![]).unwrap());
    let mut native = NativeRecovery::new().unwrap();
    let gone = recover_record(
        &mut row,
        &(),
        &mut native,
        Instant::now() + Duration::from_secs(10),
    )
    .unwrap();
    assert_eq!(gone.mark(), identity.mark);
    assert!(row
        .0
        .inventory()
        .iter()
        .any(|target| target.identity.pid == identity.pid));
    assert!(
        marked.child.try_exit().unwrap().is_some(),
        "the owned process must still be waitable"
    );
    assert!(unrelated.child.try_exit().unwrap().is_none());
}
