use super::*;
use crate::{accounts::identity::AccountId, hostdb::HostDbInit};
use org_office_launch::writer::HighWaters;
use std::os::unix::fs::PermissionsExt;

const OPERATION: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

/// What a reservation binds, owned so a test child process can receive it.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Input {
    instance: String,
    organization: String,
    office: String,
    account: String,
    member: String,
}
impl Input {
    fn binding(&self) -> Binding<'_> {
        Binding {
            organization_id: &self.organization,
            office_id: &self.office,
            owner_account: &self.account,
            owner_member: &self.member,
        }
    }
}

fn fixture() -> (tempfile::TempDir, Arc<Writers>, Input) {
    let root = tempfile::tempdir().unwrap();
    let instance_root = root.path().join("instance");
    fs::create_dir(&instance_root).unwrap();
    fs::set_permissions(&instance_root, fs::Permissions::from_mode(0o700)).unwrap();
    let home = instance_root.join("office-state");
    fs::create_dir(&home).unwrap();
    fs::set_permissions(&home, fs::Permissions::from_mode(0o700)).unwrap();
    let keys = root.path().join("keys");
    fs::create_dir(&keys).unwrap();
    fs::set_permissions(&keys, fs::Permissions::from_mode(0o700)).unwrap();
    let input = Input {
        instance: crate::ids::uuid_v7().unwrap(),
        organization: crate::ids::uuid_v7().unwrap(),
        office: crate::ids::uuid_v7().unwrap(),
        account: crate::ids::generate().unwrap(),
        member: crate::ids::uuid_v7().unwrap(),
    };
    fs::create_dir_all(home.join("offices").join(&input.office)).unwrap();
    let custody: Arc<dyn HostSecrets> = Arc::new(
        crate::host_secrets::dev::DevSecrets::open(&keys, &home, &input.instance).unwrap(),
    );
    let password = org_host_control::Password::new("synthetic writer password".into()).unwrap();
    let init = HostDbInit {
        instance_id: input.instance.clone(),
        clock_high_ms: 1,
    };
    let db = Arc::new(HostDb::create(&home, &*custody, &init).unwrap());
    db.seed_account(
        &input.account,
        "Owner",
        &crate::service::authentication::hash_password(&password).unwrap(),
        true,
        1,
    )
    .unwrap();
    let admin = db
        .account_authority(&AccountId::parse(&input.account).unwrap())
        .unwrap()
        .unwrap();
    db.establish_organization_owner(&admin, &input.organization, &input.member)
        .unwrap();
    let writers = Writers::open(custody, db, &input.instance, &instance_root).unwrap();
    (root, writers, input)
}
fn anchor() -> Head {
    Head {
        ordinal: 0,
        sha256: "a".repeat(64),
        high_waters: HighWaters {
            organization: 0,
            work: 73,
            message_content: 0,
            message_state: 0,
        },
    }
}
/// The head `entries` ordinals after `before`, with `events` more work positions.
fn later(before: &Head, entries: u64, events: u64) -> Head {
    let ordinal = before.ordinal + entries;
    Head {
        ordinal,
        sha256: format!("{ordinal:064x}"),
        high_waters: HighWaters {
            work: before.high_waters.work + events,
            ..before.high_waters
        },
    }
}
fn lease(writers: &Arc<Writers>, office: &str) -> Lease {
    Lease {
        writers: Arc::clone(writers),
        office: office.into(),
        record: writers.load(office).unwrap().unwrap(),
        live: Arc::new(AtomicBool::new(true)),
    }
}
fn confirm(lease: &mut Lease, before: Option<Head>, after: Head) -> io::Result<Response> {
    lease.answer(Request::Confirm {
        identity: lease.record.admission.identity.clone(),
        before,
        after,
    })
}
fn checkpoint(lease: &mut Lease) -> Option<Head> {
    match lease
        .answer(Request::Checkpoint {
            identity: lease.record.admission.identity.clone(),
        })
        .unwrap()
    {
        Response::Checkpoint { head } => head,
        other => panic!("{other:?}"),
    }
}

