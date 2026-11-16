use super::*;
use crate::{
    accounts::identity::AccountId,
    hostdb::{BootstrapRecord, FirstAccount, HostDbInit},
};
use org_proto::selected_tasks::WriterIdentity;
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
        bootstrap: BootstrapRecord {
            operation_id: crate::ids::generate().unwrap(),
            hq_operation_id: crate::ids::generate().unwrap(),
            fingerprint: "fixture".into(),
            account: account.clone(),
            finished: false,
        },
        first_account: FirstAccount::new(
            &account,
            "Owner",
            crate::service::authentication::hash_password(&password).unwrap(),
        )
        .unwrap(),
        tls_pair: None,
    };
    let db = Arc::new(HostDb::create(&home, &*custody, &init).unwrap());
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
        bootstrap: BootstrapRecord {
            operation_id: crate::ids::generate().unwrap(),
            hq_operation_id: crate::ids::generate().unwrap(),
            fingerprint: "reset-attempt".into(),
            account: crate::ids::generate().unwrap(),
            finished: false,
        },
        first_account: FirstAccount::new(
            &crate::ids::generate().unwrap(),
            "Other",
            crate::service::authentication::hash_password(
                &org_host_control::Password::new("synthetic other password".into()).unwrap(),
            )
            .unwrap(),
        )
        .unwrap(),
        tls_pair: None,
    };
    // Use the same first-account identity so the writer guard is the refusal.
    let init = HostDbInit {
        bootstrap: BootstrapRecord {
            account: init.first_account.account_id().into(),
            ..init.bootstrap
        },
        ..init
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
    assert_eq!(
        serde_json::to_vec(&writers.reserve(operation, input.clone()).unwrap()).unwrap(),
        serde_json::to_vec(&admission).unwrap()
    );
    let mut changed = input;
    changed.public_root.push_str("-other");
    assert!(writers.reserve(operation, changed).is_err());
}
