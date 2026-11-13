//! IA-12b: the work activity port over office.db (freeze rev 4.8 decisions 4-7).

use super::super::tests::{fixture, initialize_origin, T0};
use super::*;
use org_proto::work::WorkEventKind;
use org_proto::Position;
use org_work::LatestActivity::{At, Unknown};
use std::time::Instant;

#[derive(Clone)]
struct Ev {
    target: String,
    at: String,
    actor: Option<String>,
    change: WorkEventKind,
}

fn ev(target: &str, at_ms: u64) -> Ev {
    Ev {
        target: target.into(),
        at: org_work::format_ms(at_ms),
        actor: Some("m-other".into()),
        change: WorkEventKind::Created,
    }
}

impl Ev {
    fn by(mut self, actor: &str) -> Self {
        self.actor = Some(actor.into());
        self
    }
    fn assigned(mut self, to: &str) -> Self {
        self.change = WorkEventKind::Assigned {
            from: None,
            to: Some(to.into()),
        };
        self
    }
}

struct Fx {
    _root: tempfile::TempDir,
    store: Arc<Store>,
    reads: OfficeWorkActivity,
}

fn setup(with_incarnation: bool) -> Fx {
    let (root, store) = fixture();
    if with_incarnation {
        initialize_origin(&store);
    }
    let store = Arc::new(store);
    Fx {
        _root: root,
        reads: OfficeWorkActivity(Arc::clone(&store)),
        store,
    }
}

fn incarnation() -> Option<String> {
    Some(org_work::ulid_from_parts(T0, 1))
}

impl Fx {
    /// Each inner list is one feed row; every event is its target's next version.
    fn commit(&self, rows: Vec<Vec<Ev>>) -> Vec<WorkActivityPosition> {
        self.store
            .write(move |db| {
                let mut positions = Vec::new();
                for row in &rows {
                    let mut targets: Vec<&str> = Vec::new();
                    for e in row {
                        if !targets.contains(&e.target.as_str()) {
                            targets.push(&e.target);
                        }
                    }
                    let seq: i64 = db
                        .query_row(
                            "INSERT INTO work_feed(targets) VALUES (?1) RETURNING seq",
                            [serde_json::to_string(&targets).unwrap()],
                            |r| r.get(0),
                        )
                        .map_err(OfficeError::storage)?;
                    for e in row {
                        let version = insert_event(db, e, seq)?;
                        positions.push(WorkActivityPosition {
                            feed_seq: seq as u64,
                            target_id: e.target.clone(),
                            version,
                        });
                    }
                }
                Ok((positions, None))
            })
            .unwrap()
    }

    /// Puts the item in `project` (now, whatever its events say).
    fn place(&self, id: &str, project: &str) {
        let (id, project) = (id.to_owned(), project.to_owned());
        self.store
            .write(move |db| {
                ensure_item(db, &id)?;
                db.execute(
                    "UPDATE work_items SET data = json_set(data, '$.project_id', ?2) WHERE id = ?1",
                    params![id, project],
                )
                .map_err(OfficeError::storage)?;
                Ok(((), None))
            })
            .unwrap();
    }

    fn drain(&self, mut q: WorkActivityQuery) -> Vec<(WorkActivityPosition, u64)> {
        let mut out = Vec::new();
        for _ in 0..100_000 {
            let page = self.reads.activity_page(&q).unwrap();
            assert!(page.entries.len() <= q.limit.max(1) as usize);
            out.extend(page.entries.iter().map(|e| (e.position.clone(), e.key)));
            if page.exhausted {
                return out;
            }
            q.before = Some(page.scanned_to.expect("a page that scans moves on"));
            q.prev_key = page.frontier;
        }
        panic!("the walk made no progress")
    }

    fn meta_and_sequences(&self) -> Vec<(String, String)> {
        self.store
            .read(|db| {
                let mut q = db
                    .prepare(
                        "SELECT key, value FROM meta UNION ALL
                         SELECT name, CAST(seq AS TEXT) FROM sqlite_sequence ORDER BY 1",
                    )
                    .map_err(OfficeError::storage)?;
                let rows = q
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                    .map_err(OfficeError::storage)?;
                rows.collect::<rusqlite::Result<_>>()
                    .map_err(OfficeError::storage)
            })
            .unwrap()
    }
}

fn ensure_item(db: &Connection, id: &str) -> Result<(), OfficeError> {
    db.execute(
        "INSERT OR IGNORE INTO work_items (id, generation, version, data)
         VALUES (?1, 1, 1, json_object('id', ?1, 'generation', 1, 'version', 1))",
        [id],
    )
    .map(drop)
    .map_err(OfficeError::storage)
}

