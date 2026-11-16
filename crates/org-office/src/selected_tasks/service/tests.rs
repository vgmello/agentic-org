use super::*;
use crate::{
    office::OFFICE_SCHEMA,
    store::{
        acceptance::{AcceptanceConfig, AcceptanceIdentity},
        Opening,
    },
    Key,
};
use tempfile::TempDir;
mod crash_reopen;
mod directory_import;
mod legacy_import;

struct Authority {
    account: String,
    valid: bool,
}
impl OwnerAuthority for Authority {
    fn verify_live(&self, _: &AcceptanceIdentity) -> Result<OwnerAccount, OfficeError> {
        if !self.valid {
            return Err(unauthorized());
        }
        Ok(OwnerAccount {
            account_id: self.account.clone(),
            member_id: "owner-member".into(),
        })
    }
}
fn authority() -> Arc<dyn OwnerAuthority> {
    Arc::new(Authority {
        account: "account".into(),
        valid: true,
    })
}
fn directory_source(active: bool) -> Vec<u8> {
    let source = r#"{"schema_version":1,"organization_id":"org","source_position":{"epoch":1,"seq":9},"teams":{"accountable":{"schema_version":1,"revision":1,"id":"accountable","name":"Accountable","organization_id":"org","status":"active","parent_team_id":null,"lead_employee_id":null},"doing-team":{"schema_version":1,"revision":1,"id":"doing-team","name":"Doing","organization_id":"org","status":"TEAM-TARGET","parent_team_id":null,"lead_employee_id":null}},"employees":{"employee":{"schema_version":1,"revision":1,"id":"employee","name":"Employee","handle":"employee","kind":"person","organization_id":"org","status":"ACTIVE-TARGET","home_team_id":"accountable","reports_to_employee_id":null}},"projects":{},"extension":123456789012345678901234567890.12345678901234567890E+012}"#;
    source
        .replace("TEAM-TARGET", if active { "active" } else { "archived" })
        .replace("ACTIVE-TARGET", if active { "active" } else { "inactive" })
        .into_bytes()
}
const SOURCE:&str="---\nschema_version: 1\nid: opaque-task\nnumber: 42\ntype: request\nteam_id: accountable\nproject_id: null\ntitle: Test task\nstatus: todo\nrevision: 1\nassigned_team_id: null\nassignee_id: null\ncreated_at: null\ncreated_by_kind: unknown\ncreated_by_id: null\nupdated_at: null\nupdated_by_kind: unknown\nupdated_by_id: null\nextension:\n  lexical: 1.234567890123456789E+012\n---\nBody remains café.\n";
fn fixture() -> (TempDir, Store, AcceptanceConfig) {
    fixture_at_revision(1)
}

fn staged_request(
    stage: &str,
    source: &[u8],
    offset: Option<usize>,
) -> org_proto::selected_tasks::staging::Params {
    use org_proto::selected_tasks::staging::{Blob, Chunk, Params};
    Params {
        stage_id: org_proto::gateway::OperationId::parse(stage.into()).unwrap(),
        manifest_sha256: "a".repeat(64),
        blob: Blob {
            sha256: codec::sha256(source),
            byte_length: source.len() as u64,
        },
        chunk: offset.map(|offset| Chunk {
            offset: offset as u64,
            hex: source[offset..source.len().min(offset + codec::MAX_CHUNK_BYTES)]
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        }),
    }
}

#[test]
fn protected_stage_retains_opaque_chunks_across_reopen_without_accepting_or_journaling_them() {
    use super::super::staging;
    let (dir, store, config) = fixture();
    let root = dir.path().join("staging");
    let stage = crate::ulid::generate().unwrap();
    let mut source = vec![0xff; codec::MAX_CHUNK_BYTES + 7];
    source[..7].copy_from_slice(b"opaque\0");
    let ordinal = store
        .selected_read(|db, _| {
            db.query_row("SELECT ordinal FROM selected_journal_cursor", [], |r| {
                r.get::<_, i64>(0)
            })
            .map_err(storage)
        })
        .unwrap();
    let status = staging::run(
        &store,
        root.clone(),
        authority(),
        staged_request(&stage, &source, Some(0)),
    )
    .unwrap();
    assert_eq!(status.chunk_offsets, [0]);
    assert!(!status.verified);
    assert!(serde_json::to_vec(&status).unwrap().len() < org_proto::MAX_FRAME_BYTES);
    drop(store);
    let store = Store::open(
        &dir.path().join("office.db"),
        &Key::from_bytes([31; 32]),
        Opening::Existing,
        &OFFICE_SCHEMA,
    )
    .unwrap();
    store.install_acceptance(config).unwrap();
    let status = staging::run(
        &store,
        root.clone(),
        authority(),
        staged_request(&stage, &source, None),
    )
    .unwrap();
    assert_eq!(status.chunk_offsets, [0]);
    assert!(!status.verified);
    let status = staging::run(
        &store,
        root.clone(),
        authority(),
        staged_request(&stage, &source, Some(codec::MAX_CHUNK_BYTES)),
    )
    .unwrap();
    assert_eq!(status.chunk_offsets, [0, codec::MAX_CHUNK_BYTES as u64]);
    assert!(status.verified);
    // Exact lost-reply retry compares original bytes and preserves both offsets.
    assert!(
        staging::run(
            &store,
            root.clone(),
            authority(),
            staged_request(&stage, &source, Some(0))
        )
        .unwrap()
        .verified
    );
    let bytes = std::fs::read(
        root.join(&stage)
            .join("blobs")
            .join(codec::sha256(&source))
            .join("0.bin"),
    )
    .unwrap();
    assert_eq!(bytes, source[..codec::MAX_CHUNK_BYTES]);
    store
        .selected_read(move |db, _| {
            assert_eq!(
                db.query_row("SELECT ordinal FROM selected_journal_cursor", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                ordinal
            );
            assert_eq!(
                db.query_row("SELECT COUNT(*) FROM selected_tasks", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                1
            );
            assert!(!db
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM selected_artifacts WHERE path LIKE '%staging%')",
                    [],
                    |r| r.get::<_, bool>(0)
                )
                .unwrap());
            Ok(())
        })
        .unwrap();
}

