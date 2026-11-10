//! The key hand-off (launch version 3) against the fake office server: every create, start and
//! discard gets its secrets on a one-shot key socket, never on stdin; only the server the host
//! started may take them; and the socket never outlives its spawn.
//!
//! The tests never see a secret: the fake records fingerprints, and what its other connections
//! got only as byte counts.

// Every host here keeps its offices' keys in a development key folder, which only debug builds
// have, and the hand-off's debug rule (the peer's pid) is the one these unsigned servers meet.
#![cfg(debug_assertions)]

mod support;

use std::fs;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::time::Duration;

use org_office_host::Host;
use org_proto::{
    ApiError, ErrorCode, OfficeStatus, OfficeStatusReason, OrganizationCreate,
    OrganizationCreateParams,
};
use serde_json::{json, Value};
use support::{alive, eventually, has_hex_run, hq, office_info, Client, TestHome};

const WAIT: Duration = Duration::from_secs(10);
const A: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";
const U: &str = "01UNFINISHEDUNFINISHEDUNFI";
const T: &str = "2026-11-08T00:00:00.000Z";

fn create(socket: &Path, name: &str) -> Result<org_proto::Organization, ApiError> {
    Client::connect(socket)
        .call::<OrganizationCreate>(&OrganizationCreateParams { name: name.into() })
}

fn wait_for_reason(
    socket: &Path,
    office_id: &str,
    status: OfficeStatus,
    reason: OfficeStatusReason,
) {
    eventually(
        WAIT,
        &format!("{office_id} to be {status:?}/{reason:?}"),
        || {
            office_info(socket, office_id)
                .is_some_and(|o| o.status == status && o.status_reason == Some(reason))
        },
    );
}

/// Asserts each first line names a key socket and carries no secret: no `app_token` or `db_key`
/// field, and nothing that looks like one (64 hex digits in a row).
fn assert_secret_free(lines: &[String], op: &str) {
    assert!(!lines.is_empty(), "no first line was recorded for {op}");
    for line in lines {
        assert!(
            !has_hex_run(line, 64),
            "a first line holds a secret: {line}"
        );
        let message: Value = serde_json::from_str(line).unwrap();
        assert_eq!(message["v"], 3, "{line}");
        assert_eq!(message["op"], op, "{line}");
        let id = message["key_socket_id"].as_str().unwrap();
        assert!(
            id.len() == 16
                && id
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
            "{line}"
        );
        assert!(
            message.get("app_token").is_none() && message.get("db_key").is_none(),
            "{line}"
        );
    }
}

/// The happy path, for a create, a start and a discard: the key arrives on the key socket (the
/// stored key's fingerprint), the socket was a 0600 socket of ours while it existed and is gone
/// afterwards, and stdin carried no secret.
#[test]
fn every_create_start_and_discard_gets_its_key_on_the_socket_and_none_on_stdin() {
    let home = TestHome::new();
    home.write_office(&hq(A, "01ORGA", "Acme", T));
    fs::create_dir_all(home.office_dir(U)).unwrap();
    home.write_key(U);
    let (stored_a, stored_u) = (
        home.stored_key_fingerprint(A).unwrap(),
        home.stored_key_fingerprint(U).unwrap(),
    );
    let host = Host::start(home.config()).unwrap();
    eventually(WAIT, "the startup discard", || home.key_ids() == [A]);
    let org = create(host.socket_path(), "Beta").unwrap();
    let created = org.offices[0].id.clone();
    for id in [A, created.as_str()] {
        eventually(WAIT, "the offices to be active", || {
            office_info(host.socket_path(), id).is_some_and(|o| o.status == OfficeStatus::Active)
        });
    }

    // The key each got is the stored one, and each got the office's app token.
    assert_eq!(home.db_key_fingerprints(A), [stored_a]);
    assert_eq!(home.discard_keys(), [(U.to_owned(), stored_u)]);
    assert_eq!(
        home.db_key_fingerprints("create"),
        [home.stored_key_fingerprint(&created).unwrap()]
    );
    assert_eq!(home.token_fingerprints(A).len(), 1);
    assert_eq!(home.token_fingerprints("create").len(), 1);
    // Stdin: only an id.
    assert_secret_free(&home.first_lines(A), "start");
    assert_secret_free(&home.first_lines("create"), "create");
    assert_secret_free(&home.first_lines("discard"), "discard");
    // Each socket was a socket (0o14), owner-only, of ours; none is left.
    for key in [A, "create", "discard"] {
        assert_eq!(
            home.key_socket_modes(key),
            [("140600".to_owned(), "ours".to_owned())],
            "{key}"
        );
    }
    assert_eq!(home.key_sockets(), Vec::<String>::new());
    host.shutdown();
    home.assert_no_secret_in_home();
}

