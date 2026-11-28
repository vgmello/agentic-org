use org_proto::directory::MemberPresence;

use super::*;

fn data(member: &str, status: &str) -> MemberPresenceChangedData {
    MemberPresenceChangedData {
        member_id: member.to_owned(),
        presence: MemberPresence::Offline,
        last_seen_at: "2026-11-09T12:00:00.000Z".to_owned(),
        status: Some(status.to_owned()),
        status_expires_at: None,
        current_task_id: None,
    }
}

#[test]
fn the_first_presence_goes_at_once_a_sooner_one_waits_and_the_latest_wins() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let gap = Duration::from_secs(1);
    let mut gate = PresenceGate::new(gap);
    let t0 = Instant::now();
    assert_eq!(gate.offer(data("A", "one"), t0), Some(data("A", "one")));
    // Within the gap: held, and a newer one replaces it.
    assert_eq!(
        gate.offer(data("A", "two"), t0 + Duration::from_millis(100)),
        None
    );
    assert_eq!(
        gate.offer(data("A", "three"), t0 + Duration::from_millis(200)),
        None
    );
    assert!(gate.due(t0 + Duration::from_millis(999)).is_empty());
    assert_eq!(
        gate.next_due(t0 + Duration::from_millis(400)),
        Some(Duration::from_millis(600))
    );
    assert_eq!(gate.due(t0 + gap), [data("A", "three")]);
    // Sent again at t0+gap: the next gap starts there.
    assert_eq!(
        gate.offer(data("A", "four"), t0 + gap + Duration::from_millis(10)),
        None
    );
    assert_eq!(
        gate.next_due(t0 + gap + Duration::from_millis(10)),
        Some(Duration::from_millis(990))
    );
}

#[test]
fn members_are_gated_apart_and_one_after_the_gap_goes_at_once() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let gap = Duration::from_secs(1);
    let mut gate = PresenceGate::new(gap);
    let t0 = Instant::now();
    assert!(gate.offer(data("A", "a"), t0).is_some());
    assert!(
        gate.offer(data("B", "b"), t0).is_some(),
        "another member is not held"
    );
    assert!(
        gate.offer(data("A", "a2"), t0 + gap).is_some(),
        "after the gap it goes now"
    );
    assert_eq!(gate.next_due(t0 + gap), None, "nothing waits");
    // A held event that a later one overtakes after the gap is not sent twice.
    assert!(gate
        .offer(data("B", "b2"), t0 + Duration::from_millis(10))
        .is_none());
    assert!(gate.offer(data("B", "b3"), t0 + gap).is_some());
    assert!(gate.due(t0 + gap * 2).is_empty());
}

#[test]
fn live_org_changed_uses_last_delivered_position_never_advancing_to_head() {
    use std::io::BufReader;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    struct AdvancingHeadFeed {
        calls: AtomicUsize,
        head_seq: AtomicU64,
    }

    impl OrgFeed for AdvancingHeadFeed {
        fn head(&self) -> Result<Position, String> {
            Ok(Position {
                epoch: 1,
                seq: self.head_seq.load(Ordering::SeqCst),
            })
        }

        fn can_resume_from(&self, _: Position) -> Result<bool, String> {
            Ok(true)
        }

        fn changes_after(&self, after: Position, _: usize) -> Result<Vec<OrgChangedData>, String> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            if call == 0 && after.seq < 10 {
                Ok(vec![OrgChangedData {
                    position: Position { epoch: 1, seq: 10 },
                    command_id: "real-cmd-10".into(),
                    changes: Vec::new(),
                    actor: None,
                }])
            } else {
                // Simulate a concurrent org commit advancing `head()` AFTER the log drain finishes.
                self.head_seq.store(99, Ordering::SeqCst);
                Ok(Vec::new())
            }
        }

        fn token(&self) -> u64 {
            0
        }

        fn wait(&self, seen: u64, timeout: Duration) -> u64 {
            std::thread::sleep(timeout.min(Duration::from_millis(5)));
            seen
        }
    }

    let _spawn = crate::transfer::tests::without_process_spawn();
    let feed = Arc::new(AdvancingHeadFeed {
        calls: AtomicUsize::new(0),
        head_seq: AtomicU64::new(10),
    });
    let live = Arc::new(LiveEvents::new(Box::new(|| {})));
    let start_live = live.head();
    live.record_org_changed();

    let (client, server) = UnixStream::pair().unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let out = Arc::new(Mutex::new(server));
    let generation = Arc::new(AtomicU64::new(1));
    let seq = Arc::new(AtomicU64::new(0));

    let feed_t = Arc::clone(&feed);
    let live_t = Arc::clone(&live);
    let out_t = Arc::clone(&out);
    let gen_t = Arc::clone(&generation);
    let seq_t = Arc::clone(&seq);
    let handle = std::thread::spawn(move || {
        let wire = Wire {
            out: &out_t,
            generation: &gen_t,
            mine: 1,
            seq: &seq_t,
            timing: Timing {
                idle: Duration::from_millis(10),
                ..Timing::default()
            },
        };
        push(
            feed_t.as_ref(),
            &live_t,
            &wire,
            Start {
                position: Position { epoch: 1, seq: 9 },
                live: start_live,
            },
        );
    });

    let mut reader = BufReader::new(client);
    let ev1: Event = org_proto::frame::read_message(&mut reader)
        .unwrap()
        .expect("real org.changed event");
    let ev2: Event = org_proto::frame::read_message(&mut reader)
        .unwrap()
        .expect("synthetic live-org org.changed event");

    generation.store(2, Ordering::SeqCst);
    handle.join().unwrap();

    let d1: OrgChangedData = serde_json::from_value(ev1.data).unwrap();
    let d2: OrgChangedData = serde_json::from_value(ev2.data).unwrap();
    assert_eq!(d1.command_id, "real-cmd-10");
    assert_eq!(d1.position, Position { epoch: 1, seq: 10 });
    assert_eq!(d2.command_id, "live-org:1");
    assert_eq!(
        d2.position,
        Position { epoch: 1, seq: 10 },
        "synthetic Live::OrgChanged must use last delivered position, never concurrent head (99)"
    );
    assert_eq!(feed.head().unwrap().seq, 99);
}
