//! The office server over a real socket, driven by a bare-bones client built straight on
//! `org_proto::frame`, so these tests don't depend on `org-client`.

use std::io::BufReader;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use org_office::{InMemoryKeys, Key};
use org_office_server::{Config, Server, ServerError};
use org_proto::frame;
use org_proto::{
    Call, CallError, Empty, ErrorCode, Event, Hello, ListOrganizationsResult, Organization,
    OrganizationCreate, OrganizationCreateParams, OrganizationCreated, OrganizationList, Request,
    Response, ServerMessage, Subscribe, API_VERSION,
};
use tempfile::TempDir;

fn keys() -> InMemoryKeys {
    InMemoryKeys::fixed(Key::from_bytes([3; 32]))
}

struct Fixture {
    // Fields drop in this order: the server must stop before its directory goes away.
    server: Server,
    home: PathBuf,
    _dir: TempDir,
}

fn start() -> Fixture {
    let dir = TempDir::new().unwrap();
    let home = dir.path().join("office");
    let socket = home.join("office.sock");
    let server = Server::start(Config::new(&home, socket), &keys()).unwrap();
    Fixture {
        server,
        home,
        _dir: dir,
    }
}

struct Client {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    next_id: u64,
}

impl Client {
    fn connect(socket: &Path) -> Self {
        let stream = UnixStream::connect(socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        Self {
            reader: BufReader::new(stream.try_clone().unwrap()),
            stream,
            next_id: 1,
        }
    }

    fn send<C: Call>(&mut self, params: &C::Params) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        frame::write_message(&mut self.stream, &Request::new::<C>(id, params).unwrap()).unwrap();
        id
    }

    fn read(&mut self) -> ServerMessage {
        frame::read_message(&mut self.reader)
            .unwrap()
            .expect("server closed the connection")
    }

    fn read_response(&mut self) -> Response {
        match self.read() {
            ServerMessage::Response(response) => response,
            other => panic!("expected a response, got {other:?}"),
        }
    }

    fn call<C: Call>(&mut self, params: &C::Params) -> Result<C::Result, CallError> {
        let id = self.send::<C>(params);
        let response = self.read_response();
        assert_eq!(response.id, id);
        response.into_result::<C>()
    }

    fn create(&mut self, name: &str) -> Result<Organization, CallError> {
        self.call::<OrganizationCreate>(&OrganizationCreateParams { name: name.into() })
    }

    fn raw_line(&mut self, line: &[u8]) {
        use std::io::Write;
        self.stream.write_all(line).unwrap();
        self.stream.write_all(b"\n").unwrap();
    }
}

fn error_code(result: Result<impl std::fmt::Debug, CallError>) -> ErrorCode {
    match result {
        Err(CallError::Api(error)) => error.code,
        other => panic!("expected an API error, got {other:?}"),
    }
}

