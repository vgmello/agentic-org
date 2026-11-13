//! Socket acceptance oracles also run on the frozen a1b baseline. The baseline fixture
//! omits only the new enable_inbox_hints setup call; test bodies and source inputs are identical.
use super::*;
use crate::test_teardown::teardown_failed;
use org_proto::inbox::InboxChangedData;
use std::time::Instant;

struct Counts {
    total: AtomicU64,
    reads: AtomicUsize,
    failed: AtomicBool,
}
impl Source for Counts {
    fn name(&self) -> &'static str {
        "probe"
    }
    fn read(&self, _: &Query<'_>) -> Result<SourceRead, SourceFailure> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        if self.failed.load(Ordering::SeqCst) {
            return Err(SourceFailure);
        }
        let n = self.total.load(Ordering::SeqCst);
        Ok(SourceRead {
            items: vec![],
            counts: InboxCounts {
                problems: n,
                total: n,
                ..InboxCounts::default()
            },
        })
    }
}
struct Lookup {
    inner: Arc<dyn MemberDirectory + Send + Sync>,
    failed: AtomicBool,
}
impl MemberDirectory for Lookup {
    fn resolve_member(
        &self,
        id: &str,
    ) -> Result<Option<(org_proto::Member, org_proto::Position)>, org_mailbox::StoreError> {
        if self.failed.load(Ordering::SeqCst) {
            return Err(org_mailbox::StoreError::new("injected lookup failure"));
        }
        self.inner.resolve_member(id)
    }
}

struct JoinedClient {
    client: org_client::Client,
    joined: Arc<AtomicUsize>,
}
impl std::ops::Deref for JoinedClient {
    type Target = org_client::Client;
    fn deref(&self) -> &Self::Target {
        &self.client
    }
}
impl Drop for JoinedClient {
    fn drop(&mut self) {
        match self
            .client
            .close_and_join(Instant::now() + Duration::from_secs(10))
        {
            Ok(()) => {
                self.joined.fetch_add(1, Ordering::SeqCst);
            }
            Err(error) => teardown_failed(error),
        }
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    path: std::path::PathBuf,
    counts: Arc<Counts>,
    directory: Arc<Lookup>,
    feed: Arc<FakeFeed>,
    stop: Arc<AtomicBool>,
    clients_joined: Arc<AtomicUsize>,
    server: Option<std::thread::JoinHandle<()>>,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::Builder::new().prefix("ih").tempdir().unwrap();
        let path = dir.path().join("s");
        let listener = bind(&path).unwrap();
        let feed = Arc::new(FakeFeed::default());
        let signal = feed.clone();
        let live = Arc::new(LiveEvents::new(Box::new(move || signal.wake())));
        let (mailbox, inner) = messaging_with(live.clone());
        let directory = Arc::new(Lookup {
            inner,
            failed: AtomicBool::new(false),
        });
        let counts = Arc::new(Counts {
            total: AtomicU64::new(1),
            reads: AtomicUsize::new(0),
            failed: AtomicBool::new(false),
        });
        let mut ports = ports(Box::new(|| panic!("not an inbox source")));
        ports.mailbox = mailbox;
        ports.directory = directory.clone();
        ports.feed = Box::new(feed.clone());
        ports.live = live;
        let mut shared = Shared::new(&test_info(), &path, ports, token());
        shared.register_inbox(
            Hq {
                sources: vec![counts.clone()],
                clock: Clock::new("acceptance".into()),
            },
            Ok(true),
        );
        shared.enable_inbox_hints().unwrap(); // omitted only in the baseline fixture
        let shared = Arc::new(shared);
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let server = std::thread::spawn(move || {
            run(&listener, &shared, &|| stopping.load(Ordering::SeqCst))
        });
        Self {
            _dir: dir,
            path,
            counts,
            directory,
            feed,
            stop,
            clients_joined: Arc::new(AtomicUsize::new(0)),
            server: Some(server),
        }
    }
    fn client(&self, id: &str) -> JoinedClient {
        let client = JoinedClient {
            client: org_client::Client::connect(&self.path).unwrap(),
            joined: Arc::clone(&self.clients_joined),
        };
        client
            .call::<MsgBind>(&MsgBindParams {
                member: Some(id.into()),
                app_token: Some(token()),
            })
            .unwrap();
        client
    }
    fn commit(&self, total: u64) {
        self.counts.total.store(total, Ordering::SeqCst);
        self.feed.push(self.feed.head().unwrap().seq + 1);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.feed.wake();
        self.server.take().unwrap().join().unwrap();
    }
}

