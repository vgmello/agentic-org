//! Existing-installation fixtures keep their original SQL TEXT bytes. Imports
//! use the real protected staging, writer, journals, gates and reopen path.
use super::*;
use crate::selected_tasks::{adoption, staging};
use org_proto::selected_tasks::{adoption::Action, adoption::Outcome, Revision};
use org_proto::work::{ItemKind, WorkEvent};
use org_work::StoredItem;
use serde_json::json;
mod gates;

struct Legacy {
    item: StoredItem,
    snapshot: String,
    event: String,
}
impl Legacy {
    fn new(kind: &str) -> Self {
        let id = org_work::ulid_from_parts(1_793_000_000_000, 71);
        let reporter = org_work::ulid_from_parts(1_793_000_000_000, 72);
        let mut item: StoredItem = serde_json::from_value(json!({
            "id":id,"kind":kind,"project_id":if kind=="request" {None} else {Some("project")},
            "number":43,"title":"Legacy café","status":if kind=="request" {"new"} else {"todo"},"resolution":null,
            "priority":null,"assignee_id":reporter,"created_by":reporter,
            "created_at_ms":1_793_000_000_000_u64,"updated_at_ms":1_793_000_000_000_u64,
            "owner_office_id":"office","generation":1,"version":1,"deleted":false
        }))
        .unwrap();
        if kind != "request" {
            use org_work::{
                testkit::{MemOrg, MemStore, TestClock},
                Caller, TxnOutput, WorkCall, WorkService,
            };
            let mut org = MemOrg::new("owner-member")
                .with_person(&reporter)
                .with_project("project", "tst", &["team"])
                .with_team_lead("team", &reporter);
            org.position = org_proto::Position { epoch: 1, seq: 9 };
            let memory = Arc::new(MemStore::new(org));
            let service =
                WorkService::new(memory.clone(), TestClock::at(item.created_at_ms), "office");
            let mut p = json!({"change_id":org_work::ulid_from_parts(item.created_at_ms,70),"item_id":id,"project_id":"project","title":"Legacy café"});
            let call = if kind == "bug" {
                p["what_happened"] = json!("Observed public bug");
                p["severity"] = json!("minor");
                WorkCall::BugReport(serde_json::from_value(p).unwrap())
            } else {
                p["assignee_id"] = json!(reporter);
                WorkCall::TaskCreate(serde_json::from_value(p).unwrap())
            };
            assert!(matches!(
                service.call(Caller::app(&reporter), call).unwrap(),
                TxnOutput::Committed { .. }
            ));
            let snapshot = memory.snapshot();
            item = snapshot.items[&id].clone();
            assert_eq!(item.version, 1);
            // This installation's permanent allocation is 43, not a new
            // selected allocation. The immutable Created carries no number.
            item.number = Some(43);
        }
        let mut snapshot = serde_json::to_string(&item).unwrap();
        snapshot.pop();
        snapshot.push_str(r#", "public_extension":{"decimal":123456789012345678901234567890.123456789E+012,"negative":-0,"huge":1e+99999999999999999999999999,"a/b~c":true}}"#);
        let event = format!(
            r#"{{"target_id":"{}","version":1,"at":"{}","actor_id":"{}","org_position":{{"epoch":1,"seq":9}},"change":{{"type":"created","future":1E+0009}},"public_event":{{"n":18446744073709551616000000}}}}"#,
            item.id,
            org_work::format_ms(item.created_at_ms),
            item.created_by
        );
        // The known typed schema is still valid; unknown numbers deliberately
        // cannot pass through serde_json::Value without changing their content.
        serde_json::from_str::<StoredItem>(&snapshot).unwrap();
        serde_json::from_str::<WorkEvent>(&event).unwrap();
        Self {
            item,
            snapshot,
            event,
        }
    }
    fn source(&self) -> Vec<u8> {
        SOURCE
            .replace("opaque-task", &self.item.id)
            .replace("number: 42", "number: 43")
            .replace(
                "type: request",
                match self.item.kind {
                    ItemKind::Bug => "type: bug",
                    ItemKind::Task => "type: chore",
                    _ => "type: request",
                },
            )
            .replace(
                "project_id: null",
                if self.item.project_id.is_some() {
                    "project_id: project"
                } else {
                    "project_id: null"
                },
            )
            .replace("title: Test task", "title: Legacy café")
            .replace(
                "status: todo",
                if self.item.kind != ItemKind::Task {
                    "status: new"
                } else {
                    "status: todo"
                },
            )
            .replace(
                "created_at: null",
                &format!(
                    "created_at: {}",
                    org_work::format_ms(self.item.created_at_ms)
                ),
            )
            .replace("created_by_kind: unknown", "created_by_kind: employee")
            .replace("created_by_id: null", "created_by_id: employee")
            .replace(
                "assignee_id: null",
                if self.item.assignee_id.is_some() {
                    "assignee_id: employee"
                } else {
                    "assignee_id: null"
                },
            )
            .into_bytes()
    }
    fn mapping(&self) -> serde_json::Value {
        let project = self.item.project_id.as_deref();
        json!({"version":1,"source_kind":"public_legacy_work",
            "source_snapshot_sha256":codec::sha256(self.snapshot.as_bytes()),
            "source_event_sha256":codec::sha256(self.event.as_bytes()),
            "source_generation":self.item.generation,"source_version":1,
            "original_kind":match self.item.kind {ItemKind::Bug=>"bug",ItemKind::Task=>"task",_=>"request"},
            "original_reference":match self.item.kind {ItemKind::Bug=>"B-43",ItemKind::Task=>"T-43",_=>"REQ-43"},
            "accountable_team_id":"accountable","project_id":project,
            "scope_kind":if project.is_some() {"project"} else {"team"},
            "scope_id":project.unwrap_or("accountable"),"number":43,
            "reporter":{"legacy_id":self.item.created_by,"kind":"employee","id":"employee"},
            "assignee":{"legacy_id":self.item.assignee_id,"employee_id":self.item.assignee_id.as_ref().map(|_|"employee")},
            "public_extensions":{"snapshot":["/public_extension"],"event":["/change/future","/public_event"]}})
    }
    fn template(&self, source: &[u8], mapping: serde_json::Value) -> Vec<u8> {
        let change=codec::parse(&serde_json::to_vec(&json!({"type":"history_baseline","known_facts":{"id":self.item.id,"team_id":"accountable","project_id":self.item.project_id,"revision":1,"assigned_team_id":null,"assignee_id":self.item.assignee_id.as_ref().map(|_|"employee")},"public_snapshot":{"encoding":"utf-8-markdown","text":std::str::from_utf8(source).unwrap()}})).unwrap()).unwrap();
        history::encode(
            EventFields {
                organization_id: "org".into(),
                task_id: self.item.id.clone(),
                revision: 1,
                occurred_at: None,
                actor_kind: "unknown".into(),
                actor_id: None,
                operation_id: None,
                change,
                source_metadata: BTreeMap::from([
                    (
                        "legacy_snapshot".into(),
                        codec::parse(self.snapshot.as_bytes()).unwrap(),
                    ),
                    (
                        "legacy_event".into(),
                        codec::parse(self.event.as_bytes()).unwrap(),
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
        .unwrap()
    }
}
fn setup(source: &Legacy) -> (TempDir, Store, AcceptanceConfig) {
    let dir = TempDir::new().unwrap();
    let store = Store::open(
        &dir.path().join("office.db"),
        &Key::from_bytes([31; 32]),
        Opening::New,
        &OFFICE_SCHEMA,
    )
    .unwrap();
    let item = source.item.clone();
    let snapshot = source.snapshot.clone();
    let event = source.event.clone();
    store.write(move|db| {
        db.execute("INSERT INTO meta VALUES('office_id','office'),('instance_id','instance'),('message_sequence','41')",[]).map_err(storage)?;
        db.execute("INSERT INTO work_items VALUES(?1,?2,?3,?4)",params![item.id,item.generation as i64,item.version as i64,snapshot]).map_err(storage)?;
        db.execute("INSERT INTO work_ids VALUES(?1,?1,'item')",[&item.id]).map_err(storage)?;
        let refs=vec![org_proto::work::EventRef {target_id:item.id.clone(),version:1}];
        db.execute("INSERT INTO work_feed(change_id,targets) VALUES('legacy-created',?1)",[serde_json::to_string(&refs).unwrap()]).map_err(storage)?;
        let feed=db.last_insert_rowid();
        db.execute("INSERT INTO work_events VALUES(?1,1,?2,?3)",params![item.id,feed,event]).map_err(storage)?;
        crate::store::origin::append(db,org_proto::Position{epoch:1,seq:9},&[crate::store::origin::OriginRef{target_id:item.id.clone(),generation:item.generation,version:1}],feed as u64,1_793_000_000_000)?;
        Ok(((),None))
    }).unwrap();
    let config = AcceptanceConfig {
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
    };
    store.install_acceptance(config.clone()).unwrap();
    super::super::super::directory::bind_owner(&store, authority()).unwrap();
    let mut directory = String::from_utf8(directory_source(true)).unwrap();
    if source.item.project_id.is_some() {
        directory=directory.replace("\"projects\":{}",r#""projects":{"project":{"schema_version":1,"revision":1,"id":"project","name":"Project","organization_id":"org","owner_team_id":"accountable"}}"#);
    }
    super::super::super::directory::admit(&store, authority(), None, directory.into_bytes())
        .unwrap();
    (dir, store, config)
}
fn stage(
    store: &Store,
    root: &std::path::Path,
    legacy: &Legacy,
    source: Vec<u8>,
    event: Vec<u8>,
) -> Action {
    let stage = crate::ulid::generate().unwrap();
    let (scope, scope_id, path) = if legacy.item.project_id.is_some() {
        (
            "project",
            "project",
            format!("projects/project/work/tasks/{}.md", legacy.item.id),
        )
    } else {
        (
            "team",
            "accountable",
            format!("teams/accountable/work/tasks/{}.md", legacy.item.id),
        )
    };
    let counter =
        format!("scope_kind: {scope}\nscope_id: {scope_id}\nentity_kind: task\nlast_number: 43\n")
            .into_bytes();
    let allocation=format!("scope_kind: {scope}\nscope_id: {scope_id}\nentity_kind: task\nnumber: 43\nentity_id: {}\nallocated_at: 2026-01-01T00:00:00Z\n",legacy.item.id).into_bytes();
    let blob = |b: &[u8]| json!({"sha256":codec::sha256(b),"byte_length":b.len()});
    let manifest=serde_json::to_vec(&json!({"version":1,"resource_profile":codec::RESOURCE_PROFILE,"task_id":legacy.item.id,"canonical_path":path,"public_snapshot":blob(&source),"history_origin":{"kind":"baseline","revision":1},"events":[{"revision":1,"blob":blob(&event)}],"public_blobs":[],"number_counter":blob(&counter),"number_allocation":blob(&allocation)})).unwrap();
    let hash = codec::sha256(&manifest);
    for bytes in [&manifest, &source, &event, &counter, &allocation] {
        let mut request = staged_request(&stage, bytes, Some(0));
        request.manifest_sha256 = hash.clone();
        assert!(
            staging::run(store, root.to_path_buf(), authority(), request)
                .unwrap()
                .verified
        );
    }
    Action::Commit {
        operation_id: org_proto::gateway::OperationId::parse(crate::ulid::generate().unwrap())
            .unwrap(),
        stage_id: org_proto::gateway::OperationId::parse(stage).unwrap(),
        manifest_sha256: hash,
        expected_directory_generation: Revision::new(1).unwrap(),
    }
}
fn run(
    store: &Store,
    root: std::path::PathBuf,
    auth: Arc<dyn OwnerAuthority>,
    action: Action,
) -> Result<Outcome, OfficeError> {
    let reporter = org_work::ulid_from_parts(1_793_000_000_000, 72);
    let mut org = org_work::testkit::MemOrg::new("owner-member").with_person(&reporter);
    org.position = org_proto::Position { epoch: 1, seq: 9 };
    adoption::run_with_org(store, root, auth, action, Some(Arc::new(org)))
}
fn retained(store: &Store, task: &str) -> (String, String, i64, i64, String) {
    let task = task.to_owned();
    store
        .selected_read(move |db, _| {
            Ok((
                db.query_row("SELECT data FROM work_items WHERE id=?1", [&task], |r| {
                    r.get(0)
                })
                .map_err(storage)?,
                db.query_row(
                    "SELECT data FROM work_events WHERE target_id=?1",
                    [&task],
                    |r| r.get(0),
                )
                .map_err(storage)?,
                db.query_row("SELECT sum(last_seq) FROM work_origin_counter", [], |r| {
                    r.get(0)
                })
                .map_err(storage)?,
                db.query_row(
                    "SELECT feed_seq FROM work_events WHERE target_id=?1",
                    [&task],
                    |r| r.get(0),
                )
                .map_err(storage)?,
                db.query_row(
                    "SELECT value FROM meta WHERE key='message_sequence'",
                    [],
                    |r| r.get(0),
                )
                .map_err(storage)?,
            ))
        })
        .unwrap()
}
#[test]
fn legacy_profile_one_preserves_raw_sources_number_reference_origin_and_original_retry_after_assignment_and_reopen(
) {
    for kind in ["request", "bug", "task"] {
        let legacy = Legacy::new(kind);
        let (dir, store, config) = setup(&legacy);
        let before = retained(&store, &legacy.item.id);
        let source = legacy.source();
        let template = legacy.template(&source, legacy.mapping());
        let root = dir.path().join("staging");
        let action = stage(&store, &root, &legacy, source.clone(), template.clone());
        let operation = match &action {
            Action::Commit { operation_id, .. } => operation_id.as_str().to_owned(),
            _ => unreachable!(),
        };
        let expected = Outcome::Completed {
            task_id: legacy.item.id.clone(),
            revision: 1,
            history_coverage: "snapshot_baseline".into(),
        };
        assert_eq!(
            run(&store, root.clone(), authority(), action.clone()).unwrap(),
            expected
        );
        assert_eq!(retained(&store, &legacy.item.id), before);
        let path = if legacy.item.project_id.is_some() {
            format!("projects/project/work/tasks/{}.md", legacy.item.id)
        } else {
            format!("teams/accountable/work/tasks/{}.md", legacy.item.id)
        };
        assert_eq!(
            std::fs::read(config.public_root.join(&path)).unwrap(),
            source
        );
        let baseline = std::fs::read(
            config
                .public_root
                .join(format!("work-history/tasks/{}/1.json", legacy.item.id)),
        )
        .unwrap();
        assert_ne!(baseline, template); // Template was never an accepted selected identity.
        let baseline = history::verify(
            &baseline,
            history::ExpectedIdentity {
                organization_id: "org",
                task_id: &legacy.item.id,
                filename: "1.json",
            },
            Admission::New,
        )
        .unwrap();
        assert_eq!(
            baseline
                .value()
                .get("actor_kind")
                .and_then(codec::Json::as_str),
            Some("owner")
        );
        assert_eq!(
            baseline
                .value()
                .get("operation_id")
                .and_then(codec::Json::as_str),
            Some(operation.as_str())
        );
        assert!(baseline
            .value()
            .get("occurred_at")
            .and_then(codec::Json::as_str)
            .is_some());
        let metadata = baseline.value().get("source_metadata").unwrap();
        assert_eq!(
            metadata.get("legacy_snapshot"),
            Some(&codec::parse(legacy.snapshot.as_bytes()).unwrap())
        );
        assert_eq!(
            metadata.get("legacy_event"),
            Some(&codec::parse(legacy.event.as_bytes()).unwrap())
        );
        let mut change = request(1, Some("doing-team"), None);
        change.task_id = legacy.item.id.clone();
        assert!(matches!(
            assign(&store, authority(), change).unwrap(),
            AssignmentOutcome::Completed { revision: 2, .. }
        ));
        assert_eq!(
            run(&store, root.clone(), authority(), action.clone()).unwrap(),
            expected
        );
        assert_eq!(retained(&store, &legacy.item.id).0, legacy.snapshot);
        // A selected assignment consumes its own original event position; adoption
        // consumed zero. Source work and message/feed positions are untouched.
        let after = retained(&store, &legacy.item.id);
        assert_eq!(after.2, before.2 + 1);
        assert_eq!(after.3, before.3);
        assert_eq!(after.4, before.4);
        drop(store);
        let store = Store::open(
            &dir.path().join("office.db"),
            &Key::from_bytes([31; 32]),
            Opening::Existing,
            &OFFICE_SCHEMA,
        )
        .unwrap();
        store.install_acceptance(config.clone()).unwrap();
        assert_eq!(run(&store, root, authority(), action).unwrap(), expected);
        assert!(
            String::from_utf8(std::fs::read(config.public_root.join(path)).unwrap())
                .unwrap()
                .contains("revision: 2\n")
        );
        assert_eq!(retained(&store, &legacy.item.id), after);
    }
}

#[test]
fn legacy_profile_one_stages_private_unclassified_changed_or_invented_mappings_without_projection()
{
    for case in [
        "unclassified",
        "private",
        "source_hash",
        "reporter",
        "owner_employee",
        "reference",
        "assignee",
        "generation",
        "source_bytes",
    ] {
        let mut legacy = Legacy::new("request");
        if case == "private" {
            legacy.snapshot = legacy.snapshot.replace("public_extension", "private_notes");
        }
        let (dir, store, config) = setup(&legacy);
        let before = retained(&store, &legacy.item.id);
        let mut mapping = legacy.mapping();
        match case {
            "unclassified" => mapping["public_extensions"]["event"] = json!([]),
            "private" => mapping["public_extensions"]["snapshot"] = json!(["/private_notes"]),
            "source_hash" => mapping["source_event_sha256"] = json!("0".repeat(64)),
            "reporter" => mapping["reporter"]["id"] = json!("untrusted"),
            "owner_employee" => mapping["reporter"]["kind"] = json!("owner"),
            "reference" => mapping["original_reference"] = json!("B-43"),
            "assignee" => mapping["assignee"]["legacy_id"] = serde_json::Value::Null,
            "generation" => mapping["source_generation"] = json!(2),
            "source_bytes" => legacy.event = legacy.event.replace("1E+0009", "1e9"),
            _ => unreachable!(),
        }
        let source = legacy.source();
        let template = legacy.template(&source, mapping);
        let root = dir.path().join("staging");
        let action = stage(&store, &root, &legacy, source, template);
        assert!(
            matches!(
                run(&store, root.clone(), authority(), action.clone()).unwrap(),
                Outcome::Staged { .. }
            ),
            "{case}"
        );
        assert_eq!(retained(&store, &legacy.item.id), before, "{case}");
        let task = legacy.item.id.clone();
        store
            .selected_read(move |db, _| {
                assert_eq!(
                    db.query_row(
                        "SELECT count(*) FROM selected_tasks WHERE task_id=?1",
                        [task],
                        |r| r.get::<_, i64>(0)
                    )
                    .map_err(storage)?,
                    0
                );
                Ok(())
            })
            .unwrap();
        assert!(!config
            .public_root
            .join(format!("work-history/tasks/{}/1.json", legacy.item.id))
            .exists());
        let Action::Commit { stage_id, .. } = action else {
            unreachable!()
        };
        assert!(root.join(stage_id.as_str()).exists());
    }
}
