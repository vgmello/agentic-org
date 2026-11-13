use super::*;
use crate::inbox::{Clock, Query, SourceFailure, SourceRead};
use org_proto::{inbox::InboxCounts, org::OrgChangedData};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    Arc,
};

#[derive(Default)]
struct Feed(
    Mutex<Vec<OrgChangedData>>,
    AtomicBool,
    AtomicBool,
    AtomicBool,
    AtomicU64,
);
impl Feed {
    fn commit(&self, seq: u64) {
        self.0.lock().unwrap().push(OrgChangedData {
            position: Position { epoch: 1, seq },
            command_id: format!("c{seq}"),
            changes: vec![],
            actor: None,
        });
    }
}
impl OrgFeed for Feed {
    fn head(&self) -> Result<Position, String> {
        if self.1.load(Ordering::SeqCst) {
            return Err("head unavailable".into());
        }
        let limit = self.4.load(Ordering::SeqCst);
        if limit > 0 {
            return Ok(Position {
                epoch: 1,
                seq: limit,
            });
        }
        Ok(self
            .0
            .lock()
            .unwrap()
            .last()
            .map_or(Position { epoch: 1, seq: 1 }, |r| r.position))
    }
    fn changes_after(&self, after: Position, limit: usize) -> Result<Vec<OrgChangedData>, String> {
        if self.2.load(Ordering::SeqCst) {
            return Err("rows unavailable".into());
        }
        if self.3.load(Ordering::SeqCst) {
            return Ok(vec![]);
        }
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.position > after)
            .take(limit)
            .cloned()
            .collect())
    }
    fn can_resume_from(&self, _: Position) -> Result<bool, String> {
        Ok(true)
    }
    fn token(&self) -> u64 {
        0
    }
    fn wait(&self, _: u64, _: Duration) -> u64 {
        0
    }
}

struct Counts {
    total: AtomicU64,
    reads: AtomicUsize,
    fail: AtomicBool,
}
impl Source for Counts {
    fn name(&self) -> &'static str {
        "probe"
    }
    fn read(&self, query: &Query<'_>) -> Result<SourceRead, SourceFailure> {
        assert_eq!(query.limit, 0, "hint recount must not request items");
        self.reads.fetch_add(1, Ordering::SeqCst);
        if self.fail.load(Ordering::SeqCst) {
            return Err(SourceFailure);
        }
        Ok(SourceRead {
            items: vec![],
            counts: InboxCounts {
                total: self.total.load(Ordering::SeqCst),
                ..InboxCounts::default()
            },
        })
    }
}

fn fixture() -> (Feed, Arc<Counts>, Hq, LiveEvents, Hint) {
    let feed = Feed::default();
    let counts = Arc::new(Counts {
        total: AtomicU64::new(7),
        reads: AtomicUsize::new(0),
        fail: AtomicBool::new(false),
    });
    let hq = Hq {
        sources: vec![counts.clone()],
        clock: Clock::new("instance".into()),
    };
    let live = LiveEvents::new(Box::new(|| {}));
    let hint = Hint::new(feed.head().unwrap(), live.mailbox_head());
    (feed, counts, hq, live, hint)
}

fn events(live: &LiveEvents) -> Vec<InboxChangedData> {
    live.since(0, live.head().messages)
        .events
        .into_iter()
        .map(|event| match event {
            Live::InboxChanged(data) => data,
            _ => panic!("unexpected live event"),
        })
        .collect()
}

#[test]
fn five_committed_rows_in_one_wake_advance_five_with_exactly_one_recount() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let (feed, counts, hq, live, hint) = fixture();
    for seq in 2..=6 {
        feed.commit(seq);
    }
    let now = Instant::now();
    hint.poll(&hq, &feed, &live, || now);
    assert_eq!(counts.reads.load(Ordering::SeqCst), 1);
    let data = events(&live);
    assert_eq!(data.len(), 1);
    assert_eq!(data[0].as_of.seq, 5);
    assert_eq!(data[0].as_of.epoch, "instance");
    assert_eq!(
        data[0].counts.total, 7,
        "reuse source total, not item length"
    );
    for _subscriber in 0..8 {
        assert_eq!(events(&live), data);
    }
    hint.poll(&hq, &feed, &live, || now + GAP);
    assert_eq!(
        counts.reads.load(Ordering::SeqCst),
        1,
        "quiet wake and subscribers do not recount"
    );
    assert_eq!(
        hq.clock.as_of().seq,
        5,
        "wake token without committed rows changes nothing"
    );
}

