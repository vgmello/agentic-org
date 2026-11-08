//! The host's org-proto surface: hello, unknown methods, unreadable lines, and the
//! owner-only files it serves from.

mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use org_office_host::Host;
use org_proto::{
    Empty, ErrorCode, Hello, OfficeInfo, OfficeList, OfficeStatus, OfficeStatusChanged,
    OrganizationCreate, OrganizationCreateParams, OrganizationCreated, OrganizationList, Outcome,
    ServerMessage, Subscribe, SubscribeParams, API_VERSION, UNREADABLE_REQUEST_ID,
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
    // Every optional call and event this host implements, once each.
    assert_eq!(
        hello.capabilities,
        [
            org_proto::capability::OFFICE_APP_TOKEN,
            org_proto::capability::OFFICE_RESTART,
            org_proto::capability::OFFICE_STATUS_CHANGED,
        ]
    );
    assert!(hello.supports(org_proto::capability::OFFICE_STATUS_CHANGED));
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

/// An event published after a list snapshot never reaches a client before that snapshot: every
/// organization a subscriber has heard of is in every list answer that comes after the event.
/// List calls stream in on one subscribed connection (handled up to 16 at a time) while
/// organizations are created on another.
#[test]
fn a_list_answer_never_arrives_after_an_event_it_doesnt_reflect() {
    use std::collections::{HashMap, HashSet};
    use std::io::{BufRead, BufReader, Write};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;

    const CREATES: usize = 40;
    let home = TestHome::new();
    let host = Host::start(home.config()).unwrap();
    let path = host.socket_path().to_path_buf();
    let stream = std::os::unix::net::UnixStream::connect(&path).unwrap();
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(30)));
    let mut out = stream.try_clone().unwrap();
    out.write_all(b"{\"id\":0,\"method\":\"subscribe\",\"params\":{}}\n")
        .unwrap();
    // Subscribed before anything is created, so every event reaches this connection.
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert!(
        matches!(serde_json::from_str(&line), Ok(ServerMessage::Response(r)) if r.id == 0),
        "{line}"
    );

    let (creating, sent) = (Arc::new(AtomicBool::new(true)), Arc::new(AtomicU64::new(0)));
    let answered = Arc::new(AtomicU64::new(0));
    let writer = {
        let (creating, sent, answered) = (
            Arc::clone(&creating),
            Arc::clone(&sent),
            Arc::clone(&answered),
        );
        std::thread::spawn(move || {
            let mut id = 0u64;
            // Lists keep coming until the creates are done, then one more. At most 64 are
            // unanswered, so the answers never fill the connection's queue (a client that lets
            // it fill is disconnected).
            loop {
                while id - answered.load(Ordering::SeqCst) >= 64 {
                    std::thread::yield_now();
                }
                let last = !creating.load(Ordering::SeqCst);
                id += 1;
                // Both lists, in turn.
                let method = if id % 2 == 1 {
                    "organization.list"
                } else {
                    "office.list"
                };
                let line = format!("{{\"id\":{id},\"method\":\"{method}\",\"params\":{{}}}}\n");
                out.write_all(line.as_bytes()).unwrap();
                sent.store(id, Ordering::SeqCst);
                if last {
                    return;
                }
            }
        })
    };
    let creator = {
        let (path, creating) = (path.clone(), Arc::clone(&creating));
        std::thread::spawn(move || {
            let mut client = Client::connect(&path);
            for i in 0..CREATES {
                client
                    .call::<OrganizationCreate>(&OrganizationCreateParams {
                        name: format!("Org {i}"),
                    })
                    .unwrap();
            }
            creating.store(false, Ordering::SeqCst);
        })
    };

    // Each office goes from `starting` to `active`; no message may show an office in an
    // earlier state than one before it on this connection (an event older than a list answer,
    // or a list answer older than an event).
    let mut furthest: HashMap<String, OfficeStatus> = HashMap::new();
    let mut see = |office: &OfficeInfo, what: &str| {
        let rank = |status: OfficeStatus| match status {
            OfficeStatus::Starting => 0,
            OfficeStatus::Active => 1,
            other => panic!("{} became {other:?}", office.id),
        };
        let seen = furthest.entry(office.id.clone()).or_insert(office.status);
        assert!(
            rank(office.status) >= rank(*seen),
            "{what} showed {} {:?} after {:?}",
            office.id,
            office.status,
            seen
        );
        *seen = office.status;
    };
    let (mut heard, mut answers) = (HashSet::new(), 0u64);
    loop {
        let done = writer.is_finished() && answers == sent.load(Ordering::SeqCst);
        if done && heard.len() == CREATES {
            break;
        }
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0, "the host hung up");
        match serde_json::from_str::<ServerMessage>(&line).unwrap() {
            ServerMessage::Event(event) => match event.event.as_str() {
                "organization.created" => {
                    let org = event.data::<OrganizationCreated>().unwrap().unwrap();
                    for office in &org.offices {
                        see(office, "organization.created");
                    }
                    heard.insert(org.id);
                }
                "office.status_changed" => {
                    let office = event.data::<OfficeStatusChanged>().unwrap().unwrap().office;
                    // Never before its organization was announced.
                    assert!(
                        heard.contains(&office.organization_id),
                        "office.status_changed for {} came before its organization.created",
                        office.id
                    );
                    see(&office, "office.status_changed");
                }
                other => panic!("unexpected event {other}"),
            },
            ServerMessage::Response(response) => {
                // The organizations each list answer shows.
                let listed: HashSet<String> = if response.id % 2 == 1 {
                    let orgs = response
                        .into_result::<OrganizationList>()
                        .unwrap()
                        .organizations;
                    for office in orgs.iter().flat_map(|o| &o.offices) {
                        see(office, "organization.list");
                    }
                    orgs.into_iter().map(|o| o.id).collect()
                } else {
                    let offices = response.into_result::<OfficeList>().unwrap().offices;
                    for office in &offices {
                        see(office, "office.list");
                    }
                    offices.into_iter().map(|o| o.organization_id).collect()
                };
                let missing: Vec<_> = heard.difference(&listed).collect();
                assert!(
                    missing.is_empty(),
                    "a list answer came after the event for {missing:?} without it"
                );
                answers += 1;
                answered.store(answers, Ordering::SeqCst);
            }
            ServerMessage::Unknown(v) => panic!("unexpected {v}"),
        }
    }
    writer.join().unwrap();
    creator.join().unwrap();
    host.shutdown();
}