/// A connection from a process that isn't the child (the fake's helper, another pid) gets
/// nothing: the child is killed, the socket unlinked, and the start is `storage_error`, retried.
/// A create fails with `storage_error` and its key goes only with its discard; a discard keeps
/// the folder and the key.
#[test]
fn a_peer_that_isnt_the_server_gets_nothing() {
    let home = TestHome::new();
    home.write_office(&hq(A, "01ORGA", "Acme", T));
    fs::create_dir_all(home.office_dir(U)).unwrap();
    home.write_key(U);
    let helper = json!({"hand_off": "helper"});
    home.script(json!({ A: helper, "discard": [helper, {}], "create": helper }));
    let mut config = home.config();
    // Long enough to see it offline.
    config.timings.backoff_initial = Duration::from_secs(30);
    let host = Host::start(config).unwrap();

    // The start.
    wait_for_reason(
        host.socket_path(),
        A,
        OfficeStatus::Offline,
        OfficeStatusReason::StorageError,
    );
    let pid = home.pids(A)[0];
    eventually(WAIT, "the refused server to be killed", || !alive(pid));
    assert_eq!(
        home.other_connections(A),
        [("helper".to_owned(), "0".to_owned())],
        "the helper got something"
    );
    assert!(home.token_fingerprints(A).is_empty());
    assert!(home.db_key_fingerprints(A).is_empty());

    // The startup discard: refused, so the folder and its key stay.
    eventually(WAIT, "the discard's helper", || {
        home.other_connections("discard").len() == 1
    });
    assert_eq!(home.other_connections("discard")[0].1, "0");
    assert!(home.discard_keys().is_empty());
    assert!(home.has_key(U) && home.office_dir(U).exists());

    // The create: refused; its key goes with its (successful) discard.
    let err = create(host.socket_path(), "Beta").unwrap_err();
    assert_eq!(err.code, ErrorCode::StorageError);
    assert_eq!(
        home.other_connections("create"),
        [("helper".to_owned(), "0".to_owned())]
    );
    assert!(home.db_key_fingerprints("create").is_empty());
    eventually(WAIT, "the create's key to go", || {
        home.key_ids() == [A.to_owned(), U.to_owned()]
    });
    assert_eq!(home.key_sockets(), Vec::<String>::new());
    host.shutdown();
    home.assert_no_secret_in_home();
}

/// A second connection gets nothing; the first gets the key once, and the office runs.
#[test]
fn a_second_connection_gets_nothing() {
    let home = TestHome::new();
    home.write_office(&hq(A, "01ORGA", "Acme", T));
    home.script(json!({ A: {"hand_off": "twice"} }));
    let host = Host::start(home.config()).unwrap();
    eventually(WAIT, "the office to be active", || {
        office_info(host.socket_path(), A).is_some_and(|o| o.status == OfficeStatus::Active)
    });
    assert_eq!(
        home.db_key_fingerprints(A),
        [home.stored_key_fingerprint(A).unwrap()]
    );
    let others = home.other_connections(A);
    assert_eq!(others.len(), 1, "{others:?}");
    assert_eq!(others[0].0, "second");
    // Closed with nothing sent, or (had the host already unlinked) not connected at all.
    assert!(
        others[0].1 == "0" || others[0].1 == "connect-failed",
        "the second connection got {}",
        others[0].1
    );
    assert_eq!(home.key_sockets(), Vec::<String>::new());
    host.shutdown();
    home.assert_no_secret_in_home();
}

/// No connection within the hand-off wait: the child is killed, the socket unlinked, and the
/// start is `storage_error`, retried with backoff.
#[test]
fn no_connection_in_time_is_a_storage_error() {
    let home = TestHome::new();
    home.write_office(&hq(A, "01ORGA", "Acme", T));
    home.script(json!({ A: {"hand_off": "none"} }));
    let mut config = home.config();
    // Short, but long enough for the fake's first (cold) launch to reach its hand-off.
    config.timings.hand_off_timeout = Duration::from_secs(3);
    config.timings.backoff_initial = Duration::from_secs(30);
    let host = Host::start(config).unwrap();
    wait_for_reason(
        host.socket_path(),
        A,
        OfficeStatus::Offline,
        OfficeStatusReason::StorageError,
    );
    let pid = home.pids(A)[0];
    eventually(WAIT, "the server to be killed", || !alive(pid));
    assert_eq!(home.key_socket_modes(A).len(), 1);
    assert!(home.token_fingerprints(A).is_empty());
    assert!(home.eof_exits(A).is_empty(), "it wasn't killed");
    assert_eq!(home.key_sockets(), Vec::<String>::new());
    // The key stays.
    assert!(home.has_key(A));
    host.shutdown();
    home.assert_no_secret_in_home();
}

