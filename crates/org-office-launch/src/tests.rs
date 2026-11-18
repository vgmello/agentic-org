use std::io::Cursor;
use std::path::Path;

use super::*;
use crate::write_server_event;

/// A fixed database key's wire form, for the secret-line reader's tests.
const KEY_HEX: &str = "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";

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

/// Reads one worker event as the host does ([`crate::read_server_event`]).
fn decode_event(r: &mut impl BufRead) -> Result<Option<ServerEvent>, LaunchError> {
    Ok(crate::read_server_event(r)?.map(|event| match event {
        crate::RendezvousEvent::Server(event) => event,
        other => panic!("not a worker event: {other:?}"),
    }))
}

/// A line holding a key, with its `\n`, for the secret-line reader's tests.
fn secret_line() -> Vec<u8> {
    format!("{{\"v\":3,\"db_key\":\"{KEY_HEX}\"}}\n").into_bytes()
}

fn json(bytes: &[u8]) -> serde_json::Value {
    serde_json::from_slice(bytes).unwrap()
}

#[test]
fn server_events_have_the_agreed_shapes() {
    let cases = [
        (
            ServerEvent::Ready {
                socket: "/h/run/3RXF2NS0SH84.sock".into(),
                pid: 4242,
            },
            serde_json::json!({"v":1,"event":"ready","socket":"/h/run/3RXF2NS0SH84.sock","pid":4242}),
        ),
        (
            ServerEvent::Failed {
                code: FailCode::InUse,
                message: "the office is in use".into(),
            },
            serde_json::json!({"v":1,"event":"failed","code":"in_use",
                "message":"the office is in use"}),
        ),
        (
            ServerEvent::Created {
                office: office_file(),
            },
            serde_json::json!({"v":1,"event":"created","office":{"v":1,
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
fn unknown_events_and_failure_codes_are_tolerated() {
    let line = br#"{"v":1,"event":"status","busy":true}
"#;
    assert_eq!(
        decode_event(&mut Cursor::new(&line[..])).unwrap(),
        Some(ServerEvent::Unknown)
    );

    // An unknown code, the retired `key_missing` included, is a retried storage error.
    for code in ["disk_on_fire", "key_missing"] {
        let line =
            format!("{{\"v\":1,\"event\":\"failed\",\"code\":\"{code}\",\"message\":\"m\"}}\n");
        assert_eq!(
            decode_event(&mut Cursor::new(line.as_bytes())).unwrap(),
            Some(ServerEvent::Failed {
                code: FailCode::StorageError,
                message: "m".into()
            }),
            "{code}"
        );
    }
}

#[test]
fn the_types_check_their_rules_on_every_serde_path() {
    // Encoding a bad office file through serde directly fails too.
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
    let line = serde_json::json!({"v":1,"event":"created","office":office}).to_string() + "\n";
    assert!(matches!(
        decode_event(&mut Cursor::new(line)),
        Err(LaunchError::Json(_))
    ));

    let mut office = serde_json::to_value(office_file()).unwrap();
    office["v"] = 7.into();
    let line = serde_json::json!({"v":1,"event":"created","office":office}).to_string() + "\n";
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
    assert_eq!(json(&out), serde_json::json!({"v":1,"event":"discarded"}));
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
    assert!(serde_json::to_string(&ServerEvent::Unknown).is_err());
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
    let line = secret_line();
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
    let line = secret_line();
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
fn a_version_1_office_file_still_reads_and_is_written_as_version_1() {
    let stored = br#"{"v":1,"office_id":"01M6WGY1WJCC503RXF2NS0SH84","office_name":"HQ","role":"hq","org_id":"01M6WGY1WJCC503RXF2NS0SH83","org_name":"Acme","created_at":"2026-11-08T02:10:00.000Z"}"#;
    let file = OfficeFile::from_slice(stored).unwrap();
    assert_eq!(file, office_file());
    assert_eq!(json(&file.to_vec().unwrap())["v"], 1);
    assert_eq!(OFFICE_FILE_VERSION, 1);
    assert_eq!(crate::LAUNCH_VERSION, 1);
    // The office inside a `created` event keeps its own version.
    let mut out = Vec::new();
    write_server_event(&mut out, &ServerEvent::Created { office: file }).unwrap();
    let event = json(&out);
    assert_eq!(
        (event["v"].clone(), event["office"]["v"].clone()),
        (1.into(), 1.into())
    );
}
