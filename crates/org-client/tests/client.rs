//! `org-client` against a fake server written directly on `org_proto::frame`, so each side
//! of the protocol is tested without the other. (Tests against the real server live in
//! `apps/org-server`, which may depend on this crate; the reverse may not.)

use std::io::{BufReader, ErrorKind};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use org_client::{Client, ClientError};
use org_proto::frame;
use org_proto::msg::{MsgBind, MsgBindResult};
use org_proto::{
    ApiError, AppToken, Call, Empty, ErrorCode, Event, Hello, HelloResult, ListOfficesResult,
    ListOrganizationsResult, OfficeInfo, OfficeList, OfficeRole, OfficeStatus, Organization,
    OrganizationCreate, OrganizationCreateParams, OrganizationCreated, OrganizationList, Outcome,
    Position, Request, Response, API_VERSION, APP_TOKEN_BYTES, MAX_FRAME_BYTES,
};
use serde_json::{json, Value};
use tempfile::TempDir;

/// One accepted connection, seen from the server's side.
struct Peer {
    reader: BufReader<UnixStream>,
    stream: UnixStream,
}

impl Peer {
    fn next_request(&mut self) -> Option<Request> {
        frame::read_message(&mut self.reader).unwrap()
    }

    fn reply<C: Call>(&mut self, id: u64, result: &C::Result) {
        frame::write_message(&mut self.stream, &Response::ok::<C>(id, result).unwrap()).unwrap();
    }

    fn fail(&mut self, id: u64, code: ErrorCode) {
        let response = Response::error(id, ApiError::new(code, "no"));
        frame::write_message(&mut self.stream, &response).unwrap();
    }

    fn raw(&mut self, value: Value) {
        frame::write_message(&mut self.stream, &value).unwrap();
    }
}

struct Fake {
    _dir: TempDir,
    socket: PathBuf,
    server: Option<JoinHandle<()>>,
}

/// Starts a server that hands its first connection to `script`.
fn fake(script: impl FnOnce(Peer) + Send + 'static) -> Fake {
    let dir = TempDir::new().unwrap();
    let socket = dir.path().join("s.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        script(Peer {
            reader: BufReader::new(stream.try_clone().unwrap()),
            stream,
        });
    });
    Fake {
        _dir: dir,
        socket,
        server: Some(server),
    }
}

impl Fake {
    fn client(&self) -> Client {
        Client::connect(&self.socket).unwrap()
    }

    fn finish(mut self) {
        self.server.take().unwrap().join().unwrap();
    }
}

fn hello_result(api_version: u32) -> HelloResult {
    let mut hello = HelloResult::host("9.9.9", "This Mac");
    hello.api_version = api_version;
    hello
}

fn office(id: &str, endpoint: Option<&str>) -> OfficeInfo {
    OfficeInfo {
        id: id.into(),
        organization_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        name: "HQ".into(),
        role: OfficeRole::Hq,
        host: "This Mac".into(),
        endpoint: endpoint.map(str::to_owned),
        status: OfficeStatus::Active,
        status_reason: None,
        next_restart_at: None,
    }
}

fn organization(name: &str) -> Organization {
    Organization {
        id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        name: name.into(),
        created_at: "2026-11-08T02:10:00.000Z".into(),
        offices: vec![office("01ARZ3NDEKTSV4RRFFQ69G5FAW", None)],
    }
}

/// Held by the tests that spawn many threads or move a lot of data, and by the tests that
/// depend on tight timing or sleeps, so the heavy ones don't starve the timing-sensitive ones.
fn exclusive() -> std::sync::MutexGuard<'static, ()> {
    static HEAVY: std::sync::Mutex<()> = std::sync::Mutex::new(());
    HEAVY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Runs a test body on its own thread and fails the test if it has not finished in `limit`,
/// so a regression that hangs the client fails instead of hanging `cargo test`.
fn watchdog<T: Send + 'static>(limit: Duration, body: impl FnOnce() -> T + Send + 'static) -> T {
    let (done, finished) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let result = body();
        let _ = done.send(());
        result
    });
    match finished.recv_timeout(limit) {
        Ok(()) => handle.join().expect("the test body finished"),
        // The body ended without reporting: it panicked. Show its own panic, not ours.
        Err(mpsc::RecvTimeoutError::Disconnected) => match handle.join() {
            Err(panic) => std::panic::resume_unwind(panic),
            Ok(_) => unreachable!("a finished body reports before it returns"),
        },
        Err(mpsc::RecvTimeoutError::Timeout) => {
            panic!("the test body did not finish within {limit:?}")
        }
    }
}

#[test]
fn hello_returns_the_servers_answer_and_ids_start_at_one() {
    let server = fake(|mut peer| {
        let request = peer.next_request().unwrap();
        assert_eq!((request.id, request.method.as_str()), (1, "hello"));
        peer.reply::<Hello>(request.id, &hello_result(API_VERSION));
        let second = peer.next_request().unwrap();
        assert_eq!(second.id, 2);
        peer.reply::<Hello>(second.id, &hello_result(API_VERSION));
    });
    let client = server.client();

    assert_eq!(client.hello().unwrap().server_version, "9.9.9");
    client.hello().unwrap();
    drop(client);
    server.finish();
}

#[test]
fn a_server_of_another_protocol_version_is_refused() {
    let server = fake(|mut peer| {
        let request = peer.next_request().unwrap();
        peer.reply::<Hello>(request.id, &hello_result(API_VERSION + 1));
    });
    let client = server.client();

    match client.hello() {
        Err(ClientError::VersionMismatch {
            client: c,
            server: s,
        }) => {
            assert_eq!((c, s), (API_VERSION, API_VERSION + 1));
        }
        other => panic!("expected a version mismatch, got {other:?}"),
    }
    drop(client);
    server.finish();
}

#[test]
fn typed_params_and_results_round_trip() {
    let server = fake(|mut peer| {
        let request = peer.next_request().unwrap();
        let params = request.params::<OrganizationCreate>().unwrap();
        assert_eq!(params.name, "Acme");
        peer.reply::<OrganizationCreate>(request.id, &organization(&params.name));
    });
    let client = server.client();

    let created = client
        .call::<OrganizationCreate>(&OrganizationCreateParams {
            name: "Acme".into(),
        })
        .unwrap();

    assert_eq!(created, organization("Acme"));
    drop(client);
    server.finish();
}

