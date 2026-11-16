//! Real staging/adoption/journal regressions. All roots and credentials are disposable.
use super::*;
use crate::selected_tasks::{adoption, read, staging};
use org_proto::selected_tasks::{
    adoption::{Action, Outcome},
    read::{Query, Result as Read},
    staging::Blob,
    Revision,
};
use std::path::Path;

struct StagedTask {
    action: Action,
    source: Vec<u8>,
    manifest: Vec<u8>,
}

fn stage_task(store: &Store, root: &Path, task: &str) -> StagedTask {
    let source = SOURCE
        .replace(
            "id: opaque-task\n",
            &format!("id: {}\n", serde_json::to_string(task).unwrap()),
        )
        .replace("number: 42\n", "number: null\n")
        .into_bytes();
    let manifest = serde_json::to_vec(&serde_json::json!({
        "version": 1,
        "resource_profile": codec::RESOURCE_PROFILE,
        "task_id": task,
        "canonical_path": format!("teams/accountable/work/tasks/{task}.md"),
        "public_snapshot": {"sha256": codec::sha256(&source), "byte_length": source.len()},
        "history_origin": {"kind": "snapshot_only"},
        "events": [], "public_blobs": [], "number_counter": null, "number_allocation": null
    }))
    .unwrap();
    let stage = crate::ulid::generate().unwrap();
    let digest = codec::sha256(&manifest);
    for bytes in [&manifest, &source] {
        let mut params = staged_request(&stage, bytes, Some(0));
        params.manifest_sha256 = digest.clone();
        assert!(
            staging::run(store, root.to_path_buf(), authority(), params)
                .unwrap()
                .verified
        );
    }
    StagedTask {
        action: Action::Commit {
            operation_id: org_proto::gateway::OperationId::parse(crate::ulid::generate().unwrap())
                .unwrap(),
            stage_id: org_proto::gateway::OperationId::parse(stage).unwrap(),
            manifest_sha256: digest,
            expected_directory_generation: Revision::new(1).unwrap(),
        },
        source,
        manifest,
    }
}

fn assert_retained(store: &Store, root: &Path, staged: &StagedTask) {
    let Action::Commit {
        stage_id,
        manifest_sha256,
        ..
    } = &staged.action
    else {
        unreachable!()
    };
    for expected in [&staged.manifest, &staged.source] {
        let root = root.to_path_buf();
        let stage = stage_id.as_str().to_owned();
        let manifest = manifest_sha256.clone();
        let blob = Blob {
            sha256: codec::sha256(expected),
            byte_length: expected.len() as u64,
        };
        let bytes = store
            .selected_read(move |_, id| {
                staging::source(
                    &root,
                    id,
                    &OwnerAccount {
                        account_id: "account".into(),
                        member_id: "owner-member".into(),
                    },
                    &stage,
                    &manifest,
                    &blob,
                    codec::MAX_JSON_BYTES,
                )
            })
            .unwrap();
        assert_eq!(&bytes, expected);
    }
}

#[derive(Debug, PartialEq, Eq)]
struct State {
    ordinal: i64,
    digest: String,
    origin: (String, i64),
    tasks_events_adoptions: [i64; 3],
    public: Vec<(String, Vec<u8>)>,
}

