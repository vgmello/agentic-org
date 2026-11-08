//! `organization.create` end to end, against the fake office server.

mod support;

use std::fs;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use org_office_host::Host;
use org_office_launch::OfficeFile;
use org_proto::{
    Empty, ErrorCode, Hello, OfficeList, OfficeRole, OfficeStatus, Organization,
    OrganizationCreate, OrganizationCreateParams, OrganizationCreated, OrganizationList, Subscribe,
    SubscribeParams,
};
use serde_json::json;
use support::{alive, eventually, restart_host, Client, TestHome};

const WAIT: Duration = Duration::from_secs(10);

fn create(client: &mut Client, name: &str) -> Result<Organization, org_proto::ApiError> {
    client.call::<OrganizationCreate>(&OrganizationCreateParams { name: name.into() })
}

fn subscriber(host: &Host) -> Client {
    let mut client = Client::connect(host.socket_path());
    client
        .call::<Subscribe>(&SubscribeParams::default())
        .unwrap();
    client
}

fn status_of(client: &mut Client, office_id: &str) -> OfficeStatus {
    client
        .call::<OfficeList>(&Empty {})
        .unwrap()
        .offices
        .into_iter()
        .find(|o| o.id == office_id)
        .map(|o| o.status)
        .unwrap_or(OfficeStatus::Unknown)
}

#[test]
fn create_stores_the_office_starts_it_and_tells_subscribers() {
    let home = TestHome::new();
    let host = Host::start(home.config()).unwrap();
    let mut events = subscriber(&host);
    let mut client = Client::connect(host.socket_path());

    let org = create(&mut client, "  Acme  ").unwrap();
    assert_eq!(org.name, "Acme");
    assert_eq!(org.offices.len(), 1);
    let hq = &org.offices[0];
    assert_eq!(hq.organization_id, org.id);
    assert_eq!(hq.name, "HQ");
    assert_eq!(hq.role, OfficeRole::Hq);
    assert_eq!(hq.host, "This Mac");
    assert!(matches!(
        hq.status,
        OfficeStatus::Starting | OfficeStatus::Active
    ));
    let expected_socket = org_proto::location::office_socket_path(&home.path(), &hq.id).unwrap();
    assert_eq!(hq.endpoint.as_deref(), expected_socket.to_str());

    // Stored: office.json in the office's folder, matching the answer.
    let stored =
        OfficeFile::from_slice(&fs::read(home.office_dir(&hq.id).join("office.json")).unwrap())
            .unwrap();
    assert_eq!(stored.org_id, org.id);
    assert_eq!(stored.org_name, "Acme");
    assert_eq!(stored.created_at, org.created_at);

    // Listed, and its server becomes active and answers hello on its endpoint.
    let listed = client.call::<OrganizationList>(&Empty {}).unwrap();
    assert_eq!(listed.organizations.len(), 1);
    assert_eq!(listed.organizations[0].id, org.id);
    eventually(WAIT, "the HQ to be active", || {
        status_of(&mut client, &hq.id) == OfficeStatus::Active
    });
    let mut office = Client::connect(&expected_socket);
    let hello = office.call::<Hello>(&Empty {}).unwrap();
    assert_eq!(hello.office.unwrap().office.id, hq.id);

    // The subscriber hears about it, with the same organization.
    let event = events.next_event(WAIT).expect("organization.created");
    let data = event.data::<OrganizationCreated>().unwrap().unwrap();
    assert_eq!(data.id, org.id);
    assert_eq!(data.offices[0].id, hq.id);

    // The create run is the office's server from now on: no second start.
    assert_eq!(home.runs("create"), 1);
    assert_eq!(home.runs(&hq.id), 0);
    host.shutdown();
}

