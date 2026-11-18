use super::*;
use crate::testkit::{id, identity, DiskOp, FakeCustody, Faults, Rng, Target};

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    faults: Arc<Faults>,
    custody: FakeCustody,
    identity: OfficeIdentity,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap().join("durable");
    Fixture {
        _dir: dir,
        root,
        faults: Faults::new(),
        custody: FakeCustody::new(),
        identity: identity(),
    }
}

impl Fixture {
    fn create(&self) -> Journal {
        self.create_from(&AnchorFacts::default())
    }

    fn create_from(&self, facts: &AnchorFacts) -> Journal {
        Journal::create_on(self.faults.disk(), &self.root, &self.identity, facts).unwrap()
    }

    fn open(&self) -> Result<(Journal, Recovered), JournalError> {
        Journal::open_on(
            self.faults.disk(),
            &self.root,
            &self.identity,
            &self.custody,
        )
    }

    fn segment(&self, first: u64) -> PathBuf {
        journal_dir(&self.root, &self.identity)
            .join("segments")
            .join(segment_name(first))
    }

    /// Appends `count` entries, one batch each, and flushes.
    fn fill(&self, journal: &Journal, count: u64) -> Vec<Arc<Entry>> {
        let mut entries = Vec::new();
        for _ in 0..count {
            let entry = next(&journal.head(), &self.identity);
            journal.append(std::slice::from_ref(&entry)).unwrap();
            entries.push(entry);
        }
        journal.flush().unwrap();
        entries
    }
}

/// A mixed entry after `point`: a put, a receipt, sometimes a removal and positions.
fn next(point: &JournalPoint, identity: &OfficeIdentity) -> Arc<Entry> {
    let n = point.ordinal + 1;
    let mut after = point.high_waters;
    if n.is_multiple_of(2) {
        after.work += 2;
        after.organization += 1;
    }
    let mut files = vec![
        FileDraft {
            path: DurablePath::parse(&format!("state/teams/{}/team.yaml", id(n))).unwrap(),
            op: FileOp::Put,
            bytes: Some(format!("name: \"Team {n}\"\n").into_bytes().into()),
            previous_sha256: None,
        },
        FileDraft {
            path: DurablePath::parse(&format!("recovery/commands/{}.json", id(n))).unwrap(),
            op: FileOp::Create,
            bytes: Some(b"{\"reply\":\"ok\"}".to_vec().into()),
            previous_sha256: None,
        },
    ];
    if n.is_multiple_of(3) {
        files.push(FileDraft {
            path: DurablePath::parse("state/gone.yaml").unwrap(),
            op: FileOp::Remove,
            bytes: None,
            previous_sha256: Some(Sha256::of(b"gone")),
        });
    }
    let draft = EntryDraft {
        ordinal: n,
        previous_sha256: point.sha256,
        office_id: identity.office_id.clone(),
        journal_incarnation: identity.journal_incarnation.clone(),
        at: Timestamp(1_000 + n),
        ack: if n.is_multiple_of(2) {
            Ack::Durable
        } else {
            Ack::Committed
        },
        feed: FeedPosition(point.feed.0 + 1),
        operation: (!n.is_multiple_of(4)).then(|| OperationRecord {
            id: id(n),
            kind: "probe.put".into(),
            actor: Actor::Employee(id(900)),
            fingerprint: Sha256::of(&n.to_be_bytes()),
            outcome: if n.is_multiple_of(5) {
                Outcome::Rejected
            } else {
                Outcome::Completed
            },
        }),
        positions: positions_between(&point.high_waters, &after),
        files,
    };
    Arc::new(Entry::frame(draft).unwrap())
}

fn head_of(entry: &Entry) -> JournalHead {
    JournalHead {
        ordinal: entry.ordinal(),
        sha256: entry.sha256,
        high_waters: HighWaters::default(),
    }
}

/// The byte offset after each entry in one segment.
fn ends(entries: &[Arc<Entry>]) -> Vec<usize> {
    let mut at = 0;
    let lengths = entries.iter().map(|e| e.stored().len());
    lengths
        .map(|length| {
            at += length;
            at
        })
        .collect()
}

#[test]
fn a_reopened_journal_verifies_the_chain_and_recovers_every_position_from_itself() {
    let f = fixture();
    let journal = f.create();
    assert_eq!(journal.head().ordinal, 0);
    let entries = f.fill(&journal, 6);
    let head = journal.head();
    assert_eq!(head.ordinal, 6);
    assert_eq!(head.feed, FeedPosition(6));
    assert_eq!(
        (head.high_waters.work, head.high_waters.organization),
        (6, 3)
    );
    drop(journal);

    let (journal, recovered) = f.open().unwrap();
    assert_eq!(recovered.entries, entries);
    assert_eq!(recovered.truncated, None);
    assert_eq!(recovered.confirmed, None);
    assert_eq!(journal.head(), head);
    assert_eq!(journal.flushed(), 6);
    assert_eq!(journal.base().ordinal, 0);
    // The first entry chains to the anchor's bytes.
    let anchor = fs::read(journal.dir().join("anchor.json")).unwrap();
    assert_eq!(entries[0].header.previous_sha256, Sha256::of(&anchor));
    assert_eq!(entries[2].payload(2), b"");
    assert_eq!(entries[0].payload(1), b"{\"reply\":\"ok\"}");
    let versions: Vec<_> = entries[2].versions().collect();
    assert_eq!(versions[2].bytes, None);
    assert_eq!(
        versions[0].sha256,
        Some(Sha256::of(versions[0].bytes.unwrap()))
    );
}

