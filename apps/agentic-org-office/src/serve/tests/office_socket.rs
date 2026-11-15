use super::*;
use serde_json::{json, Value};

struct Running {
    stop: Arc<AtomicBool>,
    server: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Running {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let result = self.server.take().unwrap().join();
        if !std::thread::panicking() {
            result.unwrap();
        }
    }
}
fn running(path: &Path, shared: &Arc<Shared>) -> Running {
    let (stop, server) = start(path, shared);
    Running {
        stop,
        server: Some(server),
    }
}
fn auth(grants: bool) -> LaunchAuth {
    if grants {
        LaunchAuth::Grants {
            _key: org_proto::GrantKey::from_bytes([7; 32]),
        }
    } else {
        token().into()
    }
}
pub(super) fn principal_values() -> [Value; 3] {
    let vectors: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../crates/org-proto/test-vectors/v2/proto-grant-mac.json"
    )))
    .unwrap();
    let grant = vectors
        .get("cases")
        .and_then(Value::as_array)
        .unwrap()
        .first()
        .unwrap()
        .get("grant")
        .unwrap()
        .clone();
    let valid = json!({"grant":grant});
    serde_json::from_value::<org_proto::Principal>(valid.clone()).unwrap();
    [valid, Value::Null, json!({"bad":"principal"})]
}
pub(super) fn with_principal(id: u64, method: &str, params: Value, principal: Value) -> Request {
    // Use the actual envelope decoder, including its distinction between absent and null.
    serde_json::from_value(json!({"id":id,"method":method,"params":params,"principal":principal}))
        .unwrap()
}

#[test]
fn operation_lookup_refuses_agents_and_anonymous_without_calling_owner_port_in_both_modes() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    for grants in [false, true] {
        for stored_principal in [Some("owner"), None] {
            let calls = Arc::new(AtomicUsize::new(0));
            let observed = Arc::clone(&calls);
            let mut ports = ports(Box::new(|| Err(SnapshotFailure::Storage)));
            let outcome = org_proto::OperationOutcome::Committed {
                result: json!({"principal":stored_principal}),
            };
            let stored = outcome.clone();
            ports.operation_get = Box::new(move |_| {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(Ok(stored.clone()))
            });
            let shared = Shared::new(&test_info(), Path::new("/unused"), ports, auth(grants));
            for (mut binding, allowed) in [
                (Binding::Anonymous, false),
                (
                    Binding::Member {
                        id: "agent".into(),
                        with_app_token: false,
                    },
                    false,
                ),
                (Binding::App, true),
                (
                    Binding::Member {
                        id: "owner".into(),
                        with_app_token: true,
                    },
                    true,
                ),
                (
                    Binding::Member {
                        id: "pat".into(),
                        with_app_token: true,
                    },
                    true,
                ),
            ] {
                let before = calls.load(Ordering::SeqCst);
                for (params, expected) in [
                    (
                        json!({"kind":"org","id":"bad"}),
                        Some(ErrorCode::InvalidRequest),
                    ),
                    (
                        json!({"kind":"work","id":"01ARZ3NDEKTSV4RRFFQ69G5FAV"}),
                        Some(ErrorCode::Unavailable),
                    ),
                    (
                        json!({"kind":"org","id":"01ARZ3NDEKTSV4RRFFQ69G5FAV"}),
                        None,
                    ),
                ] {
                    let request = Request {
                        id: 42,
                        method: "operation.get".into(),
                        params,
                        principal: None,
                    };
                    let response = answer(&request, &shared, &mut binding);
                    assert_eq!(response.id, 42);
                    if let Some(code) = expected {
                        assert_eq!(code_of(response.outcome), code);
                        assert_eq!(calls.load(Ordering::SeqCst), before);
                    } else if allowed {
                        assert_eq!(
                            response.into_result::<org_proto::OperationGet>().unwrap(),
                            outcome
                        );
                        assert_eq!(calls.load(Ordering::SeqCst), before + 1);
                    } else {
                        assert_eq!(code_of(response.outcome), ErrorCode::NotFound);
                        assert_eq!(calls.load(Ordering::SeqCst), before);
                    }
                }
            }
        }
    }
}

