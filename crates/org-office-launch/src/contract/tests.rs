use std::collections::BTreeSet;
use std::io::Cursor;

use serde_json::{json, Value};

use super::*;

const INSTANCE: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a1";
const OFFICE: &str = "01900000-0000-7000-8000-000000000002";
const ORG: &str = "01900000-0000-7000-8000-000000000001";
/// The ids of a version 1 `office.json`, which are plain.
const FILE_OFFICE: &str = "01M72N8MG00000000000000F01";
const FILE_ORG: &str = "01M72N8MG00000000000000G01";
const OWNER: &str = "01900000-0000-7000-8000-0000000000ee";
const RENDEZVOUS: &str = "/run/user/501/agentic-org/r/0123456789abcdef";

fn office() -> OfficeLine {
    OfficeLine {
        instance_id: INSTANCE.into(),
        office_id: OFFICE.into(),
        rendezvous: RENDEZVOUS.into(),
    }
}

/// A launch line names its office by a UUIDv7: a ULID or another plain id is refused on
/// both sides, and so is the retired `id_profile` key.
#[test]
fn a_first_line_names_a_uuidv7_office_only() {
    let line = office();
    let mut bytes = Vec::new();
    write_first_line(&mut bytes, &HostFirstLine::Start(line.clone())).unwrap();
    assert!(
        matches!(read_first_line(&mut bytes.as_slice(), FirstLine::Start).unwrap(),Some(HostFirstLine::Start(actual)) if actual==line)
    );
    let mut value: Value = serde_json::from_slice(&bytes).unwrap();
    value["id_profile"] = json!("selected_v1");
    let wire = serde_json::to_vec(&value).unwrap();
    assert!(read_first_line(&mut wire.as_slice(), FirstLine::Start).is_err());
    for plain in ["01M72N8MG00000000000000F01", "legacy"] {
        let mut line = office();
        line.office_id = plain.into();
        let mut bytes = Vec::new();
        assert!(write_first_line(&mut bytes, &HostFirstLine::Start(line)).is_err());
        assert!(bytes.is_empty());
        let mut value: Value = serde_json::from_slice(
            &written(&HostFirstLine::Start(office()))
                .to_string()
                .into_bytes(),
        )
        .unwrap();
        value["office_id"] = json!(plain);
        let wire = serde_json::to_vec(&value).unwrap();
        assert!(
            read_first_line(&mut wire.as_slice(), FirstLine::Start).is_err(),
            "{plain}"
        );
    }
}

/// Every create is the selected HQ creation.
fn create() -> CreateLine {
    selected_create()
}

fn selected_create() -> CreateLine {
    let id = |n| format!("01900000-0000-7000-8000-{n:012x}");
    let prepared = org_proto::selected_bootstrap::PreparedV1 {
        plan: org_proto::selected_bootstrap::PlanV1 {
            version: 1,
            name: "Acme".into(),
            slug: "acme".into(),
            repository_remote_url: "https://example.test/org.git".into(),
            state_branch: "org/state".into(),
        },
        organization_id: id(1),
        office_id: id(2),
        repository_id: id(3),
        home_team_id: id(4),
        hr_employee_id: id(5),
        creator: org_proto::selected_bootstrap::CreatorV1::from_login(OWNER.into(), "Ana"),
    };
    CreateLine {
        office: OfficeLine {
            instance_id: INSTANCE.into(),
            office_id: prepared.office_id.clone(),
            rendezvous: RENDEZVOUS.into(),
        },
        org_id: prepared.organization_id.clone(),
        org_name: prepared.plan.name.clone(),
        office_name: "HQ".into(),
        role: OfficeRole::Hq,
        owner_member_id: OWNER.into(),
        hq_creation: HqCreationV1 {
            version: 2,
            operation_id: "01ARZ3NDEKTSV4RRFFQ69G5FAX".into(),
            escrow_profile: 1,
            bootstrap: Box::new(prepared),
        },
    }
}

/// Version 2 of the creation context names no path, and no launch line carries a writer
/// flag: every office requires custody, which the host holds, not the line.
#[test]
fn complete_selected_create_round_trips_without_a_path_or_writer_flag() {
    let expected = HostFirstLine::Create(selected_create());
    let mut wire = Vec::new();
    write_first_line(&mut wire, &expected).unwrap();
    let value = serde_json::from_slice::<Value>(&wire).unwrap();
    assert!(value.get("selected_writer_required").is_none());
    assert_eq!(value["hq_creation"]["version"], 2);
    assert!(value["hq_creation"].get("public_root").is_none());
    assert_eq!(
        read_first_line(&mut wire.as_slice(), FirstLine::Create).unwrap(),
        Some(expected)
    );
}