fn insert_event(db: &Connection, e: &Ev, seq: i64) -> Result<u64, OfficeError> {
    ensure_item(db, &e.target)?;
    let version: i64 = db
        .query_row(
            "SELECT coalesce(max(version), 0) + 1 FROM work_events WHERE target_id = ?1",
            [&e.target],
            |r| r.get(0),
        )
        .map_err(OfficeError::storage)?;
    let event = WorkEvent {
        target_id: e.target.clone(),
        version: version as u64,
        at: e.at.clone(),
        actor_id: e.actor.clone(),
        auto: false,
        cause: None,
        org_position: Position::default(),
        change: e.change.clone(),
    };
    db.execute(
        "INSERT INTO work_events (target_id, version, feed_seq, data) VALUES (?1, ?2, ?3, ?4)",
        params![
            e.target,
            version,
            seq,
            serde_json::to_string(&event).unwrap()
        ],
    )
    .map_err(OfficeError::storage)?;
    Ok(version as u64)
}

fn query(ceiling: u64) -> WorkActivityQuery {
    WorkActivityQuery {
        incarnation: incarnation(),
        ceiling,
        before: None,
        prev_key: u64::MAX,
        member: None,
        team_members: None,
        project: None,
        limit: 100,
        scan: 2000,
    }
}

fn pos(feed_seq: u64, target: &str, version: u64) -> WorkActivityPosition {
    WorkActivityPosition {
        feed_seq,
        target_id: target.into(),
        version,
    }
}

fn newest_first(mut all: Vec<WorkActivityPosition>) -> Vec<WorkActivityPosition> {
    all.sort_by(|a, b| b.cmp(a));
    all
}

fn positions(page: &WorkActivityPage) -> Vec<WorkActivityPosition> {
    page.entries.iter().map(|e| e.position.clone()).collect()
}

#[test]
fn activity_orders_feed_rows_targets_and_versions_newest_first() {
    let fx = setup(true);
    fx.commit(vec![
        vec![ev("A", T0)],
        vec![ev("A", T0), ev("C", T0), ev("C", T0), ev("B", T0)],
        vec![ev("B", T0)],
    ]);
    let page = fx.reads.activity_page(&query(3)).unwrap();
    assert_eq!(
        positions(&page),
        [
            pos(3, "B", 2),
            pos(2, "C", 2),
            pos(2, "C", 1),
            pos(2, "B", 1),
            pos(2, "A", 2),
            pos(1, "A", 1),
        ]
    );
    assert!(page.exhausted);
    assert_eq!(page.scanned_to, Some(pos(1, "A", 1)));
    assert_eq!(page.frontier, T0);
    // The entry carries the stored event itself.
    assert_eq!(page.entries[1].event.target_id, "C");
    assert_eq!(page.entries[1].event.version, 2);
    // Resuming mid-row: the rest of row 2 after C v2, then row 1.
    let mut q = query(3);
    q.before = Some(pos(2, "C", 2));
    let page = fx.reads.activity_page(&q).unwrap();
    assert_eq!(
        positions(&page),
        [
            pos(2, "C", 1),
            pos(2, "B", 1),
            pos(2, "A", 2),
            pos(1, "A", 1)
        ]
    );
}

#[test]
fn activity_pages_join_without_gaps_or_duplicates_for_any_limit_and_scan() {
    let fx = setup(true);
    let mut rows = Vec::new();
    for n in 0..40u64 {
        let mut row = vec![ev("H", T0 + n)];
        if n % 3 == 0 {
            row.push(ev("G", T0 + n));
            row.push(ev("G", T0 + n));
        }
        if n % 4 == 1 {
            row.push(ev("K", T0 + n));
        }
        if n % 7 == 6 {
            row.clear();
        }
        rows.push(row);
    }
    let all = newest_first(fx.commit(rows));
    let whole = fx.drain(query(40));
    assert_eq!(
        whole.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>(),
        all
    );
    for limit in [1, 2, 3, 100] {
        for scan in [1, 2, 5, 2000] {
            let mut q = query(40);
            (q.limit, q.scan) = (limit, scan);
            assert_eq!(fx.drain(q), whole, "limit {limit}, scan {scan}");
        }
    }
}

#[test]
fn activity_resumes_mid_row_on_a_hot_item_with_unseen_targets_in_the_row() {
    let fx = setup(true);
    // H and G both have many versions; every row holds both, so resuming after H at row F
    // finds G's version at F by searching its history, not from this page.
    let rows = (0..300u64)
        .map(|n| vec![ev("H", T0 + n), ev("G", T0 + n), ev("G", T0 + n)])
        .collect();
    fx.commit(rows);
    let mut q = query(300);
    q.before = Some(pos(150, "H", 150));
    q.limit = 5;
    let page = fx.reads.activity_page(&q).unwrap();
    assert_eq!(
        positions(&page),
        [
            pos(150, "G", 300),
            pos(150, "G", 299),
            pos(149, "H", 149),
            pos(149, "G", 298),
            pos(149, "G", 297),
        ]
    );
    assert!(!page.exhausted);
    assert_eq!(page.scanned_to, Some(pos(149, "G", 297)));
}

