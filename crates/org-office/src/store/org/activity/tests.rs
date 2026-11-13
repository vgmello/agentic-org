use std::time::Instant;

use org_proto::org::OrgChange;
use serde_json::{json, Value};
use tempfile::TempDir;

use super::*;
use crate::key::Key;
use crate::organization::activity::TeamFilter;
use crate::store::org::{genesis, Genesis, SCHEMA};
use crate::store::{Opening, Store};

/// Log rows to append: each one's `at` and changes.
type Rows = Vec<(String, Vec<OrgChange>)>;

const T0: u64 = 1_794_139_200_000; // 2026-11-08T12:00:00.000Z
const OWNER: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const SECRET: &str = "SENTINEL-free-text";

fn at(ms: u64) -> String {
    time::format_epoch_ms(ms)
}

fn store() -> (TempDir, Store) {
    let dir = TempDir::new().unwrap();
    let store = Store::open(
        &dir.path().join("org.db"),
        &Key::from_bytes([5; 32]),
        Opening::New,
        &SCHEMA,
    )
    .unwrap();
    let genesis_row = Genesis {
        org_id: "ORG1".into(),
        org_name: "Acme".into(),
        created_at: at(T0),
        owner_id: OWNER.into(),
    };
    store
        .write(move |db| genesis(db, &genesis_row).map(|_| ((), None)))
        .unwrap();
    (dir, store)
}

fn change(value: Value) -> OrgChange {
    serde_json::from_value(value).unwrap()
}
fn team(id: &str, version: u64) -> OrgChange {
    change(json!({"type": "team_upserted", "team": {"id": id, "name": SECRET, "version": version}}))
}
fn member(id: &str, version: u64) -> OrgChange {
    change(
        json!({"type": "member_upserted", "member": {"id": id, "kind": "agent",
        "name": SECRET, "contact": SECRET, "version": version,
        "agent": {"role": SECRET, "title": SECRET, "focus": SECRET, "runtime": "claude_code", "messaging_name": "m"}}}),
    )
}
fn membership(id: &str, team: &str, member: &str, version: u64) -> OrgChange {
    change(
        json!({"type": "membership_upserted", "membership": {"id": id, "team_id": team,
        "member_id": member, "title_in_team": SECRET, "version": version}}),
    )
}
fn project_team(id: &str, project: &str, team: &str, version: u64) -> OrgChange {
    change(
        json!({"type": "project_team_added", "project_team": {"id": id,
        "project_id": project, "team_id": team, "version": version}}),
    )
}
fn project(id: &str, version: u64, archived: bool) -> OrgChange {
    change(
        json!({"type": "project_upserted", "project": {"id": id, "key": "pk", "name": SECRET,
        "description": SECRET, "archived": archived, "version": version}}),
    )
}
fn area(id: &str, project: &str, version: u64, deleted: bool) -> OrgChange {
    change(
        json!({"type": "area_upserted", "area": {"id": id, "project_id": project,
        "name": SECRET, "team_id": "TX", "deleted": deleted, "version": version}}),
    )
}
fn removed(kind: &str, id: &str) -> OrgChange {
    let (tag, field) = match kind {
        "team" => ("team_deleted", "team_id"),
        "member" => ("member_removed", "member_id"),
        "membership" => ("membership_removed", "membership_id"),
        _ => ("project_team_removed", "project_team_id"),
    };
    change(json!({"type": tag, field: id}))
}

/// Appends one log row per item, each its own command `C<seq>`, in one write.
fn append(store: &Store, rows: Rows) {
    store
        .write(move |db| {
            for (at, changes) in &rows {
                let seq = super::super::head(db)?.seq + 1;
                super::super::append_event(db, &format!("C{seq}"), at, changes)?;
            }
            Ok(((), None))
        })
        .unwrap();
}

/// `n` filler rows: one team change each, times rising from `T0 + 1s`.
fn filler(n: usize) -> Rows {
    (0..n)
        .map(|i| (at(T0 + 1_000 + i as u64), vec![team(&format!("F{i}"), 2)]))
        .collect()
}

fn query(ceiling: u64) -> OrgActivityQuery {
    OrgActivityQuery {
        epoch: 1,
        ceiling,
        before: None,
        prev_key: u64::MAX,
        member: None,
        team: None,
        project: None,
        limit: 50,
        scan: 2_000,
    }
}

fn head_seq(store: &Store) -> u64 {
    store.read(super::super::head).unwrap().seq
}

fn read(store: &Store, query: &OrgActivityQuery) -> Result<ActivityPage, ActivityError> {
    store.read(|db| Ok(page(db, query))).unwrap()
}

fn latest(store: &Store, ids: &[&str]) -> ProjectActivity {
    let ids: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
    let ceiling = store.read(super::super::head).unwrap();
    store
        .read(|db| Ok(latest_by_project(db, ceiling, &ids)))
        .unwrap()
        .unwrap()
}

/// Every entry, paging until exhausted and feeding each page's position and key back.
fn all(store: &Store, query: OrgActivityQuery) -> Vec<ActivityEntry> {
    paged(store, query, usize::MAX)
}

/// [`all`] in at most `max` pages, each stopping strictly below the previous one: a resume that
/// does not move on fails instead of looping.
fn paged(store: &Store, mut query: OrgActivityQuery, max: usize) -> Vec<ActivityEntry> {
    let mut entries = Vec::new();
    for _ in 0..max {
        let page = read(store, &query).unwrap();
        entries.extend(page.entries);
        if page.exhausted {
            return entries;
        }
        let below = |before| page.scanned_to < Some(before);
        let moved = page.scanned_to.is_some() && query.before.is_none_or(below);
        assert!(
            moved,
            "a page resumed at {:?} stopped at {:?}",
            query.before, page.scanned_to
        );
        query.before = page.scanned_to;
        query.prev_key = page.frontier;
    }
    panic!("not exhausted in {max} pages");
}

fn verbs(entries: &[ActivityEntry]) -> Vec<&str> {
    entries.iter().map(|e| e.verb.as_str()).collect()
}