#[test]
fn an_error_response_surfaces_with_its_code() {
    let server = fake(|mut peer| {
        let request = peer.next_request().unwrap();
        peer.fail(request.id, ErrorCode::AlreadyExists);
    });
    let client = server.client();

    match client.call::<OrganizationList>(&Empty {}) {
        Err(ClientError::Api(error)) => assert_eq!(error.code, ErrorCode::AlreadyExists),
        other => panic!("expected an API error, got {other:?}"),
    }
    drop(client);
    server.finish();
}

#[test]
fn answers_arriving_out_of_order_reach_the_right_callers() {
    let server = fake(|mut peer| {
        let requests: Vec<Request> = (0..3).map(|_| peer.next_request().unwrap()).collect();
        // Answer last-first.
        for request in requests.iter().rev() {
            let params = request.params::<OrganizationCreate>().unwrap();
            peer.reply::<OrganizationCreate>(request.id, &organization(&params.name));
        }
    });
    let client = std::sync::Arc::new(server.client());

    let callers: Vec<_> = ["one", "two", "three"]
        .into_iter()
        .map(|name| {
            let client = std::sync::Arc::clone(&client);
            std::thread::spawn(move || {
                let created = client
                    .call::<OrganizationCreate>(&OrganizationCreateParams { name: name.into() })
                    .unwrap();
                assert_eq!(created.name, name);
            })
        })
        .collect();
    for caller in callers {
        caller.join().unwrap();
    }
    drop(client);
    server.finish();
}

#[test]
fn a_call_that_gets_no_answer_times_out_and_a_late_answer_is_ignored() {
    let (late, send_late) = mpsc::channel::<()>();
    let server = fake(move |mut peer| {
        let silent = peer.next_request().unwrap();
        let next = peer.next_request().unwrap();
        // Answer the second call, then the first one, long after it gave up.
        peer.reply::<Hello>(next.id, &hello_result(API_VERSION));
        send_late.recv().unwrap();
        peer.reply::<Hello>(silent.id, &hello_result(API_VERSION));
        // Then a fresh call, whose answer must not be confused with the late one.
        let third = peer.next_request().unwrap();
        let mut answer = hello_result(API_VERSION);
        answer.server_version = "third".into();
        peer.reply::<Hello>(third.id, &answer);
    });
    let client = server.client();

    let started = Instant::now();
    let result = client.call_with_timeout::<Hello>(&Empty {}, Duration::from_millis(200));
    assert!(matches!(result, Err(ClientError::Timeout)));
    assert!(started.elapsed() < Duration::from_secs(5));

    // The connection is still good, and the late answer doesn't confuse it.
    assert_eq!(client.hello().unwrap().server_version, "9.9.9");
    late.send(()).unwrap();
    assert_eq!(client.hello().unwrap().server_version, "third");
    drop(client);
    server.finish();
}

#[test]
fn the_server_hanging_up_fails_the_waiting_call_at_once() {
    let server = fake(|mut peer| {
        peer.next_request().unwrap();
        // Drop the connection without answering.
    });
    let client = server.client();

    let started = Instant::now();
    let result = client.call::<Hello>(&Empty {});

    assert!(
        matches!(result, Err(ClientError::Disconnected)),
        "{result:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "waited out the timeout"
    );
    // And a call on the dead connection fails at once too, not after the timeout.
    let started = Instant::now();
    assert!(client.call::<Hello>(&Empty {}).is_err());
    assert!(started.elapsed() < Duration::from_secs(5));
    drop(client);
    server.finish();
}

#[test]
fn a_call_on_a_half_closed_connection_fails_at_once_instead_of_timing_out() {
    let (finished, wait_for_end) = mpsc::channel::<()>();
    let server = fake(move |peer| {
        // Stop sending but keep reading: the client's writes still succeed, nothing answers.
        peer.stream.shutdown(std::net::Shutdown::Write).unwrap();
        wait_for_end.recv().unwrap();
    });
    let client = server.client();
    std::thread::sleep(Duration::from_millis(300)); // let the client's reader see the end

    let started = Instant::now();
    let result = client.call_with_timeout::<Hello>(&Empty {}, Duration::from_secs(5));

    assert!(
        matches!(result, Err(ClientError::Disconnected)),
        "{result:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "waited out the timeout"
    );
    finished.send(()).unwrap();
    drop(client);
    server.finish();
}

#[test]
fn no_server_means_not_running() {
    let dir = TempDir::new().unwrap();
    let missing = Client::connect(dir.path().join("nothing.sock"))
        .err()
        .unwrap();
    assert!(matches!(missing, ClientError::NotRunning(_)));

    // A socket file nobody listens on (a server that died).
    let stale = dir.path().join("stale.sock");
    drop(UnixListener::bind(&stale).unwrap());
    let refused = Client::connect(&stale).err().unwrap();
    assert!(matches!(refused, ClientError::NotRunning(_)));
}

#[test]
fn an_oversized_request_is_refused_without_breaking_the_connection() {
    let server = fake(|mut peer| {
        let request = peer.next_request().unwrap();
        assert_eq!(request.method, "hello");
        peer.reply::<Hello>(request.id, &hello_result(API_VERSION));
    });
    let client = server.client();

    let huge = OrganizationCreateParams {
        name: "x".repeat(MAX_FRAME_BYTES + 1),
    };
    let result = client.call::<OrganizationCreate>(&huge);
    assert!(
        matches!(result, Err(ClientError::RequestTooLarge)),
        "{result:?}"
    );

    client.hello().unwrap();
    drop(client);
    server.finish();
}