#[test]
fn protected_stage_checks_owner_identity_aggregate_limits_and_never_overwrites_a_chunk() {
    use super::super::staging;
    let (dir, store, _) = fixture();
    let root = dir.path().join("staging");
    let stage = crate::ulid::generate().unwrap();
    let denied = Arc::new(Authority {
        account: "other-account".into(),
        valid: true,
    });
    assert!(staging::run(
        &store,
        root.clone(),
        denied,
        staged_request(&stage, b"original", Some(0))
    )
    .is_err());
    assert!(!root.exists());
    let p = staged_request(&stage, b"original", Some(0));
    let hash = p.blob.sha256.clone();
    assert!(
        staging::run(&store, root.clone(), authority(), p)
            .unwrap()
            .verified
    );
    let mut conflict = staged_request(&stage, b"original", Some(0));
    conflict.chunk.as_mut().unwrap().hex = "636f6e666c696374".into();
    assert!(staging::run(&store, root.clone(), authority(), conflict).is_err());
    assert_eq!(
        std::fs::read(root.join(&stage).join("blobs").join(hash).join("0.bin")).unwrap(),
        b"original"
    );
    let mut changed = staged_request(&stage, b"original", None);
    changed.manifest_sha256 = "b".repeat(64);
    assert!(staging::run(&store, root.clone(), authority(), changed).is_err());
    let another = crate::ulid::generate().unwrap();
    let mut large = staged_request(&another, &vec![0; codec::MAX_CHUNK_BYTES], Some(0));
    large.blob.byte_length = codec::MAX_BLOB_BYTES;
    assert!(
        !staging::run(&store, root.clone(), authority(), large)
            .unwrap()
            .verified
    );
    assert!(staging::run(
        &store,
        root.clone(),
        authority(),
        staged_request(&another, b"x", Some(0))
    )
    .is_err());
    let invalid = crate::ulid::generate().unwrap();
    let mut p = staged_request(&invalid, b"x", Some(0));
    p.chunk.as_mut().unwrap().offset = 1;
    assert!(staging::run(&store, root.clone(), authority(), p).is_err());
    assert!(!root.join(invalid).exists());
}

#[test]
fn protected_stage_refuses_symlink_redirect_without_touching_destination() {
    use super::super::staging;
    let (dir, store, _) = fixture();
    let root = dir.path().join("staging");
    std::fs::create_dir(&root).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let stage = crate::ulid::generate().unwrap();
    std::os::unix::fs::symlink(outside.path(), root.join(&stage)).unwrap();
    assert!(staging::run(
        &store,
        root,
        authority(),
        staged_request(&stage, b"opaque", Some(0))
    )
    .is_err());
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
}
fn fixture_at_revision(revision: i64) -> (TempDir, Store, AcceptanceConfig) {
    fixture_with_office(revision, "office")
}
fn fixture_with_office(revision: i64, office: &str) -> (TempDir, Store, AcceptanceConfig) {
    fixture_with_source(revision, office, SOURCE)
}
fn fixture_with_source(
    revision: i64,
    office: &str,
    initial: &str,
) -> (TempDir, Store, AcceptanceConfig) {
    let initial = initial.to_owned();
    let dir = TempDir::new().unwrap();
    let store = Store::open(
        &dir.path().join("office.db"),
        &Key::from_bytes([31; 32]),
        Opening::New,
        &OFFICE_SCHEMA,
    )
    .unwrap();
    let stored_office = office.to_owned();
    store
        .write(move |db| {
            db.execute(
                "INSERT INTO meta VALUES('office_id',?1),('instance_id','instance')",
                [&stored_office],
            )
            .map_err(storage)?;
            Ok(((), None))
        })
        .unwrap();
    let config = AcceptanceConfig {
        authority: None,
        identity: AcceptanceIdentity {
            instance_id: "instance".into(),
            organization_id: "org".into(),
            office_id: office.into(),
            journal_incarnation: "journal".into(),
            writer_epoch: 1,
            machine_id: "machine".into(),
        },
        protected_root: dir.path().join("recovery"),
        public_root: dir.path().join("public"),
    };
    store.install_acceptance(config.clone()).unwrap();
    super::super::directory::bind_owner(&store, authority()).unwrap();
    super::super::directory::admit(&store, authority(), None, directory_source(true)).unwrap();
    store.selected_write(move|db| {
        let source=initial.replace("revision: 1",&format!("revision: {revision}"));
        db.execute("INSERT INTO selected_tasks VALUES('org','opaque-task',?1,'accountable',NULL,NULL,NULL,'teams/accountable/work/tasks/opaque-task.md',?2,?3)",params![revision,source.as_bytes(),codec::sha256(source.as_bytes())]).map_err(storage)?;
        db.execute("INSERT INTO selected_adoptions VALUES('org','opaque-task',1,1,?1,'{}')",["a".repeat(64)]).map_err(storage)?;
        put_artifact(db,"public","teams/accountable/work/tasks/opaque-task.md",source.as_bytes(),false)?;
        let change=codec::parse(&serde_json::to_vec(&serde_json::json!({"type":"history_baseline","known_facts":{"id":"opaque-task","team_id":"accountable","project_id":null,"revision":revision,"assigned_team_id":null,"assignee_id":null},"public_snapshot":{"encoding":"utf-8-markdown","text":source}})).unwrap()).unwrap();
        let event=history::encode(EventFields {organization_id:"org".into(),task_id:"opaque-task".into(),revision,occurred_at:None,actor_kind:"unknown".into(),actor_id:None,operation_id:None,change,source_metadata:BTreeMap::new(),extensions:BTreeMap::new()},Admission::Import).unwrap();
        let verified=history::verify(&event,history::ExpectedIdentity {organization_id:"org",task_id:"opaque-task",filename:&format!("{revision}.json")},Admission::Import).unwrap();
        let hash=verified.value().get("payload_sha256").unwrap().as_str().unwrap();
        db.execute("INSERT INTO selected_events VALUES('org','opaque-task',?1,2,?2,?3,?4)",params![revision,codec::RESOURCE_PROFILE,hash,event]).map_err(storage)?;
        put_artifact(db,"public",&format!("work-history/tasks/opaque-task/{revision}.json"),&event,true)?;
        Ok(())
    }).unwrap();
    (dir, store, config)
}
fn request(revision: i64, team: Option<&str>, employee: Option<&str>) -> AssignmentRequest {
    AssignmentRequest {
        operation_id: crate::ulid::generate().unwrap(),
        task_id: "opaque-task".into(),
        command: AssignmentCommand {
            expected_revision: revision,
            assigned_team_id: team.map(str::to_owned),
            assignee_id: employee.map(str::to_owned),
        },
    }
}