#[test]
fn invalid_names_are_refused_before_anything_starts() {
    let home = TestHome::new();
    let host = Host::start(home.config()).unwrap();
    let mut client = Client::connect(host.socket_path());
    let too_long = "x".repeat(65);
    for name in [" \t ", "a\u{7}b", too_long.as_str()] {
        let err = create(&mut client, name).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidName, "{name:?}");
    }
    let wrong_params = client.raw(r#"{"id":99,"method":"organization.create","params":{}}"#);
    assert!(matches!(
        wrong_params.outcome,
        org_proto::Outcome::Error(ref e) if e.code == ErrorCode::InvalidRequest
    ));
    assert_eq!(home.runs("create"), 0);
    host.shutdown();
}

#[test]
fn a_duplicate_name_ignoring_case_already_exists() {
    let home = TestHome::new();
    let host = Host::start(home.config()).unwrap();
    let mut client = Client::connect(host.socket_path());
    create(&mut client, "Acme Ünïon").unwrap();
    let err = create(&mut client, " aCME üNÏON ").unwrap_err();
    assert_eq!(err.code, ErrorCode::AlreadyExists);
    assert_eq!(home.runs("create"), 1);
    assert_eq!(home.office_folders().len(), 1);
    host.shutdown();
}

#[test]
fn concurrent_creates_of_one_name_have_exactly_one_winner() {
    let home = TestHome::new();
    // A slow create, so the others really wait on it.
    home.script(json!({"create": {"do": "ready", "delay_ms": 200}}));
    let host = Host::start(home.config()).unwrap();
    let barrier = Arc::new(Barrier::new(4));
    let threads: Vec<_> = (0..4)
        .map(|i| {
            let (path, barrier) = (host.socket_path().to_path_buf(), Arc::clone(&barrier));
            thread::spawn(move || {
                let mut client = Client::connect(&path);
                barrier.wait();
                create(&mut client, if i % 2 == 0 { "Same" } else { "SAME" })
            })
        })
        .collect();
    let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    let winners = results.iter().filter(|r| r.is_ok()).count();
    assert_eq!(winners, 1, "{results:?}");
    for err in results.iter().filter_map(|r| r.as_ref().err()) {
        assert_eq!(err.code, ErrorCode::AlreadyExists);
    }
    assert_eq!(home.runs("create"), 1);
    host.shutdown();
}

#[test]
fn server_failures_map_to_errors_clean_up_and_emit_nothing() {
    let home = TestHome::new();
    home.script(json!({"create": [
        {"do": "fail", "code": "invalid_name"},
        {"do": "fail", "code": "storage_error"},
        {"do": "fail", "code": "key_missing"},
        {"do": "fail", "code": "some_future_code"},
        {"do": "ready"}
    ]}));
    let host = Host::start(home.config()).unwrap();
    let mut events = subscriber(&host);
    let mut client = Client::connect(host.socket_path());

    let expected = [
        ErrorCode::InvalidName,
        ErrorCode::StorageError,
        ErrorCode::StorageError,
        ErrorCode::StorageError,
    ];
    for (run, code) in expected.into_iter().enumerate() {
        let err = create(&mut client, "Acme").unwrap_err();
        assert_eq!(err.code, code, "run {run}");
        assert!(!err.message.contains('/'), "a path leaked: {}", err.message);
        // The fake made the folder before failing; the host had it discarded.
        assert_eq!(home.office_folders(), Vec::<String>::new(), "run {run}");
        assert_eq!(home.runs("discard"), run as u64 + 1);
    }

    // The first event this subscriber sees is the successful create: the failures sent none.
    let org = create(&mut client, "Acme").unwrap();
    let event = events.next_event(WAIT).unwrap();
    assert_eq!(
        event.data::<OrganizationCreated>().unwrap().unwrap().id,
        org.id
    );
    assert_eq!(event.seq, 1);
    host.shutdown();
}

#[test]
fn already_exists_from_the_server_keeps_the_folder() {
    let home = TestHome::new();
    home.script(json!({"create": {"do": "fail", "code": "already_exists"}}));
    let host = Host::start(home.config()).unwrap();
    let mut client = Client::connect(host.socket_path());
    let err = create(&mut client, "Acme").unwrap_err();
    assert_eq!(err.code, ErrorCode::AlreadyExists);
    assert_eq!(home.office_folders().len(), 1);
    assert_eq!(
        home.runs("discard"),
        0,
        "a folder that isn't ours was discarded"
    );
    host.shutdown();
}