#[test]
fn selected_create_rejects_retired_fields_missing_bootstrap_unknown_and_mismatched_scope() {
    let original = written(&HostFirstLine::Create(selected_create()));
    for mutation in [
        "writer-flag",
        "writer-flag-false",
        "public-root",
        "version-1",
        "retired-profile",
        "ulid-organization",
        "no-bootstrap",
        "no-context",
        "organization",
        "office",
        "name",
        "unknown",
    ] {
        let mut v = original.clone();
        match mutation {
            "writer-flag" => v["selected_writer_required"] = json!(true),
            "writer-flag-false" => v["selected_writer_required"] = json!(false),
            "public-root" => v["hq_creation"]["public_root"] = json!("/private/tmp/state"),
            "version-1" => v["hq_creation"]["version"] = json!(1),
            "retired-profile" => v["id_profile"] = json!("selected_v1"),
            "ulid-organization" => {
                v["org_id"] = json!("01M72N8MG00000000000000G01");
                v["hq_creation"]["bootstrap"]["organization_id"] =
                    json!("01M72N8MG00000000000000G01");
            }
            "no-bootstrap" => {
                v["hq_creation"]
                    .as_object_mut()
                    .unwrap()
                    .remove("bootstrap");
            }
            "no-context" => {
                v.as_object_mut().unwrap().remove("hq_creation");
            }
            "organization" => {
                v["hq_creation"]["bootstrap"]["organization_id"] =
                    json!("01900000-0000-7000-8000-000000000099")
            }
            "office" => {
                v["hq_creation"]["bootstrap"]["office_id"] =
                    json!("01900000-0000-7000-8000-000000000099")
            }
            "name" => v["hq_creation"]["bootstrap"]["plan"]["name"] = json!("Other"),
            _ => v["unknown"] = json!(true),
        }
        let wire = serde_json::to_vec(&v).unwrap();
        assert!(
            read_first_line(&mut wire.as_slice(), FirstLine::Create).is_err(),
            "{mutation}"
        );
    }
    // The creator's person is the bound owner member, never another id.
    let mut other_owner = selected_create();
    other_owner.owner_member_id = "01900000-0000-7000-8000-0000000000ef".into();
    let mut wire = Vec::new();
    assert!(write_first_line(&mut wire, &HostFirstLine::Create(other_owner)).is_err());
    assert!(wire.is_empty());
    // Invalid in-memory preparation must also fail before emitting any bytes.
    let mut invalid = selected_create();
    invalid.hq_creation.bootstrap.creator.handle = "Not A Handle".into();
    let mut wire = Vec::new();
    assert!(write_first_line(&mut wire, &HostFirstLine::Create(invalid)).is_err());
    assert!(wire.is_empty());
}

fn written(line: &HostFirstLine) -> Value {
    let mut out = Vec::new();
    write_first_line(&mut out, line).unwrap();
    assert_eq!(out.last(), Some(&b'\n'));
    serde_json::from_slice(&out).unwrap()
}

fn keys(v: &Value) -> BTreeSet<String> {
    v.as_object().unwrap().keys().cloned().collect()
}

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|s| (*s).to_owned()).collect()
}

/// X-02 §4.1: the launch first-line types hold no secret. Every secret type is non-`Clone`, so a
/// `Clone` first line can't hold one; and the lines carry exactly these keys.
#[test]
fn first_lines_have_no_secret_fields() {
    fn clone_only<T: Clone>() {}
    clone_only::<HostFirstLine>();
    clone_only::<CreateLine>();
    clone_only::<OfficeLine>();

    let create_wire = written(&HostFirstLine::Create(create()));
    assert_eq!(
        create_wire,
        json!({
            "v": 1, "op": "create", "instance_id": INSTANCE, "office_id": OFFICE,
            "rendezvous": RENDEZVOUS, "org_id": ORG, "org_name": "Acme", "office_name": "HQ",
            "role": "hq", "owner_member_id": OWNER,
            "hq_creation": serde_json::to_value(create().hq_creation).unwrap()
        })
    );
    let common = ["v", "op", "instance_id", "office_id", "rendezvous"];
    for line in [
        HostFirstLine::Start(office()),
        HostFirstLine::Discard(office()),
    ] {
        assert_eq!(keys(&written(&line)), set(&common));
    }
    let secret_names = [
        "app_token",
        "db_key",
        "grant_key",
        "recovery_code",
        "key",
        "token",
    ];
    for line in [
        HostFirstLine::Create(create()),
        HostFirstLine::Start(office()),
        HostFirstLine::Discard(office()),
    ] {
        let k = keys(&written(&line));
        assert!(secret_names.iter().all(|s| !k.contains(*s)), "{k:?}");
    }
}

#[test]
fn first_lines_round_trip_and_match_the_flag() {
    for (line, flag) in [
        (HostFirstLine::Create(create()), FirstLine::Create),
        (HostFirstLine::Start(office()), FirstLine::Start),
        (HostFirstLine::Discard(office()), FirstLine::Discard),
    ] {
        let mut out = Vec::new();
        write_first_line(&mut out, &line).unwrap();
        let back = read_first_line(&mut Cursor::new(out.clone()), flag)
            .unwrap()
            .unwrap();
        assert_eq!(back, line);
        assert_eq!(back.office().unwrap().instance_id, INSTANCE);
        let other = if flag == FirstLine::Start {
            FirstLine::Create
        } else {
            FirstLine::Start
        };
        assert!(matches!(
            read_first_line(&mut Cursor::new(out), other),
            Err(LaunchError::Invalid(_))
        ));
    }
    assert!(
        read_first_line(&mut Cursor::new(Vec::new()), FirstLine::Start)
            .unwrap()
            .is_none()
    );
}

#[test]
fn the_reader_refuses_retired_launch_versions() {
    // Launch v2 (secrets on stdin) and the numbered v3 are gone (A1.5, A1.6): the only reader
    // refuses either line, keys included, as another version.
    for retired in [2, 3] {
        let line = format!(
            "{{\"v\":{retired},\"op\":\"start\",\"app_token\":\"{}\",\"db_key\":\"{}\"}}\n",
            "01".repeat(32),
            "02".repeat(32)
        )
        .into_bytes();
        let err = read_first_line(&mut Cursor::new(line), FirstLine::Start).unwrap_err();
        assert!(matches!(
            err,
            LaunchError::UnsupportedVersion { found, .. } if found == retired
        ));
        // The reader names the version it speaks.
        assert_eq!(
            err.to_string(),
            format!("launch version {retired} isn't supported (this build speaks 1)")
        );
    }
}