#[test]
fn selected_reads_preserve_exact_snapshot_bytes_and_full_revision_without_employee_owner_profile() {
    use org_proto::selected_tasks::read::{Query, Result as Read};
    let (_dir, store, _config) = fixture_at_revision(i64::MAX);
    let Read::Detail {
        task,
        history_coverage,
    } = super::super::read::read(
        &store,
        authority(),
        Query::Detail {
            task_id: "opaque-task".into(),
        },
    )
    .unwrap()
    else {
        panic!("detail");
    };
    assert_eq!(task.revision, i64::MAX);
    assert_eq!(task.accountable_team_id, "accountable");
    assert_eq!(history_coverage, "unknown");
    // This protected security owner is absent from the employee directory.
    assert_ne!(task.assignee_id.as_deref(), Some("owner-member"));
    let mut offset = 0;
    let mut bytes = Vec::new();
    loop {
        let Read::Chunk {
            reference,
            hex,
            next,
            ..
        } = super::super::read::read(
            &store,
            authority(),
            Query::Chunk {
                task_id: task.task_id.clone(),
                reference: task.snapshot.clone(),
                offset,
                limit: 5,
            },
        )
        .unwrap()
        else {
            panic!("chunk");
        };
        assert_eq!(reference, task.snapshot);
        for pair in hex.as_bytes().as_chunks::<2>().0 {
            bytes.push(u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap());
        }
        let Some(next) = next else {
            break;
        };
        offset = next;
    }
    assert_eq!(
        String::from_utf8(bytes).unwrap(),
        SOURCE.replace("revision: 1", &format!("revision: {}", i64::MAX))
    );
}

#[test]
fn selected_reads_filter_complete_pair_queues_and_deny_scope_or_stale_chunks() {
    use org_proto::selected_tasks::read::{Query, Queue, Result as Read, TargetKind};
    let (_dir, store, _config) = fixture();
    let Read::Detail { task, .. } = super::super::read::read(
        &store,
        authority(),
        Query::Detail {
            task_id: "opaque-task".into(),
        },
    )
    .unwrap() else {
        panic!("detail");
    };
    assign(
        &store,
        authority(),
        request(1, Some("doing-team"), Some("employee")),
    )
    .unwrap();
    for queue in [
        Queue::Owned {
            team_id: "accountable".into(),
        },
        Queue::AssignedTeam {
            team_id: "doing-team".into(),
        },
        Queue::AssignedEmployee {
            employee_id: "employee".into(),
        },
    ] {
        let Read::List { items, next } = super::super::read::read(
            &store,
            authority(),
            Query::List {
                after: None,
                queue,
                limit: 1,
            },
        )
        .unwrap() else {
            panic!("list");
        };
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].revision, 2);
        assert!(next.is_none());
    }
    assert!(matches!(
        super::super::read::read(
            &store,
            authority(),
            Query::Chunk {
                task_id: task.task_id.clone(),
                reference: task.snapshot.clone(),
                offset: 0,
                limit: 5
            }
        ),
        Err(OfficeError::NotFound)
    ));
    let wrong = Arc::new(Authority {
        account: "another-owner".into(),
        valid: true,
    });
    assert!(super::super::read::read(
        &store,
        wrong,
        Query::Detail {
            task_id: "opaque-task".into()
        }
    )
    .is_err());
    let Read::Targets { items, .. } = super::super::read::read(
        &store,
        authority(),
        Query::Targets {
            task_id: task.task_id.clone(),
            target_kind: TargetKind::Team,
            after: None,
            limit: 50,
        },
    )
    .unwrap() else {
        panic!("targets");
    };
    assert_eq!(items.len(), 2);
    let Read::Detail { task, .. } = super::super::read::read(
        &store,
        authority(),
        Query::Detail {
            task_id: "opaque-task".into(),
        },
    )
    .unwrap() else {
        panic!("detail");
    };
    assert!(super::super::read::read(
        &store,
        authority(),
        Query::Chunk {
            task_id: "different-task".into(),
            reference: task.snapshot.clone(),
            offset: 0,
            limit: 5
        }
    )
    .is_err());
    assert!(super::super::read::read(
        &store,
        authority(),
        Query::Chunk {
            task_id: task.task_id,
            reference: task.snapshot,
            offset: 0,
            limit: 256 * 1024 + 1
        }
    )
    .is_err());
}

