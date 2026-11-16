//! Real staging/acceptance coverage for Markdown provenance, including older snapshots.
use super::*;
use crate::selected_tasks::{adoption, staging};
use org_proto::selected_tasks::{adoption::Action, adoption::Outcome, Revision};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug)]
enum Form {
    Current,
    HistoricalInline,
    HistoricalReference,
}

struct Staged {
    action: Action,
    sources: Vec<Vec<u8>>,
    current: Vec<u8>,
    events: Vec<(i64, Vec<u8>)>,
    blobs: Vec<Vec<u8>>,
}

fn snapshot(revision: i64, provenance: &str) -> Vec<u8> {
    String::from_utf8(imported_source())
        .unwrap()
        .replace("revision: 1", &format!("revision: {revision}"))
        .replace("extension:\n", &format!("{provenance}extension:\n"))
        .into_bytes()
}

fn event(revision: i64, change: Json) -> Vec<u8> {
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
}

fn reference(bytes: &[u8]) -> serde_json::Value {
    serde_json::json!({"sha256":codec::sha256(bytes),"byte_length":bytes.len()})
}

fn stage(store: &Store, root: &Path, form: Form, provenance: &str, opaque: bool) -> Staged {
    let historical = !matches!(form, Form::Current);
    let current = snapshot(
        if historical { 2 } else { 1 },
        if historical { "" } else { provenance },
    );
    let mut blobs = Vec::new();
    let events = if historical {
        let old = snapshot(1, provenance);
        let public_snapshot = if matches!(form, Form::HistoricalReference) {
            let mut r = reference(&old);
            r["encoding"] = "utf-8-markdown".into();
            r["resource_profile"] = codec::RESOURCE_PROFILE.into();
            blobs.push(old);
            r
        } else {
            serde_json::json!({"encoding":"utf-8-markdown","text":String::from_utf8(old).unwrap()})
        };
        let change = codec::parse(&serde_json::to_vec(&serde_json::json!({
            "type":"history_baseline",
            "known_facts":{"id":"imported-task","team_id":"accountable","project_id":null,"revision":1,"assigned_team_id":null,"assignee_id":null},
            "public_snapshot":public_snapshot
        })).unwrap()).unwrap();
        vec![
            (1, event(1, change)),
            (2, event(2, codec::parse(br#"{"type":"future_public_change","extension":123456789012345678901234567890.123456789E+012}"#).unwrap())),
        ]
    } else {
        Vec::new()
    };
    // A public opaque attachment is not a Markdown snapshot merely because it
    // contains words that are private provenance when used as structured fields.
    if opaque {
        blobs.push(b"\xff\0classification: private\nprivate_notes: opaque public example".to_vec());
    }
    let counter =
        b"scope_kind: team\nscope_id: accountable\nentity_kind: task\nlast_number: 43\n".to_vec();
    let allocation = b"scope_kind: team\nscope_id: accountable\nentity_kind: task\nnumber: 43\nentity_id: imported-task\nallocated_at: 2026-01-01T00:00:00Z\n".to_vec();
    let origin = if historical {
        serde_json::json!({"kind":"baseline","revision":1})
    } else {
        serde_json::json!({"kind":"snapshot_only"})
    };
    let manifest = serde_json::to_vec(&serde_json::json!({
        "version":1,"resource_profile":codec::RESOURCE_PROFILE,"task_id":"imported-task",
        "canonical_path":"teams/accountable/work/tasks/imported-task.md",
        "public_snapshot":reference(&current),"history_origin":origin,
        "events":events.iter().map(|(revision,bytes)|serde_json::json!({"revision":revision,"blob":reference(bytes)})).collect::<Vec<_>>(),
        "public_blobs":blobs.iter().map(|bytes|reference(bytes)).collect::<Vec<_>>(),
        "number_counter":reference(&counter),"number_allocation":reference(&allocation)
    })).unwrap();
    let manifest_sha256 = codec::sha256(&manifest);
    let stage_id = crate::ulid::generate().unwrap();
    let mut sources = vec![manifest, current.clone(), counter, allocation];
    sources.extend(events.iter().map(|(_, bytes)| bytes.clone()));
    sources.extend(blobs.iter().cloned());
    for bytes in &sources {
        for offset in (0..bytes.len().max(1)).step_by(codec::MAX_CHUNK_BYTES) {
            let mut request = staged_request(&stage_id, bytes, Some(offset));
            request.manifest_sha256 = manifest_sha256.clone();
            let result = staging::run(store, root.to_path_buf(), authority(), request).unwrap();
            assert!(result.verified || offset + codec::MAX_CHUNK_BYTES < bytes.len());
        }
    }
    Staged {
        action: Action::Commit {
            operation_id: org_proto::gateway::OperationId::parse(crate::ulid::generate().unwrap())
                .unwrap(),
            stage_id: org_proto::gateway::OperationId::parse(stage_id).unwrap(),
            manifest_sha256,
            expected_directory_generation: Revision::new(1).unwrap(),
        },
        sources,
        current,
        events,
        blobs,
    }
}

fn rows(db: &Connection, sql: &str) -> Vec<Vec<rusqlite::types::Value>> {
    let mut query = db.prepare(sql).unwrap();
    let columns = query.column_count();
    query
        .query_map([], |row| (0..columns).map(|i| row.get(i)).collect())
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn domain(store: &Store) -> BTreeMap<String, Vec<Vec<rusqlite::types::Value>>> {
    store
        .selected_read(|db, _| {
            let mut state = BTreeMap::new();
            for table in [
                "selected_tasks",
                "selected_events",
                "selected_adoptions",
                "selected_directory",
                "selected_directory_head",
                "selected_owner_authority",
                "work_items",
                "work_events",
                "work_ids",
                "work_feed",
                "work_origin_counter",
                "work_origin_log",
                "work_change_results",
                "meta",
            ] {
                state.insert(
                    table.into(),
                    rows(db, &format!("SELECT * FROM {table} ORDER BY 1,2")),
                );
            }
            state.insert(
                "public_artifacts".into(),
                rows(
                    db,
                    "SELECT * FROM selected_artifacts WHERE namespace='public' ORDER BY path",
                ),
            );
            state.insert(
                "origin_cursor".into(),
                rows(
                    db,
                    "SELECT origin_incarnation,origin_high_water FROM selected_journal_cursor",
                ),
            );
            Ok(state)
        })
        .unwrap()
}

fn files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, directory: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                visit(root, &path, out);
            } else {
                out.insert(
                    path.strip_prefix(root).unwrap().into(),
                    std::fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    visit(root, root, &mut out);
    out
}

fn progress(store: &Store) -> (i64, i64) {
    store
        .selected_read(|db, _| {
            Ok((
                db.query_row("SELECT ordinal FROM selected_journal_cursor", [], |r| {
                    r.get(0)
                })
                .map_err(storage)?,
                db.query_row("SELECT count(*) FROM selected_receipts", [], |r| r.get(0))
                    .map_err(storage)?,
            ))
        })
        .unwrap()
}

fn retained(store: &Store, root: &Path, staged: &Staged) {
    let Action::Commit {
        stage_id,
        manifest_sha256,
        ..
    } = staged.action.clone()
    else {
        unreachable!()
    };
    let sources = staged.sources.clone();
    let root = root.to_path_buf();
    store
        .selected_read(move |db, id| {
            let owner = super::super::owner(db, id, &*authority())?;
            for bytes in sources {
                let actual = staging::source(
                    &root,
                    id,
                    &owner,
                    stage_id.as_str(),
                    &manifest_sha256,
                    &org_proto::selected_tasks::staging::Blob {
                        sha256: codec::sha256(&bytes),
                        byte_length: bytes.len() as u64,
                    },
                    codec::MAX_BLOB_BYTES as usize,
                )?;
                assert_eq!(actual, bytes);
            }
            Ok(())
        })
        .unwrap();
}

fn receipt_only(config: &AcceptanceConfig, ordinal: i64, action: &Action) {
    let bytes = std::fs::read(config.protected_root.join(format!(
        "offices/office/journals/work-acceptance/journal/transactions/{ordinal}.json"
    )))
    .unwrap();
    let envelope: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(envelope["origin_entries"].as_array().unwrap().is_empty());
    assert_eq!(envelope["origin_before"], envelope["origin_after"]);
    let mut receipt = false;
    for mutation in envelope["mutations"].as_array().unwrap() {
        match mutation["table"].as_str().unwrap() {
            "work_clock" => {}
            "selected_receipts" => receipt = true,
            "selected_artifacts" => {
                let namespace = mutation["columns"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .position(|c| c == "namespace")
                    .unwrap();
                assert_eq!(mutation["after"][namespace]["value"], "protected");
            }
            table => panic!("unexpected refusal mutation: {table}"),
        }
    }
    assert!(receipt);
    let Action::Commit { operation_id, .. } = action else {
        unreachable!()
    };
    let receipt: serde_json::Value = serde_json::from_slice(
        &std::fs::read(config.protected_root.join(format!(
            "offices/office/receipts/{}.json",
            operation_id.as_str()
        )))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(receipt["result"]["status"], "staged");
    assert_eq!(
        receipt["result"]["reason_code"],
        "unsupported_or_inconsistent_source"
    );
}

fn refuses(form: Form, provenance: &str) {
    let (dir, store, config) = fixture();
    let root = dir.path().join("snapshot-staging");
    let staged = stage(&store, &root, form, provenance, false);
    let before = domain(&store);
    let public_before = files(&config.public_root);
    let (ordinal, receipts) = progress(&store);
    let expected = Outcome::Staged {
        reason_code: "unsupported_or_inconsistent_source".into(),
    };
    assert_eq!(
        adoption::run(&store, root.clone(), authority(), staged.action.clone()).unwrap(),
        expected,
        "{form:?}: {provenance}"
    );
    assert_eq!(domain(&store), before);
    assert_eq!(files(&config.public_root), public_before);
    assert_eq!(progress(&store), (ordinal + 1, receipts + 1));
    receipt_only(&config, ordinal + 1, &staged.action);
    retained(&store, &root, &staged);
    let protected = files(&config.protected_root);
    assert_eq!(
        adoption::run(&store, root.clone(), authority(), staged.action.clone()).unwrap(),
        expected
    );
    assert_eq!(progress(&store), (ordinal + 1, receipts + 1));
    assert_eq!(files(&config.protected_root), protected);
    drop(store);
    let store = Store::open(
        &dir.path().join("office.db"),
        &Key::from_bytes([31; 32]),
        Opening::Existing,
        &OFFICE_SCHEMA,
    )
    .unwrap();
    store.install_acceptance(config.clone()).unwrap();
    assert_eq!(domain(&store), before);
    assert_eq!(files(&config.public_root), public_before);
    retained(&store, &root, &staged);
    assert_eq!(
        adoption::run(&store, root.clone(), authority(), staged.action.clone()).unwrap(),
        expected
    );
    assert_eq!(progress(&store), (ordinal + 1, receipts + 1));
    assert_eq!(files(&config.protected_root), protected);
    let valid = staged_adoption(
        &store,
        &root,
        imported_source(),
        "snapshot_only",
        Vec::new(),
    );
    assert!(matches!(
        adoption::run(&store, root.clone(), authority(), valid).unwrap(),
        Outcome::Completed { revision: 1, .. }
    ));
    let mut change = request(1, Some("doing-team"), Some("employee"));
    change.task_id = "imported-task".into();
    assert!(matches!(
        assign(&store, authority(), change).unwrap(),
        AssignmentOutcome::Completed { revision: 2, .. }
    ));
    let later = progress(&store);
    assert_eq!(
        adoption::run(&store, root.clone(), authority(), staged.action.clone()).unwrap(),
        expected
    );
    assert_eq!(progress(&store), later);
    retained(&store, &root, &staged);
}

const PRIVATE: &str = "classification: private\n";
const UNCLASSIFIED: &str = "metadata:\n  nested:\n    - classification: unclassified\n";
const NOTES: &str = "metadata:\n  nested:\n    - private_notes:\n        id: private-note-id\n        author_id: private-author\n        body: must remain staged\n";
macro_rules! privacy_test {
    ($name:ident, $form:ident, $provenance:ident) => {
        #[test]
        fn $name() {
            refuses(Form::$form, $provenance);
        }
    };
}
privacy_test!(current_private, Current, PRIVATE);
privacy_test!(current_unclassified, Current, UNCLASSIFIED);
privacy_test!(current_private_notes, Current, NOTES);
privacy_test!(historical_inline_private, HistoricalInline, PRIVATE);
privacy_test!(
    historical_inline_unclassified,
    HistoricalInline,
    UNCLASSIFIED
);
privacy_test!(historical_inline_private_notes, HistoricalInline, NOTES);
privacy_test!(historical_referenced_private, HistoricalReference, PRIVATE);
privacy_test!(
    historical_referenced_unclassified,
    HistoricalReference,
    UNCLASSIFIED
);
privacy_test!(
    historical_referenced_private_notes,
    HistoricalReference,
    NOTES
);

#[test]
fn public_unknown_snapshot_extensions_numbers_and_body_and_opaque_blobs_stay_exact() {
    let provenance = "classification: public\nunknown_public_extension:\n  nested:\n    - amount: 1.234567890123456789E+112\n      example: 'private_notes: ordinary public text'\n";
    for form in [
        Form::Current,
        Form::HistoricalInline,
        Form::HistoricalReference,
    ] {
        let (dir, store, config) = fixture();
        let root = dir.path().join("snapshot-staging");
        let staged = stage(&store, &root, form, provenance, true);
        assert!(matches!(
            adoption::run(&store, root.clone(), authority(), staged.action.clone()).unwrap(),
            Outcome::Completed { .. }
        ));
        assert_eq!(
            std::fs::read(
                config
                    .public_root
                    .join("teams/accountable/work/tasks/imported-task.md")
            )
            .unwrap(),
            staged.current
        );
        for (revision, bytes) in &staged.events {
            assert_eq!(
                std::fs::read(
                    config
                        .public_root
                        .join(format!("work-history/tasks/imported-task/{revision}.json"))
                )
                .unwrap(),
                *bytes
            );
        }
        for bytes in &staged.blobs {
            assert_eq!(
                std::fs::read(config.public_root.join(format!(
                    "work-history/blobs/sha256/{}",
                    codec::sha256(bytes)
                )))
                .unwrap(),
                *bytes
            );
        }
        if matches!(form, Form::Current) {
            assert_eq!(
                std::fs::read(config.public_root.join(format!(
                    "work-history/blobs/sha256/{}",
                    codec::sha256(&staged.current)
                )))
                .unwrap(),
                staged.current
            );
        }
        retained(&store, &root, &staged);
    }
}