/// P1: the reservation mints what the store's `OfficeIdentity` accepts (a UUIDv7
/// incarnation, a 64-hex device, epoch 1) and the contract's data roots.
#[test]
fn a_reservation_mints_a_uuid_v7_incarnation_and_the_contract_roots() {
    let (_root, writers, input) = fixture();
    assert!(writers.admit(&input.office).is_err());
    let admission = writers.reserve(OPERATION, &input.binding()).unwrap();
    writers.admit(&input.office).unwrap();
    assert!(admission.check());
    let identity = &admission.identity;
    assert!(org_proto::is_uuid_v7(&identity.journal_incarnation));
    assert_eq!(identity.writer_epoch, 1);
    assert_eq!(identity.machine_id.len(), 64);
    assert_eq!(
        (
            &identity.instance_id,
            &identity.organization_id,
            &identity.office_id
        ),
        (&input.instance, &input.organization, &input.office)
    );
    let base = writers.root.path.join(&input.organization);
    assert_eq!(Path::new(&admission.durable_root), base.join("durable"));
    assert_eq!(Path::new(&admission.runtime_root), base.join("runtime"));
    assert_eq!(
        (
            admission.owner_account.as_str(),
            admission.owner_member.as_str()
        ),
        (input.account.as_str(), input.member.as_str())
    );
    // The record is schema 3 and starts with no head.
    let record = writers.load(&input.office).unwrap().unwrap();
    assert_eq!(record.schema_version, 3);
    assert_eq!(record.head, None);
    // Each office gets its own incarnation.
    let (_other_root, other, other_input) = fixture();
    let second = other.reserve(OPERATION, &other_input.binding()).unwrap();
    assert_ne!(
        second.identity.journal_incarnation,
        identity.journal_incarnation
    );
}

/// F4: one confirmation per flush may advance many ordinals; an equal, identical one is
/// accepted; a lower high-water and a `before` that is not the stored head are refused,
/// and a refused confirmation stores nothing.
#[test]
fn confirmations_follow_the_custody_rule_and_persist_across_reopen() {
    let (_root, writers, input) = fixture();
    let admission = writers.reserve(OPERATION, &input.binding()).unwrap();
    let mut control = lease(&writers, &input.office);
    assert_eq!(checkpoint(&mut control), None);
    // The first confirmation is the anchor, and nothing else.
    assert!(confirm(&mut control, None, later(&anchor(), 1, 0)).is_err());
    let a = anchor();
    confirm(&mut control, None, a.clone()).unwrap();
    // Several ordinals at once.
    let three = later(&a, 3, 5);
    confirm(&mut control, Some(a.clone()), three.clone()).unwrap();
    // The same head again.
    confirm(&mut control, Some(three.clone()), three.clone()).unwrap();
    // A lower high-water.
    let mut lower = later(&three, 1, 0);
    lower.high_waters.work -= 1;
    assert!(confirm(&mut control, Some(three.clone()), lower).is_err());
    // A `before` that is not the stored head.
    assert!(confirm(&mut control, Some(a.clone()), later(&three, 1, 0)).is_err());
    assert!(confirm(&mut control, None, later(&three, 1, 0)).is_err());
    // The same ordinal with another hash, and going back.
    let mut forked = three.clone();
    forked.sha256 = "c".repeat(64);
    assert!(confirm(&mut control, Some(three.clone()), forked).is_err());
    assert!(confirm(&mut control, Some(three.clone()), a.clone()).is_err());
    // Another identity.
    let mut wrong = admission.identity.clone();
    wrong.writer_epoch += 1;
    assert!(control
        .answer(Request::Confirm {
            identity: wrong,
            before: Some(three.clone()),
            after: later(&three, 1, 0),
        })
        .is_err());
    assert_eq!(checkpoint(&mut control), Some(three.clone()));
    drop(control);
    let reopened = Writers::open(
        Arc::clone(&writers.custody),
        Arc::clone(&writers.db),
        &writers.instance,
        &writers.root.path,
    )
    .unwrap();
    let mut control = lease(&reopened, &input.office);
    assert_eq!(checkpoint(&mut control), Some(three.clone()));
    confirm(&mut control, Some(three.clone()), later(&three, 2, 1)).unwrap();
}

/// A retried reservation must be the same request: another organization (so other data
/// roots), owner or operation is refused, and the record is unchanged.
#[test]
fn a_retried_reservation_with_other_inputs_is_refused() {
    let (_root, writers, input) = fixture();
    let admission = writers.reserve(OPERATION, &input.binding()).unwrap();
    let credential = name(&input.office).unwrap();
    let original = writers.custody.load(&credential).unwrap().unwrap();
    let mut other_org = input.clone();
    other_org.organization = crate::ids::uuid_v7().unwrap();
    let mut other_member = input.clone();
    other_member.member = crate::ids::uuid_v7().unwrap();
    for (operation, changed) in [
        (OPERATION, other_org),
        (OPERATION, other_member),
        ("01ARZ3NDEKTSV4RRFFQ69G5FAW", input.clone()),
    ] {
        assert!(writers.reserve(operation, &changed.binding()).is_err());
        assert_eq!(
            writers.custody.load(&credential).unwrap().unwrap().expose(),
            original.expose()
        );
    }
    assert_eq!(
        writers.reserve(OPERATION, &input.binding()).unwrap(),
        admission
    );
}