#[test]
fn selected_assignment_publishes_snapshot_event_origin_and_bound_receipt_before_reply() {
    let (_dir, store, config) = fixture();
    let r = request(1, Some("doing-team"), Some("employee"));
    let operation = r.operation_id.clone();
    let out = assign(&store, authority(), r).unwrap();
    assert!(matches!(
        out,
        AssignmentOutcome::Completed { revision: 2, .. }
    ));
    assert_eq!(
        lookup(&store, authority(), operation.clone()).unwrap(),
        Some(out)
    );
    let bytes = std::fs::read(
        config
            .public_root
            .join("teams/accountable/work/tasks/opaque-task.md"),
    )
    .unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("lexical: 1.234567890123456789E+012"));
    assert!(text.ends_with("Body remains café.\n"));
    let event = std::fs::read(
        config
            .public_root
            .join("work-history/tasks/opaque-task/2.json"),
    )
    .unwrap();
    history::verify(
        &event,
        history::ExpectedIdentity {
            organization_id: "org",
            task_id: "opaque-task",
            filename: "2.json",
        },
        Admission::New,
    )
    .unwrap();
    assert!(config
        .protected_root
        .join(format!("offices/office/receipts/{operation}.json"))
        .is_file());
}

#[test]
fn retries_precede_targets_and_revision_checks_but_never_authority() {
    let (_dir, store, _) = fixture();
    let r = request(1, Some("doing-team"), Some("employee"));
    let operation = r.operation_id.clone();
    let original = assign(&store, authority(), r).unwrap();
    let other = assign(&store, authority(), request(2, None, None)).unwrap();
    assert!(matches!(
        other,
        AssignmentOutcome::Completed { revision: 3, .. }
    ));
    let retry = || AssignmentRequest {
        operation_id: operation.clone(),
        task_id: "opaque-task".into(),
        command: AssignmentCommand {
            expected_revision: 1,
            assigned_team_id: Some("doing-team".into()),
            assignee_id: Some("employee".into()),
        },
    };
    assert_eq!(assign(&store, authority(), retry()).unwrap(), original);
    let revoked: Arc<dyn OwnerAuthority> = Arc::new(Authority {
        account: "account".into(),
        valid: false,
    });
    assert!(assign(&store, revoked, retry()).is_err());
    let mut changed = retry();
    changed.command.assignee_id = None;
    assert!(assign(&store, authority(), changed).is_err());
    let foreign: Arc<dyn OwnerAuthority> = Arc::new(Authority {
        account: "other-account".into(),
        valid: true,
    });
    assert!(assign(&store, foreign.clone(), request(3, None, None)).is_err());
    assert_eq!(lookup(&store, foreign.clone(), operation).unwrap(), None);
    assert_eq!(
        lookup(&store, foreign, crate::ulid::generate().unwrap()).unwrap(),
        None
    );
    let expired_auth: Arc<dyn OwnerAuthority> = Arc::new(Authority {
        account: "account".into(),
        valid: false,
    });
    assert!(matches!(
        lookup(&store, expired_auth, crate::ulid::generate().unwrap()),
        Err(OfficeError::Invalid("not authorized"))
    ));
}

#[test]
fn stale_rejection_has_no_event_hole_and_equal_pairs_create_one_event() {
    let (_dir, store, _) = fixture();
    assert!(matches!(
        assign(&store, authority(), request(1, None, None)).unwrap(),
        AssignmentOutcome::Completed { revision: 2, .. }
    ));
    let r = request(1, None, None);
    let operation = r.operation_id.clone();
    let out = assign(&store, authority(), r).unwrap();
    assert!(
        matches!(&out, AssignmentOutcome::Rejected {code, current:Some(state)}
        if code=="conflict" && state.revision==2 && state.assigned_team_id.is_none() && state.assignee_id.is_none())
    );
    assert_eq!(
        lookup(&store, authority(), operation.clone()).unwrap(),
        Some(out.clone())
    );
    assert!(matches!(
        assign(&store, authority(), request(2, None, None)).unwrap(),
        AssignmentOutcome::Completed { revision: 3, .. }
    ));
    assert_eq!(lookup(&store, authority(), operation).unwrap(), Some(out));
    let positions = store
        .read(|db| {
            db.query_row(
                "SELECT ordinal,origin_high_water FROM selected_journal_cursor",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .map_err(storage)
        })
        .unwrap();
    assert_eq!(positions, (6, 2));
}

#[test]
fn all_sixteen_transitions_use_durable_service_and_preserve_source() {
    let pairs = [
        (None, None),
        (Some("doing-team"), None),
        (None, Some("employee")),
        (Some("doing-team"), Some("employee")),
    ];
    for from in pairs {
        for to in pairs {
            let (_root, store, config) = fixture();
            assign(&store, authority(), request(1, from.0, from.1)).unwrap();
            let out = assign(&store, authority(), request(2, to.0, to.1)).unwrap();
            assert!(matches!(
                out,
                AssignmentOutcome::Completed { revision: 3, .. }
            ));
            let bytes = std::fs::read(
                config
                    .public_root
                    .join("teams/accountable/work/tasks/opaque-task.md"),
            )
            .unwrap();
            let text = String::from_utf8(bytes).unwrap();
            let snapshot = TaskSnapshot::parse("org", &text).unwrap();
            assert_eq!(snapshot.assignment().assigned_team_id(), to.0);
            assert_eq!(snapshot.assignment().assignee_id(), to.1);
            assert_eq!(
                snapshot.immutable(),
                TaskSnapshot::parse("org", SOURCE).unwrap().immutable()
            );
            assert!(text.contains("lexical: 1.234567890123456789E+012"));
            assert!(text.ends_with("Body remains café.\n"));
        }
    }
}

#[test]
fn deactivation_and_directory_generation_serialize_with_assignment_and_exact_retries() {
    let (_root, store, _) = fixture();
    let r = request(1, Some("doing-team"), Some("employee"));
    let operation = r.operation_id.clone();
    let original = assign(&store, authority(), r).unwrap();
    assert_eq!(
        super::super::directory::admit(&store, authority(), Some(1), directory_source(false))
            .unwrap(),
        2
    );
    let before = store
        .read(|db| {
            db.query_row("SELECT ordinal FROM selected_journal_cursor", [], |r| {
                r.get::<_, i64>(0)
            })
            .map_err(storage)
        })
        .unwrap();
    assert_eq!(
        assign(
            &store,
            authority(),
            AssignmentRequest {
                operation_id: operation,
                task_id: "opaque-task".into(),
                command: AssignmentCommand {
                    expected_revision: 1,
                    assigned_team_id: Some("doing-team".into()),
                    assignee_id: Some("employee".into())
                }
            }
        )
        .unwrap(),
        original
    );
    assert_eq!(
        store
            .read(|db| db
                .query_row("SELECT ordinal FROM selected_journal_cursor", [], |r| r
                    .get::<_, i64>(0))
                .map_err(storage))
            .unwrap(),
        before
    );
    assert_eq!(
        assign(
            &store,
            authority(),
            request(2, Some("doing-team"), Some("employee"))
        )
        .unwrap(),
        rejection("invalid_assignment")
    );
    assert!(
        super::super::directory::admit(&store, authority(), Some(1), directory_source(true))
            .is_err()
    );
    let retained: String = store
        .read(|db| {
            db.query_row(
                "SELECT data FROM selected_directory WHERE generation=2",
                [],
                |r| r.get(0),
            )
            .map_err(storage)
        })
        .unwrap();
    assert!(retained.contains("123456789012345678901234567890.12345678901234567890E+012"));
}

#[test]
fn racing_same_revision_accepts_one_and_durably_rejects_the_other() {
    let (_root, store, _) = fixture();
    let store = Arc::new(store);
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let mut joins = Vec::new();
    for pair in [(Some("doing-team"), None), (None, Some("employee"))] {
        let store = store.clone();
        let barrier = barrier.clone();
        joins.push(std::thread::spawn(move || {
            barrier.wait();
            assign(&store, authority(), request(1, pair.0, pair.1)).unwrap()
        }));
    }
    barrier.wait();
    let outcomes = joins
        .into_iter()
        .map(|j| j.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        outcomes
            .iter()
            .filter(|r| matches!(r, AssignmentOutcome::Completed { .. }))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|r| matches!(r, AssignmentOutcome::Rejected {code,current:Some(state)} if code=="conflict" && state.revision==2))
            .count(),
        1
    );
    let events: i64 = store
        .read(|db| {
            db.query_row("SELECT count(*) FROM selected_events", [], |r| r.get(0))
                .map_err(storage)
        })
        .unwrap();
    assert_eq!(events, 2); // explicit baseline plus the single accepted assignment
}

