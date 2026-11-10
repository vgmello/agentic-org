#[path = "support/core_limit.rs"]
mod core_limit;

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use org_supervise::recovery::{
    recover_record, CheckpointTarget, NativeRecovery, ProcessIdentity, RecoveryCheckpoint,
    RecoveryProcesses, RecoveryRecord, RecoveryRecords,
};
use org_supervise::{
    ChildIdentity, Environment, PreparedSpawn, SearchPath, SessionMark, SpawnSpec, Spawned,
};

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

fn unique_mark() -> String {
    let mut bytes = [0; 16];
    File::open("/dev/urandom")
        .unwrap()
        .read_exact(&mut bytes)
        .unwrap();
    format!("native-recovery-{:032x}", u128::from_ne_bytes(bytes))
}

fn fixture(
    root: &Path,
    mark: &str,
    mode: &str,
) -> (org_supervise::OwnedChild, Option<ProcessIdentity>) {
    let search = SearchPath::new(
        Path::new("/usr/bin"),
        "/usr/bin:/bin".as_ref(),
        &[root.to_owned()],
    )
    .unwrap();
    let Spawned { child, stdout, .. } = PreparedSpawn::new(
        SpawnSpec {
            program: std::env::current_exe().unwrap(),
            args: vec![
                "--exact".into(),
                "hold_fixture".into(),
                "--nocapture".into(),
            ],
            cwd: root.to_owned(),
            environment: Environment::new(search, &[])
                .unwrap()
                .with_context(&[("ORG_TEST_CASE".into(), mode.into())])
                .unwrap(),
        },
        SessionMark::new(mark.to_owned()).unwrap(),
    )
    .unwrap()
    .record_mark(|_| Ok(()))
    .unwrap()
    .spawn()
    .unwrap()
    .record_identity(|_| Ok(()))
    .unwrap();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let result = loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => break Err(io::Error::other("fixture exited before readiness")),
                Ok(_) if line.split_ascii_whitespace().next() == Some("RECOVERY_READY") => {
                    break Ok(line)
                }
                Ok(_) => {}
                Err(error) => break Err(error),
            }
        };
        let _ = sender.send(result);
        let _ = io::copy(&mut reader, &mut io::sink());
    });
    let ready = receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("fixture readiness deadline")
        .unwrap();
    let mut fields = ready.split_ascii_whitespace().skip(1);
    let ready = fields.next().map(|pid| ProcessIdentity {
        pid: pid.parse().unwrap(),
        started: fields.next().unwrap().into(),
        boot: fields.next().unwrap().into(),
    });
    (child, ready)
}

#[test]
fn hold_fixture() {
    let Ok(mode) = std::env::var("ORG_TEST_CASE") else {
        return;
    };
    let mark = std::env::var("ORG_SESSION_MARK").unwrap();
    let root = std::env::current_dir().unwrap();
    let _nested = if mode == "delegate" {
        let (nested, _) = fixture(&root, &mark, "hold");
        let id = nested.identity();
        println!("RECOVERY_READY {} {} {}", id.pid, id.started, id.boot);
        Some(nested)
    } else {
        println!("RECOVERY_READY");
        None
    };
    io::stdout().flush().unwrap();
    // Bound the fixture's life even if a test fails after its parent was killed.
    std::thread::sleep(Duration::from_secs(20));
}

#[test]
fn native_recovery_signals_the_recorded_child_without_reaping_it() {
    let root = tempfile::tempdir().unwrap();
    let (mut marked, _) = fixture(root.path(), &unique_mark(), "hold");
    let (mut unrelated, _) = fixture(root.path(), &unique_mark(), "hold");
    let identity = marked.identity().clone();
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
        marked.try_exit().unwrap().is_some(),
        "the owned process must still be waitable"
    );
    assert!(unrelated.try_exit().unwrap().is_none());
}

#[test]
fn native_recovery_refuses_a_reused_non_child_identity_and_scans_detached_tools() {
    let root = tempfile::tempdir().unwrap();
    let mark = unique_mark();
    let (mut owner, tool) = fixture(root.path(), &mark, "delegate");
    let tool = tool.expect("delegate fixture must report its child identity");
    let stale = ChildIdentity {
        pid: tool.pid,
        pgid: tool.pid,
        started: format!("{}-stale", tool.started),
        boot: tool.boot.clone(),
        mark: format!("{mark}-stale"),
    };
    let mut native = NativeRecovery::new().unwrap();
    let mut stale_row = Row(RecoveryRecord::new(stale.mark.clone(), Some(stale), vec![]).unwrap());
    let gone = recover_record(
        &mut stale_row,
        &(),
        &mut native,
        Instant::now() + Duration::from_secs(10),
    )
    .unwrap();
    assert_eq!(gone.report().signals, 0);
    assert!(!native.wait_exited(&tool, Instant::now()).unwrap());
    assert!(owner.try_exit().unwrap().is_none());

    let mut row = Row(RecoveryRecord::new(mark.clone(), None, vec![]).unwrap());
    let gone = recover_record(
        &mut row,
        &(),
        &mut native,
        Instant::now() + Duration::from_secs(10),
    )
    .unwrap();
    assert_eq!(gone.mark(), mark);
    assert!(row.0.inventory().iter().any(|entry| entry.identity == tool));
    assert!(native.wait_exited(&tool, Instant::now()).unwrap());
    assert!(owner.try_exit().unwrap().is_some());
}