#[test]
fn activity_keys_are_the_running_minimum_and_survive_a_clock_step_back() {
    let fx = setup(true);
    fx.commit(vec![
        vec![ev("A", T0 + 10)],
        vec![ev("A", T0 + 20)],
        vec![ev("A", T0 + 5)],
        vec![ev("A", T0 + 30)],
    ]);
    let keys = |q: &WorkActivityQuery| -> Vec<u64> {
        let page = fx.reads.activity_page(q).unwrap();
        page.entries.iter().map(|e| e.key).collect()
    };
    assert_eq!(keys(&query(4)), [T0 + 30, T0 + 5, T0 + 5, T0 + 5]);
    let page = fx.reads.activity_page(&query(4)).unwrap();
    assert_eq!(page.entries[2].at_ms, Some(T0 + 20));
    assert_eq!(page.frontier, T0 + 5);
    // Seeded by the previous page's key.
    let mut seeded = query(4);
    seeded.prev_key = T0 + 25;
    assert_eq!(keys(&seeded), [T0 + 25, T0 + 5, T0 + 5, T0 + 5]);
}

#[test]
fn activity_unparseable_at_keeps_the_previous_key_and_is_signalled() {
    let fx = setup(true);
    let mut broken = ev("A", T0);
    broken.at = "yesterday".into();
    fx.commit(vec![
        vec![ev("A", T0 + 10)],
        vec![broken],
        vec![ev("A", T0 + 40)],
    ]);
    let page = fx.reads.activity_page(&query(3)).unwrap();
    let got: Vec<_> = page.entries.iter().map(|e| (e.at_ms, e.key)).collect();
    assert_eq!(
        got,
        [
            (Some(T0 + 40), T0 + 40),
            (None, T0 + 40),
            (Some(T0 + 10), T0 + 10)
        ]
    );
    assert_eq!(page.entries[1].event.at, "yesterday");
}

#[test]
fn activity_never_shows_rows_above_the_ceiling() {
    let fx = setup(true);
    fx.commit((0..5).map(|n| vec![ev("A", T0 + n)]).collect());
    assert_eq!(fx.reads.head().unwrap(), (incarnation(), 5));
    let mut q = query(3);
    q.limit = 1;
    let first = fx.reads.activity_page(&q).unwrap();
    assert_eq!(positions(&first), [pos(3, "A", 3)]);
    // Appends after page 1 stay out of later pages; a `before` above the ceiling is refused.
    fx.commit(vec![vec![ev("A", T0 + 9)], vec![ev("B", T0 + 9)]]);
    q.before = first.scanned_to;
    q.limit = 100;
    let second = fx.reads.activity_page(&q).unwrap();
    assert_eq!(positions(&second), [pos(2, "A", 2), pos(1, "A", 1)]);
    let mut above = query(3);
    above.before = Some(pos(7, "B", 1));
    assert!(matches!(
        fx.reads.activity_page(&above),
        Err(WorkActivityError::InvalidPosition)
    ));
    assert_eq!(fx.reads.head().unwrap(), (incarnation(), 7));
}

#[test]
fn activity_refuses_a_feed_row_whose_unseen_target_has_no_event_there() {
    let fx = setup(true);
    let bare = |target: &str| {
        let targets = serde_json::to_string(&[target]).unwrap();
        fx.store
            .write(move |db| {
                db.execute("INSERT INTO work_feed(targets) VALUES (?1)", [&targets])
                    .map_err(OfficeError::storage)?;
                Ok(((), None))
            })
            .unwrap();
    };
    fx.commit(vec![vec![ev("A", T0)], vec![ev("B", T0 + 1)]]);
    bare("A"); // row 3: A's events are in rows 1 and 4
    fx.commit(vec![vec![ev("A", T0 + 3)]]);
    bare("B"); // row 5: B's newest event is in row 2
               // Ceiling 3 finds A by searching its history; ceiling 5 reads B's newest event directly.
    for ceiling in [3, 5] {
        assert!(matches!(
            fx.reads.activity_page(&query(ceiling)),
            Err(WorkActivityError::Store(_))
        ));
    }
}

