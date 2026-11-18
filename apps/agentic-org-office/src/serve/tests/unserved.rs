//! An office with no mailbox and no organization feed, as the worker composes it until B6
//! rebuilds messaging and live events on the office store: the messaging calls and `subscribe`
//! answer `unsupported`, before anything else is said about them, and `msg.bind` still binds
//! through the directory.
use super::*;

/// A server over the fixture members with no mailbox and no feed.
fn unserved_server(path: &Path) -> (Arc<AtomicBool>, std::thread::JoinHandle<()>) {
    let (_mailbox, directory) = messaging();
    let mut ports = ports(Box::new(|| Err(SnapshotFailure::Storage)));
    ports.mailbox = None;
    ports.feed = None;
    ports.directory = directory;
    let shared = Arc::new(Shared::new(&test_info(), path, ports, launch_key()));
    start(path, &shared)
}

fn unsupported(outcome: org_proto::Outcome) -> bool {
    code_of(outcome) == ErrorCode::Unsupported
}

#[test]
fn without_a_mailbox_every_messaging_call_is_unsupported_and_an_agent_still_binds() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
    let path = dir.path().join("s.sock");
    let (stop, server) = unserved_server(&path);

    // Unbound: `unsupported`, not `not_bound`. The call is not served at all.
    let mut anonymous = Client::connect(&path);
    assert!(unsupported(
        anonymous.call::<MsgSend>(&send_params(1, "remy", "hi"))
    ));
    assert!(unsupported(anonymous.call::<MsgInbox>(&inbox_params())));
    assert!(unsupported(
        anonymous.call::<MsgRead>(&MsgReadParams { id: None })
    ));
    assert!(unsupported(anonymous.call::<MsgWait>(&wait_params())));

    // `msg.bind` still binds through the directory.
    let mut ada = Client::connect(&path);
    let org_proto::Outcome::Result(bound) = ada.bind(Some("ada"), None) else {
        panic!("ada should bind without a mailbox");
    };
    assert_eq!(bound["member_id"], ADA_ID);
    assert!(unsupported(
        ada.call::<MsgSend>(&send_params(2, "remy", "hi"))
    ));
    assert!(unsupported(ada.call::<MsgInbox>(&inbox_params())));
    assert!(unsupported(
        ada.call::<MsgRead>(&MsgReadParams { id: None })
    ));
    // A wait is answered at once, not held.
    assert!(unsupported(ada.call::<MsgWait>(&wait_params())));

    // The same for a person acting through a grant.
    let mut pat = Client::grant_bound(&path, "pat");
    assert!(unsupported(
        pat.call::<MsgSend>(&send_params(3, "ada", "hi"))
    ));
    assert!(unsupported(pat.call::<MsgInbox>(&inbox_params())));

    stop.store(true, Ordering::SeqCst);
    server.join().unwrap();
}

#[test]
fn without_a_feed_subscribe_is_unsupported_for_every_caller() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
    let path = dir.path().join("s.sock");
    let (stop, server) = unserved_server(&path);

    let mut anonymous = Client::connect(&path);
    assert!(unsupported(anonymous.subscribe(None)));
    let mut pat = Client::grant_bound(&path, "pat");
    assert!(unsupported(pat.subscribe(None)));
    assert!(unsupported(pat.subscribe(Some(1))));
    // The connection is not left subscribed: other calls are still answered on it.
    assert!(unsupported(pat.call::<MsgInbox>(&inbox_params())));

    stop.store(true, Ordering::SeqCst);
    server.join().unwrap();
}

/// With no feed there is no organization change to wait for: the member watcher looks on every
/// idle tick, so a connection bound as a member that is gone from the directory is still
/// revoked, without the connection sending anything.
#[test]
fn without_a_feed_the_watcher_still_revokes_a_member_gone_from_the_directory() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    use org_mailbox::testkit::fixture::POSITION;
    let dir = tempfile::Builder::new().prefix("sv").tempdir().unwrap();
    let path = dir.path().join("s.sock");
    let (_mailbox, directory) = messaging();
    let mut ports = ports(Box::new(|| Err(SnapshotFailure::Storage)));
    ports.mailbox = None;
    ports.feed = None;
    ports.directory = directory.clone();
    let timing = Timing {
        idle: Duration::from_millis(50),
        ..Timing::default()
    };
    let shared =
        Arc::new(Shared::new(&test_info(), &path, ports, launch_key()).with_timing(timing));
    let (stop, server) = start(&path, &shared);

    let ada = Client::bound_as(&path, "ada");
    let remy = Client::bound_as(&path, "remy");
    let revoked = |member: &str| {
        shared
            .bound
            .lock()
            .unwrap()
            .values()
            .any(|(bound, control)| bound == member && control.revoked.load(Ordering::SeqCst))
    };
    // Nothing changes: a tick that looks finds both still bindable.
    std::thread::sleep(Duration::from_millis(200));
    assert!(!revoked(ADA_ID) && !revoked(REMY_ID));
    directory.remove(ADA_ID, POSITION);
    let started = std::time::Instant::now();
    // No feed moved, and the idle connection sends nothing: a tick looks and revokes it.
    until("ada's idle connection is revoked", || revoked(ADA_ID));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    // Another member's connection is left alone.
    assert!(!revoked(REMY_ID));
    drop(ada);
    drop(remy);

    stop.store(true, Ordering::SeqCst);
    server.join().unwrap();
}