#[test]
fn principal_envelopes_are_refused_before_each_special_dispatch_in_both_modes() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    for grants in [false, true] {
        let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let mut ports = ports(Box::new(|| panic!("principal must not read snapshots")));
        ports.submit = Box::new(|_| panic!("principal must not submit or store a command"));
        let shared = Arc::new(Shared::new(&test_info(), &path, ports, auth(grants)));
        let _server = running(&path, &shared);
        let mut client = Client::connect(&path);
        // Missing principal continues through normal dispatch.
        assert!(matches!(
            client.call::<Hello>(&Default::default()),
            org_proto::Outcome::Result(_)
        ));
        for principal in principal_values() {
            for method in ["hello", "msg.bind", "msg.wait", "subscribe", "team.create"] {
                let outcome =
                    client.ask(|id| with_principal(id, method, json!({}), principal.clone()));
                assert_eq!(
                    code_of(outcome),
                    ErrorCode::NotAuthorized,
                    "{grants} {method}"
                );
            }
        }
        assert!(matches!(
            client.call::<Hello>(&Default::default()),
            org_proto::Outcome::Result(_)
        ));
    }
}

#[test]
fn refused_principal_bind_preserves_binding_held_calls_and_subscription_generation() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
    let path = dir.path().join("s.sock");
    let feed = Arc::new(FakeFeed::default());
    let mut ports = ports(Box::new(|| Err(SnapshotFailure::Storage)));
    ports.submit = Box::new(|_| panic!("principal must not submit or store a command"));
    ports.feed = Box::new(Arc::clone(&feed));
    let mailbox = Arc::clone(&ports.mailbox);
    let shared = Arc::new(Shared::new(&test_info(), &path, ports, token()));
    let _server = running(&path, &shared);
    let mut client = Client::connect(&path);
    assert!(matches!(
        client.bind(Some("pat"), Some(token())),
        org_proto::Outcome::Result(_)
    ));
    let _wait = client.call_later::<MsgWait>(&wait_params());
    until("wait is held", || mailbox.held_waits() == 1);
    assert!(matches!(
        client.subscribe(None),
        org_proto::Outcome::Result(_)
    ));
    for (index, principal) in principal_values().into_iter().enumerate() {
        assert_eq!(
            code_of(client.ask(|id| with_principal(
                id,
                "msg.bind",
                json!({"member":"unknown"}),
                principal.clone()
            ))),
            ErrorCode::NotAuthorized
        );
        assert_eq!(mailbox.held_waits(), 1);
        for (method, params) in [
            ("hello", json!({"api":{"min":999,"max":999}})),
            ("msg.wait", json!({"timeout_ms":0})),
            ("subscribe", json!({})),
        ] {
            assert_eq!(
                code_of(client.ask(|id| with_principal(id, method, params, principal.clone()))),
                ErrorCode::NotAuthorized
            );
            assert_eq!(mailbox.held_waits(), 1);
        }
        assert!(matches!(
            client.call::<MsgInbox>(&inbox_params()),
            org_proto::Outcome::Result(_)
        ));
        feed.push(index as u64 + 2);
        let event = client.next_message();
        assert_eq!(event["event"], "org.changed");
        assert_eq!(event["data"]["position"]["seq"], index as u64 + 2);
        let params = serde_json::to_value(TeamCreateParams {
            command_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
            team_id: "T1".into(),
            name: "Platform".into(),
            owner_team_id: None,
        })
        .unwrap();
        assert_eq!(
            code_of(client.ask(|id| with_principal(id, "team.create", params, principal))),
            ErrorCode::NotAuthorized
        );
    }
}

#[test]
fn test_client_setup_accepts_a_live_peer() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let (stream, _peer) = UnixStream::pair().unwrap();
    let client = Client::from_stream(stream);
    assert_eq!(
        client.stream.read_timeout().unwrap(),
        Some(Duration::from_secs(10))
    );
}

#[test]
#[should_panic(expected = "expected peer EOF")]
fn test_client_setup_rejects_invalid_timeout_on_a_live_peer() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let (stream, _peer) = UnixStream::pair().unwrap();
    Client::with_timeout_result(stream, Err(std::io::Error::from_raw_os_error(libc::EINVAL)));
}

#[test]
#[should_panic(expected = "expected peer EOF")]
fn test_client_setup_rejects_invalid_timeout_with_buffered_data() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let (stream, mut peer) = UnixStream::pair().unwrap();
    std::io::Write::write_all(&mut peer, b"x").unwrap();
    Client::with_timeout_result(stream, Err(std::io::Error::from_raw_os_error(libc::EINVAL)));
}