#[test]
fn a_journal_that_is_not_this_writers_or_not_whole_refuses_to_open() {
    let f = fixture();
    let journal = f.create();
    assert_eq!(
        Journal::create_on(
            f.faults.disk(),
            &f.root,
            &f.identity,
            &AnchorFacts::default()
        )
        .err(),
        Some(JournalError::Exists)
    );
    let entries = f.fill(&journal, 5);
    drop(journal);

    // A fork: the host confirmed another entry at that ordinal.
    let mut other = head_of(&entries[1]);
    other.sha256 = Sha256::of(b"another history");
    f.custody.hold(&f.identity, other);
    assert_eq!(f.open().err(), Some(JournalError::Fork { ordinal: 2 }));

    // Behind the confirmed head.
    let mut ahead = head_of(&entries[4]);
    ahead.ordinal = 9;
    f.custody.hold(&f.identity, ahead);
    assert_eq!(
        f.open().err(),
        Some(JournalError::Behind {
            journal: 5,
            custody: 9
        })
    );
    f.custody.hold(&f.identity, head_of(&entries[4]));
    assert!(f.open().is_ok());

    // The host channel is closed: custody is asked at opening and fails closed.
    f.custody.close();
    assert!(matches!(f.open().err(), Some(JournalError::Custody(_))));
    f.custody.reopen();

    // Another writer epoch.
    let mut epoch = f.identity.clone();
    epoch.writer_epoch = 2;
    let custody = FakeCustody::new();
    assert_eq!(
        Journal::open_on(f.faults.disk(), &f.root, &epoch, &custody).err(),
        Some(JournalError::IdentityMismatch)
    );

    // An unknown format.
    let anchor = journal_dir(&f.root, &f.identity).join("anchor.json");
    let text = fs::read_to_string(&anchor).unwrap();
    fs::remove_file(&anchor).unwrap();
    fs::write(
        &anchor,
        text.replace("acceptance-journal/1", "acceptance-journal/9"),
    )
    .unwrap();
    assert_eq!(
        f.open().err(),
        Some(JournalError::UnknownFormat("acceptance-journal/9".into()))
    );
}

#[test]
fn a_missing_predecessor_segment_refuses_to_open() {
    for missing in [1, 2] {
        let f = fixture();
        let journal = f.create();
        journal.set_segment_bytes(1);
        let entries = f.fill(&journal, 3);
        assert_eq!(journal.segment_firsts(), [1, 2, 3]);
        drop(journal);
        // The host confirmed all three: nothing below that head is a torn tail.
        f.custody.hold(&f.identity, head_of(&entries[2]));
        fs::remove_file(f.segment(missing)).unwrap();
        assert_eq!(
            f.open().err(),
            Some(JournalError::MissingPredecessor {
                ordinal: missing + 1
            })
        );
    }
}

/// A segment is rolled without a flush, so a power cut can empty a segment that later
/// ones follow. Everything in the gap is above the confirmed head: it is a torn tail.
#[test]
fn a_power_cut_in_a_roll_window_is_a_torn_tail_and_not_a_missing_predecessor() {
    let f = fixture();
    let journal = f.create();
    journal.set_segment_bytes(1);
    for _ in 0..2 {
        let entry = next(&journal.head(), &f.identity);
        journal.append(std::slice::from_ref(&entry)).unwrap();
    }
    assert_eq!(journal.segment_firsts(), [1, 2]);
    drop(journal);
    f.faults.power_cut().unwrap();
    assert_eq!(fs::metadata(f.segment(1)).unwrap().len(), 0);

    f.faults.revive();
    let (journal, recovered) = f.open().unwrap();
    assert_eq!(recovered.truncated, Some(1));
    assert!(recovered.entries.is_empty());
    assert_eq!(journal.head().ordinal, 0);
    assert_eq!(journal.segment_firsts(), [1]);
    assert!(!f.segment(2).exists());
    // The journal goes on from what it kept.
    journal.set_segment_bytes(1);
    f.fill(&journal, 2);
    drop(journal);
    let (journal, recovered) = f.open().unwrap();
    assert_eq!((journal.head().ordinal, recovered.truncated), (2, None));
}