fn position(seq: u64, index: usize) -> ActivityPosition {
    ActivityPosition {
        epoch: 1,
        seq,
        index,
    }
}

fn subjects(entries: &[ActivityEntry]) -> Vec<(&str, [Option<&str>; 3])> {
    entries
        .iter()
        .map(|e| {
            let s = &e.subject;
            let ids = [s.member.as_deref(), s.team.as_deref(), s.project.as_deref()];
            (e.verb.as_str(), ids)
        })
        .collect()
}

const NONE: [Option<&str>; 3] = [None, None, None];

// ---- verbs and subjects ----------------------------------------------------------------------

#[test]
fn verbs_and_subjects_follow_the_change_and_its_entitys_previous_upsert() {
    let (_dir, store) = store();
    append(
        &store,
        vec![
            (
                at(T0 + 1),
                vec![
                    team("T1", 1),
                    member("M1", 1),
                    membership("S1", "T1", "M1", 1),
                    project_team("Q1", "P1", "T1", 1),
                    project("P1", 1, false),
                    area("A1", "P1", 1, false),
                ],
            ),
            (
                at(T0 + 2),
                vec![
                    team("T1", 2),
                    member("M1", 2),
                    membership("S1", "T1", "M1", 2),
                    project_team("Q1", "P1", "T1", 2),
                    project("P1", 2, true),
                    area("A1", "P1", 2, false),
                ],
            ),
            // A rename of an archived project; legacy version 0 is never `added`, but flips.
            (
                at(T0 + 3),
                vec![
                    project("P1", 3, true),
                    project("P0", 0, false),
                    project("P0", 0, true),
                ],
            ),
            (
                at(T0 + 4),
                vec![project("P1", 4, false), area("A1", "P1", 3, true)],
            ),
            (
                at(T0 + 5),
                vec![
                    removed("membership", "S1"),
                    removed("project_team", "Q1"),
                    removed("team", "T1"),
                    removed("member", "M1"),
                    removed("membership", "GHOST"),
                    change(json!({"type": "a_kind_from_the_future"})),
                ],
            ),
        ],
    );
    let entries = all(&store, query(6));
    let (m1, t1, p1) = (Some("M1"), Some("T1"), Some("P1"));
    assert_eq!(
        subjects(&entries[..entries.len() - 1]),
        vec![
            ("org.changed", NONE),
            ("org.membership.removed", NONE),
            ("org.member.removed", [m1, None, None]),
            ("org.team.removed", [None, t1, None]),
            ("org.project_team.removed", [None, t1, p1]),
            ("org.membership.removed", [m1, t1, None]),
            ("org.area.removed", [None, None, p1]),
            ("org.project.unarchived", [None, None, p1]),
            ("org.project.archived", [None, None, Some("P0")]),
            ("org.project.changed", [None, None, Some("P0")]),
            ("org.project.changed", [None, None, p1]),
            ("org.area.changed", [None, None, p1]),
            ("org.project.archived", [None, None, p1]),
            ("org.project_team.changed", [None, t1, p1]),
            ("org.membership.changed", [m1, t1, None]),
            ("org.member.changed", [m1, None, None]),
            ("org.team.changed", [None, t1, None]),
            ("org.area.added", [None, None, p1]),
            ("org.project.added", [None, None, p1]),
            ("org.project_team.added", [None, t1, p1]),
            ("org.membership.added", [m1, t1, None]),
            ("org.member.added", [m1, None, None]),
            ("org.team.added", [None, t1, None]),
        ]
    );
    assert_eq!(
        entries.last().unwrap().verb,
        "org.member.added",
        "the genesis owner"
    );
}

#[test]
fn a_team_or_member_removal_is_about_its_own_id() {
    let (_dir, store) = store();
    let mut rows = vec![(at(T0 + 1), vec![team("TFAR", 1), member("MFAR", 1)])];
    rows.extend(filler(2_001));
    rows.push((
        at(T0 + 5_000_000),
        vec![removed("team", "TFAR"), removed("member", "MFAR")],
    ));
    append(&store, rows);
    let page = read(&store, &query(head_seq(&store))).unwrap();
    assert_eq!(
        subjects(&page.entries[..2]),
        [
            ("org.member.removed", [Some("MFAR"), None, None]),
            ("org.team.removed", [None, Some("TFAR"), None]),
        ]
    );
}

#[test]
fn no_entry_carries_free_text() {
    let (_dir, store) = store();
    append(&store, mixed(300));
    let entries = all(&store, query(301));
    assert_eq!(entries.len(), 1 + 500);
    assert!(!format!("{entries:?}").contains(SECRET));
}

// ---- order -----------------------------------------------------------------------------------

#[test]
fn entries_run_in_commit_order_grouped_by_command_with_its_principal_as_actor() {
    let (_dir, store) = store();
    append(
        &store,
        vec![
            (
                at(T0 + 1),
                vec![
                    member("M1", 1),
                    team("T1", 1),
                    membership("S1", "T1", "M1", 1),
                ],
            ),
            // A member removal's cascade: its membership, then the member.
            (
                at(T0 + 2),
                vec![removed("membership", "S1"), removed("member", "M1")],
            ),
        ],
    );
    store.run_sql_for_test(
        "INSERT INTO commands (command_id, epoch, seq, expires_at, principal_member)
         VALUES ('C3', 1, 3, '2026-12-08T12:00:00.000Z', 'M9')",
    );
    let entries = all(&store, query(3));
    let positions: Vec<_> = entries.iter().map(|e| e.position).collect();
    assert!(positions.windows(2).all(|pair| pair[0] > pair[1]));
    assert_eq!(&positions[..2], [position(3, 1), position(3, 0)]);
    assert_eq!(
        verbs(&entries[..2]),
        ["org.member.removed", "org.membership.removed"]
    );
    assert!(entries[..2]
        .iter()
        .all(|e| e.group == "C3" && e.actor.as_deref() == Some("M9")));
    assert!(entries[2..5]
        .iter()
        .all(|e| e.group == "C2" && e.actor.is_none()));
    assert_eq!(entries[1].subject.team.as_deref(), Some("T1"));
}