#[test]
fn first_lines_check_their_rules_both_ways() {
    let mut bad = Vec::new();
    let mut not_plain = office();
    not_plain.office_id = "../x".into();
    bad.push(HostFirstLine::Start(not_plain));
    let mut relative = office();
    relative.rendezvous = "r/0123".into();
    bad.push(HostFirstLine::Discard(relative));
    let mut nul = office();
    nul.rendezvous = "/r/\0".into();
    bad.push(HostFirstLine::Start(nul));
    let mut satellite = create();
    satellite.role = OfficeRole::Satellite;
    bad.push(HostFirstLine::Create(satellite));
    let mut owner = create();
    owner.owner_member_id = "owner".into();
    bad.push(HostFirstLine::Create(owner));
    let mut org = create();
    org.org_id = String::new();
    bad.push(HostFirstLine::Create(org));
    bad.push(HostFirstLine::Unknown);
    for line in &bad {
        assert!(
            matches!(
                write_first_line(&mut Vec::new(), line),
                Err(LaunchError::Invalid(_))
            ),
            "{line:?}"
        );
    }
    // The same rules on the way in.
    for wire in [
        json!({"v": 1, "op": "start", "instance_id": INSTANCE, "office_id": "a/b", "rendezvous": RENDEZVOUS}),
        json!({"v": 1, "op": "start", "instance_id": INSTANCE, "office_id": OFFICE, "rendezvous": "rel"}),
        json!({"v": 1, "op": "start", "office_id": OFFICE, "rendezvous": RENDEZVOUS}),
        json!({"v": 1, "op": "create", "instance_id": INSTANCE, "office_id": OFFICE,
               "rendezvous": RENDEZVOUS, "org_id": ORG, "org_name": "Acme", "office_name": "HQ",
               "role": "satellite", "owner_member_id": OWNER}),
        json!({"v": 1, "op": "create", "instance_id": INSTANCE, "office_id": OFFICE,
               "rendezvous": RENDEZVOUS, "org_id": ORG, "org_name": "Acme", "office_name": "HQ",
               "role": "hq"}),
        // A field the line doesn't define, a v2 key above all: the first line holds no secret.
        json!({"v": 1, "op": "start", "instance_id": INSTANCE, "office_id": OFFICE,
               "rendezvous": RENDEZVOUS, "db_key": "01".repeat(32)}),
        json!({"v": 1, "op": "discard", "instance_id": INSTANCE, "office_id": OFFICE,
               "rendezvous": RENDEZVOUS, "db_key": null}),
        json!({"v": 1, "op": "create", "instance_id": INSTANCE, "office_id": OFFICE,
               "rendezvous": RENDEZVOUS, "org_id": ORG, "org_name": "Acme", "office_name": "HQ",
               "role": "hq", "owner_member_id": OWNER, "db_key": "01".repeat(32)}),
        json!({"v": 1, "op": "start", "instance_id": INSTANCE, "office_id": OFFICE,
               "rendezvous": RENDEZVOUS, "extra": 1}),
    ] {
        let mut line = serde_json::to_vec(&wire).unwrap();
        line.push(b'\n');
        let flag = match wire["op"].as_str() {
            Some("create") => FirstLine::Create,
            Some("discard") => FirstLine::Discard,
            _ => FirstLine::Start,
        };
        assert!(
            read_first_line(&mut Cursor::new(line), flag).is_err(),
            "{wire}"
        );
    }
    let unknown = b"{\"v\":1,\"op\":\"join\"}\n".to_vec();
    assert!(matches!(
        read_first_line(&mut Cursor::new(unknown), FirstLine::Create),
        Err(LaunchError::Invalid(_))
    ));
}

#[test]
fn ready_for_secrets_has_its_wire_shape() {
    let wire = serde_json::to_value(ReadyForSecrets).unwrap();
    assert_eq!(
        wire,
        json!({"v": 1, "event": "ready_for_secrets", "channel_profile": CHANNEL_PROFILE})
    );
    assert_eq!(
        serde_json::from_value::<ReadyForSecrets>(wire).unwrap(),
        ReadyForSecrets
    );
    for bad in [
        json!({"v": 2, "event": "ready_for_secrets"}),
        json!({"v": 1, "event": "ready"}),
        json!({"event": "ready_for_secrets"}),
        json!({"v": 1, "event": "ready_for_secrets", "db_key": "01"}),
    ] {
        assert!(serde_json::from_value::<ReadyForSecrets>(bad).is_err());
    }
}

/// A transfer record carries exactly its fields: an extra key is refused, not dropped.
#[test]
fn transfer_records_refuse_unknown_fields() {
    let revoked = json!({"type": "revoked", "session_ids": []});
    serde_json::from_value::<TransferRecord>(revoked.clone()).unwrap();
    let mut extra = revoked;
    extra["fd"] = json!(1);
    assert!(serde_json::from_value::<TransferRecord>(extra).is_err());
    let channel = json!({"type": "channel", "kind": "web", "office_id": "o", "channel_id": "c"});
    serde_json::from_value::<TransferRecord>(channel.clone()).unwrap();
    let mut extra = channel;
    extra["grant_key"] = json!("secret");
    assert!(serde_json::from_value::<TransferRecord>(extra).is_err());
}

const KEY_HEX: &str = "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";
const GRANT_HEX: &str = "efefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefef";

fn line_value(bytes: &[u8]) -> Value {
    assert_eq!(bytes.last(), Some(&b'\n'));
    serde_json::from_slice(&bytes[..bytes.len() - 1]).unwrap()
}

