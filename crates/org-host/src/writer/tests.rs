use super::*;
use crate::{accounts::identity::AccountId, hostdb::HostDbInit};
use org_proto::work::WriterIdentity;
use std::os::unix::fs::PermissionsExt;

fn fixture() -> (tempfile::TempDir, Arc<Writers>, Admission) {
    let root = tempfile::tempdir().unwrap();
    let instance_root = root.path().join("instance");
    fs::create_dir(&instance_root).unwrap();
    fs::set_permissions(&instance_root, fs::Permissions::from_mode(0o700)).unwrap();
    let home = instance_root.join("office-state");
    fs::create_dir(&home).unwrap();
    fs::set_permissions(&home, fs::Permissions::from_mode(0o700)).unwrap();
    fs::create_dir_all(home.join("offices/office")).unwrap();
    let keys = root.path().join("keys");
    fs::create_dir(&keys).unwrap();
    fs::set_permissions(&keys, fs::Permissions::from_mode(0o700)).unwrap();
    let instance = crate::ids::generate().unwrap();
    let account = crate::ids::generate().unwrap();
    let custody: Arc<dyn HostSecrets> =
        Arc::new(crate::host_secrets::dev::DevSecrets::open(&keys, &home, &instance).unwrap());
    let password = org_host_control::Password::new("synthetic writer password".into()).unwrap();
    let init = HostDbInit {
        instance_id: instance.clone(),
        clock_high_ms: 1,
    };
    let db = Arc::new(HostDb::create(&home, &*custody, &init).unwrap());
    db.seed_account(
        &account,
        "Owner",
        &crate::service::authentication::hash_password(&password).unwrap(),
        true,
        1,
    )
    .unwrap();
    let admin = db
        .account_authority(&AccountId::parse(&account).unwrap())
        .unwrap()
        .unwrap();
    db.establish_organization_owner(&admin, "org", "01ARZ3NDEKTSV4RRFFQ69G5FAV")
        .unwrap();
    let writers = Writers::open(custody, db, &instance, &instance_root).unwrap();
    let admission = Admission {
        version: wire::VERSION,
        identity: WriterIdentity {
            instance_id: instance,
            organization_id: "org".into(),
            office_id: "office".into(),
            journal_incarnation: String::new(),
            writer_epoch: 0,
            machine_id: String::new(),
        },
        protected_root: instance_root.join("recovery").to_str().unwrap().into(),
        public_root: instance_root.join("public").to_str().unwrap().into(),
        owner_account: account,
        owner_member: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
    };
    (root, writers, admission)
}
fn anchor() -> JournalPosition {
    JournalPosition {
        ordinal: 0,
        sha256: "a".repeat(64),
        previous_sha256: None,
        origin_incarnation: "original-work-origin".into(),
        origin_high_water: 73,
    }
}
fn next(before: &JournalPosition, event_count: i64) -> JournalPosition {
    JournalPosition {
        ordinal: before.ordinal + 1,
        sha256: format!("{:064x}", before.ordinal + 1),
        previous_sha256: Some(before.sha256.clone()),
        origin_incarnation: before.origin_incarnation.clone(),
        origin_high_water: before.origin_high_water + event_count,
    }
}
fn lease(writers: &Arc<Writers>) -> Lease {
    Lease {
        writers: Arc::clone(writers),
        office: "office".into(),
        record: writers.load("office").unwrap().unwrap(),
        live: Arc::new(AtomicBool::new(true)),
    }
}
fn confirm(
    lease: &mut Lease,
    before: Option<JournalPosition>,
    after: JournalPosition,
) -> io::Result<Response> {
    lease.answer(Request::Confirm {
        identity: lease.record.admission.identity.clone(),
        before,
        after,
    })
}