impl Fx {
    /// Rewrites stored rows the way a corrupt office.db would hold them.
    fn corrupt(&self, statements: &'static str) {
        self.store
            .write(move |db| {
                db.execute_batch(&format!(
                    "PRAGMA ignore_check_constraints=ON; DROP TRIGGER work_events_no_update;
                     {statements} PRAGMA ignore_check_constraints=OFF;"
                ))
                .map_err(OfficeError::storage)?;
                Ok(((), None))
            })
            .unwrap();
    }
}

/// The read fails as an inconsistent store (the cause is logged, never shown to a client).
fn assert_inconsistent<T: std::fmt::Debug>(result: Result<T, WorkActivityError>) {
    let Err(WorkActivityError::Store(error)) = result else {
        panic!("expected an inconsistent store, got {result:?}");
    };
    let cause = std::error::Error::source(&error).unwrap().to_string();
    assert!(cause.contains("inconsistent"), "{cause}");
}

// A target already read on this page whose next older version sits in a feed row newer than
// the one being read is a corrupt store: the history search must not paper over it.
#[test]
fn activity_refuses_a_seen_target_whose_older_version_is_in_a_newer_row() {
    let fx = setup(true);
    fx.commit(vec![
        vec![ev("T", T0)],
        vec![ev("T", T0 + 1)],
        vec![ev("U", T0)],
        vec![ev("U", T0 + 1)],
        vec![ev("T", T0 + 2)],
    ]);
    // Row 3 lists T, whose v2 claims row 4 (T v3 is read in row 5, so T is seen when row 3
    // is reached); a history search below row 3 would find v1 and hide the damage.
    fx.corrupt(
        "UPDATE work_feed SET targets = '[\"T\",\"U\"]' WHERE seq = 3;
         UPDATE work_events SET feed_seq = 4 WHERE target_id = 'T' AND version = 2;",
    );
    assert_inconsistent(fx.reads.activity_page(&query(5)));
}

// Reading a target's events in one feed row, an older version in a NEWER row than a newer
// version is a corrupt store: it must fail, not be read as one more event of the row.
#[test]
fn activity_refuses_an_older_version_stored_in_a_newer_feed_row_than_a_newer_one() {
    let fx = setup(true);
    fx.commit(vec![
        vec![ev("T", T0)],
        vec![ev("T", T0 + 1)],
        vec![ev("U", T0)],
    ]);
    // T v2 is in row 2 (the page's row); T v1 now claims row 3, above it.
    fx.corrupt("UPDATE work_events SET feed_seq = 3 WHERE target_id = 'T' AND version = 1;");
    assert_inconsistent(fx.reads.activity_page(&query(3)));
}

// The event JSON must name the very target and version of its row: either one wrong alone
// is a corrupt store.
#[test]
fn activity_refuses_an_event_whose_data_names_another_target_or_version() {
    for damage in [
        "UPDATE work_events SET data = json_set(data, '$.target_id', 'X');",
        "UPDATE work_events SET data = json_set(data, '$.version', 9);",
    ] {
        let fx = setup(true);
        fx.commit(vec![vec![ev("A", T0)]]);
        fx.corrupt(damage);
        assert_inconsistent(fx.reads.activity_page(&query(1)));
    }
}

// BUG-197: a ceiling is a head the client saw. One above this feed's head is a rollback or a
// restored backup (the org page says so too), not a page: it would change after the next commit.
#[test]
fn activity_refuses_a_ceiling_above_the_head_like_the_org_page_does() {
    let fx = setup(true);
    fx.commit(vec![vec![ev("A", T0)], vec![ev("A", T0 + 1)]]);
    let snapshot = |ceiling: u64| {
        matches!(
            fx.reads.activity_page(&query(ceiling)),
            Err(WorkActivityError::SnapshotRequired)
        )
    };
    // The head itself and below are pages; one above and far above are not.
    assert!(!snapshot(2) && !snapshot(1) && !snapshot(0));
    for ceiling in [3, 10, i64::MAX as u64, u64::MAX] {
        assert!(snapshot(ceiling), "ceiling {ceiling} above head 2");
    }
    // Once committed, the head moves and the same ceiling is a page: no page changes under it.
    fx.commit(vec![vec![ev("B", T0 + 2)]]);
    assert!(!snapshot(3));
    // An empty feed has head 0: ceiling 0 only.
    let empty = setup(true);
    assert!(empty.reads.activity_page(&query(0)).is_ok());
    assert!(matches!(
        empty.reads.activity_page(&query(1)),
        Err(WorkActivityError::SnapshotRequired)
    ));
}

/// 102 events, only the oldest matches: a filter applied after the cap would return nothing.
fn only_last_matches(fx: &Fx, last: Ev, rest: impl Fn(u64) -> Ev) -> WorkActivityPosition {
    let mut rows = vec![vec![last]];
    rows.extend((1..102).map(|n| vec![rest(n)]));
    fx.commit(rows)[0].clone()
}

