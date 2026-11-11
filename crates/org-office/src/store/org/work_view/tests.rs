use super::*;
use crate::store::org::{self, Genesis, SCHEMA};
use crate::store::Opening;
use crate::{Key, OfficeError};
use org_proto::org::{AgentStatus, MemberKind};
use org_work::testkit::MemOrg;
use serde_json::{json, Value};
use std::panic::{catch_unwind, AssertUnwindSafe};

pub(crate) const OWNER: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const PERSON: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";
const AGENT: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAX";
const REMOVED: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAY";
const TEAM_A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FA1";
const TEAM_Z: &str = "01ARZ3NDEKTSV4RRFFQ69G5FA2";
const NOW: u64 = 1_762_600_000_000;

pub(crate) fn command(store: &Store, method: &str, mut params: Value) {
    let next = store.read(org::last_position).unwrap().unwrap().seq + 1;
    params["command_id"] = json!(org_work::ulid_from_parts(NOW, next as u128));
    let command = crate::rules::OrgCommand::parse(&org_proto::Request {
        id: 1,
        method: method.into(),
        principal: None,
        params,
    })
    .unwrap()
    .unwrap();
    let result = store
        .write(move |db| Ok((org::submit(db, NOW, &command)?, None)))
        .unwrap();
    assert!(result.is_ok(), "{method}: {result:?}");
}

/// Members, teams and leads use commands; projects and areas have no commands yet.
/// Normalize MemOrg from the actual committed member bodies and position.
pub(crate) fn load(root: &std::path::Path, memory: &mut MemOrg) -> Arc<Store> {
    let store = Arc::new(
        Store::open(
            &root.join("org.db"),
            &Key::from_bytes([19; 32]),
            Opening::New,
            &SCHEMA,
        )
        .unwrap(),
    );
    let owner_id = memory.owner_id.clone();
    store
        .write(move |db| {
            org::genesis(
                db,
                &Genesis {
                    org_id: "ORG1".into(),
                    org_name: "Test organization".into(),
                    created_at: "2025-11-08T12:00:00.000Z".into(),
                    owner_id: owner_id.clone(),
                },
            )
            .map(|p| (p, None))
        })
        .unwrap();
    store.run_sql_for_test("INSERT INTO meta (key,value) VALUES ('office_id','hq')");
    for (i, member) in memory.members.values().filter(|m| !m.owner).enumerate() {
        if member.kind == MemberKind::Agent {
            let mut profile = member.agent.clone().unwrap();
            profile.messaging_name = format!("agent{i}");
            let offboarded = profile.status == AgentStatus::Offboarded;
            if offboarded {
                profile.status = AgentStatus::Active;
                profile.offboarded_at = None;
            }
            command(
                &store,
                "member.add_agent",
                json!({"member_id":member.id,"name":format!("Agent {i}"),"office_id":"hq","profile":profile}),
            );
            if offboarded {
                command(
                    &store,
                    "member.offboard",
                    json!({"member_id":member.id,"expected_version":1}),
                );
            }
        } else {
            command(
                &store,
                "member.add_person",
                json!({"member_id":member.id,"name":format!("Person {i}")}),
            );
        }
    }
    let teams: std::collections::BTreeSet<_> = memory
        .project_teams
        .values()
        .flatten()
        .chain(memory.team_leads.keys())
        .cloned()
        .collect();
    for (i, team) in teams.iter().rev().enumerate() {
        command(
            &store,
            "team.create",
            json!({"team_id":team,"name":format!("Team {i}")}),
        );
        if let Some(lead) = memory.team_leads.get(team) {
            command(
                &store,
                "team.add_member",
                json!({"membership_id":format!("{:026}",i+100),"team_id":team,"member_id":lead}),
            );
            command(
                &store,
                "team.set_lead",
                json!({"team_id":team,"expected_version":1,"lead_member_id":lead}),
            );
        }
    }
    let fixture = memory.clone();
    store
        .write(move |db| {
            for project in fixture.projects.values() {
                db.execute(
                    "INSERT INTO projects VALUES (?1,?2)",
                    (&project.id, serde_json::to_string(project).unwrap()),
                )
                .map_err(OfficeError::storage)?;
            }
            for area in fixture.areas.values() {
                db.execute(
                    "INSERT INTO areas VALUES (?1,?2,?3)",
                    (
                        &area.id,
                        &area.project_id,
                        serde_json::to_string(area).unwrap(),
                    ),
                )
                .map_err(OfficeError::storage)?;
            }
            for (project, teams) in &fixture.project_teams {
                for team in teams {
                    let id = format!("{project}:{team}");
                    let body = json!({"id":id,"project_id":project,"team_id":team,"version":1});
                    db.execute(
                        "INSERT INTO project_teams VALUES (?1,?2,?3,?4)",
                        (&id, project, team, body.to_string()),
                    )
                    .map_err(OfficeError::storage)?;
                }
            }
            Ok(((), None))
        })
        .unwrap();
    let snapshot = store.read(org::snapshot).unwrap();
    memory.members = snapshot
        .members
        .into_iter()
        .map(|m| (m.id.clone(), m))
        .collect();
    memory.position = snapshot.position;
    store
}

