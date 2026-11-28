//! SQLite delivery adapter unit tests (P2-7 counter timestamp fidelity & stale row pruning).

use org_docs::delivery::records::{
    ActorRef, FeatureRecord, FeatureStatus, NumberAllocation, NumberCounterRecord, RecordMeta,
};
use org_office::delivery::ports::{DeliveryRead, DeliveryWrite};
use org_office::organization_family::ports::OrganizationWrite;
use org_office::organization_family::records::{
    By, Employee, EmployeeKind, EmployeeStatus, Meta, Organization, Repository, RepositoryKind,
    Team, TeamStatus,
};
use org_office::projects::ports::ProjectsWrite;
use org_office::projects::records::Project;
use org_store::testkit::engine_suite::cursor;
use org_store::{Engine, Id, Timestamp};

use super::super::SqliteEngine;

const ORG: &str = "01900000-0000-7000-8000-00000000d105";
const REPO: &str = "01900000-0001-7000-8000-00000000d105";
const TEAM: &str = "01900000-0002-7000-8000-00000000d105";
const OWNER: &str = "01900000-0005-7000-8000-00000000d105";
const PROJ: &str = "01900000-0003-7000-8000-00000000d105";
const ENT_1: &str = "01900000-0004-7000-8000-000000000001";
const ENT_2: &str = "01900000-0004-7000-8000-000000000002";
const ENT_3: &str = "01900000-0004-7000-8000-000000000003";

#[test]
fn put_delivery_counter_persists_allocated_at_and_prunes_stale_allocations() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().canonicalize().expect("canon").join("office.db");
    let org = Id::parse(ORG).expect("org id");
    let repo = Id::parse(REPO).expect("repo id");
    let team = Id::parse(TEAM).expect("team id");
    let owner = Id::parse(OWNER).expect("owner id");
    let proj = Id::parse(PROJ).expect("project id");
    let engine = SqliteEngine::open(&path)
        .expect("open")
        .for_organization(&org);
    let meta = Meta::created(&By::Employee(owner.clone()), Timestamp(1_700_000_000_000));

    let mut tx = engine.begin().expect("begin seed");
    tx.put_organization(&Organization {
        id: org.clone(),
        slug: "example".into(),
        name: "Example".into(),
        organization_repository_id: repo.clone(),
        state_branch: "org/state".into(),
        default_dod: None,
        meta: meta.clone(),
    })
    .expect("org");
    tx.put_repositories(&[Repository {
        id: repo,
        key: Some("organization".into()),
        kind: RepositoryKind::Remote,
        remote_url: Some("https://example.test/org.git".into()),
        meta: meta.clone(),
    }])
    .expect("repo");
    tx.put_team(&Team {
        id: team.clone(),
        name: "Top".into(),
        number: 1,
        parent_team_id: None,
        lead_employee_id: Some(owner.clone()),
        status: TeamStatus::Active,
        max_members: 6,
        meta: meta.clone(),
    })
    .expect("team");
    tx.put_employee(&Employee {
        id: owner,
        handle: "owner".into(),
        kind: EmployeeKind::Person,
        name: "Owner".into(),
        home_team_id: team.clone(),
        role_id: None,
        reports_to_employee_id: None,
        status: EmployeeStatus::Active,
        contact: None,
        title: None,
        focus: None,
        role_text: None,
        title_in_team: None,
        offboarded_at: None,
        may_hire: true,
        builtin_key: None,
        meta: meta.clone(),
        body: String::new(),
    })
    .expect("employee");
    tx.put_project(&Project {
        id: proj.clone(),
        number: 1,
        slug: "alpha".into(),
        name: "Alpha".into(),
        owner_team_id: team,
        archived: false,
        development_process_id: None,
        contributing_team_ids: Vec::new(),
        repositories: Vec::new(),
        default_dod: None,
        meta,
    })
    .expect("project");

    let rmeta = RecordMeta {
        schema_version: 1,
        revision: 1,
        created_at: Some("2026-11-20T10:00:00.000Z".into()),
        created_by: ActorRef::Employee(OWNER.to_owned()),
        updated_at: Some("2026-11-20T10:00:00.000Z".into()),
        updated_by: ActorRef::Employee(OWNER.to_owned()),
    };
    for (num, fid, slug) in [
        (1, ENT_1, "f-one"),
        (2, ENT_2, "f-two"),
        (3, ENT_3, "f-three"),
    ] {
        tx.put_feature(&FeatureRecord {
            id: fid.to_owned(),
            project_id: PROJ.to_owned(),
            number: num,
            local_slug: slug.to_owned(),
            owner_employee_id: None,
            work_type: None,
            title: slug.to_owned(),
            status: FeatureStatus::Planned,
            body_markdown: String::new(),
            delivered_at: None,
            retired_reason: None,
            replaced_by_feature_id: None,
            meta: rmeta.clone(),
        })
        .expect("feature");
    }

    let initial = NumberCounterRecord {
        next_number: 3,
        allocations: vec![
            NumberAllocation {
                number: 1,
                entity_id: ENT_1.to_owned(),
                allocated_at: "2026-11-20T10:00:00.123Z".to_owned(),
            },
            NumberAllocation {
                number: 2,
                entity_id: ENT_2.to_owned(),
                allocated_at: "2026-11-21T11:30:00.456Z".to_owned(),
            },
        ],
    };
    tx.put_delivery_counter("project", &proj, "feature", &initial)
        .expect("put initial counter");
    engine.commit(tx, &cursor(1)).expect("commit initial");

    let snap = engine.snapshot().expect("snapshot 1");
    let got = snap
        .delivery_counter("project", &proj, "feature")
        .expect("read initial")
        .expect("present");
    assert_eq!(got, initial);
    drop(snap);

    // Replace with a shrunk/updated allocation list: number 2 MUST be pruned and number 3
    // MUST persist its real allocated_at timestamp.
    let updated = NumberCounterRecord {
        next_number: 4,
        allocations: vec![
            NumberAllocation {
                number: 1,
                entity_id: ENT_1.to_owned(),
                allocated_at: "2026-11-20T10:00:00.123Z".to_owned(),
            },
            NumberAllocation {
                number: 3,
                entity_id: ENT_3.to_owned(),
                allocated_at: "2026-11-22T12:45:00.789Z".to_owned(),
            },
        ],
    };
    let mut tx = engine.begin().expect("begin 2");
    tx.put_delivery_counter("project", &proj, "feature", &updated)
        .expect("put updated counter");
    engine.commit(tx, &cursor(2)).expect("commit updated");

    let snap = engine.snapshot().expect("snapshot 2");
    let got2 = snap
        .delivery_counter("project", &proj, "feature")
        .expect("read updated")
        .expect("present");
    assert_eq!(got2, updated);
}