fn assert_only(fx: &Fx, q: WorkActivityQuery, expected: &WorkActivityPosition) {
    let mut q = q;
    q.limit = 3;
    let page = fx.reads.activity_page(&q).unwrap();
    assert_eq!(positions(&page), std::slice::from_ref(expected));
    assert!(page.exhausted);
}

#[test]
fn activity_member_filter_runs_inside_the_scan_before_the_limit() {
    let fx = setup(true);
    let target = only_last_matches(&fx, ev("A", T0).assigned("m-x"), |n| {
        ev(&format!("I{n:03}"), T0 + n).assigned("m-y")
    });
    let mut q = query(102);
    q.member = Some("m-x".into());
    assert_only(&fx, q, &target);
}

#[test]
fn activity_team_filter_runs_inside_the_scan_before_the_limit() {
    let fx = setup(true);
    let target = only_last_matches(&fx, ev("A", T0).by("t-2"), |n| {
        ev(&format!("I{n:03}"), T0 + n).by("m-y")
    });
    let mut q = query(102);
    q.team_members = Some(vec!["t-1".into(), "t-2".into()]);
    assert_only(&fx, q, &target);
}

#[test]
fn activity_project_filter_runs_inside_the_scan_before_the_limit() {
    let fx = setup(true);
    let target = only_last_matches(&fx, ev("A", T0), |n| ev("B", T0 + n));
    fx.place("A", "p-x");
    fx.place("B", "p-y");
    let mut q = query(102);
    q.project = Some("p-x".into());
    assert_only(&fx, q, &target);
    // "Currently in": once A moves away, its history leaves the project's feed.
    fx.place("A", "p-y");
    let mut q = query(102);
    q.project = Some("p-x".into());
    assert!(fx.reads.activity_page(&q).unwrap().entries.is_empty());
}

#[test]
fn activity_member_and_team_filters_combine_inside_the_scan() {
    let fx = setup(true);
    // Member-only and team-only matches fill the newer rows; only the oldest has both.
    let target = only_last_matches(&fx, ev("A", T0).by("m-x").assigned("t-1"), |n| {
        let e = ev(&format!("I{n:03}"), T0 + n);
        if n % 2 == 0 {
            e.by("m-x")
        } else {
            e.by("t-1")
        }
    });
    let mut q = query(102);
    q.member = Some("m-x".into());
    q.team_members = Some(vec!["t-1".into()]);
    assert_only(&fx, q, &target);
}

#[test]
fn activity_scan_cap_counts_rows_not_matches() {
    let fx = setup(true);
    only_last_matches(&fx, ev("A", T0).by("m-x"), |n| {
        ev(&format!("I{n:03}"), T0 + n)
    });
    let mut q = query(102);
    (q.member, q.scan) = (Some("m-x".into()), 100);
    let page = fx.reads.activity_page(&q).unwrap();
    assert!(page.entries.is_empty() && !page.exhausted);
    assert_eq!(page.scanned_to, Some(pos(3, "I002", 1)));
    assert_eq!(page.frontier, T0 + 2);
}

#[test]
fn activity_zero_scan_and_limit_still_move_on() {
    let fx = setup(true);
    fx.commit(vec![vec![ev("A", T0)], vec![ev("A", T0 + 1)]]);
    let mut q = query(2);
    (q.scan, q.limit) = (0, 0);
    let page = fx.reads.activity_page(&q).unwrap();
    assert_eq!(positions(&page), [pos(2, "A", 2)]);
    assert_eq!(fx.drain(q).len(), 2);
}

#[test]
fn activity_truncates_to_the_last_returned_entry_when_matches_are_left_over() {
    let fx = setup(true);
    let at = [50, 40, 45, 30, 35];
    fx.commit(vec![at.iter().map(|n| ev("A", T0 + n)).collect()]);
    let mut q = query(1);
    q.limit = 2;
    let page = fx.reads.activity_page(&q).unwrap();
    assert_eq!(positions(&page), [pos(1, "A", 5), pos(1, "A", 4)]);
    assert!(!page.exhausted);
    assert_eq!(page.scanned_to, Some(pos(1, "A", 4)));
    assert_eq!(page.frontier, T0 + 30);
    q.before = page.scanned_to;
    q.prev_key = page.frontier;
    let next = fx.reads.activity_page(&q).unwrap();
    assert_eq!(positions(&next), [pos(1, "A", 3), pos(1, "A", 2)]);
    assert_eq!(next.entries[0].key, T0 + 30);
    q.before = next.scanned_to;
    q.prev_key = next.frontier;
    let last = fx.reads.activity_page(&q).unwrap();
    assert_eq!(positions(&last), [pos(1, "A", 1)]);
    assert!(last.exhausted);
}