/// The data roots and the office folder are required empty at the first reservation:
/// retained data without its record is never given a new writer.
#[test]
fn a_first_reservation_over_retained_data_is_refused() {
    for retained in ["office", "durable", "runtime", "durable-file"] {
        let (root, writers, input) = fixture();
        let (durable, runtime) = writers.data_roots(&input.organization).unwrap();
        let path = match retained {
            "office" => writers
                .marker(&input.office)
                .unwrap()
                .with_file_name("leftover"),
            "durable" => durable.join("state/organization.yaml"),
            "runtime" => runtime.join(&input.office).join("office.db"),
            _ => durable.clone(),
        };
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"retained").unwrap();
        let before = retained_files(root.path());
        assert!(
            writers.reserve(OPERATION, &input.binding()).is_err(),
            "{retained}"
        );
        assert!(writers.load(&input.office).unwrap().is_none());
        assert_eq!(retained_files(root.path()), before, "{retained}");
    }
}

/// Only schema 3 is read. An older record (schema 2: one origin high-water, public and
/// protected roots, a ULID incarnation) is refused for a start and for a retry, and it is
/// kept as it is: no migration, no replacement.
#[test]
fn an_older_writer_record_is_refused_and_kept() {
    let (_root, writers, input) = fixture();
    writers.reserve(OPERATION, &input.binding()).unwrap();
    let credential = name(&input.office).unwrap();
    let current: serde_json::Value =
        serde_json::from_slice(writers.custody.load(&credential).unwrap().unwrap().expose())
            .unwrap();
    let mut schema_two = current.clone();
    schema_two["schema_version"] = serde_json::json!(2);
    let mut old_shape = current.clone();
    old_shape.as_object_mut().unwrap().remove("head");
    old_shape["schema_version"] = serde_json::json!(2);
    old_shape["position"] = serde_json::Value::Null;
    old_shape["admission"]["version"] = serde_json::json!(1);
    old_shape["admission"]["identity"]["journal_incarnation"] =
        serde_json::json!("01ARZ3NDEKTSV4RRFFQ69G5FAV");
    let mut wrong_roots = current.clone();
    wrong_roots["admission"]["durable_root"] =
        serde_json::json!(writers.root.path.join("durable").to_str().unwrap());
    for planted in [schema_two, old_shape, wrong_roots] {
        let bytes = serde_json::to_vec(&planted).unwrap();
        writers
            .custody
            .put(&credential, &Value::new(bytes.clone()).unwrap())
            .unwrap();
        assert_eq!(writers.admit(&input.office), Err(AdmitError::Refused));
        let (host, _worker) = UnixStream::pair().unwrap();
        assert!(writers
            .serve(&input.office, host, Arc::new(AtomicBool::new(true)))
            .is_err());
        assert!(writers.reserve(OPERATION, &input.binding()).is_err());
        assert_eq!(
            writers.custody.load(&credential).unwrap().unwrap().expose(),
            &bytes[..]
        );
    }
}

