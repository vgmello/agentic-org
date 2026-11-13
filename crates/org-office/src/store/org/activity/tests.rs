use std::time::Instant;

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

/// Every entry, paging until exhausted and feeding each page's position and key back.
fn all(store: &Store, mut query: OrgActivityQuery) -> Vec<ActivityEntry> {
    let mut entries = Vec::new();
    loop {
        let page = read(store, &query).unwrap();
        entries.extend(page.entries);
        if page.exhausted {
            return entries;
        }
        query.before = page.scanned_to;
        query.prev_key = page.frontier;
    }
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
    // A rollback below the ceiling.
    store.run_sql_for_test("DELETE FROM events WHERE seq > 4");
    assert!(snapshot(&query(6)) && !snapshot(&query(4)));
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
    let run = |limit, scan| {
        let mut q = query(4_500);
        (q.limit, q.scan) = (limit, scan);
        all(&store, q)
    };
    let whole = run(100, 2_000);
    assert_eq!(whole.len(), 6_168);
    assert!(whole.windows(2).all(|pair| pair[0].key >= pair[1].key));
    for (limit, scan) in [(1, 2_000), (3, 2_000), (100, 7)] {
        assert!(run(limit, scan) == whole, "limit {limit}, scan {scan}");
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
            let (source, mut log) = ((db, 1, 2_201), Log::default());
            assert!(log.has(source, 0)?);
            let (verb, subject) = log.describe(source, 0, 0)?;
            assert!(
                log.rows.iter().any(|row| row.seq == 200),
                "the upsert is in the read"
            );
            assert_eq!(
                (verb.as_str(), subject),
                ("org.membership.removed", Default::default())
            );
            let (source, mut log) = ((db, 1, 2_100), Log::default());
            assert!(log.has(source, 0)?);
            let (_, subject) = log.describe(source, 0, 0)?;
            assert_eq!(subject.member.as_deref(), Some("M1"));
            Ok(())
        })
        .unwrap();
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
    if !cfg!(debug_assertions) {
        assert!(plain.as_millis() <= 20 && selective.as_millis() <= 20);
    }
}
