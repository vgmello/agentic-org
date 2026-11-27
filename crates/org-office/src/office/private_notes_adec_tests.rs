//! `TASK-111` ADEC-1/2 through the office facade, on BOTH `Memory` and `Sqlite`: notes, plans
//! and decisions on and off tasks, titles, rationale, tags, patches, tombstones, cross-task
//! recall with compact fields, and `ADR-0031` isolation from every other agent, including the
//! team lead who later holds the task.

use org_proto::org::{ProjectCreateParams, TeamCreateParams, TeamSetLeadParams};
use org_proto::private_notes::{
    PrivateNoteAddParams, PrivateNoteField, PrivateNoteKind, PrivateNoteListParams,
    PrivateNoteUpdateParams, TasksNotesGetParams, TasksNotesListResult, TasksNotesPublishParams,
    TasksNotesRemoveParams,
};
use org_proto::tasks::{PatchField, TasksAssignParams, TasksCreateParams};
use org_proto::ErrorCode;

use crate::office::private_notes_tests::{
    dir_contains_bytes_excluding, next_uuid, seed_fixture, PrivateNoteFixture,
};
use crate::rules::OrgCommand;
use crate::store::open::EngineChoice;

const SECRET_WHY: &str = "SECRET_DECISION_RATIONALE_Q7";
const SECRET_ALTERNATIVES: &str = "SECRET_REJECTED_OPTION_Q7";

struct Adec {
    f: PrivateNoteFixture,
    project: String,
    project_task: String,
}

/// The shared fixture plus a project, its task assigned to agent A, and agent B as the lead of
/// the team that owns both.
fn adec_fixture(engine: EngineChoice) -> Adec {
    let f = seed_fixture(engine);
    let team = f.office.org_snapshot().unwrap().teams[0].clone();
    f.office
        .submit_org(
            &f.owner,
            OrgCommand::TeamSetLead(TeamSetLeadParams {
                command_id: next_uuid(),
                team_id: team.id.clone(),
                expected_version: team.version,
                lead_member_id: Some(f.agent_b.clone()),
            }),
        )
        .unwrap()
        .unwrap();
    let project = next_uuid();
    f.office
        .submit_org(
            &f.owner,
            OrgCommand::ProjectCreate(ProjectCreateParams {
                command_id: next_uuid(),
                project_id: project.clone(),
                name: "Decision Capture".into(),
                key: None,
                description: None,
                teams: vec![],
                general_area_id: String::new(),
                general_area_name: None,
                repo: None,
                integration_branch: None,
                slug: None,
                owner_team_id: Some(team.id.clone()),
                create_team_id: None,
                parent_team_id: None,
                contributing_team_ids: vec![],
            }),
        )
        .unwrap()
        .unwrap();
    let project_task = f
        .office
        .task_create(
            &f.owner,
            TasksCreateParams {
                command_id: next_uuid(),
                task_id: next_uuid(),
                team_id: Some(team.id),
                project_id: Some(project.clone()),
                assigned_team_id: None,
                assignee_id: Some(f.agent_a.clone()),
                task_type: Some("task".into()),
                work_type: None,
                title: "Pick the notes engine".into(),
                body_markdown: String::new(),
                priority: None,
                effort: None,
                severity: None,
                rank_order: Some(2),
                labels: vec![],
                reference_files: vec![],
                modified_files: vec![],
                depends_on: vec![],
                details: None,
                dod: None,
            },
        )
        .unwrap()
        .unwrap()
        .task
        .key;
    Adec {
        f,
        project,
        project_task,
    }
}

fn add(kind: PrivateNoteKind, task: Option<&str>, body: &str) -> PrivateNoteAddParams {
    PrivateNoteAddParams {
        command_id: next_uuid(),
        note_id: next_uuid(),
        task: task.map(str::to_owned),
        project_id: None,
        kind,
        title: None,
        why: None,
        alternatives: None,
        tags: Vec::new(),
        body_markdown: body.into(),
    }
}