fn fixture() -> (tempfile::TempDir, Arc<Store>, MemOrg) {
    let root = tempfile::tempdir().unwrap();
    let mut memory = MemOrg::new(OWNER)
        .with_person(PERSON)
        .with_person(REMOVED)
        .with_agent(AGENT, AgentStatus::Offboarded)
        .with_project("p", "key", &[TEAM_Z, TEAM_A])
        .with_project("empty", "", &[])
        .with_project("other", "other", &[TEAM_A])
        .with_project("z-duplicate", "key", &[])
        .with_area("old", "p", Some(TEAM_A), Some(PERSON))
        .with_team_lead(TEAM_A, PERSON);
    memory.delete_area("old", "p-general");
    let mut extra_general = memory.areas["p-general"].clone();
    extra_general.id = "a-general".into();
    memory.areas.insert(extra_general.id.clone(), extra_general);
    let store = load(root.path(), &mut memory);
    command(
        &store,
        "member.remove",
        json!({"member_id":REMOVED,"expected_version":1}),
    );
    memory.members.remove(REMOVED);
    memory.position = store.read(org::last_position).unwrap().unwrap();
    store
        .read(|db| {
            db.pragma_update(None, "reverse_unordered_selects", true)
                .map_err(OfficeError::storage)
        })
        .unwrap();
    (root, store, memory)
}

#[test]
fn every_orgview_query_matches_memory_including_tombstones_offboarded_empty_keys_and_order() {
    let (_root, store, mem) = fixture();
    let source = OrgDbSource::new(store);
    let view = source.snapshot().unwrap();
    assert_eq!(
        view.applied_position().unwrap(),
        mem.applied_position().unwrap()
    );
    assert_eq!(view.owner_id().unwrap(), OWNER);
    for id in [OWNER, PERSON, AGENT, REMOVED, "missing", "' OR 1=1 --"] {
        assert_eq!(view.member(id).unwrap(), mem.member(id).unwrap());
    }
    assert!(view
        .member(AGENT)
        .unwrap()
        .unwrap()
        .agent
        .unwrap()
        .offboarded_at
        .is_some());
    for id in ["p", "empty", "other", "missing", "' OR 1=1 --"] {
        assert_eq!(view.project(id).unwrap(), mem.project(id).unwrap());
        assert_eq!(
            view.project_teams(id).unwrap(),
            mem.project_teams(id).unwrap()
        );
        assert_eq!(
            view.general_area(id).unwrap(),
            mem.general_area(id).unwrap()
        );
    }
    assert_eq!(view.project_teams("p").unwrap(), [TEAM_A, TEAM_Z]);
    assert_eq!(view.project_by_key("key").unwrap().unwrap().id, "p");
    assert_eq!(
        view.general_area("p").unwrap().as_deref(),
        Some("a-general")
    );
    for key in ["key", "other", "", "missing", "' OR 1=1 --"] {
        assert_eq!(
            view.project_by_key(key).unwrap(),
            mem.project_by_key(key).unwrap()
        );
    }
    for area in ["old", "p-general", "other-general", "missing"] {
        assert_eq!(view.area(area).unwrap(), mem.area(area).unwrap());
    }
    assert!(view.area("old").unwrap().unwrap().deleted);
    for team in [TEAM_A, TEAM_Z, "missing"] {
        assert_eq!(view.team_lead(team).unwrap(), mem.team_lead(team).unwrap());
    }
}

