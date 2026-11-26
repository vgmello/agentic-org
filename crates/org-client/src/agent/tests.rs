use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::PathBuf;

use org_proto::OfficeRole;

use super::*;

const A: &str = "01OFFICE0000000000000000AA";
const B: &str = "01OFFICE0000000000000000BB";

fn home() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::Builder::new()
        .prefix("oca")
        .tempdir_in("/tmp")
        .unwrap();
    let home = dir.path().to_path_buf();
    (dir, home)
}

fn office_file(id: &str) -> Vec<u8> {
    OfficeFile::new(
        id,
        "HQ",
        OfficeRole::Hq,
        "01ORG0000000000000000ACME1",
        "Acme",
        "2026-11-09T10:00:00.000Z",
    )
    .to_vec()
    .unwrap()
}

/// `<home>/offices/<folder>/office.json` holding office `id`.
fn put(home: &Path, folder: &str, id: &str) {
    let dir = home.join(OFFICES_DIR_NAME).join(folder);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(OFFICE_FILE_NAME), office_file(id)).unwrap();
}

fn spooled(office_id: &str, id: &str, body: &str) -> SpooledSend {
    SpooledSend {
        v: org_proto::msg::SPOOL_VERSION,
        office_id: office_id.into(),
        from: "pixel".into(),
        id: id.into(),
        to: "nova".into(),
        kind: org_proto::msg::KIND_MSG.into(),
        priority: org_proto::msg::MessagePriority::Normal,
        subject: None,
        body: body.into(),
        meta: Default::default(),
        reply_to: None,
        created_at: "2026-11-09T10:00:00.000Z".into(),
    }
}

const MSG: &str = "01MSG0000000000000000000A1";

#[test]
fn a_spooled_send_lands_in_the_offices_spool_owner_only() {
    let (_dir, home) = home();
    put(&home, A, A);
    let placed = spool_send(&home, &spooled(A, MSG, "hello")).unwrap();
    let office = home.join(OFFICES_DIR_NAME).join(A);
    assert_eq!(placed, office.join("spool/new").join(format!("{MSG}.json")));
    let read: SpooledSend = serde_json::from_slice(&fs::read(&placed).unwrap()).unwrap();
    assert_eq!(read, spooled(A, MSG, "hello"));
    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&placed), 0o600);
    for dir in ["spool", "spool/tmp", "spool/new"] {
        assert_eq!(mode(&office.join(dir)), 0o700, "{dir}");
    }
    assert_eq!(fs::read_dir(office.join("spool/tmp")).unwrap().count(), 0);
    // The same id again is refused, never replacing the waiting send or leaving a copy.
    assert!(spool_send(&home, &spooled(A, MSG, "again")).is_err());
    let still: SpooledSend = serde_json::from_slice(&fs::read(&placed).unwrap()).unwrap();
    assert_eq!(still.body, "hello");
    assert_eq!(fs::read_dir(office.join("spool/tmp")).unwrap().count(), 0);
}

#[test]
fn spooling_never_creates_an_office_or_follows_a_symlink() {
    let (_dir, home) = home();
    assert!(matches!(
        spool_send(&home, &spooled(A, MSG, "x")),
        Err(SpoolSendError::NoSuchOffice)
    ));
    assert!(!home.join(OFFICES_DIR_NAME).join(A).exists());
    assert!(matches!(
        spool_send(&home, &spooled("../x", MSG, "x")),
        Err(SpoolSendError::NoSuchOffice)
    ));
    assert!(matches!(
        spool_send(&home, &spooled(A, "../../evil", "x")),
        Err(SpoolSendError::Invalid(_))
    ));
    // A spool folder that is a symlink elsewhere is refused, and nothing lands there.
    put(&home, A, A);
    let elsewhere = home.join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    symlink(
        &elsewhere,
        home.join(OFFICES_DIR_NAME).join(A).join("spool"),
    )
    .unwrap();
    assert!(matches!(
        spool_send(&home, &spooled(A, MSG, "x")),
        Err(SpoolSendError::NotAFolder(_))
    ));
    assert_eq!(fs::read_dir(&elsewhere).unwrap().count(), 0);
}