fn update(note_id: &str, expected_revision: u64) -> PrivateNoteUpdateParams {
    PrivateNoteUpdateParams {
        command_id: next_uuid(),
        note_id: note_id.into(),
        expected_revision,
        kind: None,
        title: None,
        why: None,
        alternatives: None,
        tags: None,
        body_markdown: None,
    }
}

fn list(a: &Adec, actor: &str, params: PrivateNoteListParams) -> TasksNotesListResult {
    a.f.office.task_note_list(actor, params).unwrap().unwrap()
}

fn list_refusal(a: &Adec, actor: &str, params: PrivateNoteListParams) -> ErrorCode {
    a.f.office
        .task_note_list(actor, params)
        .unwrap()
        .unwrap_err()
        .code
}

fn ids(result: &TasksNotesListResult) -> Vec<&str> {
    result.notes.iter().map(|n| n.id.as_str()).collect()
}

/// IDs of agent A's decision (on the project task), plan (project only) and note (neither).
struct Seeded {
    decision: String,
    plan: String,
    note: String,
}

fn assert_creation_rules(a: &Adec) -> Seeded {
    let office = &a.f.office;
    let agent = a.f.agent_a.as_str();
    let mut decision = add(
        PrivateNoteKind::Decision,
        Some(&a.project_task),
        "## Use SQLite WAL for private notes\n\nDurable and isolated.",
    );
    decision.why = Some(format!("  {SECRET_WHY}  "));
    decision.alternatives = Some(SECRET_ALTERNATIVES.into());
    decision.tags = vec![" Arch ".into(), "db:SQLite".into(), "arch".into()];
    let created = office
        .task_note_add(agent, decision.clone())
        .unwrap()
        .unwrap()
        .note;
    assert_eq!(created.kind, PrivateNoteKind::Decision);
    assert_eq!(
        created.title.as_deref(),
        Some("Use SQLite WAL for private notes")
    );
    assert_eq!(created.why.as_deref(), Some(SECRET_WHY));
    assert_eq!(created.alternatives.as_deref(), Some(SECRET_ALTERNATIVES));
    assert_eq!(created.tags, ["arch", "db:sqlite"]);
    assert_eq!(created.project_id.as_deref(), Some(a.project.as_str()));
    assert!(created.task_id.is_some() && created.editable);
    // Exact replay answers the same note; the same command with other content is refused.
    let replay = office
        .task_note_add(agent, decision.clone())
        .unwrap()
        .unwrap();
    assert_eq!(replay.note, created);
    let mut reused = decision;
    reused.tags = vec!["other".into()];
    assert_eq!(
        office
            .task_note_add(agent, reused)
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::OperationReused
    );

    let mut plan = add(PrivateNoteKind::Plan, None, "Roll out behind a flag");
    plan.project_id = Some(a.project.clone());
    plan.title = Some("Rollout plan".into());
    let plan = office.task_note_add(agent, plan).unwrap().unwrap().note;
    assert_eq!(
        (plan.task_id.clone(), plan.title.as_deref()),
        (None, Some("Rollout plan"))
    );
    assert_eq!(plan.project_id.as_deref(), Some(a.project.as_str()));
    let note = office
        .task_note_add(agent, add(PrivateNoteKind::Note, None, "Loose thought"))
        .unwrap()
        .unwrap()
        .note;
    assert_eq!(
        (note.task_id.clone(), note.project_id.clone()),
        (None, None)
    );

    // Refusals: scope, tags, sizes.
    let refused = |params: PrivateNoteAddParams| {
        office
            .task_note_add(agent, params)
            .unwrap()
            .unwrap_err()
            .code
    };
    let mut wrong_project = add(PrivateNoteKind::Note, Some(&a.project_task), "x");
    wrong_project.project_id = Some(next_uuid());
    assert_eq!(refused(wrong_project), ErrorCode::InvalidRequest);
    let mut bad_project = add(PrivateNoteKind::Note, None, "x");
    bad_project.project_id = Some("not-a-uuid".into());
    assert_eq!(refused(bad_project), ErrorCode::InvalidRequest);
    let mut nine_tags = add(PrivateNoteKind::Note, None, "x");
    nine_tags.tags = (0..9).map(|n| format!("t{n}")).collect();
    assert_eq!(refused(nine_tags), ErrorCode::InvalidRequest);
    let mut bad_tag = add(PrivateNoteKind::Note, None, "x");
    bad_tag.tags = vec!["no spaces".into()];
    assert_eq!(refused(bad_tag), ErrorCode::InvalidRequest);
    let mut long_title = add(PrivateNoteKind::Decision, None, "x");
    long_title.title = Some("t".repeat(201));
    assert_eq!(refused(long_title), ErrorCode::TooLarge);
    assert_eq!(
        refused(add(PrivateNoteKind::Note, Some(&next_uuid()), "x")),
        ErrorCode::NotFound
    );
    Seeded {
        decision: created.id,
        plan: plan.id,
        note: note.id,
    }
}