/// A host that died mid-spawn left key sockets behind: the next one removes them at startup, and
/// only them.
#[test]
fn stale_key_sockets_are_removed_at_startup() {
    let home = TestHome::new();
    let run = home.path().join("run");
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&run)
        .unwrap();
    fs::set_permissions(home.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let stale = run.join("k-0123456789abcdef.sock");
    drop(UnixListener::bind(&stale).unwrap()); // leaves its file behind
    let not_a_socket = run.join("k-fedcba9876543210.sock");
    fs::write(&not_a_socket, b"").unwrap();
    let host = Host::start(home.config()).unwrap();
    assert!(!stale.exists(), "a stale key socket survived startup");
    assert!(
        not_a_socket.exists(),
        "a file that isn't a socket was removed"
    );
    host.shutdown();
}

/// The fd hygiene closes what the server inherited by listing its open fds, not by counting up to
/// its fd limit: a parent that lowers `RLIMIT_NOFILE` below an fd it leaves open can't hide it.
/// It also resets the task's exception ports, `EXC_CRASH` included.
#[test]
fn fd_hygiene_closes_an_inherited_fd_above_a_lowered_limit() {
    let home = TestHome::new();
    home.write_office(&hq(A, "01ORGA", "Acme", T));
    let mut config = home.config();
    config.set_server_program(home.wrapper(
        "low-limit",
        r#"exec 20</dev/null; ulimit -n 16; exec "$FAKE" "$@""#,
    ));
    let host = Host::start(config).unwrap();
    eventually(WAIT, "the office to be active", || {
        office_info(host.socket_path(), A).is_some_and(|o| o.status == OfficeStatus::Active)
    });
    assert_eq!(
        home.inherited_fds(A),
        ["20"],
        "the test's fd wasn't inherited"
    );
    assert!(home.fd_hygiene_refusals().is_empty());
    let notes = home.fd_hygiene_notes();
    assert!(
        notes.contains(&"exception-ports-reset".to_owned()),
        "{notes:?}"
    );
    assert!(
        notes.contains(&"corpse-notify-reset".to_owned())
            || notes.contains(&"corpse-notify-refused-by-the-kernel".to_owned()),
        "{notes:?}"
    );
    host.shutdown();
}