#[test]
fn a_crash_before_created_is_a_storage_error_and_cleans_up() {
    let home = TestHome::new();
    home.script(json!({"create": [{"do": "crash"}, {"do": "ready"}]}));
    let host = Host::start(home.config()).unwrap();
    let mut events = subscriber(&host);
    let mut client = Client::connect(host.socket_path());
    let err = create(&mut client, "Acme").unwrap_err();
    assert_eq!(err.code, ErrorCode::StorageError);
    assert_eq!(home.office_folders(), Vec::<String>::new());
    assert!(client
        .call::<OrganizationList>(&Empty {})
        .unwrap()
        .organizations
        .is_empty());
    assert!(events.next_event(Duration::from_millis(200)).is_none());
    // The failed create released its reservation: the name can be created again.
    create(&mut client, "Acme").unwrap();
    host.shutdown();
}

#[test]
fn a_create_that_stored_its_office_but_never_reported_succeeds() {
    // The server writes office.json, then crashes, reports a failure, or hangs until the create
    // times out.
    for step in [
        json!({"do": "crash", "at": "stored"}),
        json!({"do": "fail", "at": "stored", "code": "storage_error"}),
        json!({"do": "hang", "at": "stored", "ignore_eof": true}),
    ] {
        let home = TestHome::new();
        home.script(json!({ "create": step }));
        let mut config = home.config();
        config.timings.create_timeout = Duration::from_secs(2);
        let host = Host::start(config).unwrap();
        let mut events = subscriber(&host);
        let mut client = Client::connect(host.socket_path());
        let org = create(&mut client, "Acme").unwrap();
        let office_id = org.offices[0].id.clone();
        assert_eq!(org.offices[0].status, OfficeStatus::Starting, "{step}");
        // Listed, announced, and served by a fresh `--office` run.
        let event = events.next_event(WAIT).expect("organization.created");
        assert_eq!(
            event.data::<OrganizationCreated>().unwrap().unwrap().id,
            org.id
        );
        let listed = client.call::<OrganizationList>(&Empty {}).unwrap();
        assert_eq!(listed.organizations.len(), 1, "{step}");
        eventually(WAIT, "the office to be active", || {
            status_of(&mut client, &office_id) == OfficeStatus::Active
        });
        assert_eq!(home.runs(&office_id), 1, "{step}");
        assert_eq!(home.runs("discard"), 0, "{step}");
        assert!(!alive(home.pids("create")[0]), "{step}");
        // Its name is taken.
        let err = create(&mut client, " aCME ").unwrap_err();
        assert_eq!(err.code, ErrorCode::AlreadyExists, "{step}");
        host.shutdown();
    }
}

#[test]
fn a_create_that_stored_another_office_but_never_reported_is_refused() {
    for action in ["crash", "fail"] {
        let home = TestHome::new();
        home.script(json!({"create": {
            "do": action, "at": "stored", "code": "storage_error",
            "report": {"org_name": "Other"}
        }}));
        let host = Host::start(home.config()).unwrap();
        let mut events = subscriber(&host);
        let mut client = Client::connect(host.socket_path());
        let err = create(&mut client, "Acme").unwrap_err();
        assert_eq!(err.code, ErrorCode::StorageError, "{action}");
        assert_eq!(home.office_folders(), [".refused"], "{action}");
        assert_eq!(home.refused_folders().len(), 1, "{action}");
        assert_eq!(home.runs("discard"), 0, "{action}");
        assert!(client
            .call::<OrganizationList>(&Empty {})
            .unwrap()
            .organizations
            .is_empty());
        assert!(events.next_event(Duration::from_millis(200)).is_none());
        host.shutdown();
    }
}

