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
        service: Service::Registered,
        root,
        credentials: None,
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
fn v1_setup_record_resumes_as_registered_but_configuration_still_requires_service() {
    // The bytes published by origin/v1's Journal::write.
    let bytes = include_bytes!("setup-v1-record.json");
    let (_temp, base) = base();
    let journal = Journal::open(&base, None).unwrap();
    journal.directory.publish(FILE, bytes, None).unwrap();

    let mut record = Journal::snapshot(&base, None).unwrap();
    assert_eq!(record.input.service, Service::Registered);
    assert!(record.service_registered && record.registry_created);
    assert_eq!(journal.read().unwrap(), Some(record.clone()));
    record.initial_link_issued = true;
    journal.write(&record).unwrap();
    assert_eq!(journal.read().unwrap(), Some(record));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(
            &fs::read(base.join("setup/default").join(FILE)).unwrap()
        )
        .unwrap()["input"]["service"],
        "registered"
    );

    let raw: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    let omitted = serde_json::to_vec(&raw["input"]).unwrap();
    assert!(Input::from_configuration(&omitted).is_err());
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
    // HTTPS decodes as a listener and is refused as an input: no build serves it yet, so
    // no instance is set up for it (X5 review V11). The refusal names the modes that work.
    let https = |origin: &str| {
        serde_json::from_str::<ListenConfig>(&format!(
            r#"{{"address":"0.0.0.0","port":7420,"scheme":"https","origin":"{origin}","hsts":"off"}}"#
        ))
        .map(|listen| Input {
            listen,
            ..original.clone()
        })
    };
    let secure = https("https://office.example:7420").unwrap();
    assert!(secure.listen.address().is_unspecified());
    let refused = secure.validate().unwrap_err().to_string();
    assert!(
        refused.contains("\"https\" is not available in this build")
            && refused.contains("http_loopback")
            && refused.contains("http_published"),
        "{refused}"
    );
    assert!(https("https://office.example:7421").is_err());
    assert!(https("http://office.example:7420").is_err());
    // The half-built certificate property is gone: a file that names it has an unknown
    // property.
    let mut raw = serde_json::to_value(&original).unwrap();
    raw["certificate"] = serde_json::json!({"chain": "/c.pem", "key": "/k.pem"});
    assert!(serde_json::from_value::<Input>(raw).is_err());
    // The credential directory is spelled as it is walked.
    for (path, valid) in [
        ("/secrets", true),
        ("/secrets/", false),
        ("/a/../secrets", false),
        ("secrets", false),
    ] {
        bad = original.clone();
        bad.credentials = Some(path.into());
        assert_eq!(bad.validate().is_ok(), valid, "{path}");
    }
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

/// The configuration of non-interactive setup is the record's own input, read strictly:
/// every property is required or named optional, nothing else is accepted, and a mistake
/// is answered with the whole shape.
#[test]
fn a_configuration_file_is_the_strict_input_and_names_its_shape_when_it_is_wrong() {
    let text = r#"{
      "version": 1, "service": "foreground", "root": "/data/instance",
      "credentials": "/secrets",
      "listen": {"address": "0.0.0.0", "port": 8443, "scheme": "http_published",
                 "origin": "http://127.0.0.1:8443", "hsts": "off"}
    }"#;
    let input = Input::from_configuration(text.as_bytes()).unwrap();
    assert_eq!(input.service, Service::Foreground);
    assert_eq!(input.root, PathBuf::from("/data/instance"));
    assert_eq!(input.credentials, Some(PathBuf::from("/secrets")));
    assert_eq!(input.listen.port(), 8443);
    // It is exactly what the record stores.
    assert_eq!(
        serde_json::from_value::<Input>(serde_json::to_value(&input).unwrap()).unwrap(),
        input
    );
    let value: serde_json::Value = serde_json::from_str(text).unwrap();
    let with = |change: &dyn Fn(&mut serde_json::Value)| {
        let mut changed = value.clone();
        change(&mut changed);
        Input::from_configuration(changed.to_string().as_bytes())
    };
    assert!(with(&|_| {}).is_ok());
    for (what, change) in [
        (
            "an unknown property",
            (&|v: &mut serde_json::Value| v["password"] = "x".into())
                as &dyn Fn(&mut serde_json::Value),
        ),
        ("no service mode", &|v| {
            v.as_object_mut().unwrap().remove("service");
        }),
        ("an unknown service mode", &|v| {
            v["service"] = "daemon".into()
        }),
        ("another version", &|v| v["version"] = 2.into()),
        ("no version", &|v| {
            v.as_object_mut().unwrap().remove("version");
        }),
        ("a relative root", &|v| v["root"] = "data".into()),
        ("an unknown listener property", &|v| {
            v["listen"]["tls"] = true.into()
        }),
        ("plain HTTP off loopback", &|v| {
            v["listen"]["scheme"] = "http_loopback".into();
            v["listen"]["origin"] = "http://127.0.0.1:8443".into();
        }),
        ("an unknown certificate property", &|v| {
            v["certificate"] = serde_json::json!({"chain": "/c", "key": "/k", "pass": "x"});
        }),
        ("a certificate without a key", &|v| {
            v["certificate"] = serde_json::json!({"chain": "/c"})
        }),
    ] {
        assert!(with(change).is_err(), "{what} was accepted");
    }
    // A repeated property, something that is not an object, and a file that is too large.
    for bytes in [
        text.replacen("\"version\": 1,", "\"version\": 1, \"version\": 1,", 1)
            .into_bytes(),
        b"[]".to_vec(),
        b"".to_vec(),
        format!("{text}{}", " ".repeat(LIMIT)).into_bytes(),
    ] {
        assert!(Input::from_configuration(&bytes).is_err());
    }
    let refused = Input::from_configuration(b"{}").unwrap_err().to_string();
    assert!(
        refused.contains("\"service\": \"registered\" or \"foreground\""),
        "{refused}"
    );
    assert!(
        refused.contains("no other property is accepted"),
        "{refused}"
    );
}