#[test]
fn snapshot_is_pinned_before_first_view_method_and_next_attempt_sees_commit() {
    let (_root, store, _) = fixture();
    let source = OrgDbSource::new(store.clone());
    let view = source.snapshot().unwrap();
    // No view method was called: snapshot() itself must have pinned a table read.
    command(
        &store,
        "member.add_person",
        json!({"member_id":"01ARZ3NDEKTSV4RRFFQ69G5FAZ","name":"Later"}),
    );
    let next = source.snapshot().unwrap();
    assert!(view.member("01ARZ3NDEKTSV4RRFFQ69G5FAZ").unwrap().is_none());
    assert!(next.member("01ARZ3NDEKTSV4RRFFQ69G5FAZ").unwrap().is_some());
    assert_eq!(
        view.applied_position().unwrap().seq + 1,
        next.applied_position().unwrap().seq
    );
}

fn fresh_snapshot_after_discard(store: &Store) {
    assert_eq!(store.idle_readers().len(), 0);
    let view = store.read_snapshot().unwrap();
    assert!(!view.is_autocommit());
    drop(view);
    assert_eq!(store.idle_readers().len(), 1);
    assert!(store.read(|db| Ok(db.is_autocommit())).unwrap());
}

#[test]
fn reader_pool_returns_clean_readers_but_discards_error_panic_and_failed_rollback() {
    let (_root, store, _) = fixture();
    store.idle_readers().clear();
    let source = OrgDbSource::new(store.clone());
    drop(source.snapshot().unwrap());
    assert_eq!(store.idle_readers().len(), 1);
    assert!(store.read(|db| Ok(db.is_autocommit())).unwrap());
    // Domain refusal marks the guard, even when the SQL itself succeeds.
    store.run_sql_for_test("DELETE FROM members WHERE handle_key IS NULL");
    let view = source.snapshot().unwrap();
    assert!(view.owner_id().is_err());
    drop(view);
    fresh_snapshot_after_discard(&store);
    let result = catch_unwind(AssertUnwindSafe(|| {
        let _view = source.snapshot().unwrap();
        panic!("inside f");
    }));
    assert!(result.is_err());
    fresh_snapshot_after_discard(&store);
    let view = store.read_snapshot().unwrap();
    view.execute_batch("ROLLBACK").unwrap();
    drop(view);
    fresh_snapshot_after_discard(&store);
}

#[test]
fn pool_capacity_boundary_poison_recovery_and_query_only_new_connections() {
    let (_root, store, _) = fixture();
    store.idle_readers().clear();
    let mut views = (0..=crate::store::MAX_IDLE_READERS)
        .map(|_| store.read_snapshot().unwrap())
        .collect::<Vec<_>>();
    for view in &views {
        assert!(view
            .pragma_query_value(None, "query_only", |r| r.get::<_, bool>(0))
            .unwrap());
        assert!(view.execute("DELETE FROM members", []).is_err());
    }
    while let Some(view) = views.pop() {
        drop(view);
    }
    assert_eq!(store.idle_readers().len(), crate::store::MAX_IDLE_READERS);
    assert!(catch_unwind(AssertUnwindSafe(|| {
        let _lock = store.readers.lock().unwrap();
        panic!("poison");
    }))
    .is_err());
    drop(store.read_snapshot().unwrap());
    assert_eq!(store.idle_readers().len(), crate::store::MAX_IDLE_READERS);
}