#[test]
fn full_i64_revisions_are_exact_and_overflow_yields_only_a_receipt() {
    let (_root, store, config) = fixture_at_revision(i64::MAX - 1);
    assert!(matches!(
        assign(&store, authority(), request(i64::MAX - 1, None, None)).unwrap(),
        AssignmentOutcome::Completed {
            revision: i64::MAX,
            ..
        }
    ));
    assert_eq!(
        assign(&store, authority(), request(i64::MAX, None, None)).unwrap(),
        rejection("revision_exhausted")
    );
    let text = std::fs::read_to_string(
        config
            .public_root
            .join("teams/accountable/work/tasks/opaque-task.md"),
    )
    .unwrap();
    assert!(text.contains("revision: 9223372036854775807"));
}

#[test]
fn protected_binding_cannot_be_replaced_by_profile_data_or_new_callback_identity() {
    let (_root, store, _) = fixture();
    let foreign: Arc<dyn OwnerAuthority> = Arc::new(Authority {
        account: "other-account".into(),
        valid: true,
    });
    assert!(super::super::directory::bind_owner(&store, foreign).is_err());
    let mut source = String::from_utf8(directory_source(true)).unwrap();
    source = source.replace(
        "\"projects\":{}",
        "\"projects\":{},\"is_owner\":true,\"account_id\":\"other-account\"",
    );
    super::super::directory::admit(&store, authority(), Some(1), source.into_bytes()).unwrap();
    let foreign: Arc<dyn OwnerAuthority> = Arc::new(Authority {
        account: "other-account".into(),
        valid: true,
    });
    assert!(assign(&store, foreign, request(1, None, None)).is_err());
}

#[test]
fn original_receipt_survives_reopen_and_changed_targets() {
    let (root, store, config) = fixture();
    let r = request(1, Some("doing-team"), Some("employee"));
    let operation = r.operation_id.clone();
    let original = assign(&store, authority(), r).unwrap();
    super::super::directory::admit(&store, authority(), Some(1), directory_source(false)).unwrap();
    drop(store);
    let store = Store::open(
        &root.path().join("office.db"),
        &Key::from_bytes([31; 32]),
        Opening::Existing,
        &OFFICE_SCHEMA,
    )
    .unwrap();
    store.install_acceptance(config).unwrap();
    assert_eq!(
        lookup(&store, authority(), operation).unwrap(),
        Some(original)
    );
}

#[test]
fn malformed_or_relocating_directory_input_preserves_generation_and_source() {
    let (_root, store, _) = fixture();
    let valid = String::from_utf8(directory_source(true)).unwrap();
    let sources = [
        valid.replace(
            "\"organization_id\":\"org\"",
            "\"organization_id\":\"other-org\"",
        ),
        valid.replace(
            "\"parent_team_id\":null",
            "\"parent_team_id\":\"accountable\"",
        ),
        valid.replace(
            "\"home_team_id\":\"accountable\"",
            "\"home_team_id\":\"missing\"",
        ),
        valid.replacen(
            "\"schema_version\":1",
            "\"schema_version\":1,\"schema_version\":1",
            1,
        ),
    ];
    for source in sources {
        assert!(
            super::super::directory::admit(&store, authority(), Some(1), source.into_bytes())
                .is_err()
        );
    }
    let generation: i64 = store
        .read(|db| {
            db.query_row("SELECT generation FROM selected_directory_head", [], |r| {
                r.get(0)
            })
            .map_err(storage)
        })
        .unwrap();
    assert_eq!(generation, 1);
    let retained: String = store
        .read(|db| {
            db.query_row(
                "SELECT data FROM selected_directory WHERE generation=1",
                [],
                |r| r.get(0),
            )
            .map_err(storage)
        })
        .unwrap();
    assert_eq!(retained, valid);
}