#[test]
fn events_arrive_even_before_the_subscribe_answer_and_unknown_messages_are_ignored() {
    let server = fake(|mut peer| {
        let request = peer.next_request().unwrap();
        assert_eq!(request.method, "subscribe");
        // An event first, then a message shape this client has never seen, then the answer.
        let event = Event::new::<OrganizationCreated>(1, &organization("Early")).unwrap();
        frame::write_message(&mut peer.stream, &event).unwrap();
        peer.raw(json!({"from": "a newer server", "x": 1}));
        peer.reply::<org_proto::Subscribe>(request.id, &Empty {});
        let event = Event::new::<OrganizationCreated>(2, &organization("Later")).unwrap();
        frame::write_message(&mut peer.stream, &event).unwrap();
    });
    let client = server.client();

    let events = client.subscribe().unwrap();

    let names: Vec<String> = (0..2)
        .map(|_| {
            let event = events.recv_timeout(Duration::from_secs(5)).unwrap();
            event.data::<OrganizationCreated>().unwrap().unwrap().name
        })
        .collect();
    assert_eq!(names, ["Early", "Later"]);
    server.finish();
    // The server is gone: the event channel ends instead of hanging.
    assert!(matches!(
        events.recv_timeout(Duration::from_secs(5)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
    drop(client);
}

#[test]
fn a_garbled_answer_fails_only_its_own_call() {
    let server = fake(|mut peer| {
        let first = peer.next_request().unwrap();
        // Known shape, impossible content: an `error` that is a string, which serde reports
        // by quoting it.
        peer.raw(json!({"id": first.id, "error": "SECRETVALUE"}));
        let second = peer.next_request().unwrap();
        peer.reply::<Hello>(second.id, &hello_result(API_VERSION));
    });
    let client = server.client();

    let garbled = client.call::<Hello>(&Empty {});
    assert!(
        matches!(garbled, Err(ClientError::Protocol(_))),
        "{garbled:?}"
    );
    // Whatever the server sent stays out of the error.
    assert!(
        !format!("{garbled:?}").contains("SECRETVALUE"),
        "{garbled:?}"
    );
    client.hello().unwrap();
    drop(client);
    server.finish();
}

#[test]
fn a_result_of_the_wrong_shape_is_a_protocol_error() {
    let server = fake(|mut peer| {
        let request = peer.next_request().unwrap();
        let response = Response {
            id: request.id,
            outcome: Outcome::Result(json!({"api_version": "not a number"})),
        };
        frame::write_message(&mut peer.stream, &response).unwrap();
    });
    let client = server.client();

    let result = client.call::<Hello>(&Empty {});

    assert!(
        matches!(result, Err(ClientError::Protocol(_))),
        "{result:?}"
    );
    // The error says what went wrong, never what the server sent.
    let shown = result.unwrap_err().to_string();
    assert!(!shown.contains("not a number"), "{shown}");
    assert!(shown.contains("hello"), "it names the call: {shown}");
    drop(client);
    server.finish();
}

#[test]
fn bind_app_sends_the_token_and_no_member_and_a_refusal_is_the_servers_code() {
    let server = fake(|mut peer| {
        let bind = peer.next_request().unwrap();
        assert_eq!(bind.method, "msg.bind");
        assert!(bind.params.get("member").is_none(), "{}", bind.params);
        assert_eq!(bind.params["app_token"], "03".repeat(32));
        peer.reply::<MsgBind>(
            bind.id,
            &MsgBindResult {
                member_id: None,
                office_id: "OFFICE1".into(),
            },
        );
        let wrong = peer.next_request().unwrap();
        assert_eq!(wrong.params["app_token"], "04".repeat(32));
        peer.fail(wrong.id, ErrorCode::NotAuthorized);
    });
    let client = server.client();

    let bound = client
        .bind_app(AppToken::from_bytes([3; APP_TOKEN_BYTES]))
        .unwrap();
    assert_eq!(bound.office_id, "OFFICE1");
    assert_eq!(bound.member_id, None);
    let refused = client
        .bind_app(AppToken::from_bytes([4; APP_TOKEN_BYTES]))
        .unwrap_err();
    assert!(
        matches!(&refused, ClientError::Api(e) if e.code == ErrorCode::NotAuthorized),
        "{refused:?}"
    );
    drop(client);
    server.finish();
}

#[test]
fn the_host_methods_are_typed_wrappers_over_the_wire_calls() {
    let server = fake(|mut peer| {
        let create = peer.next_request().unwrap();
        assert_eq!(create.method, "organization.create");
        assert_eq!(create.params["name"], "Acme");
        peer.reply::<OrganizationCreate>(create.id, &organization("Acme"));

        let list = peer.next_request().unwrap();
        assert_eq!(list.method, "organization.list");
        peer.reply::<OrganizationList>(
            list.id,
            &ListOrganizationsResult {
                organizations: vec![organization("Acme"), organization("Globex")],
            },
        );

        let offices = peer.next_request().unwrap();
        assert_eq!(offices.method, "office.list");
        peer.reply::<OfficeList>(
            offices.id,
            &ListOfficesResult {
                offices: vec![office("01ARZ3NDEKTSV4RRFFQ69G5FAW", Some("/tmp/x.sock"))],
            },
        );
    });
    let client = server.client();

    let created = client.create_organization("Acme").unwrap();
    assert_eq!(created.offices.len(), 1);
    assert_eq!(created.offices[0].role, OfficeRole::Hq);
    let names: Vec<_> = client
        .list_organizations()
        .unwrap()
        .into_iter()
        .map(|o| o.name)
        .collect();
    assert_eq!(names, ["Acme", "Globex"]);
    let offices = client.list_offices().unwrap();
    assert_eq!(offices[0].endpoint.as_deref(), Some("/tmp/x.sock"));
    drop(client);
    server.finish();
}

#[test]
fn connect_host_in_finds_host_sock_in_the_office_home() {
    let home = TempDir::new().unwrap();
    let socket = home.path().join("host.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let accepted = std::thread::spawn(move || listener.accept().is_ok());

    Client::connect_host_in(home.path()).unwrap();

    assert!(accepted.join().unwrap(), "nothing connected to host.sock");
}

#[test]
fn connect_office_uses_the_endpoint_or_the_location_rule_and_refuses_anything_else() {
    let home = TempDir::new().unwrap();
    std::fs::create_dir(home.path().join("run")).unwrap();

    // By the location rule: <home>/run/<last 12 chars of the id>.sock.
    let id = "01ARZ3NDEKTSV4RRFFQ69G5FAW";
    let by_rule = home
        .path()
        .join(format!("run/{}.sock", &id[id.len() - 12..]));
    let listener = UnixListener::bind(&by_rule).unwrap();
    let accepted = std::thread::spawn(move || listener.accept().is_ok());
    Client::connect_office(&office(id, None), home.path()).unwrap();
    assert!(accepted.join().unwrap());

    // By the endpoint the host gave.
    let given = home.path().join("run/given.sock");
    let listener = UnixListener::bind(&given).unwrap();
    let accepted = std::thread::spawn(move || listener.accept().is_ok());
    let endpoint = given.to_str().unwrap();
    Client::connect_office(&office(id, Some(endpoint)), home.path()).unwrap();
    assert!(accepted.join().unwrap());

    // A relative endpoint, and an id that could point outside `run/`, are never followed.
    for bad in [
        office(id, Some("relative.sock")),
        office("../../etc/x", None),
        office("", None),
    ] {
        let error = Client::connect_office(&bad, home.path()).err().unwrap();
        assert!(
            matches!(error, ClientError::OfficeNotReachable),
            "{error:?}"
        );
    }
}

#[test]
fn a_socket_path_that_is_too_long_is_reported_before_connecting() {
    let long_home = std::path::PathBuf::from(format!("/tmp/{}", "x".repeat(120)));
    let error = Client::connect_host_in(&long_home).err().unwrap();
    assert!(
        matches!(error, ClientError::SocketPathTooLong(_)),
        "{error:?}"
    );
}

#[test]
fn a_server_that_stops_reading_cannot_hang_a_caller_or_the_others() {
    let _exclusive = exclusive();
    watchdog(Duration::from_secs(60), || {
        let (release, hold) = mpsc::channel::<()>();
        let (write_started, started) = mpsc::channel::<()>();
        // Reads one byte (proof that the big request's write has started), then never reads
        // again.
        let server = fake(move |mut peer| {
            use std::io::Read;
            let mut first = [0u8; 1];
            if peer.stream.read_exact(&mut first).is_ok() {
                let _ = write_started.send(());
            }
            hold.recv().unwrap();
            drop(peer);
        });
        let client = std::sync::Arc::new(server.client());

        let timed = |call: Box<dyn FnOnce() -> Result<(), ClientError> + Send>| {
            std::thread::spawn(move || {
                let started = Instant::now();
                let result = call();
                (result, started.elapsed())
            })
        };
        let big = {
            let client = std::sync::Arc::clone(&client);
            timed(Box::new(move || {
                let huge = OrganizationCreateParams {
                    name: "x".repeat(MAX_FRAME_BYTES - 200),
                };
                client
                    .call_with_timeout::<OrganizationCreate>(&huge, Duration::from_secs(2))
                    .map(|_| ())
            }))
        };
        started
            .recv_timeout(Duration::from_secs(30))
            .expect("the big request's write never started");
        let other = {
            let client = std::sync::Arc::clone(&client);
            timed(Box::new(move || {
                client
                    .call_with_timeout::<Hello>(&Empty {}, Duration::from_millis(600))
                    .map(|_| ())
            }))
        };

        // The other call is bounded by its OWN timeout, not by the stuck write ahead of it,
        // and it must not have closed the connection (the big request is still going).
        let (other_result, other_took) = other.join().unwrap();
        assert!(
            matches!(other_result, Err(ClientError::Timeout)),
            "{other_result:?}"
        );
        assert!(
            other_took < Duration::from_millis(1100),
            "other took {other_took:?}"
        );
        // The big call is bounded by its timeout (2 s) plus the short grace before it closes.
        let (big_result, big_took) = big.join().unwrap();
        assert!(
            matches!(big_result, Err(ClientError::Timeout)),
            "{big_result:?}"
        );
        assert!(
            big_took < Duration::from_millis(2800),
            "big took {big_took:?}"
        );
        // The half-written request left the stream out of step: the connection was given up.
        let started = Instant::now();
        let after = client.call_with_timeout::<Hello>(&Empty {}, Duration::from_secs(3));
        assert!(matches!(after, Err(ClientError::Disconnected)), "{after:?}");
        assert!(started.elapsed() < Duration::from_secs(1));
        release.send(()).unwrap();
        drop(client);
        server.finish();
    });
}

#[test]
fn an_endpoint_outside_the_run_folder_is_never_followed() {
    let home = TempDir::new().unwrap();
    std::fs::create_dir(home.path().join("run")).unwrap();
    // A live socket somewhere else that the host could point us at.
    let elsewhere = TempDir::new().unwrap();
    let target = elsewhere.path().join("x.sock");
    let listener = UnixListener::bind(&target).unwrap();
    listener.set_nonblocking(true).unwrap();

    let id = "01ARZ3NDEKTSV4RRFFQ69G5FAW";
    let run = home.path().join("run");
    let attempts = [
        target.to_str().unwrap().to_owned(),
        format!("{}/../x.sock", run.display()),
        format!("{}/sub/x.sock", run.display()),
        format!("{}/notasocket.txt", run.display()),
        run.display().to_string(),
    ];
    for endpoint in attempts {
        let error = Client::connect_office(&office(id, Some(&endpoint)), home.path())
            .err()
            .unwrap_or_else(|| panic!("followed {endpoint}"));
        assert!(
            matches!(error, ClientError::OfficeNotReachable),
            "{endpoint}: {error:?}"
        );
    }
    assert!(
        listener.accept().is_err(),
        "something connected to the outside socket"
    );
}

#[test]
fn a_relative_office_home_is_refused() {
    let office = office("01ARZ3NDEKTSV4RRFFQ69G5FAW", None);
    let error = Client::connect_office(&office, std::path::Path::new("relative/home"))
        .err()
        .unwrap();
    assert!(matches!(error, ClientError::NoOfficeHome), "{error:?}");
    let error = Client::connect_host_in(std::path::Path::new("relative/home"))
        .err()
        .unwrap();
    assert!(matches!(error, ClientError::NoOfficeHome), "{error:?}");
}

#[test]
fn a_flood_of_events_nobody_reads_closes_the_connection_instead_of_filling_memory() {
    watchdog(Duration::from_secs(60), || {
        let (finish, hold) = mpsc::channel::<()>();
        let server = fake(move |mut peer| {
            let request = peer.next_request().unwrap();
            peer.reply::<org_proto::Subscribe>(request.id, &Empty {});
            for n in 0..2_000 {
                let event =
                    Event::new::<OrganizationCreated>(n + 1, &organization("Flood")).unwrap();
                if frame::write_message(&mut peer.stream, &event).is_err() {
                    break; // the client gave up on us
                }
            }
            // Keep the connection open either way: a hang-up here must come from the client,
            // not from this script ending.
            hold.recv().unwrap();
        });
        let client = server.client();
        let events = client.subscribe().unwrap();

        // The app doesn't read for a while.
        std::thread::sleep(Duration::from_millis(500));

        let mut received = 0;
        while events.recv_timeout(Duration::from_secs(2)).is_ok() {
            received += 1;
        }
        assert!(
            received > 0 && received <= 256,
            "buffered {received} events"
        );
        let after = client.call_with_timeout::<Hello>(&Empty {}, Duration::from_secs(2));
        assert!(matches!(after, Err(ClientError::Disconnected)), "{after:?}");
        finish.send(()).unwrap();
        drop(client);
        server.finish();
    });
}

#[test]
fn a_second_subscribe_is_refused_and_the_first_keeps_working() {
    let (go, wait_for_go) = mpsc::channel::<()>();
    let server = fake(move |mut peer| {
        let request = peer.next_request().unwrap();
        peer.reply::<org_proto::Subscribe>(request.id, &Empty {});
        wait_for_go.recv().unwrap();
        let event = Event::new::<OrganizationCreated>(1, &organization("Still here")).unwrap();
        frame::write_message(&mut peer.stream, &event).unwrap();
    });
    let client = server.client();
    let first = client.subscribe().unwrap();

    let second = client.subscribe();
    assert!(
        matches!(second, Err(ClientError::AlreadySubscribed)),
        "{second:?}"
    );

    go.send(()).unwrap();
    let event = first.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(
        event.data::<OrganizationCreated>().unwrap().unwrap().name,
        "Still here"
    );
    drop(client);
    server.finish();
}

#[test]
fn when_the_reader_gives_up_the_socket_is_closed_so_the_server_is_told() {
    let server = fake(|mut peer| {
        use std::io::{Read, Write};
        peer.stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        // A "line" longer than the protocol allows, with no newline: the client can't stay
        // in step after it.
        let _ = peer.stream.write_all(&vec![b'a'; MAX_FRAME_BYTES + 10]);
        // Drain what the client sent (its own request), then expect the end of the stream.
        // A reset also counts as closed; only a read that times out means it stayed open.
        let mut sink = [0u8; 64];
        loop {
            match peer.reader.read(&mut sink) {
                Ok(0) => break,
                Ok(_) => {}
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    panic!("the client never closed its end")
                }
                Err(_) => break,
            }
        }
    });
    let client = server.client();

    let result = client.call_with_timeout::<Hello>(&Empty {}, Duration::from_secs(5));

    assert!(
        matches!(result, Err(ClientError::Disconnected)),
        "{result:?}"
    );
    server.finish();
    drop(client);
}

#[test]
fn dropping_the_client_closes_the_connection() {
    let (dropped, wait_for_drop) = mpsc::channel::<()>();
    let server = fake(move |mut peer| {
        use std::io::Read;
        // The client may already be gone, and macOS then refuses a socket option: not an error.
        let _ = peer.stream.set_read_timeout(Some(Duration::from_secs(5)));
        wait_for_drop.recv().unwrap();
        let mut sink = [0u8; 8];
        let end = peer.reader.read(&mut sink);
        assert!(matches!(end, Ok(0)), "the connection stayed open: {end:?}");
    });
    let client = server.client();
    drop(client);
    dropped.send(()).unwrap();
    server.finish();
}

#[test]
fn a_reply_under_id_zero_matches_no_call_and_fails_none() {
    let server = fake(|mut peer| {
        let request = peer.next_request().unwrap();
        // The server could not read some request of ours and answers under id 0.
        peer.raw(json!({"id": 0, "error": {"code": "invalid_request", "message": "x"}}));
        peer.reply::<Hello>(request.id, &hello_result(API_VERSION));
    });
    let client = server.client();

    assert_eq!(client.hello().unwrap().server_version, "9.9.9");
    drop(client);
    server.finish();
}

#[test]
fn a_server_that_reads_a_few_bytes_at_a_time_cannot_stretch_the_timeout() {
    let _exclusive = exclusive();
    watchdog(Duration::from_secs(60), || {
        let (stop, stopped) = mpsc::channel::<()>();
        let server = fake(move |mut peer| {
            use std::io::Read;
            // A real slow drip: 2000 bytes every 100 ms straight off the socket (not through a
            // BufReader, which would read 8 KiB at a time), so every write makes some progress
            // and the kernel's send timeout keeps restarting.
            let mut chunk = [0u8; 2000];
            while stopped.try_recv().is_err() {
                if peer.stream.read(&mut chunk).map_or(true, |n| n == 0) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        let client = server.client();

        let started = Instant::now();
        let huge = OrganizationCreateParams {
            name: "x".repeat(MAX_FRAME_BYTES - 200),
        };
        // Long enough that the request reaches the writer well before its deadline even when a
        // slow debug build, on a busy machine, takes most of a second to encode 1 MiB.
        let result =
            client.call_with_timeout::<OrganizationCreate>(&huge, Duration::from_millis(1500));

        assert!(matches!(result, Err(ClientError::Timeout)), "{result:?}");
        // 1.5 s plus the short grace before the call closes the stuck connection itself.
        assert!(
            started.elapsed() < Duration::from_millis(2400),
            "took {:?}",
            started.elapsed()
        );
        // The half-sent request put half a frame on the wire: the connection must have been
        // given up, not left limping. (Not just "the caller stopped waiting".)
        let started = Instant::now();
        let after = client.call_with_timeout::<Hello>(&Empty {}, Duration::from_secs(2));
        assert!(matches!(after, Err(ClientError::Disconnected)), "{after:?}");
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "took {:?}",
            started.elapsed()
        );
        stop.send(()).unwrap();
        drop(client);
        server.finish();
    });
}

#[test]
fn a_server_that_half_closes_while_a_request_is_being_written_wakes_the_caller_at_once() {
    let _exclusive = exclusive();
    let (stop, stopped) = mpsc::channel::<()>();
    let (seen_end, saw_end) = mpsc::channel::<()>();
    let server = fake(move |mut peer| {
        use std::io::Read;
        // Reads one byte, which proves the client's write has started, and nothing more, so
        // the write blocks; then says it is done sending. After that macOS refuses the
        // client's `shutdown(Both)`, so only the fallback can wake it.
        peer.stream
            .set_read_timeout(Some(Duration::from_secs(60)))
            .unwrap();
        let mut first = [0u8; 1];
        peer.stream.read_exact(&mut first).unwrap();
        peer.stream.shutdown(std::net::Shutdown::Write).unwrap();
        // Now read what the client had written and wait for the end of the stream. It only
        // comes if the client freed its blocked writer and shut its end: a writer left blocked
        // would just finish writing into our reads and then go quiet.
        peer.stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut sink = [0u8; 64 * 1024];
        loop {
            match peer.stream.read(&mut sink) {
                Ok(0) => break,
                Ok(_) => {}
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    panic!("the client never closed its end after the half-close")
                }
                Err(_) => break,
            }
        }
        // Reported while the client is still alive: dropping it would close its socket and
        // give the same end-of-stream for the wrong reason.
        seen_end.send(()).unwrap();
        stopped.recv().unwrap();
    });
    let client = server.client();

    let started = Instant::now();
    let huge = OrganizationCreateParams {
        name: "x".repeat(MAX_FRAME_BYTES - 200),
    };
    let result = client.call_with_timeout::<OrganizationCreate>(&huge, Duration::from_secs(5));

    assert!(
        matches!(result, Err(ClientError::Disconnected)),
        "{result:?}"
    );
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "took {:?}",
        started.elapsed()
    );
    assert!(
        saw_end.recv_timeout(Duration::from_secs(10)).is_ok(),
        "the server never saw the client's end close while the client was alive"
    );
    stop.send(()).unwrap();
    drop(client);
    server.finish();
}

#[test]
fn an_endpoint_that_is_not_a_plain_socket_in_run_is_never_followed() {
    let home = TempDir::new().unwrap();
    let run = home.path().join("run");
    std::fs::create_dir(&run).unwrap();
    let id = "01ARZ3NDEKTSV4RRFFQ69G5FAW";
    let refused = |endpoint: &std::path::Path| {
        let error = Client::connect_office(&office(id, endpoint.to_str()), home.path())
            .err()
            .unwrap_or_else(|| panic!("followed {}", endpoint.display()));
        assert!(
            matches!(error, ClientError::OfficeNotReachable),
            "{error:?}"
        );
    };

    // Another directory level that merely looks like a socket name.
    refused(&run.join("evil.sock").join("x.sock"));
    // A regular file with a socket's name.
    std::fs::write(run.join("file.sock"), b"not a socket").unwrap();
    refused(&run.join("file.sock"));
    // A link to a live socket somewhere else.
    let elsewhere = TempDir::new().unwrap();
    let target = elsewhere.path().join("real.sock");
    let listener = UnixListener::bind(&target).unwrap();
    listener.set_nonblocking(true).unwrap();
    std::os::unix::fs::symlink(&target, run.join("link.sock")).unwrap();
    refused(&run.join("link.sock"));
    assert!(
        listener.accept().is_err(),
        "something connected through the link"
    );

    // `run` itself is a link to somewhere else.
    let other_home = TempDir::new().unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), other_home.path().join("run")).unwrap();
    let error = Client::connect_office(&office(id, None), other_home.path())
        .err()
        .unwrap();
    assert!(
        matches!(error, ClientError::OfficeNotReachable),
        "{error:?}"
    );
}

#[test]
fn a_subscription_can_be_redone_as_soon_as_it_is_dropped_even_on_a_quiet_connection() {
    let server = fake(|mut peer| {
        for _ in 0..2 {
            let request = peer.next_request().unwrap();
            peer.reply::<org_proto::Subscribe>(request.id, &Empty {});
        }
    });
    let client = server.client();

    drop(client.subscribe().unwrap());
    // No event has arrived in between: dropping must release it on its own.
    let _again = client.subscribe().expect("could not subscribe again");
    drop(client);
    server.finish();
}

#[test]
fn a_subscribe_the_server_refuses_can_be_retried() {
    let server = fake(|mut peer| {
        let first = peer.next_request().unwrap();
        peer.fail(first.id, ErrorCode::InvalidRequest);
        let second = peer.next_request().unwrap();
        peer.reply::<org_proto::Subscribe>(second.id, &Empty {});
    });
    let client = server.client();

    assert!(matches!(client.subscribe(), Err(ClientError::Api(_))));
    let _again = client.subscribe().expect("the slot was not released");
    drop(client);
    server.finish();
}

#[test]
fn zero_and_enormous_timeouts_behave_and_a_call_that_ran_out_of_time_is_never_sent() {
    let server = fake(|mut peer| {
        // The zero-timeout call was withdrawn before the writer started on it, so the first
        // request this server ever sees is the second call (id 2).
        let request = peer.next_request().unwrap();
        assert_eq!(request.id, 2, "a call that had already timed out was sent");
        peer.reply::<Hello>(request.id, &hello_result(API_VERSION));
    });
    let client = server.client();

    let zero = client.call_with_timeout::<Hello>(&Empty {}, Duration::ZERO);
    assert!(matches!(zero, Err(ClientError::Timeout)), "{zero:?}");
    // A timeout too big to add to a clock reading must not panic or fail early.
    let answered = client.call_with_timeout::<Hello>(&Empty {}, Duration::MAX);
    assert!(answered.is_ok(), "{answered:?}");
    drop(client);
    server.finish();
}

#[test]
fn a_socket_path_that_is_too_long_is_reported_for_every_way_of_connecting() {
    let long_home = std::path::PathBuf::from(format!("/tmp/{}", "x".repeat(120)));
    let id = "01ARZ3NDEKTSV4RRFFQ69G5FAW";
    let error = Client::connect_office(&office(id, None), &long_home)
        .err()
        .unwrap();
    assert!(
        matches!(error, ClientError::SocketPathTooLong(_)),
        "{error:?}"
    );
    let error = Client::connect(long_home.join("x.sock")).err().unwrap();
    assert!(
        matches!(error, ClientError::SocketPathTooLong(_)),
        "{error:?}"
    );
}

#[test]
fn tiny_timeouts_against_a_healthy_server_never_close_the_connection() {
    let _exclusive = exclusive();
    watchdog(Duration::from_secs(120), || {
        let server = fake(|mut peer| {
            while let Ok(Some(request)) = frame::read_message::<Request>(&mut peer.reader) {
                let answer = Response::ok::<Hello>(request.id, &hello_result(API_VERSION)).unwrap();
                if frame::write_message(&mut peer.stream, &answer).is_err() {
                    return;
                }
            }
        });
        let client = std::sync::Arc::new(server.client());

        // Deadlines of tens of microseconds land while the writer is writing, has just
        // finished, or has not started. None of that is a reason to close.
        let callers: Vec<_> = (0..8)
            .map(|n| {
                let client = std::sync::Arc::clone(&client);
                std::thread::spawn(move || {
                    let mut disconnected = 0;
                    for i in 0..300u64 {
                        let timeout = Duration::from_micros(20 + (i * 7 + n * 13) % 400);
                        if let Err(ClientError::Disconnected) =
                            client.call_with_timeout::<Hello>(&Empty {}, timeout)
                        {
                            disconnected += 1;
                        }
                    }
                    disconnected
                })
            })
            .collect();
        let disconnected: i32 = callers.into_iter().map(|c| c.join().unwrap()).sum();

        assert_eq!(disconnected, 0, "a healthy connection was closed");
        client.hello().expect("the connection did not survive");
        drop(client);
        server.finish();
    });
}

#[test]
fn a_burst_of_large_requests_to_a_healthy_server_all_succeed() {
    let _exclusive = exclusive();
    watchdog(Duration::from_secs(120), || {
        let server = fake(|mut peer| {
            while let Ok(Some(request)) = frame::read_message::<Request>(&mut peer.reader) {
                let name = request.params::<OrganizationCreate>().unwrap().name;
                let answer =
                    Response::ok::<OrganizationCreate>(request.id, &organization(&name[..4]))
                        .unwrap();
                if frame::write_message(&mut peer.stream, &answer).is_err() {
                    return;
                }
            }
        });
        let client = std::sync::Arc::new(server.client());

        // 24 requests of about 1 MiB at once: more than the queue's byte budget holds. They
        // wait for room (up to their own deadline) instead of closing the connection.
        let callers: Vec<_> = (0..24)
            .map(|_| {
                let client = std::sync::Arc::clone(&client);
                std::thread::spawn(move || {
                    let big = OrganizationCreateParams {
                        name: "x".repeat(MAX_FRAME_BYTES - 300),
                    };
                    client.call_with_timeout::<OrganizationCreate>(&big, Duration::from_secs(60))
                })
            })
            .collect();
        for caller in callers {
            let result = caller.join().unwrap();
            assert!(result.is_ok(), "{result:?}");
        }
        // The connection is still there.
        client
            .create_organization("Acme")
            .expect("the connection did not survive");
        drop(client);
        server.finish();
    });
}

#[test]
fn subscribe_from_sends_the_position_and_a_refused_resume_can_be_retried_live() {
    watchdog(Duration::from_secs(60), || {
        let server = fake(|mut peer| {
            let first = peer.next_request().unwrap();
            assert_eq!(first.method, "subscribe");
            assert_eq!(
                first.params["from_position"],
                json!({"epoch": 1, "seq": 42})
            );
            peer.fail(first.id, ErrorCode::SnapshotRequired);
            // After a snapshot, the app follows live: no position on the wire.
            let second = peer.next_request().unwrap();
            assert!(
                second.params.get("from_position").is_none(),
                "{}",
                second.params
            );
            peer.reply::<org_proto::Subscribe>(second.id, &Empty {});
        });
        let client = server.client();

        match client.subscribe_from(Some(Position { epoch: 1, seq: 42 })) {
            Err(ClientError::Api(error)) => assert_eq!(error.code, ErrorCode::SnapshotRequired),
            other => panic!("expected snapshot_required, got {other:?}"),
        }
        // The refused attempt released the slot.
        let _live = client
            .subscribe()
            .expect("could not subscribe live after a refused resume");
        drop(client);
        server.finish();
    });
}

#[test]
fn a_deadline_that_falls_mid_write_does_not_close_a_healthy_connection() {
    let _exclusive = exclusive();
    // The case needs the deadline to fall after the writer starts and before the write ends,
    // with the write still ending inside the grace period. On a busy machine any of those
    // can slip, so the test passes if ANY of ten attempts shows the right behaviour. A
    // client that closes at the deadline fails every attempt, so a real regression still fails.
    let mut failures = Vec::new();
    for attempt in 1..=10 {
        match mid_write_attempt() {
            Ok(()) => return,
            Err(why) => failures.push(format!("attempt {attempt}: {why}")),
        }
    }
    panic!(
        "no attempt showed a healthy connection surviving a mid-write deadline:\n{}",
        failures.join("\n")
    );
}

fn mid_write_attempt() -> Result<(), String> {
    watchdog(Duration::from_secs(60), || {
        let saw_big = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let server_saw = std::sync::Arc::clone(&saw_big);
        let (go, wait_for_go) = mpsc::channel::<()>();
        let server = fake(move |mut peer| {
            use std::io::Read;
            // Reads nothing until told to, so the client's write of the big request blocks
            // once the socket buffers are full: it is mid-write when the deadline falls. Then
            // it reads everything at once, and the write ends a moment after the deadline,
            // inside the grace period.
            if wait_for_go.recv().is_err() {
                return;
            }
            let mut pending = Vec::new();
            let mut chunk = vec![0u8; 64 * 1024];
            loop {
                match peer.stream.read(&mut chunk) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => pending.extend_from_slice(&chunk[..n]),
                }
                while let Some(end) = pending.iter().position(|&b| b == b'\n') {
                    let line: Vec<u8> = pending.drain(..=end).collect();
                    if let Ok(request) = serde_json::from_slice::<Request>(&line) {
                        if request.method == "organization.create" {
                            server_saw.store(true, std::sync::atomic::Ordering::SeqCst);
                        }
                        if request.method == "hello" {
                            let answer =
                                Response::ok::<Hello>(request.id, &hello_result(API_VERSION))
                                    .unwrap();
                            if frame::write_message(&mut peer.stream, &answer).is_err() {
                                return;
                            }
                        }
                    }
                }
            }
        });
        let client = server.client();

        let big = OrganizationCreateParams {
            name: "x".repeat(256 * 1024),
        };
        // 20 ms after the 150 ms deadline, the server starts reading.
        let timer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(170));
            let _ = go.send(());
        });
        let result =
            client.call_with_timeout::<OrganizationCreate>(&big, Duration::from_millis(150));
        timer.join().unwrap();
        let outcome = if !matches!(result, Err(ClientError::Timeout)) {
            Err(format!("the big call did not time out: {result:?}"))
        } else if let Err(e) = client.hello() {
            Err(format!(
                "the connection did not survive the mid-write deadline: {e:?}"
            ))
        } else if !saw_big.load(std::sync::atomic::Ordering::SeqCst) {
            Err("the deadline fell before the writer started: nothing was mid-write".to_string())
        } else {
            Ok(())
        };
        drop(client);
        server.finish();
        outcome
    })
}