#[test]
fn times_are_the_recorded_at_and_keys_never_rise_across_a_clock_step() {
    let (_dir, store) = store();
    let ats = [
        at(T0 + 10_000),
        at(T0 + 5_000),
        at(T0 + 20_000),
        "not a time".into(),
        at(T0 + 30_000),
    ];
    append(
        &store,
        ats.into_iter()
            .map(|at| (at, vec![team("T1", 2)]))
            .collect(),
    );
    let page = read(&store, &query(6)).unwrap();
    assert!(page.exhausted);
    let times: Vec<_> = page.entries.iter().map(|e| (e.at_ms, e.key)).collect();
    let ms = |s: u64| T0 + s * 1_000;
    assert_eq!(
        times,
        [
            (Some(ms(30)), ms(30)),
            (None, ms(30)),
            (Some(ms(20)), ms(20)),
            (Some(ms(5)), ms(5)),
            (Some(ms(10)), ms(5)),
            (Some(T0), T0),
        ]
    );
    // A page continues from the given key.
    let mut q = query(6);
    (q.before, q.prev_key) = (Some(position(5, 0)), ms(1));
    let page = read(&store, &q).unwrap();
    assert_eq!(page.entries[0].key, ms(1));
    assert_eq!(page.entries.last().unwrap().key, T0);
}

#[test]
fn a_filtered_out_row_with_a_bad_time_leaves_the_returned_entries_alone() {
    let (_dir, store) = store();
    append(
        &store,
        vec![
            (at(T0 + 10_000), vec![project("P7", 2, false)]),
            ("not a time".into(), vec![team("T5", 2)]),
            (at(T0 + 20_000), vec![project("P7", 3, false)]),
        ],
    );
    let everything = read(&store, &query(4)).unwrap().entries;
    assert_eq!(everything[1].at_ms, None, "the bad row's own entry");
    let mut q = query(4);
    q.project = Some("P7".into());
    let filtered = read(&store, &q).unwrap().entries;
    let expected: Vec<_> = everything
        .into_iter()
        .filter(|e| e.subject.project.as_deref() == Some("P7"))
        .collect();
    assert_eq!(filtered, expected);
    let times: Vec<_> = filtered.iter().map(|e| (e.at_ms, e.key)).collect();
    let ms = |s: u64| T0 + s * 1_000;
    assert_eq!(times, [(Some(ms(20)), ms(20)), (Some(ms(10)), ms(10))]);
}

// ---- snapshot --------------------------------------------------------------------------------

#[test]
fn rows_appended_after_page_one_stay_above_its_ceiling() {
    let (_dir, store) = store();
    append(&store, filler(5));
    let mut q = query(head_seq(&store));
    let before: Vec<_> = all(&store, q.clone());
    q.limit = 2;
    let first = read(&store, &q).unwrap();
    append(&store, filler(3));
    assert_eq!(head_seq(&store), 9);
    assert_eq!(read(&store, &q).unwrap(), first);
    (q.before, q.prev_key) = (first.scanned_to, first.frontier);
    let mut paged = first.entries;
    paged.extend(all(&store, q));
    assert_eq!(paged, before);
    assert!(paged.iter().all(|e| e.position.seq <= 6));
}

#[test]
fn another_epoch_a_rollback_or_an_unretained_position_needs_a_snapshot() {
    let (_dir, store) = store();
    append(&store, filler(5));
    let snapshot =
        |q: &OrgActivityQuery| matches!(read(&store, q), Err(ActivityError::SnapshotRequired));
    let latest_at = |ceiling: Position| {
        store
            .read(|db| Ok(latest_by_project(db, ceiling, &["P".into()])))
            .unwrap()
    };
    let mut q = query(6);
    assert!(!snapshot(&q) && !read(&store, &q).unwrap().history_truncated);
    q.epoch = 2;
    assert!(snapshot(&q));
    q = query(6);
    q.before = Some(ActivityPosition {
        epoch: 2,
        seq: 4,
        index: 0,
    });
    assert!(snapshot(&q));
    assert!(matches!(
        latest_at(Position { epoch: 2, seq: 6 }),
        Err(ActivityError::SnapshotRequired)
    ));
    // A rollback below the ceiling.
    store.run_sql_for_test("DELETE FROM events WHERE seq > 4");
    assert!(snapshot(&query(6)) && !snapshot(&query(4)));
    assert!(matches!(
        latest_at(Position { epoch: 1, seq: 6 }),
        Err(ActivityError::SnapshotRequired)
    ));
    assert!(latest_at(Position { epoch: 1, seq: 4 }).is_ok());
    // A position below the oldest retained row; the genesis is gone too.
    store.run_sql_for_test("DELETE FROM events WHERE seq < 3");
    q = query(4);
    q.before = Some(position(2, 1));
    assert!(snapshot(&q));
    q.before = Some(position(5, 1));
    assert!(snapshot(&q), "a position above the ceiling");
    q.before = Some(position(3, 1));
    let page = read(&store, &q).unwrap();
    assert_eq!(
        page.entries.iter().map(|e| e.position).collect::<Vec<_>>(),
        [position(3, 0)]
    );
    assert!(page.exhausted && page.history_truncated);
    // The ceiling row itself is a valid resume position, inside it or below it.
    let resumed = |before| {
        let mut q = query(4);
        q.before = Some(before);
        let page = read(&store, &q).unwrap();
        page.entries.iter().map(|e| e.position).collect::<Vec<_>>()
    };
    assert_eq!(resumed(position(4, 1)), [position(4, 0), position(3, 0)]);
    assert_eq!(resumed(position(4, 0)), [position(3, 0)]);
}