/// Four one-entry segments. Segment 2 is cut to nothing, in the middle and one byte
/// short; an empty one is found by the gap it leaves before segment 3.
#[test]
fn a_torn_cut_in_a_segment_that_is_not_the_last_drops_the_later_segments() {
    for case in 0..3 {
        let f = fixture();
        let journal = f.create();
        journal.set_segment_bytes(1);
        let entries = f.fill(&journal, 4);
        drop(journal);
        f.custody.hold(&f.identity, head_of(&entries[0]));
        let whole = entries[1].stored().len() as u64;
        let cut = [0, whole / 2, whole - 1][case];
        let file = fs::OpenOptions::new()
            .write(true)
            .open(f.segment(2))
            .unwrap();
        file.set_len(cut).unwrap();
        // The torn tail may span segments: segment 3 is cut as well.
        let file = fs::OpenOptions::new()
            .write(true)
            .open(f.segment(3))
            .unwrap();
        file.set_len(5).unwrap();

        let (journal, recovered) = f.open().unwrap_or_else(|e| panic!("cut {cut}: {e}"));
        assert_eq!(recovered.entries, entries[..1], "cut {cut}");
        assert_eq!(recovered.truncated, Some(2), "cut {cut}");
        assert_eq!(journal.head().ordinal, 1, "cut {cut}");
        assert_eq!(fs::metadata(f.segment(2)).unwrap().len(), 0, "cut {cut}");
        assert!(!f.segment(3).exists() && !f.segment(4).exists());
        assert_eq!(journal.segment_firsts(), [1, 2], "cut {cut}");
        f.fill(&journal, 2);
        drop(journal);
        let (journal, recovered) = f.open().unwrap();
        assert_eq!((journal.head().ordinal, recovered.truncated), (3, None));
    }
}

/// Two segments of two entries each; the first loses its second entry at a clean
/// boundary. Above the confirmed head that is a torn tail spanning both segments; at
/// or below it, the journal refuses.
#[test]
fn a_gap_between_segments_is_a_torn_tail_only_above_the_confirmed_head() {
    for confirmed in [1, 2] {
        let f = fixture();
        let journal = f.create();
        let mut entries = f.fill(&journal, 2);
        journal.set_segment_bytes(1);
        entries.extend(f.fill(&journal, 1));
        journal.set_segment_bytes(SEGMENT_BYTES);
        entries.extend(f.fill(&journal, 1));
        assert_eq!(journal.segment_firsts(), [1, 3]);
        drop(journal);
        f.custody
            .hold(&f.identity, head_of(&entries[confirmed - 1]));
        let file = fs::OpenOptions::new()
            .write(true)
            .open(f.segment(1))
            .unwrap();
        file.set_len(ends(&entries)[0] as u64).unwrap();

        let opened = f.open();
        if confirmed == 2 {
            assert_eq!(
                opened.err(),
                Some(JournalError::MissingPredecessor { ordinal: 3 })
            );
            assert!(f.segment(3).exists());
            continue;
        }
        let (journal, recovered) = opened.unwrap();
        assert_eq!(recovered.entries, entries[..1]);
        assert_eq!(recovered.truncated, Some(2));
        assert_eq!(journal.segment_firsts(), [1]);
        assert!(!f.segment(3).exists());
        f.fill(&journal, 1);
        drop(journal);
        assert_eq!(f.open().unwrap().0.head().ordinal, 2);
    }
}

/// The host confirmed an entry the journal released: there is nothing to compare, and
/// that is a refusal, not a pass.
#[test]
fn a_confirmed_head_the_journal_no_longer_holds_refuses_to_open() {
    let f = fixture();
    let journal = f.create();
    journal.set_segment_bytes(1);
    let entries = f.fill(&journal, 4);
    journal.set_materialized(4).unwrap();
    assert_eq!(journal.release_through(4).unwrap(), 3);
    drop(journal);
    f.custody.hold(&f.identity, head_of(&entries[1]));
    assert_eq!(
        f.open().err(),
        Some(JournalError::MissingPredecessor { ordinal: 2 })
    );
    // The checkpoint itself still compares.
    f.custody.hold(&f.identity, head_of(&entries[2]));
    assert!(f.open().is_ok());
}