#[test]
fn test_client_setup_accepts_invalid_timeout_only_after_peer_eof() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let (stream, peer) = UnixStream::pair().unwrap();
    drop(peer);
    let mut client =
        Client::with_timeout_result(stream, Err(std::io::Error::from_raw_os_error(libc::EINVAL)));
    assert!(client.connection_ended());
}

#[test]
fn test_client_setup_rejects_other_timeout_errors_after_peer_eof() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    for errno in [Some(libc::ECONNRESET), Some(libc::EACCES), None] {
        let make_error = || match errno {
            Some(errno) => std::io::Error::from_raw_os_error(errno),
            None => std::io::Error::from(std::io::ErrorKind::InvalidInput),
        };
        let (stream, peer) = UnixStream::pair().unwrap();
        drop(peer);
        let expected = format!("set test client read timeout: {}", make_error());
        let panic =
            std::panic::catch_unwind(|| Client::with_timeout_result(stream, Err(make_error())))
                .err()
                .expect("unexpected timeout error must fail even after EOF");
        let message = panic.downcast_ref::<String>().unwrap();
        assert!(message.contains(&expected), "{message}");
    }
}

#[test]
fn test_client_setup_accepts_an_already_closed_peer() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let (stream, peer) = UnixStream::pair().unwrap();
    drop(peer);
    let mut client = Client::from_stream(stream);
    assert!(client.connection_ended());
}

#[test]
fn accepted_office_socket_serves_only_matching_peer_uid_in_both_modes() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let uid = org_socket::effective_uid();
    for grants in [false, true] {
        for expected in [uid, uid.wrapping_add(1)] {
            let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
            let path = dir.path().join("s.sock");
            let shared = Arc::new(Shared::new(
                &test_info(),
                &path,
                ports(Box::new(|| Err(SnapshotFailure::Storage))),
                auth(grants),
            ));
            let listener = bind(&path).unwrap();
            let stop = Arc::new(AtomicBool::new(false));
            let flag = Arc::clone(&stop);
            let server = std::thread::spawn(move || {
                run_for_uid(
                    &listener,
                    &shared,
                    &|| flag.load(Ordering::SeqCst),
                    expected,
                )
            });
            let _server = Running {
                stop,
                server: Some(server),
            };
            let mut client = Client::connect(&path);
            if expected == uid {
                assert!(matches!(
                    client.call::<Hello>(&Default::default()),
                    org_proto::Outcome::Result(_)
                ));
            } else {
                // A refusal may close before or just after this write; neither sends a response.
                let request = Request::new::<Hello>(1, &Default::default()).unwrap();
                let _ = frame::write_message(&mut client.stream, &request);
                assert!(client.connection_ended());
            }
        }
    }
}

#[test]
fn principal_refusal_precedes_even_the_live_member_lookup() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    struct Directory {
        inner: Arc<org_mailbox::testkit::InMemoryDirectory>,
        forbidden: Arc<AtomicBool>,
    }
    impl MemberDirectory for Directory {
        fn resolve_member(
            &self,
            handle: &str,
        ) -> Result<Option<(org_proto::Member, org_proto::org::Position)>, org_mailbox::StoreError>
        {
            assert!(
                !self.forbidden.load(Ordering::SeqCst),
                "principal must not query the directory"
            );
            self.inner.resolve_member(handle)
        }
    }
    for grants in [false, true] {
        let (_, directory) = messaging();
        let forbidden = Arc::new(AtomicBool::new(false));
        let mut ports = ports(Box::new(|| Err(SnapshotFailure::Storage)));
        ports.directory = Arc::new(Directory {
            inner: directory,
            forbidden: Arc::clone(&forbidden),
        });
        let shared = Arc::new(Shared::new(
            &test_info(),
            Path::new("/unused"),
            ports,
            auth(grants),
        ));
        let (stream, peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        // Exercise the real loop without a watcher that independently queries this directory.
        let server = std::thread::spawn(move || serve_connection(stream, &shared, 1, &flag));
        let _server = Running {
            stop,
            server: Some(server),
        };
        let mut client = Client {
            reader: BufReader::new(peer.try_clone().unwrap()),
            stream: peer,
            next: 0,
        };
        assert!(matches!(
            client.bind(
                Some(if grants { "ada" } else { "pat" }),
                if grants { None } else { Some(token()) },
            ),
            org_proto::Outcome::Result(_)
        ));
        forbidden.store(true, Ordering::SeqCst);
        for principal in principal_values() {
            for method in ["hello", "msg.bind", "msg.wait", "subscribe", "team.create"] {
                assert_eq!(
                    code_of(client.ask(|id| with_principal(
                        id,
                        method,
                        json!({}),
                        principal.clone()
                    ))),
                    ErrorCode::NotAuthorized
                );
            }
        }
    }
}