fn hint(sub: &org_client::Subscription) -> InboxChangedData {
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        let event = sub
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("expected inbox.changed from the committed change");
        if event.event == "inbox.changed" {
            return serde_json::from_value(event.data).unwrap();
        }
    }
}
fn no_hint(sub: &org_client::Subscription, wait: Duration) {
    let deadline = Instant::now() + wait;
    while let Ok(event) = sub.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        assert_ne!(
            event.event, "inbox.changed",
            "unauthorized or duplicate hint"
        );
    }
}

#[test]
fn acceptance_owner_receives_the_same_hint_denied_to_a_nonowner_without_extra_recount() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let fixture = Fixture::new();
    let owner = fixture.client(OWNER_ID);
    let nonowner = fixture.client(PAT_ID);
    let owner_sub = owner.subscribe().unwrap();
    let other_sub = nonowner.subscribe().unwrap();
    fixture.commit(7);
    assert_eq!(hint(&owner_sub).counts.total, 7);
    no_hint(&other_sub, Duration::from_millis(300));
    assert_eq!(fixture.counts.reads.load(Ordering::SeqCst), 1);
}

#[test]
fn acceptance_lookup_error_delivers_nothing_then_the_same_owner_can_receive_again() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let fixture = Fixture::new();
    let owner = fixture.client(OWNER_ID);
    let sub = owner.subscribe().unwrap();
    fixture.commit(1);
    assert_eq!(hint(&sub).as_of.seq, 1);
    fixture.directory.failed.store(true, Ordering::SeqCst);
    fixture.commit(2);
    no_hint(&sub, Duration::from_millis(350));
    assert_eq!(
        fixture.counts.reads.load(Ordering::SeqCst),
        2,
        "the denied hint was produced"
    );
    fixture.directory.failed.store(false, Ordering::SeqCst);
    fixture.commit(3);
    let recovered = hint(&sub);
    assert_eq!(recovered.as_of.seq, 3);
    assert_eq!(recovered.counts.total, 3);
}

#[test]
fn acceptance_burst_keeps_the_last_counts_and_emits_a_quiet_trailing_hint() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let fixture = Fixture::new();
    let owner = fixture.client(OWNER_ID);
    let sub = owner.subscribe().unwrap();
    fixture.commit(1);
    assert_eq!(hint(&sub).counts.total, 1);
    fixture.commit(2);
    fixture.commit(3);
    let last = hint(&sub);
    assert_eq!(last.counts.total, 3);
    assert_eq!(last.as_of.seq, 3);
    no_hint(&sub, Duration::from_millis(350));
}

#[test]
fn acceptance_failed_source_returns_degraded_with_omitted_counts() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let fixture = Fixture::new();
    let owner = fixture.client(OWNER_ID);
    let sub = owner.subscribe().unwrap();
    fixture.counts.failed.store(true, Ordering::SeqCst);
    fixture.commit(9);
    let data = hint(&sub);
    assert_eq!(data.degraded, ["probe"]);
    assert_eq!(data.counts, InboxCounts::default());
}

#[test]
fn acceptance_emitted_positions_are_monotonic_and_keep_the_instance_epoch() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let fixture = Fixture::new();
    let owner = fixture.client(OWNER_ID);
    let sub = owner.subscribe().unwrap();
    for n in 1..=3 {
        fixture.commit(n);
        let data = hint(&sub);
        assert_eq!(data.as_of.seq, n);
        assert_eq!(data.as_of.epoch, "acceptance");
        assert_eq!(data.counts.total, n);
    }
}

#[test]
fn failed_bind_keeps_client_teardown_inside_the_case_fence() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let fixture = Fixture::new();
    fixture.directory.failed.store(true, Ordering::SeqCst);
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _client = fixture.client(OWNER_ID);
    }));
    assert!(failure.is_err(), "the real bind was not refused");
    assert_eq!(
        fixture.clients_joined.load(Ordering::SeqCst),
        1,
        "bind failure escaped explicit joining before the case fence is released"
    );
    fixture.directory.failed.store(false, Ordering::SeqCst);
    let client = fixture.client(OWNER_ID);
    assert_eq!(fixture.clients_joined.load(Ordering::SeqCst), 1);
    drop(client);
    assert_eq!(
        fixture.clients_joined.load(Ordering::SeqCst),
        2,
        "successful bind did not join exactly once on teardown"
    );
}
