//! Existing-installation acceptance through Office's real OrgDbSource and work
//! service. The old project is a historical fixture because its native create
//! command is unserved. This is not host authentication/native-listener proof.
use super::*;
use crate::selected_tasks::{
    AcceptanceConfig, AcceptanceIdentity, AssignmentOutcome, AssignmentRequest, OwnerAccount,
    OwnerAuthority,
};
use org_proto::selected_tasks::{adoption, staging, Revision};
use org_work::{
    selected_codec as codec,
    selected_history::{self as history, Admission, EventFields},
    Caller, TxnOutput, WorkCall,
};
use serde_json::json;
use std::collections::BTreeMap;

struct FixtureOwner(String);
impl OwnerAuthority for FixtureOwner {
    fn verify_live(&self, _: &AcceptanceIdentity) -> Result<OwnerAccount, OfficeError> {
        Ok(OwnerAccount {
            account_id: "generated-test-account".into(),
            member_id: self.0.clone(),
        })
    }
}
fn blob(bytes: &[u8]) -> staging::Blob {
    staging::Blob {
        sha256: codec::sha256(bytes),
        byte_length: bytes.len() as u64,
    }
}
fn raw(office: &Office, task: &str) -> (String, String) {
    office
        .local_store()
        .read(|db| {
            Ok((
                db.query_row("SELECT data FROM work_items WHERE id=?1", [task], |r| {
                    r.get(0)
                })
                .map_err(OfficeError::storage)?,
                db.query_row(
                    "SELECT data FROM work_events WHERE target_id=?1 AND version=1",
                    [task],
                    |r| r.get(0),
                )
                .map_err(OfficeError::storage)?,
            ))
        })
        .unwrap()
}
fn create(office: &Office, owner: &str, project: &str, task: &str) {
    let p = json!({"change_id":crate::ulid::generate().unwrap(),"item_id":task,"project_id":project,"title":"Retained legacy task"});
    assert!(matches!(
        office
            .work_call(
                Caller::app(owner),
                WorkCall::TaskCreate(serde_json::from_value(p).unwrap())
            )
            .unwrap(),
        TxnOutput::Committed { .. }
    ));
}
#[test]
fn actual_office_org_source_adopts_owner_reported_legacy_without_employee_and_fences_only_adopted_id_after_reopen(
) {
    let (root, key, office) = made();
    let org = office.info().org_id.clone();
    let owner = office
        .org_snapshot()
        .unwrap()
        .members
        .into_iter()
        .find(|m| m.owner)
        .unwrap()
        .id;
    let team = crate::ulid::generate().unwrap();
    let project = crate::ulid::generate().unwrap();
    let task = crate::ulid::generate().unwrap();
    let project_row = serde_json::to_string(
        &json!({"id":project,"key":"old","name":"Historical project","version":1}),
    )
    .unwrap();
    office
        .org_store()
        .write(move |db| {
            db.execute(
                "INSERT INTO projects VALUES(json_extract(?1,'$.id'),?1)",
                [&project_row],
            )
            .map_err(OfficeError::storage)?;
            Ok(((), None))
        })
        .unwrap();
    create(&office, &owner, &project, &task);
    let (legacy, event) = raw(&office, &task);
    let item: org_work::StoredItem = serde_json::from_str(&legacy).unwrap();
    assert_eq!(item.number, Some(1));
    assert_eq!(item.created_by, owner);
    assert!(item.assignee_id.is_none());
    let position = office
        .org_store()
        .read(org_db::last_position)
        .unwrap()
        .unwrap();
    let initial_origin = office
        .local_store()
        .read(crate::store::origin::local_origin)
        .unwrap();
    let before = office
        .local_store()
        .read(|db| crate::store::origin::entries_after(db, &initial_origin.incarnation, 0, 1024))
        .unwrap();
    assert_eq!(before.len(), 1);
    // Existing pre-v3 installation adopts its explicit instance through the
    // ordinary exclusive Office opener before selected writer admission.
    let instance = crate::ulid::generate().unwrap();
    drop(office);
    let office = Office::open_for_instance(folder_of(&root, ID), key.clone(), &instance).unwrap();
    let config = AcceptanceConfig {
        authority: None,
        identity: AcceptanceIdentity {
            instance_id: instance.clone(),
            organization_id: org.clone(),
            office_id: ID.into(),
            journal_incarnation: "existing-journal".into(),
            writer_epoch: 1,
            machine_id: "fixture-machine".into(),
        },
        protected_root: root.path().join("protected"),
        public_root: root.path().join("public"),
    };
    // The actual host verifier is tested separately. This private unit authority
    // cannot be selected by any production entry point.
    let authority: Arc<dyn OwnerAuthority> = Arc::new(FixtureOwner(owner.clone()));
    office.install_selected_acceptance(config.clone()).unwrap();
    office.bind_selected_owner(authority.clone()).unwrap();
    let directory = json!({"schema_version":1,"organization_id":org,"source_position":position,
      "teams":{team.clone():{"schema_version":1,"revision":1,"id":team,"name":"Historical accountable team","organization_id":org,"number":1,"status":"active","parent_team_id":null,"lead_employee_id":null}},
      "employees":{},"projects":{project.clone():{"schema_version":1,"revision":1,"id":project,"name":"Historical project","organization_id":org,"number":1,"owner_team_id":team}}});
    assert_eq!(
        office
            .admit_selected_directory(
                authority.clone(),
                None,
                serde_json::to_vec(&directory).unwrap()
            )
            .unwrap(),
        1
    );
    let at = org_work::format_ms(item.created_at_ms);
    let snapshot=format!("---\nschema_version: 1\nid: {task}\nnumber: 1\ntype: chore\nteam_id: {team}\nproject_id: {project}\ntitle: Retained legacy task\nstatus: todo\nrevision: 1\nassigned_team_id: null\nassignee_id: null\ncreated_at: {at}\ncreated_by_kind: owner\ncreated_by_id: null\nupdated_at: null\nupdated_by_kind: unknown\nupdated_by_id: null\nlexical: 1.234567890123456789E+012\n---\nRetained public café body.\n").into_bytes();
    let mapping = json!({"version":1,"source_kind":"public_legacy_work","source_snapshot_sha256":codec::sha256(legacy.as_bytes()),"source_event_sha256":codec::sha256(event.as_bytes()),"source_generation":item.generation,"source_version":1,"original_kind":"task","original_reference":"T-1","accountable_team_id":team,"project_id":project,"scope_kind":"project","scope_id":project,"number":1,"reporter":{"legacy_id":owner,"kind":"owner","id":null},"assignee":{"legacy_id":null,"employee_id":null},"public_extensions":{"snapshot":[],"event":[]}});
    let change = json!({"type":"history_baseline","known_facts":{"id":task,"team_id":team,"project_id":project,"revision":1,"assigned_team_id":null,"assignee_id":null},"public_snapshot":{"encoding":"utf-8-markdown","text":std::str::from_utf8(&snapshot).unwrap()}});
    let baseline = history::encode(
        EventFields {
            organization_id: org.clone(),
            task_id: task.clone(),
            revision: 1,
            occurred_at: None,
            actor_kind: "unknown".into(),
            actor_id: None,
            operation_id: None,
            change: codec::parse(&serde_json::to_vec(&change).unwrap()).unwrap(),
            source_metadata: BTreeMap::from([
                (
                    "legacy_snapshot".into(),
                    codec::parse(legacy.as_bytes()).unwrap(),
                ),
                (
                    "legacy_event".into(),
                    codec::parse(event.as_bytes()).unwrap(),
                ),
                (
                    "legacy_mapping".into(),
                    codec::parse(&serde_json::to_vec(&mapping).unwrap()).unwrap(),
                ),
            ]),
            extensions: BTreeMap::new(),
        },
        Admission::Import,
    )
    .unwrap();
    let counter =
        format!("scope_kind: project\nscope_id: {project}\nentity_kind: task\nlast_number: 1\n")
            .into_bytes();
    let allocation=format!("scope_kind: project\nscope_id: {project}\nentity_kind: task\nnumber: 1\nentity_id: {task}\nallocated_at: {at}\n").into_bytes();
    let canonical = format!("projects/{project}/work/tasks/{task}.md");
    let manifest = serde_json::to_vec(&adoption::Manifest {
        version: 1,
        resource_profile: codec::RESOURCE_PROFILE.into(),
        task_id: task.clone(),
        canonical_path: canonical.clone(),
        public_snapshot: blob(&snapshot),
        history_origin: adoption::HistoryOrigin::Baseline {
            revision: Revision::new(1).unwrap(),
        },
        events: vec![adoption::Event {
            revision: Revision::new(1).unwrap(),
            blob: blob(&baseline),
        }],
        public_blobs: vec![],
        number_counter: Some(blob(&counter)),
        number_allocation: Some(blob(&allocation)),
    })
    .unwrap();
    let stage = org_proto::gateway::OperationId::parse(crate::ulid::generate().unwrap()).unwrap();
    let hash = codec::sha256(&manifest);
    for bytes in [&manifest, &snapshot, &baseline, &counter, &allocation] {
        assert!(
            office
                .stage_selected_sources(
                    authority.clone(),
                    staging::Params {
                        stage_id: stage.clone(),
                        manifest_sha256: hash.clone(),
                        blob: blob(bytes),
                        chunk: Some(staging::Chunk {
                            offset: 0,
                            hex: bytes.iter().map(|b| format!("{b:02x}")).collect()
                        })
                    }
                )
                .unwrap()
                .verified
        );
    }
    let action = adoption::Action::Commit {
        operation_id: org_proto::gateway::OperationId::parse(crate::ulid::generate().unwrap())
            .unwrap(),
        stage_id: stage,
        manifest_sha256: hash,
        expected_directory_generation: Revision::new(1).unwrap(),
    };
    let accepted = office
        .adopt_selected_task(authority.clone(), action.clone())
        .unwrap();
    assert!(
        matches!(accepted, adoption::Outcome::Completed { revision: 1, .. }),
        "{accepted:?}"
    );
    assert_eq!(raw(&office, &task), (legacy.clone(), event.clone()));
    let after = office
        .local_store()
        .selected_read(move |db, _| {
            crate::store::origin::entries_after(db, &initial_origin.incarnation, 0, 1024)
        })
        .unwrap();
    assert_eq!(
        after, before,
        "adoption must not allocate a replacement legacy event identity"
    );
    let assignment = AssignmentRequest {
        operation_id: crate::ulid::generate().unwrap(),
        task_id: task.clone(),
        command: serde_json::from_value(
            json!({"expected_revision":1,"assigned_team_id":team,"assignee_id":null}),
        )
        .unwrap(),
    };
    assert!(matches!(
        office
            .set_selected_assignment(authority.clone(), assignment)
            .unwrap(),
        AssignmentOutcome::Completed { revision: 2, .. }
    ));
    let edited = std::fs::read(config.public_root.join(&canonical)).unwrap();
    assert!(edited.ends_with(b"Retained public caf\xc3\xa9 body.\n"));
    assert_eq!(raw(&office, &task), (legacy.clone(), event.clone()));
    drop(office);
    let office = Office::open(folder_of(&root, ID), key).unwrap();
    office.install_selected_acceptance(config).unwrap();
    office.bind_selected_owner(authority.clone()).unwrap();
    assert_eq!(
        office.adopt_selected_task(authority, action).unwrap(),
        accepted
    );
    let p = json!({"change_id":crate::ulid::generate().unwrap(),"item_id":task,"expected_version":1,"assignee_id":owner});
    assert!(office
        .work_call(
            Caller::app(&owner),
            WorkCall::TaskAssign(serde_json::from_value(p).unwrap())
        )
        .is_err());
    assert_eq!(raw(&office, &task), (legacy, event));
    let control = crate::ulid::generate().unwrap();
    create(&office, &owner, &project, &control);
    let p = json!({"change_id":crate::ulid::generate().unwrap(),"item_id":control,"expected_version":1,"assignee_id":owner});
    assert!(matches!(
        office
            .work_call(
                Caller::app(&owner),
                WorkCall::TaskAssign(serde_json::from_value(p).unwrap())
            )
            .unwrap(),
        TxnOutput::Committed { .. }
    ));
    let control: org_work::StoredItem = serde_json::from_str(&raw(&office, &control).0).unwrap();
    assert_eq!(control.assignee_id.as_deref(), Some(owner.as_str()));
    assert_eq!(control.number, Some(2));
}