#[test]
fn activity_truncation_keeps_the_frontier_at_the_last_returned_key() {
    // Newest first the keys run 50 then 20: the held-back match must not lower the frontier.
    let fx = setup(true);
    fx.commit(vec![vec![ev("A", T0 + 20), ev("A", T0 + 50)]]);
    let mut q = query(1);
    q.limit = 1;
    let page = fx.reads.activity_page(&q).unwrap();
    assert_eq!(positions(&page), [pos(1, "A", 2)]);
    assert!(!page.exhausted);
    assert_eq!(page.frontier, T0 + 50);
    (q.before, q.prev_key) = (page.scanned_to, page.frontier);
    let next = fx.reads.activity_page(&q).unwrap();
    assert_eq!(positions(&next), [pos(1, "A", 1)]);
    assert_eq!(next.entries[0].key, T0 + 20);
}

#[test]
fn activity_project_filter_keeps_a_deleted_item_in_its_project() {
    let fx = setup(true);
    fx.commit(vec![vec![ev("A", T0)]]);
    fx.place("A", "p-x");
    fx.store
        .write(|db| {
            db.execute(
                "UPDATE work_items SET data = json_set(data, '$.deleted', json('true')) WHERE id = 'A'",
                [],
            )
            .map_err(OfficeError::storage)?;
            Ok(((), None))
        })
        .unwrap();
    let mut q = query(1);
    q.project = Some("p-x".into());
    assert_eq!(
        positions(&fx.reads.activity_page(&q).unwrap()),
        [pos(1, "A", 1)]
    );
}

#[test]
fn activity_incarnation_mismatch_requires_a_snapshot() {
    let fx = setup(true);
    fx.commit(vec![vec![ev("A", T0)]]);
    let mut q = query(1);
    q.incarnation = Some(org_work::ulid_from_parts(T0, 2));
    assert!(matches!(
        fx.reads.activity_page(&q),
        Err(WorkActivityError::SnapshotRequired)
    ));
    // None → one is compatible only while the cursor's ceiling is 0.
    q.incarnation = None;
    assert!(matches!(
        fx.reads.activity_page(&q),
        Err(WorkActivityError::SnapshotRequired)
    ));
    q.ceiling = 0;
    let page = fx.reads.activity_page(&q).unwrap();
    assert!(page.entries.is_empty() && page.exhausted);
    let fresh = setup(false);
    let mut q = query(0);
    assert!(matches!(
        fresh.reads.activity_page(&q),
        Err(WorkActivityError::SnapshotRequired)
    ));
    q.incarnation = None;
    assert!(fresh.reads.activity_page(&q).unwrap().exhausted);
}

#[test]
fn activity_reads_never_write_or_make_an_incarnation() {
    let fx = setup(false);
    let before = fx.meta_and_sequences();
    assert_eq!(fx.reads.head().unwrap(), (None, 0));
    let mut q = query(0);
    q.incarnation = None;
    fx.reads.activity_page(&q).unwrap();
    fx.reads.latest_by_project(0, &["p".into()]).unwrap();
    assert_eq!(fx.meta_and_sequences(), before);
    // The read itself, even on the writer's connection, leaves `meta` alone.
    let made = fx
        .store
        .write(|db| {
            let changes = db.total_changes();
            let stored = origin::stored_incarnation(db)?;
            Ok(((stored, db.total_changes() - changes), None))
        })
        .unwrap();
    assert_eq!(made, (None, 0));
    assert_eq!(fx.meta_and_sequences(), before);
    // The pool it reads from refuses writes.
    let refused = fx.store.read(|db| {
        db.execute("INSERT INTO meta VALUES ('x', 'y')", [])
            .map_err(OfficeError::storage)
    });
    assert!(refused.is_err());
}