/// The served channel speaks version 2: the admission first, then checkpoint and confirm
/// with heads; a refused request is answered `denied` and closes the channel.
#[test]
fn the_served_channel_speaks_version_two() {
    use std::io::BufReader;
    let (_root, writers, input) = fixture();
    let reserved = writers.reserve(OPERATION, &input.binding()).unwrap();
    let (host, worker) = UnixStream::pair().unwrap();
    writers
        .serve(&input.office, host, Arc::new(AtomicBool::new(true)))
        .unwrap();
    // One live channel per office.
    let (second, _peer) = UnixStream::pair().unwrap();
    assert!(writers
        .serve(&input.office, second, Arc::new(AtomicBool::new(true)))
        .is_err());
    let mut worker = BufReader::new(worker);
    let admission: Admission = wire::read(&mut worker).unwrap().unwrap();
    assert_eq!(admission, reserved);
    let identity = admission.identity;
    let ask = |worker: &mut BufReader<UnixStream>, request: &Request| {
        wire::write(worker.get_mut(), request).unwrap();
        wire::read::<Response>(worker).unwrap()
    };
    assert!(matches!(
        ask(
            &mut worker,
            &Request::Checkpoint {
                identity: identity.clone()
            }
        ),
        Some(Response::Checkpoint { head: None })
    ));
    assert!(matches!(
        ask(
            &mut worker,
            &Request::Confirm {
                identity: identity.clone(),
                before: None,
                after: anchor(),
            }
        ),
        Some(Response::Confirmed)
    ));
    let three = later(&anchor(), 3, 2);
    assert!(matches!(
        ask(
            &mut worker,
            &Request::Confirm {
                identity: identity.clone(),
                before: Some(anchor()),
                after: three.clone(),
            }
        ),
        Some(Response::Confirmed)
    ));
    assert!(matches!(
        ask(&mut worker, &Request::Checkpoint { identity: identity.clone() }),
        Some(Response::Checkpoint { head: Some(h) }) if h == three
    ));
    assert!(matches!(
        ask(&mut worker, &Request::Owner { identity: identity.clone() }),
        Some(Response::Owner { account, member }) if account == input.account && member == input.member
    ));
    // A stale `before`: denied, and the channel ends.
    assert!(matches!(
        ask(
            &mut worker,
            &Request::Confirm {
                identity,
                before: Some(anchor()),
                after: later(&three, 1, 0),
            }
        ),
        Some(Response::Denied)
    ));
    assert!(wire::read::<Response>(&mut worker).unwrap().is_none());
    assert_eq!(
        writers.load(&input.office).unwrap().unwrap().head,
        Some(three)
    );
}

/// Admission tells a refusal (no record, an invalid one, no exact marker) from custody that
/// can't be read now, and reports what an admitted writer is bound to.
#[test]
fn admission_tells_a_refusal_from_unreadable_custody() {
    let dir = tempfile::tempdir().unwrap();
    let instance = crate::ids::uuid_v7().unwrap();
    let office = crate::ids::uuid_v7().unwrap();
    let custody = TestCustody::reserve(dir.path(), &instance, &[&office]);
    let writers = &custody.writers;
    assert_eq!(
        writers.admit(&office),
        Ok(Bound {
            organization_id: custody.organization.clone(),
            owner_member: custody.member.clone(),
        })
    );
    assert_eq!(
        writers.admit(&crate::ids::uuid_v7().unwrap()),
        Err(AdmitError::Refused)
    );
    custody.unreadable.store(true, Ordering::SeqCst);
    assert_eq!(writers.admit(&office), Err(AdmitError::Unreadable));
    custody.unreadable.store(false, Ordering::SeqCst);
    fs::remove_file(writers.marker(&office).unwrap()).unwrap();
    assert_eq!(writers.admit(&office), Err(AdmitError::Refused));
}

/// Lasting store states are refusals, never retried as unreadable: a symlinked custody
/// item and a removed store directory.
#[test]
fn lasting_custody_failures_are_refusals_not_unreadable() {
    use std::os::unix::fs::symlink;
    // The development store keeps each item at `<keys>/host-secrets/<instance>/<name>`.
    let store = |root: &tempfile::TempDir, input: &Input| {
        root.path().join("keys/host-secrets").join(&input.instance)
    };
    let (root, writers, input) = fixture();
    writers.reserve(OPERATION, &input.binding()).unwrap();
    writers.admit(&input.office).unwrap();
    let item = store(&root, &input).join(name(&input.office).unwrap().as_str());
    assert!(item.is_file(), "{item:?}");
    let target = root.path().join("linked-record");
    fs::rename(&item, &target).unwrap();
    symlink(&target, &item).unwrap();
    assert_eq!(writers.admit(&input.office), Err(AdmitError::Refused));

    let (root, writers, input) = fixture();
    writers.reserve(OPERATION, &input.binding()).unwrap();
    fs::rename(store(&root, &input), root.path().join("moved-store")).unwrap();
    assert_eq!(writers.admit(&input.office), Err(AdmitError::Refused));
}