/// Which systems an input fits: a directory is named only where secrets are kept in one,
/// and the foreground mode must name it there.
#[test]
fn an_input_names_a_credential_directory_only_where_the_system_keeps_one() {
    let mut input = input(PathBuf::from("/data/instance"));
    let directory = Some(ProtectedStore::ReleaseDirectory);
    for store in [directory, Some(ProtectedStore::Keychain), None] {
        assert_eq!(
            input.fits(store),
            Ok(()),
            "registered, none named: {store:?}"
        );
    }
    input.credentials = Some("/secrets".into());
    assert_eq!(input.fits(directory), Ok(()));
    assert!(input
        .fits(Some(ProtectedStore::Keychain))
        .unwrap_err()
        .contains("remove it"));
    assert!(input.fits(None).is_err());
    input.service = Service::Foreground;
    assert_eq!(input.fits(directory), Ok(()));
    assert!(input.fits(Some(ProtectedStore::Keychain)).is_err());
    input.credentials = None;
    assert!(input
        .fits(directory)
        .unwrap_err()
        .contains("mounted secrets volume"));
    // Where secrets are kept elsewhere, the foreground mode names none.
    assert_eq!(input.fits(Some(ProtectedStore::Keychain)), Ok(()));
    assert_eq!(input.fits(None), Ok(()));
}

/// Plain HTTP on a published port is chosen by name, for a foreground instance only. An
/// input, a configuration file and a record are all judged: with `registered` it is refused
/// when it is read, before anything is made, and a record edited by hand to say so is not
/// read by the host.
#[test]
fn http_published_is_only_for_the_foreground_mode_and_is_never_implied() {
    let published: ListenConfig = serde_json::from_str(
        r#"{"address":"0.0.0.0","port":8443,"scheme":"http_published","origin":"http://127.0.0.1:8443","hsts":"off"}"#,
    )
    .unwrap();
    let mut foreground = input(PathBuf::from("/data/instance"));
    foreground.service = Service::Foreground;
    foreground.listen = published.clone();
    assert!(foreground.validate().is_ok());
    let mut registered = foreground.clone();
    registered.service = Service::Registered;
    let refused = registered.validate().unwrap_err().to_string();
    assert!(refused.contains(PUBLISHED_NEEDS_FOREGROUND), "{refused}");
    // The same two as configuration files.
    let text = |service: &str, scheme: &str, address: &str, origin: &str| {
        format!(
            r#"{{"version":1,"service":"{service}","root":"/data/instance","credentials":"/secrets",
                "listen":{{"address":"{address}","port":8443,"scheme":"{scheme}","origin":"{origin}","hsts":"off"}}}}"#
        )
    };
    let read = |service, scheme, address, origin| {
        Input::from_configuration(text(service, scheme, address, origin).as_bytes())
    };
    assert!(read(
        "foreground",
        "http_published",
        "0.0.0.0",
        "http://127.0.0.1:8443"
    )
    .is_ok());
    let refused = read(
        "registered",
        "http_published",
        "0.0.0.0",
        "http://127.0.0.1:8443",
    )
    .unwrap_err()
    .to_string();
    assert!(
        refused.contains("only for \"service\": \"foreground\""),
        "{refused}"
    );
    // Never implied, never widened: plain `http_loopback` with another address, another
    // spelling of the scheme, a published listener on loopback, or a host that is not a
    // loopback name are all refused, in either service mode.
    for service in ["foreground", "registered"] {
        for (scheme, address, origin) in [
            ("http_loopback", "0.0.0.0", "http://localhost:8443"),
            ("http_loopback", "0.0.0.0", "http://0.0.0.0:8443"),
            ("http", "0.0.0.0", "http://localhost:8443"),
            ("http-published", "0.0.0.0", "http://localhost:8443"),
            ("http_published", "127.0.0.1", "http://127.0.0.1:8443"),
            ("http_published", "0.0.0.0", "http://office.example:8443"),
            ("http_published", "0.0.0.0", "http://192.168.1.20:8443"),
            ("http_published", "0.0.0.0", "https://localhost:8443"),
        ] {
            assert!(
                read(service, scheme, address, origin).is_err(),
                "{service} {scheme} {address} {origin}"
            );
        }
    }
    // A record: written for a foreground instance, then edited by hand to `registered`.
    let (_temp, base) = base();
    let mut stored = foreground.clone();
    stored.root = base.join("instance");
    stored.credentials = None;
    let journal = Journal::open(&base, None).unwrap();
    let record = journal.freeze(stored, true, pin()).unwrap();
    drop(journal);
    assert_eq!(Journal::snapshot(&base, None).unwrap(), record);
    let file = base.join("setup/default/setup.v1.json");
    let edited = fs::read_to_string(&file).unwrap().replacen(
        "\"service\":\"foreground\"",
        "\"service\":\"registered\"",
        1,
    );
    assert!(edited.contains("\"service\":\"registered\""));
    fs::write(&file, edited).unwrap();
    let refused = Journal::snapshot(&base, None).unwrap_err().to_string();
    assert!(refused.contains("http_published"), "{refused}");
    assert!(Journal::open(&base, None).unwrap().read().is_err());
}
