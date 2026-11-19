use super::*;
use std::{
    fs,
    os::unix::fs::{symlink, PermissionsExt},
};
fn listen(port: u16) -> ListenConfig {
    serde_json::from_str(&format!(
        r#"{{"address":"127.0.0.1","port":{port},"scheme":"http_loopback","origin":"http://127.0.0.1:{port}","hsts":"off"}}"#
    ))
    .unwrap()
}
fn input(root: PathBuf) -> Input {
    Input {
        version: 1,
        root,
        listen: listen(7420),
    }
}
fn pin() -> ReleasePin {
    ReleasePin {
        release_id: "fixture-1".into(),
        sequence: 1,
        manifest_sha256: "a".repeat(64),
    }
}
fn base() -> (tempfile::TempDir, PathBuf) {
    let temp = super::super::test_directory();
    let base = temp.path().canonicalize().unwrap();
    fs::set_permissions(&base, fs::Permissions::from_mode(0o700)).unwrap();
    (temp, base)
}
#[test]
fn original_operation_survives_reopen_and_input_is_frozen() {
    let (_temp, base) = base();
    let original = input(base.join("instance"));
    let journal = Journal::open(&base, None).unwrap();
    let r = journal.freeze(original.clone(), true, pin()).unwrap();
    assert!(org_proto::is_uuid_v7(&r.setup_operation));
    assert_eq!(
        (
            r.instance_id.as_deref(),
            r.service_registered,
            r.registry_created,
            r.initial_link_issued
        ),
        (None, false, false, false)
    );
    assert!(Journal::open(&base, None).is_err(), "one setup at a time");
    assert_eq!(journal.freeze(original.clone(), true, pin()).unwrap(), r);
    assert!(journal.freeze(original.clone(), false, pin()).is_err());
    let mut moved = original.clone();
    moved.listen = listen(7421);
    assert!(journal.freeze(moved, true, pin()).is_err());
    moved = original.clone();
    moved.root = base.join("elsewhere");
    assert!(journal.freeze(moved, true, pin()).is_err());
    let mut new_pin = pin();
    new_pin.sequence = 2;
    assert!(journal.freeze(original.clone(), true, new_pin).is_err());
    drop(journal);
    let reopened = Journal::open(&base, None).unwrap();
    assert_eq!(reopened.freeze(original, true, pin()).unwrap(), r);
    assert!(Journal::open(&base, Some("../escape")).is_err());
}
#[test]
fn progress_moves_forward_in_order_and_never_replaces_identity() {
    let (_temp, base) = base();
    let journal = Journal::open(&base, None).unwrap();
    let r = journal
        .freeze(input(base.join("instance")), true, pin())
        .unwrap();
    let id = crate::ids::uuid_v7().unwrap();
    // Out of order: each step needs the one before it.
    for broken in [
        Record {
            service_registered: true,
            ..r.clone()
        },
        Record {
            instance_id: Some(id.clone()),
            registry_created: true,
            ..r.clone()
        },
        Record {
            instance_id: Some(id.clone()),
            service_registered: true,
            initial_link_issued: true,
            ..r.clone()
        },
        Record {
            instance_id: Some("not-an-instance".into()),
            ..r.clone()
        },
        Record {
            setup_operation: crate::ids::generate().unwrap(),
            ..r.clone()
        },
    ] {
        assert!(journal.write(&broken).is_err(), "{broken:?}");
    }
    let mut step = r.clone();
    step.instance_id = Some(id.clone());
    journal.write(&step).unwrap();
    step.service_registered = true;
    journal.write(&step).unwrap();
    step.registry_created = true;
    journal.write(&step).unwrap();
    step.initial_link_issued = true;
    journal.write(&step).unwrap();
    assert_eq!(journal.read().unwrap().unwrap(), step);
    for backwards in [
        Record {
            initial_link_issued: false,
            ..step.clone()
        },
        Record {
            instance_id: Some(crate::ids::uuid_v7().unwrap()),
            ..step.clone()
        },
        Record {
            setup_operation: crate::ids::uuid_v7().unwrap(),
            ..step.clone()
        },
        r.clone(),
    ] {
        assert!(journal.write(&backwards).is_err(), "{backwards:?}");
    }
    assert_eq!(Journal::snapshot(&base, None).unwrap(), step);
}
#[test]
fn strict_input_and_replaced_setup_lock_refuse_without_secrets() {
    let (_temp, base) = base();
    let original = input(base.join("instance"));
    for extra in ["password", "login", "organization"] {
        let mut raw = serde_json::to_value(&original).unwrap();
        raw.as_object_mut()
            .unwrap()
            .insert(extra.into(), "forbidden".into());
        assert!(serde_json::from_value::<Input>(raw).is_err(), "{extra}");
    }
    let mut raw = serde_json::to_value(&original).unwrap();
    raw["listen"]["port"] = serde_json::json!(7420.0);
    assert!(
        serde_json::from_value::<Input>(raw).is_err(),
        "no float port"
    );
    let mut bad = original.clone();
    bad.root = base.join("../escape");
    assert!(bad.validate().is_err());
    bad = original.clone();
    bad.version = 2;
    assert!(bad.validate().is_err());
    // Plain HTTP on a non-loopback address cannot even be decoded into the record.
    let mut raw = serde_json::to_value(&original).unwrap();
    raw["listen"]["address"] = serde_json::json!("0.0.0.0");
    assert!(
        serde_json::from_value::<Input>(raw).is_err(),
        "plain HTTP stays on loopback"
    );
    // A valid HTTPS listener on every address decodes, and this setup adapter refuses it.
    bad = original.clone();
    bad.listen = serde_json::from_str(r#"{"address":"0.0.0.0","port":7420,"scheme":"https","origin":"https://office.example:7420","hsts":"off"}"#).unwrap();
    assert!(bad.listen.address().is_unspecified());
    assert!(bad.validate().is_err(), "HTTPS setup is unavailable");
    let journal = Journal::open(&base, None).unwrap();
    let r = journal.freeze(original, true, pin()).unwrap();
    let lock = base.join("setup/default/setup.lock");
    fs::rename(&lock, lock.with_extension("original")).unwrap();
    symlink("setup.v1.json", lock).unwrap();
    assert!(journal.read().is_err());
    assert!(journal.write(&r).is_err());
}
#[test]
fn records_with_unknown_fields_or_over_the_size_limit_are_refused() {
    let (_temp, base) = base();
    let journal = Journal::open(&base, None).unwrap();
    let r = journal
        .freeze(input(base.join("instance")), true, pin())
        .unwrap();
    let file = base.join("setup/default").join(FILE);
    let mut raw = serde_json::to_value(&r).unwrap();
    raw.as_object_mut()
        .unwrap()
        .insert("hq_operation".into(), r.setup_operation.clone().into());
    let tampered = serde_json::to_vec(&raw).unwrap();
    fs::write(&file, &tampered).unwrap();
    assert!(journal.read().is_err());
    assert!(Journal::snapshot(&base, None).is_err());
    let mut padded = serde_json::to_vec(&r).unwrap();
    padded.extend(std::iter::repeat_n(b' ', LIMIT));
    fs::write(&file, &padded).unwrap();
    assert!(journal.read().is_err(), "a record over 64 KiB is not read");
}

#[test]
fn a_credential_directory_is_recorded_only_where_a_release_host_keeps_one() {
    use ProtectedStore::{Keychain, ReleaseDirectory};
    // (store, release build, instance reserved, directory recorded)
    for allowed in [
        // The Keychain, and a system with no protected store: never a directory.
        (Some(Keychain), true, false, false),
        (Some(Keychain), true, true, false),
        (Some(Keychain), false, true, false),
        (None, true, true, false),
        (None, false, false, false),
        // A release directory. A release build names it once its instance is reserved.
        (Some(ReleaseDirectory), true, false, false),
        (Some(ReleaseDirectory), true, true, true),
        // A directory given before the instance is reserved (non-interactive setup).
        (Some(ReleaseDirectory), true, false, true),
        // A development build keeps its development stores and may record either.
        (Some(ReleaseDirectory), false, true, false),
        (Some(ReleaseDirectory), false, true, true),
        (Some(ReleaseDirectory), false, false, false),
    ] {
        let (store, release, reserved, recorded) = allowed;
        assert_eq!(
            credentials_rule(store, release, reserved, recorded),
            Ok(()),
            "{allowed:?}"
        );
    }
    for refused in [
        (Some(Keychain), true, true, true),
        (Some(Keychain), true, false, true),
        (Some(Keychain), false, true, true),
        (None, true, true, true),
        (None, false, false, true),
        // A reserved release instance whose record does not say where its secrets are.
        (Some(ReleaseDirectory), true, true, false),
    ] {
        let (store, release, reserved, recorded) = refused;
        assert!(
            credentials_rule(store, release, reserved, recorded).is_err(),
            "{refused:?}"
        );
    }
}

/// The record on the system these tests run on: where a release host keeps a credential
/// directory, the property is validated strictly and frozen once it is written; where it
/// keeps none (the Keychain), a record that names one is refused.
#[test]
fn the_credential_directory_is_validated_strictly_and_frozen_with_the_record() {
    let (_temp, base) = base();
    let journal = Journal::open(&base, None).unwrap();
    let r = journal
        .freeze(input(base.join("instance")), true, pin())
        .unwrap();
    assert_eq!(r.credentials, None);
    let id = crate::ids::uuid_v7().unwrap();
    let directory = base.join("state/agentic-org/credentials").join(&id);
    let named = Record {
        instance_id: Some(id.clone()),
        credentials: Some(directory.clone()),
        ..r.clone()
    };
    if org_platform_host::credentials::protected_store() != Ok(ProtectedStore::ReleaseDirectory) {
        // Nothing falls back to a directory: the record cannot name one here.
        assert!(journal.write(&named).is_err());
        assert_eq!(journal.read().unwrap().unwrap(), r);
        return;
    }
    // Not an absolute, normalized path: refused, and nothing is written.
    for bad in [
        "relative/credentials",
        "",
        "/",
        "/secrets/../data",
        "/secrets/./x",
        "/secrets//x",
        "/secrets/",
    ] {
        let broken = Record {
            credentials: Some(PathBuf::from(bad)),
            ..named.clone()
        };
        assert!(journal.write(&broken).is_err(), "{bad:?}");
    }
    assert_eq!(journal.read().unwrap().unwrap(), r);
    // Written with the instance it belongs to, it is read back exactly, by the journal and
    // by the host's lock-free snapshot.
    journal.write(&named).unwrap();
    assert_eq!(journal.read().unwrap().unwrap(), named);
    assert_eq!(
        Journal::snapshot(&base, None).unwrap().credentials,
        Some(directory.clone())
    );
    // Frozen: it cannot move or go, whatever else moves forward.
    let mut step = named.clone();
    step.service_registered = true;
    journal.write(&step).unwrap();
    for changed in [
        Record {
            credentials: Some(base.join("elsewhere")),
            ..step.clone()
        },
        Record {
            credentials: None,
            ..step.clone()
        },
    ] {
        assert!(journal.write(&changed).is_err(), "{changed:?}");
    }
    assert_eq!(journal.read().unwrap().unwrap(), step);
    // The property has its own key; a record without the key reads as one that names none,
    // and any other shape of it is refused.
    let file = base.join("setup/default").join(FILE);
    let raw: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    assert_eq!(raw["credentials"], serde_json::json!(directory));
    for shape in [
        serde_json::json!(7),
        serde_json::json!(["/a"]),
        serde_json::json!({}),
    ] {
        let mut tampered = raw.clone();
        tampered["credentials"] = shape;
        fs::write(&file, serde_json::to_vec(&tampered).unwrap()).unwrap();
        assert!(journal.read().is_err());
        assert!(Journal::snapshot(&base, None).is_err());
    }
}
