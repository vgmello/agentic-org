use super::*;
use crate::selected_tasks::{adoption, staging};
use org_proto::selected_tasks::{
    adoption::{Action, Outcome},
    Revision,
};
use serde_json::json;
use std::collections::BTreeMap;

fn canonical_files() -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let records = [
        ("organization.yaml", "schema_version: 1\nrevision: 1\nid: org\nname: Organization\nslug: organization\nstate_branch: state\norganization_repository_id: repository\n".into()),
        ("repositories.yaml", "schema_version: 1\nrepositories:\n  - schema_version: 1\n    revision: 1\n    id: repository\n    kind: remote\n    remote_url: https://example.invalid/organization.git\n".into()),
        ("teams/accountable/team.yaml", "schema_version: 1\nrevision: 1\nid: accountable\nname: Accountable\nnumber: 1\nstatus: active\nparent_team_id: null\nlead_employee_id: null\nextension: 1.234567890123456789E+012\n".into()),
        ("teams/doing-team/team.yaml", "schema_version: 1\nrevision: 1\nid: doing-team\nname: Doing\nnumber: 2\nstatus: active\nparent_team_id: null\nlead_employee_id: null\n".into()),
        ("teams/accountable/people/employee/profile.md", "---\nschema_version: 1\nrevision: 1\nid: employee\nname: Employee\nhandle: employee\nkind: person\nstatus: active\nhome_team_id: accountable\nreports_to_employee_id: null\nrole_id: null\nis_owner: false\n---\nOriginal café employee body.\n".into()),
        ("identity/counters/organization/org/team.yaml", "scope_kind: organization\nscope_id: org\nentity_kind: team\nlast_number: 2\n".into()),
        ("identity/allocations/organization/org/team/1.yaml", "scope_kind: organization\nscope_id: org\nentity_kind: team\nnumber: 1\nentity_id: accountable\nallocated_at: 2026-01-01T00:00:00Z\n".into()),
        ("identity/allocations/organization/org/team/2.yaml", "scope_kind: organization\nscope_id: org\nentity_kind: team\nnumber: 2\nentity_id: doing-team\nallocated_at: 2026-01-01T00:00:00Z\n".into()),
    ];
    for (path, bytes) in records {
        files.insert(path.into(), String::into_bytes(bytes));
    }
    files
}
fn stage(
    store: &Store,
    root: &std::path::Path,
    files: &BTreeMap<String, Vec<u8>>,
    expected: Option<i64>,
) -> Action {
    let entries=files.iter().map(|(path,bytes)| {
        let kind=if path=="organization.yaml" {"organization"} else if path=="repositories.yaml" {"repositories"} else if path.ends_with("team.yaml") && path.starts_with("teams/") {"team"} else if path.ends_with("profile.md") {"employee"} else if path.starts_with("identity/counters/") {"counter"} else {"allocation"};
        json!({"kind":kind,"canonical_path":path,"blob":{"sha256":codec::sha256(bytes),"byte_length":bytes.len()}})
    }).collect::<Vec<_>>();
    let raw=serde_json::to_vec(&json!({"version":1,"resource_profile":codec::RESOURCE_PROFILE,"source_position":{"epoch":1,"seq":10},"files":entries})).unwrap();
    let stage = crate::ulid::generate().unwrap();
    let digest = codec::sha256(&raw);
    for bytes in std::iter::once(&raw).chain(files.values()) {
        let mut request = staged_request(&stage, bytes, Some(0));
        request.manifest_sha256 = digest.clone();
        assert!(
            staging::run(store, root.to_path_buf(), authority(), request)
                .unwrap()
                .verified
        );
    }
    Action::DirectoryCommit {
        operation_id: org_proto::gateway::OperationId::parse(crate::ulid::generate().unwrap())
            .unwrap(),
        stage_id: org_proto::gateway::OperationId::parse(stage).unwrap(),
        manifest_sha256: digest,
        expected_directory_generation: expected.map(|n| Revision::new(n).unwrap()),
    }
}
fn generation(store: &Store) -> i64 {
    store
        .selected_read(|db, _| {
            db.query_row("SELECT generation FROM selected_directory_head", [], |r| {
                r.get(0)
            })
            .map_err(storage)
        })
        .unwrap()
}
#[test]
fn canonical_directory_import_retains_bytes_numbers_origin_and_original_receipt_after_advancement_and_reopen(
) {
    let (dir, store, config) = fixture();
    let root = dir.path().join("staged-directory");
    let files = canonical_files();
    let action = stage(&store, &root, &files, Some(1));
    let before = store
        .selected_read(|db, _| {
            db.query_row(
                "SELECT COALESCE(MAX(last_seq),0) FROM work_origin_counter",
                [],
                |r| r.get::<_, i64>(0),
            )
            .map_err(storage)
        })
        .unwrap();
    assert_eq!(
        adoption::run(&store, root.clone(), authority(), action.clone()).unwrap(),
        Outcome::DirectoryCompleted { generation: 2 }
    );
    assert_eq!(generation(&store), 2);
    store.selected_read(move|db,_| {
        assert_eq!(db.query_row("SELECT COALESCE(MAX(last_seq),0) FROM work_origin_counter",[],|r|r.get::<_,i64>(0)).map_err(storage)?,before);
        for (path,bytes) in files {
            let retained:Vec<u8>=db.query_row("SELECT content FROM selected_artifacts WHERE namespace='public' AND path=?1",[path],|r|r.get(0)).map_err(storage)?;
            assert_eq!(retained,bytes);
        }Ok(())
    }).unwrap();
    let mut next = canonical_files();
    let path = "teams/accountable/people/employee/profile.md";
    next.insert(
        path.into(),
        String::from_utf8(next[path].clone())
            .unwrap()
            .replace("revision: 1", "revision: 2")
            .replace("status: active", "status: inactive")
            .into_bytes(),
    );
    let advanced = stage(&store, &root, &next, Some(2));
    assert_eq!(
        adoption::run(&store, root.clone(), authority(), advanced).unwrap(),
        Outcome::DirectoryCompleted { generation: 3 }
    );
    assert_eq!(
        adoption::run(&store, root.clone(), authority(), action.clone()).unwrap(),
        Outcome::DirectoryCompleted { generation: 2 }
    );
    assert_eq!(generation(&store), 3);
    drop(store);
    let store = Store::open(
        &dir.path().join("office.db"),
        &Key::from_bytes([31; 32]),
        Opening::Existing,
        &OFFICE_SCHEMA,
    )
    .unwrap();
    store.install_acceptance(config).unwrap();
    assert_eq!(
        adoption::run(&store, root, authority(), action).unwrap(),
        Outcome::DirectoryCompleted { generation: 2 }
    );
    assert_eq!(generation(&store), 3);
}
#[test]
fn invalid_directory_sources_remain_staged_without_partial_facts_files_or_origin_positions() {
    let mut cases = Vec::new();
    let mut broken = canonical_files();
    broken
        .get_mut("teams/doing-team/team.yaml")
        .unwrap()
        .extend(b"number: 3\n");
    cases.push(broken);
    let mut broken = canonical_files();
    let source = broken.remove("teams/doing-team/team.yaml").unwrap();
    broken.insert("teams/wrong/team.yaml".into(), source);
    cases.push(broken);
    let mut broken = canonical_files();
    broken.insert(
        "identity/counters/organization/org/team.yaml".into(),
        b"scope_kind: organization\nscope_id: org\nentity_kind: team\nlast_number: 1\n".to_vec(),
    );
    cases.push(broken);
    let mut broken = canonical_files();
    broken.remove("teams/doing-team/team.yaml");
    cases.push(broken);
    let mut broken = canonical_files();
    broken
        .get_mut("teams/doing-team/team.yaml")
        .unwrap()
        .extend(b"huge_extension: 999999999999999999999999999999999999999999999999999999\n");
    cases.push(broken);
    let mut broken = canonical_files();
    broken
        .get_mut("identity/counters/organization/org/team.yaml")
        .unwrap()
        .extend(b"---\nlast_number: 999\n");
    cases.push(broken);
    for files in cases {
        let (dir, store, _) = fixture();
        let root = dir.path().join("directory-staging");
        let action = stage(&store, &root, &files, Some(1));
        assert!(matches!(
            adoption::run(&store, root.clone(), authority(), action.clone()).unwrap(),
            Outcome::Staged { .. }
        ));
        assert_eq!(generation(&store), 1);
        store.selected_read(|db,_|{
            assert!(!db.query_row("SELECT EXISTS(SELECT 1 FROM selected_artifacts WHERE path='organization.yaml')",[],|r|r.get::<_,bool>(0)).map_err(storage)?);
            Ok(())
        }).unwrap();
        assert!(root.is_dir());
        assert!(matches!(
            adoption::run(&store, root, authority(), action).unwrap(),
            Outcome::Staged { .. }
        ));
    }
}
fn empty_directory_fixture() -> (TempDir, Store) {
    let dir = TempDir::new().unwrap();
    let store = Store::open(
        &dir.path().join("office.db"),
        &Key::from_bytes([31; 32]),
        Opening::New,
        &OFFICE_SCHEMA,
    )
    .unwrap();
    store
        .write(|db| {
            db.execute(
                "INSERT INTO meta VALUES('office_id','office'),('instance_id','instance')",
                [],
            )
            .map_err(storage)?;
            Ok(((), None))
        })
        .unwrap();
    store
        .install_acceptance(AcceptanceConfig {
            authority: None,
            identity: AcceptanceIdentity {
                instance_id: "instance".into(),
                organization_id: "org".into(),
                office_id: "office".into(),
                journal_incarnation: "journal".into(),
                writer_epoch: 1,
                machine_id: "machine".into(),
            },
            protected_root: dir.path().join("recovery"),
            public_root: dir.path().join("public"),
        })
        .unwrap();
    crate::selected_tasks::directory::bind_owner(&store, authority()).unwrap();
    (dir, store)
}
#[test]
fn initial_directory_admission_is_independent_of_assignment_readiness_but_requires_protected_owner()
{
    let (dir, store) = empty_directory_fixture();
    assert!(!crate::selected_tasks::read::ready(&store).unwrap());
    assert!(crate::selected_tasks::read::adoption_ready(&store).unwrap());
    let root = dir.path().join("initial-directory");
    let action = stage(&store, &root, &canonical_files(), None);
    assert_eq!(
        adoption::run(&store, root, authority(), action).unwrap(),
        Outcome::DirectoryCompleted { generation: 1 }
    );
    assert!(crate::selected_tasks::read::ready(&store).unwrap());
}

