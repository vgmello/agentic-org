use std::{fs, path::PathBuf, sync::Arc, time::Duration};

use super::*;
use crate::{
    change::{Ack, Timestamp},
    custody::WriterAuthority,
    engine::CoreRead,
    journal::AnchorFacts,
    state::Limits,
    store::{Opening, Store, StoreParts},
    testkit::{
        identity, probe::REFUSED, rebuild_equivalence, with_faults, CrashPoint, Dump, FakeCustody,
        Faults, FixedClock, MemoryEngine, Probe,
    },
};

struct Office {
    _dir: tempfile::TempDir,
    root: PathBuf,
    faults: Arc<Faults>,
    custody: Arc<FakeCustody>,
    engine: Arc<MemoryEngine>,
    probe: Probe,
    limits: Limits,
}

impl Office {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap().join("durable");
        Self {
            _dir: dir,
            root,
            faults: Faults::new(),
            custody: Arc::new(FakeCustody::new()),
            engine: Arc::new(MemoryEngine::new()),
            probe: Probe::new(&identity().office_id),
            limits: Limits::default(),
        }
    }

    fn open(&self, opening: Opening) -> Result<Store<MemoryEngine>, StoreError> {
        self.faults.revive();
        let mut parts = StoreParts::new(
            self.root.clone(),
            identity(),
            Arc::clone(&self.custody) as Arc<dyn WriterAuthority>,
            Arc::clone(&self.engine),
            vec![self.probe.projector()],
            Arc::new(FixedClock(Timestamp(1_700_000_000_000))),
        );
        parts.opening = opening;
        parts.flush_interval = Duration::from_secs(3_600);
        parts.limits = self.limits;
        Store::open(with_faults(parts, &self.faults))
    }

    /// An office with a few records, closed in order. Returns its dump.
    fn filled(&self) -> Vec<String> {
        let store = self.open(Opening::Create(AnchorFacts::default())).unwrap();
        for name in ["a", "b", "c", "b"] {
            let put = self.probe.put(name, name.as_bytes(), Ack::Committed);
            store.submit(None, false, put).unwrap();
        }
        let create = self.probe.create("d", b"d");
        let op = self
            .probe
            .operation(crate::testkit::id(1), "probe.create", "d", b"d");
        store.submit(Some(op), false, create).unwrap();
        store.close().unwrap();
        self.engine.dump()
    }
}

/// The property of the design's section 6, on the probe family and the memory engine.
#[test]
fn rebuild_equals_the_write_path_with_and_without_a_crash() {
    let probe = Probe::new(&identity().office_id);
    rebuild_equivalence(&|| (Arc::new(MemoryEngine::new()), ()), &probe, 0..24);
}

#[test]
fn a_file_no_family_owns_fails_the_rebuild_with_its_path_and_the_old_projection_stays() {
    let office = Office::new();
    let before = office.filled();
    fs::create_dir_all(office.root.join("state")).unwrap();
    fs::write(office.root.join("state/stray.yaml"), b"who wrote this\n").unwrap();

    // Asked for directly, over a live projection.
    let files = Files::open(&office.root).unwrap();
    let (journal, _) = Journal::open(&office.root, &identity(), office.custody.as_ref()).unwrap();
    let families = [office.probe.projector::<MemoryEngine>()];
    let refused = rebuild(&files, &journal, office.engine.as_ref(), &families);
    match refused {
        Err(StoreError::Projection(error)) => {
            assert_eq!(error.path.unwrap().as_str(), "state/stray.yaml");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(office.engine.dump(), before);
    drop(journal);

    // At opening, when the projection is gone: the office does not open on a partial one.
    office.engine.wipe();
    assert!(matches!(
        office.open(Opening::Existing).err(),
        Some(StoreError::Projection(_))
    ));
    assert_eq!(office.engine.dump(), ["cursor None"]);
    fs::remove_file(office.root.join("state/stray.yaml")).unwrap();
    office.open(Opening::Existing).unwrap().close().unwrap();
    assert_eq!(office.engine.dump(), before);
}

#[test]
fn a_record_its_family_marks_skippable_is_left_out_and_listed() {
    let office = Office::new();
    office.filled();
    let bad = office.probe.path("damaged");
    fs::write(office.root.join(bad.as_str()), REFUSED).unwrap();

    let files = Files::open(&office.root).unwrap();
    let (journal, _) = Journal::open(&office.root, &identity(), office.custody.as_ref()).unwrap();
    let families = [office.probe.projector::<MemoryEngine>()];
    let report = rebuild(&files, &journal, office.engine.as_ref(), &families).unwrap();
    // a, b, c, d and d's receipt are projected; the damaged record is reported.
    assert_eq!(report.files, 5);
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].path.as_ref(), Some(&bad));
    assert_eq!(report.cursor, journal.head().cursor());
    let snapshot = office.engine.snapshot().unwrap();
    assert_eq!(snapshot.source_hash(&bad).unwrap(), None);
    assert!(snapshot
        .source_hash(&office.probe.path("a"))
        .unwrap()
        .is_some());
    assert_eq!(snapshot.cursor().unwrap(), Some(report.cursor));
}