#[test]
fn a_torn_tail_above_the_confirmed_head_is_truncated_at_any_cut_byte() {
    for seed in 0..48 {
        let mut rng = Rng::new(seed);
        let f = fixture();
        let journal = f.create();
        let entries = f.fill(&journal, 5);
        drop(journal);
        f.custody.hold(&f.identity, head_of(&entries[2]));
        let ends = ends(&entries);
        let cut = rng.between(ends[2] as u64, ends[4] as u64) as usize;
        let segment = f.segment(1);
        let file = fs::OpenOptions::new().write(true).open(&segment).unwrap();
        file.set_len(cut as u64).unwrap();

        let kept = ends.iter().filter(|end| **end <= cut).count();
        let (journal, recovered) = f.open().unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        assert_eq!(recovered.entries, entries[..kept], "seed {seed}");
        assert_eq!(journal.head().ordinal, kept as u64, "seed {seed}");
        let torn = (ends[kept - 1] != cut).then_some(kept as u64 + 1);
        assert_eq!(recovered.truncated, torn, "seed {seed}");
        assert_eq!(
            fs::metadata(&segment).unwrap().len(),
            ends[kept - 1] as u64,
            "seed {seed}"
        );
        // The journal goes on from what it kept.
        f.fill(&journal, 1);
        drop(journal);
        let (journal, _) = f.open().unwrap();
        assert_eq!(journal.head().ordinal, kept as u64 + 1, "seed {seed}");
    }
}

#[test]
fn one_flipped_byte_at_or_below_the_confirmed_head_refuses_and_names_the_ordinal() {
    for seed in 0..48 {
        let mut rng = Rng::new(seed);
        let f = fixture();
        let journal = f.create();
        let entries = f.fill(&journal, 5);
        drop(journal);
        f.custody.hold(&f.identity, head_of(&entries[2]));
        let ends = ends(&entries);
        let segment = f.segment(1);
        let original = fs::read(&segment).unwrap();

        let at = rng.below(ends[2] as u64) as usize;
        let mut flipped = original.clone();
        flipped[at] ^= 1 << rng.below(8);
        fs::write(&segment, &flipped).unwrap();
        let ordinal = ends.iter().filter(|end| **end <= at).count() as u64 + 1;
        match f.open().err() {
            Some(JournalError::Corrupt { ordinal: named, .. }) => {
                assert_eq!(named, ordinal, "seed {seed}")
            }
            other => panic!("seed {seed}: byte {at} gave {other:?}"),
        }
        // Corruption is never truncated away.
        assert_eq!(fs::read(&segment).unwrap(), flipped, "seed {seed}");

        // The same damage above the confirmed head is a torn tail.
        let at = rng.between(ends[2] as u64, ends[4] as u64) as usize;
        let mut flipped = original.clone();
        flipped[at] ^= 1 << rng.below(8);
        fs::write(&segment, &flipped).unwrap();
        let kept = ends.iter().filter(|end| **end <= at).count();
        let (journal, recovered) = f.open().unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        assert_eq!(journal.head().ordinal, kept as u64, "seed {seed}");
        assert_eq!(recovered.truncated, Some(kept as u64 + 1), "seed {seed}");
    }
}

#[test]
fn a_whole_entry_that_does_not_continue_the_chain_is_corruption_even_above_the_head() {
    let f = fixture();
    let journal = f.create();
    let entries = f.fill(&journal, 2);
    drop(journal);
    // Entry 2 written twice: correctly framed and hashed, but not a successor of itself.
    let mut bytes = fs::read(f.segment(1)).unwrap();
    bytes.extend_from_slice(entries[1].stored());
    fs::write(f.segment(1), &bytes).unwrap();
    assert!(matches!(
        f.open().err(),
        Some(JournalError::Corrupt { ordinal: 3, .. })
    ));
}

#[test]
fn a_failed_or_short_append_keeps_nothing_and_leaves_the_segment_at_its_last_good_entry() {
    let f = fixture();
    let journal = f.create();
    let entries = f.fill(&journal, 2);
    let good = ends(&entries)[1] as u64;
    let head = journal.head();

    f.faults.fail(DiskOp::Write, Target::Segment, 1);
    let batch = [next(&head, &f.identity)];
    assert!(matches!(
        journal.append(&batch),
        Err(JournalError::Append(_))
    ));
    assert_eq!(journal.head(), head);
    assert_eq!(fs::metadata(f.segment(1)).unwrap().len(), good);

    f.faults.clear_log();
    f.faults.short_write(Target::Segment, 1, 10);
    assert!(matches!(
        journal.append(&batch),
        Err(JournalError::Append(_))
    ));
    assert_eq!(journal.head(), head);
    assert_eq!(fs::metadata(f.segment(1)).unwrap().len(), good);
    // The truncation is flushed before the next batch.
    let calls: Vec<_> = f.faults.log().into_iter().map(|(op, _)| op).collect();
    assert_eq!(calls, [DiskOp::Write, DiskOp::Truncate, DiskOp::Flush]);

    journal.append(&batch).unwrap();
    journal.flush().unwrap();
    drop(journal);
    let (journal, recovered) = f.open().unwrap();
    assert_eq!(journal.head().ordinal, 3);
    assert_eq!(recovered.truncated, None);
}