/// A subscriber that creates organizations on its own connection gets each answer before any
/// event about that organization: the answer, `organization.created` and the office's status
/// changes go out in that order.
#[test]
fn a_subscribed_creator_gets_its_answer_before_any_event_about_it() {
    use std::collections::HashSet;
    use std::io::{BufRead, BufReader, Write};

    let home = TestHome::new();
    let host = Host::start(home.config()).unwrap();
    let stream = std::os::unix::net::UnixStream::connect(host.socket_path()).unwrap();
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(30)));
    let mut out = stream.try_clone().unwrap();
    let mut reader = BufReader::new(stream);
    let mut next = || {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0, "the host hung up");
        serde_json::from_str::<ServerMessage>(&line).unwrap()
    };
    out.write_all(b"{\"id\":0,\"method\":\"subscribe\",\"params\":{}}\n")
        .unwrap();
    assert!(matches!(next(), ServerMessage::Response(r) if r.id == 0));

    let mut answered: HashSet<String> = HashSet::new();
    let mut announced: HashSet<String> = HashSet::new();
    for id in 1..=15u64 {
        let line = format!(
            "{{\"id\":{id},\"method\":\"organization.create\",\"params\":{{\"name\":\"Org {id}\"}}}}\n"
        );
        out.write_all(line.as_bytes()).unwrap();
        // Read until this create's answer; every event before it must be about organizations
        // already answered (and, for a status change, already announced).
        loop {
            match next() {
                ServerMessage::Response(response) => {
                    assert_eq!(response.id, id);
                    let org = response.into_result::<OrganizationCreate>().unwrap();
                    answered.insert(org.id);
                    break;
                }
                ServerMessage::Event(event) => match event.event.as_str() {
                    "organization.created" => {
                        let org = event.data::<OrganizationCreated>().unwrap().unwrap();
                        assert!(answered.contains(&org.id), "announced before its answer");
                        announced.insert(org.id);
                    }
                    "office.status_changed" => {
                        let office = event.data::<OfficeStatusChanged>().unwrap().unwrap().office;
                        assert!(
                            answered.contains(&office.organization_id)
                                && announced.contains(&office.organization_id),
                            "a status change came before its organization's answer or event"
                        );
                    }
                    other => panic!("unexpected event {other}"),
                },
                ServerMessage::Unknown(v) => panic!("unexpected {v}"),
            }
        }
    }
    host.shutdown();
}