#[test]
fn a_live_subscription_does_not_keep_the_connection_open_after_the_client_is_dropped() {
    watchdog(Duration::from_secs(60), || {
        let (done, finished) = mpsc::channel::<()>();
        let server = fake(move |mut peer| {
            let request = peer.next_request().unwrap();
            peer.reply::<org_proto::Subscribe>(request.id, &Empty {});
            // Write events until the client is gone. On macOS a write that is blocked because
            // nobody reads is only released when the client's socket is really CLOSED, not
            // merely shut down.
            let event = Event::new::<OrganizationCreated>(1, &organization("Flood")).unwrap();
            while frame::write_message(&mut peer.stream, &event).is_ok() {}
            let _ = done.send(());
        });
        let client = server.client();
        let subscription = client.subscribe().unwrap();
        std::thread::sleep(Duration::from_millis(200)); // the server is now blocked writing

        drop(client);

        // The subscription is still alive, and must not be what holds the socket open.
        assert!(
            finished.recv_timeout(Duration::from_secs(10)).is_ok(),
            "the server stayed blocked while a Subscription was alive"
        );
        drop(subscription);
        server.finish();
    });
}

#[test]
fn a_subscribe_that_gets_no_answer_closes_the_connection_but_one_that_was_never_sent_does_not() {
    let _exclusive = exclusive();
    watchdog(Duration::from_secs(60), || {
        // (a) Sent, never answered: the server may or may not have subscribed, so reconnect.
        let (release, hold) = mpsc::channel::<()>();
        let server = fake(move |mut peer| {
            let _ = peer.next_request();
            hold.recv().ok();
        });
        let client = server.client();
        let timed_out = client.subscribe_from_with_timeout(None, Duration::from_millis(300));
        assert!(
            matches!(timed_out, Err(ClientError::Timeout)),
            "{timed_out:?}"
        );
        let after = client.call_with_timeout::<Hello>(&Empty {}, Duration::from_secs(2));
        assert!(matches!(after, Err(ClientError::Disconnected)), "{after:?}");
        release.send(()).unwrap();
        drop(client);
        server.finish();

        // (b) Stuck behind another call's write, so nothing of it was sent: leave the
        // connection alone.
        let (release, hold) = mpsc::channel::<()>();
        let (write_started, started) = mpsc::channel::<()>();
        let server = fake(move |mut peer| {
            use std::io::Read;
            // One byte of the big request proves the writer has started on it; the rest is
            // never read, so the write stays blocked.
            let mut first = [0u8; 1];
            if peer.stream.read_exact(&mut first).is_ok() {
                let _ = write_started.send(());
            }
            hold.recv().ok();
            drop(peer);
        });
        let client = std::sync::Arc::new(server.client());
        let stuck = {
            let client = std::sync::Arc::clone(&client);
            std::thread::spawn(move || {
                let big = OrganizationCreateParams {
                    name: "x".repeat(MAX_FRAME_BYTES - 200),
                };
                client.call_with_timeout::<OrganizationCreate>(&big, Duration::from_secs(3))
            })
        };
        started
            .recv_timeout(Duration::from_secs(30))
            .expect("the big request's write never started");
        let waiting = client.subscribe_from_with_timeout(None, Duration::from_millis(300));
        assert!(matches!(waiting, Err(ClientError::Timeout)), "{waiting:?}");
        // The stuck write is still going, and the subscribe did not kill the connection.
        let still_open = client.call_with_timeout::<Hello>(&Empty {}, Duration::from_millis(200));
        assert!(
            matches!(still_open, Err(ClientError::Timeout)),
            "{still_open:?}"
        );
        assert!(matches!(stuck.join().unwrap(), Err(ClientError::Timeout)));
        release.send(()).unwrap();
        drop(client);
        server.finish();
    });
}

