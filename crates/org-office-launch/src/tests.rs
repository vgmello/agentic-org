use std::io::Cursor;
use std::path::Path;

use super::*;

/// A fixed token for tests; its wire form is `TOKEN_HEX`.
fn token() -> AppToken {
    AppToken::from_bytes([0xab; 32])
}

const TOKEN_HEX: &str = "abababababababababababababababababababababababababababababababab";

/// A fixed database key for tests; its wire form is `KEY_HEX`.
fn key() -> DbKey {
    DbKey::from_bytes([0xcd; DB_KEY_BYTES])
}

const KEY_HEX: &str = "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";

/// A fixed key socket id for tests; its wire form is `KSID_HEX`.
fn ksid() -> KeySocketId {
    KeySocketId::generate_from([0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef])
}

const KSID_HEX: &str = "0123456789abcdef";

fn start() -> StartOffice {
    StartOffice {
        key_socket_id: ksid(),
    }
}

fn discard() -> DiscardOffice {
    DiscardOffice {
        key_socket_id: ksid(),
    }
}

/// A hand-off of `token()` and the given key.
fn hand_off(db_key: Option<DbKey>) -> HandOff {
    HandOff {
        app_token: token(),
        db_key,
    }
}

/// The hand-off line `write_hand_off` sends for `token()` and `key()`, or no key.
fn hand_off_line(with_key: bool) -> Vec<u8> {
    let mut out = Vec::new();
    write_hand_off(&mut out, &token(), with_key.then(key).as_ref()).unwrap();
    out
}

/// Every flag a server can start with.
const FIRST_LINES: [FirstLine; 3] = [FirstLine::Create, FirstLine::Start, FirstLine::Discard];

fn create() -> CreateOffice {
    CreateOffice {
        org_id: "01M6WGY1WJCC503RXF2NS0SH83".into(),
        org_name: "Acme".into(),
        office_id: "01M6WGY1WJCC503RXF2NS0SH84".into(),
        office_name: "HQ".into(),
        role: OfficeRole::Hq,
        key_socket_id: ksid(),
    }
}

fn office_file() -> OfficeFile {
    OfficeFile::new(
        "01M6WGY1WJCC503RXF2NS0SH84",
        "HQ",
        OfficeRole::Hq,
        "01M6WGY1WJCC503RXF2NS0SH83",
        "Acme",
        "2026-11-08T02:10:00.000Z",
    )
}

fn json(bytes: &[u8]) -> serde_json::Value {
    serde_json::from_slice(bytes).unwrap()
}

#[test]
fn the_create_message_has_the_agreed_shape() {
    let mut out = Vec::new();
    write_host_message(&mut out, &HostMessage::Create(create())).unwrap();
    assert_eq!(out.last(), Some(&b'\n'));
    assert_eq!(
        json(&out),
        serde_json::json!({"v":3,"op":"create","org_id":"01M6WGY1WJCC503RXF2NS0SH83",
            "org_name":"Acme","office_id":"01M6WGY1WJCC503RXF2NS0SH84",
            "office_name":"HQ","role":"hq","key_socket_id":KSID_HEX})
    );
}

#[test]
fn host_messages_round_trip_and_eof_is_none() {
    let mut out = Vec::new();
    write_host_message(&mut out, &HostMessage::Create(create())).unwrap();
    let mut r = Cursor::new(out);
    assert_eq!(
        read_host_message(&mut r).unwrap(),
        Some(HostMessage::Create(create()))
    );
    assert_eq!(read_host_message(&mut r).unwrap(), None);
}

#[test]
fn server_events_have_the_agreed_shapes() {
    let cases = [
        (
            ServerEvent::Ready {
                socket: "/h/run/3RXF2NS0SH84.sock".into(),
                pid: 4242,
            },
            serde_json::json!({"v":3,"event":"ready","socket":"/h/run/3RXF2NS0SH84.sock","pid":4242}),
        ),
        (
            ServerEvent::Failed {
                code: FailCode::KeyMissing,
                message: "the office's key is missing".into(),
            },
            serde_json::json!({"v":3,"event":"failed","code":"key_missing",
                "message":"the office's key is missing"}),
        ),
        (
            ServerEvent::Created {
                office: office_file(),
            },
            serde_json::json!({"v":3,"event":"created","office":{"v":1,
                "office_id":"01M6WGY1WJCC503RXF2NS0SH84","office_name":"HQ","role":"hq",
                "org_id":"01M6WGY1WJCC503RXF2NS0SH83","org_name":"Acme",
                "created_at":"2026-11-08T02:10:00.000Z"}}),
        ),
    ];
    for (event, expected) in cases {
        let mut out = Vec::new();
        write_server_event(&mut out, &event).unwrap();
        assert_eq!(json(&out), expected);
        assert_eq!(
            read_server_event(&mut Cursor::new(out)).unwrap(),
            Some(event)
        );
    }
}

#[test]
fn every_fail_code_has_its_wire_name() {
    let names = [
        (FailCode::InvalidRequest, "invalid_request"),
        (FailCode::UnsupportedVersion, "unsupported_version"),
        (FailCode::InvalidName, "invalid_name"),
        (FailCode::AlreadyExists, "already_exists"),
        (FailCode::KeyMissing, "key_missing"),
        (FailCode::InUse, "in_use"),
        (FailCode::StorageError, "storage_error"),
    ];
    for (code, name) in names {
        assert_eq!(serde_json::to_value(code).unwrap(), name);
        assert_eq!(
            serde_json::from_value::<FailCode>(name.into()).unwrap(),
            code
        );
    }
}

#[test]
fn unknown_ops_events_codes_and_fields_are_tolerated() {
    let line = br#"{"v":3,"op":"shutdown","grace_ms":5}
"#;
    assert_eq!(
        read_host_message(&mut Cursor::new(&line[..])).unwrap(),
        Some(HostMessage::Unknown)
    );

    let line = br#"{"v":3,"event":"status","busy":true}
"#;
    assert_eq!(
        read_server_event(&mut Cursor::new(&line[..])).unwrap(),
        Some(ServerEvent::Unknown)
    );

    let line = br#"{"v":3,"event":"failed","code":"disk_on_fire","message":"m","extra":1}
"#;
    assert_eq!(
        read_server_event(&mut Cursor::new(&line[..])).unwrap(),
        Some(ServerEvent::Failed {
            code: FailCode::StorageError,
            message: "m".into()
        })
    );

    let line = br#"{"v":3,"op":"create","org_id":"A1","org_name":"Acme","office_id":"B2","office_name":"HQ","role":"hq","hq_address":"x","key_socket_id":"0123456789abcdef"}
"#;
    let Some(HostMessage::Create(c)) = read_host_message(&mut Cursor::new(&line[..])).unwrap()
    else {
        panic!("not a create");
    };
    assert_eq!(c.role, OfficeRole::Hq);
    assert_eq!(c.key_socket_id, ksid());
}

#[test]
fn a_create_with_a_role_this_version_cant_create_is_refused() {
    for line in [
        &br#"{"v":3,"op":"create","org_id":"A1","org_name":"Acme","office_id":"B2","office_name":"HQ","role":"satellite","hq_address":"x","key_socket_id":"0123456789abcdef"}
"#[..],
        &br#"{"v":3,"op":"create","org_id":"A1","org_name":"Acme","office_id":"B2","office_name":"HQ","role":"moon","key_socket_id":"0123456789abcdef"}
"#[..],
        &br#"{"v":3,"op":"create","org_id":"A1","org_name":"Acme","office_id":"B2","office_name":"HQ","key_socket_id":"0123456789abcdef"}
"#[..],
    ] {
        let err = read_host_message(&mut Cursor::new(line)).unwrap_err();
        assert!(matches!(err, LaunchError::Json(_)), "{err}");
        assert_eq!(err.fail_code(), FailCode::InvalidRequest);
    }
    for role in [OfficeRole::Unknown, OfficeRole::Satellite] {
        let message = HostMessage::Create(CreateOffice { role, ..create() });
        assert!(matches!(
            write_host_message(&mut Vec::new(), &message),
            Err(LaunchError::Invalid(_))
        ));
    }
}