#[test]
fn a_create_that_stored_an_unreadable_office_keeps_it_and_its_name() {
    let home = TestHome::new();
    // The server writes an office.json that isn't valid, then fails.
    home.script(json!({"create": {
        "do": "fail", "at": "stored", "code": "storage_error", "corrupt_office_file": true
    }}));
    let host = Host::start(home.config()).unwrap();
    let mut client = Client::connect(host.socket_path());
    let err = create(&mut client, "Acme").unwrap_err();
    assert_eq!(err.code, ErrorCode::StorageError);
    // Left alone for a person: neither discarded nor moved aside, and its name stays taken.
    let folders = home.office_folders();
    assert_eq!(folders.len(), 1);
    assert!(home.office_dir(&folders[0]).join("office.json").exists());
    assert_eq!(home.runs("discard"), 0);
    let err = create(&mut client, " aCME ").unwrap_err();
    assert_eq!(err.code, ErrorCode::AlreadyExists);
    assert_eq!(home.runs("create"), 1);
    host.shutdown();
    // The next start lists it nowhere and leaves it alone too.
    let host = restart_host(&home.config());
    thread::sleep(Duration::from_millis(300));
    assert!(Client::connect(host.socket_path())
        .call::<OrganizationList>(&Empty {})
        .unwrap()
        .organizations
        .is_empty());
    assert_eq!(home.runs("discard"), 0);
    assert!(home.office_dir(&folders[0]).join("office.json").exists());
    host.shutdown();
}

#[test]
fn a_create_that_never_answers_times_out_kills_the_server_and_cleans_up() {
    let home = TestHome::new();
    home.script(json!({"create": {"do": "hang", "ignore_eof": true}}));
    let mut config = home.config();
    // Long enough for the fake to start and read the create message (a freshly built binary's
    // first launch on macOS can take a few hundred ms), short enough for a test.
    config.timings.create_timeout = Duration::from_secs(2);
    let host = Host::start(config).unwrap();
    let mut client = Client::connect(host.socket_path());
    let err = create(&mut client, "Acme").unwrap_err();
    assert_eq!(err.code, ErrorCode::StorageError);
    let pids = home.pids("create");
    assert_eq!(pids.len(), 1);
    assert!(!alive(pids[0]), "the hung create server is still running");
    // It hung after making its folder, which the host removed.
    assert_eq!(home.office_folders(), Vec::<String>::new());
    host.shutdown();
}

#[test]
fn a_crash_right_after_created_still_succeeds_and_the_office_restarts() {
    let home = TestHome::new();
    home.script(json!({"create": {"do": "crash", "at": "created"}}));
    let host = Host::start(home.config()).unwrap();
    let mut client = Client::connect(host.socket_path());
    let org = create(&mut client, "Acme").unwrap();
    let office_id = org.offices[0].id.clone();
    eventually(WAIT, "the office to restart and be active", || {
        status_of(&mut client, &office_id) == OfficeStatus::Active
    });
    assert_eq!(home.runs(&office_id), 1);
    host.shutdown();
}

#[test]
fn a_created_office_other_than_the_one_asked_for_is_refused_and_moved_aside() {
    for report in [
        json!({"org_name": "Other"}),
        json!({"office_name": "Branch"}),
        json!({"role": "satellite"}),
        json!({"org_id": "01OTHERORGOTHERORGOTHERORG"}),
    ] {
        let home = TestHome::new();
        home.script(json!({"create": {"do": "ready", "report": report}}));
        let host = Host::start(home.config()).unwrap();
        let mut events = subscriber(&host);
        let mut client = Client::connect(host.socket_path());
        let err = create(&mut client, "  Acme  ").unwrap_err();
        assert_eq!(err.code, ErrorCode::StorageError, "{report}");
        let pids = home.pids("create");
        assert_eq!(pids.len(), 1, "{report}");
        assert!(!alive(pids[0]), "{report}: its server is still running");
        assert!(
            client
                .call::<OrganizationList>(&Empty {})
                .unwrap()
                .organizations
                .is_empty(),
            "{report}"
        );
        assert!(
            events.next_event(Duration::from_millis(200)).is_none(),
            "{report}"
        );
        // The requested folder was moved, whole, to offices/.refused/, and never discarded.
        assert_eq!(home.office_folders(), [".refused"], "{report}");
        let refused = home.refused_folders();
        assert_eq!(refused.len(), 1, "{report}");
        assert!(
            org_office_launch::is_plain_id(&refused[0]),
            "{report}: {refused:?}"
        );
        let moved = home.path().join("offices/.refused").join(&refused[0]);
        let stored = fs::read(moved.join("office.json")).unwrap();
        // It is the requested office's folder (the server reported its id here).
        let office = OfficeFile::from_slice(&stored).unwrap();
        assert_eq!(refused[0], office.office_id, "{report}");
        // Its server was stopped through its stdin, so it removed its socket.
        let socket = org_proto::location::office_socket_path(&home.path(), &refused[0]).unwrap();
        assert!(!socket.exists(), "{report}: its socket is still there");
        assert_eq!(home.runs("discard"), 0, "{report}");
        host.shutdown();

        // The next host run doesn't list it, discard it or touch it.
        let host = restart_host(&home.config());
        assert!(
            Client::connect(host.socket_path())
                .call::<OrganizationList>(&Empty {})
                .unwrap()
                .organizations
                .is_empty(),
            "{report}: listed after a restart"
        );
        assert_eq!(home.runs("discard"), 0, "{report}");
        assert_eq!(home.refused_folders(), refused, "{report}");
        assert_eq!(fs::read(moved.join("office.json")).unwrap(), stored);
        host.shutdown();
    }
}