/// Only failures that pass on their own are unreadable; every lasting store or registry
/// state is a refusal. (A corrupted registry file is not read again while the host holds
/// it open, so its classification is checked here directly.)
#[test]
fn only_transient_store_and_registry_failures_are_unreadable() {
    use crate::{host_secrets::Error as S, hostdb::Error as D};
    use io::ErrorKind as K;
    use rusqlite::ErrorCode as C;
    let of = |error: io::Error| AdmitError::from(error);
    for kind in [
        K::Interrupted,
        K::WouldBlock,
        K::TimedOut,
        K::ResourceBusy,
        K::OutOfMemory,
    ] {
        assert_eq!(of(store(S::Io(kind))), AdmitError::Unreadable, "{kind:?}");
        assert_eq!(
            of(registry(D::Io(kind))),
            AdmitError::Unreadable,
            "{kind:?}"
        );
    }
    assert_eq!(of(store(S::Os(-25308))), AdmitError::Unreadable);
    assert_eq!(of(store(S::InUse)), AdmitError::Unreadable);
    for code in [
        C::DatabaseBusy,
        C::DatabaseLocked,
        C::SystemIoFailure,
        C::OutOfMemory,
    ] {
        assert_eq!(
            of(registry(D::Sql(code))),
            AdmitError::Unreadable,
            "{code:?}"
        );
    }
    for kind in [
        K::PermissionDenied,
        K::NotFound,
        K::Unsupported,
        K::Other,
        K::InvalidData,
    ] {
        assert_eq!(of(store(S::Io(kind))), AdmitError::Refused, "{kind:?}");
        assert_eq!(of(registry(D::Io(kind))), AdmitError::Refused, "{kind:?}");
    }
    for error in [
        S::Corrupt,
        S::Unsafe,
        S::CaseAlias,
        S::InvalidName,
        S::TooLarge,
        // errSecMissingEntitlement, errSecDecode, errSecParam, errSecNoSuchKeychain,
        // errSecInvalidKeychain, errSecNotAvailable.
        S::Os(-34018),
        S::Os(-26275),
        S::Os(-50),
        S::Os(-25294),
        S::Os(-25295),
        S::Os(-25291),
    ] {
        let shown = format!("{error:?}");
        assert_eq!(of(store(error)), AdmitError::Refused, "{shown}");
    }
    for code in [
        C::NotADatabase,
        C::DatabaseCorrupt,
        C::SchemaChanged,
        C::ConstraintViolation,
        C::PermissionDenied,
        C::ReadOnly,
    ] {
        assert_eq!(of(registry(D::Sql(code))), AdmitError::Refused, "{code:?}");
    }
    for error in [D::Unsafe, D::Version, D::Outdated] {
        assert_eq!(of(registry(error)), AdmitError::Refused);
    }
}

