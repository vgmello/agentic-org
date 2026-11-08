//! The host's org-proto surface: hello, unknown methods, unreadable lines, and the
//! owner-only files it serves from.

mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use org_office_host::Host;
use org_proto::{
    Empty, ErrorCode, Hello, OfficeList, Outcome, Subscribe, SubscribeParams, API_VERSION,
    UNREADABLE_REQUEST_ID,
};
use support::{Client, TestHome};

fn error_code(outcome: &Outcome) -> Option<ErrorCode> {
    match outcome {
        Outcome::Error(e) => Some(e.code),
        Outcome::Result(_) => None,
    }
}

#[test]
fn hello_names_the_host_and_no_office() {
    let home = TestHome::new();
    let host = Host::start(home.config()).unwrap();
    let hello = Client::connect(host.socket_path())
        .call::<Hello>(&Empty {})
        .unwrap();
    assert_eq!(hello.api_version, API_VERSION);
    assert_eq!(hello.server_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(hello.host, "This Mac");
    assert_eq!(hello.office, None);
    host.shutdown();
}

#[test]
fn unknown_methods_and_unreadable_lines_get_errors_and_the_connection_goes_on() {
    let home = TestHome::new();
    let host = Host::start(home.config()).unwrap();
    let mut client = Client::connect(host.socket_path());

    let reply = client.raw(r#"{"id":5,"method":"org.teleport","params":{}}"#);
    assert_eq!(reply.id, 5);
    assert_eq!(error_code(&reply.outcome), Some(ErrorCode::UnknownMethod));

    let reply = client.raw("this isn't json");
    assert_eq!(reply.id, UNREADABLE_REQUEST_ID);
    assert_eq!(error_code(&reply.outcome), Some(ErrorCode::InvalidRequest));

    let reply = client.raw(r#"{"id":6,"method":"office.list","params":[1]}"#);
    assert_eq!(reply.id, 6);
    assert_eq!(error_code(&reply.outcome), Some(ErrorCode::InvalidRequest));

    // Subscribe's position (for offices) is accepted and ignored by the host.
    let reply = client
        .raw(r#"{"id":7,"method":"subscribe","params":{"from_position":{"epoch":1,"seq":2}}}"#);
    assert_eq!(error_code(&reply.outcome), None);
    client
        .call::<Subscribe>(&SubscribeParams::default())
        .unwrap();
    assert!(client
        .call::<OfficeList>(&Empty {})
        .unwrap()
        .offices
        .is_empty());
    host.shutdown();
}

#[test]
fn the_home_and_socket_are_owner_only() {
    let home = TestHome::new();
    fs::create_dir_all(home.path()).unwrap();
    fs::set_permissions(home.path(), fs::Permissions::from_mode(0o755)).unwrap();
    let host = Host::start(home.config()).unwrap();
    let mode = |p: &std::path::Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&home.path()), 0o700);
    for dir in ["run", "logs", "offices"] {
        assert_eq!(mode(&home.path().join(dir)), 0o700, "{dir}");
    }
    assert_eq!(mode(host.socket_path()), 0o600);
    assert_eq!(mode(&home.path().join("host.lock")), 0o600);
    host.shutdown();
}

#[test]
fn a_stale_socket_is_replaced_but_another_file_is_not() {
    // A socket file left behind by a host that was killed.
    let stale = TestHome::new();
    fs::create_dir_all(stale.path()).unwrap();
    drop(std::os::unix::net::UnixListener::bind(stale.path().join("host.sock")).unwrap());
    let host = Host::start(stale.config()).unwrap();
    Client::connect(host.socket_path())
        .call::<Hello>(&Empty {})
        .unwrap();
    host.shutdown();

    let blocked = TestHome::new();
    fs::create_dir_all(blocked.path()).unwrap();
    fs::write(blocked.path().join("host.sock"), b"mine").unwrap();
    assert!(matches!(
        Host::start(blocked.config()),
        Err(org_office_host::HostError::SocketInTheWay { .. })
    ));
    assert_eq!(fs::read(blocked.path().join("host.sock")).unwrap(), b"mine");
}

/// Shutdown with clients connected (idle, subscribed, and one with a create in flight) ends
/// promptly. It would hang if waking the accept thread or ending a connection's writer broke,
/// so a watchdog turns that into a failure instead of a stuck test run.
#[test]
fn shutdown_with_clients_connected_finishes_promptly() {
    let home = TestHome::new();
    home.script(serde_json::json!({"create": {"do": "hang"}}));
    let host = Host::start(home.config()).unwrap();
    let _idle = std::os::unix::net::UnixStream::connect(host.socket_path()).unwrap();
    let mut subscribed = Client::connect(host.socket_path());
    subscribed
        .call::<Subscribe>(&SubscribeParams::default())
        .unwrap();
    let mut creating = std::os::unix::net::UnixStream::connect(host.socket_path()).unwrap();
    creating
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    std::io::Write::write_all(
        &mut creating,
        b"{\"id\":1,\"method\":\"organization.create\",\"params\":{\"name\":\"Acme\"}}\n",
    )
    .unwrap();
    support::eventually(
        std::time::Duration::from_secs(10),
        "the create to start",
        || home.runs("create") == 1,
    );

    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        host.shutdown();
        let _ = done.send(());
    });
    finished
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("shutdown hung");
    // The create in flight got an error or its connection closed: never a success.
    let mut answer = String::new();
    let _ = std::io::BufRead::read_line(&mut std::io::BufReader::new(creating), &mut answer);
    assert!(!answer.contains("\"result\""), "{answer}");
    assert!(!home.path().join("host.sock").exists());
    // Nothing was stored. The interrupted create's folder can't be discarded while stopping,
    // so the next start discards it.
    for folder in home.office_folders() {
        assert!(!home.office_dir(&folder).join("office.json").exists());
    }
    home.script(serde_json::json!({}));
    let host = support::restart_host(&home.config());
    support::eventually(
        std::time::Duration::from_secs(10),
        "the interrupted create's folder to be discarded",
        || home.office_folders().is_empty(),
    );
    host.shutdown();
}