#[test]
fn a_created_office_reported_under_another_offices_id_leaves_that_office_alone() {
    const OTHER: &str = "01OTHEROFFICEOTHEROFFICE00";
    let home = TestHome::new();
    let other = support::hq(OTHER, "01ORGOTHER", "Other", "2026-11-08T00:00:00.000Z");
    home.write_office(&other);
    let stored = fs::read(home.office_dir(OTHER).join("office.json")).unwrap();
    home.script(json!({"create": {"do": "ready", "report": {"office_id": OTHER}}}));
    let host = Host::start(home.config()).unwrap();
    let mut events = subscriber(&host);
    let mut client = Client::connect(host.socket_path());
    eventually(WAIT, "the other office to be active", || {
        status_of(&mut client, OTHER) == OfficeStatus::Active
    });

    let err = create(&mut client, "Acme").unwrap_err();
    assert_eq!(err.code, ErrorCode::StorageError);
    assert!(events.next_event(Duration::from_millis(200)).is_none());
    // Only the requested folder was moved aside; the other office is untouched and still runs.
    assert_eq!(home.runs("discard"), 0);
    assert_eq!(home.office_folders(), [".refused", OTHER]);
    let refused = home.refused_folders();
    assert_eq!(refused.len(), 1);
    assert_ne!(refused[0], OTHER);
    assert_eq!(
        fs::read(home.office_dir(OTHER).join("office.json")).unwrap(),
        stored
    );
    let orgs = client.call::<OrganizationList>(&Empty {}).unwrap();
    let ids: Vec<_> = orgs.organizations.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, ["01ORGOTHER"]);
    assert_eq!(status_of(&mut client, OTHER), OfficeStatus::Active);
    assert_eq!(home.runs(OTHER), 1);
    let socket = org_proto::location::office_socket_path(&home.path(), OTHER).unwrap();
    let hello = Client::connect(&socket).call::<Hello>(&Empty {}).unwrap();
    assert_eq!(hello.office.unwrap().office.id, OTHER);
    host.shutdown();
}

#[test]
fn a_discard_refused_with_already_exists_after_a_failed_create_is_left_alone() {
    let home = TestHome::new();
    home.script(json!({
        "create": {"do": "fail", "code": "storage_error"},
        "discard": {"do": "fail", "code": "already_exists"}
    }));
    let host = Host::start(home.config()).unwrap();
    let mut client = Client::connect(host.socket_path());
    let err = create(&mut client, "Acme").unwrap_err();
    assert_eq!(err.code, ErrorCode::StorageError);
    // One discard, refused; the folder is kept and nothing retries it.
    thread::sleep(Duration::from_millis(500));
    assert_eq!(home.runs("discard"), 1);
    assert_eq!(home.office_folders().len(), 1);
    host.shutdown();
}