#[test]
fn a_send_the_office_would_not_ingest_is_refused() {
    use org_proto::msg::{json_string_len, MAX_BODY_BYTES};
    let (_dir, home) = home();
    put(&home, A, A);
    // A body just over the office's limit (as JSON), though the file would fit 512 KiB.
    let over = "x".repeat(MAX_BODY_BYTES - 1);
    assert_eq!(json_string_len(&over), MAX_BODY_BYTES + 1);
    let refused = spool_send(&home, &spooled(A, MSG, &over)).unwrap_err();
    assert!(refused.to_string().contains("body"), "{refused}");
    let at = "x".repeat(MAX_BODY_BYTES - 2);
    spool_send(&home, &spooled(A, MSG, &at)).unwrap();
    let mut subject = spooled(A, "01MSG0000000000000000000A2", "x");
    subject.subject = Some("s".repeat(2000));
    assert!(spool_send(&home, &subject)
        .unwrap_err()
        .to_string()
        .contains("subject"));
    let refused = |change: &dyn Fn(&mut SpooledSend)| {
        let mut send = spooled(A, "01MSG0000000000000000000A3", "x");
        change(&mut send);
        matches!(spool_send(&home, &send), Err(SpoolSendError::Invalid(_)))
    };
    assert!(refused(&|s| s.to = " Office ".into()));
    assert!(refused(&|s| s.to = String::new()));
    assert!(refused(&|s| s.to = " \t".into()));
    assert!(refused(&|s| s.to = "n".repeat(300)));
    assert!(refused(&|s| s.from = "owner".into()));
    assert!(refused(&|s| s.from = "office".into()));
    assert!(refused(&|s| s.from = " RUN:o1/r1 ".into()));
    assert!(refused(&|s| s.v = 2));
    assert!(refused(&|s| s.reply_to = Some("NOTAMESSAGEID".into())));
    assert!(refused(&|s| s.kind = "request".into()));
    assert!(refused(&|s| s.kind = String::new()));
    assert!(refused(
        &|s| s.priority = org_proto::msg::MessagePriority::Unknown
    ));
    assert!(refused(&|s| s.reply_to = Some("r".repeat(200))));
    assert!(refused(&|s| {
        s.meta
            .insert("k".into(), serde_json::json!("m".repeat(70 * 1024)));
    }));
    // An id that is plain but not a ULID.
    assert!(matches!(
        spool_send(&home, &spooled(A, "NOTAULID", "x")),
        Err(SpoolSendError::Invalid(_))
    ));
    // What ingest accepts is not refused.
    let accepted = |n: u8, change: &dyn Fn(&mut SpooledSend)| {
        let id = format!("01MSG00000000000000000000{}", char::from(b'B' + n));
        let mut send = spooled(A, &id, "x");
        change(&mut send);
        spool_send(&home, &send)
            .map(|_| ())
            .map_err(|e| e.to_string())
    };
    accepted(0, &|s| s.to = "owner".into()).unwrap();
    accepted(1, &|s| s.to = " nova ".into()).unwrap();
    accepted(2, &|s| s.from = " pixel ".into()).unwrap();
    accepted(3, &|s| {
        s.kind = "note".into();
        s.priority = org_proto::msg::MessagePriority::Urgent;
    })
    .unwrap();
    // At the limits: a 258-byte (as JSON) recipient, a ULID reply_to.
    accepted(4, &|s| s.to = "n".repeat(256)).unwrap();
    accepted(5, &|s| s.reply_to = Some(MSG.into())).unwrap();
    // Nothing refused left anything behind.
    let spool = home.join(OFFICES_DIR_NAME).join(A).join("spool");
    assert_eq!(fs::read_dir(spool.join("new")).unwrap().count(), 7);
    assert_eq!(fs::read_dir(spool.join("tmp")).unwrap().count(), 0);
}

