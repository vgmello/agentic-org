use super::*;
use crate::selected_tasks::{adoption, staging};
use org_proto::selected_tasks::adoption::{Action, Outcome};

fn imported_event(revision: i64, tag: &str, metadata: Json) -> Vec<u8> {
    let source = String::from_utf8(imported_source()).unwrap();
    let change = if matches!(tag, "created" | "history_baseline") {
        codec::parse(&serde_json::to_vec(&serde_json::json!({
            "type":tag,"known_facts":{"id":"imported-task","team_id":"accountable","project_id":null,"revision":1,"assigned_team_id":null,"assignee_id":null},
            "public_snapshot":{"encoding":"utf-8-markdown","text":source}
        })).unwrap()).unwrap()
    } else {
        codec::parse(br#"{"type":"future_public_change","opaque":123456789012345678901234567890.123456789E+012}"#).unwrap()
    };
    let Json::Object(source_metadata) = metadata else {
        panic!("metadata")
    };
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
            source_metadata,
            extensions: BTreeMap::new(),
        },
        Admission::Import,
    )
    .unwrap()
}

#[test]
fn canonical_adoption_keeps_private_sources_staged_without_public_mutation() {
    for tag in ["created", "history_baseline", "future_public_change"] {
        for metadata in [
            br#"{"classification":"private"}"#.as_slice(),
            br#"{"nested":[{"private_notes":{"body":"must remain staged"}}]}"#.as_slice(),
            br#"{"nested":{"classification":"unclassified"}}"#.as_slice(),
        ] {
            let (dir, store, config) = fixture();
            let root = dir.path().join("adoption-staging");
            let later = tag == "future_public_change";
            let revision = if later { 2 } else { 1 };
            let event = imported_event(revision, tag, codec::parse(metadata).unwrap());
            let mut events = Vec::new();
            if later {
                events.push((
                    1,
                    imported_event(1, "history_baseline", codec::parse(b"{}").unwrap()),
                ));
            }
            events.push((revision, event.clone()));
            let source = String::from_utf8(imported_source())
                .unwrap()
                .replace("revision: 1", &format!("revision: {revision}"))
                .into_bytes();
            let action = staged_adoption(
                &store,
                &root,
                source,
                if tag == "created" {
                    "creation"
                } else {
                    "baseline"
                },
                events,
            );
            let Action::Commit {
                stage_id,
                manifest_sha256,
                ..
            } = action.clone()
            else {
                unreachable!()
            };
            let public_before = store.selected_read(|db,_| {
                assert!(!db.query_row("SELECT EXISTS(SELECT 1 FROM work_ids WHERE id='imported-task') OR EXISTS(SELECT 1 FROM work_items WHERE id='imported-task')",[],|r|r.get::<_,bool>(0)).unwrap());
                db.query_row("SELECT count(*) FROM selected_artifacts WHERE namespace='public'",[],|r|r.get::<_,i64>(0)).map_err(storage)
            }).unwrap();
            let outcome = adoption::run(&store, root.clone(), authority(), action.clone()).unwrap();
            assert_eq!(
                outcome,
                Outcome::Staged {
                    reason_code: "unsupported_or_inconsistent_source".into()
                },
                "tag={tag} metadata={}",
                std::str::from_utf8(metadata).unwrap()
            );
            assert_eq!(
                adoption::run(&store, root.clone(), authority(), action.clone()).unwrap(),
                outcome
            );
            store
                .selected_read(move |db, id| {
                    for table in ["selected_tasks", "selected_events", "selected_adoptions"] {
                        assert_eq!(
                            db.query_row(
                                &format!(
                                    "SELECT count(*) FROM {table} WHERE task_id='imported-task'"
                                ),
                                [],
                                |r| r.get::<_, i64>(0)
                            )
                            .unwrap(),
                            0
                        );
                    }
                    assert_eq!(
                        db.query_row(
                            "SELECT count(*) FROM selected_artifacts WHERE namespace='public'",
                            [],
                            |r| r.get::<_, i64>(0)
                        )
                        .unwrap(),
                        public_before
                    );
                    let retained = staging::source(
                        &root,
                        id,
                        &OwnerAccount {
                            account_id: "account".into(),
                            member_id: "owner-member".into(),
                        },
                        stage_id.as_str(),
                        &manifest_sha256,
                        &org_proto::selected_tasks::staging::Blob {
                            sha256: codec::sha256(&event),
                            byte_length: event.len() as u64,
                        },
                        codec::MAX_JSON_BYTES,
                    )?;
                    assert_eq!(retained, event);
                    Ok(())
                })
                .unwrap();
            assert!(!config
                .public_root
                .join("teams/accountable/work/tasks/imported-task.md")
                .exists());
            assert!(!config
                .public_root
                .join("work-history/tasks/imported-task")
                .exists());
        }
    }
}

#[test]
fn canonical_adoption_retains_public_extensions_and_exact_original_event_bytes() {
    for tag in ["created", "history_baseline", "future_public_change"] {
        let (dir, store, config) = fixture();
        let root = dir.path().join("adoption-staging");
        let revision = if tag == "future_public_change" { 2 } else { 1 };
        let event=imported_event(revision,tag,codec::parse(br#"{"classification":"public","extension":{"future":[null,123456789012345678901234567890.123456789E+012]}}"#).unwrap());
        let source = String::from_utf8(imported_source())
            .unwrap()
            .replace("revision: 1", &format!("revision: {revision}"))
            .into_bytes();
        let mut events = Vec::new();
        if revision == 2 {
            events.push((
                1,
                imported_event(1, "history_baseline", codec::parse(b"{}").unwrap()),
            ));
        }
        events.push((revision, event.clone()));
        let action = staged_adoption(
            &store,
            &root,
            source.clone(),
            if tag == "created" {
                "creation"
            } else {
                "baseline"
            },
            events,
        );
        assert!(matches!(
            adoption::run(&store, root, authority(), action).unwrap(),
            Outcome::Completed { revision: r, .. } if r == revision
        ));
        assert_eq!(
            std::fs::read(
                config
                    .public_root
                    .join(format!("work-history/tasks/imported-task/{revision}.json"))
            )
            .unwrap(),
            event
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
    }
}