#[test]
fn a_rebuild_that_dies_leaves_the_old_projection_and_the_next_one_completes() {
    let office = Office::new();
    let before = office.filled();
    // A projection that is not on the journal's chain forces a rebuild at opening.
    let tx = office.engine.begin().unwrap();
    let mut stale = office.engine.snapshot().unwrap().cursor().unwrap().unwrap();
    stale.sha256 = Sha256::of(b"another history");
    office.engine.commit(tx, &stale).unwrap();
    let stale_dump = office.engine.dump();
    assert_ne!(stale_dump, before);

    office.faults.revive();
    let mut parts = StoreParts::new(
        office.root.clone(),
        identity(),
        Arc::clone(&office.custody) as Arc<dyn WriterAuthority>,
        Arc::clone(&office.engine),
        vec![office.probe.projector()],
        Arc::new(FixedClock(Timestamp(1))),
    );
    parts.opening = Opening::Existing;
    office.faults.arm(CrashPoint::DuringRebuild, 2);
    assert!(Store::open(with_faults(parts, &office.faults)).is_err());
    assert_eq!(office.engine.dump(), stale_dump);

    office.open(Opening::Existing).unwrap().close().unwrap();
    assert_eq!(office.engine.dump(), before);
}

/// Lead ruling F1: the directory generation is the organization stream's high-water. It
/// comes from the journal and its checkpoint, never from a file, so it is the same after
/// the projection is deleted and after the segments that took it are released.
#[test]
fn the_generation_survives_a_rebuild_and_the_release_of_its_segments() {
    let mut office = Office::new();
    office.limits.segment_bytes = 1;
    let store = office
        .open(Opening::Create(AnchorFacts::default()))
        .unwrap();
    for name in ["a", "b", "c"] {
        store
            .submit(None, false, office.probe.create(name, name.as_bytes()))
            .unwrap();
    }
    // A rejected command leaves the generation where it was.
    let rejected = store
        .submit(None, false, office.probe.create("a", b"a"))
        .unwrap();
    assert_eq!(rejected.outcome, crate::change::Outcome::Rejected);
    let put = office.probe.put("p", b"p", Ack::Durable);
    store.submit(None, false, put).unwrap();
    store.sync().unwrap();
    let generation = |store: &Store<MemoryEngine>| {
        let read = store
            .read(|s| Ok(s.cursor()?.unwrap().high_waters))
            .unwrap();
        read.0
    };
    let before = generation(&store);
    assert_eq!((before.organization, before.work), (3, 1));
    store.close().unwrap();

    // Every rolled segment is released; the chain now starts at the checkpoint.
    let journal = crate::journal::journal_dir(&office.root, &identity());
    assert!(journal.join("checkpoint.json").is_file());
    let segments = fs::read_dir(journal.join("segments")).unwrap().count();
    assert_eq!(segments, 1);

    office.engine.wipe();
    let store = office.open(Opening::Existing).unwrap();
    assert_eq!(generation(&store), before);
    let next = store
        .submit(None, false, office.probe.create("e", b"e"))
        .unwrap();
    assert_eq!(&*next.bytes, b"created e at generation 4");
}