#[test]
fn the_types_check_their_rules_on_every_serde_path() {
    // Reading with the plain frame API, bypassing `read_host_message`, is still refused.
    let mut line = serde_json::to_value(HostMessage::Create(create())).unwrap();
    line["office_id"] = "../../etc".into();
    let mut r = Cursor::new(line.to_string() + "\n");
    assert!(org_proto::frame::read_message::<HostMessage>(&mut r).is_err());

    // Encoding a bad create or office file through serde directly fails too.
    let bad = CreateOffice {
        org_id: "../x".into(),
        ..create()
    };
    assert!(serde_json::to_string(&bad).is_err());
    let bad = OfficeFile {
        office_id: "a/b".into(),
        ..office_file()
    };
    assert!(serde_json::to_string(&bad).is_err());
    let mut value = serde_json::to_value(office_file()).unwrap();
    value["org_id"] = "../x".into();
    assert!(serde_json::from_value::<OfficeFile>(value).is_err());
}

#[test]
fn a_created_event_with_a_bad_office_is_refused() {
    let mut office = serde_json::to_value(office_file()).unwrap();
    office["office_id"] = "../../x".into();
    let line = serde_json::json!({"v":3,"event":"created","office":office}).to_string() + "\n";
    assert!(matches!(
        read_server_event(&mut Cursor::new(line)),
        Err(LaunchError::Json(_))
    ));

    let mut office = serde_json::to_value(office_file()).unwrap();
    office["v"] = 7.into();
    let line = serde_json::json!({"v":3,"event":"created","office":office}).to_string() + "\n";
    assert!(matches!(
        read_server_event(&mut Cursor::new(line)),
        Err(LaunchError::Json(_))
    ));

    let bad = ServerEvent::Created {
        office: OfficeFile {
            org_id: "a/b".into(),
            ..office_file()
        },
    };
    assert!(matches!(
        write_server_event(&mut Vec::new(), &bad),
        Err(LaunchError::Invalid(_))
    ));
}

#[test]
fn unknown_variants_are_never_written() {
    let mut out = Vec::new();
    assert!(matches!(
        write_host_message(&mut out, &HostMessage::Unknown),
        Err(LaunchError::Invalid(_))
    ));
    assert!(matches!(
        write_server_event(&mut out, &ServerEvent::Unknown),
        Err(LaunchError::Invalid(_))
    ));
    assert!(out.is_empty());
}

#[test]
fn oversized_lines_are_refused_both_ways() {
    let mut line = br#"{"v":3,"event":"failed","code":"in_use","message":""#.to_vec();
    line.extend(std::iter::repeat_n(b'x', org_proto::MAX_FRAME_BYTES));
    line.extend(b"\"}\n{\"v\":3,\"event\":\"ready\",\"socket\":\"/s\",\"pid\":1}\n");
    let err = read_server_event(&mut Cursor::new(line)).unwrap_err();
    assert!(
        matches!(err, LaunchError::Frame(FrameError::TooLarge)),
        "{err}"
    );
    assert!(err.closes_stream());

    let huge = ServerEvent::Failed {
        code: FailCode::StorageError,
        message: "x".repeat(org_proto::MAX_FRAME_BYTES),
    };
    let mut out = Vec::new();
    assert!(matches!(
        write_server_event(&mut out, &huge),
        Err(LaunchError::Frame(FrameError::TooLargeToSend))
    ));
    assert!(out.is_empty());
}

#[test]
fn errors_say_whether_the_pipe_is_still_in_step() {
    let mut r =
        Cursor::new(&b"not json\n{\"v\":3,\"event\":\"ready\",\"socket\":\"/s\",\"pid\":2}\n"[..]);
    let err = read_server_event(&mut r).unwrap_err();
    assert!(!err.closes_stream());
    assert!(std::error::Error::source(&err).is_some());
    assert_eq!(
        read_server_event(&mut r).unwrap(),
        Some(ServerEvent::Ready {
            socket: "/s".into(),
            pid: 2
        })
    );
    let err = LaunchError::UnsupportedVersion(2);
    assert_eq!(err.fail_code(), FailCode::UnsupportedVersion);
    let err = LaunchError::UnsupportedOfficeFileVersion(2);
    assert_eq!(err.fail_code(), FailCode::UnsupportedVersion);
    assert_eq!(
        err.to_string(),
        "office.json version 2 isn't supported (this build reads 1)"
    );
    assert_eq!(
        LaunchError::UnsupportedVersion(2).to_string(),
        "launch version 2 isn't supported (this build speaks 3)"
    );
    assert!(!err.closes_stream());
}

#[test]
fn another_version_is_refused_before_its_shape_is_read() {
    // A v4 line with a shape v3 can't decode still reports the version, not a JSON error.
    let line = br#"{"v":4,"op":"create","org":{"nested":true}}
"#;
    assert!(matches!(
        read_host_message(&mut Cursor::new(&line[..])),
        Err(LaunchError::UnsupportedVersion(4))
    ));
    let line = br#"{"v":0,"event":"ready","socket":"/s","pid":1}
"#;
    assert!(matches!(
        read_server_event(&mut Cursor::new(&line[..])),
        Err(LaunchError::UnsupportedVersion(0))
    ));
    let line = br#"{"op":"create"}
"#;
    assert!(matches!(
        read_host_message(&mut Cursor::new(&line[..])),
        Err(LaunchError::Json(_))
    ));
}

#[test]
fn a_create_with_a_hostile_id_is_refused() {
    for (org_id, office_id) in [
        ("../x", "B2"),
        ("A1", "../../etc"),
        ("A1", ""),
        ("a/b", "B2"),
    ] {
        let message = HostMessage::Create(CreateOffice {
            org_id: org_id.into(),
            office_id: office_id.into(),
            ..create()
        });
        let mut out = Vec::new();
        assert!(
            matches!(
                write_host_message(&mut out, &message),
                Err(LaunchError::Invalid(_))
            ),
            "wrote {org_id:?} / {office_id:?}"
        );
        assert!(out.is_empty());

        // A peer that doesn't use this crate may still send one: the reader refuses it.
        let mut line = serde_json::to_value(HostMessage::Create(create())).unwrap();
        line["v"] = 3.into();
        line["org_id"] = org_id.into();
        line["office_id"] = office_id.into();
        let line = line.to_string() + "\n";
        assert!(
            matches!(
                read_host_message(&mut Cursor::new(line)),
                Err(LaunchError::Json(_))
            ),
            "accepted {org_id:?} / {office_id:?}"
        );
    }
}

