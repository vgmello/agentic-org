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

fn start() -> StartOffice {
    StartOffice {
        app_token: token(),
        db_key: key(),
    }
}

fn create() -> CreateOffice {
    CreateOffice {
        org_id: "01M6WGY1WJCC503RXF2NS0SH83".into(),
        org_name: "Acme".into(),
        office_id: "01M6WGY1WJCC503RXF2NS0SH84".into(),
        office_name: "HQ".into(),
        role: OfficeRole::Hq,
        app_token: token(),
        db_key: key(),
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

#[test]
fn selected_marker_round_trip_keeps_version_and_explicit_uuid_identity() {
    let office = "019a8928-1337-7456-8456-0123456789ac";
    let org = "019a8928-1337-7123-8123-0123456789ab";
    assert!(!is_plain_id(office));
    assert!(is_storage_id(office));
    for invalid in [
        "019a8928-1337-4456-8456-0123456789ac",
        "019A8928-1337-7456-8456-0123456789ac",
        "019a8928-1337-7456-7456-0123456789ac",
        "../019a8928-1337-7456-8456-0123456789ac",
    ] {
        assert!(!is_selected_entity_id(invalid));
        assert!(!is_storage_id(invalid));
    }
    assert!(OfficeFile::new(
        office,
        "HQ",
        OfficeRole::Hq,
        org,
        "Example",
        "2026-11-15T00:00:00.000Z"
    )
    .to_vec()
    .is_err());
    let selected = OfficeFile::selected(
        office,
        "HQ",
        OfficeRole::Hq,
        org,
        "Example",
        "2026-11-15T00:00:00.000Z",
    )
    .unwrap();
    let bytes = selected.to_vec().unwrap();
    assert_eq!(OfficeFile::from_slice(&bytes).unwrap(), selected);
    assert_eq!(selected.version(), 2);
    assert_eq!(office_file().version(), 1);
    let mut extended = json(&bytes);
    extended["extension"] = serde_json::json!({"unknown":"retained"});
    let parsed = OfficeFile::from_slice(&serde_json::to_vec(&extended).unwrap()).unwrap();
    assert_eq!(
        json(&parsed.to_vec().unwrap())["extension"],
        extended["extension"]
    );
    assert_eq!(json(&parsed.to_vec().unwrap())["v"], 2);
    assert_eq!(
        office_dir(Path::new("/tmp/private"), office),
        Some(Path::new("/tmp/private/offices").join(office))
    );
}

/// Decodes one line the kept v2 writers emit, with serde alone. No worker reads v2 any more (the
/// v2 readers went in A1.5 step 3a), so this checks the writers' bytes and the types' own rules,
/// not a reader's. Removed with the writers in A1.5 step 3b (host carrier, test fake).
fn decode_host(r: &mut impl BufRead) -> Result<Option<HostMessage>, LaunchError> {
    let Some(line) = frame::read_line(r).map_err(LaunchError::Frame)? else {
        return Ok(None);
    };
    serde_json::from_slice(&line)
        .map(Some)
        .map_err(LaunchError::Json)
}

/// As [`decode_host`], for a v2 server event.
fn decode_event(r: &mut impl BufRead) -> Result<Option<ServerEvent>, LaunchError> {
    let Some(line) = frame::read_line(r).map_err(LaunchError::Frame)? else {
        return Ok(None);
    };
    serde_json::from_slice(&line)
        .map(Some)
        .map_err(LaunchError::Json)
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
        serde_json::json!({"v":2,"op":"create","org_id":"01M6WGY1WJCC503RXF2NS0SH83",
            "org_name":"Acme","office_id":"01M6WGY1WJCC503RXF2NS0SH84",
            "office_name":"HQ","role":"hq","app_token":TOKEN_HEX,"db_key":KEY_HEX})
    );
}

#[test]
fn server_events_have_the_agreed_shapes() {
    let cases = [
        (
            ServerEvent::Ready {
                socket: "/h/run/3RXF2NS0SH84.sock".into(),
                pid: 4242,
            },
            serde_json::json!({"v":2,"event":"ready","socket":"/h/run/3RXF2NS0SH84.sock","pid":4242}),
        ),
        (
            ServerEvent::Failed {
                code: FailCode::KeyMissing,
                message: "the office's key is missing".into(),
            },
            serde_json::json!({"v":2,"event":"failed","code":"key_missing",
                "message":"the office's key is missing"}),
        ),
        (
            ServerEvent::Created {
                office: office_file(),
            },
            serde_json::json!({"v":2,"event":"created","office":{"v":1,
                "office_id":"01M6WGY1WJCC503RXF2NS0SH84","office_name":"HQ","role":"hq",
                "org_id":"01M6WGY1WJCC503RXF2NS0SH83","org_name":"Acme",
                "created_at":"2026-11-08T02:10:00.000Z"}}),
        ),
    ];
    for (event, expected) in cases {
        let mut out = Vec::new();
        write_server_event(&mut out, &event).unwrap();
        assert_eq!(json(&out), expected);
        assert_eq!(decode_event(&mut Cursor::new(out)).unwrap(), Some(event));
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
    let line = br#"{"v":2,"op":"shutdown","grace_ms":5}
"#;
    assert_eq!(
        decode_host(&mut Cursor::new(&line[..])).unwrap(),
        Some(HostMessage::Unknown)
    );

    let line = br#"{"v":2,"event":"status","busy":true}
"#;
    assert_eq!(
        decode_event(&mut Cursor::new(&line[..])).unwrap(),
        Some(ServerEvent::Unknown)
    );

    let line = br#"{"v":2,"event":"failed","code":"disk_on_fire","message":"m","extra":1}
"#;
    assert_eq!(
        decode_event(&mut Cursor::new(&line[..])).unwrap(),
        Some(ServerEvent::Failed {
            code: FailCode::StorageError,
            message: "m".into()
        })
    );

    let line = br#"{"v":2,"op":"create","org_id":"A1","org_name":"Acme","office_id":"B2","office_name":"HQ","role":"hq","hq_address":"x","app_token":"abababababababababababababababababababababababababababababababab","db_key":"cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd"}
"#;
    let Some(HostMessage::Create(c)) = decode_host(&mut Cursor::new(&line[..])).unwrap() else {
        panic!("not a create");
    };
    assert_eq!(c.role, OfficeRole::Hq);
}

#[test]
fn a_create_with_a_role_this_version_cant_create_is_refused() {
    for line in [
        &br#"{"v":2,"op":"create","org_id":"A1","org_name":"Acme","office_id":"B2","office_name":"HQ","role":"satellite","hq_address":"x","app_token":"abababababababababababababababababababababababababababababababab","db_key":"cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd"}
"#[..],
        &br#"{"v":2,"op":"create","org_id":"A1","org_name":"Acme","office_id":"B2","office_name":"HQ","role":"moon","app_token":"abababababababababababababababababababababababababababababababab","db_key":"cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd"}
"#[..],
        &br#"{"v":2,"op":"create","org_id":"A1","org_name":"Acme","office_id":"B2","office_name":"HQ","app_token":"abababababababababababababababababababababababababababababababab","db_key":"cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd"}
"#[..],
    ] {
        let err = decode_host(&mut Cursor::new(line)).unwrap_err();
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
    // Reading with the plain frame API, is still refused by the type itself.
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
    let line = serde_json::json!({"v":2,"event":"created","office":office}).to_string() + "\n";
    assert!(matches!(
        decode_event(&mut Cursor::new(line)),
        Err(LaunchError::Json(_))
    ));

    let mut office = serde_json::to_value(office_file()).unwrap();
    office["v"] = 7.into();
    let line = serde_json::json!({"v":2,"event":"created","office":office}).to_string() + "\n";
    assert!(matches!(
        decode_event(&mut Cursor::new(line)),
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
fn an_oversized_server_event_is_not_sent() {
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
        line["v"] = 2.into();
        line["org_id"] = org_id.into();
        line["office_id"] = office_id.into();
        let line = line.to_string() + "\n";
        assert!(
            matches!(
                decode_host(&mut Cursor::new(line)),
                Err(LaunchError::Json(_))
            ),
            "accepted {org_id:?} / {office_id:?}"
        );
    }
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
    assert_eq!(json(&out), serde_json::json!({"v":2,"event":"discarded"}));
    assert_eq!(
        decode_event(&mut Cursor::new(out)).unwrap(),
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
    let bad_json = decode_event(&mut Cursor::new(&b"{]\n"[..])).unwrap_err();
    assert_eq!(bad_json.fail_code(), FailCode::InvalidRequest);
}

#[test]
fn office_file_checks_version_and_ids() {
    // The selected marker version is not permission to reinterpret legacy IDs.
    let mut value = serde_json::to_value(office_file()).unwrap();
    value["v"] = 2.into();
    assert!(matches!(
        OfficeFile::from_slice(&serde_json::to_vec(&value).unwrap()),
        Err(LaunchError::Invalid(_))
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
        serde_json::json!({"v":2,"op":"start","app_token":TOKEN_HEX,"db_key":KEY_HEX})
    );
    assert_eq!(decode_host(&mut Cursor::new(out)).unwrap(), Some(start));
}

#[test]
fn the_app_token_never_shows_in_debug_or_errors() {
    let shown = format!(
        "{:?} {:?}",
        HostMessage::Create(create()),
        HostMessage::Start(start())
    );
    assert!(!shown.contains("abab"), "{shown}");

    // A bad token, and a create missing one, fail without echoing the line.
    let bad = format!("{}Z", &TOKEN_HEX[..63]);
    for line in [
        format!("{{\"v\":2,\"op\":\"start\",\"app_token\":\"{bad}\",\"db_key\":\"{KEY_HEX}\"}}\n"),
        format!(
            "{{\"v\":2,\"op\":\"start\",\"app_token\":{},\"db_key\":\"{KEY_HEX}\"}}\n",
            "1234567890".repeat(7)
        ),
    ] {
        let err = decode_host(&mut Cursor::new(line)).unwrap_err();
        let text = format!("{err} {err:?}");
        assert!(
            !text.contains("abababab") && !text.contains("12345678"),
            "{text}"
        );
        assert_eq!(err.fail_code(), FailCode::InvalidRequest);
    }
    let mut value = serde_json::to_value(HostMessage::Create(create())).unwrap();
    value.as_object_mut().unwrap().remove("app_token");
    value["v"] = 2.into();
    let err = decode_host(&mut Cursor::new(value.to_string() + "\n")).unwrap_err();
    assert!(matches!(err, LaunchError::Json(_)), "{err}");
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
fn host_messages_are_flushed_as_one_line() {
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
    let mut w = Recorder {
        bytes: Vec::new(),
        flushed_at: None,
    };
    write_host_message(&mut w, &HostMessage::Start(start())).unwrap();
    assert_eq!(w.flushed_at, Some(w.bytes.len()));
    assert_eq!(w.bytes.iter().filter(|&&b| b == b'\n').count(), 1);
    assert_eq!(w.bytes.last(), Some(&b'\n'));
}

#[test]
fn a_newer_office_file_reports_its_version_even_with_another_shape() {
    assert!(matches!(
        OfficeFile::from_slice(br#"{"v":3,"office":{"id":"x"}}"#),
        Err(LaunchError::UnsupportedOfficeFileVersion(3))
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
fn host_lines_are_encoded_without_reallocating() {
    let line = encode_host_line(&HostMessage::Create(create())).unwrap();
    assert_eq!(line.capacity(), line.len());
    assert_eq!(line.last(), Some(&b'\n'));
}

#[test]
fn a_host_line_read_in_pieces_stays_in_its_first_buffer() {
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
    let line = encode_host_line(&HostMessage::Start(start())).unwrap();
    let mut chunks = Vec::new();
    for piece in line.chunks(16) {
        chunks.push(piece.to_vec());
    }
    let mut r = std::io::BufReader::with_capacity(16, Chunks(chunks));
    let read = read_secret_line(&mut r).unwrap().unwrap();
    assert_eq!(read.capacity(), org_proto::MAX_FRAME_BYTES + 1);
    assert_eq!(read, line[..line.len() - 1]);
}

#[test]
fn the_largest_host_line_is_sent_and_one_byte_more_is_not() {
    let base = encode_host_line(&HostMessage::Create(create())).unwrap();
    // The line without its newline may be exactly MAX_FRAME_BYTES long.
    let pad = org_proto::MAX_FRAME_BYTES - (base.len() - 1);
    let fits = HostMessage::Create(CreateOffice {
        org_name: format!("Acme{}", "x".repeat(pad)),
        ..create()
    });
    let mut out = Vec::new();
    write_host_message(&mut out, &fits).unwrap();
    assert_eq!(out.len(), org_proto::MAX_FRAME_BYTES + 1);
    assert!(decode_host(&mut Cursor::new(out)).unwrap().is_some());

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
            "{{\"v\":2,\"op\":\"create\",\"org_id\":\"A1\",\"org_name\":\"Acme\",\
             \"office_id\":\"B2\",\"office_name\":\"HQ\",\"role\":{role},\
             \"app_token\":\"{TOKEN_HEX}\",\"db_key\":\"{KEY_HEX}\"}}\n"
        );
        let err = decode_host(&mut Cursor::new(line)).unwrap_err().to_string();
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
fn a_broken_host_line_is_zeroed() {
    let line = encode_host_line(&HostMessage::Start(start())).unwrap();
    let mut buf = Vec::with_capacity(org_proto::MAX_FRAME_BYTES + 1);
    // Truncated: the host died mid-line, after part of the token.
    let cut = &line[..line.len() - 10];
    assert!(matches!(
        read_secret_line_into(&mut Cursor::new(cut), &mut buf),
        Err(LaunchError::Frame(FrameError::Truncated))
    ));
    assert!(!buf.is_empty() && buf.iter().all(|&b| b == 0));
    // Too large: a token followed by more than a line's worth.
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
        decode_event(&mut Cursor::new(out)).unwrap(),
        Some(ServerEvent::Created { .. })
    ));

    let over = office_file_of_len(MAX_OFFICE_FILE_BYTES + 1);
    assert!(over.to_vec().is_err());
    let mut out = Vec::new();
    assert!(write_server_event(&mut out, &ServerEvent::Created { office: over }).is_err());
    assert!(out.is_empty());
}

#[test]
fn a_sent_host_line_is_zeroed_whether_or_not_the_write_works() {
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("closed"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let message = HostMessage::Start(start());
    let mut line = encode_host_line(&message).unwrap();
    let mut out = Vec::new();
    send_and_zero(&mut out, &mut line).unwrap();
    assert!(out.ends_with(b"\n") && out.len() == line.len());
    assert!(line.iter().all(|&b| b == 0));

    let mut line = encode_host_line(&message).unwrap();
    assert!(send_and_zero(&mut Broken, &mut line).is_err());
    assert!(!line.is_empty() && line.iter().all(|&b| b == 0));
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
fn the_discard_message_has_the_agreed_shapes_and_round_trips() {
    for (discard, expected) in [
        (
            DiscardOffice {
                db_key: Some(key()),
            },
            serde_json::json!({"v":2,"op":"discard","db_key":KEY_HEX}),
        ),
        (
            DiscardOffice { db_key: None },
            serde_json::json!({"v":2,"op":"discard","db_key":null}),
        ),
    ] {
        let message = HostMessage::Discard(discard);
        let mut out = Vec::new();
        write_host_message(&mut out, &message).unwrap();
        assert_eq!(out.last(), Some(&b'\n'));
        assert_eq!(json(&out), expected);
        assert_eq!(decode_host(&mut Cursor::new(out)).unwrap(), Some(message));
    }
}

#[test]
fn create_and_start_carry_the_key_and_round_trip() {
    for message in [HostMessage::Create(create()), HostMessage::Start(start())] {
        let mut out = Vec::new();
        write_host_message(&mut out, &message).unwrap();
        assert_eq!(json(&out)["db_key"], KEY_HEX);
        let read = decode_host(&mut Cursor::new(out)).unwrap().unwrap();
        let read_key = match &read {
            HostMessage::Create(c) => &c.db_key,
            HostMessage::Start(s) => &s.db_key,
            other => panic!("read {other:?}"),
        };
        assert_eq!(read_key.expose_bytes(), &[0xcd; DB_KEY_BYTES]);
        assert_eq!(read, message);
    }
    // A different key is a different line.
    let other = HostMessage::Start(StartOffice {
        db_key: DbKey::from_bytes([0xce; DB_KEY_BYTES]),
        ..start()
    });
    assert_ne!(other, HostMessage::Start(start()));
}

#[test]
fn a_create_or_start_without_a_key_is_refused() {
    for message in [HostMessage::Create(create()), HostMessage::Start(start())] {
        for db_key in [None, Some(serde_json::Value::Null)] {
            let mut value = json(&encode_host_line(&message).unwrap());
            let object = value.as_object_mut().unwrap();
            object.remove("db_key");
            if let Some(db_key) = db_key.clone() {
                object.insert("db_key".into(), db_key);
            }
            let err = decode_host(&mut Cursor::new(value.to_string() + "\n")).unwrap_err();
            assert!(matches!(err, LaunchError::Json(_)), "{db_key:?}: {err}");
            assert_eq!(err.fail_code(), FailCode::InvalidRequest);
        }
    }
    // A discard needs the field too, `null` included: a missing one isn't read as `null`.
    let err = decode_host(&mut Cursor::new(&b"{\"v\":2,\"op\":\"discard\"}\n"[..])).unwrap_err();
    assert!(matches!(err, LaunchError::Json(_)), "{err}");
}

#[test]
fn a_bad_key_is_refused_without_echoing_it() {
    let bad = format!("{}Z", &KEY_HEX[..63]);
    for line in [
        format!(
            "{{\"v\":2,\"op\":\"start\",\"app_token\":\"{TOKEN_HEX}\",\"db_key\":\"{bad}\"}}\n"
        ),
        format!("{{\"v\":2,\"op\":\"discard\",\"db_key\":\"{bad}\"}}\n"),
        format!(
            "{{\"v\":2,\"op\":\"discard\",\"db_key\":\"{}\"}}\n",
            &KEY_HEX[..62]
        ),
        format!(
            "{{\"v\":2,\"op\":\"discard\",\"db_key\":{}}}\n",
            "1234567890".repeat(7)
        ),
    ] {
        let err = decode_host(&mut Cursor::new(line)).unwrap_err();
        let text = format!("{err} {err:?}");
        assert!(
            !text.contains("cdcdcdcd") && !text.contains("12345678"),
            "{text}"
        );
        assert_eq!(err.fail_code(), FailCode::InvalidRequest);
    }
}

#[test]
fn the_key_never_shows_in_debug_output() {
    let lines = [
        HostMessage::Create(create()),
        HostMessage::Start(start()),
        HostMessage::Discard(DiscardOffice {
            db_key: Some(key()),
        }),
        HostMessage::Discard(DiscardOffice { db_key: None }),
    ];
    for line in &lines {
        for shown in [format!("{line:?}"), format!("{line:#?}")] {
            assert!(
                !shown.contains("cdcd")
                    && !shown.contains("abab")
                    // A derived Debug would print the bytes in decimal.
                    && !shown.contains("205")
                    && !shown.contains("171"),
                "{shown}"
            );
        }
    }
    let shown = format!("{:?}", lines[0]);
    assert!(shown.contains("DbKey(<redacted>)"), "{shown}");
}

#[test]
fn key_lines_are_zeroed_and_sent_without_reallocating() {
    for message in [
        HostMessage::Create(create()),
        HostMessage::Start(start()),
        HostMessage::Discard(DiscardOffice {
            db_key: Some(key()),
        }),
    ] {
        let mut line = encode_host_line(&message).unwrap();
        assert_eq!(line.capacity(), line.len());
        let mut out = Vec::new();
        send_and_zero(&mut out, &mut line).unwrap();
        assert!(line.iter().all(|&b| b == 0));

        assert_eq!(decode_host(&mut Cursor::new(&out)).unwrap(), Some(message));
    }
}

#[test]
fn a_version_1_office_file_still_reads_and_is_written_as_version_1() {
    let stored = br#"{"v":1,"office_id":"01M6WGY1WJCC503RXF2NS0SH84","office_name":"HQ","role":"hq","org_id":"01M6WGY1WJCC503RXF2NS0SH83","org_name":"Acme","created_at":"2026-11-08T02:10:00.000Z"}"#;
    let file = OfficeFile::from_slice(stored).unwrap();
    assert_eq!(file, office_file());
    assert_eq!(json(&file.to_vec().unwrap())["v"], 1);
    assert_eq!(OFFICE_FILE_VERSION, 1);
    assert_eq!(LAUNCH_VERSION, 2);
    // The office inside a v2 `created` event keeps its own v1.
    let mut out = Vec::new();
    write_server_event(&mut out, &ServerEvent::Created { office: file }).unwrap();
    let event = json(&out);
    assert_eq!(
        (event["v"].clone(), event["office"]["v"].clone()),
        (2.into(), 1.into())
    );
}