#[test]
fn failed_principal_refusal_write_ends_before_polling_another_request() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    for grants in [false, true] {
        let (stream, mut peer) = UnixStream::pair().unwrap();
        frame::write_message(
            &mut peer,
            &with_principal(1, "hello", json!({}), Value::Null),
        )
        .unwrap();
        // macOS accepts writes after SHUT_RD; a fully closed peer gives a real EPIPE.
        drop(peer);
        let shared = Shared::new(
            &test_info(),
            Path::new("/unused"),
            ports(Box::new(|| Err(SnapshotFailure::Storage))),
            auth(grants),
        );
        let polls = AtomicUsize::new(0);
        serve_connection_with_poll(
            stream,
            &shared,
            1,
            &AtomicBool::new(false),
            Duration::from_millis(1),
            |fd, tick| {
                assert_eq!(
                    polls.fetch_add(1, Ordering::SeqCst),
                    0,
                    "failed refusal must exit before another read"
                );
                org_socket::poll_readable(fd, tick)
            },
        );
        assert_eq!(polls.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn revocation_precedes_principal_refusal_and_closes_the_office_socket() {
    for grants in [false, true] {
        for principal in std::iter::once(None).chain(principal_values().into_iter().map(Some)) {
            let (stream, mut peer) = UnixStream::pair().unwrap();
            peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
            let shared = Shared::new(
                &test_info(),
                Path::new("/unused-test-socket"),
                ports(Box::new(|| Err(SnapshotFailure::Storage))),
                auth(grants),
            );
            frame::write_message(
                &mut peer,
                &Request::new::<MsgBind>(
                    1,
                    &MsgBindParams {
                        member: Some("ada".into()),
                        app_token: None,
                    },
                )
                .unwrap(),
            )
            .unwrap();
            let mut polls = 0;
            serve_connection_with_poll(
                stream,
                &shared,
                1,
                &AtomicBool::new(false),
                org_socket::READ_TICK,
                |fd, _| {
                    polls += 1;
                    assert!(polls <= 3, "revoked connection did not stop");
                    if polls == 3 {
                        // Let an incorrectly continued loop finish, so the response assertion
                        // below identifies the wrong refusal rather than timing out.
                        return Ok(false);
                    }
                    if polls == 2 {
                        let control = Arc::clone(&shared.bound.lock().unwrap().get(&1).unwrap().1);
                        control.revoked.store(true, Ordering::SeqCst);
                        let mut call = Request::new::<MsgInbox>(2, &inbox_params()).unwrap();
                        if let Some(value) = principal.clone() {
                            let method = call.method.clone();
                            call = with_principal(2, &method, call.params, value);
                        }
                        frame::write_message(&mut peer, &call).unwrap();
                    }
                    org_socket::poll_readable(fd, Duration::from_millis(1))
                },
            );
            let mut responses = BufReader::new(peer);
            let bound = frame::read_message::<Response>(&mut responses)
                .unwrap()
                .unwrap();
            assert_eq!(bound.id, 1);
            assert!(matches!(bound.outcome, org_proto::Outcome::Result(_)));
            let refused = frame::read_message::<Response>(&mut responses)
                .unwrap()
                .unwrap();
            assert_eq!(refused.id, 2);
            assert_eq!(
                code_of(refused.outcome),
                ErrorCode::NotBound,
                "established revocation must win over a principal (grants={grants})"
            );
            assert!(frame::read_message::<Response>(&mut responses)
                .unwrap()
                .is_none());
            assert_eq!(polls, 2, "refusal must close without polling another frame");
            assert!(shared.bound.lock().unwrap().is_empty());
        }
    }
}