#[test]
fn copied_root_missing_custody_replaced_root_and_changed_epoch_never_promote() {
    let (root, writers, input) = fixture();
    let admission = writers.reserve(OPERATION, &input.binding()).unwrap();
    assert!(writers
        .reserve("01ARZ3NDEKTSV4RRFFQ69G5FAW", &input.binding())
        .is_err());
    let mut control = lease(&writers, &input.office);
    let mut wrong = admission.identity.clone();
    wrong.writer_epoch += 1;
    assert!(control
        .answer(Request::Checkpoint { identity: wrong })
        .is_err());
    let record = control.record.clone();
    let device = Name::parse("writer-device").unwrap();
    writers.custody.delete(&device).unwrap();
    assert!(writers.verify(&record, &input.office).is_err());
    assert!(writers.admit(&input.office).is_err());
    let moved = root.path().join("retained-instance");
    fs::rename(&writers.root.path, &moved).unwrap();
    fs::create_dir(&writers.root.path).unwrap();
    fs::set_permissions(&writers.root.path, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(writers.verify(&record, &input.office).is_err());
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
    writers.reserve(OPERATION, &input.binding()).unwrap();
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
    let admission = writers.reserve(OPERATION, &input.binding()).unwrap();
    let mut control = lease(&writers, &input.office);
    let anchor = anchor();
    confirm(&mut control, None, anchor.clone()).unwrap();
    let accepted = later(&anchor, 1, 1);
    confirm(&mut control, Some(anchor), accepted.clone()).unwrap();
    // Interrupted setup's marker publication is resumable only by its protected
    // operation. This is not adoption of an unknown/missing authority record.
    fs::remove_file(writers.marker(&input.office).unwrap()).unwrap();
    assert!(writers.admit(&input.office).is_err());
    let abandoned = writers
        .marker(&input.office)
        .unwrap()
        .with_file_name(".selected-writer-abandoned.tmp");
    fs::write(&abandoned, b"selected").unwrap();
    let resumed = writers.reserve(OPERATION, &input.binding()).unwrap();
    assert_eq!(resumed, admission);
    assert_eq!(
        writers.load(&input.office).unwrap().unwrap().head,
        Some(accepted)
    );
    assert!(writers.marker(&input.office).unwrap().is_file());
    writers.admit(&input.office).unwrap();
    assert_eq!(fs::read(abandoned).unwrap(), b"selected");
    assert_eq!(
        writers.reserve(OPERATION, &input.binding()).unwrap(),
        admission
    );
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
    for retained in ["marker", "office", "durable", "runtime"] {
        for lose_device in [false, true] {
            let (root, writers, input) = fixture();
            let admission = writers.reserve(OPERATION, &input.binding()).unwrap();
            let mut control = lease(&writers, &input.office);
            let initial = anchor();
            confirm(&mut control, None, initial.clone()).unwrap();
            confirm(&mut control, Some(initial.clone()), later(&initial, 1, 3)).unwrap();
            drop(control);
            let credential = name(&input.office).unwrap();
            let original = writers.custody.load(&credential).unwrap().unwrap();
            if retained != "marker" {
                fs::remove_file(writers.marker(&input.office).unwrap()).unwrap();
                let directory = match retained {
                    "office" => writers
                        .marker(&input.office)
                        .unwrap()
                        .parent()
                        .unwrap()
                        .to_owned(),
                    "durable" => PathBuf::from(&admission.durable_root),
                    "runtime" => PathBuf::from(&admission.runtime_root),
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
                writers.reserve(OPERATION, &input.binding()).is_err(),
                "{retained}/{lose_device}"
            );
            assert!(writers.custody.load(&credential).unwrap().is_none());
            assert_eq!(retained_files(root.path()), before);
            // Retained original custody is evidence, never an automatically restored item.
            let original: Record = serde_json::from_slice(original.expose()).unwrap();
            assert_eq!(original.admission.identity, admission.identity);
            assert_eq!(original.head, Some(later(&initial, 1, 3)));
        }
    }
}

#[test]
fn missing_device_cannot_be_recreated_while_another_writer_record_survives() {
    let (root, writers, input) = fixture();
    writers.reserve(OPERATION, &input.binding()).unwrap();
    writers
        .custody
        .delete(&Name::parse("writer-device").unwrap())
        .unwrap();
    let mut second = input.clone();
    second.office = crate::ids::uuid_v7().unwrap();
    fs::create_dir(writers.marker(&second.office).unwrap().parent().unwrap()).unwrap();
    let before = retained_files(root.path());
    assert!(writers
        .reserve("01ARZ3NDEKTSV4RRFFQ69G5FAW", &second.binding())
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
        writers.reserve(OPERATION, &input.binding()).unwrap();
        let credential = name(&input.office).unwrap();
        let original = writers.custody.load(&credential).unwrap().unwrap();
        let path = writers.marker(&input.office).unwrap();
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
        assert!(
            writers.reserve(OPERATION, &input.binding()).is_err(),
            "{kind}"
        );
        assert!(writers.admit(&input.office).is_err(), "{kind}");
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
    input: Input,
    limit: Option<u64>,
}

fn reopen_marker_writer(root: &Path, input: &Input) -> Arc<Writers> {
    let instance_root = root.join("instance");
    let home = instance_root.join("office-state");
    let custody = Arc::new(
        crate::host_secrets::dev::DevSecrets::open(&root.join("keys"), &home, &input.instance)
            .unwrap(),
    );
    let db = Arc::new(HostDb::open(&home, &*custody).unwrap());
    Writers::open(custody, db, &input.instance, &instance_root).unwrap()
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
    let result = writers.reserve(OPERATION, &input.input.binding());
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
        let admission = writers.reserve(OPERATION, &input.binding()).unwrap();
        let mut control = lease(&writers, &input.office);
        let initial = anchor();
        confirm(&mut control, None, initial.clone()).unwrap();
        confirm(&mut control, Some(initial.clone()), later(&initial, 1, 2)).unwrap();
        drop(control);
        let credential = name(&input.office).unwrap();
        let original = writers.custody.load(&credential).unwrap().unwrap();
        let marker = writers.marker(&input.office).unwrap();
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
        let resumed = writers.reserve(OPERATION, &input.binding()).unwrap();
        assert_eq!(resumed, admission);
        assert_eq!(
            writers.custody.load(&credential).unwrap().unwrap().expose(),
            original.expose()
        );
        assert_eq!(
            writers.load(&input.office).unwrap().unwrap().head,
            Some(later(&initial, 1, 2))
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