#[test]
fn protected_device_binding_preserves_zero_one_multi_mapping_and_reopen() {
    let (_root, writers, input) = fixture();
    assert!(!writers.required("office").unwrap());
    let admission = writers
        .reserve("01ARZ3NDEKTSV4RRFFQ69G5FAV", input)
        .unwrap();
    assert!(writers.required("office").unwrap());
    assert_eq!(admission.identity.writer_epoch, 1);
    assert!(!admission.identity.machine_id.is_empty());
    let mut control = lease(&writers);
    let a = anchor();
    confirm(&mut control, None, a.clone()).unwrap();
    let one = next(&a, 1);
    confirm(&mut control, Some(a), one.clone()).unwrap();
    let rejected = next(&one, 0);
    confirm(&mut control, Some(one), rejected.clone()).unwrap();
    let multi = next(&rejected, 3);
    confirm(&mut control, Some(rejected), multi.clone()).unwrap();
    assert_eq!((multi.ordinal, multi.origin_high_water), (3, 77));
    drop(control);
    let reopened = Writers::open(
        Arc::clone(&writers.custody),
        Arc::clone(&writers.db),
        &writers.instance,
        &writers.root.path,
    )
    .unwrap();
    let mut control = lease(&reopened);
    assert!(
        matches!(control.answer(Request::Checkpoint {identity:admission.identity}).unwrap(),Response::Checkpoint {position:Some(p)} if p==multi)
    );
    assert!(confirm(&mut control, Some(anchor()), next(&multi, 1)).is_err());
    let max = JournalPosition {
        ordinal: i64::MAX,
        ..multi.clone()
    };
    assert!(!JournalPosition {
        ordinal: 1,
        ..multi
    }
    .follows(&max));
}
#[test]
fn copied_root_missing_custody_replaced_root_and_changed_epoch_never_promote() {
    let (root, writers, input) = fixture();
    let admission = writers
        .reserve("01ARZ3NDEKTSV4RRFFQ69G5FAV", input.clone())
        .unwrap();
    assert!(writers
        .reserve("01ARZ3NDEKTSV4RRFFQ69G5FAW", input)
        .is_err());
    let mut control = lease(&writers);
    let mut wrong = admission.identity.clone();
    wrong.writer_epoch += 1;
    assert!(control
        .answer(Request::Checkpoint { identity: wrong })
        .is_err());
    let record = control.record.clone();
    let device = Name::parse("writer-device").unwrap();
    writers.custody.delete(&device).unwrap();
    assert!(writers.verify(&record, "office").is_err());
    assert!(writers.required("office").unwrap());
    let moved = root.path().join("retained-instance");
    fs::rename(&writers.root.path, &moved).unwrap();
    fs::create_dir(&writers.root.path).unwrap();
    fs::set_permissions(&writers.root.path, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(writers.verify(&record, "office").is_err());
    assert!(!writers
        .root
        .path
        .join("office-state/host-registry.db")
        .exists());
    assert!(moved.join("office-state/host-registry.db").is_file());
}
#[test]
fn missing_registry_cannot_sweep_retained_writer_custody() {
    let (_root, writers, input) = fixture();
    writers
        .reserve("01ARZ3NDEKTSV4RRFFQ69G5FAV", input)
        .unwrap();
    let home = writers.root.path.join("office-state");
    // Actual restoration loss; preserve the complete old DB and every secret.
    fs::rename(home.join("host-registry.db"), home.join("retained.db")).unwrap();
    let before = writers.custody.list().unwrap();
    let init = HostDbInit {
        instance_id: writers.instance.clone(),
        clock_high_ms: 1,
    };
    assert!(matches!(
        HostDb::create(&home, &*writers.custody, &init),
        Err(crate::hostdb::Error::Unsafe)
    ));
    assert_eq!(writers.custody.list().unwrap(), before);
    assert!(home.join("retained.db").exists());
}

#[test]
fn protected_operation_resume_repairs_missing_marker_without_resetting_epoch_or_ack() {
    let (_root, writers, input) = fixture();
    let operation = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    let admission = writers.reserve(operation, input.clone()).unwrap();
    let mut control = lease(&writers);
    let anchor = anchor();
    confirm(&mut control, None, anchor.clone()).unwrap();
    let accepted = next(&anchor, 1);
    confirm(&mut control, Some(anchor), accepted.clone()).unwrap();
    // Interrupted setup's marker publication is resumable only by its protected
    // operation. This is not adoption of an unknown/missing authority record.
    fs::remove_file(writers.marker("office").unwrap()).unwrap();
    let abandoned = writers
        .marker("office")
        .unwrap()
        .with_file_name(".selected-writer-abandoned.tmp");
    fs::write(&abandoned, b"selected").unwrap();
    let resumed = writers.reserve(operation, input.clone()).unwrap();
    assert_eq!(
        serde_json::to_vec(&resumed).unwrap(),
        serde_json::to_vec(&admission).unwrap()
    );
    assert_eq!(
        writers.load("office").unwrap().unwrap().position,
        Some(accepted)
    );
    assert!(writers.marker("office").unwrap().is_file());
    assert_eq!(fs::read(abandoned).unwrap(), b"selected");
    assert_eq!(
        serde_json::to_vec(&writers.reserve(operation, input.clone()).unwrap()).unwrap(),
        serde_json::to_vec(&admission).unwrap()
    );
    let mut changed = input;
    changed.public_root.push_str("-other");
    assert!(writers.reserve(operation, changed).is_err());
}

fn retained_files(path: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    let mut files = std::collections::BTreeMap::new();
    for entry in fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(retained_files(&path));
        } else {
            files.insert(path.clone(), fs::read(path).unwrap());
        }
    }
    files
}