#[test]
fn an_installed_snapshot_truncates_the_history() {
    use crate::rules::OrgCommand;
    use crate::{NewOffice, Office, OfficeRole, SnapshotStageMode};
    use org_proto::org::TeamCreateParams;
    use org_sync::organization::{self, PeerIdentity, PeerRole, VerifiedPeer};
    use org_sync_proto::{Digest, Epoch, OfficeId, OrgId};

    struct Hq;
    impl VerifiedPeer for Hq {
        fn identity(&self) -> PeerIdentity {
            PeerIdentity {
                org_id: OrgId::new("ORG").unwrap(),
                office_id: OfficeId::new("HQ").unwrap(),
                role: PeerRole::Hq,
                authority: Epoch::new(1).unwrap(),
            }
        }
    }
    const MAX: u64 = 1024 * 1024;
    let dir = TempDir::new().unwrap();
    let office = |id: &str| {
        let new = NewOffice {
            office_id: id.into(),
            office_name: id.into(),
            role: OfficeRole::Hq,
            org_id: "ORG".into(),
            org_name: "Organization".into(),
        };
        Office::create(dir.path().join(id), &new, Key::from_bytes([3; 32])).unwrap()
    };
    let first_page = |office: &Office| {
        let head = office.org_head().unwrap();
        let mut q = query(head.seq);
        q.epoch = head.epoch;
        office.org_activity(&q).unwrap()
    };
    let hq = office("HQ");
    hq.bind_replication_hq(Digest::ZERO, MAX).unwrap();
    hq.submit_org(OrgCommand::TeamCreate(TeamCreateParams {
        command_id: crate::ulid::generate().unwrap(),
        team_id: "team".into(),
        name: SECRET.into(),
        owner_team_id: None,
    }))
    .unwrap()
    .unwrap();
    let page = first_page(&hq);
    assert!(!page.history_truncated && page.exhausted);
    assert_eq!(verbs(&page.entries)[0], "org.team.added");

    let satellite = office("SAT");
    let (hq_id, one) = (OfficeId::new("HQ").unwrap(), Epoch::new(1).unwrap());
    satellite
        .bind_replication_satellite(hq_id, Digest::ZERO, one, Digest::ZERO)
        .unwrap();
    let transport = Key::from_bytes([8; 32]);
    let exported = hq.export_replication_snapshot(&transport, MAX).unwrap();
    let staged = satellite
        .stage_replication_snapshot(
            &Hq,
            &exported.manifest,
            &transport,
            exported.ciphertext.as_slice(),
            MAX,
            SnapshotStageMode::Normal,
        )
        .unwrap();
    organization::install_snapshot(&satellite, staged).unwrap();
    assert_eq!(satellite.org_head().unwrap(), hq.org_head().unwrap());
    let page = first_page(&satellite);
    assert!(page.history_truncated && page.exhausted && page.entries.is_empty());
    // Nothing is retained below the head, so no position can resume.
    let head = satellite.org_head().unwrap();
    let mut q = query(head.seq);
    q.before = Some(position(head.seq, 0));
    assert!(matches!(
        satellite.org_activity(&q),
        Err(ActivityError::SnapshotRequired)
    ));
    let recency = satellite.org_latest_by_project(satellite.org_head().unwrap(), &["P".into()]);
    assert_eq!(recency.unwrap(), ProjectActivity::default());
}

#[test]
fn another_epochs_first_row_is_not_the_genesis() {
    let (_dir, store) = store();
    append(&store, filler(2));
    store.run_sql_for_test("UPDATE events SET epoch = 2");
    let mut q = query(3);
    q.epoch = 2;
    let page = read(&store, &q).unwrap();
    assert!(page.history_truncated && page.exhausted);
    assert_eq!(page.entries.len(), 3);
}

#[test]
fn rows_of_another_event_kind_are_not_read() {
    let (_dir, store) = store();
    append(&store, filler(1));
    store.run_sql_for_test(
        "INSERT INTO events (epoch, seq, kind, command_id, at, body)
         VALUES (1, 3, 'a.kind.from.the.future', 'X3', 'not a time', 'not json')",
    );
    append(&store, filler(1));
    let entries = all(&store, query(4));
    let seqs: Vec<_> = entries.iter().map(|e| e.position.seq).collect();
    assert_eq!(seqs, [4, 2, 1]);
}

#[test]
fn a_storage_error_drops_the_reader_and_a_snapshot_answer_keeps_it() {
    use crate::{NewOffice, Office, OfficeRole};
    let dir = TempDir::new().unwrap();
    let new = NewOffice {
        office_id: "HQ".into(),
        office_name: "HQ".into(),
        role: OfficeRole::Hq,
        org_id: "ORG".into(),
        org_name: "Organization".into(),
    };
    let office = Office::create(dir.path().join("HQ"), &new, Key::from_bytes([3; 32])).unwrap();
    let head = office.org_head().unwrap();
    let mut q = query(head.seq);
    (q.epoch, q.before) = (head.epoch + 1, None);
    let idle = |query: &OrgActivityQuery| {
        office.org_store().idle_readers().clear();
        let answer = office.org_activity(query);
        (answer, office.org_store().idle_readers().len())
    };
    let (answer, readers) = idle(&q);
    assert!(matches!(answer, Err(ActivityError::SnapshotRequired)) && readers == 1);
    office
        .org_store()
        .run_sql_for_test("UPDATE events SET body = 'not json'");
    q.epoch = head.epoch;
    let (answer, readers) = idle(&q);
    assert!(matches!(answer, Err(ActivityError::Store(_))), "{answer:?}");
    assert_eq!(readers, 0, "the reader that failed is not reused");
}

// ---- filters ---------------------------------------------------------------------------------

/// `rows` as the oldest after the genesis, under 101 filler rows; a limit-3 page of `filter`.
fn filtered(rows: Rows, filter: impl FnOnce(&mut OrgActivityQuery)) -> ActivityPage {
    let (_dir, store) = store();
    let mut all_rows = rows;
    all_rows.extend(filler(101));
    append(&store, all_rows);
    store.run_sql_for_test(
        "INSERT INTO commands (command_id, epoch, seq, expires_at, principal_member)
         VALUES ('C2', 1, 2, '2026-12-08T12:00:00.000Z', 'MA')",
    );
    let mut q = query(head_seq(&store));
    q.limit = 3;
    filter(&mut q);
    read(&store, &q).unwrap()
}