#[test]
fn no_owner_two_owners_and_empty_log_are_fallible_and_discard_the_reader() {
    for sql in ["DELETE FROM members WHERE handle_key IS NULL", "INSERT INTO members SELECT 'second-owner',NULL,NULL,body FROM members WHERE handle_key IS NULL", "DELETE FROM events"] {
        let (_root,store,_) = fixture();
        store.idle_readers().clear();
        store.run_sql_for_test(sql);
        let source = OrgDbSource::new(store.clone());
        let view = source.snapshot().unwrap();
        let error = if sql == "DELETE FROM events" {view.applied_position().unwrap_err()} else {view.owner_id().unwrap_err()};
        assert_eq!(error.to_string(),StoreError::new("control").to_string());
        drop(view);
        fresh_snapshot_after_discard(&store);
    }
}

#[test]
fn all_query_and_decode_errors_are_fixed_display_and_discard_connections() {
    type Query = fn(&dyn OrgView) -> Result<(), StoreError>;
    let cases: [(&str, Query); 15] = [
        ("DROP TABLE members", |v| v.member(OWNER).map(|_| ())),
        ("DROP TABLE members", |v| v.owner_id().map(|_| ())),
        ("DROP TABLE projects", |v| v.project("p").map(|_| ())),
        ("DROP TABLE projects", |v| {
            v.project_by_key("key").map(|_| ())
        }),
        ("DROP TABLE areas", |v| v.area("old").map(|_| ())),
        ("DROP TABLE areas", |v| v.general_area("p").map(|_| ())),
        ("DROP TABLE project_teams", |v| {
            v.project_teams("p").map(|_| ())
        }),
        ("DROP TABLE teams", |v| v.team_lead(TEAM_A).map(|_| ())),
        ("UPDATE members SET body='secret malformed JSON'", |v| {
            v.member(OWNER).map(|_| ())
        }),
        ("UPDATE projects SET body='{}'", |v| {
            v.project("p").map(|_| ())
        }),

        ("UPDATE areas SET body='{}'", |v| v.area("old").map(|_|())),
        ("UPDATE projects SET body='not-json'", |v| v.project_by_key("key").map(|_|())),
        ("DROP TABLE teams; CREATE TABLE teams (id TEXT, lead_member_id); INSERT INTO teams VALUES ('bad',42)", |v| v.team_lead("bad").map(|_|())),
        ("DROP TABLE project_teams; CREATE TABLE project_teams(project_id TEXT,team_id); INSERT INTO project_teams VALUES ('p',NULL)", |v| v.project_teams("p").map(|_|())),
        ("DROP TABLE members; CREATE TABLE members(id,handle_key); INSERT INTO members VALUES (NULL,NULL)", |v| v.owner_id().map(|_|())),
    ];
    for (sql, query) in cases {
        let (_root, store, _) = fixture();
        store.run_sql_for_test(sql);
        let source = OrgDbSource::new(store.clone());
        let view = source.snapshot().unwrap();
        assert_eq!(
            query(&*view).unwrap_err().to_string(),
            StoreError::new("control").to_string()
        );
        drop(view);
        fresh_snapshot_after_discard(&store);
    }
}

#[test]
fn snapshot_setup_errors_discard_reader_and_preserve_read_api() {
    let (_root, store, _) = fixture();
    store.idle_readers().clear();
    // An already open transaction fails BEGIN; the failed checkout rolls it back and discards it.
    store
        .read(|db| db.execute_batch("BEGIN").map_err(OfficeError::storage))
        .unwrap();
    assert!(store.read_snapshot().is_err());
    fresh_snapshot_after_discard(&store);
    store.run_sql_for_test("DROP TABLE events");
    assert!(store.read_snapshot().is_err());
    assert_eq!(store.idle_readers().len(), 0);
}