fn state(store: &Store) -> State {
    store
        .selected_read(|db, _| {
            let (ordinal, digest, incarnation, high_water) = db
                .query_row(
                    "SELECT ordinal,digest,origin_incarnation,origin_high_water FROM selected_journal_cursor",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .map_err(storage)?;
            let mut counts = [0; 3];
            for (i, table) in ["selected_tasks", "selected_events", "selected_adoptions"]
                .iter()
                .enumerate()
            {
                counts[i] = db
                    .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                    .map_err(storage)?;
            }
            let mut query = db
                .prepare("SELECT path,content FROM selected_artifacts WHERE namespace='public' ORDER BY path")
                .map_err(storage)?;
            let public = query
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(storage)?
                .collect::<Result<_, _>>()
                .map_err(storage)?;
            Ok(State {
                ordinal,
                digest,
                origin: (incarnation, high_water),
                tasks_events_adoptions: counts,
                public,
            })
        })
        .unwrap()
}

fn public_files(root: &Path) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
    fn visit(root: &Path, directory: &Path, out: &mut BTreeMap<std::path::PathBuf, Vec<u8>>) {
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
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

fn assert_receipt_only(config: &AcceptanceConfig, ordinal: i64) {
    let bytes = std::fs::read(config.protected_root.join(format!(
        "offices/office/journals/work-acceptance/journal/transactions/{ordinal}.json"
    )))
    .unwrap();
    let envelope: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(envelope["origin_entries"].as_array().unwrap().is_empty());
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
}

fn assert_detail(store: &Store, task_id: &str, revision: i64) {
    let Read::Detail { task, .. } = read::read(
        store,
        authority(),
        Query::Detail {
            task_id: task_id.into(),
        },
    )
    .unwrap() else {
        panic!("detail")
    };
    assert_eq!(task.task_id, task_id);
    assert_eq!(task.revision, revision);
    assert!(read::ready(store).unwrap());
}

fn unsupported_task_stays_staged(task: &str) {
    let (dir, store, config) = fixture();
    let root = dir.path().join("adoption-staging");
    let before = state(&store);
    let files_before = public_files(&config.public_root);
    let staged = stage_task(&store, &root, task);
    assert_eq!(state(&store), before, "upload must not journal acceptance");
    let outcome = adoption::run(&store, root.clone(), authority(), staged.action.clone()).unwrap();
    assert_eq!(
        outcome,
        Outcome::Staged {
            reason_code: "unsupported_or_inconsistent_source".into()
        }
    );
    let refused = state(&store);
    assert_eq!(refused.ordinal, before.ordinal + 1);
    assert_eq!(refused.origin, before.origin);
    assert_eq!(
        refused.tasks_events_adoptions,
        before.tasks_events_adoptions
    );
    assert_eq!(refused.public, before.public);
    assert_eq!(public_files(&config.public_root), files_before);
    assert_receipt_only(&config, refused.ordinal);
    assert_retained(&store, &root, &staged);
    assert_eq!(
        adoption::run(&store, root.clone(), authority(), staged.action.clone()).unwrap(),
        outcome
    );
    assert_eq!(
        state(&store),
        refused,
        "exact retry must not advance the journal"
    );

    let valid = stage_task(&store, &root, "after-invalid");
    assert!(matches!(
        adoption::run(&store, root.clone(), authority(), valid.action).unwrap(),
        Outcome::Completed { revision: 1, .. }
    ));
    assert_detail(&store, "after-invalid", 1);
    let mut change = request(1, None, None);
    change.task_id = "after-invalid".into();
    assert!(matches!(
        assign(&store, authority(), change).unwrap(),
        AssignmentOutcome::Completed { revision: 2, .. }
    ));
    let after_valid = state(&store);
    drop(store);
    let store = Store::open(
        &dir.path().join("office.db"),
        &Key::from_bytes([31; 32]),
        Opening::Existing,
        &OFFICE_SCHEMA,
    )
    .unwrap();
    store.install_acceptance(config).unwrap();
    assert_eq!(state(&store), after_valid);
    assert_detail(&store, "after-invalid", 2);
    assert_retained(&store, &root, &staged);
    assert_eq!(
        adoption::run(&store, root, authority(), staged.action).unwrap(),
        outcome
    );
    assert_eq!(state(&store), after_valid);
}

#[test]
fn ascii_253_stays_staged() {
    unsupported_task_stays_staged(&"a".repeat(253));
}
#[test]
fn ascii_254_stays_staged() {
    unsupported_task_stays_staged(&"a".repeat(254));
}
#[test]
fn ascii_255_stays_staged() {
    unsupported_task_stays_staged(&"a".repeat(255));
}
#[test]
fn utf8_253_bytes_stays_staged() {
    unsupported_task_stays_staged(&format!("{}x", "é".repeat(126)));
}
#[test]
fn utf8_254_bytes_stays_staged() {
    unsupported_task_stays_staged(&"é".repeat(127));
}
#[test]
fn utf8_255_bytes_stays_staged() {
    unsupported_task_stays_staged(&format!("{}x", "é".repeat(127)));
}
#[test]
fn backslash_stays_staged() {
    unsupported_task_stays_staged("task\\id");
}

#[test]
fn boundary_and_opaque_ids_publish_exact_sources_and_reopen() {
    let (dir, store, config) = fixture();
    let root = dir.path().join("adoption-staging");
    let ids = [
        "a".repeat(252),
        "é".repeat(126),
        "%5C".into(),
        "%2f".into(),
        "%2e%2e".into(),
        "task café 東京 with spaces".into(),
        "task?#fragment".into(),
    ];
    let mut expected = Vec::new();
    for id in ids {
        let staged = stage_task(&store, &root, &id);
        assert_eq!(
            adoption::run(&store, root.clone(), authority(), staged.action.clone()).unwrap(),
            Outcome::Completed {
                task_id: id.clone(),
                revision: 1,
                history_coverage: "snapshot_baseline".into()
            }
        );
        let path = config
            .public_root
            .join(format!("teams/accountable/work/tasks/{id}.md"));
        if id.len() == 252 {
            assert_eq!(path.file_name().unwrap().as_encoded_bytes().len(), 255);
        }
        assert_eq!(std::fs::read(&path).unwrap(), staged.source);
        assert_detail(&store, &id, 1);
        assert_retained(&store, &root, &staged);
        expected.push((id, staged.source));
    }
    let before = state(&store);
    drop(store);
    let store = Store::open(
        &dir.path().join("office.db"),
        &Key::from_bytes([31; 32]),
        Opening::Existing,
        &OFFICE_SCHEMA,
    )
    .unwrap();
    store.install_acceptance(config.clone()).unwrap();
    assert_eq!(state(&store), before);
    for (id, source) in expected {
        assert_detail(&store, &id, 1);
        assert_eq!(
            std::fs::read(
                config
                    .public_root
                    .join(format!("teams/accountable/work/tasks/{id}.md"))
            )
            .unwrap(),
            source
        );
    }
}