fn team_filter(id: &str, members: &[&str]) -> Option<TeamFilter> {
    let members = members.iter().map(|m| m.to_string()).collect();
    Some(TeamFilter {
        id: id.into(),
        members,
    })
}

#[test]
fn filters_apply_inside_the_scan_before_the_limit() {
    let row = |changes| vec![(at(T0 + 1), changes)];
    type Case = (&'static str, Rows, Box<dyn FnOnce(&mut OrgActivityQuery)>);
    let cases: Vec<Case> = vec![
        (
            "project",
            row(vec![project("P7", 1, false)]),
            Box::new(|q| q.project = Some("P7".into())),
        ),
        (
            "member subject",
            row(vec![member("M7", 1)]),
            Box::new(|q| q.member = Some("M7".into())),
        ),
        (
            "member actor",
            row(vec![team("T5", 2)]),
            Box::new(|q| q.member = Some("MA".into())),
        ),
        (
            "membership member",
            row(vec![membership("S7", "T7", "M8", 1)]),
            Box::new(|q| q.member = Some("M8".into())),
        ),
        (
            "team subject",
            row(vec![membership("S7", "T7", "M8", 1)]),
            Box::new(|q| q.team = team_filter("T7", &[])),
        ),
        (
            "team member set",
            row(vec![member("M7", 2)]),
            Box::new(|q| q.team = team_filter("T9", &["M0", "M7"])),
        ),
        (
            "team actor in set",
            row(vec![team("T5", 2)]),
            Box::new(|q| q.team = team_filter("T9", &["MA"])),
        ),
        (
            "member and team",
            vec![
                (at(T0 + 1), vec![membership("S7", "T7", "M7", 1)]),
                (at(T0 + 2), vec![member("M7", 2)]),
                (at(T0 + 3), vec![team("T7", 2)]),
            ],
            Box::new(|q| (q.member, q.team) = (Some("M7".into()), team_filter("T7", &[]))),
        ),
    ];
    for (name, rows, filter) in cases {
        let page = filtered(rows, filter);
        let found: Vec<_> = page.entries.iter().map(|e| e.position).collect();
        assert_eq!(found, [position(2, 0)], "{name}");
        assert!(page.exhausted, "{name}");
    }
}

#[test]
fn a_capped_page_without_matches_is_empty_but_not_exhausted() {
    let rows = vec![(at(T0 + 1), vec![project("P7", 1, false)])];
    let page = filtered(rows.clone(), |q| {
        (q.project, q.scan) = (Some("P7".into()), 10)
    });
    assert!(page.entries.is_empty() && !page.exhausted);
    // Rows 103 down to 94: filler 100 to 91, the oldest at T0 + 1,091 ms.
    assert_eq!(page.scanned_to, Some(position(94, 0)));
    assert_eq!(page.frontier, T0 + 1_091);
    let (_dir, store) = store();
    let mut all_rows = rows;
    all_rows.extend(filler(101));
    append(&store, all_rows);
    let mut q = query(103);
    (q.project, q.scan) = (Some("P7".into()), 10);
    assert_eq!(
        all(&store, q)
            .iter()
            .map(|e| e.position)
            .collect::<Vec<_>>(),
        [position(2, 0)]
    );
}

#[test]
fn a_scan_that_reads_exactly_the_remaining_rows_is_exhausted() {
    let (_dir, store) = store();
    append(&store, filler(3));
    let rows = head_seq(&store) as usize;
    let mut q = query(head_seq(&store));
    q.scan = rows;
    let page = read(&store, &q).unwrap();
    assert!(page.exhausted, "every retained row fits the scan");
    assert_eq!(page.scanned_to, Some(position(1, 0)));
    q.scan = rows - 1;
    let page = read(&store, &q).unwrap();
    assert!(!page.exhausted, "the genesis row is left for the next page");
    assert_eq!(page.scanned_to, Some(position(2, 0)));
}

#[test]
fn more_matches_than_the_limit_end_the_page_at_its_last_entry() {
    let (_dir, store) = store();
    let mut rows: Vec<_> = (0..5)
        .map(|i| (at(T0 + 10 - i), vec![project("P7", 2 + i, false)]))
        .collect();
    // One row with three matches, so a page can end inside it.
    rows.push((
        at(T0 + 20),
        vec![
            area("A7", "P7", 1, false),
            project_team("Q7", "P7", "T1", 1),
            project("P7", 9, false),
        ],
    ));
    rows.extend(filler(3));
    append(&store, rows);
    let mut q = query(head_seq(&store));
    (q.project, q.limit) = (Some("P7".into()), 2);
    let page = read(&store, &q).unwrap();
    assert_eq!(
        page.entries.iter().map(|e| e.position).collect::<Vec<_>>(),
        [position(7, 2), position(7, 1)]
    );
    assert!(!page.exhausted);
    assert_eq!(
        (page.scanned_to, page.frontier),
        (Some(position(7, 1)), page.entries[1].key)
    );
    (q.before, q.prev_key, q.limit) = (page.scanned_to, page.frontier, 3);
    let page = read(&store, &q).unwrap();
    let found: Vec<_> = page.entries.iter().map(|e| e.position).collect();
    assert_eq!(found, [position(7, 0), position(6, 0), position(5, 0)]);
    assert_eq!(
        (page.scanned_to, page.frontier, page.exhausted),
        (Some(position(5, 0)), T0 + 6, false)
    );
    (q.before, q.prev_key) = (page.scanned_to, page.frontier);
    let page = read(&store, &q).unwrap();
    assert_eq!(
        page.entries
            .iter()
            .map(|e| e.position.seq)
            .collect::<Vec<_>>(),
        [4, 3, 2]
    );
    assert!(page.exhausted);
}

#[test]
fn a_page_past_its_limit_keeps_its_last_entrys_key_when_older_times_fall() {
    let (_dir, store) = store();
    append(
        &store,
        vec![
            (at(T0 + 50), vec![project("P7", 2, false)]),
            (at(T0 + 1), vec![team("T1", 2)]),
            (at(T0 + 40), vec![project("P7", 3, false)]),
            (at(T0 + 30), vec![project("P7", 4, false)]),
        ],
    );
    let mut q = query(5);
    (q.project, q.limit) = (Some("P7".into()), 2);
    let page = read(&store, &q).unwrap();
    let found: Vec<_> = page.entries.iter().map(|e| (e.position, e.key)).collect();
    assert_eq!(
        found,
        [(position(5, 0), T0 + 30), (position(4, 0), T0 + 30)]
    );
    // The next match (seq 2) sits below the unmatched seq 3, whose time is lower.
    assert_eq!(
        (page.scanned_to, page.frontier),
        (Some(position(4, 0)), T0 + 30)
    );
    assert!(!page.exhausted);
}

#[test]
fn a_limit_or_scan_of_0_counts_as_1() {
    let (_dir, store) = store();
    append(&store, filler(3));
    for (limit, scan) in [(0, 2_000), (50, 0)] {
        let mut q = query(4);
        (q.limit, q.scan) = (limit, scan);
        let page = read(&store, &q).unwrap();
        let found: Vec<_> = page.entries.iter().map(|e| e.position).collect();
        assert_eq!(found, [position(4, 0)], "limit {limit}, scan {scan}");
        assert_eq!(
            (page.scanned_to, page.exhausted),
            (Some(position(4, 0)), false)
        );
    }
}

// ---- page independence -----------------------------------------------------------------------

/// 4,500 rows with removals, archive flips and clock steps, and lookbacks of exactly 2,000
/// (`SB0` at 100 removed at 2,100; `PB0` at 300 archived at 2,300) and 2,001 rows (`SB1`, `PB1`).
fn independence_log() -> Rows {
    (2..=4_500u64)
        .map(|seq| {
            let n = seq % 40;
            let id = |p: &str| format!("{p}{n}");
            let changes = match seq {
                100 => vec![membership("SB0", "T1", "M1", 1)],
                200 => vec![membership("SB1", "T2", "M2", 1)],
                300 => vec![project("PB0", 2, false)],
                400 => vec![project("PB1", 2, false)],
                2_100 => vec![removed("membership", "SB0")],
                2_201 => vec![removed("membership", "SB1")],
                2_300 => vec![project("PB0", 3, true)],
                2_401 => vec![project("PB1", 3, true)],
                s if s % 7 == 0 => vec![
                    membership(&id("S"), "T1", &id("M"), 1),
                    removed("membership", &id("S")),
                ],
                s if s % 5 == 0 => vec![project(&id("P"), s, s % 3 == 0)],
                s if s % 3 == 0 => vec![
                    removed("project_team", &id("Q")),
                    project_team(&id("Q"), "P1", "T1", 1),
                ],
                _ => vec![member(&id("M"), 2)],
            };
            (at(T0 + seq * 1_000 - (seq % 11) * 3_000), changes)
        })
        .collect()
}

#[test]
fn entries_do_not_depend_on_how_pages_fall() {
    let (_dir, store) = store();
    append(&store, independence_log());
    // Unfiltered, every page returns an entry; at most `max` pages.
    let run = |ceiling, limit, scan, max| {
        let mut q = query(ceiling);
        (q.limit, q.scan) = (limit, scan);
        paged(&store, q, max)
    };
    let whole = run(4_500, 100, 2_000, usize::MAX);
    assert_eq!(whole.len(), 6_168);
    assert!(whole.windows(2).all(|pair| pair[0].key >= pair[1].key));
    for (limit, scan) in [(1, 2_000), (3, 2_000), (100, 7)] {
        let paged = run(4_500, limit, scan, whole.len());
        assert!(paged == whole, "limit {limit}, scan {scan}");
    }
    // One row per page, over the lookback edges at 2,100 to 2,401: a row has at most two
    // changes, so a limit of 3 takes one page per row and a limit of 1 one per entry.
    let lower = run(2_500, 100, 2_000, usize::MAX);
    for (limit, max) in [(3, 2_500), (1, lower.len())] {
        assert!(run(2_500, limit, 1, max) == lower, "limit {limit}, scan 1");
    }
    let at = |seq| whole.iter().find(|e| e.position.seq == seq).unwrap();
    assert_eq!(
        subjects(&[at(2_100).clone()])[0],
        ("org.membership.removed", [Some("M1"), Some("T1"), None])
    );
    assert_eq!(
        subjects(&[at(2_201).clone()])[0],
        ("org.membership.removed", NONE)
    );
    assert_eq!(at(2_300).verb, "org.project.archived");
    assert_eq!(at(2_401).verb, "org.project.changed");
}

#[test]
fn an_upsert_read_for_the_page_but_outside_its_window_does_not_resolve() {
    let (_dir, store) = store();
    append(&store, independence_log());
    store
        .read(|db| {
            // Row 0 is seq 2,201; its lookback reads in blocks past row 2,000 to seq 200.
            let (source, mut log) = ((db, 1, 2_201, None), Log::default());
            assert!(log.has(source, 0)?);
            let (verb, subject) = log.describe(source, 0, 0)?;
            assert!(
                log.rows.iter().any(|row| row.seq == 200),
                "the upsert is in the read"
            );
            assert!(log.rows.len() <= 1 + LOOKBACK_ROWS + 511 && !log.end);
            assert_eq!(
                (verb.as_str(), subject),
                ("org.membership.removed", Default::default())
            );
            let (source, mut log) = ((db, 1, 2_100, None), Log::default());
            assert!(log.has(source, 0)?);
            let (_, subject) = log.describe(source, 0, 0)?;
            assert_eq!(subject.member.as_deref(), Some("M1"));
            Ok(())
        })
        .unwrap();
}

#[test]
fn a_lookback_without_an_upsert_in_its_window_reads_no_further() {
    let (_dir, store) = store();
    let mut rows = independence_log();
    // Seq 4,000 removes `SB0`, last upserted at seq 100; seq 3,000 removes a never-logged id.
    rows[4_000 - 2].1 = vec![removed("membership", "SB0")];
    rows[3_000 - 2].1 = vec![removed("membership", "GHOST")];
    append(&store, rows);
    store
        .read(|db| {
            for k in [500, 1_500] {
                let (source, mut log) = ((db, 1, 4_500, None), Log::default());
                assert!(log.has(source, k)?);
                assert_eq!(log.rows[k].seq, 4_500 - k as u64);
                let (verb, subject) = log.describe(source, k, 0)?;
                assert_eq!(
                    (verb.as_str(), subject),
                    ("org.membership.removed", Default::default())
                );
                // At most the window and one growing block past it; never the whole log.
                let read = log.rows.len();
                assert!(
                    read <= k + 1 + LOOKBACK_ROWS + 511 && !log.end,
                    "row {k}: {read}"
                );
            }
            Ok(())
        })
        .unwrap();
}

// ---- latest by project -----------------------------------------------------------------------

#[test]
fn latest_by_project_is_the_first_entry_activity_returns_for_it() {
    let (_dir, store) = store();
    let ms = |s: u64| T0 + s * 1_000;
    append(
        &store,
        vec![
            (
                at(ms(10)),
                vec![project_team("Q9", "PR", "T1", 1), project("PA", 1, false)],
            ),
            (
                at(ms(50)),
                vec![project("PC", 1, false), project("PU", 1, false)],
            ),
            (at(ms(20)), vec![project("PC", 2, false)]),
            (
                at(ms(30)),
                vec![
                    project("PA", 2, true),
                    project("PT1", 1, false),
                    project("PT2", 1, false),
                ],
            ),
            ("garbage".into(), vec![area("AU", "PU", 2, false)]),
            (at(ms(40)), vec![removed("project_team", "Q9")]),
        ],
    );
    append(&store, filler(30));
    let ids = ["PN", "PT1", "PT2", "PC", "PR", "PA", "PU"];
    let found = latest(&store, &ids);
    // PN has no activity (absent); PU's newest entry has an unparseable time (None).
    let expected = [
        ("PT1", Some(ms(30))),
        ("PT2", Some(ms(30))),
        ("PC", Some(ms(20))),
        ("PR", Some(ms(40))),
        ("PA", Some(ms(30))),
        ("PU", None),
    ];
    assert_eq!(found, expected.map(|(id, at)| (id.to_string(), at)).into());
    for id in ids {
        let mut q = query(head_seq(&store));
        (q.project, q.limit) = (Some(id.into()), 1);
        let first = read(&store, &q).unwrap().entries.first().map(|e| e.at_ms);
        assert_eq!(found.get(id).copied(), first, "{id}");
    }
}

/// Recency for `ids`, checked against the first entry paging with `project = id` returns.
fn recency_as_pages_see_it(store: &Store, ids: &[&str]) -> ProjectActivity {
    let found = latest(store, ids);
    for id in ids {
        let mut q = query(head_seq(store));
        (q.project, q.limit) = (Some(id.to_string()), 1);
        let first = all(store, q).first().map(|e| e.at_ms);
        assert_eq!(found.get(*id).copied(), first, "{id}");
    }
    found
}

/// Appends one raw row at `seq`, as a damaged or foreign log could hold it.
fn raw(store: &Store, seq: i64, body: &'static str) {
    store
        .write(move |db| {
            db.execute(
                "INSERT INTO events (epoch, seq, kind, command_id, at, body)
                 VALUES (1, ?1, 'org.changed', 'RAW', '2026-11-08T13:00:00.000Z', ?2)",
                (seq, body),
            )
            .map_err(OfficeError::storage)?;
            Ok(((), None))
        })
        .unwrap();
}

#[test]
fn recency_counts_every_row_toward_the_lookback() {
    // The removal is 2,002 rows above the add, so it has no subject whether the rows between
    // mention a project or not: PZ's newest entry stays the add.
    let projects = (0..2_001).map(|i| {
        (
            at(T0 + 2_000 + i),
            vec![project(&format!("PO{i}"), 1, false)],
        )
    });
    for between in [filler(2_001), projects.collect()] {
        let (_dir, store) = store();
        let mut rows = vec![(at(T0 + 1_000), vec![project_team("QZ", "PZ", "T1", 1)])];
        rows.extend(between);
        rows.push((at(T0 + 9_000_000), vec![removed("project_team", "QZ")]));
        append(&store, rows);
        assert_eq!(
            recency_as_pages_see_it(&store, &["PZ"])["PZ"],
            Some(T0 + 1_000)
        );
    }
}

#[test]
fn recency_skips_unknown_changes_and_takes_any_request() {
    let (_dir, store) = store();
    append(
        &store,
        vec![(at(T0 + 1_000), vec![project("PK", 1, false)])],
    );
    raw(
        &store,
        3,
        r#"[{"type":"project_renamed_later","project_id":"PK"}]"#,
    );
    let page = read(&store, &query(3)).unwrap();
    assert_eq!(subjects(&page.entries[..1]), [("org.changed", NONE)]);
    assert_eq!(
        recency_as_pages_see_it(&store, &["PK"])["PK"],
        Some(T0 + 1_000)
    );
    // A duplicate id counts once; an empty request is empty but still checks the snapshot.
    assert_eq!(latest(&store, &["PK", "PK"]), latest(&store, &["PK"]));
    assert_eq!(latest(&store, &[]), ProjectActivity::default());
    for ceiling in [Position { epoch: 2, seq: 3 }, Position { epoch: 1, seq: 4 }] {
        let read = store
            .read(|db| Ok(latest_by_project(db, ceiling, &[])))
            .unwrap();
        assert!(
            matches!(read, Err(ActivityError::SnapshotRequired)),
            "{ceiling:?}"
        );
    }
}

#[test]
fn a_stored_change_missing_a_required_field_fails_the_read() {
    let missing = [
        r#"[{"type":"membership_upserted","membership":{"id":"S1","member_id":"M1","version":1}}]"#,
        r#"[{"type":"team_upserted","team":{"id":"T1","name":"x"}}]"#,
        r#"[{"type":"member_upserted","member":{"id":"M1","kind":"person","version":1}}]"#,
        r#"[{"type":"area_upserted","area":{"id":"A1","name":"x","version":1}}]"#,
        r#"[{"type":"project_team_added","project_team":{"id":"Q1","team_id":"T1","version":1}}]"#,
        r#"[{"type":"team_deleted"}]"#,
    ];
    for body in missing {
        assert!(
            serde_json::from_str::<Vec<OrgChange>>(body).is_err(),
            "{body}"
        );
        let (_dir, store) = store();
        raw(&store, 2, body);
        assert!(
            matches!(read(&store, &query(2)), Err(ActivityError::Store(_))),
            "{body}"
        );
    }
    // What the contract defaults may be missing.
    let body = r#"[{"type":"project_upserted","project":{"id":"P1","name":"x"}}]"#;
    assert!(serde_json::from_str::<Vec<OrgChange>>(body).is_ok());
    let (_dir, store) = store();
    raw(&store, 2, body);
    let page = read(&store, &query(2)).unwrap();
    assert_eq!(page.entries[0].verb, "org.project.changed");
}

// ---- no writes -------------------------------------------------------------------------------

#[test]
fn reads_write_nothing() {
    let (_dir, store) = store();
    append(&store, mixed(50));
    let version =
        |db: &Connection| db.query_row("PRAGMA data_version", [], |row| row.get::<_, i64>(0));
    store
        .read(|spy| {
            let before = version(spy).unwrap();
            store.read(|db| {
                let changes = db.total_changes();
                page(db, &query(51)).unwrap();
                latest_by_project(
                    db,
                    Position { epoch: 1, seq: 51 },
                    &["P1".into(), "X".into()],
                )
                .unwrap();
                assert_eq!(db.total_changes(), changes);
                Ok(())
            })?;
            assert_eq!(version(spy).unwrap(), before);
            Ok(())
        })
        .unwrap();
    // The spy sees a commit by another connection.
    store
        .read(|spy| {
            let before = version(spy).unwrap();
            append(&store, filler(1));
            assert_ne!(version(spy).unwrap(), before);
            Ok(())
        })
        .unwrap();
}

// ---- budgets ---------------------------------------------------------------------------------

/// A realistic mix: hires, teams, memberships, projects, areas and removals.
fn mixed(n: usize) -> Rows {
    (0..n)
        .map(|i| {
            let id = |p: &str| format!("{p}{}", i % 500);
            let changes = match i % 6 {
                0 => vec![member(&id("M"), 1 + (i % 3) as u64)],
                1 => vec![
                    team(&id("T"), 2),
                    membership(&id("S"), &id("T"), &id("M"), 1),
                ],
                2 => vec![
                    project(&id("P"), 2, i % 4 == 0),
                    area(&id("A"), &id("P"), 2, false),
                ],
                3 => vec![project_team(&id("Q"), &id("P"), &id("T"), 1)],
                4 => vec![
                    removed("membership", &id("S")),
                    removed("project_team", &id("Q")),
                ],
                _ => vec![member(&id("M"), 4), removed("member", &id("N"))],
            };
            (at(T0 + 1_000 * i as u64), changes)
        })
        .collect()
}

#[test]
#[ignore = "release-only budget measurement: cargo test --release -p org-office -- --ignored budget"]
fn budget_org_activity_on_a_20000_row_log() {
    let (_dir, store) = store();
    append(&store, mixed(20_000));
    // Every command has its answer row, as on a live HQ.
    store.run_sql_for_test(
        "INSERT INTO commands (command_id, epoch, seq, expires_at, principal_member)
         SELECT command_id, epoch, seq, '2026-12-08T12:00:00.000Z', 'M1' FROM events WHERE seq > 1",
    );
    let ceiling = head_seq(&store);
    let time = |label: &str, run: &dyn Fn()| {
        run();
        let runs = 20;
        let start = Instant::now();
        for _ in 0..runs {
            run();
        }
        let each = start.elapsed() / runs;
        println!("budget {label}: {each:?} per call");
        each
    };
    let plain = time("page (scan 2000, limit 50)", &|| {
        assert_eq!(read(&store, &query(ceiling)).unwrap().entries.len(), 50);
    });
    let selective = time("page, selective filter with no matches", &|| {
        let mut q = query(ceiling);
        q.project = Some("nowhere".into());
        let page = read(&store, &q).unwrap();
        assert!(page.entries.is_empty() && !page.exhausted);
    });
    // The accepted worst case (freeze rev 4.8): the oldest requested project's last activity
    // deep in the log: idle, legacy or truncated; bounded by the log length. Here: 3 projects
    // with no retained activity, and `OLD` below, idle since seq 2. Both read the whole log.
    let absent = time("latest_by_project (7 projects, 3 absent)", &|| {
        let found = latest(&store, &["P1", "P2", "P3", "P499", "X1", "X2", "X3"]);
        assert_eq!(found.len(), 4);
    });
    let recency = time("latest_by_project (7 projects, all present)", &|| {
        let found = latest(&store, &["P1", "P2", "P3", "P499", "P0", "P250", "P7"]);
        assert_eq!(found.len(), 7);
    });
    let (_old_dir, old) = self::store();
    let mut rows = vec![(at(T0), vec![project("OLD", 1, false)])];
    rows.extend(mixed(20_000));
    append(&old, rows);
    let idle = time("latest_by_project (OLD idle since seq 2, P1, P2)", &|| {
        let found = latest(&old, &["OLD", "P1", "P2"]);
        assert_eq!((found.len(), found["OLD"]), (3, Some(T0)));
    });
    if !cfg!(debug_assertions) {
        assert!(plain.as_millis() <= 20 && selective.as_millis() <= 20);
        let recency = [recency, absent, idle];
        assert!(
            recency.iter().all(|each| each.as_millis() <= 30),
            "{recency:?}"
        );
    }
}