#[test]
fn activity_latest_by_project_is_the_newest_commit_order_event_per_project() {
    let fx = setup(true);
    for (item, project) in [
        ("A", "p1"),
        ("B", "p1"),
        ("C", "p2"),
        ("D", "p5"),
        ("E", "p5"),
    ] {
        fx.place(item, project);
    }
    fx.commit(vec![
        vec![ev("A", T0 + 100)],
        vec![ev("C", T0 + 300)],
        vec![ev("B", T0 + 200)],
        // A clock step: the newest in commit order wins, even though B's time is later.
        vec![ev("A", T0 + 50)],
        // A tie on the row: the higher target id comes first.
        vec![ev("D", T0 + 70), ev("E", T0 + 80)],
        vec![ev("C", T0 + 300)],
    ]);
    let projects: Vec<String> = ["p1", "p2", "p3", "p5"].map(String::from).into();
    let latest = fx.reads.latest_by_project(6, &projects).unwrap();
    let expected = [
        ("p1", At(T0 + 50)),
        ("p2", At(T0 + 300)),
        ("p3", LatestActivity::None),
        ("p5", At(T0 + 80)),
    ]
    .map(|(p, at)| (p.to_owned(), at));
    assert_eq!(latest, expected);
    // Each equals the first entry activity.list would return with that project.
    for (project, latest) in &latest {
        let mut q = query(6);
        q.project = Some(project.clone());
        let page = fx.reads.activity_page(&q).unwrap();
        let first = page.entries.first().map(|e| e.at_ms.map_or(Unknown, At));
        assert_eq!(first.unwrap_or(LatestActivity::None), *latest, "{project}");
    }
    // At a lower ceiling, later events are left out (A's history is searched below it).
    let lower = fx.reads.latest_by_project(3, &projects).unwrap();
    assert_eq!(lower[0], ("p1".to_owned(), At(T0 + 200)));
    assert_eq!(lower[3], ("p5".to_owned(), LatestActivity::None));
    let empty = fx.reads.latest_by_project(0, &projects).unwrap();
    assert_eq!(empty[1].1, LatestActivity::None);
}

#[test]
fn activity_latest_by_project_tells_an_unparseable_newest_at_from_no_event() {
    let fx = setup(true);
    fx.place("A", "p1");
    fx.place("B", "p1");
    fx.place("C", "p2");
    let mut broken = ev("B", T0);
    broken.at = "yesterday".into();
    // p1's newest event in commit order is B's, whose `at` doesn't parse: older parseable
    // events of A don't stand in for it.
    fx.commit(vec![
        vec![ev("A", T0 + 10)],
        vec![broken],
        vec![ev("C", T0 + 5)],
    ]);
    let projects: Vec<String> = ["p1", "p2", "p3"].map(String::from).into();
    let latest = fx.reads.latest_by_project(3, &projects).unwrap();
    let expected = [
        ("p1", Unknown),
        ("p2", At(T0 + 5)),
        ("p3", LatestActivity::None),
    ]
    .map(|(p, at)| (p.to_owned(), at));
    assert_eq!(latest, expected);
    // Below the broken event, A's time shows again.
    assert_eq!(
        fx.reads.latest_by_project(1, &projects).unwrap()[0].1,
        At(T0 + 10)
    );
}

#[test]
fn activity_before_must_be_a_stored_position_at_or_below_the_ceiling() {
    let fx = setup(true);
    fx.commit(vec![
        vec![ev("A", T0), ev("A", T0)],
        vec![ev("B", T0)],
        vec![ev("A", T0)],
        vec![ev("C", T0)],
    ]);
    let page = |before: WorkActivityPosition| {
        let mut q = query(3);
        q.before = Some(before);
        fx.reads.activity_page(&q)
    };
    let invalid = [
        // Above the ceiling, though stored.
        pos(4, "C", 1),
        // No such feed row.
        pos(0, "", 0),
        pos(9, "", 0),
        // A target not in that row, with or without a real version.
        pos(2, "A", 2),
        pos(2, "Z", 1),
        // A version above the target's versions at that row, or not stored at all.
        pos(1, "A", 3),
        pos(1, "A", 9),
        // A version below them, committed in another row.
        pos(3, "A", 1),
        // The start-of-row marker carries version 0 only.
        pos(2, "", 1),
        // Integers SQLite can't hold.
        pos(u64::MAX, "A", 1),
        pos(1, "A", u64::MAX),
    ];
    for before in invalid {
        assert!(
            matches!(
                page(before.clone()),
                Err(WorkActivityError::InvalidPosition)
            ),
            "{before:?}"
        );
    }
    // Stored positions and row starts page on.
    assert_eq!(
        positions(&page(pos(3, "A", 3)).unwrap()),
        [pos(2, "B", 1), pos(1, "A", 2), pos(1, "A", 1)]
    );
    assert_eq!(positions(&page(pos(1, "A", 2)).unwrap()), [pos(1, "A", 1)]);
    assert_eq!(
        positions(&page(pos(2, "", 0)).unwrap()),
        [pos(1, "A", 2), pos(1, "A", 1)]
    );
    // An incarnation mismatch is reported first.
    let mut q = query(3);
    (q.incarnation, q.before) = (None, Some(pos(9, "", 0)));
    assert!(matches!(
        fx.reads.activity_page(&q),
        Err(WorkActivityError::SnapshotRequired)
    ));
}