#[test]
fn selected_and_legacy_receipts_cannot_reuse_each_others_operation_ids() {
    let (_root, store, _) = fixture();
    let r = request(1, None, None);
    let old = r.operation_id.clone();
    store.selected_write(move|db| {
        db.execute("INSERT INTO work_change_results(change_id,params_hash,answer,expires_at_ms) VALUES(?1,zeroblob(32),?2,9000)",params![old,r#"{"err":{"code":"not_found","message":"legacy result"}}"#]).map_err(storage)?;Ok(())
    }).unwrap();
    assert!(assign(&store, authority(), r).is_err());
    let r = request(1, None, None);
    let new = r.operation_id.clone();
    assign(&store, authority(), r).unwrap();
    assert!(store.selected_write(move|db| {
        db.execute("INSERT OR REPLACE INTO work_change_results(change_id,params_hash,answer,expires_at_ms) VALUES(?1,zeroblob(32),'{}',9000)",[new]).map_err(storage)?;Ok(())
    }).is_err());
}

#[test]
fn receipt_expires_at_operation_time_plus_thirty_days_across_reopen() {
    const DAY: u64 = 86_400_000;
    let (root, store, config) = fixture();
    let time = crate::time::now_ms() - 28 * DAY;
    let mut r = request(1, None, None);
    r.operation_id = crate::ulid::from_ms(time, 19);
    let operation = r.operation_id.clone();
    let outcome = assign(&store, authority(), r).unwrap();
    let expiry = time + 30 * DAY;
    let stored: i64 = store
        .selected_read(move |db, _| {
            db.query_row(
                "SELECT expires_at_ms FROM selected_receipts WHERE operation_id=?1",
                [operation],
                |r| r.get(0),
            )
            .map_err(storage)
        })
        .unwrap();
    assert_eq!(stored as u64, expiry);
    store
        .selected_write(move |db| {
            db.execute(
                "UPDATE work_clock SET high_water_ms=?1",
                [(expiry - 1) as i64],
            )
            .map_err(storage)?;
            Ok(())
        })
        .unwrap();
    let operation = crate::ulid::from_ms(time, 19);
    assert_eq!(
        lookup(&store, authority(), operation.clone()).unwrap(),
        Some(outcome.clone())
    );
    drop(store);
    let store = Store::open(
        &root.path().join("office.db"),
        &Key::from_bytes([31; 32]),
        Opening::Existing,
        &OFFICE_SCHEMA,
    )
    .unwrap();
    store.install_acceptance(config).unwrap();
    assert_eq!(
        lookup(&store, authority(), operation.clone()).unwrap(),
        Some(outcome)
    );
    store
        .selected_write(move |db| {
            db.execute("UPDATE work_clock SET high_water_ms=?1", [expiry as i64])
                .map_err(storage)?;
            Ok(())
        })
        .unwrap();
    assert!(matches!(
        lookup(&store, authority(), operation),
        Err(OfficeError::Invalid("operation expired"))
    ));
}

#[test]
fn archived_team_is_retained_but_cannot_be_a_new_target() {
    let (_root, store, _) = fixture();
    super::super::directory::admit(&store, authority(), Some(1), directory_source(false)).unwrap();
    let rejected = assign(&store, authority(), request(1, Some("doing-team"), None)).unwrap();
    assert!(matches!(rejected, AssignmentOutcome::Rejected { .. }));
    let invalid = String::from_utf8(directory_source(false))
        .unwrap()
        .replace("\"archived\"", "\"inactive\"");
    assert!(
        super::super::directory::admit(&store, authority(), Some(2), invalid.into_bytes()).is_err()
    );
}

#[test]
fn selected_read_pages_obey_encoded_byte_budget_without_truncating_large_status_fields() {
    use org_proto::selected_tasks::read::{Query, Queue, Result as Read};
    let (_dir, store, _config) = fixture();
    let status = "\\\"".repeat(15000);
    let retained = status.clone();
    store.selected_write(move|db|{
        for n in 0..20 {
            let id=format!("task-{n:02}");
            let source=SOURCE.replace("id: opaque-task",&format!("id: {id}")).replace("status: todo",&format!("status: {}",serde_json::to_string(&status).unwrap()));
            let path=format!("teams/accountable/work/tasks/{id}.md");
            db.execute("INSERT INTO selected_tasks VALUES('org',?1,1,'accountable',NULL,NULL,NULL,?2,?3,?4)",params![id,path,source.as_bytes(),codec::sha256(source.as_bytes())]).map_err(storage)?;
            db.execute("INSERT INTO selected_adoptions VALUES('org',?1,1,1,?2,'{}')",params![id,"b".repeat(64)]).map_err(storage)?;
            put_artifact(db,"public",&path,source.as_bytes(),false)?;
        }
        Ok(())
    }).unwrap();
    let mut after = None;
    let mut ids = std::collections::BTreeSet::new();
    let mut pages = 0;
    loop {
        let answer = super::super::read::read(
            &store,
            authority(),
            Query::List {
                after,
                queue: Queue::All,
                limit: 50,
            },
        )
        .unwrap();
        let encoded = serde_json::to_vec(&answer).unwrap();
        assert!(encoded.len() < org_proto::MAX_FRAME_BYTES);
        let Read::List { items, next } = answer else {
            panic!("list")
        };
        assert!(!items.is_empty());
        for card in items {
            if card.task_id != "opaque-task" {
                assert_eq!(card.status, retained);
            }
            assert!(ids.insert(card.task_id));
        }
        pages += 1;
        if next.is_none() {
            break;
        }
        after = next;
    }
    assert_eq!(ids.len(), 21);
    assert!(pages > 1);
}

fn staged_adoption(
    store: &Store,
    root: &std::path::Path,
    source: Vec<u8>,
    origin: &str,
    events: Vec<(i64, Vec<u8>)>,
) -> org_proto::selected_tasks::adoption::Action {
    use org_proto::selected_tasks::{adoption::Action, Revision};
    let stage = crate::ulid::generate().unwrap();
    let counter =
        b"scope_kind: team\nscope_id: accountable\nentity_kind: task\nlast_number: 43\n".to_vec();
    let allocation=b"scope_kind: team\nscope_id: accountable\nentity_kind: task\nnumber: 43\nentity_id: imported-task\nallocated_at: 2026-01-01T00:00:00Z\n".to_vec();
    let reference =
        |bytes: &[u8]| serde_json::json!({"sha256":codec::sha256(bytes),"byte_length":bytes.len()});
    let history_origin = if origin == "baseline" {
        serde_json::json!({"kind":"baseline","revision":1})
    } else {
        serde_json::json!({"kind":origin})
    };
    let raw=serde_json::to_vec(&serde_json::json!({"version":1,"resource_profile":codec::RESOURCE_PROFILE,"task_id":"imported-task","canonical_path":"teams/accountable/work/tasks/imported-task.md","public_snapshot":reference(&source),"history_origin":history_origin,"events":events.iter().map(|(r,b)|serde_json::json!({"revision":r,"blob":reference(b)})).collect::<Vec<_>>(),"public_blobs":[],"number_counter":reference(&counter),"number_allocation":reference(&allocation)})).unwrap();
    let digest = codec::sha256(&raw);
    for bytes in std::iter::once(&raw)
        .chain([&source, &counter, &allocation])
        .chain(events.iter().map(|(_, b)| b))
    {
        for offset in (0..bytes.len().max(1)).step_by(codec::MAX_CHUNK_BYTES) {
            let mut p = staged_request(&stage, bytes, Some(offset));
            p.manifest_sha256 = digest.clone();
            assert!(
                super::super::staging::run(store, root.to_path_buf(), authority(), p)
                    .unwrap()
                    .verified
                    || offset + codec::MAX_CHUNK_BYTES < bytes.len()
            );
        }
    }
    Action::Commit {
        operation_id: org_proto::gateway::OperationId::parse(crate::ulid::generate().unwrap())
            .unwrap(),
        stage_id: org_proto::gateway::OperationId::parse(stage).unwrap(),
        manifest_sha256: digest,
        expected_directory_generation: Revision::new(1).unwrap(),
    }
}
fn imported_source() -> Vec<u8> {
    SOURCE
        .replace("opaque-task", "imported-task")
        .replace("number: 42", "number: 43")
        .into_bytes()
}

#[test]
fn event_content_roundtrips_full_baseline_unknown_and_new_assignment_bytes() {
    use org_proto::selected_tasks::read::{Query, Result as Read};
    let (dir, store, config) = fixture();
    let mut original = String::from_utf8(imported_source()).unwrap();
    original.push_str(&"large public body café.\n".repeat(15000));
    let baseline_change = codec::parse(&serde_json::to_vec(&serde_json::json!({
        "type":"history_baseline",
        "known_facts":{"id":"imported-task","team_id":"accountable","project_id":null,"revision":1,"assigned_team_id":null,"assignee_id":null},
        "public_snapshot":{"encoding":"utf-8-markdown","text":original}
    })).unwrap()).unwrap();
    let encode = |revision, change| {
        history::encode(
            EventFields {
                organization_id: "org".into(),
                task_id: "imported-task".into(),
                revision,
                occurred_at: None,
                actor_kind: "unknown".into(),
                actor_id: None,
                operation_id: None,
                change,
                source_metadata: BTreeMap::new(),
                extensions: BTreeMap::new(),
            },
            Admission::Import,
        )
        .unwrap()
    };
    let baseline = encode(1, baseline_change);
    let unknown = encode(2, codec::parse(br#"{"type":"future_change","extension":123456789012345678901234567890.123456789E+012}"#).unwrap());
    let source = original.replace("revision: 1", "revision: 2").into_bytes();
    let action = staged_adoption(
        &store,
        &dir.path().join("staging"),
        source,
        "baseline",
        vec![(1, baseline.clone()), (2, unknown.clone())],
    );
    assert!(matches!(
        super::super::adoption::run(&store, dir.path().join("staging"), authority(), action)
            .unwrap(),
        org_proto::selected_tasks::adoption::Outcome::Completed { revision: 2, .. }
    ));
    let mut change = request(2, Some("doing-team"), Some("employee"));
    change.task_id = "imported-task".into();
    assert!(matches!(
        assign(&store, authority(), change).unwrap(),
        AssignmentOutcome::Completed { revision: 3, .. }
    ));
    let fresh = std::fs::read(
        config
            .public_root
            .join("work-history/tasks/imported-task/3.json"),
    )
    .unwrap();
    let Read::History { events, .. } = super::super::read::read(
        &store,
        authority(),
        Query::History {
            task_id: "imported-task".into(),
            after: None,
            limit: 50,
        },
    )
    .unwrap() else {
        panic!("history")
    };
    assert_eq!(events.len(), 3);
    for (reference, expected) in events.into_iter().zip([baseline, unknown, fresh]) {
        assert_eq!(reference.sha256, codec::sha256(&expected));
        let verified = codec::parse(&expected).unwrap();
        assert_ne!(
            Some(reference.sha256.as_str()),
            verified.get("payload_sha256").and_then(codec::Json::as_str)
        );
        let mut offset = 0;
        let mut actual = Vec::new();
        let mut chunks = 0;
        loop {
            let response = super::super::read::read(
                &store,
                authority(),
                Query::Chunk {
                    task_id: "imported-task".into(),
                    reference: reference.clone(),
                    offset,
                    limit: codec::MAX_CHUNK_BYTES as u32,
                },
            )
            .unwrap();
            assert!(serde_json::to_vec(&response).unwrap().len() < org_proto::MAX_FRAME_BYTES);
            let Read::Chunk { hex, next, .. } = response else {
                panic!("chunk")
            };
            for pair in hex.as_bytes().as_chunks::<2>().0 {
                actual.push(u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap());
            }
            chunks += 1;
            let Some(next) = next else { break };
            offset = next;
        }
        assert_eq!(actual, expected);
        if reference.revision == 1 {
            assert!(chunks > 1);
        }
        let mut old = reference.clone();
        old.sha256 = verified
            .get("payload_sha256")
            .unwrap()
            .as_str()
            .unwrap()
            .into();
        assert!(matches!(
            super::super::read::read(
                &store,
                authority(),
                Query::Chunk {
                    task_id: "imported-task".into(),
                    reference: old,
                    offset: 0,
                    limit: 1
                }
            ),
            Err(OfficeError::NotFound)
        ));
    }
}

#[test]
fn owner_staged_adoption_is_durable_exact_retryable_and_then_assignable_after_reopen() {
    use org_proto::selected_tasks::adoption::{Action, Outcome};
    let (dir, store, config) = fixture();
    let root = dir.path().join("adoption-staging");
    let source = imported_source();
    let action = staged_adoption(&store, &root, source.clone(), "snapshot_only", vec![]);
    let operation = match &action {
        Action::Commit { operation_id, .. } => operation_id.clone(),
        _ => unreachable!(),
    };
    let accepted =
        super::super::adoption::run(&store, root.clone(), authority(), action.clone()).unwrap();
    assert_eq!(
        accepted,
        Outcome::Completed {
            task_id: "imported-task".into(),
            revision: 1,
            history_coverage: "snapshot_baseline".into()
        }
    );
    assert_eq!(
        std::fs::read(
            config
                .public_root
                .join("teams/accountable/work/tasks/imported-task.md")
        )
        .unwrap(),
        source
    );
    let event = std::fs::read(
        config
            .public_root
            .join("work-history/tasks/imported-task/1.json"),
    )
    .unwrap();
    let event = history::verify(
        &event,
        history::ExpectedIdentity {
            organization_id: "org",
            task_id: "imported-task",
            filename: "1.json",
        },
        Admission::Import,
    )
    .unwrap();
    assert_eq!(
        event
            .value()
            .get("change")
            .unwrap()
            .get("type")
            .unwrap()
            .as_str(),
        Some("history_baseline")
    );
    assert_eq!(
        event.value().get("operation_id").unwrap().as_str(),
        Some(operation.as_str())
    );
    let mut change = request(1, Some("doing-team"), Some("employee"));
    change.task_id = "imported-task".into();
    assert!(matches!(
        assign(&store, authority(), change).unwrap(),
        AssignmentOutcome::Completed { revision: 2, .. }
    ));
    super::super::directory::admit(&store, authority(), Some(1), directory_source(false)).unwrap();
    assert_eq!(
        super::super::adoption::run(&store, root.clone(), authority(), action.clone()).unwrap(),
        accepted
    );
    drop(store);
    let store = Store::open(
        &dir.path().join("office.db"),
        &Key::from_bytes([31; 32]),
        Opening::Existing,
        &OFFICE_SCHEMA,
    )
    .unwrap();
    store.install_acceptance(config.clone()).unwrap();
    assert_eq!(
        super::super::adoption::run(
            &store,
            root.clone(),
            authority(),
            Action::Lookup {
                operation_id: operation
            }
        )
        .unwrap(),
        accepted
    );
    assert_eq!(
        super::super::adoption::run(&store, root.clone(), authority(), action.clone()).unwrap(),
        accepted
    );
    let denied = Arc::new(Authority {
        account: "other".into(),
        valid: true,
    });
    assert!(super::super::adoption::run(&store, root, denied, action).is_err());
    let current = std::fs::read_to_string(
        config
            .public_root
            .join("teams/accountable/work/tasks/imported-task.md"),
    )
    .unwrap();
    assert!(current.contains("lexical: 1.234567890123456789E+012"));
    assert!(current.ends_with("Body remains café.\n"));
}

#[test]
fn adoption_rejects_known_history_baseline_concealment_without_partial_publication() {
    use org_proto::selected_tasks::adoption::Outcome;
    let (dir, store, config) = fixture();
    let root = dir.path().join("adoption-staging");
    let source = imported_source();
    let action = staged_adoption(
        &store,
        &root,
        source,
        "snapshot_only",
        vec![(1, b"unsupported original bytes".to_vec())],
    );
    let result =
        super::super::adoption::run(&store, root.clone(), authority(), action.clone()).unwrap();
    assert_eq!(
        result,
        Outcome::Staged {
            reason_code: "unsupported_or_inconsistent_source".into()
        }
    );
    assert!(!config
        .public_root
        .join("teams/accountable/work/tasks/imported-task.md")
        .exists());
    assert!(root.is_dir());
    assert_eq!(
        super::super::adoption::run(&store, root, authority(), action).unwrap(),
        result
    );
}

#[test]
fn uuid_office_assignment_retains_exact_origin_identity_on_the_existing_allocator() {
    let office = "019a0000-0000-7000-8000-000000000001";
    let (_root, store, _) = fixture_with_office(1, office);
    let outcome = assign(&store, authority(), request(1, None, None));
    assert!(outcome.is_ok(), "{outcome:?}");
    store
        .selected_read(move |db, _| {
            let origin: String = db
                .query_row(
                    "SELECT origin_office_id FROM work_origin_log WHERE origin_seq=1",
                    [],
                    |r| r.get(0),
                )
                .map_err(storage)?;
            assert_eq!(origin, office);
            Ok(())
        })
        .unwrap();
}