#[test]
fn a_create_of_another_name_proceeds_while_one_waits_and_a_duplicate_is_refused_at_once() {
    let home = TestHome::new();
    // The first create's server is slow to report; the second's isn't.
    home.script(json!({"create": [{"do": "ready", "delay_ms": 5000}, {"do": "ready"}]}));
    let host = Host::start(home.config()).unwrap();
    let path = host.socket_path().to_path_buf();
    let slow = thread::spawn(move || create(&mut Client::connect(&path), "Slow"));
    eventually(WAIT, "the slow create's server to start", || {
        home.runs("create") == 1
    });

    let mut client = Client::connect(host.socket_path());
    let fast = create(&mut client, "Fast").unwrap();
    assert!(
        !slow.is_finished(),
        "the second create waited for the first"
    );
    // A name in flight is taken, ignoring case: refused without starting a server.
    let err = create(&mut client, "sLOW").unwrap_err();
    assert_eq!(err.code, ErrorCode::AlreadyExists);
    assert!(
        !slow.is_finished(),
        "the duplicate waited for the first create"
    );
    assert_eq!(home.runs("create"), 2);

    let slow = slow.join().unwrap().unwrap();
    let names: Vec<_> = client
        .call::<OrganizationList>(&Empty {})
        .unwrap()
        .organizations
        .into_iter()
        .map(|o| o.name)
        .collect();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&fast.name) && names.contains(&slow.name));
    // Once the slow office is in the catalog, its name stays taken.
    let err = create(&mut client, "SLOW").unwrap_err();
    assert_eq!(err.code, ErrorCode::AlreadyExists);
    host.shutdown();
}

#[test]
fn a_shutdown_during_a_create_stops_its_server_adopts_nothing_and_sends_no_event() {
    let home = TestHome::new();
    // The create's server never reports; on stdin EOF it exits.
    home.script(json!({"create": {"do": "hang"}}));
    let host = Host::start(home.config()).unwrap();
    let mut events = subscriber(&host);
    let path = host.socket_path().to_path_buf();
    let creator = thread::spawn(move || {
        let mut stream = std::os::unix::net::UnixStream::connect(&path).unwrap();
        let _ = stream.set_read_timeout(Some(Duration::from_secs(20)));
        use std::io::{BufRead, Write};
        stream
            .write_all(
                b"{\"id\":1,\"method\":\"organization.create\",\"params\":{\"name\":\"Acme\"}}\n",
            )
            .unwrap();
        let mut line = String::new();
        let _ = std::io::BufReader::new(stream).read_line(&mut line);
        line
    });
    eventually(WAIT, "the create's server to start", || {
        home.runs("create") == 1 && !home.pids("create").is_empty()
    });
    let pid = home.pids("create")[0];
    host.shutdown();

    // The answer is an error, or none (the connection closed): never a created organization.
    let answer = creator.join().unwrap();
    assert!(!answer.contains("\"result\""), "{answer}");
    assert!(!alive(pid), "the create's server is still running");
    assert!(events.next_event(Duration::from_millis(200)).is_none());
    // Its cleanup ran into the shutdown: no discard was started then.
    assert_eq!(home.runs("discard"), 0);
    assert_eq!(home.office_folders().len(), 1);

    // The next start discards the unfinished folder and lists nothing.
    let host = restart_host(&home.config());
    eventually(WAIT, "the unfinished folder to be discarded", || {
        home.office_folders().is_empty()
    });
    assert!(Client::connect(host.socket_path())
        .call::<OrganizationList>(&Empty {})
        .unwrap()
        .organizations
        .is_empty());
    host.shutdown();
}