#[test]
fn handoffs_carry_the_keys_and_name_their_descriptors() {
    let create = Handoff::Create {
        office_secrets_key: OfficeSecretsKey::from_bytes([0xcd; 32]),
        grant_key: GrantKey::from_bytes([0xef; 32]),
        recovery_code: Some(RecoveryCode::new("ABCD-EFGH-IJKL".into()).unwrap()),
        fds: vec![HandoffFd::Transfer],
    };
    let bytes = encode_handoff(&create).unwrap();
    assert_eq!(
        line_value(&bytes),
        json!({"v": 1, "op": "create", "office_secrets_key": KEY_HEX, "grant_key": GRANT_HEX,
               "recovery_code": "ABCD-EFGH-IJKL", "fds": ["transfer"]})
    );
    let back = decode_handoff(&bytes[..bytes.len() - 1]).unwrap();
    assert_eq!(back.fds(), &[HandoffFd::Transfer]);
    let Handoff::Create {
        office_secrets_key,
        grant_key,
        recovery_code: Some(recovery_code),
        ..
    } = &back
    else {
        panic!("{back:?}");
    };
    assert_eq!(office_secrets_key.expose_bytes(), &[0xcd; 32]);
    assert_eq!(grant_key.expose_bytes(), &[0xef; 32]);
    assert_eq!(recovery_code.expose(), "ABCD-EFGH-IJKL");
    // Debug shows no secret.
    let printed = format!("{back:?}");
    assert!(!printed.contains("cdcd") && !printed.contains("efef") && !printed.contains("ABCD"));

    // A create without a recovery code leaves the field out (never `null`) and decodes back.
    let bare = Handoff::Create {
        office_secrets_key: OfficeSecretsKey::from_bytes([0xcd; 32]),
        grant_key: GrantKey::from_bytes([0xef; 32]),
        recovery_code: None,
        fds: vec![HandoffFd::Transfer],
    };
    let bytes = encode_handoff(&bare).unwrap();
    assert_eq!(
        line_value(&bytes),
        json!({"v": 1, "op": "create", "office_secrets_key": KEY_HEX, "grant_key": GRANT_HEX,
               "fds": ["transfer"]})
    );
    assert!(matches!(
        decode_handoff(&bytes[..bytes.len() - 1]).unwrap(),
        Handoff::Create {
            recovery_code: None,
            ..
        }
    ));

    let start = Handoff::Start {
        office_secrets_key: Some(OfficeSecretsKey::from_bytes([0xcd; 32])),
        grant_key: GrantKey::from_bytes([0xef; 32]),
        fds: vec![HandoffFd::Transfer],
    };
    assert_eq!(
        line_value(&encode_handoff(&start).unwrap()),
        json!({"v": 1, "op": "start", "office_secrets_key": KEY_HEX, "grant_key": GRANT_HEX,
               "fds": ["transfer"]})
    );
    // A start without the key carries `null` (present, not missing) and proceeds.
    let keyless = Handoff::Start {
        office_secrets_key: None,
        grant_key: GrantKey::from_bytes([0xef; 32]),
        fds: vec![HandoffFd::Transfer],
    };
    let bytes = encode_handoff(&keyless).unwrap();
    assert_eq!(
        line_value(&bytes),
        json!({"v": 1, "op": "start", "office_secrets_key": null, "grant_key": GRANT_HEX,
               "fds": ["transfer"]})
    );
    assert!(matches!(
        decode_handoff(&bytes[..bytes.len() - 1]).unwrap(),
        Handoff::Start {
            office_secrets_key: None,
            ..
        }
    ));
    let missing = format!(r#"{{"v":1,"op":"start","grant_key":"{GRANT_HEX}","fds":["transfer"]}}"#);
    assert!(decode_handoff(missing.as_bytes()).is_err());

    // A discard gets no transfer socket and no secret.
    let discard = Handoff::Discard { fds: vec![] };
    assert_eq!(
        line_value(&encode_handoff(&discard).unwrap()),
        json!({"v": 1, "op": "discard", "fds": []})
    );
    assert!(decode_handoff(br#"{"v":1,"op":"discard","fds":[]}"#).is_ok());
    assert!(
        decode_handoff(br#"{"v":1,"op":"discard","office_secrets_key":null,"fds":[]}"#).is_err()
    );
}

#[test]
fn the_borrowing_encoders_match_the_owned_handoffs() {
    let discard = encode_discard_handoff().unwrap();
    assert_eq!(
        line_value(&discard),
        json!({"v": 1, "op": "discard", "fds": []})
    );
    assert_eq!(
        discard,
        encode_handoff(&Handoff::Discard { fds: vec![] }).unwrap()
    );

    let key = OfficeSecretsKey::from_bytes([0xcd; 32]);
    let grant = GrantKey::from_bytes([0xef; 32]);
    for borrowed in [Some(&key), None] {
        let line = encode_start_handoff(borrowed, &grant).unwrap();
        assert_eq!(
            line,
            encode_handoff(&Handoff::Start {
                office_secrets_key: borrowed.map(|_| OfficeSecretsKey::from_bytes([0xcd; 32])),
                grant_key: GrantKey::from_bytes([0xef; 32]),
                fds: vec![HandoffFd::Transfer],
            })
            .unwrap()
        );
        let Handoff::Start {
            office_secrets_key, ..
        } = decode_handoff(&line[..line.len() - 1]).unwrap()
        else {
            panic!("not a start");
        };
        assert_eq!(
            office_secrets_key.map(|k| *k.expose_bytes()),
            borrowed.map(|k| *k.expose_bytes())
        );
    }

    let code = RecoveryCode::new("ABCD-EFGH-IJKL".into()).unwrap();
    assert_eq!(
        encode_create_handoff(&key, &grant, &code).unwrap(),
        encode_handoff(&Handoff::Create {
            office_secrets_key: OfficeSecretsKey::from_bytes([0xcd; 32]),
            grant_key: GrantKey::from_bytes([0xef; 32]),
            recovery_code: Some(RecoveryCode::new("ABCD-EFGH-IJKL".into()).unwrap()),
            fds: vec![HandoffFd::Transfer],
        })
        .unwrap()
    );
}

#[test]
fn a_handoff_naming_the_wrong_descriptors_or_version_is_refused() {
    let wrong = Handoff::Start {
        office_secrets_key: Some(OfficeSecretsKey::from_bytes([1; 32])),
        grant_key: GrantKey::from_bytes([2; 32]),
        fds: vec![],
    };
    assert!(matches!(
        encode_handoff(&wrong),
        Err(LaunchError::Invalid(_))
    ));
    let k = "01".repeat(32);
    for (line, version) in [
        (
            r#"{"v":1,"op":"discard","fds":["transfer"]}"#.to_owned(),
            false,
        ),
        (
            format!(
                r#"{{"v":1,"op":"start","office_secrets_key":"{k}","grant_key":"{k}","fds":["transfer","transfer"]}}"#
            ),
            false,
        ),
        (
            format!(
                r#"{{"v":1,"op":"start","office_secrets_key":"{k}","grant_key":"{k}","fds":["listener"]}}"#
            ),
            false,
        ),
        (
            format!(
                r#"{{"v":2,"op":"start","office_secrets_key":"{k}","grant_key":"{k}","fds":["transfer"]}}"#
            ),
            true,
        ),
    ] {
        let result = decode_handoff(line.as_bytes());
        if version {
            assert!(matches!(
                result,
                Err(LaunchError::UnsupportedVersion { found: 2, .. })
            ));
        } else {
            assert!(result.is_err(), "{line}");
        }
    }
    // The retired contract version 3, in its own shape or this one, is refused as a version.
    for line in [
        format!(r#"{{"v":3,"op":"start","db_key":"{k}","grant_key":"{k}","fds":["transfer"]}}"#),
        format!(
            r#"{{"v":3,"op":"start","office_secrets_key":"{k}","grant_key":"{k}","fds":["transfer"]}}"#
        ),
        r#"{"v":3,"op":"discard","fds":[]}"#.to_owned(),
    ] {
        assert!(
            matches!(
                decode_handoff(line.as_bytes()),
                Err(LaunchError::UnsupportedVersion { found: 3, .. })
            ),
            "{line}"
        );
    }
}

#[test]
fn transfer_records_match_the_vectors_and_the_cap() {
    let doc: Value = serde_json::from_str(include_str!(
        "../../test-vectors/launch/transfer-records.json"
    ))
    .unwrap();
    assert_eq!(doc["header_max_bytes"], TRANSFER_HEADER_MAX_BYTES);
    for case in doc["records"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let wire = case["wire"].as_str().unwrap();
        let record = decode_transfer_record(wire.as_bytes()).unwrap();
        assert_eq!(
            record.fd_count() as u64,
            case["fd_count"].as_u64().unwrap(),
            "{name}"
        );
        assert_eq!(
            encode_transfer_record(&record).unwrap(),
            wire.as_bytes(),
            "{name}"
        );
        if let Some(bytes) = case.get("bytes") {
            assert_eq!(wire.len() as u64, bytes.as_u64().unwrap(), "{name}");
        }
    }
    for case in doc["refused"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let wire = case["wire"].as_str().unwrap();
        let result = decode_transfer_record(wire.as_bytes());
        if case["reason"] == "too_large" {
            assert!(
                matches!(result, Err(LaunchError::Frame(FrameError::TooLarge))),
                "{name}"
            );
            // And the sender refuses to write it: the record decodes without the cap.
            let record: TransferRecord = serde_json::from_str(wire).unwrap();
            assert!(
                matches!(
                    encode_transfer_record(&record),
                    Err(LaunchError::Frame(FrameError::TooLargeToSend))
                ),
                "{name}"
            );
        } else {
            assert!(matches!(result, Err(LaunchError::Json(_))), "{name}");
        }
    }
}

#[test]
fn the_launch_constants() {
    assert_eq!(LAUNCH_VERSION, 1);
    assert_eq!(TRANSFER_HEADER_MAX_BYTES, 1024);
    assert_eq!(TRANSFER_SOCKET_BUFFER_BYTES, 64 * 1024);
}

#[test]
fn a_handoff_carries_exactly_the_keys_of_its_op() {
    let k = "01".repeat(32);
    for line in [
        // A start with a recovery code, a discard with any secret, an unknown op.
        format!(
            r#"{{"v":1,"op":"start","office_secrets_key":"{k}","grant_key":"{k}","recovery_code":"c","fds":["transfer"]}}"#
        ),
        // A field the op doesn't carry is refused even as null.
        format!(
            r#"{{"v":1,"op":"start","office_secrets_key":"{k}","grant_key":"{k}","recovery_code":null,"fds":["transfer"]}}"#
        ),
        r#"{"v":1,"op":"discard","grant_key":null,"fds":[]}"#.to_owned(),
        r#"{"v":1,"op":"discard","recovery_code":null,"fds":[]}"#.to_owned(),
        format!(r#"{{"v":1,"op":"discard","office_secrets_key":"{k}","fds":[]}}"#),
        format!(r#"{{"v":1,"op":"discard","grant_key":"{k}","fds":[]}}"#),
        // A secret the op requires can't be null or missing; an optional recovery code is
        // left out, never null.
        format!(
            r#"{{"v":1,"op":"create","office_secrets_key":"{k}","grant_key":null,"recovery_code":"c","fds":["transfer"]}}"#
        ),
        format!(
            r#"{{"v":1,"op":"create","office_secrets_key":null,"grant_key":"{k}","recovery_code":"c","fds":["transfer"]}}"#
        ),
        format!(
            r#"{{"v":1,"op":"create","grant_key":"{k}","recovery_code":"c","fds":["transfer"]}}"#
        ),
        format!(
            r#"{{"v":1,"op":"create","office_secrets_key":"{k}","grant_key":"{k}","recovery_code":null,"fds":["transfer"]}}"#
        ),
        format!(
            r#"{{"v":1,"op":"start","office_secrets_key":"{k}","grant_key":null,"fds":["transfer"]}}"#
        ),
        format!(r#"{{"v":1,"op":"start","office_secrets_key":"{k}","fds":["transfer"]}}"#),
        format!(r#"{{"v":1,"op":"join","office_secrets_key":"{k}","fds":[]}}"#),
        // A field the handoff doesn't define, a retired one above all.
        format!(
            r#"{{"v":1,"op":"start","office_secrets_key":"{k}","grant_key":"{k}","fds":["transfer"],"app_token":"{k}"}}"#
        ),
        format!(r#"{{"v":1,"op":"start","db_key":"{k}","grant_key":"{k}","fds":["transfer"]}}"#),
        r#"{"v":1,"op":"discard","db_key":null,"fds":[]}"#.to_owned(),
        format!(r#"{{"v":1,"op":"discard","fds":[],"app_token":"{k}"}}"#),
        format!(
            r#"{{"v":1,"op":"create","office_secrets_key":"{k}","grant_key":"{k}","recovery_code":"c","fds":["transfer"],"extra":1}}"#
        ),
    ] {
        let error = decode_handoff(line.as_bytes()).unwrap_err().to_string();
        assert!(!error.contains("0101"), "{error}");
    }
    for ok in [
        format!(
            r#"{{"v":1,"op":"create","office_secrets_key":"{k}","grant_key":"{k}","recovery_code":"c","fds":["transfer"]}}"#
        ),
        format!(
            r#"{{"v":1,"op":"create","office_secrets_key":"{k}","grant_key":"{k}","fds":["transfer"]}}"#
        ),
    ] {
        assert!(matches!(
            decode_handoff(ok.as_bytes()).unwrap(),
            Handoff::Create { .. }
        ));
    }
}

#[test]
fn worker_events_on_the_rendezvous_are_launch_lines() {
    let ready = ServerEvent::Ready {
        socket: "/run/o.sock".into(),
        pid: 42,
    };
    let mut out = Vec::new();
    write_server_event(&mut out, &ready).unwrap();
    assert_eq!(
        line_value(&out),
        json!({"v": 1, "event": "ready", "socket": "/run/o.sock", "pid": 42})
    );
    assert_eq!(
        read_server_event(&mut Cursor::new(out)).unwrap(),
        Some(RendezvousEvent::Server(ready.clone()))
    );
    let v2 = b"{\"v\":2,\"event\":\"ready\",\"socket\":\"/run/o.sock\",\"pid\":42}\n".to_vec();
    assert!(matches!(
        read_server_event(&mut Cursor::new(v2)),
        Err(LaunchError::UnsupportedVersion { found: 2, .. })
    ));
    assert!(write_server_event(&mut Vec::new(), &ServerEvent::Unknown).is_err());
}

#[test]
fn worker_events_refuse_unknown_fields() {
    let read = |v: serde_json::Value| {
        let mut line = serde_json::to_vec(&v).unwrap();
        line.push(b'\n');
        read_server_event(&mut Cursor::new(line))
    };
    for (good, field) in [
        (
            json!({"v": 1, "event": "ready", "socket": "/s", "pid": 1}),
            "db_key",
        ),
        (json!({"v": 1, "event": "discarded"}), "extra"),
        (
            json!({"v": 1, "event": "failed", "code": "invalid_request", "message": "m"}),
            "extra",
        ),
    ] {
        assert!(read(good.clone()).unwrap().is_some(), "{good}");
        let mut bad = good;
        bad[field] = json!("x");
        assert!(
            matches!(read(bad.clone()), Err(LaunchError::Json(_))),
            "{bad}"
        );
    }
    // `created`: refused with an extra field on the line; the nested `office` keeps
    // `office.json`'s own rule, so a field this build doesn't know there is kept.
    let created = ServerEvent::Created {
        office: crate::OfficeFile::new(
            "01M6WGY1WJCC503RXF2NS0SH84",
            "HQ",
            crate::OfficeRole::Hq,
            "01M6WGY1WJCC503RXF2NS0SH83",
            "Acme",
            "2026-11-08T02:10:00.000Z",
        ),
    };
    let mut out = Vec::new();
    write_server_event(&mut out, &created).unwrap();
    let good = line_value(&out);
    assert_eq!(
        read(good.clone()).unwrap(),
        Some(RendezvousEvent::Server(created))
    );
    let mut bad = good.clone();
    bad["db_key"] = json!("x");
    assert!(matches!(read(bad), Err(LaunchError::Json(_))));
    let mut newer = good;
    newer["office"]["later"] = json!(1);
    assert!(matches!(
        read(newer).unwrap(),
        Some(RendezvousEvent::Server(ServerEvent::Created { .. }))
    ));
    // An unknown event name is still ignorable, whatever it carries.
    assert_eq!(
        read(json!({"v": 1, "event": "later", "anything": 1})).unwrap(),
        Some(RendezvousEvent::Server(ServerEvent::Unknown))
    );
    // `v` is required on a launch line.
    assert!(read(json!({"event": "discarded"})).is_err());
}

#[test]
fn the_launch_reader_refuses_any_v2_line_as_a_version() {
    // Whatever its op or shape, a v2 line is a version mismatch, not a bad request.
    for line in [
        b"{\"v\":2,\"op\":\"discard\",\"db_key\":null}\n".to_vec(),
        b"{\"v\":2,\"op\":\"create\",\"role\":\"satellite\"}\n".to_vec(),
        b"{\"v\":2,\"op\":\"start\"}\n".to_vec(),
    ] {
        assert!(matches!(
            read_first_line(&mut Cursor::new(line), FirstLine::Start),
            Err(LaunchError::UnsupportedVersion { found: 2, .. })
        ));
    }
}

#[test]
fn fd_receipts_are_recognized_as_launch_events() {
    for of in ["handoff", "01ARZ3NDEKTSV4RRFFQ69G5FAV"] {
        let wire = format!("{{\"v\":1,\"event\":\"fds_received\",\"of\":\"{of}\"}}\n");
        let decoded = read_server_event(&mut Cursor::new(wire)).unwrap().unwrap();
        assert_eq!(
            serde_json::to_value(decoded).ok(),
            Some(json!({"event":"fds_received", "of":of})),
            "receipt must not be ignored as an unknown event"
        );
    }
}

#[test]
fn rendezvous_receipt_vectors_pin_encoding_decoding_and_strict_fields() {
    let vectors: Value =
        serde_json::from_str(include_str!("../../test-vectors/launch/worker-events.json")).unwrap();
    assert_eq!(vectors["schema_version"], 1);
    let events = vectors["events"].as_array().unwrap();
    let invalid = vectors["invalid"].as_array().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(invalid.len(), 10);
    for case in events {
        let expected = RendezvousEvent::FdsReceived {
            of: case["of"].as_str().unwrap().into(),
        };
        let wire = case["wire"].as_str().unwrap();
        assert_eq!(
            read_server_event(&mut Cursor::new(wire)).unwrap(),
            Some(expected.clone())
        );
        let mut encoded = Vec::new();
        write_rendezvous_event(&mut encoded, &expected).unwrap();
        assert_eq!(encoded, wire.as_bytes());
    }
    for case in invalid {
        let error =
            read_server_event(&mut Cursor::new(case["wire"].as_str().unwrap())).unwrap_err();
        assert!(
            matches!(
                (case["error"].as_str(), &error),
                (Some("json"), LaunchError::Json(_))
                    | (
                        Some("unsupported"),
                        LaunchError::UnsupportedVersion { found: 2, .. }
                    )
            ),
            "{}: {error:?}",
            case["name"]
        );
    }
    // Version refusal must precede decoding fields, including malformed receipt fields.
    assert!(matches!(
        read_server_event(&mut Cursor::new(
            b"{\"v\":4,\"event\":\"fds_received\",\"extra\":null}\n"
        )),
        Err(LaunchError::UnsupportedVersion { found: 4, .. })
    ));
    // The retired contract version 3 is refused as a version, whatever the event.
    for wire in [
        &b"{\"v\":3,\"event\":\"fds_received\",\"of\":\"handoff\"}\n"[..],
        &b"{\"v\":3,\"event\":\"ready\",\"socket\":\"/run/o.sock\",\"pid\":42}\n"[..],
    ] {
        assert!(matches!(
            read_server_event(&mut Cursor::new(wire)),
            Err(LaunchError::UnsupportedVersion { found: 3, .. })
        ));
    }
}

#[test]
fn rendezvous_writer_preserves_ordinary_event_validation() {
    let office = crate::OfficeFile::new(
        FILE_OFFICE,
        "HQ",
        OfficeRole::Hq,
        FILE_ORG,
        "Acme",
        "2026-11-08T02:10:00.000Z",
    );
    let mut bad_office = office.clone();
    bad_office.office_id = "bad/id".into();
    for event in [
        ServerEvent::Unknown,
        ServerEvent::Created { office: bad_office },
    ] {
        let mut encoded = Vec::new();
        assert!(write_rendezvous_event(&mut encoded, &RendezvousEvent::Server(event)).is_err());
        assert!(encoded.is_empty());
    }
    for event in [
        ServerEvent::Created { office },
        ServerEvent::Ready {
            socket: "/run/o.sock".into(),
            pid: 42,
        },
        ServerEvent::Discarded,
        ServerEvent::Failed {
            code: crate::FailCode::StorageError,
            message: "fixed message".into(),
        },
    ] {
        let mut encoded = Vec::new();
        write_rendezvous_event(&mut encoded, &RendezvousEvent::Server(event.clone())).unwrap();
        assert_eq!(
            read_server_event(&mut Cursor::new(encoded)).unwrap(),
            Some(RendezvousEvent::Server(event))
        );
    }
}

#[test]
fn rendezvous_receipts_keep_the_frame_size_limit() {
    let of = "x".repeat(org_proto::MAX_FRAME_BYTES);
    let mut encoded = Vec::new();
    assert!(matches!(
        write_rendezvous_event(
            &mut encoded,
            &RendezvousEvent::FdsReceived { of: of.clone() }
        ),
        Err(LaunchError::Frame(FrameError::TooLargeToSend))
    ));
    assert!(encoded.is_empty());
    let wire = format!("{{\"v\":1,\"event\":\"fds_received\",\"of\":\"{of}\"}}\n");
    assert!(matches!(
        read_server_event(&mut Cursor::new(wire)),
        Err(LaunchError::Frame(FrameError::TooLarge))
    ));
}

#[test]
fn rendezvous_events_keep_the_office_file_size_limit() {
    let office = crate::OfficeFile::new(
        FILE_OFFICE,
        "HQ",
        OfficeRole::Hq,
        FILE_ORG,
        "Acme",
        "2026-11-08T02:10:00.000Z",
    );
    for size in [
        crate::MAX_OFFICE_FILE_BYTES,
        crate::MAX_OFFICE_FILE_BYTES + 1,
    ] {
        // The forward-compatible extra field fits compact input but its pretty office.json
        // reaches the exact file cap. A generic frame writer alone cannot enforce this rule.
        let mut value = serde_json::to_value(&office).unwrap();
        value["pad"] = json!("");
        let base = serde_json::to_vec_pretty(&value).unwrap().len() + 1;
        value["pad"] = json!("x".repeat(size - base));
        let compact = serde_json::to_vec(&value).unwrap();
        assert!(compact.len() <= crate::MAX_OFFICE_FILE_BYTES);
        let padded = crate::OfficeFile::from_slice(&compact).unwrap();
        let event = RendezvousEvent::Server(ServerEvent::Created { office: padded });
        let mut encoded = Vec::new();
        let written = write_rendezvous_event(&mut encoded, &event);
        let wire = json!({"v":1, "event":"created", "office":value}).to_string() + "\n";
        let read = read_server_event(&mut Cursor::new(wire));
        if size == crate::MAX_OFFICE_FILE_BYTES {
            assert!(written.is_ok());
            assert_eq!(read.unwrap(), Some(event));
        } else {
            assert!(matches!(written, Err(LaunchError::Invalid(_))));
            assert!(encoded.is_empty());
            assert!(matches!(read, Err(LaunchError::Invalid(_))));
        }
    }
}

/// A start line has no writer flag: one that carries it, either value, is refused.
#[test]
fn a_start_line_with_the_retired_writer_flag_is_refused() {
    let line = HostFirstLine::Start(office());
    let mut bytes = Vec::new();
    write_first_line(&mut bytes, &line).unwrap();
    assert_eq!(
        read_first_line(&mut Cursor::new(&bytes), FirstLine::Start).unwrap(),
        Some(line)
    );
    let mut value: Value = serde_json::from_slice(&bytes).unwrap();
    for flag in [true, false] {
        value["selected_writer_required"] = json!(flag);
        let wire = serde_json::to_vec(&value).unwrap();
        assert!(read_first_line(&mut wire.as_slice(), FirstLine::Start).is_err());
    }
}

#[test]
fn hq_creation_context_is_explicit_versioned_and_strictly_decoded_from_raw_bytes() {
    let mut creation = create();
    let line = HostFirstLine::Create(creation.clone());
    let mut bytes = Vec::new();
    write_first_line(&mut bytes, &line).unwrap();
    assert_eq!(
        read_first_line(&mut Cursor::new(&bytes), FirstLine::Create).unwrap(),
        Some(line)
    );
    let text = String::from_utf8(bytes).unwrap();
    for bad in [
        text.replace("\"version\":2", "\"version\":1"),
        text.replace("\"version\":2", "\"version\":3"),
        text.replace(
            "\"version\":2",
            "\"version\":2,\"public_root\":\"/tmp/test-state\"",
        ),
        text.replace("\"escrow_profile\":1", "\"escrow_profile\":2"),
        text.replace("\"version\":2", "\"version\":2,\"version\":2"),
        text.replace("\"version\":2", "\"version\":2,\"ver\\u0073ion\":2"),
        text.replace(
            "\"escrow_profile\":1",
            "\"escrow_profile\":1,\"unknown\":true",
        ),
        text.replace(
            "\"hq_creation\":{",
            "\"hq_creation\":null,\"hq_creation\":{",
        ),
    ] {
        assert!(read_first_line(&mut Cursor::new(bad), FirstLine::Create).is_err());
    }
    creation.hq_creation.version = 1;
    let mut out = Vec::new();
    assert!(write_first_line(&mut out, &HostFirstLine::Create(creation)).is_err());
    assert!(out.is_empty());
}

#[test]
fn channel_profile_is_required_before_secret_handoff() {
    let ready = serde_json::to_value(ReadyForSecrets).unwrap();
    assert_eq!(ready["channel_profile"], "office-channels/1");
    for raw in [
        r#"{"v":1,"event":"ready_for_secrets"}"#,
        r#"{"v":1,"event":"ready_for_secrets","channel_profile":"other"}"#,
        r#"{"v":1,"event":"ready_for_secrets","channel_profile":"office-channels/1","channel_profile":"office-channels/1"}"#,
    ] {
        assert!(serde_json::from_str::<ReadyForSecrets>(raw).is_err());
    }
}

#[test]
fn lifecycle_events_keep_exact_strict_fields_and_refusal_reason() {
    for event in [
        RendezvousEvent::ChannelClosed { of: "c".into() },
        RendezvousEvent::FdsRefused {
            of: "c".into(),
            reason: RefusalReason::Capacity,
        },
    ] {
        let mut bytes = Vec::new();
        write_rendezvous_event(&mut bytes, &event).unwrap();
        assert_eq!(
            read_server_event(&mut Cursor::new(bytes)).unwrap(),
            Some(event)
        );
    }
    for wire in [
        r#"{"v":1,"event":"channel_closed","of":"c","reason":"capacity"}"#,
        r#"{"v":1,"event":"channel_closed","of":"c","of":"d"}"#,
        r#"{"v":1,"event":"fds_refused","of":"c"}"#,
        r#"{"v":1,"event":"fds_refused","of":"c","reason":"unknown"}"#,
        r#"{"v":1,"event":"fds_refused","of":"c","reason":"capacity","extra":0}"#,
    ] {
        assert!(
            read_server_event(&mut Cursor::new(format!("{wire}\n"))).is_err(),
            "{wire}"
        );
    }
}

/// The instance id is canonical lowercase UUIDv7 text: a ULID (the earlier form) and uppercase
/// UUIDv7 are refused on the way out and on the way in.
#[test]
fn first_lines_refuse_a_ulid_or_uppercase_instance() {
    for instance in [
        "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "0190A5D2-7C3E-7A01-8B2C-3D4E5F6071A1",
    ] {
        let mut line = office();
        line.instance_id = instance.into();
        assert!(
            matches!(
                write_first_line(&mut Vec::new(), &HostFirstLine::Start(line)),
                Err(LaunchError::Invalid(_))
            ),
            "{instance}"
        );
        let wire = json!({"v": 1, "op": "start", "instance_id": instance,
            "office_id": OFFICE, "rendezvous": RENDEZVOUS});
        let bytes = format!("{wire}\n");
        assert!(
            read_first_line(&mut bytes.as_bytes(), FirstLine::Start).is_err(),
            "{instance}"
        );
    }
}