#[test]
fn missing_committed_writer_custody_never_reallocates_on_original_operation_retry() {
    for retained in ["marker", "office", "public", "journal"] {
        for lose_device in [false, true] {
            let (root, writers, input) = fixture();
            let operation = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
            let admission = writers.reserve(operation, input.clone()).unwrap();
            let mut control = lease(&writers);
            let initial = anchor();
            confirm(&mut control, None, initial.clone()).unwrap();
            confirm(&mut control, Some(initial.clone()), next(&initial, 3)).unwrap();
            drop(control);
            let credential = name("office").unwrap();
            let original = writers.custody.load(&credential).unwrap().unwrap();
            if retained != "marker" {
                fs::remove_file(writers.marker("office").unwrap()).unwrap();
                let directory = match retained {
                    "office" => writers
                        .marker("office")
                        .unwrap()
                        .parent()
                        .unwrap()
                        .to_owned(),
                    "public" => PathBuf::from(&admission.public_root),
                    "journal" => PathBuf::from(&admission.protected_root),
                    _ => unreachable!(),
                };
                fs::create_dir_all(&directory).unwrap();
                fs::write(directory.join("retained-committed-state"), b"never replace").unwrap();
            }
            writers.custody.delete(&credential).unwrap();
            if lose_device {
                writers
                    .custody
                    .delete(&Name::parse("writer-device").unwrap())
                    .unwrap();
            }
            let before = retained_files(root.path());
            assert!(
                writers.reserve(operation, input).is_err(),
                "{retained}/{lose_device}"
            );
            assert!(writers.custody.load(&credential).unwrap().is_none());
            assert_eq!(retained_files(root.path()), before);
            // Retained original custody is evidence, never an automatically restored item.
            let original: Record = serde_json::from_slice(original.expose()).unwrap();
            assert_eq!(original.admission.identity, admission.identity);
            assert_eq!(original.position, Some(next(&initial, 3)));
        }
    }
}

#[test]
fn missing_device_cannot_be_recreated_while_another_writer_record_survives() {
    let (root, writers, mut input) = fixture();
    writers
        .reserve("01ARZ3NDEKTSV4RRFFQ69G5FAV", input.clone())
        .unwrap();
    writers
        .custody
        .delete(&Name::parse("writer-device").unwrap())
        .unwrap();
    input.identity.office_id = "second".into();
    fs::create_dir(writers.marker("second").unwrap().parent().unwrap()).unwrap();
    let before = retained_files(root.path());
    assert!(writers
        .reserve("01ARZ3NDEKTSV4RRFFQ69G5FAW", input)
        .is_err());
    assert_eq!(retained_files(root.path()), before);
}