#[test]
fn burst_is_throttled_and_a_quiet_trailing_wake_emits_final_counts() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let (feed, counts, hq, live, hint) = fixture();
    let now = Instant::now();
    feed.commit(2);
    hint.poll(&hq, &feed, &live, || now);
    counts.total.store(8, Ordering::SeqCst);
    feed.commit(3);
    assert_eq!(
        hint.poll(&hq, &feed, &live, || now + Duration::from_millis(10)),
        Duration::from_millis(240)
    );
    counts.total.store(9, Ordering::SeqCst);
    feed.commit(4);
    hint.poll(&hq, &feed, &live, || now + Duration::from_millis(20));
    assert_eq!(events(&live).len(), 1);
    hint.poll(&hq, &feed, &live, || now + GAP);
    let data = events(&live);
    assert_eq!(data.len(), 2);
    assert_eq!(data[1].counts.total, 9);
    assert_eq!(data[1].as_of.seq, 3);
    assert_eq!(
        counts.reads.load(Ordering::SeqCst),
        3,
        "trailing emission reuses the last recount"
    );
    hint.poll(&hq, &feed, &live, || now + GAP * 2);
    assert_eq!(events(&live).len(), 2);
}

#[test]
fn failed_recount_source_is_omitted_and_named_in_the_hint() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let (feed, counts, hq, live, hint) = fixture();
    counts.fail.store(true, Ordering::SeqCst);
    feed.commit(2);
    hint.poll(&hq, &feed, &live, Instant::now);
    let data = events(&live);
    assert_eq!(data.len(), 1);
    assert_eq!(data[0].counts, InboxCounts::default());
    assert_eq!(data[0].degraded, ["probe"]);
    assert_eq!(data[0].as_of.seq, 1);
    assert_eq!(counts.reads.load(Ordering::SeqCst), 1);
}

#[test]
fn hint_samples_as_of_before_a_source_read_that_observes_a_later_change() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    struct DuringRead(std::sync::OnceLock<std::sync::Weak<Hq>>);
    impl Source for DuringRead {
        fn name(&self) -> &'static str {
            "during-read"
        }
        fn read(&self, _: &Query<'_>) -> Result<SourceRead, SourceFailure> {
            self.0.get().unwrap().upgrade().unwrap().clock.changed();
            Ok(SourceRead {
                items: vec![],
                counts: InboxCounts::default(),
            })
        }
    }
    let source = Arc::new(DuringRead(std::sync::OnceLock::new()));
    let hq = Arc::new(Hq {
        sources: vec![source.clone()],
        clock: Clock::new("instance".into()),
    });
    assert!(source.0.set(Arc::downgrade(&hq)).is_ok());
    let live = LiveEvents::new(Box::new(|| {}));
    let feed = Feed::default();
    let hint = Hint::new(feed.head().unwrap(), 0);
    feed.commit(2);
    hint.poll(&hq, &feed, &live, Instant::now);
    assert_eq!(events(&live)[0].as_of.seq, 1);
    assert_eq!(
        hq.clock.as_of().seq,
        2,
        "the mid-read change must make this result stale"
    );
}

#[test]
fn feed_errors_or_empty_pages_preserve_the_cursor_and_recovery_recounts_once() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let (feed, counts, hq, live, hint) = fixture();
    feed.commit(2);
    feed.1.store(true, Ordering::SeqCst);
    hint.poll(&hq, &feed, &live, Instant::now);
    feed.1.store(false, Ordering::SeqCst);
    feed.2.store(true, Ordering::SeqCst);
    hint.poll(&hq, &feed, &live, Instant::now);
    feed.2.store(false, Ordering::SeqCst);
    feed.3.store(true, Ordering::SeqCst);
    hint.poll(&hq, &feed, &live, Instant::now);
    assert!(events(&live).is_empty());
    assert_eq!(counts.reads.load(Ordering::SeqCst), 0);
    feed.3.store(false, Ordering::SeqCst);
    hint.poll(&hq, &feed, &live, Instant::now);
    assert_eq!(events(&live)[0].as_of.seq, 1);
    assert_eq!(counts.reads.load(Ordering::SeqCst), 1);
}

#[test]
fn head_snapshot_bounds_concurrent_commits_and_multiple_pages_share_one_recount() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let (feed, counts, hq, live, hint) = fixture();
    for seq in 2..=106 {
        feed.commit(seq);
    }
    feed.4.store(105, Ordering::SeqCst);
    let now = Instant::now();
    hint.poll(&hq, &feed, &live, || now);
    assert_eq!(events(&live)[0].as_of.seq, 104);
    assert_eq!(counts.reads.load(Ordering::SeqCst), 1);
    feed.4.store(106, Ordering::SeqCst);
    hint.poll(&hq, &feed, &live, || now + GAP);
    assert_eq!(events(&live)[1].as_of.seq, 105);
    assert_eq!(counts.reads.load(Ordering::SeqCst), 2);
}

mod mailbox;