#[test]
fn more_callers_than_the_queue_holds_wait_for_room_and_time_out_without_closing_anything() {
    let _exclusive = exclusive();
    watchdog(Duration::from_secs(90), || {
        let (release, hold) = mpsc::channel::<()>();
        let (write_started, started) = mpsc::channel::<()>();
        let server = fake(move |mut peer| {
            use std::io::Read;
            let mut first = [0u8; 1];
            if peer.stream.read_exact(&mut first).is_ok() {
                let _ = write_started.send(());
            }
            hold.recv().ok();
            drop(peer);
        });
        let client = std::sync::Arc::new(server.client());
        let stuck = {
            let client = std::sync::Arc::clone(&client);
            std::thread::spawn(move || {
                let big = OrganizationCreateParams {
                    name: "x".repeat(MAX_FRAME_BYTES - 200),
                };
                client.call_with_timeout::<OrganizationCreate>(&big, Duration::from_secs(4))
            })
        };
        started
            .recv_timeout(Duration::from_secs(30))
            .expect("the big request's write never started");

        // 1,100 small calls behind a stuck write: more than the 1,024-slot queue holds.
        let callers: Vec<_> = (0..1_100)
            .map(|_| {
                let client = std::sync::Arc::clone(&client);
                std::thread::Builder::new()
                    .stack_size(256 * 1024)
                    .spawn(move || {
                        client.call_with_timeout::<Hello>(&Empty {}, Duration::from_millis(600))
                    })
                    .unwrap()
            })
            .collect();
        let results: Vec<_> = callers.into_iter().map(|c| c.join().unwrap()).collect();

        let timeouts = results
            .iter()
            .filter(|r| matches!(r, Err(ClientError::Timeout)))
            .count();
        let mut kinds = std::collections::BTreeMap::<String, usize>::new();
        for r in &results {
            let kind = match r {
                Ok(_) => "ok".to_string(),
                Err(e) => format!("{e:?}").chars().take(30).collect(),
            };
            *kinds.entry(kind).or_default() += 1;
        }
        assert_eq!(
            timeouts,
            results.len(),
            "a caller got something other than Timeout: {kinds:?}"
        );
        assert!(matches!(stuck.join().unwrap(), Err(ClientError::Timeout)));
        release.send(()).unwrap();
        drop(client);
        server.finish();
    });
}