#[test]
fn directory_import_refuses_employee_and_team_relocation_with_zero_adopted_tasks() {
    for employee in [true, false] {
        let (dir, store) = empty_directory_fixture();
        let root = dir.path().join("relocation-staging");
        let files = canonical_files();
        let first = stage(&store, &root, &files, None);
        assert_eq!(
            adoption::run(&store, root.clone(), authority(), first).unwrap(),
            Outcome::DirectoryCompleted { generation: 1 }
        );
        store
            .selected_read(|db, _| {
                assert_eq!(
                    db.query_row("SELECT COUNT(*) FROM selected_tasks", [], |r| r
                        .get::<_, i64>(0))
                        .map_err(storage)?,
                    0
                );
                Ok(())
            })
            .unwrap();
        let mut moved = files.clone();
        let (old, new, field, value) = if employee {
            (
                "teams/accountable/people/employee/profile.md",
                "teams/doing-team/people/employee/profile.md",
                "home_team_id: accountable",
                "home_team_id: doing-team",
            )
        } else {
            (
                "teams/doing-team/team.yaml",
                "teams/accountable/teams/doing-team/team.yaml",
                "parent_team_id: null",
                "parent_team_id: accountable",
            )
        };
        // Equal revision at a new path used to evade the original-path check.
        let bytes = String::from_utf8(moved.remove(old).unwrap())
            .unwrap()
            .replace(field, value)
            .into_bytes();
        moved.insert(new.into(), bytes);
        let rejected = stage(&store, &root, &moved, Some(1));
        assert!(matches!(
            adoption::run(&store, root.clone(), authority(), rejected).unwrap(),
            Outcome::Staged { .. }
        ));
        assert_eq!(generation(&store), 1);
        let old = old.to_owned();
        let new = new.to_owned();
        let expected = files[&old].clone();
        store.selected_read(move|db,_|{
            let original:Vec<u8>=db.query_row("SELECT content FROM selected_artifacts WHERE namespace='public' AND path=?1",[old],|r|r.get(0)).map_err(storage)?;assert_eq!(original,expected);
            assert!(!db.query_row("SELECT EXISTS(SELECT 1 FROM selected_artifacts WHERE namespace='public' AND path=?1)",[new],|r|r.get::<_,bool>(0)).map_err(storage)?);Ok(())
        }).unwrap();
    }
}