#[test]
fn a_failed_truncation_after_a_short_append_stops_the_journal() {
    let f = fixture();
    let journal = f.create();
    f.fill(&journal, 2);
    let head = journal.head();
    let batch = [next(&head, &f.identity)];
    f.faults.short_write(Target::Segment, 1, 10);
    f.faults.fail(DiskOp::Truncate, Target::Segment, 1);
    assert!(matches!(
        journal.append(&batch),
        Err(JournalError::Broken { .. })
    ));
    assert!(matches!(
        journal.append(&batch),
        Err(JournalError::Broken { .. })
    ));
    assert_eq!(journal.head(), head);
    drop(journal);
    // The next process finds the partial entry as a torn tail.
    f.faults.revive();
    let (journal, recovered) = f.open().unwrap();
    assert_eq!(journal.head().ordinal, 2);
    assert_eq!(recovered.truncated, Some(3));
}

#[test]
fn no_entry_is_handed_out_before_its_flush_and_a_flush_error_is_reported() {
    let f = fixture();
    let journal = f.create();
    let first = next(&journal.head(), &f.identity);
    journal.append(std::slice::from_ref(&first)).unwrap();
    assert_eq!(journal.flushed(), 0);
    assert!(journal.after(0).is_empty());
    assert!(matches!(
        journal.set_materialized(1),
        Err(JournalError::Internal(_))
    ));
    assert_eq!(journal.flush().unwrap().ordinal, 1);
    assert_eq!(journal.after(0), [first]);

    let second = next(&journal.head(), &f.identity);
    journal.append(std::slice::from_ref(&second)).unwrap();
    f.faults.fail(DiskOp::Flush, Target::Segment, 1);
    match journal.flush() {
        Err(JournalError::Flush { path, .. }) => assert!(path.ends_with(".log"), "{path}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(journal.flushed(), 1);
    assert_eq!(journal.after(0).len(), 1);
}

#[test]
fn a_failed_flush_is_never_followed_by_another_flush_or_append() {
    let f = fixture();
    let journal = f.create();
    let first = next(&journal.head(), &f.identity);
    journal.append(std::slice::from_ref(&first)).unwrap();
    f.faults.fail(DiskOp::Flush, Target::Segment, 1);
    let failed = journal.flush().unwrap_err();
    assert!(matches!(failed, JournalError::Flush { .. }));
    f.faults.clear_log();
    // The disk would now succeed, and must not be asked: the pages may be gone.
    assert_eq!(journal.flush().err(), Some(failed.clone()));
    let second = next(&journal.head(), &f.identity);
    assert_eq!(
        journal.append(std::slice::from_ref(&second)).err(),
        Some(failed)
    );
    assert!(f.faults.log().is_empty());
    assert_eq!(journal.flushed(), 0);
    assert!(journal.after(0).is_empty());

    // The same when the flush that fails is the one after a truncation.
    let g = fixture();
    let journal = g.create();
    g.fill(&journal, 1);
    let batch = [next(&journal.head(), &g.identity)];
    g.faults.short_write(Target::Segment, 1, 5);
    g.faults.fail(DiskOp::Flush, Target::Segment, 1);
    assert!(matches!(
        journal.append(&batch),
        Err(JournalError::Broken { .. })
    ));
    assert!(matches!(journal.flush(), Err(JournalError::Broken { .. })));
}

#[test]
fn a_tree_is_never_framed_as_a_file_and_positions_past_the_limit_refuse_to_open() {
    let f = fixture();
    let journal = f.create();
    let head = journal.head();
    let draft = EntryDraft {
        ordinal: 1,
        previous_sha256: head.sha256,
        office_id: f.identity.office_id.clone(),
        journal_incarnation: f.identity.journal_incarnation.clone(),
        at: Timestamp(1),
        ack: Ack::Committed,
        feed: FeedPosition(1),
        operation: None,
        positions: BTreeMap::new(),
        files: vec![FileDraft {
            path: DurablePath::parse("state").unwrap(),
            op: FileOp::Put,
            bytes: Some(b"x".to_vec().into()),
            previous_sha256: None,
        }],
    };
    assert!(matches!(
        Entry::frame(draft),
        Err(JournalError::Internal(_))
    ));
    drop(journal);
    let past = Checkpoint {
        format: FORMAT.to_owned(),
        ordinal: 3,
        sha256: head.sha256,
        high_waters: HighWaters::default(),
        feed: FeedPosition(u64::MAX),
    };
    fs::write(
        journal_dir(&f.root, &f.identity).join("checkpoint.json"),
        serde_json::to_vec(&past).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        f.open().err(),
        Some(JournalError::Corrupt { ordinal: 3, .. })
    ));
}

#[test]
fn segments_roll_and_are_released_only_after_the_checkpoint_is_written() {
    let f = fixture();
    let journal = f.create();
    journal.set_segment_bytes(1);
    let entries = f.fill(&journal, 4);
    assert_eq!(journal.segment_firsts(), [1, 2, 3, 4]);
    // Nothing is released before its files are written.
    assert_eq!(journal.release_through(4).unwrap(), 0);
    journal.set_materialized(3).unwrap();
    assert_eq!(journal.after(0), entries[3..]);

    f.faults.clear_log();
    assert_eq!(journal.release_through(4).unwrap(), 3);
    let log = f.faults.log();
    let checkpoint = log
        .iter()
        .position(|(op, path)| *op == DiskOp::Rename && path.ends_with("checkpoint.json"))
        .unwrap();
    let flushed = log
        .iter()
        .rposition(|(op, _)| *op == DiskOp::Flush)
        .unwrap();
    let unlinked = log
        .iter()
        .position(|(op, path)| {
            *op == DiskOp::Unlink && path.extension().is_some_and(|e| e == "log")
        })
        .unwrap();
    assert!(checkpoint < flushed && flushed < unlinked, "{log:?}");
    assert_eq!(journal.segment_firsts(), [4]);
    assert_eq!(journal.base().ordinal, 3);
    let head = journal.head();
    drop(journal);

    // The chain now starts at the checkpoint, and the positions are still recovered.
    f.custody.hold(&f.identity, head_of(&entries[3]));
    let (journal, recovered) = f.open().unwrap();
    assert_eq!(journal.head(), head);
    assert_eq!(journal.base().ordinal, 3);
    assert_eq!(journal.base().sha256, entries[2].sha256);
    assert_eq!(recovered.entries, entries[3..]);
    assert!(!f.segment(1).exists());
}

#[test]
fn a_release_cut_after_its_checkpoint_is_finished_at_the_next_opening() {
    let f = fixture();
    let journal = f.create();
    journal.set_segment_bytes(1);
    f.fill(&journal, 4);
    journal.set_materialized(4).unwrap();
    f.faults.crash(DiskOp::Unlink, Target::Segment, 2, false);
    assert!(journal.release_through(4).is_err());
    let head = journal.head();
    drop(journal);
    assert!(f.segment(2).exists());

    f.faults.revive();
    let (journal, recovered) = f.open().unwrap();
    assert_eq!(journal.head(), head);
    assert_eq!(journal.segment_firsts(), [4]);
    assert_eq!(recovered.entries.len(), 1);
    assert!(!f.segment(2).exists() && !f.segment(3).exists());
}

impl Fixture {
    /// Three flushed one-entry segments, a fourth rolled since the last flush, and a
    /// release through 3 that dies before its first unlink. Then the power goes, taking
    /// every directory entry that was not flushed.
    fn cut_in_a_release(&self) {
        let journal = self.create();
        journal.set_segment_bytes(1);
        self.fill(&journal, 3);
        journal.set_materialized(3).unwrap();
        let fourth = next(&journal.head(), &self.identity);
        journal.append(std::slice::from_ref(&fourth)).unwrap();
        assert_eq!(journal.segment_firsts(), [1, 2, 3, 4]);
        self.faults.crash(DiskOp::Unlink, Target::Segment, 1, false);
        assert!(journal.release_through(3).is_err());
        drop(journal);
        self.faults.power_cut_losing_entries().unwrap();
        self.faults.revive();
    }

    /// Every segment file and what it holds.
    fn segment_bytes(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        let dir = journal_dir(&self.root, &self.identity).join("segments");
        let listing = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path());
        listing
            .map(|path| (path.clone(), fs::read(&path).unwrap()))
            .collect()
    }
}