/// Freeze decision 7's budgets (rev 4.8; release, Mac). Run on a quiet Mac (1-minute load
/// below 6) or in the heavy lane: under load the timings run up to 1.5x slow (retro rowan-8).
/// `~/.agency/bin/heavy cargo test -p org-office --release -- --ignored activity_budget --nocapture`
/// The budget assertions run in release builds only.
#[test]
#[ignore = "benchmark: 100,000 events; run in release"]
fn activity_budget_100k_events_2000_items_one_hot_item() {
    let fx = setup(true);
    let projects: Vec<String> = (0..7).map(|p| format!("p{p}")).collect();
    let pad = "x".repeat(1500);
    fx.store
        .write(move |db| {
            // 2,000 items with 45 events each, plus H with 10,000: one event per feed row.
            for n in 0..100_000u64 {
                let target = if n % 10 == 9 {
                    "H".to_owned()
                } else {
                    format!("I{:04}", (n - n / 10) % 2000)
                };
                let seq: i64 = db
                    .query_row(
                        "INSERT INTO work_feed(targets) VALUES (?1) RETURNING seq",
                        [serde_json::to_string(&[&target]).unwrap()],
                        |r| r.get(0),
                    )
                    .map_err(OfficeError::storage)?;
                insert_event(db, &ev(&target, T0 + n), seq)?;
            }
            // Items as the store keeps them: about 1.5 KB of JSON each (a typical description).
            db.execute(
                "UPDATE work_items SET data = json_set(data, '$.project_id',
                     'p' || (unicode(substr(id, -1)) % 7), '$.description', ?1)",
                [&pad],
            )
            .map_err(OfficeError::storage)?;
            Ok(((), None))
        })
        .unwrap();
    let time = |what: &str, f: &dyn Fn()| {
        f();
        let start = Instant::now();
        for _ in 0..10 {
            f();
        }
        let ms = start.elapsed().as_secs_f64() * 100.0;
        println!("{what}: {ms:.2} ms");
        ms
    };
    let mut nothing = query(100_000);
    nothing.member = Some("nobody".into());
    let page = time("selective page, no match, scan 2000", &|| {
        let page = fx.reads.activity_page(&nothing).unwrap();
        assert!(page.entries.is_empty() && !page.exhausted);
    });
    let mut elsewhere = query(100_000);
    elsewhere.project = Some("p-none".into());
    let project_page = time("project page, no match, scan 2000", &|| {
        let page = fx.reads.activity_page(&elsewhere).unwrap();
        assert!(page.entries.is_empty() && !page.exhausted);
    });
    // Continuation pages: every target is first seen below its newest event, so each one is
    // found by searching its history (more B-tree pages than at the head).
    let mut deep = nothing.clone();
    deep.before = Some(pos(50_000, "", 0));
    let deep_page = time("selective page mid-history, no match, scan 2000", &|| {
        assert!(fx.reads.activity_page(&deep).unwrap().entries.is_empty());
    });
    let mut deep_project = elsewhere.clone();
    deep_project.before = Some(pos(50_000, "", 0));
    let deep_project_page = time("project page mid-history, no match, scan 2000", &|| {
        assert!(fx
            .reads
            .activity_page(&deep_project)
            .unwrap()
            .entries
            .is_empty());
    });
    let mut hot = query(100_000);
    hot.before = Some(pos(50_000, "H", 5_000));
    hot.limit = 50;
    let resume = time("hot item mid-row resume, limit 50", &|| {
        assert_eq!(fx.reads.activity_page(&hot).unwrap().entries.len(), 50);
    });
    // Row 50,001 holds an ordinary item: H is unseen there, so its version at row 50,000 is
    // found by searching its 10,000 versions.
    let mut searched = query(100_000);
    searched.before = Some(pos(50_001, "", 0));
    searched.limit = 50;
    let search = time("hot item unseen on the page, limit 50", &|| {
        let page = fx.reads.activity_page(&searched).unwrap();
        assert_eq!(page.entries[0].position, pos(50_000, "H", 5_000));
    });
    let recency = time("latest_by_project, 7 projects", &|| {
        let latest = fx.reads.latest_by_project(100_000, &projects).unwrap();
        assert!(latest.iter().all(|(_, at)| matches!(at, At(_))));
    });
    if !cfg!(debug_assertions) {
        assert!(
            page <= 20.0 && resume <= 20.0 && search <= 20.0,
            "page budget"
        );
        // Each item's whole row is read for its project: no item-to-project index without DDL.
        assert!(project_page <= 40.0, "project page budget");
        assert!(deep_page <= 40.0, "continuation page budget");
        assert!(
            deep_project_page <= 60.0,
            "project continuation page budget"
        );
        assert!(recency <= 30.0, "recency budget");
    }
}