#[test]
fn an_unfinished_office_folder_takes_no_spooled_send() {
    let (_dir, home) = home();
    let folder = home.join(OFFICES_DIR_NAME).join(A);
    fs::create_dir_all(&folder).unwrap();
    assert!(matches!(
        spool_send(&home, &spooled(A, MSG, "x")),
        Err(SpoolSendError::NoSuchOffice)
    ));
    assert!(!folder.join("spool").exists());
    // An office.json for another office doesn't count either.
    fs::write(folder.join(OFFICE_FILE_NAME), office_file(B)).unwrap();
    assert!(matches!(
        spool_send(&home, &spooled(A, MSG, "x")),
        Err(SpoolSendError::NoSuchOffice)
    ));
}

#[test]
fn the_office_asked_for_wins_and_must_be_a_plain_id() {
    let (_dir, home) = home();
    put(&home, A, A);
    put(&home, B, B);
    assert_eq!(find_office(&home, Some(B)).unwrap(), Some(B.to_owned()));
    // Asked for by id, it must be an office of this Mac: a typo is told.
    match find_office(&home, Some("X1")) {
        Err(FindOfficeError::NotOnThisMac(id)) => assert_eq!(id, "X1"),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        find_office(&home, Some("X1")).unwrap_err().to_string(),
        "office X1 isn't on this Mac"
    );
    // A folder without office.json (an unfinished create) isn't one either.
    fs::create_dir_all(home.join(OFFICES_DIR_NAME).join("X2")).unwrap();
    assert!(matches!(
        find_office(&home, Some("X2")),
        Err(FindOfficeError::NotOnThisMac(_))
    ));
    for bad in ["", "../x", "a/b", "a.b", &"a".repeat(65)] {
        assert!(
            matches!(
                find_office(&home, Some(bad)),
                Err(FindOfficeError::NotAPlainId)
            ),
            "{bad:?}"
        );
    }
}

#[test]
fn no_offices_folder_or_an_empty_one_is_no_office() {
    let (_dir, home) = home();
    assert_eq!(find_office(&home, None).unwrap(), None);
    fs::create_dir_all(home.join(OFFICES_DIR_NAME)).unwrap();
    assert_eq!(find_office(&home, None).unwrap(), None);
}

#[test]
fn the_only_office_is_chosen_and_two_are_ambiguous() {
    let (_dir, home) = home();
    put(&home, A, A);
    assert_eq!(find_office(&home, None).unwrap(), Some(A.to_owned()));
    put(&home, B, B);
    match find_office(&home, None) {
        Err(FindOfficeError::Ambiguous(ids)) => assert_eq!(ids, vec![A, B]),
        other => panic!("{other:?}"),
    }
    let text = find_office(&home, None).unwrap_err().to_string();
    assert!(
        text.contains("ORG_OFFICE") && text.contains("--office"),
        "{text}"
    );
}

#[test]
fn folders_that_are_not_offices_are_skipped() {
    let (_dir, home) = home();
    put(&home, A, A);
    let offices = home.join(OFFICES_DIR_NAME);
    // An unfinished create: no office.json.
    fs::create_dir_all(offices.join("UNFINISHED1")).unwrap();
    // An office.json for another office.
    put(&home, "MISPLACED1", B);
    // An invalid office.json.
    fs::create_dir_all(offices.join("GARBLED1")).unwrap();
    fs::write(offices.join("GARBLED1").join(OFFICE_FILE_NAME), b"{}").unwrap();
    // An office.json that is a symlink to a valid one.
    let elsewhere = home.join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    fs::write(elsewhere.join(OFFICE_FILE_NAME), office_file("LINKED1")).unwrap();
    fs::create_dir_all(offices.join("LINKED1")).unwrap();
    symlink(
        elsewhere.join(OFFICE_FILE_NAME),
        offices.join("LINKED1").join(OFFICE_FILE_NAME),
    )
    .unwrap();
    // A folder that is a symlink to a valid office.
    put(&home, "REAL2", "REAL2");
    let real = offices.join("REAL2");
    fs::rename(&real, home.join("REAL2")).unwrap();
    symlink(home.join("REAL2"), &real).unwrap();
    // Names that aren't plain ids, and a plain file.
    fs::create_dir_all(offices.join(".refused").join(B)).unwrap();
    fs::write(offices.join("FILE1"), b"x").unwrap();

    assert_eq!(find_office(&home, None).unwrap(), Some(A.to_owned()));
}