/// The checkpoint names the segment after the released ones. That segment may have been
/// rolled since the last flush, so the release flushes the directory first: a power cut
/// cannot keep the checkpoint and lose the segment it leads to.
#[test]
fn a_power_cut_in_a_release_keeps_the_segment_the_checkpoint_leads_to() {
    let f = fixture();
    f.cut_in_a_release();
    // On the drive by name; its one entry was never flushed.
    assert_eq!(fs::metadata(f.segment(4)).unwrap().len(), 0);
    assert!(f.segment(1).exists() && f.segment(3).exists());

    let (journal, recovered) = f.open().unwrap();
    assert_eq!((journal.base().ordinal, journal.head().ordinal), (3, 3));
    assert_eq!(journal.segment_firsts(), [4]);
    assert!(recovered.entries.is_empty());
    assert!(!f.segment(1).exists() && !f.segment(3).exists());
    f.fill(&journal, 1);
    drop(journal);
    let (journal, recovered) = f.open().unwrap();
    assert_eq!((journal.head().ordinal, recovered.entries.len()), (4, 1));
}

/// A journal can be left with the checkpoint and only released segments (one written
/// before the release flushed the directory first). The last of them has no successor to
/// prove it by name: it is dropped when all it holds is whole and at or below the
/// checkpoint, and refused otherwise.
#[test]
fn a_sole_segment_wholly_below_the_checkpoint_is_dropped_and_any_other_refuses() {
    let f = fixture();
    f.cut_in_a_release();
    fs::remove_file(f.segment(4)).unwrap();
    let whole = fs::read(f.segment(3)).unwrap();

    // Not whole: it is not what a release left.
    fs::write(f.segment(3), &whole[..whole.len() - 1]).unwrap();
    let before = f.segment_bytes();
    assert_eq!(
        f.open().err(),
        Some(JournalError::MissingPredecessor { ordinal: 3 })
    );
    assert_eq!(f.segment_bytes(), before);
    // Holding an entry past the checkpoint: the same.
    let third = JournalPoint {
        ordinal: 3,
        sha256: Sha256::of(b"entry three"),
        high_waters: HighWaters::default(),
        feed: FeedPosition(3),
    };
    let stray = next(&third, &f.identity);
    let mut longer = whole.clone();
    longer.extend_from_slice(stray.stored());
    fs::write(f.segment(3), &longer).unwrap();
    assert_eq!(
        f.open().err(),
        Some(JournalError::MissingPredecessor { ordinal: 3 })
    );

    // Whole and ending at the checkpoint, but with another entry than the one the
    // checkpoint was written from: corruption, and every byte stays as evidence.
    let second = JournalPoint {
        ordinal: 2,
        sha256: Sha256::of(b"another entry two"),
        high_waters: HighWaters::default(),
        feed: FeedPosition(2),
    };
    let other = next(&second, &f.identity);
    assert_eq!(other.ordinal(), 3);
    fs::write(f.segment(3), other.stored()).unwrap();
    let before = f.segment_bytes();
    assert!(matches!(
        f.open().err(),
        Some(JournalError::Corrupt { ordinal: 3, .. })
    ));
    assert_eq!(f.segment_bytes(), before);

    fs::write(f.segment(3), &whole).unwrap();
    let (journal, recovered) = f.open().unwrap();
    assert_eq!((journal.base().ordinal, journal.head().ordinal), (3, 3));
    assert!(journal.segment_firsts().is_empty() && recovered.entries.is_empty());
    assert!(f.segment_bytes().is_empty());
    // The journal goes on from the checkpoint.
    f.fill(&journal, 2);
    assert_eq!(journal.segment_firsts(), [4]);
    drop(journal);
    let (journal, recovered) = f.open().unwrap();
    assert_eq!((journal.head().ordinal, recovered.entries.len()), (5, 2));
}