#[test]
fn an_adopted_server_gets_its_full_ready_wait_from_adoption() {
    let home = TestHome::new();
    // `created` comes 800 ms after the spawn, `ready` 800 ms after that: 1.6 s from the spawn,
    // over the ready wait counted from the spawn, within it counted from the adoption.
    home.script(json!({"create": {"do": "ready", "delay_ms": 800, "ready_delay_ms": 800}}));
    let mut config = home.config();
    config.timings.ready_timeout = Duration::from_millis(1200);
    let host = Host::start(config).unwrap();
    let mut client = Client::connect(host.socket_path());
    let org = create(&mut client, "Acme").unwrap();
    let office_id = org.offices[0].id.clone();
    eventually(WAIT, "the office to be active", || {
        status_of(&mut client, &office_id) == OfficeStatus::Active
    });
    thread::sleep(Duration::from_millis(500));
    assert_eq!(home.runs(&office_id), 0, "the adopted server was restarted");
    assert!(alive(home.pids("create")[0]));
    host.shutdown();
}

#[test]
fn subscribing_twice_on_one_connection_sends_each_event_once() {
    let home = TestHome::new();
    let host = Host::start(home.config()).unwrap();
    let mut events = subscriber(&host);
    events
        .call::<Subscribe>(&SubscribeParams::default())
        .unwrap();
    let org = create(&mut Client::connect(host.socket_path()), "Acme").unwrap();
    let event = events.next_event(WAIT).expect("organization.created");
    assert_eq!(
        event.data::<OrganizationCreated>().unwrap().unwrap().id,
        org.id
    );
    assert!(
        events.next_event(Duration::from_millis(300)).is_none(),
        "the event came twice"
    );
    host.shutdown();
}

#[test]
fn at_most_four_creates_run_at_once_and_a_fifth_waits_then_gets_unavailable() {
    let home = TestHome::new();
    // Every create's server takes 5 s to report.
    home.script(json!({"create": {"do": "ready", "delay_ms": 5000}}));
    let mut config = home.config();
    config.timings.create_wait = Duration::from_millis(500);
    let host = Host::start(config).unwrap();
    let creates: Vec<_> = (0..4)
        .map(|i| {
            let path = host.socket_path().to_path_buf();
            thread::spawn(move || create(&mut Client::connect(&path), &format!("Org {i}")))
        })
        .collect();
    eventually(WAIT, "four creates to run", || home.runs("create") == 4);

    let mut client = Client::connect(host.socket_path());
    let started = Instant::now();
    let err = create(&mut client, "Fifth").unwrap_err();
    assert_eq!(err.code, ErrorCode::Unavailable);
    assert_eq!(err.message, "too many creates in progress");
    assert!(started.elapsed() >= Duration::from_millis(500));
    assert_eq!(home.runs("create"), 4, "a fifth server started");
    // A name already in flight is answered at once, slots or not (waiting for a slot would end
    // in `unavailable`).
    let err = create(&mut client, "ORG 0").unwrap_err();
    assert_eq!(err.code, ErrorCode::AlreadyExists);

    for create in creates {
        create.join().unwrap().unwrap();
    }
    // The slots are free again.
    create(&mut client, "Sixth").unwrap();
    host.shutdown();
}

#[test]
fn a_waiting_create_goes_ahead_as_soon_as_a_slot_frees() {
    let home = TestHome::new();
    // The first four creates' servers take 1 s to report; the fifth's doesn't wait.
    home.script(json!({"create": [
        {"do": "ready", "delay_ms": 1000},
        {"do": "ready", "delay_ms": 1000},
        {"do": "ready", "delay_ms": 1000},
        {"do": "ready", "delay_ms": 1000},
        {"do": "ready"}
    ]}));
    let host = Host::start(home.config()).unwrap(); // a 10 s create wait
    let creates: Vec<_> = (0..4)
        .map(|i| {
            let path = host.socket_path().to_path_buf();
            thread::spawn(move || create(&mut Client::connect(&path), &format!("Org {i}")))
        })
        .collect();
    eventually(WAIT, "four creates to run", || home.runs("create") == 4);
    let started = Instant::now();
    create(&mut Client::connect(host.socket_path()), "Fifth").unwrap();
    // It went ahead when a slot freed (about 1 s), not at the end of its wait.
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "waited {:?}",
        started.elapsed()
    );
    assert_eq!(home.runs("create"), 5);
    for create in creates {
        create.join().unwrap().unwrap();
    }
    host.shutdown();
}