#[test]
fn creating_an_organization_waits_longer_than_the_short_default() {
    // The host may hold a create for a while at startup: an answer that arrives after the
    // short default (10 s) but well inside the create timeout is still an answer.
    let server = fake(|mut peer| {
        let request = peer.next_request().unwrap();
        std::thread::sleep(Duration::from_millis(10_500));
        peer.reply::<OrganizationCreate>(request.id, &organization("Acme"));
    });
    let client = server.client();

    let created = watchdog(Duration::from_secs(40), move || {
        let created = client.create_organization("Acme");
        drop(client);
        created
    });

    assert_eq!(created.unwrap().name, "Acme");
    server.finish();
}

#[test]
fn a_server_that_answers_before_it_has_read_the_whole_request_cannot_wedge_the_connection() {
    // The call is answered (ids are sequential, so a server can predict them) while the rest of
    // its request is still being written. The writer itself must give the stuck write up, so
    // later calls fail with `Disconnected` at once instead of timing out for ever.
    let _exclusive = exclusive();
    let (release, hold) = mpsc::channel::<()>();
    let server = fake(move |mut peer| {
        use std::io::Read;
        let mut first = [0u8; 1];
        peer.stream.read_exact(&mut first).unwrap();
        // Answer id 1 now, then never read the request's rest.
        peer.reply::<OrganizationCreate>(1, &organization("Acme"));
        hold.recv().ok();
        drop(peer);
    });
    let client = server.client();

    let big = OrganizationCreateParams {
        name: "x".repeat(256 * 1024),
    };
    let answered =
        client.call_with_timeout::<OrganizationCreate>(&big, Duration::from_millis(1500));
    assert!(answered.is_ok(), "{answered:?}");

    let started = Instant::now();
    let later = client.call_with_timeout::<Hello>(&Empty {}, Duration::from_secs(20));
    assert!(
        matches!(later, Err(ClientError::Disconnected)),
        "{later:?} after {:?}",
        started.elapsed()
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    release.send(()).unwrap();
    drop(client);
    server.finish();
}
