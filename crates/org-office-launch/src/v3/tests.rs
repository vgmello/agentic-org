use std::collections::BTreeSet;
use std::io::Cursor;

use serde_json::{json, Value};

use super::*;
use crate::{read_first_host_message, write_host_message, HostMessage, ServerEvent, StartOffice};

const INSTANCE: &str = "01M72N8MG00000000000000I01";
const OFFICE: &str = "01M72N8MG00000000000000F01";
const ORG: &str = "01M72N8MG00000000000000G01";
const OWNER: &str = "01M72N8MG00000000000000M01";
const RENDEZVOUS: &str = "/run/user/501/agentic-org/r/0123456789abcdef";

fn office() -> OfficeLine {
    OfficeLine {
        instance_id: INSTANCE.into(),
        office_id: OFFICE.into(),
        rendezvous: RENDEZVOUS.into(),
    }
}

fn create() -> CreateLine {
    CreateLine {
        office: office(),
        org_id: ORG.into(),
        org_name: "Acme".into(),
        office_name: "HQ".into(),
        role: OfficeRole::Hq,
        owner_member_id: OWNER.into(),
    }
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

/// X-02 §4.1: the v3 first-line types hold no secret. Every secret type is non-`Clone`, so a
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
            "v": 3, "op": "create", "instance_id": INSTANCE, "office_id": OFFICE,
            "rendezvous": RENDEZVOUS, "org_id": ORG, "org_name": "Acme", "office_name": "HQ",
            "role": "hq", "owner_member_id": OWNER
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
fn v2_and_v3_refuse_each_others_lines() {
    // A v2 reader refuses a v3 line as another version, and the other way round.
    let mut v3 = Vec::new();
    write_first_line(&mut v3, &HostFirstLine::Start(office())).unwrap();
    assert!(matches!(
        read_first_host_message(&mut Cursor::new(v3), FirstLine::Start),
        Err(LaunchError::UnsupportedVersion { found: 3, .. })
    ));
    let mut v2 = Vec::new();
    write_host_message(
        &mut v2,
        &HostMessage::Start(StartOffice {
            app_token: org_proto::AppToken::from_bytes([1; 32]),
            db_key: DbKey::from_bytes([2; 32]),
        }),
    )
    .unwrap();
    let err = read_first_line(&mut Cursor::new(v2), FirstLine::Start).unwrap_err();
    assert!(matches!(
        err,
        LaunchError::UnsupportedVersion { found: 2, .. }
    ));
    // Each reader names the version it speaks, not a fixed one.
    assert_eq!(
        err.to_string(),
        "launch version 2 isn't supported (this build speaks 3)"
    );
    let mut v3 = Vec::new();
    write_first_line(&mut v3, &HostFirstLine::Start(office())).unwrap();
    assert_eq!(
        read_first_host_message(&mut Cursor::new(v3), FirstLine::Start)
            .unwrap_err()
            .to_string(),
        "launch version 3 isn't supported (this build speaks 2)"
    );
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
        json!({"v": 3, "op": "start", "instance_id": INSTANCE, "office_id": "a/b", "rendezvous": RENDEZVOUS}),
        json!({"v": 3, "op": "start", "instance_id": INSTANCE, "office_id": OFFICE, "rendezvous": "rel"}),
        json!({"v": 3, "op": "start", "office_id": OFFICE, "rendezvous": RENDEZVOUS}),
        json!({"v": 3, "op": "create", "instance_id": INSTANCE, "office_id": OFFICE,
               "rendezvous": RENDEZVOUS, "org_id": ORG, "org_name": "Acme", "office_name": "HQ",
               "role": "satellite", "owner_member_id": OWNER}),
        json!({"v": 3, "op": "create", "instance_id": INSTANCE, "office_id": OFFICE,
               "rendezvous": RENDEZVOUS, "org_id": ORG, "org_name": "Acme", "office_name": "HQ",
               "role": "hq"}),
    ] {
        let mut line = serde_json::to_vec(&wire).unwrap();
        line.push(b'\n');
        let flag = if wire["op"] == "create" {
            FirstLine::Create
        } else {
            FirstLine::Start
        };
        assert!(
            read_first_line(&mut Cursor::new(line), flag).is_err(),
            "{wire}"
        );
    }
    let unknown = b"{\"v\":3,\"op\":\"join\"}\n".to_vec();
    assert!(matches!(
        read_first_line(&mut Cursor::new(unknown), FirstLine::Create),
        Err(LaunchError::Invalid(_))
    ));
}