fn assert_recall_filters_and_pages(a: &Adec, s: &Seeded) {
    let agent = a.f.agent_a.as_str();
    let all = list(a, agent, PrivateNoteListParams::default());
    assert_eq!(ids(&all), [&s.note, &s.plan, &s.decision]);
    assert_eq!((all.total, all.task_id.clone()), (3, None));

    let by_kind = list(
        a,
        agent,
        PrivateNoteListParams {
            kind: Some(PrivateNoteKind::Decision),
            ..Default::default()
        },
    );
    assert_eq!(ids(&by_kind), [&s.decision]);
    let by_tag = list(
        a,
        agent,
        PrivateNoteListParams {
            tag: Some("DB:SQLite".into()),
            ..Default::default()
        },
    );
    assert_eq!(ids(&by_tag), [&s.decision]);
    let by_project = list(
        a,
        agent,
        PrivateNoteListParams {
            project_id: Some(a.project.clone()),
            ..Default::default()
        },
    );
    assert_eq!(ids(&by_project), [&s.plan, &s.decision]);
    let by_task = list(
        a,
        agent,
        PrivateNoteListParams {
            task: Some(a.project_task.clone()),
            ..Default::default()
        },
    );
    assert_eq!(ids(&by_task), [&s.decision]);
    assert!(by_task.task_id.is_some());
    let future = list(
        a,
        agent,
        PrivateNoteListParams {
            since: Some("2999-01-01T00:00:00Z".into()),
            ..Default::default()
        },
    );
    assert_eq!(future.total, 0);

    // Compact recall: titles and tags without bodies or rationale.
    let compact = list(
        a,
        agent,
        PrivateNoteListParams {
            fields: vec![PrivateNoteField::Title, PrivateNoteField::Tags],
            ..Default::default()
        },
    );
    assert!(compact
        .notes
        .iter()
        .all(|n| n.body_markdown.is_none() && n.why.is_none()));
    assert!(compact.notes.iter().all(|n| n.title.is_some()));
    let compact_json = serde_json::to_string(&compact).unwrap();
    assert!(!compact_json.contains("body_markdown") && !compact_json.contains(SECRET_WHY));

    // Pages: one note at a time, then the cursor runs out.
    let mut seen = Vec::new();
    let mut cursor = None;
    loop {
        let page = list(
            a,
            agent,
            PrivateNoteListParams {
                limit: Some(1),
                cursor: cursor.clone(),
                ..Default::default()
            },
        );
        assert_eq!(page.total, 3);
        seen.extend(page.notes.into_iter().map(|n| n.id));
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(seen, [s.note.clone(), s.plan.clone(), s.decision.clone()]);

    for bad in [
        PrivateNoteListParams {
            cursor: Some("forged".into()),
            ..Default::default()
        },
        PrivateNoteListParams {
            limit: Some(0),
            ..Default::default()
        },
        PrivateNoteListParams {
            since: Some("last week".into()),
            ..Default::default()
        },
        PrivateNoteListParams {
            tag: Some("Bad Tag".into()),
            ..Default::default()
        },
        PrivateNoteListParams {
            author_id: Some("someone".into()),
            ..Default::default()
        },
    ] {
        assert_eq!(list_refusal(a, agent, bad), ErrorCode::InvalidRequest);
    }
}

fn assert_patches_publish_and_tombstones(a: &Adec, s: &Seeded) {
    let office = &a.f.office;
    let agent = a.f.agent_a.as_str();
    assert_eq!(
        office
            .task_note_update(agent, update(&s.note, 1))
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest,
        "an update that changes nothing is refused"
    );
    let mut promote = update(&s.note, 1);
    promote.kind = Some(PrivateNoteKind::Decision);
    promote.tags = Some(vec!["Recall".into()]);
    promote.why = Some("Agents forget by turn 40".into());
    let promoted = office
        .task_note_update(agent, promote)
        .unwrap()
        .unwrap()
        .note;
    assert_eq!(promoted.revision, 2);
    assert_eq!(promoted.kind, PrivateNoteKind::Decision);
    assert_eq!(promoted.title.as_deref(), Some("Loose thought"));
    assert_eq!(promoted.tags, ["recall"]);
    let mut clear = update(&s.note, 2);
    clear.why = Some(String::new());
    clear.tags = Some(Vec::new());
    clear.title = Some("Recall decisions".into());
    let cleared = office.task_note_update(agent, clear).unwrap().unwrap().note;
    assert_eq!((cleared.why.clone(), cleared.tags.len()), (None, 0));
    assert_eq!(cleared.title.as_deref(), Some("Recall decisions"));

    // Publishing needs a task.
    let publish = |note_id: &str, rev: u64| {
        office
            .task_note_publish(
                agent,
                TasksNotesPublishParams {
                    command_id: next_uuid(),
                    note_id: note_id.into(),
                    expected_revision: rev,
                    comment_id: next_uuid(),
                },
            )
            .unwrap()
    };
    assert_eq!(
        publish(&s.plan, 1).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    let (published, replayed) = publish(&s.decision, 1).unwrap();
    assert!(!replayed);
    assert!(published.comment.body_markdown.contains("Use SQLite WAL"));
    assert!(
        !published.comment.body_markdown.contains(SECRET_WHY),
        "publish copies the body only"
    );

    let removed = office
        .task_note_remove(
            agent,
            TasksNotesRemoveParams {
                command_id: next_uuid(),
                note_id: s.plan.clone(),
                expected_revision: 1,
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!((removed.revision, removed.task_id), (2, None));
    let gone = office
        .task_note_get(
            agent,
            TasksNotesGetParams {
                note_id: s.plan.clone(),
            },
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(gone.code, ErrorCode::NotFound);
    assert_eq!(list(a, agent, PrivateNoteListParams::default()).total, 2);
}

fn assert_adr_0031_isolation(a: &Adec, s: &Seeded) {
    let office = &a.f.office;
    let (agent, lead, owner) = (
        a.f.agent_a.as_str(),
        a.f.agent_b.as_str(),
        a.f.owner.as_str(),
    );
    // The task moves to the team lead: still nothing of agent A's is visible to it.
    let task = office.task_inspect(&a.project_task).unwrap().unwrap().task;
    office
        .task_assign(
            owner,
            TasksAssignParams {
                command_id: next_uuid(),
                task: a.project_task.clone(),
                expected_revision: task.revision,
                assignee_id: PatchField::Set(Some(lead.to_owned())),
                assigned_team_id: PatchField::Unchanged,
            },
        )
        .unwrap()
        .unwrap();
    for params in [
        PrivateNoteListParams::default(),
        PrivateNoteListParams {
            author_id: Some(agent.to_owned()),
            kind: Some(PrivateNoteKind::Decision),
            ..Default::default()
        },
        PrivateNoteListParams {
            task: Some(a.project_task.clone()),
            ..Default::default()
        },
        PrivateNoteListParams {
            project_id: Some(a.project.clone()),
            tag: Some("arch".into()),
            ..Default::default()
        },
    ] {
        let seen = list(a, lead, params);
        assert!(seen.notes.is_empty() && seen.total == 0 && seen.next_cursor.is_none());
    }
    let lead_get = office
        .task_note_get(
            lead,
            TasksNotesGetParams {
                note_id: s.decision.clone(),
            },
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(lead_get.code, ErrorCode::NotFound);
    assert_lead_touches_nothing(a, lead, &s.decision);

    // An active person reads every author's decisions but edits none.
    let person_view = list(
        a,
        owner,
        PrivateNoteListParams {
            author_id: Some(agent.to_owned()),
            kind: Some(PrivateNoteKind::Decision),
            ..Default::default()
        },
    );
    assert_eq!(ids(&person_view), [&s.decision]);
    assert!(person_view.notes.iter().all(|n| !n.editable));
    let mut person_edit = update(&s.decision, 1);
    person_edit.why = Some("overwrite".into());
    assert_eq!(
        office
            .task_note_update(owner, person_edit)
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );

    // A person's own decision stays out of every agent's view.
    let mine = office
        .task_note_add(owner, add(PrivateNoteKind::Decision, None, "Hire later"))
        .unwrap()
        .unwrap()
        .note;
    let agent_view = list(
        a,
        agent,
        PrivateNoteListParams {
            author_id: Some(owner.to_owned()),
            ..Default::default()
        },
    );
    assert_eq!(agent_view.total, 0);
    assert_eq!(
        office
            .task_note_get(agent, TasksNotesGetParams { note_id: mine.id })
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );

    // No private text reaches Debug output or any shared durable file.
    let rendered = format!("{person_view:?}");
    assert!(!rendered.contains(SECRET_WHY) && !rendered.contains(SECRET_ALTERNATIVES));
    let private_dir = a.f.roots.durable.join("private");
    for secret in [SECRET_WHY, SECRET_ALTERNATIVES] {
        assert!(
            !dir_contains_bytes_excluding(&a.f.roots.durable, &private_dir, secret.as_bytes()),
            "{secret} leaked outside durable/private"
        );
    }
    let task_view = office.task_inspect(&a.project_task).unwrap().unwrap();
    assert!(!format!("{task_view:?}").contains(SECRET_WHY));
}

/// `ADR-0031`: a lead of the note's team, or of any team above it, learns nothing of an
/// agent's note and changes nothing: reading, editing, removing and publishing it all answer
/// `NotFound`, and the author still holds it at revision 1.
fn assert_lead_touches_nothing(a: &Adec, lead: &str, note_id: &str) {
    let office = &a.f.office;
    let get = office.task_note_get(
        lead,
        TasksNotesGetParams {
            note_id: note_id.to_owned(),
        },
    );
    assert_eq!(get.unwrap().unwrap_err().code, ErrorCode::NotFound);
    let mut edit = update(note_id, 1);
    edit.title = Some("Hijacked".into());
    let edited = office.task_note_update(lead, edit).unwrap();
    assert_eq!(edited.unwrap_err().code, ErrorCode::NotFound);
    let removed = office.task_note_remove(
        lead,
        TasksNotesRemoveParams {
            command_id: next_uuid(),
            note_id: note_id.to_owned(),
            expected_revision: 1,
        },
    );
    assert_eq!(removed.unwrap().unwrap_err().code, ErrorCode::NotFound);
    let published = office.task_note_publish(
        lead,
        TasksNotesPublishParams {
            command_id: next_uuid(),
            note_id: note_id.to_owned(),
            expected_revision: 1,
            comment_id: next_uuid(),
        },
    );
    assert_eq!(published.unwrap().unwrap_err().code, ErrorCode::NotFound);
    let held = office.task_note_get(
        &a.f.agent_a,
        TasksNotesGetParams {
            note_id: note_id.to_owned(),
        },
    );
    assert_eq!(held.unwrap().unwrap().note.revision, 1);
}

/// Agent B leads the top team; a decision agent A records on a task of a team below it stays
/// out of agent B's reach exactly like one on agent B's own team.
fn assert_ancestor_lead_touches_nothing(a: &Adec) {
    let office = &a.f.office;
    let top = office.org_snapshot().unwrap().teams[0].id.clone();
    let child = next_uuid();
    office
        .submit_org(
            &a.f.owner,
            OrgCommand::TeamCreate(TeamCreateParams {
                command_id: next_uuid(),
                team_id: child.clone(),
                name: "Delivery".into(),
                owner_team_id: Some(top),
            }),
        )
        .unwrap()
        .unwrap();
    let task = office
        .task_create(
            &a.f.owner,
            TasksCreateParams {
                command_id: next_uuid(),
                task_id: next_uuid(),
                team_id: Some(child),
                project_id: None,
                assigned_team_id: None,
                assignee_id: None,
                task_type: Some("task".into()),
                work_type: None,
                title: "Ship the delivery team's decision".into(),
                body_markdown: String::new(),
                priority: None,
                effort: None,
                severity: None,
                rank_order: Some(3),
                labels: vec![],
                reference_files: vec![],
                modified_files: vec![],
                depends_on: vec![],
                details: None,
                dod: None,
            },
        )
        .unwrap()
        .unwrap()
        .task
        .key;
    let decision = office
        .task_note_add(
            &a.f.agent_a,
            add(
                PrivateNoteKind::Decision,
                Some(&task),
                "Use the child queue",
            ),
        )
        .unwrap()
        .unwrap()
        .note;
    assert_lead_touches_nothing(a, &a.f.agent_b, &decision.id);
    // The author's own publish of the same note goes through: the refusals were not a fault.
    let published = office
        .task_note_publish(
            &a.f.agent_a,
            TasksNotesPublishParams {
                command_id: next_uuid(),
                note_id: decision.id,
                expected_revision: 1,
                comment_id: next_uuid(),
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(published.0.comment.body_markdown, "Use the child queue");
}

#[test]
fn team_and_ancestor_leads_never_remove_or_publish_an_agents_decision() {
    for engine in [EngineChoice::Memory, EngineChoice::Sqlite] {
        let a = adec_fixture(engine);
        assert_ancestor_lead_touches_nothing(&a);
    }
}

/// A satellite holds no private notes (`ARCH-02`): every `tasks.notes.*` call is refused with
/// `Unavailable` before any caller lookup or validation, so the request goes to HQ instead.
#[test]
fn a_satellite_refuses_every_private_note_call_as_unavailable() {
    let satellite = crate::office::TestSatellite::new();
    let office = &satellite.office;
    let actor = next_uuid();
    let note_id = next_uuid();
    let code = |refusal: Result<Result<(), org_proto::ApiError>, crate::error::OfficeError>| {
        refusal.unwrap().unwrap_err().code
    };
    let mut edit = update(&note_id, 1);
    edit.title = Some("Edited".into());
    let codes = [
        code(
            office
                .task_note_add(&actor, add(PrivateNoteKind::Decision, None, "Body"))
                .map(|r| r.map(drop)),
        ),
        code(
            office
                .task_note_list(&actor, PrivateNoteListParams::default())
                .map(|r| r.map(drop)),
        ),
        code(
            office
                .task_note_get(
                    &actor,
                    TasksNotesGetParams {
                        note_id: note_id.clone(),
                    },
                )
                .map(|r| r.map(drop)),
        ),
        code(office.task_note_update(&actor, edit).map(|r| r.map(drop))),
        code(
            office
                .task_note_remove(
                    &actor,
                    TasksNotesRemoveParams {
                        command_id: next_uuid(),
                        note_id: note_id.clone(),
                        expected_revision: 1,
                    },
                )
                .map(|r| r.map(drop)),
        ),
        code(
            office
                .task_note_publish(
                    &actor,
                    TasksNotesPublishParams {
                        command_id: next_uuid(),
                        note_id,
                        expected_revision: 1,
                        comment_id: next_uuid(),
                    },
                )
                .map(|r| r.map(drop)),
        ),
    ];
    assert_eq!(codes, [ErrorCode::Unavailable; 6]);
    assert!(
        !office.durable.join("private/task-notes.db").exists(),
        "a satellite never creates the private notes database"
    );
}

#[test]
fn adec_decisions_recall_and_isolation_on_memory_and_sqlite() {
    for engine in [EngineChoice::Memory, EngineChoice::Sqlite] {
        let a = adec_fixture(engine);
        let seeded = assert_creation_rules(&a);
        assert_recall_filters_and_pages(&a, &seeded);
        assert_adr_0031_isolation(&a, &seeded);
        assert_patches_publish_and_tombstones(&a, &seeded);
    }
}