#[test]
fn malformed_lines_and_truncation_are_errors() {
    let mut r = Cursor::new(&b"not json\n"[..]);
    assert!(matches!(
        read_server_event(&mut r),
        Err(LaunchError::Json(_))
    ));
    let mut r = Cursor::new(&br#"{"v":3,"event":"ready""#[..]);
    assert!(matches!(
        read_server_event(&mut r),
        Err(LaunchError::Frame(FrameError::Truncated))
    ));
}

#[test]
fn office_file_round_trips_and_ignores_unknown_fields() {
    let file = office_file();
    let bytes = file.to_vec().unwrap();
    assert_eq!(bytes.last(), Some(&b'\n'));
    assert_eq!(OfficeFile::from_slice(&bytes).unwrap(), file);

    // A field from a newer build survives a read and a rewrite by this one.
    let mut value = json(&bytes);
    value["hq_address"] = "hq.local:7443".into();
    let read = OfficeFile::from_slice(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(read.office_id, file.office_id);
    assert_eq!(read.org_name, file.org_name);
    let mut renamed = read;
    renamed.org_name = "Acme Inc".into();
    let rewritten = json(&renamed.to_vec().unwrap());
    assert_eq!(rewritten["hq_address"], "hq.local:7443");
    assert_eq!(rewritten["org_name"], "Acme Inc");
    assert_eq!(rewritten["v"], OFFICE_FILE_VERSION);
}

#[test]
fn an_office_file_without_a_role_reads_as_unknown() {
    let mut value = serde_json::to_value(office_file()).unwrap();
    value.as_object_mut().unwrap().remove("role");
    let file = OfficeFile::from_slice(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(file.role, OfficeRole::Unknown);
}

#[test]
fn discarded_has_its_shape_and_fail_codes_map_errors() {
    let mut out = Vec::new();
    write_server_event(&mut out, &ServerEvent::Discarded).unwrap();
    assert_eq!(json(&out), serde_json::json!({"v":3,"event":"discarded"}));
    assert_eq!(
        read_server_event(&mut Cursor::new(out)).unwrap(),
        Some(ServerEvent::Discarded)
    );
    assert_eq!(DISCARD_FLAG, "--discard");

    let broken = LaunchError::Frame(FrameError::Io(std::io::Error::other("pipe")));
    assert_eq!(broken.fail_code(), FailCode::StorageError);
    assert_eq!(
        LaunchError::Frame(FrameError::Truncated).fail_code(),
        FailCode::StorageError
    );
    assert_eq!(
        LaunchError::Frame(FrameError::TooLarge).fail_code(),
        FailCode::InvalidRequest
    );
    let bad_json = read_server_event(&mut Cursor::new(&b"{]\n"[..])).unwrap_err();
    assert_eq!(bad_json.fail_code(), FailCode::InvalidRequest);
}

#[test]
fn office_file_checks_version_and_ids() {
    // office.json has its own version: the pipe's version 2 isn't one it reads.
    let mut value = serde_json::to_value(office_file()).unwrap();
    value["v"] = 2.into();
    assert!(matches!(
        OfficeFile::from_slice(&serde_json::to_vec(&value).unwrap()),
        Err(LaunchError::UnsupportedOfficeFileVersion(2))
    ));

    let mut value = serde_json::to_value(office_file()).unwrap();
    value["v"] = 3.into();
    assert!(matches!(
        OfficeFile::from_slice(&serde_json::to_vec(&value).unwrap()),
        Err(LaunchError::UnsupportedOfficeFileVersion(3))
    ));

    let mut value = serde_json::to_value(office_file()).unwrap();
    value["office_id"] = "../../x".into();
    assert!(matches!(
        OfficeFile::from_slice(&serde_json::to_vec(&value).unwrap()),
        Err(LaunchError::Invalid(_))
    ));

    assert!(matches!(
        OfficeFile::from_slice(b"{}"),
        Err(LaunchError::Json(_))
    ));

    let mut value = serde_json::to_value(office_file()).unwrap();
    value.as_object_mut().unwrap().remove("v");
    assert!(matches!(
        OfficeFile::from_slice(&serde_json::to_vec(&value).unwrap()),
        Err(LaunchError::Json(_))
    ));

    // The derived decoder is guarded too, not only `from_slice`.
    let mut value = serde_json::to_value(office_file()).unwrap();
    value["v"] = 9.into();
    assert!(serde_json::from_value::<OfficeFile>(value).is_err());
}

#[test]
fn office_file_always_writes_the_current_version_and_refuses_bad_ids() {
    let written = json(&office_file().to_vec().unwrap());
    assert_eq!(written["v"], OFFICE_FILE_VERSION);
    assert_eq!(written["v"], 1);
    let bad = OfficeFile {
        office_id: "../x".into(),
        ..office_file()
    };
    assert!(matches!(bad.to_vec(), Err(LaunchError::Invalid(_))));
}

#[test]
fn the_start_message_has_the_agreed_shape_and_round_trips() {
    let start = HostMessage::Start(start());
    let mut out = Vec::new();
    write_host_message(&mut out, &start).unwrap();
    assert_eq!(
        json(&out),
        serde_json::json!({"v":3,"op":"start","key_socket_id":KSID_HEX})
    );
    assert_eq!(
        read_host_message(&mut Cursor::new(out)).unwrap(),
        Some(start)
    );
}

#[test]
fn the_secrets_never_show_in_hand_off_errors() {
    let bad_token = format!("{}Z", &TOKEN_HEX[..63]);
    let bad_key = format!("{}Z", &KEY_HEX[..63]);
    let digits = "1234567890".repeat(7);
    for line in [
        format!("{{\"v\":3,\"app_token\":\"{bad_token}\",\"db_key\":\"{KEY_HEX}\"}}\n"),
        format!("{{\"v\":3,\"app_token\":{digits},\"db_key\":\"{KEY_HEX}\"}}\n"),
        format!("{{\"v\":3,\"app_token\":\"{TOKEN_HEX}\",\"db_key\":\"{bad_key}\"}}\n"),
        format!(
            "{{\"v\":3,\"app_token\":\"{TOKEN_HEX}\",\"db_key\":\"{}\"}}\n",
            &KEY_HEX[..62]
        ),
        format!("{{\"v\":3,\"app_token\":\"{TOKEN_HEX}\",\"db_key\":{digits}}}\n"),
        // An unknown field after the secrets: the error names the field, never the secrets.
        format!("{{\"v\":3,\"app_token\":\"{TOKEN_HEX}\",\"db_key\":\"{KEY_HEX}\",\"x\":1}}\n"),
    ] {
        for first_line in FIRST_LINES {
            let err = read_hand_off(&mut Cursor::new(line.clone()), first_line).unwrap_err();
            let text = format!("{err} {err:?}");
            assert!(
                !text.contains("abababab")
                    && !text.contains("cdcdcdcd")
                    // Digits echoed in any form (a float's mantissa included).
                    && !text.contains("2345678"),
                "{text}"
            );
            assert!(matches!(err, LaunchError::Json(_)), "{err}");
            assert_eq!(err.fail_code(), FailCode::InvalidRequest);
            assert!(!err.is_host_gone());
        }
    }
}

#[test]
fn ids_are_capped_at_64_characters() {
    assert!(is_plain_id(&"a".repeat(64)));
    assert!(!is_plain_id(&"a".repeat(65)));
    assert_eq!(MAX_ID_CHARS, 64);
}

#[test]
fn office_dir_uses_the_same_plain_id_rule_as_sockets() {
    let home = Path::new("/h");
    assert_eq!(
        office_dir(home, "01M6WGY1WJCC503RXF2NS0SH84"),
        Some(PathBuf::from("/h/offices/01M6WGY1WJCC503RXF2NS0SH84"))
    );
    for id in [
        "",
        "/etc/x",
        "../../x",
        "a/b",
        "a.b",
        "é1234567890",
        "abc def",
        "abc",
        "01M6WGY1WJCC503RXF2NS0SH84",
        &"a".repeat(64),
        &"a".repeat(65),
    ] {
        assert_eq!(
            office_dir(home, id).is_some(),
            org_proto::location::office_socket_path(home, id).is_some(),
            "rules disagree on {id:?}"
        );
    }
}

#[test]
fn an_unknown_role_is_written_back_as_it_was() {
    for (stored, expected) in [
        (
            Some(serde_json::json!("relay")),
            Some(serde_json::json!("relay")),
        ),
        (
            Some(serde_json::json!({"kind":"x"})),
            Some(serde_json::json!({"kind":"x"})),
        ),
        (Some(serde_json::Value::Null), Some(serde_json::Value::Null)),
        (None, None),
    ] {
        let mut value = serde_json::to_value(office_file()).unwrap();
        let object = value.as_object_mut().unwrap();
        object.remove("role");
        if let Some(role) = stored {
            object.insert("role".into(), role);
        }
        let mut file = OfficeFile::from_slice(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(file.role, OfficeRole::Unknown);
        file.org_name = "Renamed".into();
        let written = json(&file.to_vec().unwrap());
        assert_eq!(written.get("role").cloned(), expected);
    }
    // A known role set by this build replaces whatever was there.
    let mut value = serde_json::to_value(office_file()).unwrap();
    value["role"] = "relay".into();
    let mut file = OfficeFile::from_slice(&serde_json::to_vec(&value).unwrap()).unwrap();
    file.role = OfficeRole::Hq;
    assert_eq!(json(&file.to_vec().unwrap())["role"], "hq");
}

#[test]
fn unknown_variants_never_serialize_through_plain_serde() {
    assert!(serde_json::to_string(&HostMessage::Unknown).is_err());
    assert!(serde_json::to_string(&ServerEvent::Unknown).is_err());
}

#[test]
fn an_oversized_host_message_is_not_sent() {
    let message = HostMessage::Create(CreateOffice {
        org_name: "x".repeat(org_proto::MAX_FRAME_BYTES),
        ..create()
    });
    let mut out = Vec::new();
    assert!(matches!(
        write_host_message(&mut out, &message),
        Err(LaunchError::Frame(FrameError::TooLargeToSend))
    ));
    assert!(out.is_empty());
}

#[test]
fn host_messages_and_the_hand_off_are_flushed_as_one_line() {
    struct Recorder {
        bytes: Vec<u8>,
        flushed_at: Option<usize>,
    }
    impl Write for Recorder {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.bytes.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.flushed_at = Some(self.bytes.len());
            Ok(())
        }
    }
    let recorder = || Recorder {
        bytes: Vec::new(),
        flushed_at: None,
    };
    let mut first = recorder();
    write_host_message(&mut first, &HostMessage::Start(start())).unwrap();
    let mut hand_off = recorder();
    write_hand_off(&mut hand_off, &token(), Some(&key())).unwrap();
    for w in [first, hand_off] {
        assert_eq!(w.flushed_at, Some(w.bytes.len()));
        assert_eq!(w.bytes.iter().filter(|&&b| b == b'\n').count(), 1);
        assert_eq!(w.bytes.last(), Some(&b'\n'));
    }
}

#[test]
fn a_newer_office_file_reports_its_version_even_with_another_shape() {
    assert!(matches!(
        OfficeFile::from_slice(br#"{"v":2,"office":{"id":"x"}}"#),
        Err(LaunchError::UnsupportedOfficeFileVersion(2))
    ));
}

#[test]
fn an_office_file_made_without_a_role_cant_be_written() {
    let file = OfficeFile::new(
        "A1",
        "HQ",
        OfficeRole::Unknown,
        "B2",
        "Acme",
        "2026-11-08T02:10:00.000Z",
    );
    assert!(matches!(file.to_vec(), Err(LaunchError::Invalid(_))));
    assert!(serde_json::to_string(&file).is_err());
}

#[test]
fn launch_lines_are_zeroed_after_decoding_on_every_path() {
    let good = hand_off_line(true);
    let mut line = good[..good.len() - 1].to_vec();
    assert!(decode_hand_off(&mut line).is_ok());
    assert!(line.iter().all(|&b| b == 0));

    for (mut bad, version) in [
        (
            format!("{{\"v\":2,\"app_token\":\"{TOKEN_HEX}\",\"db_key\":\"{KEY_HEX}\"}}"),
            Some(2),
        ),
        (format!("{{\"v\":3,\"app_token\":\"{TOKEN_HEX}\",]"), None),
        (
            format!("{{\"v\":3,\"app_token\":\"{TOKEN_HEX}\",\"db_key\":\"{KEY_HEX}\",\"x\":1}}"),
            None,
        ),
    ]
    .map(|(line, version)| (line.into_bytes(), version))
    {
        let err = decode_hand_off(&mut bad).unwrap_err();
        match version {
            Some(v) => assert!(matches!(err, LaunchError::UnsupportedVersion(n) if n == v)),
            None => assert!(matches!(err, LaunchError::Json(_)), "{err}"),
        }
        assert!(bad.iter().all(|&b| b == 0));
    }

    // The first line is zeroed too, though it holds no secret now.
    let good = encode_launch_line(&HostMessage::Start(start())).unwrap();
    let mut line = good[..good.len() - 1].to_vec();
    assert!(decode_host_line(&mut line).is_ok());
    assert!(line.iter().all(|&b| b == 0));
}

#[test]
fn host_lines_are_encoded_without_reallocating() {
    let line = encode_launch_line(&HostMessage::Create(create())).unwrap();
    assert_eq!(line.capacity(), line.len());
    assert_eq!(line.last(), Some(&b'\n'));
    let (app_token, db_key) = (token(), key());
    let line = encode_launch_line(&HandOffRef {
        app_token: &app_token,
        db_key: Some(&db_key),
    })
    .unwrap();
    assert_eq!(line.capacity(), line.len());
    assert_eq!(line.last(), Some(&b'\n'));
}

#[test]
fn a_hand_off_line_read_in_pieces_stays_in_its_first_buffer() {
    struct Chunks(Vec<Vec<u8>>);
    impl std::io::Read for Chunks {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.0.is_empty() {
                return Ok(0);
            }
            let chunk = self.0.remove(0);
            buf[..chunk.len()].copy_from_slice(&chunk);
            Ok(chunk.len())
        }
    }
    let chunks = |line: &[u8]| Chunks(line.chunks(16).map(<[u8]>::to_vec).collect());
    let line = hand_off_line(true);
    let mut r = std::io::BufReader::with_capacity(16, chunks(&line));
    let read = read_secret_line(&mut r).unwrap().unwrap();
    assert_eq!(read.capacity(), org_proto::MAX_FRAME_BYTES + 1);
    assert_eq!(read, line[..line.len() - 1]);

    let mut r = std::io::BufReader::with_capacity(16, chunks(&line));
    assert_eq!(
        read_hand_off(&mut r, FirstLine::Start).unwrap(),
        hand_off(Some(key()))
    );
}

#[test]
fn the_largest_host_line_is_sent_and_one_byte_more_is_not() {
    let base = encode_launch_line(&HostMessage::Create(create())).unwrap();
    // The line without its newline may be exactly MAX_FRAME_BYTES long.
    let pad = org_proto::MAX_FRAME_BYTES - (base.len() - 1);
    let fits = HostMessage::Create(CreateOffice {
        org_name: format!("Acme{}", "x".repeat(pad)),
        ..create()
    });
    let mut out = Vec::new();
    write_host_message(&mut out, &fits).unwrap();
    assert_eq!(out.len(), org_proto::MAX_FRAME_BYTES + 1);
    assert!(read_host_message(&mut Cursor::new(out)).unwrap().is_some());

    let too_big = HostMessage::Create(CreateOffice {
        org_name: format!("Acme{}", "x".repeat(pad + 1)),
        ..create()
    });
    let mut out = Vec::new();
    assert!(matches!(
        write_host_message(&mut out, &too_big),
        Err(LaunchError::Frame(FrameError::TooLargeToSend))
    ));
    assert!(out.is_empty());
}

#[test]
fn an_oversized_host_line_is_too_large_and_closes_the_pipe() {
    let mut line = vec![b'x'; org_proto::MAX_FRAME_BYTES + 1];
    line.push(b'\n');
    let err = read_host_message(&mut Cursor::new(line)).unwrap_err();
    assert!(
        matches!(err, LaunchError::Frame(FrameError::TooLarge)),
        "{err}"
    );
    assert_eq!(err.fail_code(), FailCode::InvalidRequest);
    assert!(err.closes_stream());
    // One byte less is a whole (if malformed) line, not too large.
    let mut line = vec![b'x'; org_proto::MAX_FRAME_BYTES];
    line.push(b'\n');
    assert!(matches!(
        read_host_message(&mut Cursor::new(line)),
        Err(LaunchError::Json(_))
    ));
}

#[test]
fn an_over_limit_hand_off_line_is_refused() {
    // A valid hand-off padded past the frame limit with spaces (valid JSON whitespace).
    let line = hand_off_line(true);
    let mut huge = line[..line.len() - 2].to_vec();
    huge.extend(std::iter::repeat_n(
        b' ',
        org_proto::MAX_FRAME_BYTES + 2 - line.len(),
    ));
    huge.extend(b"}\n");
    assert_eq!(huge.len(), org_proto::MAX_FRAME_BYTES + 2);
    let err = read_hand_off(&mut Cursor::new(huge.clone()), FirstLine::Start).unwrap_err();
    assert!(
        matches!(err, LaunchError::Frame(FrameError::TooLarge)),
        "{err}"
    );
    assert!(!err.is_host_gone());
    assert_eq!(err.fail_code(), FailCode::InvalidRequest);
    // One space less fits exactly, and reads.
    huge.remove(line.len());
    assert_eq!(huge.len(), org_proto::MAX_FRAME_BYTES + 1);
    assert_eq!(
        read_hand_off(&mut Cursor::new(huge), FirstLine::Start).unwrap(),
        hand_off(Some(key()))
    );
}

#[test]
fn a_created_office_without_a_known_role_is_not_sent() {
    let office = OfficeFile::new(
        "A1",
        "HQ",
        OfficeRole::Unknown,
        "B2",
        "Acme",
        "2026-11-08T02:10:00.000Z",
    );
    let mut out = Vec::new();
    assert!(matches!(
        write_server_event(&mut out, &ServerEvent::Created { office }),
        Err(LaunchError::Invalid(_))
    ));
    assert!(out.is_empty());
}

#[test]
fn the_role_rule_is_what_refuses_a_non_hq_create() {
    // These lines are complete apart from their role, so only the HQ rule can refuse them.
    for role in ["\"satellite\"", "\"moon\""] {
        let line = format!(
            "{{\"v\":3,\"op\":\"create\",\"org_id\":\"A1\",\"org_name\":\"Acme\",\
             \"office_id\":\"B2\",\"office_name\":\"HQ\",\"role\":{role},\
             \"key_socket_id\":\"{KSID_HEX}\"}}\n"
        );
        let err = read_host_message(&mut Cursor::new(line))
            .unwrap_err()
            .to_string();
        assert!(err.contains("only HQ"), "{role}: {err}");
    }
}

#[test]
fn office_files_are_capped() {
    // Valid in every other way: only the size can refuse it.
    let mut big = office_file();
    big.extra
        .insert("pad".into(), "x".repeat(MAX_OFFICE_FILE_BYTES).into());
    assert!(matches!(
        big.to_vec(),
        Err(LaunchError::Invalid("office.json would be too large"))
    ));
    let mut bytes = office_file().to_vec().unwrap();
    bytes.extend(std::iter::repeat_n(b' ', MAX_OFFICE_FILE_BYTES));
    assert!(matches!(
        OfficeFile::from_slice(&bytes),
        Err(LaunchError::Invalid(_))
    ));
    assert!(office_file().to_vec().unwrap().len() < MAX_OFFICE_FILE_BYTES);
}

#[test]
fn a_broken_hand_off_line_is_zeroed() {
    let line = hand_off_line(true);
    let mut buf = Vec::with_capacity(org_proto::MAX_FRAME_BYTES + 1);
    // Truncated: the host died mid-line, after part of the token.
    let cut = &line[..line.len() - 10];
    assert!(matches!(
        read_secret_line_into(&mut Cursor::new(cut), &mut buf),
        Err(LaunchError::Frame(FrameError::Truncated))
    ));
    assert!(!buf.is_empty() && buf.iter().all(|&b| b == 0));
    // Too large: the secrets followed by more than a line's worth.
    let mut huge = line[..line.len() - 1].to_vec();
    huge.extend(std::iter::repeat_n(b'x', org_proto::MAX_FRAME_BYTES));
    assert!(matches!(
        read_secret_line_into(&mut Cursor::new(huge), &mut buf),
        Err(LaunchError::Frame(FrameError::TooLarge))
    ));
    assert!(!buf.is_empty() && buf.iter().all(|&b| b == 0));
}

#[test]
fn the_office_file_cap_is_exact_and_holds_for_created_events() {
    // A file of exactly the cap is read (it fails on its content, not its size); one more
    // byte is too large.
    let mut bytes = office_file().to_vec().unwrap();
    let pad = MAX_OFFICE_FILE_BYTES - bytes.len();
    bytes.extend(std::iter::repeat_n(b' ', pad));
    assert_eq!(bytes.len(), MAX_OFFICE_FILE_BYTES);
    assert!(OfficeFile::from_slice(&bytes).is_ok());
    bytes.push(b' ');
    assert!(matches!(
        OfficeFile::from_slice(&bytes),
        Err(LaunchError::Invalid("office.json is too large"))
    ));

    // A created office too big for office.json isn't sent either.
    let mut big = office_file();
    big.extra
        .insert("pad".into(), "x".repeat(MAX_OFFICE_FILE_BYTES).into());
    let mut out = Vec::new();
    assert!(matches!(
        write_server_event(&mut out, &ServerEvent::Created { office: big }),
        Err(LaunchError::Invalid(_))
    ));
    assert!(out.is_empty());
}

#[test]
fn office_files_need_valid_names_and_timestamps() {
    for (field, value) in [
        ("org_name", ""),
        ("org_name", " Acme"),
        ("org_name", "Ac\u{7}me"),
        ("office_name", ""),
        ("created_at", "not a date"),
        ("created_at", "2026-13-08T02:10:00.000Z"),
        ("created_at", "2026-11-08T24:10:00.000Z"),
        ("created_at", "2026-11-08T02:10:00Z"),
        ("created_at", "2026-11-08 02:10:00.000Z"),
        ("created_at", "2026-00-08T02:10:00.000Z"),
        ("created_at", "2026-11-00T02:10:00.000Z"),
        ("created_at", "2026-11-32T02:10:00.000Z"),
        ("created_at", "2026-11-08T02:60:00.000Z"),
        ("created_at", "2026-11-08T02:10:60.000Z"),
    ] {
        let mut value_json = serde_json::to_value(office_file()).unwrap();
        value_json[field] = value.into();
        assert!(
            matches!(
                OfficeFile::from_slice(&serde_json::to_vec(&value_json).unwrap()),
                Err(LaunchError::Invalid(_))
            ),
            "read {field}={value:?}"
        );
        let mut file = office_file();
        match field {
            "org_name" => file.org_name = value.into(),
            "office_name" => file.office_name = value.into(),
            _ => file.created_at = value.into(),
        }
        assert!(file.to_vec().is_err(), "wrote {field}={value:?}");
    }
    assert!(is_timestamp("2026-11-08T02:10:00.000Z"));
    assert!(is_timestamp("1999-12-31T23:59:59.999Z"));
    assert!(is_timestamp("2026-01-01T00:00:00.000Z"));
}

/// An office file padded so `to_vec` is exactly `len` bytes.
fn office_file_of_len(len: usize) -> OfficeFile {
    let mut file = office_file();
    file.extra.insert("pad".into(), "".into());
    let base = file.to_vec().unwrap().len();
    file.extra
        .insert("pad".into(), "x".repeat(len - base).into());
    file
}

#[test]
fn the_office_file_write_cap_is_exact() {
    let fits = office_file_of_len(MAX_OFFICE_FILE_BYTES);
    assert_eq!(fits.to_vec().unwrap().len(), MAX_OFFICE_FILE_BYTES);
    let mut out = Vec::new();
    write_server_event(
        &mut out,
        &ServerEvent::Created {
            office: fits.clone(),
        },
    )
    .unwrap();
    // And it is accepted when read back.
    assert!(matches!(
        read_server_event(&mut Cursor::new(out)).unwrap(),
        Some(ServerEvent::Created { .. })
    ));

    let over = office_file_of_len(MAX_OFFICE_FILE_BYTES + 1);
    assert!(over.to_vec().is_err());
    let mut out = Vec::new();
    assert!(write_server_event(&mut out, &ServerEvent::Created { office: over }).is_err());
    assert!(out.is_empty());
}

#[test]
fn a_created_event_over_the_office_file_cap_is_refused_when_read() {
    let over = office_file_of_len(MAX_OFFICE_FILE_BYTES + 1);
    // Built by hand, as a buggy server might, since write_server_event won't send it.
    let mut value = serde_json::json!({"v":3,"event":"created"});
    let mut office = serde_json::to_value(&over).unwrap();
    office["v"] = 1.into();
    value["office"] = office;
    let line = value.to_string() + "\n";
    assert!(matches!(
        read_server_event(&mut Cursor::new(line)),
        Err(LaunchError::Invalid(_))
    ));
}

#[test]
fn a_sent_hand_off_line_is_zeroed_whether_or_not_the_write_works() {
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("closed"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let (app_token, db_key) = (token(), key());
    let line = HandOffRef {
        app_token: &app_token,
        db_key: Some(&db_key),
    };
    let mut bytes = encode_launch_line(&line).unwrap();
    let mut out = Vec::new();
    send_and_zero(&mut out, &mut bytes).unwrap();
    assert!(out.ends_with(b"\n") && out.len() == bytes.len());
    assert!(bytes.iter().all(|&b| b == 0));

    let mut bytes = encode_launch_line(&line).unwrap();
    assert!(send_and_zero(&mut Broken, &mut bytes).is_err());
    assert!(!bytes.is_empty() && bytes.iter().all(|&b| b == 0));
    assert!(matches!(
        write_hand_off(&mut Broken, &app_token, Some(&db_key)),
        Err(LaunchError::Frame(FrameError::Io(_)))
    ));
}

#[test]
fn a_host_that_dies_mid_line_is_gone_not_a_failure() {
    let line = encode_launch_line(&HostMessage::Start(start())).unwrap();
    let err = read_host_message(&mut Cursor::new(&line[..20])).unwrap_err();
    assert!(err.is_host_gone());
    let bad = read_host_message(&mut Cursor::new(&b"{]\n"[..])).unwrap_err();
    assert!(!bad.is_host_gone());
    assert!(!LaunchError::UnsupportedVersion(2).is_host_gone());
}

#[test]
fn eof_before_the_hand_off_line_means_the_host_is_gone() {
    let line = hand_off_line(true);
    for cut in [&b""[..], &line[..20], &line[..line.len() - 1]] {
        for first_line in FIRST_LINES {
            let err = read_hand_off(&mut Cursor::new(cut), first_line).unwrap_err();
            assert!(
                matches!(err, LaunchError::Frame(FrameError::Truncated)),
                "{err}"
            );
            assert!(err.is_host_gone(), "{} bytes", cut.len());
        }
    }
    // A whole line that is bad is a failure, not the host going away.
    for bad in [&b"{]\n"[..], &b"\n"[..]] {
        let err = read_hand_off(&mut Cursor::new(bad), FirstLine::Start).unwrap_err();
        assert!(!err.is_host_gone(), "{err}");
    }
    // A broken connection isn't EOF either.
    struct Reset;
    impl std::io::Read for Reset {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::ConnectionReset.into())
        }
    }
    let err = read_hand_off(&mut std::io::BufReader::new(Reset), FirstLine::Start).unwrap_err();
    assert!(!err.is_host_gone());
    assert_eq!(err.fail_code(), FailCode::StorageError);
}

#[test]
fn the_first_line_must_match_the_flag() {
    let mut create_line = Vec::new();
    write_host_message(&mut create_line, &HostMessage::Create(create())).unwrap();
    let mut start_line = Vec::new();
    write_host_message(&mut start_line, &HostMessage::Start(start())).unwrap();
    assert!(matches!(
        read_first_host_message(&mut Cursor::new(create_line.clone()), FirstLine::Create),
        Ok(Some(HostMessage::Create(_)))
    ));
    assert!(matches!(
        read_first_host_message(&mut Cursor::new(start_line.clone()), FirstLine::Start),
        Ok(Some(HostMessage::Start(_)))
    ));
    for (line, expected) in [
        (create_line, FirstLine::Start),
        (start_line, FirstLine::Create),
    ] {
        let err = read_first_host_message(&mut Cursor::new(line), expected).unwrap_err();
        assert!(matches!(err, LaunchError::Invalid(_)), "{err}");
        assert_eq!(err.fail_code(), FailCode::InvalidRequest);
    }
    let unknown = b"{\"v\":3,\"op\":\"shutdown\"}\n";
    let err =
        read_first_host_message(&mut Cursor::new(&unknown[..]), FirstLine::Start).unwrap_err();
    assert_eq!(err.fail_code(), FailCode::InvalidRequest);
    assert!(matches!(
        read_first_host_message(&mut Cursor::new(&b""[..]), FirstLine::Create),
        Ok(None)
    ));
}

#[test]
fn refused_dirs_and_folder_names_follow_one_rule() {
    let home = Path::new("/h");
    assert_eq!(
        refused_dir(home, "01M6WGY1WJCC503RXF2NS0SH84"),
        Some(PathBuf::from(
            "/h/offices/.refused/01M6WGY1WJCC503RXF2NS0SH84"
        ))
    );
    assert_eq!(refused_dir(home, "../x"), None);
    assert!(!is_plain_id(REFUSED_DIR_NAME));
    let file = office_file();
    assert!(file.names_folder("01M6WGY1WJCC503RXF2NS0SH84"));
    assert!(!file.names_folder("01M6WGY1WJCC503RXF2NS0SH83"));
    assert!(!file.names_folder("01m6wgy1wjcc503rxf2ns0sh84"));
}

#[test]
fn the_discard_message_has_the_agreed_shape_and_round_trips() {
    let message = HostMessage::Discard(discard());
    let mut out = Vec::new();
    write_host_message(&mut out, &message).unwrap();
    assert_eq!(out.last(), Some(&b'\n'));
    assert_eq!(
        json(&out),
        serde_json::json!({"v":3,"op":"discard","key_socket_id":KSID_HEX})
    );
    assert_eq!(
        read_host_message(&mut Cursor::new(out)).unwrap(),
        Some(message)
    );
}

#[test]
fn first_lines_carry_only_the_key_socket_id() {
    for message in [
        HostMessage::Create(create()),
        HostMessage::Start(start()),
        HostMessage::Discard(discard()),
    ] {
        let mut out = Vec::new();
        write_host_message(&mut out, &message).unwrap();
        let value = json(&out);
        assert_eq!(value["key_socket_id"], KSID_HEX);
        assert!(value.get("app_token").is_none() && value.get("db_key").is_none());
        let read = read_host_message(&mut Cursor::new(out)).unwrap().unwrap();
        assert_eq!(read, message);
    }
}

#[test]
fn a_first_line_carrying_a_secret_is_refused() {
    let messages = [
        (HostMessage::Create(create()), FirstLine::Create),
        (HostMessage::Start(start()), FirstLine::Start),
        (HostMessage::Discard(discard()), FirstLine::Discard),
    ];
    for (message, first_line) in messages {
        for (field, secret) in [
            ("app_token", serde_json::json!(TOKEN_HEX)),
            ("db_key", serde_json::json!(KEY_HEX)),
            ("db_key", serde_json::Value::Null),
        ] {
            let mut value = json(&encode_launch_line(&message).unwrap());
            value[field] = secret.clone();
            let line = value.to_string() + "\n";
            let err = read_first_host_message(&mut Cursor::new(line), first_line).unwrap_err();
            assert!(matches!(err, LaunchError::Json(_)), "{field}: {err}");
            assert_eq!(err.fail_code(), FailCode::InvalidRequest);
            let text = format!("{err} {err:?}");
            assert!(text.contains("no secret"), "{text}");
            assert!(
                !text.contains("abababab") && !text.contains("cdcdcdcd"),
                "{text}"
            );
        }
    }
}

#[test]
fn a_first_line_without_a_valid_key_socket_id_is_refused() {
    for message in [
        HostMessage::Create(create()),
        HostMessage::Start(start()),
        HostMessage::Discard(discard()),
    ] {
        let replacements = [
            None,
            Some(serde_json::Value::Null),
            Some(serde_json::json!(5)),
            Some(serde_json::json!("0123456789ABCDEF")),
            Some(serde_json::json!("0123456789abcde")),
            Some(serde_json::json!("/tmp/k-0123456789abcdef.sock")),
        ];
        for replacement in replacements {
            let mut value = json(&encode_launch_line(&message).unwrap());
            let object = value.as_object_mut().unwrap();
            object.remove("key_socket_id");
            if let Some(id) = replacement.clone() {
                object.insert("key_socket_id".into(), id);
            }
            let err = read_host_message(&mut Cursor::new(value.to_string() + "\n")).unwrap_err();
            assert!(
                matches!(err, LaunchError::Json(_)),
                "{replacement:?}: {err}"
            );
            assert_eq!(err.fail_code(), FailCode::InvalidRequest);
        }
    }
}

#[test]
fn key_socket_ids_are_16_lowercase_hex_characters() {
    let id = KeySocketId::generate_from([0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef]);
    assert_eq!(id.as_str(), KSID_HEX);
    assert_eq!(id.to_string(), KSID_HEX);
    assert_eq!(KeySocketId::parse(KSID_HEX).unwrap(), id);
    assert_eq!(
        KeySocketId::generate_from([0; 8]).as_str(),
        "0000000000000000"
    );
    assert_eq!(
        KeySocketId::generate_from([0xff; 8]).as_str(),
        "ffffffffffffffff"
    );
    assert_eq!(KEY_SOCKET_ID_CHARS, 16);
    assert_eq!(
        serde_json::to_string(&id).unwrap(),
        format!("\"{KSID_HEX}\"")
    );
    assert_eq!(
        serde_json::from_str::<KeySocketId>(&format!("\"{KSID_HEX}\"")).unwrap(),
        id
    );

    for bad in [
        "",
        "0123456789abcde",   // 15
        "0123456789abcdef0", // 17
        "0123456789ABCDEF",  // uppercase
        "0123456789abcdeF",
        "0123456789abcdeg", // not hex
        "0123456789abcd-f",
        "0123456789abcde\u{e9}",
        "../../../../etc/",
        "/tmp/k-0123456789abcdef.sock",
        "k-0123456789abcd",
        "../0123456789abc",
    ] {
        assert!(
            matches!(KeySocketId::parse(bad), Err(LaunchError::Invalid(_))),
            "parsed {bad:?}"
        );
        assert!(
            serde_json::from_value::<KeySocketId>(bad.into()).is_err(),
            "decoded {bad:?}"
        );
    }
    for other in ["5", "null", "[\"0123456789abcdef\"]", "{}"] {
        assert!(
            serde_json::from_str::<KeySocketId>(other).is_err(),
            "{other}"
        );
    }
}

#[test]
fn key_socket_paths_are_built_from_the_home() {
    let home = Path::new("/h");
    assert_eq!(
        key_socket_path(home, &ksid()),
        PathBuf::from("/h/run/k-0123456789abcdef.sock")
    );
    assert_eq!((KEY_SOCKET_PREFIX, KEY_SOCKET_SUFFIX), ("k-", ".sock"));
    // In the same folder as the office sockets, and never one of their names.
    let office = org_proto::location::office_socket_path(home, "01M6WGY1WJCC503RXF2NS0SH84");
    assert_eq!(
        office.unwrap().parent(),
        key_socket_path(home, &ksid()).parent()
    );
    let name = format!("{KEY_SOCKET_PREFIX}{KSID_HEX}");
    assert!(!is_plain_id(&name));
    // A typical office home leaves room under the socket path limit.
    let home = Path::new("/Users/someone/Library/Application Support/agentic.org");
    assert!(org_proto::location::check_socket_path(&key_socket_path(home, &ksid())).is_ok());
}

#[test]
fn a_version_2_line_is_refused() {
    let mut v2_create = json(&encode_launch_line(&HostMessage::Create(create())).unwrap());
    v2_create["v"] = 2.into();
    for (line, version, first_line) in [
        // Version 2's own shapes, secrets included.
        (
            format!(
                "{{\"v\":2,\"op\":\"start\",\"app_token\":\"{TOKEN_HEX}\",\"db_key\":\"{KEY_HEX}\"}}\n"
            ),
            2,
            FirstLine::Start,
        ),
        (
            "{\"v\":2,\"op\":\"discard\",\"db_key\":null}\n".to_string(),
            2,
            FirstLine::Discard,
        ),
        // A version 3 shape labelled 2.
        (v2_create.to_string() + "\n", 2, FirstLine::Create),
        (
            format!("{{\"v\":2,\"op\":\"start\",\"key_socket_id\":\"{KSID_HEX}\"}}\n"),
            2,
            FirstLine::Start,
        ),
        (
            format!("{{\"v\":1,\"op\":\"start\",\"app_token\":\"{TOKEN_HEX}\"}}\n"),
            1,
            FirstLine::Start,
        ),
    ] {
        let err = read_first_host_message(&mut Cursor::new(line), first_line).unwrap_err();
        assert!(
            matches!(err, LaunchError::UnsupportedVersion(v) if v == version),
            "{err}"
        );
        assert!(err
            .to_string()
            .contains(&format!("launch version {version} isn't supported")));
        assert_eq!(err.fail_code(), FailCode::UnsupportedVersion);
    }
    // A v2 server event is refused too: host and server ship together.
    let line = b"{\"v\":2,\"event\":\"discarded\"}\n";
    assert!(matches!(
        read_server_event(&mut Cursor::new(&line[..])),
        Err(LaunchError::UnsupportedVersion(2))
    ));
}

#[test]
fn discard_is_the_first_line_for_discard_only() {
    let mut discard_line = Vec::new();
    write_host_message(&mut discard_line, &HostMessage::Discard(discard())).unwrap();
    assert!(matches!(
        read_first_host_message(&mut Cursor::new(discard_line.clone()), FirstLine::Discard),
        Ok(Some(HostMessage::Discard(DiscardOffice { key_socket_id }))) if key_socket_id == ksid()
    ));
    for expected in [FirstLine::Create, FirstLine::Start] {
        let err =
            read_first_host_message(&mut Cursor::new(discard_line.clone()), expected).unwrap_err();
        assert!(matches!(err, LaunchError::Invalid(_)), "{err}");
        assert_eq!(err.fail_code(), FailCode::InvalidRequest);
    }
    for other in [HostMessage::Create(create()), HostMessage::Start(start())] {
        let mut line = Vec::new();
        write_host_message(&mut line, &other).unwrap();
        let err = read_first_host_message(&mut Cursor::new(line), FirstLine::Discard).unwrap_err();
        assert!(matches!(err, LaunchError::Invalid(_)), "{err}");
    }
    let unknown = b"{\"v\":3,\"op\":\"shutdown\"}\n";
    let err =
        read_first_host_message(&mut Cursor::new(&unknown[..]), FirstLine::Discard).unwrap_err();
    assert_eq!(err.fail_code(), FailCode::InvalidRequest);
    assert!(matches!(
        read_first_host_message(&mut Cursor::new(&b""[..]), FirstLine::Discard),
        Ok(None)
    ));
}

#[test]
fn the_secrets_never_show_in_debug_output() {
    let hand_offs = [hand_off(Some(key())), hand_off(None)];
    for hand_off in &hand_offs {
        for shown in [format!("{hand_off:?}"), format!("{hand_off:#?}")] {
            assert!(
                !shown.contains("cdcd")
                    && !shown.contains("abab")
                    // A derived Debug would print the bytes in decimal.
                    && !shown.contains("205")
                    && !shown.contains("171"),
                "{shown}"
            );
            assert!(shown.contains("AppToken(<redacted>)"), "{shown}");
        }
    }
    let shown = format!("{:?}", hand_offs[0]);
    assert!(shown.contains("DbKey(<redacted>)"), "{shown}");
}

#[test]
fn hand_off_lines_are_zeroed_and_sent_without_reallocating() {
    let app_token = token();
    for with_key in [true, false] {
        let db_key = with_key.then(key);
        let mut line = encode_launch_line(&HandOffRef {
            app_token: &app_token,
            db_key: db_key.as_ref(),
        })
        .unwrap();
        assert_eq!(line.capacity(), line.len());
        let mut out = Vec::new();
        send_and_zero(&mut out, &mut line).unwrap();
        assert!(line.iter().all(|&b| b == 0));

        let mut read = out[..out.len() - 1].to_vec();
        assert_eq!(decode_hand_off(&mut read).unwrap(), hand_off(db_key));
        assert!(read.iter().all(|&b| b == 0));
    }
}

#[test]
fn a_version_1_office_file_still_reads_and_is_written_as_version_1() {
    let stored = br#"{"v":1,"office_id":"01M6WGY1WJCC503RXF2NS0SH84","office_name":"HQ","role":"hq","org_id":"01M6WGY1WJCC503RXF2NS0SH83","org_name":"Acme","created_at":"2026-11-08T02:10:00.000Z"}"#;
    let file = OfficeFile::from_slice(stored).unwrap();
    assert_eq!(file, office_file());
    assert_eq!(json(&file.to_vec().unwrap())["v"], 1);
    assert_eq!(OFFICE_FILE_VERSION, 1);
    assert_eq!(LAUNCH_VERSION, 3);
    // The office inside a v3 `created` event keeps its own v1.
    let mut out = Vec::new();
    write_server_event(&mut out, &ServerEvent::Created { office: file }).unwrap();
    let event = json(&out);
    assert_eq!(
        (event["v"].clone(), event["office"]["v"].clone()),
        (3.into(), 1.into())
    );
}

#[test]
fn the_borrowed_hand_off_writer_writes_the_owned_encoding() {
    for (db_key, expected) in [
        (
            Some(key()),
            format!("{{\"v\":3,\"app_token\":\"{TOKEN_HEX}\",\"db_key\":\"{KEY_HEX}\"}}\n"),
        ),
        (
            None,
            format!("{{\"v\":3,\"app_token\":\"{TOKEN_HEX}\",\"db_key\":null}}\n"),
        ),
    ] {
        let owned = hand_off(db_key);
        let mut written = Vec::new();
        write_hand_off(&mut written, &owned.app_token, owned.db_key.as_ref()).unwrap();
        assert_eq!(written, expected.as_bytes());
        // What is read back is the owned hand-off, and it writes the same bytes again.
        let read = read_hand_off(&mut Cursor::new(written.clone()), FirstLine::Discard).unwrap();
        assert_eq!(read, owned);
        let mut again = Vec::new();
        write_hand_off(&mut again, &read.app_token, read.db_key.as_ref()).unwrap();
        assert_eq!(again, written);
    }
}

#[test]
fn the_hand_off_round_trips_with_a_key_and_with_null() {
    for first_line in FIRST_LINES {
        let read = read_hand_off(&mut Cursor::new(hand_off_line(true)), first_line).unwrap();
        assert_eq!(read, hand_off(Some(key())));
        assert_eq!(
            read.db_key.as_ref().map(DbKey::expose_bytes),
            Some(&[0xcd; DB_KEY_BYTES])
        );
    }
    let read = read_hand_off(&mut Cursor::new(hand_off_line(false)), FirstLine::Discard).unwrap();
    assert_eq!(read, hand_off(None));
    assert_eq!(
        json(&hand_off_line(false))["db_key"],
        serde_json::Value::Null
    );
}

#[test]
fn a_null_key_is_refused_where_one_is_required() {
    for first_line in [FirstLine::Create, FirstLine::Start] {
        let err = read_hand_off(&mut Cursor::new(hand_off_line(false)), first_line).unwrap_err();
        assert!(matches!(err, LaunchError::Invalid(_)), "{err}");
        assert_eq!(err.fail_code(), FailCode::InvalidRequest);
        assert!(!err.is_host_gone());
    }
}

#[test]
fn the_hand_off_checks_its_version_first_and_refuses_unknown_fields() {
    for (line, version) in [
        (
            format!("{{\"v\":2,\"app_token\":\"{TOKEN_HEX}\",\"db_key\":\"{KEY_HEX}\"}}\n"),
            2,
        ),
        ("{\"v\":4,\"secrets\":{\"nested\":true}}\n".to_string(), 4),
    ] {
        for first_line in FIRST_LINES {
            let err = read_hand_off(&mut Cursor::new(line.clone()), first_line).unwrap_err();
            assert!(
                matches!(err, LaunchError::UnsupportedVersion(v) if v == version),
                "{err}"
            );
            assert_eq!(err.fail_code(), FailCode::UnsupportedVersion);
        }
    }
    let token_field = format!("\"app_token\":\"{TOKEN_HEX}\"");
    let key_field = format!("\"db_key\":\"{KEY_HEX}\"");
    for line in [
        // Unknown fields, the first line's own included.
        format!("{{\"v\":3,{token_field},{key_field},\"x\":1}}\n"),
        format!("{{\"v\":3,{token_field},{key_field},\"key_socket_id\":\"{KSID_HEX}\"}}\n"),
        format!("{{\"v\":3,{token_field},{key_field},\"op\":\"start\"}}\n"),
        // Missing fields: `db_key` must be present, `null` included, even for a discard.
        format!("{{\"v\":3,{token_field}}}\n"),
        format!("{{\"v\":3,{key_field}}}\n"),
        format!("{{\"v\":3,\"app_token\":null,{key_field}}}\n"),
        format!("{{{token_field},{key_field}}}\n"),
        "{}\n".to_string(),
    ] {
        for first_line in FIRST_LINES {
            let err = read_hand_off(&mut Cursor::new(line.clone()), first_line).unwrap_err();
            assert!(matches!(err, LaunchError::Json(_)), "{line}: {err}");
            assert_eq!(err.fail_code(), FailCode::InvalidRequest);
        }
    }
}