#[test]
fn hello_reports_version_and_office() {
    let fixture = start();
    let mut client = Client::connect(fixture.server.socket_path());

    let hello = client.call::<Hello>(&Empty {}).unwrap();

    assert_eq!(hello.api_version, API_VERSION);
    assert_eq!(hello.server_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(hello.office.host, "This Mac");
    assert_eq!(hello.office.id.len(), 26);
}

#[test]
fn creates_and_lists_organizations() {
    let fixture = start();
    let mut client = Client::connect(fixture.server.socket_path());

    let acme = client.create("  Acme  ").unwrap();
    let globex = client.create("Globex").unwrap();
    let ListOrganizationsResult { organizations } =
        client.call::<OrganizationList>(&Empty {}).unwrap();

    assert_eq!(acme.name, "Acme");
    assert_eq!(organizations, vec![acme, globex]);
}

#[test]
fn rejects_bad_names_and_duplicates_with_their_codes() {
    let fixture = start();
    let mut client = Client::connect(fixture.server.socket_path());
    client.create("Acme").unwrap();

    assert_eq!(error_code(client.create("   ")), ErrorCode::InvalidName);
    assert_eq!(
        error_code(client.create(&"x".repeat(65))),
        ErrorCode::InvalidName
    );
    assert_eq!(error_code(client.create("ACME")), ErrorCode::AlreadyExists);
    // None of the failures disturbed the connection or the data.
    let listed = client.call::<OrganizationList>(&Empty {}).unwrap();
    assert_eq!(listed.organizations.len(), 1);
}

#[test]
fn unreadable_and_unknown_requests_get_error_responses() {
    let fixture = start();
    let mut client = Client::connect(fixture.server.socket_path());

    client.raw_line(br#"{"id":7,"method":"no.such.method","params":{}}"#);
    let unknown = client.read_response();
    assert_eq!(unknown.id, 7);
    assert!(
        matches!(unknown.outcome, org_proto::Outcome::Error(e) if e.code == ErrorCode::UnknownMethod)
    );

    // A readable id is recovered from a line that isn't a valid request.
    client.raw_line(br#"{"id":42,"method":5}"#);
    let bad = client.read_response();
    assert_eq!(bad.id, 42);
    assert!(
        matches!(bad.outcome, org_proto::Outcome::Error(e) if e.code == ErrorCode::InvalidRequest)
    );

    // Not JSON at all: answered under id 0, and the connection stays usable.
    client.raw_line(b"this is not json");
    let garbage = client.read_response();
    assert_eq!(garbage.id, org_proto::UNREADABLE_REQUEST_ID);

    // Params of the wrong shape.
    client.raw_line(br#"{"id":9,"method":"organization.create","params":{"name":5}}"#);
    let wrong = client.read_response();
    assert_eq!(wrong.id, 9);
    assert!(
        matches!(wrong.outcome, org_proto::Outcome::Error(e) if e.code == ErrorCode::InvalidRequest)
    );

    client.call::<Hello>(&Empty {}).unwrap();
}

#[test]
fn several_requests_in_flight_are_answered_by_id() {
    let fixture = start();
    let mut client = Client::connect(fixture.server.socket_path());

    let ids: Vec<u64> = (0..20)
        .map(|n| {
            client.send::<OrganizationCreate>(&OrganizationCreateParams {
                name: format!("Org {n}"),
            })
        })
        .collect();
    let mut answered = Vec::new();
    for _ in &ids {
        let response = client.read_response();
        let organization = response
            .clone()
            .into_result::<OrganizationCreate>()
            .unwrap();
        answered.push((response.id, organization.name));
    }

    answered.sort();
    let expected: Vec<(u64, String)> = ids
        .iter()
        .enumerate()
        .map(|(n, id)| (*id, format!("Org {n}")))
        .collect();
    assert_eq!(
        answered, expected,
        "every answer carries its own request's id"
    );
}

#[test]
fn subscribers_get_events_and_non_subscribers_do_not() {
    let fixture = start();
    let socket = fixture.server.socket_path().to_owned();
    let mut watcher = Client::connect(&socket);
    let mut bystander = Client::connect(&socket);
    let mut actor = Client::connect(&socket);
    watcher.call::<Subscribe>(&Empty {}).unwrap();

    let first = actor.create("Acme").unwrap();
    let second = actor.create("Globex").unwrap();

    let event = |client: &mut Client| match client.read() {
        ServerMessage::Event(event) => event,
        other => panic!("expected an event, got {other:?}"),
    };
    let (one, two): (Event, Event) = (event(&mut watcher), event(&mut watcher));
    assert_eq!((one.seq, two.seq), (1, 2));
    assert_eq!(one.data::<OrganizationCreated>().unwrap().unwrap(), first);
    assert_eq!(two.data::<OrganizationCreated>().unwrap().unwrap(), second);

    // The bystander's next message is the answer to its own request, not an event.
    assert_eq!(
        bystander.call::<Hello>(&Empty {}).unwrap().api_version,
        API_VERSION
    );
}

#[test]
fn a_subscriber_that_never_reads_is_dropped_and_others_keep_working() {
    let fixture = start();
    let socket = fixture.server.socket_path().to_owned();
    let mut stuck = Client::connect(&socket);
    stuck.call::<Subscribe>(&Empty {}).unwrap();
    let mut actor = Client::connect(&socket);

    // Far more events than the socket buffer and the outbound queue can hold.
    for n in 0..600 {
        actor.create(&format!("Organization number {n}")).unwrap();
    }

    assert_eq!(
        actor
            .call::<OrganizationList>(&Empty {})
            .unwrap()
            .organizations
            .len(),
        600
    );
    // The stuck client was closed: draining it ends in EOF or a reset, never a hang.
    let mut drained = 0;
    loop {
        match frame::read_message::<ServerMessage>(&mut stuck.reader) {
            Ok(Some(_)) => drained += 1,
            Ok(None) => break,
            // A reset is a cut-off; a timeout means the server just kept waiting.
            Err(frame::FrameError::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                panic!("the stuck subscriber was never cut off (read timed out)")
            }
            Err(_) => break,
        }
        assert!(drained <= 600, "the stuck subscriber was never cut off");
    }
}

#[test]
fn the_socket_is_owner_only_and_removed_on_shutdown() {
    let fixture = start();
    let socket = fixture.server.socket_path().to_owned();
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&socket), 0o600);
    assert_eq!(mode(&fixture.home), 0o700);

    let home = fixture.home.clone();
    fixture.server.shutdown();

    assert!(!socket.exists());
    // The office lock was released, so the same home can be served again.
    Server::start(Config::new(&home, &socket), &keys())
        .unwrap()
        .shutdown();
}

#[test]
fn a_stale_socket_is_replaced_and_a_second_server_is_refused() {
    let dir = TempDir::new().unwrap();
    let home = dir.path().join("office");
    let socket = home.join("office.sock");
    let config = || Config::new(&home, &socket);

    // A server that died without cleaning up leaves its socket file behind.
    let first = Server::start(config(), &keys()).unwrap();
    first.shutdown();
    std::os::unix::net::UnixListener::bind(&socket).unwrap();
    assert!(socket.exists());

    let live = Server::start(config(), &keys()).unwrap();
    assert!(matches!(
        Server::start(config(), &keys()),
        Err(ServerError::Office(org_office::OfficeError::InUse))
    ));
    // The refused attempt did not disturb the running server's socket.
    let mut client = Client::connect(live.socket_path());
    client.call::<Hello>(&Empty {}).unwrap();
}

#[test]
fn something_that_is_not_a_socket_is_never_replaced() {
    let dir = TempDir::new().unwrap();
    let home = dir.path().join("office");
    std::fs::create_dir_all(&home).unwrap();
    let socket = home.join("office.sock");
    std::fs::write(&socket, b"precious").unwrap();

    let result = Server::start(Config::new(&home, &socket), &keys());

    assert!(matches!(result, Err(ServerError::NotASocket)));
    assert_eq!(std::fs::read(&socket).unwrap(), b"precious");
}

#[test]
fn an_overlong_socket_path_is_refused_up_front() {
    let dir = TempDir::new().unwrap();
    let socket = dir.path().join("x".repeat(120));
    let result = Server::start(Config::new(dir.path().join("office"), socket), &keys());
    assert!(matches!(result, Err(ServerError::SocketPathTooLong)));
}

#[test]
fn data_survives_a_restart() {
    let dir = TempDir::new().unwrap();
    let home = dir.path().join("office");
    let socket = home.join("office.sock");

    let first = Server::start(Config::new(&home, &socket), &keys()).unwrap();
    let created = Client::connect(first.socket_path()).create("Acme").unwrap();
    first.shutdown();

    let second = Server::start(Config::new(&home, &socket), &keys()).unwrap();
    let mut client = Client::connect(second.socket_path());
    assert_eq!(
        client
            .call::<OrganizationList>(&Empty {})
            .unwrap()
            .organizations,
        vec![created]
    );
}