#[test]
fn ready_for_secrets_has_its_wire_shape() {
    let wire = serde_json::to_value(ReadyForSecrets).unwrap();
    assert_eq!(wire, json!({"v": 3, "event": "ready_for_secrets"}));
    assert_eq!(
        serde_json::from_value::<ReadyForSecrets>(wire).unwrap(),
        ReadyForSecrets
    );
    for bad in [
        json!({"v": 2, "event": "ready_for_secrets"}),
        json!({"v": 3, "event": "ready"}),
        json!({"event": "ready_for_secrets"}),
    ] {
        assert!(serde_json::from_value::<ReadyForSecrets>(bad).is_err());
    }
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
        db_key: DbKey::from_bytes([0xcd; 32]),
        grant_key: GrantKey::from_bytes([0xef; 32]),
        recovery_code: RecoveryCode::new("ABCD-EFGH-IJKL".into()).unwrap(),
        fds: vec![HandoffFd::Transfer],
    };
    let bytes = encode_handoff(&create).unwrap();
    assert_eq!(
        line_value(&bytes),
        json!({"v": 3, "op": "create", "db_key": KEY_HEX, "grant_key": GRANT_HEX,
               "recovery_code": "ABCD-EFGH-IJKL", "fds": ["transfer"]})
    );
    let back = decode_handoff(&bytes[..bytes.len() - 1]).unwrap();
    assert_eq!(back.fds(), &[HandoffFd::Transfer]);
    let Handoff::Create {
        grant_key,
        recovery_code,
        ..
    } = &back
    else {
        panic!("{back:?}");
    };
    assert_eq!(grant_key.expose_bytes(), &[0xef; 32]);
    assert_eq!(recovery_code.expose(), "ABCD-EFGH-IJKL");
    // Debug shows no secret.
    let printed = format!("{back:?}");
    assert!(!printed.contains("cdcd") && !printed.contains("efef") && !printed.contains("ABCD"));

    let start = Handoff::Start {
        db_key: DbKey::from_bytes([0xcd; 32]),
        grant_key: GrantKey::from_bytes([0xef; 32]),
        fds: vec![HandoffFd::Transfer],
    };
    assert_eq!(
        line_value(&encode_handoff(&start).unwrap()),
        json!({"v": 3, "op": "start", "db_key": KEY_HEX, "grant_key": GRANT_HEX, "fds": ["transfer"]})
    );

    // A discard gets no transfer socket, and its key may be null but not missing.
    let discard = Handoff::Discard {
        db_key: None,
        fds: vec![],
    };
    assert_eq!(
        line_value(&encode_handoff(&discard).unwrap()),
        json!({"v": 3, "op": "discard", "db_key": null, "fds": []})
    );
    assert!(decode_handoff(br#"{"v":3,"op":"discard","fds":[]}"#).is_err());
    assert!(decode_handoff(br#"{"v":3,"op":"discard","db_key":null,"fds":[]}"#).is_ok());
}

#[test]
fn a_handoff_naming_the_wrong_descriptors_or_version_is_refused() {
    let wrong = Handoff::Start {
        db_key: DbKey::from_bytes([1; 32]),
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
            r#"{"v":3,"op":"discard","db_key":null,"fds":["transfer"]}"#.to_owned(),
            false,
        ),
        (
            format!(
                r#"{{"v":3,"op":"start","db_key":"{k}","grant_key":"{k}","fds":["transfer","transfer"]}}"#
            ),
            false,
        ),
        (
            format!(
                r#"{{"v":3,"op":"start","db_key":"{k}","grant_key":"{k}","fds":["listener"]}}"#
            ),
            false,
        ),
        (
            format!(
                r#"{{"v":2,"op":"start","db_key":"{k}","grant_key":"{k}","fds":["transfer"]}}"#
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
}

#[test]
fn transfer_records_match_the_vectors_and_the_cap() {
    let doc: Value =
        serde_json::from_str(include_str!("../../test-vectors/v3/transfer-records.json")).unwrap();
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
fn the_v3_constants() {
    assert_eq!(LAUNCH_VERSION, 3);
    assert_eq!(crate::LAUNCH_VERSION, 2, "v2 is unchanged in move 1");
    assert_eq!(TRANSFER_HEADER_MAX_BYTES, 1024);
    assert_eq!(TRANSFER_SOCKET_BUFFER_BYTES, 64 * 1024);
}

#[test]
fn a_handoff_carries_exactly_the_keys_of_its_op() {
    let k = "01".repeat(32);
    for line in [
        // A start with a recovery code, a create without one, a discard with a grant key,
        // a start whose key is null, an unknown op.
        format!(
            r#"{{"v":3,"op":"start","db_key":"{k}","grant_key":"{k}","recovery_code":"c","fds":["transfer"]}}"#
        ),
        format!(r#"{{"v":3,"op":"create","db_key":"{k}","grant_key":"{k}","fds":["transfer"]}}"#),
        format!(r#"{{"v":3,"op":"discard","db_key":null,"grant_key":"{k}","fds":[]}}"#),
        format!(r#"{{"v":3,"op":"start","db_key":null,"grant_key":"{k}","fds":["transfer"]}}"#),
        format!(r#"{{"v":3,"op":"join","db_key":"{k}","fds":[]}}"#),
    ] {
        let error = decode_handoff(line.as_bytes()).unwrap_err().to_string();
        assert!(!error.contains("0101"), "{error}");
    }
    let ok = format!(
        r#"{{"v":3,"op":"create","db_key":"{k}","grant_key":"{k}","recovery_code":"c","fds":["transfer"]}}"#
    );
    assert!(matches!(
        decode_handoff(ok.as_bytes()).unwrap(),
        Handoff::Create { .. }
    ));
}

#[test]
fn the_rollout_reader_takes_either_version_and_checks_the_flag() {
    let mut v3 = Vec::new();
    write_first_line(&mut v3, &HostFirstLine::Start(office())).unwrap();
    assert_eq!(
        read_first_line_any(&mut Cursor::new(v3.clone()), FirstLine::Start).unwrap(),
        Some(AnyFirstLine::V3(HostFirstLine::Start(office())))
    );
    let mut v2 = Vec::new();
    write_host_message(
        &mut v2,
        &HostMessage::Start(StartOffice {
            app_token: org_proto::AppToken::from_bytes([1; 32]),
            db_key: DbKey::from_bytes([2; 32]),
        }),
    )
    .unwrap();
    assert!(matches!(
        read_first_line_any(&mut Cursor::new(v2.clone()), FirstLine::Start).unwrap(),
        Some(AnyFirstLine::V2(HostMessage::Start(_)))
    ));
    for line in [v2, v3] {
        assert!(matches!(
            read_first_line_any(&mut Cursor::new(line), FirstLine::Discard),
            Err(LaunchError::Invalid(_))
        ));
    }
    let v4 = b"{\"v\":4,\"op\":\"start\"}\n".to_vec();
    let err = read_first_line_any(&mut Cursor::new(v4), FirstLine::Start).unwrap_err();
    assert!(matches!(
        err,
        LaunchError::UnsupportedVersion { found: 4, .. }
    ));
    assert_eq!(
        err.to_string(),
        "launch version 4 isn't supported (this build speaks 2 or 3)"
    );
    assert!(
        read_first_line_any(&mut Cursor::new(Vec::new()), FirstLine::Start)
            .unwrap()
            .is_none()
    );
}

#[test]
fn worker_events_on_the_rendezvous_are_v3_lines() {
    let ready = ServerEvent::Ready {
        socket: "/run/o.sock".into(),
        pid: 42,
    };
    let mut out = Vec::new();
    write_server_event(&mut out, &ready).unwrap();
    assert_eq!(
        line_value(&out),
        json!({"v": 3, "event": "ready", "socket": "/run/o.sock", "pid": 42})
    );
    assert_eq!(
        read_server_event(&mut Cursor::new(out)).unwrap(),
        Some(ready.clone())
    );
    let mut v2 = Vec::new();
    crate::write_server_event(&mut v2, &ready).unwrap();
    assert!(matches!(
        read_server_event(&mut Cursor::new(v2)),
        Err(LaunchError::UnsupportedVersion { found: 2, .. })
    ));
    assert!(write_server_event(&mut Vec::new(), &ServerEvent::Unknown).is_err());
}

#[test]
fn the_v3_reader_refuses_any_v2_line_as_a_version() {
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