/// Recovery removes a torn tail only from a journal it accepts. An opening that ends in
/// a fork changes nothing: the tail may be the only copy of what was acknowledged, and
/// it is the evidence.
#[test]
fn an_opening_that_refuses_a_fork_leaves_every_byte_of_a_torn_tail() {
    let f = fixture();
    let journal = f.create();
    journal.set_segment_bytes(1);
    let entries = f.fill(&journal, 5);
    drop(journal);
    // Entry 3 is cut one byte short: a repair would empty its segment and remove the
    // two after it.
    let third = fs::read(f.segment(3)).unwrap();
    fs::write(f.segment(3), &third[..third.len() - 1]).unwrap();
    let before = f.segment_bytes();
    assert_eq!(before.len(), 5);

    let mut other = head_of(&entries[1]);
    other.sha256 = Sha256::of(b"another history");
    f.custody.hold(&f.identity, other);
    f.faults.clear_log();
    assert_eq!(f.open().err(), Some(JournalError::Fork { ordinal: 2 }));
    assert_eq!(f.segment_bytes(), before);
    let changed = [DiskOp::Unlink, DiskOp::Truncate, DiskOp::Write];
    let log = f.faults.log();
    assert!(log.iter().all(|(op, _)| !changed.contains(op)), "{log:?}");

    // The journal the host did confirm is repaired as before.
    f.custody.hold(&f.identity, head_of(&entries[1]));
    let (journal, recovered) = f.open().unwrap();
    assert_eq!(recovered.truncated, Some(3));
    assert_eq!(journal.head().ordinal, 2);
    assert_eq!(fs::metadata(f.segment(3)).unwrap().len(), 0);
    assert!(!f.segment(4).exists() && !f.segment(5).exists());
}