#[test]
fn an_office_with_nothing_listening_is_unreachable_after_a_short_retry() {
    let (_dir, home) = home();
    fs::create_dir_all(home.join("run")).unwrap();
    let started = Instant::now();
    let error = match open_as(&home, A, "pixel") {
        Err(error) => error,
        Ok(_) => panic!("connected to nothing"),
    };
    assert!(
        matches!(error, OpenError::Unreachable(ClientError::NotRunning(_))),
        "{error:?}"
    );
    let took = started.elapsed();
    assert!(
        took >= Duration::from_millis(300),
        "gave up at once: {took:?}"
    );
    assert!(took < Duration::from_secs(3), "{took:?}");
}

#[test]
fn a_server_that_accepts_but_never_answers_is_unreachable_within_the_setup_timeout() {
    let (_dir, home) = home();
    fs::create_dir_all(home.join("run")).unwrap();
    let socket = org_proto::location::office_socket_path(&home, A).unwrap();
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let (_release, held) = std::sync::mpsc::channel::<()>();
    std::thread::spawn(move || {
        let connection = listener.accept();
        let _ = held.recv();
        drop(connection);
    });
    let started = Instant::now();
    let error = match open_as(&home, A, "pixel") {
        Err(error) => error,
        Ok(_) => panic!("a silent server answered"),
    };
    assert!(
        matches!(error, OpenError::Unreachable(ClientError::Timeout)),
        "{error:?}"
    );
    assert!(started.elapsed() < SETUP_TIMEOUT + Duration::from_secs(2));
}

#[test]
fn a_plain_file_where_the_socket_should_be_is_unreachable_without_connecting() {
    let (_dir, home) = home();
    fs::create_dir_all(home.join("run")).unwrap();
    let socket = org_proto::location::office_socket_path(&home, A).unwrap();
    fs::write(&socket, b"not a socket").unwrap();
    assert!(matches!(
        open_as(&home, A, "pixel"),
        Err(OpenError::Unreachable(ClientError::OfficeNotReachable))
    ));
}

#[test]
fn an_office_not_ready_yet_is_unreachable_and_a_refusal_is_refused() {
    use org_proto::{ApiError, ErrorCode};
    for code in [ErrorCode::NotReady, ErrorCode::Unavailable] {
        assert!(matches!(
            OpenError::from_setup(ClientError::Api(ApiError::new(code, "starting"))),
            OpenError::Unreachable(_)
        ));
    }
    assert!(matches!(
        OpenError::from_setup(ClientError::Api(ApiError::new(
            ErrorCode::NotAuthorized,
            "no"
        ))),
        OpenError::Refused(_)
    ));
}

#[test]
fn a_relative_home_or_a_socket_path_too_long_is_refused_not_unreachable() {
    assert!(matches!(
        open_as(Path::new("rel"), A, "pixel"),
        Err(OpenError::Refused(ClientError::NoOfficeHome))
    ));
    let long = PathBuf::from(format!("/tmp/{}", "x".repeat(120)));
    assert!(matches!(
        open_as(&long, A, "pixel"),
        Err(OpenError::Refused(ClientError::SocketPathTooLong(_)))
    ));
}