#[test]
fn panic_in_production_work_attempt_releases_and_discards_org_snapshot() {
    use org_work::{TxnOutput, WorkStore};
    let (root, org, _) = fixture();
    let office = Arc::new(
        Store::open(
            &root.path().join("office.db"),
            &Key::from_bytes([42; 32]),
            Opening::New,
            &crate::office::OFFICE_SCHEMA,
        )
        .unwrap(),
    );
    let source = Arc::new(OrgDbSource::new(org.clone()));
    let work = crate::store::work::assembly::OfficeWorkStore::new(office, source, Arc::new(|| NOW))
        .unwrap();
    org.idle_readers().clear();
    assert!(work
        .transact(Arc::new(|view, _| {
            assert_eq!(view.owner_id()?, OWNER);
            panic!("work callback panicked");
        }))
        .is_err());
    fresh_snapshot_after_discard(&org);
    work.transact(Arc::new(|view, _| {
        assert_eq!(view.owner_id()?, OWNER);
        Ok(TxnOutput::Refused(org_proto::ApiError::new(
            org_proto::ErrorCode::NotFound,
            "no-op",
        )))
    }))
    .unwrap();
    assert_eq!(org.idle_readers().len(), 1);
}

#[test]
fn snapshot_reads_insert_update_delete_replace_conflict_rollback_and_reopen() {
    let (root, store, mem) = fixture();
    let before = store.read(org::last_position).unwrap().unwrap();
    store.run_sql_for_test("INSERT INTO projects VALUES ('inserted','{\"id\":\"inserted\",\"name\":\"First\",\"version\":1}');
        UPDATE projects SET body=json_set(body,'$.name','Updated') WHERE id='inserted';
        INSERT OR REPLACE INTO projects VALUES ('inserted','{\"id\":\"inserted\",\"name\":\"Replaced\",\"version\":2}')");
    let source = OrgDbSource::new(store.clone());
    let snapshot = source.snapshot().unwrap();
    assert_eq!(
        snapshot.project("inserted").unwrap().unwrap().name,
        "Replaced"
    );
    assert!(store
        .write(|db| {
            db.execute("DELETE FROM projects WHERE id='inserted'", [])
                .map_err(OfficeError::storage)?;
            db.execute(
                "INSERT INTO projects SELECT * FROM projects WHERE id='p'",
                [],
            )
            .map_err(OfficeError::storage)?;
            Ok(((), None))
        })
        .is_err());
    assert_eq!(
        source
            .snapshot()
            .unwrap()
            .project("inserted")
            .unwrap()
            .unwrap()
            .version,
        2
    );
    store.run_sql_for_test("DELETE FROM projects WHERE id='inserted'");
    assert!(source
        .snapshot()
        .unwrap()
        .project("inserted")
        .unwrap()
        .is_none());
    assert!(snapshot.project("inserted").unwrap().is_some());
    drop(snapshot);
    drop(source);
    drop(store);
    let store = Arc::new(
        Store::open(
            &root.path().join("org.db"),
            &Key::from_bytes([19; 32]),
            Opening::Existing,
            &SCHEMA,
        )
        .unwrap(),
    );
    let source = OrgDbSource::new(store);
    let view = source.snapshot().unwrap();
    assert_eq!(view.applied_position().unwrap(), before);
    assert_eq!(view.project("p").unwrap(), mem.project("p").unwrap());
    assert!(view.project("inserted").unwrap().is_none());
}

#[test]
fn query_failure_is_sticky_after_success_and_open_failure_is_fixed_display() {
    let (root, mut store, _) = fixture();
    store.run_sql_for_test("DROP TABLE members");
    let source = OrgDbSource::new(store.clone());
    let view = source.snapshot().unwrap();
    assert!(view.member(OWNER).is_err());
    assert!(view.project("p").unwrap().is_some());
    drop(view);
    drop(source);
    fresh_snapshot_after_discard(&store);
    store.idle_readers().clear();
    Arc::get_mut(&mut store).unwrap().db_path = root.path().join("nonexistent.db");
    let source = OrgDbSource::new(store.clone());
    let error = match source.snapshot() {
        Ok(_) => panic!("missing database opened"),
        Err(e) => e,
    };
    assert_eq!(error.to_string(), StoreError::new("control").to_string());
    assert_eq!(store.idle_readers().len(), 0);
    assert!(!root.path().join("nonexistent.db").exists());
}