#[test]
fn journal_and_origin_arithmetic_never_wrap_or_restart_incarnations() {
    // A new incarnation continues the positions its anchor names.
    let f = fixture();
    let facts = AnchorFacts {
        high_waters: HighWaters {
            work: 41,
            ..HighWaters::default()
        },
        feed: FeedPosition(7),
        predecessor: Some(Predecessor {
            journal_incarnation: id(0xB1),
            ordinal: 12,
            sha256: Sha256::of(b"predecessor head"),
        }),
    };
    let journal = f.create_from(&facts);
    f.fill(&journal, 2);
    drop(journal);
    let (journal, _) = f.open().unwrap();
    let head = journal.head();
    assert_eq!((head.high_waters.work, head.feed), (43, FeedPosition(9)));

    // Positions that do not follow are refused before they reach the segment.
    let mut restart = head.clone();
    restart.high_waters.work = 0;
    let wrong = next(
        &JournalPoint {
            ordinal: 3,
            ..restart
        },
        &f.identity,
    );
    assert!(matches!(
        journal.append(std::slice::from_ref(&wrong)),
        Err(JournalError::Internal(_))
    ));
    assert_eq!(journal.head(), head);

    // High-waters past the limit never start a journal.
    let g = fixture();
    let beyond = AnchorFacts {
        high_waters: HighWaters {
            work: MAX_POSITION + 1,
            ..HighWaters::default()
        },
        ..AnchorFacts::default()
    };
    assert_eq!(
        Journal::create_on(g.faults.disk(), &g.root, &g.identity, &beyond).err(),
        Some(JournalError::Exhausted("origin position"))
    );

    // At the last ordinal the journal still opens and answers, and takes nothing more.
    let journal = g.create();
    let anchor = journal.head();
    drop(journal);
    let last = Checkpoint {
        format: FORMAT.to_owned(),
        ordinal: MAX_POSITION,
        sha256: anchor.sha256,
        high_waters: HighWaters::default(),
        feed: FeedPosition(3),
    };
    fs::write(
        journal_dir(&g.root, &g.identity).join("checkpoint.json"),
        serde_json::to_vec(&last).unwrap(),
    )
    .unwrap();
    let (journal, _) = g.open().unwrap();
    assert_eq!(journal.head().ordinal, MAX_POSITION);
    assert_eq!(
        journal.next_ordinal(),
        Err(JournalError::Exhausted("journal ordinal"))
    );
    let past = next(&journal.head(), &g.identity);
    assert_eq!(
        journal.append(std::slice::from_ref(&past)),
        Err(JournalError::Exhausted("journal ordinal"))
    );
    assert!(journal.segment_firsts().is_empty());
}

#[test]
fn an_entry_over_the_limit_is_refused_when_framed() {
    let point = JournalPoint {
        ordinal: 0,
        sha256: Sha256::of(b"anchor"),
        high_waters: HighWaters::default(),
        feed: FeedPosition(0),
    };
    let identity = identity();
    let draft = |bytes: usize| EntryDraft {
        ordinal: 1,
        previous_sha256: point.sha256,
        office_id: identity.office_id.clone(),
        journal_incarnation: identity.journal_incarnation.clone(),
        at: Timestamp(1),
        ack: Ack::Committed,
        feed: FeedPosition(1),
        operation: None,
        positions: BTreeMap::new(),
        files: vec![FileDraft {
            path: DurablePath::parse("state/big").unwrap(),
            op: FileOp::Put,
            bytes: Some(vec![0u8; bytes].into()),
            previous_sha256: None,
        }],
    };
    assert!(matches!(
        Entry::frame(draft(MAX_ENTRY_BYTES)),
        Err(JournalError::TooLarge { .. })
    ));
    assert!(Entry::frame(draft(1024)).is_ok());
    let mut removal = draft(1);
    removal.files[0].op = FileOp::Remove;
    assert!(matches!(
        Entry::frame(removal),
        Err(JournalError::Internal(_))
    ));
}

/// STORE-3, part 1: an entry names files, positions and a receipt, never a table, a
/// column or an engine.
#[test]
fn no_header_key_or_value_names_a_table_a_column_or_an_engine() {
    fn strings(value: &serde_json::Value, out: &mut Vec<String>) {
        match value {
            serde_json::Value::String(text) => out.push(text.clone()),
            serde_json::Value::Array(items) => items.iter().for_each(|v| strings(v, out)),
            serde_json::Value::Object(map) => {
                for (key, value) in map {
                    out.push(key.clone());
                    strings(value, out);
                }
            }
            _ => {}
        }
    }
    let f = fixture();
    let journal = f.create();
    let entries = f.fill(&journal, 12);
    let mut seen = Vec::new();
    let mut keys = std::collections::BTreeSet::new();
    for entry in &entries {
        let header = serde_json::to_value(&entry.header).unwrap();
        // The stored header is exactly this JSON, at the front of the frame.
        let stored = serde_json::to_vec(&entry.header).unwrap();
        assert_eq!(&entry.stored()[4..4 + stored.len()], stored);
        keys.extend(header.as_object().unwrap().keys().cloned());
        strings(&header, &mut seen);
    }
    let expected = [
        "ack",
        "at",
        "feed",
        "files",
        "journal_incarnation",
        "office_id",
        "operation",
        "ordinal",
        "positions",
        "previous_sha256",
    ];
    assert_eq!(
        keys.iter().map(String::as_str).collect::<Vec<_>>(),
        expected
    );
    let engine_terms = [
        "sql",
        "table",
        "column",
        "row",
        "index",
        "pragma",
        "rusqlite",
        "postgres",
        "schema",
        "mutation_receipts",
        "store_cursor",
        "store_sources",
        "digest",
    ];
    for text in &seen {
        let lower = text.to_lowercase();
        for term in engine_terms {
            assert!(!lower.contains(term), "{text:?} names {term:?}");
        }
    }
}
