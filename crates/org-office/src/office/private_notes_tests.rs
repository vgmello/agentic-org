//! Engine-independent conformance tests for `TASK-50` (`L8`: Private task notes),
//! verifying `PN-01..06` and `ADR-0031` `TASKAUTH-4..7` on BOTH `Memory` and `Sqlite`,
//! including SQLite restart, `office.db` delete+rebuild survival, and `durable/` leak audit.

use std::path::Path;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use org_proto::private_notes::{
    TasksNotesAddParams, TasksNotesGetParams, TasksNotesListParams, TasksNotesPublishParams,
    TasksNotesRemoveParams, TasksNotesUpdateParams,
};
use org_proto::tasks::{
    PatchField, TaskStatus, TasksAssignParams, TasksCreateParams, TasksListParams,
    TasksTransitionParams,
};
use org_proto::ErrorCode;
use org_store::testkit::{identity, FakeCustody, FixedClock};

use crate::office::testkit::{places, record, request, CA_SEED};
use crate::office::{Custody, DataRoots, Office, Options};
use crate::organization::testkit::CREATION_AT;
use crate::store::open::EngineChoice;

static NEXT_ID: AtomicU64 = AtomicU64::new(50_000);

fn next_uuid() -> String {
    let n = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let ms = CREATION_AT.0 + n;
    let mut b = [0u8; 16];
    b[0..6].copy_from_slice(&ms.to_be_bytes()[2..8]);
    b[6] = 0x70 | ((n >> 8) as u8 & 0x0f);
    b[7] = (n & 0xff) as u8;
    b[8] = 0x80;
    b[15] = (n & 0xff) as u8;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

struct PrivateNoteFixture {
    _tmp: tempfile::TempDir,
    folder: std::path::PathBuf,
    roots: DataRoots,
    custody: Custody,
    office: Office,
    owner: String,
    agent_a: String,
    agent_b: String,
    task_key: String,
}

fn seed_fixture(engine: EngineChoice) -> PrivateNoteFixture {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (folder, roots) = places(tmp.path());
    std::fs::create_dir_all(&roots.durable).unwrap();
    std::fs::create_dir_all(&roots.runtime).unwrap();
    let here = identity();
    let custody = Custody {
        identity: here.clone(),
        authority: Arc::new(FakeCustody::new()),
    };
    let (new, binding) = request();
    let options = Options {
        engine,
        clock: Arc::new(FixedClock(CREATION_AT)),
        testing: Default::default(),
        before_final_checks: None,
    };
    let office = Office::provision_with(
        folder.clone(),
        &new,
        None,
        &binding,
        roots.clone(),
        custody.clone(),
        |_| record(CA_SEED),
        options,
    )
    .expect("provision test office");
    let owner = office.owner_member_id().to_owned();
    let root_team = office.org_snapshot().unwrap().teams[0].id.clone();

    let agent_a = seed_agent(&office, &owner, &root_team, "Alpha Agent", "alpha");
    let agent_b = seed_agent(&office, &owner, &root_team, "Beta Agent", "beta");

    let task = office
        .task_create(
            &owner,
            TasksCreateParams {
                command_id: next_uuid(),
                task_id: next_uuid(),
                team_id: Some(root_team),
                project_id: None,
                assigned_team_id: None,
                assignee_id: Some(agent_a.clone()),
                task_type: Some("task".into()),
                work_type: None,
                title: "Secure token rotation".into(),
                body_markdown: "Rotate signing keys safely".into(),
                priority: None,
                effort: None,
                severity: None,
                rank_order: Some(1),
                labels: vec!["security".into()],
                reference_files: vec![],
                modified_files: vec![],
                depends_on: vec![],
                details: None,
            },
        )
        .unwrap()
        .unwrap()
        .task;

    PrivateNoteFixture {
        _tmp: tmp,
        folder,
        roots,
        custody,
        office,
        owner,
        agent_a,
        agent_b,
        task_key: task.key,
    }
}

fn seed_agent(office: &Office, owner: &str, team_id: &str, name: &str, handle: &str) -> String {
    use org_proto::hiring::{HireProposalWire, HrHire, HrHireParams};
    use org_proto::{Outcome, Request};
    let employee_id = next_uuid();
    let req = Request::new::<HrHire>(
        1,
        &HrHireParams {
            command_id: next_uuid(),
            request_id: next_uuid(),
            employee_id: employee_id.clone(),
            home_team_id: team_id.into(),
            role_id: None,
            proposal: HireProposalWire {
                name: name.into(),
                handle: handle.into(),
                title_in_team: Some("Engineer".into()),
                ..Default::default()
            },
            reason: "seed test agent".into(),
        },
    )
    .expect("hr.hire request");
    let resp = office
        .dispatch_hiring_rpc(Some(owner), &req)
        .expect("hr.hire handled");
    assert!(
        matches!(resp.outcome, Outcome::Result(_)),
        "hr.hire failed: {:?}",
        resp.outcome
    );
    employee_id
}

fn assert_pn01_pn02_taskauth4_and_6(f: &PrivateNoteFixture, secret: &str) -> String {
    let note_id = next_uuid();
    let create_cmd = next_uuid();
    let created = f
        .office
        .task_note_add(
            &f.agent_a,
            TasksNotesAddParams {
                command_id: create_cmd.clone(),
                task: f.task_key.clone(),
                note_id: note_id.clone(),
                body_markdown: format!("Agent A plan: {secret}"),
            },
        )
        .unwrap()
        .unwrap()
        .note;
    assert_eq!(created.revision, 1);
    assert!(created.editable);

    // Exact replay returns the same note; mismatched reuse returns OperationReused.
    let replayed = f
        .office
        .task_note_add(
            &f.agent_a,
            TasksNotesAddParams {
                command_id: create_cmd.clone(),
                task: f.task_key.clone(),
                note_id: note_id.clone(),
                body_markdown: format!("Agent A plan: {secret}"),
            },
        )
        .unwrap()
        .unwrap()
        .note;
    assert_eq!(replayed.id, created.id);
    let reused = f
        .office
        .task_note_add(
            &f.agent_a,
            TasksNotesAddParams {
                command_id: create_cmd,
                task: f.task_key.clone(),
                note_id: note_id.clone(),
                body_markdown: "Different body".into(),
            },
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(reused.code, ErrorCode::OperationReused);

    // TASKAUTH-4 / PN-02: author reads editable=true; person reads editable=false; agent B sees NOTHING.
    let person_list = f
        .office
        .task_note_list(
            &f.owner,
            TasksNotesListParams {
                task: f.task_key.clone(),
                author_id: None,
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(person_list.notes.len(), 2);
    assert!(person_list.notes.iter().all(|n| !n.editable));

    let agent_b_list = f
        .office
        .task_note_list(
            &f.agent_b,
            TasksNotesListParams {
                task: f.task_key.clone(),
                author_id: Some(f.agent_a.clone()),
            },
        )
        .unwrap()
        .unwrap();
    assert!(agent_b_list.notes.is_empty());

    let agent_b_get = f
        .office
        .task_note_get(
            &f.agent_b,
            TasksNotesGetParams {
                note_id: note_id.clone(),
            },
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(agent_b_get.code, ErrorCode::NotFound);

    // Zero leakage into shared task inspect, search, comments, or history.
    let search = f
        .office
        .task_list(TasksListParams {
            query: Some(secret.into()),
            ..Default::default()
        })
        .unwrap()
        .unwrap();
    assert!(search.tasks.is_empty());
    let inspected = f.office.task_inspect(&f.task_key).unwrap().unwrap();
    assert!(inspected.comments.is_empty());
    assert!(!format!("{inspected:?}").contains(secret));

    // TASKAUTH-6: person cannot edit/delete agent A's note (NotAuthorized); agent B gets NotFound.
    let person_edit = f
        .office
        .task_note_update(
            &f.owner,
            TasksNotesUpdateParams {
                command_id: next_uuid(),
                note_id: note_id.clone(),
                expected_revision: 1,
                body_markdown: "Human overwrite".into(),
            },
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(person_edit.code, ErrorCode::NotAuthorized);
    let agent_b_edit = f
        .office
        .task_note_update(
            &f.agent_b,
            TasksNotesUpdateParams {
                command_id: next_uuid(),
                note_id: note_id.clone(),
                expected_revision: 1,
                body_markdown: "Peer agent overwrite".into(),
            },
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(agent_b_edit.code, ErrorCode::NotFound);

    // Stale revision rejected with Conflict; author edit at rev 1 succeeds -> rev 2.
    let stale = f
        .office
        .task_note_update(
            &f.agent_a,
            TasksNotesUpdateParams {
                command_id: next_uuid(),
                note_id: note_id.clone(),
                expected_revision: 99,
                body_markdown: "Stale".into(),
            },
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(stale.code, ErrorCode::Conflict);
    let edited = f
        .office
        .task_note_update(
            &f.agent_a,
            TasksNotesUpdateParams {
                command_id: next_uuid(),
                note_id: note_id.clone(),
                expected_revision: 1,
                body_markdown: format!("Refined plan: {secret}"),
            },
        )
        .unwrap()
        .unwrap()
        .note;
    assert_eq!(edited.revision, 2);
    note_id
}

fn assert_pn03_pn05_pn06_and_taskauth5_7(f: &PrivateNoteFixture, note_id: &str, secret: &str) {
    // PN-03 / TASKAUTH-5: reassigning task to Agent B never exposes Agent A's private note.
    let current = f.office.task_inspect(&f.task_key).unwrap().unwrap().task;
    let reassigned = f
        .office
        .task_assign(
            &f.owner,
            TasksAssignParams {
                command_id: next_uuid(),
                task: f.task_key.clone(),
                expected_revision: current.revision,
                assigned_team_id: PatchField::Unchanged,
                assignee_id: PatchField::Set(Some(f.agent_b.clone())),
            },
        )
        .unwrap()
        .unwrap()
        .task;
    assert_eq!(reassigned.assignee_id.as_deref(), Some(f.agent_b.as_str()));
    assert!(f
        .office
        .task_note_list(
            &f.agent_b,
            TasksNotesListParams {
                task: f.task_key.clone(),
                author_id: None,
            },
        )
        .unwrap()
        .unwrap()
        .notes
        .is_empty());

    // PN-05 / TASKAUTH-7: explicit publish creates shared comment readable by Agent B & Person,
    // while private original remains private (and replay does not duplicate the comment).
    let pub_cmd = next_uuid();
    let comment_id = next_uuid();
    let (pub1, replayed1) = f
        .office
        .task_note_publish(
            &f.agent_a,
            TasksNotesPublishParams {
                command_id: pub_cmd.clone(),
                note_id: note_id.to_owned(),
                expected_revision: 2,
                comment_id: comment_id.clone(),
            },
        )
        .unwrap()
        .unwrap();
    assert!(!replayed1);
    assert_eq!(pub1.comment.id, comment_id);
    assert!(pub1.comment.body_markdown.contains(secret));

    let (pub2, replayed2) = f
        .office
        .task_note_publish(
            &f.agent_a,
            TasksNotesPublishParams {
                command_id: pub_cmd,
                note_id: note_id.to_owned(),
                expected_revision: 2,
                comment_id,
            },
        )
        .unwrap()
        .unwrap();
    assert!(replayed2);
    assert_eq!(pub2.comment.id, pub1.comment.id);
    assert_eq!(
        f.office
            .task_comments(&f.task_key)
            .unwrap()
            .unwrap()
            .comments
            .len(),
        1
    );
    // Agent B STILL cannot read the private original!
    assert_eq!(
        f.office
            .task_note_get(
                &f.agent_b,
                TasksNotesGetParams {
                    note_id: note_id.to_owned(),
                },
            )
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );

    // PN-06: author soft-deletes a second note -> revisioned tombstone cannot be read or re-edited.
    let temp_id = next_uuid();
    f.office
        .task_note_add(
            &f.agent_a,
            TasksNotesAddParams {
                command_id: next_uuid(),
                task: f.task_key.clone(),
                note_id: temp_id.clone(),
                body_markdown: "Scratchpad to delete".into(),
            },
        )
        .unwrap()
        .unwrap();
    let removed = f
        .office
        .task_note_remove(
            &f.agent_a,
            TasksNotesRemoveParams {
                command_id: next_uuid(),
                note_id: temp_id.clone(),
                expected_revision: 1,
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(removed.revision, 2);
    assert_eq!(
        f.office
            .task_note_get(&f.agent_a, TasksNotesGetParams { note_id: temp_id })
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
}

fn dir_contains_bytes_excluding(root: &Path, skip_dir: &Path, needle: &[u8]) -> bool {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if dir == skip_dir {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path == skip_dir {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(bytes) = std::fs::read(&path) {
                if bytes.windows(needle.len()).any(|w| w == needle) {
                    return true;
                }
            }
        }
    }
    false
}

#[test]
fn pn_01_through_06_and_taskauth_4_through_7_on_memory_and_sqlite() {
    for engine in [EngineChoice::Memory, EngineChoice::Sqlite] {
        let f = seed_fixture(engine);
        let unpublished_secret = "UNPUBLISHED_PRIVATE_RATIONALE_XYZ987";
        let published_secret = "EXPLICITLY_PUBLISHED_RATIONALE_ABC123";

        // Add an unpublished private note so we can audit durable/ for zero leakage even after publish!
        let unpub_id = next_uuid();
        f.office
            .task_note_add(
                &f.agent_a,
                TasksNotesAddParams {
                    command_id: next_uuid(),
                    task: f.task_key.clone(),
                    note_id: unpub_id.clone(),
                    body_markdown: format!("Never published: {unpublished_secret}"),
                },
            )
            .unwrap()
            .unwrap();

        let note_id = assert_pn01_pn02_taskauth4_and_6(&f, published_secret);
        assert_pn03_pn05_pn06_and_taskauth5_7(&f, &note_id, published_secret);
        assert_extended_security_and_replay_boundaries(&f, engine);
        assert_pending_publish_lock_and_paused_agent_refusal(&f);

        // PN-04 + ARCH-01: Zero bytes of the unpublished private note anywhere under durable/
        // outside the HQ-authoritative `durable/private/` directory (`doc-79` §Private Task Notes).
        let private_dir = f.roots.durable.join("private");
        assert!(
            !dir_contains_bytes_excluding(
                &f.roots.durable,
                &private_dir,
                unpublished_secret.as_bytes()
            ),
            "private note text leaked into shared durable/ outside durable/private/!"
        );

        // PN-04 SQLite restart + task closure + ENTIRE runtime/<office-id> wipe survival!
        if engine == EngineChoice::Sqlite {
            let cur = f.office.task_inspect(&f.task_key).unwrap().unwrap().task;
            f.office
                .task_transition(
                    &f.owner,
                    TasksTransitionParams {
                        command_id: next_uuid(),
                        task: f.task_key.clone(),
                        expected_revision: cur.revision,
                        status: TaskStatus::Done,
                        blocked_reason: None,
                        resolution: Some("completed".into()),
                        note: None,
                    },
                )
                .unwrap()
                .unwrap();

            let office_id = f.office.info().office_id.clone();
            let instance_id = f.custody.identity.instance_id.as_str().to_owned();
            f.office.close().expect("close office");

            // Seed real session history + an oversized (>16 MiB) companion file under excluded
            // durable subtrees to prove full runtime wipe rebuild prunes them before descent
            // and file reads (`TASK-97` / Finding #3).
            let session_dir = f
                .roots
                .durable
                .join("local/offices")
                .join(&office_id)
                .join("sessions")
                .join(&f.agent_a);
            std::fs::create_dir_all(&session_dir).expect("create session dir");
            std::fs::write(session_dir.join("history.jsonl"), b"{\"turn\":1}\n")
                .expect("write session history");
            let oversized_dir = f.roots.durable.join("accounting/usage/oversized-test");
            std::fs::create_dir_all(&oversized_dir).expect("create oversized dir");
            std::fs::write(oversized_dir.join("big.bin"), vec![b'x'; 17 * 1024 * 1024])
                .expect("write >16 MiB companion file");

            // Delete the ENTIRE disposable runtime/<office-id> cache: HQ durable/private/task-notes.db survives!
            let runtime_office_dir = f.roots.runtime.join(&office_id);
            std::fs::remove_dir_all(&runtime_office_dir).expect("remove runtime office dir");
            assert!(
                f.roots.durable.join("private/task-notes.db").is_file(),
                "task-notes.db must live under HQ durable/private/task-notes.db"
            );
            assert!(
                !runtime_office_dir.join("private/task-notes.db").exists(),
                "task-notes.db must NOT live under disposable runtime/"
            );

            let reopened = Office::open_with(
                f.folder.clone(),
                None,
                &instance_id,
                f.roots.clone(),
                f.custody.clone(),
                Options {
                    engine: EngineChoice::Sqlite,
                    clock: Arc::new(FixedClock(CREATION_AT)),
                    testing: Default::default(),
                    before_final_checks: None,
                },
            )
            .expect("reopen office after office.db rebuild");

            // Notes survive restart, task closure, and office.db rebuild with ADR-0031 intact!
            assert_eq!(
                reopened
                    .task_note_get(
                        &f.agent_a,
                        TasksNotesGetParams {
                            note_id: unpub_id.clone()
                        }
                    )
                    .unwrap()
                    .unwrap()
                    .note
                    .body_markdown,
                format!("Never published: {unpublished_secret}")
            );
            assert_eq!(
                reopened
                    .task_note_get(&f.agent_b, TasksNotesGetParams { note_id: unpub_id })
                    .unwrap()
                    .unwrap_err()
                    .code,
                ErrorCode::NotFound
            );
            assert_eq!(
                reopened
                    .task_note_list(
                        &f.owner,
                        TasksNotesListParams {
                            task: f.task_key.clone(),
                            author_id: None,
                        },
                    )
                    .unwrap()
                    .unwrap()
                    .notes
                    .len(),
                3
            );
            reopened.close().expect("close reopened");
        }
    }
}

fn assert_extended_security_and_replay_boundaries(f: &PrivateNoteFixture, engine: EngineChoice) {
    // (a) Person-authored private note is hidden from agents (ADR-0031 §6).
    let person_note_id = next_uuid();
    let create_cmd = next_uuid();
    f.office
        .task_note_add(
            &f.owner,
            TasksNotesAddParams {
                command_id: create_cmd.clone(),
                task: f.task_key.clone(),
                note_id: person_note_id.clone(),
                body_markdown: "Human-only supervisory note v1".into(),
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        f.office
            .task_note_get(
                &f.agent_a,
                TasksNotesGetParams {
                    note_id: person_note_id.clone(),
                },
            )
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );

    // (b) Non-member is refused NotAuthorized on read and write.
    let outsider = next_uuid();
    assert_eq!(
        f.office
            .task_note_list(
                &outsider,
                TasksNotesListParams {
                    task: f.task_key.clone(),
                    author_id: None,
                },
            )
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );

    // (c) Person cannot remove or publish an agent's note (NotAuthorized); peer agent gets NotFound.
    let agent_note_id = next_uuid();
    f.office
        .task_note_add(
            &f.agent_a,
            TasksNotesAddParams {
                command_id: next_uuid(),
                task: f.task_key.clone(),
                note_id: agent_note_id.clone(),
                body_markdown: "Agent A exclusive note".into(),
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        f.office
            .task_note_remove(
                &f.owner,
                TasksNotesRemoveParams {
                    command_id: next_uuid(),
                    note_id: agent_note_id.clone(),
                    expected_revision: 1,
                },
            )
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
    assert_eq!(
        f.office
            .task_note_publish(
                &f.owner,
                TasksNotesPublishParams {
                    command_id: next_uuid(),
                    note_id: agent_note_id.clone(),
                    expected_revision: 1,
                    comment_id: next_uuid(),
                },
            )
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
    assert_eq!(
        f.office
            .task_note_remove(
                &f.agent_b,
                TasksNotesRemoveParams {
                    command_id: next_uuid(),
                    note_id: agent_note_id,
                    expected_revision: 1,
                },
            )
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );

    // (d) Post-edit and post-delete idempotent replay returns original accepted revision/body!
    let edit_cmd = next_uuid();
    f.office
        .task_note_update(
            &f.owner,
            TasksNotesUpdateParams {
                command_id: edit_cmd.clone(),
                note_id: person_note_id.clone(),
                expected_revision: 1,
                body_markdown: "Human-only supervisory note v2".into(),
            },
        )
        .unwrap()
        .unwrap();
    f.office
        .task_note_remove(
            &f.owner,
            TasksNotesRemoveParams {
                command_id: next_uuid(),
                note_id: person_note_id.clone(),
                expected_revision: 2,
            },
        )
        .unwrap()
        .unwrap();
    let replay_create = f
        .office
        .task_note_add(
            &f.owner,
            TasksNotesAddParams {
                command_id: create_cmd,
                task: f.task_key.clone(),
                note_id: person_note_id.clone(),
                body_markdown: "Human-only supervisory note v1".into(),
            },
        )
        .unwrap()
        .unwrap()
        .note;
    assert_eq!(replay_create.revision, 1);
    assert_eq!(
        replay_create.body_markdown,
        "Human-only supervisory note v1"
    );
    let replay_edit = f
        .office
        .task_note_update(
            &f.owner,
            TasksNotesUpdateParams {
                command_id: edit_cmd,
                note_id: person_note_id,
                expected_revision: 1,
                body_markdown: "Human-only supervisory note v2".into(),
            },
        )
        .unwrap()
        .unwrap()
        .note;
    assert_eq!(replay_edit.revision, 2);
    assert_eq!(replay_edit.body_markdown, "Human-only supervisory note v2");

    // (f) On SQLite Unix, durable/private directory is 0o700 and task-notes.db is 0o600.
    if engine == EngineChoice::Sqlite {
        use std::os::unix::fs::PermissionsExt;
        let private_dir = f.roots.durable.join("private");
        let db_file = private_dir.join("task-notes.db");
        assert_eq!(
            std::fs::metadata(&private_dir)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&db_file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

fn assert_pending_publish_lock_and_paused_agent_refusal(f: &PrivateNoteFixture) {
    use crate::private_notes::{request_digest, BeginPublishCmd, PublishAttempt};
    use org_proto::hiring::{AgentSetStatus, AgentSetStatusParams};
    use org_proto::{Outcome, Request};

    // (e) Simulate crash mid-publish (pending receipt): edit/delete blocked with Conflict,
    // then retrying `task_note_publish` with the same command_id completes and unlocks!
    let locked_note_id = next_uuid();
    f.office
        .task_note_add(
            &f.agent_a,
            TasksNotesAddParams {
                command_id: next_uuid(),
                task: f.task_key.clone(),
                note_id: locked_note_id.clone(),
                body_markdown: "Pending publish body".into(),
            },
        )
        .unwrap()
        .unwrap();
    let pub_cmd = next_uuid();
    let comment_id = next_uuid();
    let rev_bytes = 1u64.to_be_bytes();
    let digest = request_digest(&[
        b"publish\0",
        locked_note_id.as_bytes(),
        b"\0",
        &rev_bytes,
        b"\0",
        comment_id.as_bytes(),
    ]);
    let pending = f
        .office
        .private_notes
        .begin_publish(BeginPublishCmd {
            org_id: &f.office.info().org_id,
            caller_id: &f.agent_a,
            caller_is_person: false,
            note_id: &locked_note_id,
            expected_revision: 1,
            comment_id: &comment_id,
            operation_id: &pub_cmd,
            request_sha256: &digest,
            now: "2026-11-22T00:00:00.000Z",
        })
        .unwrap()
        .unwrap();
    assert!(matches!(pending, PublishAttempt::Pending { .. }));

    assert_eq!(
        f.office
            .task_note_update(
                &f.agent_a,
                TasksNotesUpdateParams {
                    command_id: next_uuid(),
                    note_id: locked_note_id.clone(),
                    expected_revision: 1,
                    body_markdown: "Blocked while pending".into(),
                },
            )
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );

    let (recovered, _) = f
        .office
        .task_note_publish(
            &f.agent_a,
            TasksNotesPublishParams {
                command_id: pub_cmd,
                note_id: locked_note_id.clone(),
                expected_revision: 1,
                comment_id: comment_id.clone(),
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(recovered.comment.id, comment_id);

    // Clean up locked_note_id so post-rebuild active count stays 3.
    f.office
        .task_note_remove(
            &f.agent_a,
            TasksNotesRemoveParams {
                command_id: next_uuid(),
                note_id: locked_note_id,
                expected_revision: 1,
            },
        )
        .unwrap()
        .unwrap();

    // (b2) Paused agent is refused NotAuthorized on read and write (TASKAUTH-3).
    let emp = f.office.load_employee(&f.agent_b).unwrap().unwrap();
    let pause_req = Request::new::<AgentSetStatus>(
        2,
        &AgentSetStatusParams {
            command_id: next_uuid(),
            member_id: f.agent_b.clone(),
            expected_version: emp.meta.revision,
            paused: true,
        },
    )
    .unwrap();
    assert!(matches!(
        f.office
            .dispatch_hiring_rpc(Some(&f.owner), &pause_req)
            .unwrap()
            .outcome,
        Outcome::Result(_)
    ));
    assert_eq!(
        f.office
            .task_note_list(
                &f.agent_b,
                TasksNotesListParams {
                    task: f.task_key.clone(),
                    author_id: None,
                },
            )
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
    // Unpause agent_b so post-rebuild checks still verify active-agent NotFound isolation!
    let emp2 = f.office.load_employee(&f.agent_b).unwrap().unwrap();
    let resume_req = Request::new::<AgentSetStatus>(
        3,
        &AgentSetStatusParams {
            command_id: next_uuid(),
            member_id: f.agent_b.clone(),
            expected_version: emp2.meta.revision,
            paused: false,
        },
    )
    .unwrap();
    assert!(matches!(
        f.office
            .dispatch_hiring_rpc(Some(&f.owner), &resume_req)
            .unwrap()
            .outcome,
        Outcome::Result(_)
    ));
}

/// ADR-0031 holds for the built-in Chief of Staff (`TASK-80.1`): she is an agent, so another
/// agent's private notes stay invisible to her, whoever her run acts for.
#[test]
fn the_chief_of_staff_never_reads_another_agents_private_notes() {
    for engine in [EngineChoice::Memory, EngineChoice::Sqlite] {
        let f = seed_fixture(engine);
        let tess = f
            .office
            .chief_of_staff()
            .unwrap()
            .expect("the Chief of Staff")
            .id
            .to_string();
        let note_id = next_uuid();
        f.office
            .task_note_add(
                &f.agent_a,
                TasksNotesAddParams {
                    command_id: next_uuid(),
                    task: f.task_key.clone(),
                    note_id: note_id.clone(),
                    body_markdown: "Alpha's private reasoning".into(),
                },
            )
            .unwrap()
            .unwrap();
        let listed = f
            .office
            .task_note_list(
                &tess,
                TasksNotesListParams {
                    task: f.task_key.clone(),
                    author_id: Some(f.agent_a.clone()),
                },
            )
            .unwrap()
            .unwrap();
        assert!(listed.notes.is_empty(), "{engine:?}");
        let got = f
            .office
            .task_note_get(&tess, TasksNotesGetParams { note_id })
            .unwrap()
            .unwrap_err();
        assert_eq!(got.code, ErrorCode::NotFound, "{engine:?}");
    }
}