#[test]
fn marker_replay_never_clobbers_malformed_or_unsafe_existing_entries() {
    use std::os::unix::fs::symlink;
    for kind in [
        "empty",
        "partial",
        "wrong",
        "mode",
        "symlink",
        "directory",
        "hardlink",
    ] {
        let (_root, writers, input) = fixture();
        let operation = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        writers.reserve(operation, input.clone()).unwrap();
        let credential = name("office").unwrap();
        let original = writers.custody.load(&credential).unwrap().unwrap();
        let path = writers.marker("office").unwrap();
        fs::remove_file(&path).unwrap();
        match kind {
            "directory" => fs::create_dir(&path).unwrap(),
            "symlink" | "hardlink" => {
                let target = path.with_extension("retained");
                fs::write(&target, b"selected-writer/1\n").unwrap();
                fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
                if kind == "symlink" {
                    symlink(target, &path).unwrap();
                } else {
                    fs::hard_link(target, &path).unwrap();
                }
            }
            _ => {
                let bytes: &[u8] = match kind {
                    "empty" => b"",
                    "partial" => b"selected",
                    "wrong" => b"selected-writer/2\n",
                    _ => b"selected-writer/1\n",
                };
                fs::write(&path, bytes).unwrap();
                fs::set_permissions(
                    &path,
                    fs::Permissions::from_mode(if kind == "mode" { 0o644 } else { 0o600 }),
                )
                .unwrap();
            }
        }
        let before = fs::symlink_metadata(&path).unwrap();
        let bytes = if before.is_file() {
            Some(fs::read(&path).unwrap())
        } else {
            None
        };
        assert!(writers.reserve(operation, input).is_err(), "{kind}");
        let after = fs::symlink_metadata(&path).unwrap();
        assert_eq!(
            (before.ino(), before.mode(), before.len(), before.nlink()),
            (after.ino(), after.mode(), after.len(), after.nlink())
        );
        if let Some(bytes) = bytes {
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        assert_eq!(
            writers.custody.load(&credential).unwrap().unwrap().expose(),
            original.expose()
        );
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct MarkerChild {
    root: PathBuf,
    input: Admission,
    limit: Option<u64>,
}

fn reopen_marker_writer(root: &Path, input: &Admission) -> Arc<Writers> {
    let instance_root = root.join("instance");
    let home = instance_root.join("office-state");
    let custody = Arc::new(
        crate::host_secrets::dev::DevSecrets::open(
            &root.join("keys"),
            &home,
            &input.identity.instance_id,
        )
        .unwrap(),
    );
    let db = Arc::new(HostDb::open(&home, &*custody).unwrap());
    Writers::open(custody, db, &input.identity.instance_id, &instance_root).unwrap()
}

extern "C" fn stop_marker_child(_: libc::c_int) {
    // SAFETY: kill/getpid are async-signal-safe; stop only this disposable child
    // when the OS reports the production write's file-size limit.
    unsafe {
        libc::kill(libc::getpid(), libc::SIGSTOP);
    }
}

#[test]
fn marker_interruption_child() {
    use std::io::Read;
    if std::env::var_os("ORG_TEST_WRITER_MARKER_CHILD").is_none() {
        return;
    }
    let input: MarkerChild = serde_json::from_reader(std::io::stdin().take(8192)).unwrap();
    let writers = reopen_marker_writer(&input.root, &input.input);
    if let Some(limit) = input.limit {
        let limit = libc::rlimit {
            rlim_cur: limit,
            rlim_max: limit,
        };
        // SAFETY: this disposable child alone refuses regular-file writes beyond
        // the chosen offset; pipe output remains available to the parent.
        unsafe {
            libc::signal(libc::SIGXFSZ, stop_marker_child as *const () as usize);
            assert_eq!(libc::setrlimit(libc::RLIMIT_FSIZE, &limit), 0);
        }
        println!("MARKER_BOUNDARY_REACHED");
        std::io::stdout().flush().unwrap();
    }
    let result = writers.reserve("01ARZ3NDEKTSV4RRFFQ69G5FAV", input.input);
    assert_eq!(result.is_err(), input.limit.is_some());
    println!("MARKER_BOUNDARY_REACHED");
    std::io::stdout().flush().unwrap();
    loop {
        std::thread::park();
    }
}

#[test]
fn marker_write_failure_and_sigkill_resume_original_checkpoint_without_partial_publication() {
    use std::{
        io::{BufRead, BufReader},
        process::{Child, Command, Stdio},
        sync::mpsc,
    };
    struct Kill(Child);
    impl Drop for Kill {
        fn drop(&mut self) {
            self.0.kill().ok();
            self.0.wait().ok();
        }
    }
    for limit in [Some(0), Some(7), None] {
        let (root, writers, input) = fixture();
        let operation = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        let admission = writers.reserve(operation, input.clone()).unwrap();
        let mut control = lease(&writers);
        let initial = anchor();
        confirm(&mut control, None, initial.clone()).unwrap();
        confirm(&mut control, Some(initial.clone()), next(&initial, 2)).unwrap();
        drop(control);
        let credential = name("office").unwrap();
        let original = writers.custody.load(&credential).unwrap().unwrap();
        let marker = writers.marker("office").unwrap();
        fs::remove_file(&marker).unwrap();
        drop(writers); // The disposable child owns the actual registry lock until SIGKILL.
        let mut child = Kill(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "writer::tests::marker_interruption_child",
                    "--nocapture",
                ])
                .env("ORG_TEST_WRITER_MARKER_CHILD", "1")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        );
        let bytes = serde_json::to_vec(&MarkerChild {
            root: root.path().into(),
            input: input.clone(),
            limit,
        })
        .unwrap();
        child.0.stdin.take().unwrap().write_all(&bytes).unwrap();
        let output = child.0.stdout.take().unwrap();
        let (sent, received) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                if line.unwrap().contains("MARKER_BOUNDARY_REACHED") {
                    sent.send(()).unwrap();
                    break;
                }
            }
        });
        received.recv_timeout(Duration::from_secs(10)).unwrap();
        if limit.is_some() {
            await_marker_write_stop(child.0.id());
        }
        child.0.kill().unwrap();
        assert!(!child.0.wait().unwrap().success());
        reader.join().unwrap();
        if let Some(maximum) = limit {
            assert!(
                !marker.exists(),
                "partial final marker after write limit {limit:?}"
            );
            let stages: Vec<_> = fs::read_dir(marker.parent().unwrap())
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| {
                    path.file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with(".selected-writer-")
                })
                .collect();
            // Darwin may deliver SIGXFSZ after the failed write has returned
            // and staging cleanup has run. Any surviving private stage remains
            // bounded, while the final marker must always be absent.
            for stage in stages {
                assert!(fs::metadata(stage).unwrap().len() <= maximum);
            }
        } else {
            assert_eq!(fs::read(&marker).unwrap(), b"selected-writer/1\n");
        }
        let writers = reopen_marker_writer(root.path(), &input);
        let resumed = writers.reserve(operation, input).unwrap();
        assert_eq!(
            serde_json::to_vec(&resumed).unwrap(),
            serde_json::to_vec(&admission).unwrap()
        );
        assert_eq!(
            writers.custody.load(&credential).unwrap().unwrap().expose(),
            original.expose()
        );
        assert_eq!(
            writers.load("office").unwrap().unwrap().position,
            Some(next(&initial, 2))
        );
        assert_eq!(fs::read(marker).unwrap(), b"selected-writer/1\n");
    }
}

fn await_marker_write_stop(pid: u32) {
    let until = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let mut status = 0;
        // SAFETY: poll only the owned test child's stop state; WUNTRACED does
        // not reap the stopped process that the parent will subsequently kill.
        let result = unsafe {
            libc::waitpid(
                pid as libc::pid_t,
                &mut status,
                libc::WUNTRACED | libc::WNOHANG,
            )
        };
        if result != 0 {
            assert_eq!(result, pid as libc::pid_t);
            assert!(libc::WIFSTOPPED(status));
            assert_eq!(libc::WSTOPSIG(status), libc::SIGSTOP);
            break;
        }
        assert!(
            std::time::Instant::now() < until,
            "child did not stop inside write"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