/// A socket in place of fd 1 or fd 2 (e.g. one a helper of an early resumer holds, connected to
/// the key socket): the server must not touch it at all, not even with a write that fails, since
/// any write, read or connect would make it the socket's last user, and the host would take that
/// connection for the admitted server's. The probe gives the fake one end of a socket pair there
/// and checks afterwards that the fake never used it: its last user is still the probe, and
/// nothing was sent. The server writes no `failed` line at all and exits with
/// `EXIT_LAUNCH_DESCRIPTORS_REFUSED`, which the host reports as a refusal of the launch
/// descriptors (`storage_error`), not a crash. Nobody gets a hand-off.
#[test]
fn a_server_never_touches_a_socket_in_place_of_its_stdout_or_stderr() {
    for fd in ["1", "2"] {
        let home = TestHome::new();
        home.write_office(&hq(A, "01ORGA", "Acme", T));
        let mut config = home.config();
        config.set_server_program(home.wrapper(
            &format!("probe-{fd}"),
            &format!(r#"exec "$FAKE" --stdio-probe {fd} "$@""#),
        ));
        config.timings.backoff_initial = Duration::from_secs(30);
        let host = Host::start(config).unwrap();
        wait_for_reason(
            host.socket_path(),
            A,
            OfficeStatus::Offline,
            OfficeStatusReason::StorageError,
        );
        eventually(WAIT, "the probe's record", || {
            !home.stdio_probes().is_empty()
        });
        let (probed, probe, last, sent, exit) = home.stdio_probes()[0].clone();
        assert_eq!(probed, fd);
        assert_eq!(last, probe, "fd {fd}: the server used the socket");
        assert_eq!(sent, "0", "fd {fd}: the server wrote to the socket");
        assert_eq!(
            exit,
            org_office_launch::EXIT_LAUNCH_DESCRIPTORS_REFUSED.to_string(),
            "fd {fd}: the contract's exit status"
        );
        assert_eq!(home.fd_hygiene_refusals().len(), 1, "fd {fd}");
        assert!(home.token_fingerprints(A).is_empty(), "fd {fd}");
        assert!(home.key_socket_modes(A).is_empty(), "fd {fd}");
        assert_eq!(home.key_sockets(), Vec::<String>::new());
        host.shutdown();
    }
}

/// A server that holds its key connection the way the contract forbids gets nothing, though it
/// is the admitted server (the peer rule passes): the host's peer-fd check refuses a connection
/// another process also holds (`fork`: shared), one held on two fds (`dup`), and one on fd 0-2
/// (`on_stderr`). The server is killed and the start is `storage_error`.
#[test]
fn a_server_holding_its_key_connection_badly_gets_nothing() {
    for how in ["fork", "dup", "on_stderr"] {
        let home = TestHome::new();
        home.write_office(&hq(A, "01ORGA", "Acme", T));
        home.script(json!({ A: {"hand_off": how} }));
        let mut config = home.config();
        config.timings.backoff_initial = Duration::from_secs(30);
        let host = Host::start(config).unwrap();
        wait_for_reason(
            host.socket_path(),
            A,
            OfficeStatus::Offline,
            OfficeStatusReason::StorageError,
        );
        let pid = home.pids(A)[0];
        eventually(WAIT, "the refused server to be killed", || !alive(pid));
        assert!(
            home.token_fingerprints(A).is_empty(),
            "{how}: it got a token"
        );
        assert!(
            home.db_key_fingerprints(A).is_empty(),
            "{how}: it got a key"
        );
        // It did connect, and its peer check found the host: only the host's check refused.
        assert_eq!(
            home.log_count(&format!("hand-off-peer-refused {A} ")),
            0,
            "{how}"
        );
        assert_eq!(home.key_sockets(), Vec::<String>::new());
        host.shutdown();
    }
}

/// A same-user process replaces the key socket with its own (0600, so `lstat` passes) and serves
/// a forged hand-off: the server checks who sent the line before using it (debug rule: its parent,
/// the host), refuses it, and never uses the forged secrets.
#[test]
fn the_server_refuses_a_hand_off_from_an_impostor() {
    let home = TestHome::new();
    home.write_office(&hq(A, "01ORGA", "Acme", T));
    home.script(json!({ A: {"hand_off": "impostor"} }));
    let host = Host::start(home.config()).unwrap();
    wait_for_reason(
        host.socket_path(),
        A,
        OfficeStatus::Unavailable,
        OfficeStatusReason::InvalidRequest,
    );
    eventually(WAIT, "the impostor to accept the fake's connection", || {
        home.log_count("impostor-accepted") == 1
    });
    // Refused before a byte was read: the impostor's whole line was still waiting, unread.
    let mut line = Vec::new();
    org_office_launch::write_hand_off(
        &mut line,
        &org_proto::AppToken::from_bytes([0x42; 32]),
        Some(&org_proto::DbKey::from_bytes([0x42; 32])),
    )
    .unwrap();
    assert_eq!(
        home.log_count(&format!("hand-off-peer-refused {A} ")),
        1,
        "the fake didn't refuse the impostor"
    );
    let refused = home.log_lines("hand-off-peer-refused");
    assert!(
        refused[0].ends_with(&format!(" {}", line.len())),
        "the fake read some of the forged line: {refused:?}"
    );
    assert!(
        home.token_fingerprints(A).is_empty(),
        "the forged secrets were used"
    );
    assert!(home.db_key_fingerprints(A).is_empty());
    host.shutdown();
}

/// A server that is slow to check who its peer is (descheduled between its connect and its
/// check) still finds the host there: the host keeps its end open until the server closes its
/// own, so the check never meets a closed peer.
#[test]
fn a_server_slow_to_check_the_host_still_gets_its_key() {
    let home = TestHome::new();
    home.write_office(&hq(A, "01ORGA", "Acme", T));
    home.script(json!({ A: {"peer_check_delay_ms": 300} }));
    let host = Host::start(home.config()).unwrap();
    eventually(WAIT, "the office to be active", || {
        office_info(host.socket_path(), A).is_some_and(|o| o.status == OfficeStatus::Active)
    });
    assert_eq!(
        home.db_key_fingerprints(A),
        [home.stored_key_fingerprint(A).unwrap()]
    );
    assert_eq!(home.log_count("hand-off-peer-refused"), 0);
    host.shutdown();
}
