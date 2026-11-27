//! `TASK-88` conformance on BOTH engines, and after deleting the SQLite projection: checklist
//! sections with per-section positions and counts, copy-on-create definitions of done with
//! organization/project inheritance and opt-out, the `checklist_incomplete` closing gate with
//! its "done anyway" record, and the attributed final summary.

use org_proto::tasks::{
    ChecklistSection, TaskInspectResult, TaskStatus, TaskView, TasksChecklistAddParams,
    TasksChecklistCheckParams, TasksChecklistMoveParams, TasksChecklistRemoveParams,
    TasksCommentParams, TasksCreateParams, TasksFinalSummaryParams, TasksListParams,
    TasksTransitionParams, TasksUpdateParams, TasksWatchParams, TASK_FINAL_SUMMARY_MAX_CHARS,
};
use org_proto::{ApiError, ErrorCode};
use org_store::{Ack, Change, Id, Refusal, Reply, StoreError, Stream};

use org_office::organization_family::ports::OrganizationRead;
use org_office::projects::ports::ProjectsRead;

use org_office::organization_family::features::{
    AddPerson, CreateTeam, SetOrganizationDefaultDod, UpdateTeam, NOT_AUTHORIZED,
};
use org_office::projects::features::{
    CreateProject, OwnerTeamChoice, SetProjectDefaultDod, PROJECT_DOD_DENIED,
};
use org_office::tasks::features::create::dod_step_id;
use org_office::tasks::ops::TaskWriteAuthority;
use org_office::testkit_support::store_open::task_ops::{
    TaskChecklistAddOp, TaskChecklistCheckOp, TaskChecklistMoveOp, TaskChecklistRemoveOp,
    TaskCommentOp, TaskCreateOp, TaskFinalSummaryOp, TaskInspectRead, TaskListRead,
    TaskTransitionOp, TaskUpdateOp, TaskWatchOp,
};
use org_office::testkit_support::store_open::{
    AddPersonOp, CreateOrganizationOp, CreateProjectOp, CreateTeamOp, OrganizationDefaultDodRead,
    ProjectInspectByRef, SetOrganizationDefaultDodOp, SetProjectDefaultDodOp, UpdateTeamOp,
};

use super::*;

fn uuid() -> String {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(90_000);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let ms = CREATION_AT.0;
    let mut b = [0u8; 16];
    b[0..6].copy_from_slice(&ms.to_be_bytes()[2..8]);
    b[6] = 0x70 | ((seq >> 8) as u8 & 0x0f);
    b[7] = (seq & 0xff) as u8;
    b[8] = 0x80;
    b[13] = ((seq >> 16) & 0xff) as u8;
    b[14] = ((seq >> 8) & 0xff) as u8;
    b[15] = (seq & 0xff) as u8;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13],
        b[14], b[15]
    )
}

/// An organization with `TEAM-A > TEAM-B > TEAM-C`; Bob is in B and leads it, Carol is in C
/// and leads nothing; project P is owned by C.
struct Org {
    place: Place,
    store: OfficeStore,
    here: org_store::OfficeIdentity,
    owner: Id,
    bob: Id,
    carol: Id,
    team_a: Id,
    project: Id,
}

impl Org {
    fn new(engine: EngineChoice) -> Self {
        let place = Place::new();
        let store = place.create(engine.clone());
        let here = identity();
        let cmd = creation_command();
        let owner = cmd.creator.employee_id.clone();
        let team_a = cmd.team_id.clone();
        let create = CreateOrganizationOp {
            operation_id: id(1),
            here: here.clone(),
            command: cmd,
            ca: ca_material(&here.organization_id, 1),
        };
        assert_eq!(store.run(create).unwrap().outcome, Outcome::Completed);
        let by = Actor::Employee(owner.clone());
        let (team_b, team_c) = (id(301), id(302));
        for (op, team, parent) in [(2, &team_b, &team_a), (3, &team_c, &team_b)] {
            let reply = store.run(CreateTeamOp {
                operation_id: id(op),
                here: here.clone(),
                owner: owner.clone(),
                by: by.clone(),
                command: CreateTeam {
                    id: team.clone(),
                    name: format!("Team {op}"),
                    parent: Some(parent.clone()),
                },
            });
            assert_eq!(reply.unwrap().outcome, Outcome::Completed);
        }
        let (bob, carol) = (id(401), id(402));
        for (op, person, handle, team) in [(4, &bob, "bob", &team_b), (5, &carol, "carol", &team_c)]
        {
            let reply = store.run(AddPersonOp {
                operation_id: id(op),
                here: here.clone(),
                owner: owner.clone(),
                by: by.clone(),
                command: AddPerson::new(person.clone(), handle, handle, team.clone()),
            });
            assert_eq!(reply.unwrap().outcome, Outcome::Completed);
        }
        let mut lead = UpdateTeam::new(team_b.clone(), 1);
        lead.lead = Some(bob.clone());
        let reply = store.run(UpdateTeamOp {
            operation_id: id(6),
            here: here.clone(),
            owner: owner.clone(),
            by: by.clone(),
            command: lead,
        });
        assert_eq!(reply.unwrap().outcome, Outcome::Completed);
        let project = id(501);
        let reply = store.run(CreateProjectOp {
            operation_id: id(7),
            here: here.clone(),
            owner: owner.clone(),
            by,
            command: CreateProject {
                id: project.clone(),
                name: "Billing".into(),
                slug: None,
                owner_team: OwnerTeamChoice::Existing(team_c),
                contributing_team_ids: Vec::new(),
            },
        });
        assert_eq!(reply.unwrap().outcome, Outcome::Completed);
        Self {
            place,
            store,
            here,
            owner,
            bob,
            carol,
            team_a,
            project,
        }
    }

    fn create(&self, project: bool, dod: Option<Vec<&str>>) -> TaskView {
        self.try_create(&uuid(), &uuid(), project, dod)
            .expect("created")
    }

    fn try_create(
        &self,
        command: &str,
        task: &str,
        project: bool,
        dod: Option<Vec<&str>>,
    ) -> Result<TaskView, ApiError> {
        let out = self.store.run(TaskCreateOp {
            actor: self.owner.as_str().to_owned(),
            here: self.here.clone(),
            numbering: self.store.task_numbering(),
            params: TasksCreateParams {
                command_id: command.to_owned(),
                task_id: task.to_owned(),
                team_id: (!project).then(|| self.team_a.as_str().to_owned()),
                project_id: project.then(|| self.project.as_str().to_owned()),
                assigned_team_id: None,
                assignee_id: None,
                task_type: None,
                work_type: None,
                title: "Ship the ledger".into(),
                body_markdown: String::new(),
                priority: None,
                effort: None,
                severity: None,
                rank_order: None,
                labels: Vec::new(),
                reference_files: Vec::new(),
                modified_files: Vec::new(),
                depends_on: Vec::new(),
                details: None,
                dod: dod.map(|items| items.into_iter().map(str::to_owned).collect()),
                attachment_ids: Vec::new(),
            },
            prepared_files: Default::default(),
        });
        out.unwrap().map(|o| o.task)
    }

    fn inspect(&self, task: &str) -> TaskInspectResult {
        let (out, _) = self
            .store
            .read(TaskInspectRead {
                task: task.to_owned(),
                fields: None,
            })
            .unwrap();
        out.expect("inspected")
    }

    fn texts(&self, task: &str, section: ChecklistSection) -> Vec<(String, u64)> {
        self.inspect(task)
            .checklist
            .into_iter()
            .filter(|s| s.section == section)
            .map(|s| (s.text, s.position))
            .collect()
    }

    fn org_default(&self, by: &Id, items: Option<Vec<&str>>) -> Result<Reply, StoreError> {
        let (current, _) = self.store.read(OrganizationDefaultDodRead).unwrap();
        let (_, revision) = current.expect("an organization");
        self.store.run(SetOrganizationDefaultDodOp {
            operation_id: Id::parse(&uuid()).unwrap(),
            here: self.here.clone(),
            owner: self.owner.clone(),
            by: Actor::Employee(by.clone()),
            command: SetOrganizationDefaultDod {
                expected_revision: revision,
                default_dod: items.map(|i| i.into_iter().map(str::to_owned).collect()),
            },
        })
    }

    fn project_default(&self, by: &Id, items: Option<Vec<&str>>) -> Result<Reply, StoreError> {
        let (project, _) = self
            .store
            .read(ProjectInspectByRef(self.project.as_str().to_owned()))
            .unwrap();
        self.store.run(SetProjectDefaultDodOp {
            operation_id: Id::parse(&uuid()).unwrap(),
            here: self.here.clone(),
            owner: self.owner.clone(),
            by: Actor::Employee(by.clone()),
            command: SetProjectDefaultDod {
                project_id: self.project.clone(),
                expected_revision: project.expect("the project").meta.revision,
                default_dod: items.map(|i| i.into_iter().map(str::to_owned).collect()),
            },
        })
    }

    fn add(&self, task: &str, text: &str, section: Option<ChecklistSection>) -> String {
        self.add_required(task, text, section, None)
    }

    fn add_required(
        &self,
        task: &str,
        text: &str,
        section: Option<ChecklistSection>,
        required: Option<bool>,
    ) -> String {
        let step = uuid();
        self.store
            .run(TaskChecklistAddOp {
                actor: self.owner.as_str().to_owned(),
                params: TasksChecklistAddParams {
                    command_id: uuid(),
                    task: task.to_owned(),
                    step_id: step.clone(),
                    text: text.to_owned(),
                    required,
                    section,
                    steps: vec![],
                },
                authority: TaskWriteAuthority::Unrestricted,
            })
            .unwrap()
            .expect("step added");
        step
    }

    fn check(&self, task: &str, step: &str) {
        self.store
            .run(TaskChecklistCheckOp {
                actor: self.owner.as_str().to_owned(),
                params: TasksChecklistCheckParams {
                    command_id: uuid(),
                    task: task.to_owned(),
                    step_id: step.to_owned(),
                    section: None,
                    indices: vec![],
                    expected_revision: None,
                    done: true,
                },
                authority: TaskWriteAuthority::Unrestricted,
            })
            .unwrap()
            .expect("step checked");
    }

    fn mv(&self, task: &str, step: &str, position: u64, section: Option<ChecklistSection>) {
        self.store
            .run(TaskChecklistMoveOp {
                actor: self.owner.as_str().to_owned(),
                params: TasksChecklistMoveParams {
                    command_id: uuid(),
                    task: task.to_owned(),
                    step_id: step.to_owned(),
                    from_section: None,
                    index: None,
                    expected_revision: None,
                    position,
                    section,
                },
                authority: TaskWriteAuthority::Unrestricted,
            })
            .unwrap()
            .expect("step moved");
    }

    fn transition(
        &self,
        actor: &Id,
        task: &str,
        change: Transition<'_>,
    ) -> Result<TaskView, ApiError> {
        let revision = self.inspect(task).task.revision;
        self.store
            .run(TaskTransitionOp {
                actor: actor.as_str().to_owned(),
                params: TasksTransitionParams {
                    command_id: uuid(),
                    task: task.to_owned(),
                    expected_revision: revision,
                    status: change.status,
                    reason: None,
                    blocked_reason: None,
                    waiting_on_owner: None,
                    resolution: change.resolution.map(str::to_owned),
                    note: None,
                    done_anyway_reason: change.reason.map(str::to_owned),
                    final_summary: change.summary.map(str::to_owned),
                },
                authority: TaskWriteAuthority::Unrestricted,
            })
            .unwrap()
            .map(|o| o.task)
    }

    fn summarize(
        &self,
        actor: &Id,
        task: &str,
        text: &str,
        command: &str,
        revision: u64,
    ) -> Result<TaskView, ApiError> {
        self.store
            .run(TaskFinalSummaryOp {
                actor: actor.as_str().to_owned(),
                params: TasksFinalSummaryParams {
                    command_id: command.to_owned(),
                    task: task.to_owned(),
                    expected_revision: revision,
                    final_summary: text.to_owned(),
                },
                authority: TaskWriteAuthority::Unrestricted,
            })
            .unwrap()
            .map(|o| o.task)
    }

    /// Closes the store, deletes the SQLite projection, and reopens: a rebuild from files.
    fn rebuilt(self) -> Self {
        let db = self
            .place
            .runtime()
            .join(self.here.office_id.as_str())
            .join("office.db");
        self.store.close().expect("close the store");
        std::fs::remove_file(&db).expect("delete the projection");
        let store = self
            .place
            .reopen(EngineChoice::Sqlite(org_office_sqlite::sqlite_backend()));
        Self { store, ..self }
    }
}

struct Transition<'a> {
    status: TaskStatus,
    resolution: Option<&'a str>,
    reason: Option<&'a str>,
    summary: Option<&'a str>,
}

/// Completing, with nothing else said.
fn done<'a>() -> Transition<'a> {
    Transition {
        status: TaskStatus::Done,
        resolution: None,
        reason: None,
        summary: None,
    }
}

fn pairs(items: &[(&str, u64)]) -> Vec<(String, u64)> {
    items.iter().map(|(t, p)| ((*t).to_owned(), *p)).collect()
}

fn completed(reply: Result<Reply, StoreError>) {
    assert_eq!(reply.expect("answered").outcome, Outcome::Completed);
}

fn refused_unrecorded(reply: Result<Reply, StoreError>, reason: &str) {
    match reply {
        Err(StoreError::Invalid(why)) => assert_eq!(why, reason),
        other => panic!("expected an unrecorded refusal, got {other:?}"),
    }
}

fn code(result: Result<TaskView, ApiError>) -> ErrorCode {
    result.expect_err("refused").code
}

fn sections(s: &Org, task: &str) {
    use ChecklistSection::{Acceptance, Dod, Work};
    let ac1 = s.add(task, "AC one", Some(ChecklistSection::Acceptance));
    let work1 = s.add(task, "Work one", None);
    let dod1 = s.add(task, "Docs", Some(ChecklistSection::Dod));
    let ac2 = s.add(task, "AC two", Some(ChecklistSection::Acceptance));
    let work2 = s.add_required(task, "Work two", None, Some(true));
    let inspected = s.inspect(task);
    let order: Vec<(ChecklistSection, u64, bool)> = inspected
        .checklist
        .iter()
        .map(|st| (st.section, st.position, st.required))
        .collect();
    assert_eq!(
        order,
        [
            (Acceptance, 0, true),
            (Acceptance, 1, true),
            (Dod, 0, true),
            (Work, 0, false),
            (Work, 1, true),
        ]
    );
    let view = &inspected.task;
    assert_eq!(
        (view.ac_total, view.dod_total, view.work_steps_total),
        (2, 1, 2)
    );
    assert_eq!(view.checklist_total, 5);
    s.check(task, &ac1);
    s.check(task, &work1);
    let view = s.inspect(task).task;
    assert_eq!(
        (view.ac_done, view.dod_done, view.work_steps_done),
        (1, 0, 1)
    );
    assert_eq!(view.checklist_done, 2);

    // Across sections, then within one: each section stays 0-based and contiguous.
    s.mv(task, &work2, 0, Some(Acceptance));
    s.mv(task, &ac2, 0, None);
    s.mv(task, &dod1, 9, Some(Work));
    assert_eq!(
        s.texts(task, Acceptance),
        pairs(&[("AC two", 0), ("Work two", 1), ("AC one", 2)])
    );
    assert!(s.texts(task, Dod).is_empty());
    assert_eq!(s.texts(task, Work), pairs(&[("Work one", 0), ("Docs", 1)]));
    s.mv(task, &dod1, 0, Some(Dod));
    s.store
        .run(TaskChecklistRemoveOp {
            actor: s.owner.as_str().to_owned(),
            params: TasksChecklistRemoveParams {
                command_id: uuid(),
                task: task.to_owned(),
                step_id: ac2,
                section: None,
                index: None,
                expected_revision: None,
            },
            authority: TaskWriteAuthority::Unrestricted,
        })
        .unwrap()
        .expect("removed");
    assert_eq!(
        s.texts(task, Acceptance),
        pairs(&[("Work two", 0), ("AC one", 1)])
    );
    // The list answers the same counts as the inspect.
    let (listed, _) = s
        .store
        .read(TaskListRead {
            params: TasksListParams::default(),
        })
        .unwrap();
    let listed = listed.expect("listed");
    let row = listed
        .tasks
        .iter()
        .find(|t| t.key == task)
        .expect("listed task");
    assert_eq!(
        (
            row.ac_total,
            row.ac_done,
            row.dod_total,
            row.work_steps_total,
            row.work_steps_done
        ),
        (2, 1, 1, 1, 1)
    );
    assert_eq!(row, &s.inspect(task).task);
}

#[test]
fn checklist_sections_keep_their_own_positions_and_counts_on_both_engines_and_rebuild() {
    for engine in engines() {
        let s = Org::new(engine.clone());
        let task = s.create(false, None);
        assert_eq!(
            (task.ac_total, task.dod_total, task.work_steps_total),
            (0, 0, 0)
        );
        sections(&s, &task.key);
        if engine.is_sqlite() {
            let before = s.inspect(&task.key);
            let s = s.rebuilt();
            assert_eq!(s.inspect(&task.key).checklist, before.checklist);
            assert_eq!(s.inspect(&task.key).task, before.task);
        }
    }
}

#[test]
fn a_task_copies_its_definition_of_done_at_create_and_defaults_never_change_it_later() {
    for engine in engines() {
        let s = Org::new(engine.clone());
        // No default anywhere: no `dod` steps.
        let bare = s.create(false, None);
        assert_eq!(bare.dod_total, 0);

        completed(s.org_default(&s.owner, Some(vec!["Tests pass", "Docs updated"])));
        let (org, _) = s.store.read(OrganizationDefaultDodRead).unwrap();
        assert_eq!(
            org.unwrap().0,
            Some(vec!["Tests pass".to_owned(), "Docs updated".to_owned()])
        );
        // Only the owner sets the organization's default.
        refused_unrecorded(s.org_default(&s.bob, Some(vec!["x"])), NOT_AUTHORIZED);

        // A projectless task, and a project task whose project inherits, copy the org's.
        let (command, task_id) = (uuid(), uuid());
        let inherited = s.try_create(&command, &task_id, false, None).unwrap();
        let in_project = s.create(true, None);
        for task in [&inherited, &in_project] {
            assert_eq!(task.dod_total, 2);
            assert_eq!(
                s.texts(&task.key, ChecklistSection::Dod),
                pairs(&[("Tests pass", 0), ("Docs updated", 1)])
            );
            assert!(s
                .inspect(&task.key)
                .checklist
                .iter()
                .all(|st| st.required && !st.done));
        }
        // Step IDs derive from the task: a retry of the create neither fails nor duplicates.
        let ids: Vec<String> = s
            .inspect(&inherited.key)
            .checklist
            .iter()
            .map(|st| st.id.clone())
            .collect();
        let task = Id::parse(&task_id).unwrap();
        assert_eq!(
            ids,
            [
                dod_step_id(&task, 0).as_str(),
                dod_step_id(&task, 1).as_str()
            ]
        );
        let retried = s.try_create(&command, &task_id, false, None).unwrap();
        assert_eq!(retried.key, inherited.key);
        assert_eq!(s.inspect(&inherited.key).checklist.len(), 2);
        // The create's own history records what it copied.
        let created = &s.inspect(&inherited.key).history[0].change;
        assert_eq!(created["dod"][1]["text"], "Docs updated");

        // A project override, by a lead above the owning team; a non-lead is refused.
        completed(s.project_default(&s.bob, Some(vec!["Deployed"])));
        refused_unrecorded(
            s.project_default(&s.carol, Some(vec!["y"])),
            PROJECT_DOD_DENIED,
        );
        let overridden = s.create(true, None);
        assert_eq!(
            s.texts(&overridden.key, ChecklistSection::Dod),
            pairs(&[("Deployed", 0)])
        );
        // `[]` opts the project out; the owner may set it too.
        completed(s.project_default(&s.owner, Some(vec![])));
        let opted_out = s.create(true, None);
        assert_eq!(opted_out.dod_total, 0);
        // `None` inherits the organization's again.
        completed(s.project_default(&s.owner, None));
        assert_eq!(s.create(true, None).dod_total, 2);

        // The task's own list wins over every default, `[]` included.
        let explicit = s.create(true, Some(vec!["  Custom check  "]));
        assert_eq!(
            s.texts(&explicit.key, ChecklistSection::Dod),
            pairs(&[("Custom check", 0)])
        );
        assert_eq!(s.create(false, Some(vec![])).dod_total, 0);
        let bad = s.try_create(&uuid(), &uuid(), false, Some(vec!["two\nlines"]));
        assert_eq!(bad.unwrap_err().code, ErrorCode::InvalidRequest);
        let many: Vec<String> = (0..26).map(|i| format!("item {i}")).collect();
        let many = s.try_create(
            &uuid(),
            &uuid(),
            false,
            Some(many.iter().map(String::as_str).collect()),
        );
        assert_eq!(many.unwrap_err().code, ErrorCode::TooLarge);

        // Later default changes never touch a task already created.
        completed(s.org_default(&s.owner, Some(vec!["Brand new"])));
        assert_eq!(
            s.texts(&inherited.key, ChecklistSection::Dod),
            pairs(&[("Tests pass", 0), ("Docs updated", 1)])
        );
        assert_eq!(
            s.texts(&overridden.key, ChecklistSection::Dod),
            pairs(&[("Deployed", 0)])
        );
        assert_eq!(s.create(false, None).dod_total, 1);

        if engine.is_sqlite() {
            let s = s.rebuilt();
            let (org, _) = s.store.read(OrganizationDefaultDodRead).unwrap();
            assert_eq!(org.unwrap().0, Some(vec!["Brand new".to_owned()]));
            let (project, _) = s
                .store
                .read(ProjectInspectByRef(s.project.as_str().to_owned()))
                .unwrap();
            assert_eq!(project.unwrap().default_dod, None);
            assert_eq!(s.inspect(&inherited.key).checklist.len(), 2);
            assert_eq!(
                s.texts(&overridden.key, ChecklistSection::Dod),
                pairs(&[("Deployed", 0)])
            );
        }
    }
}

#[test]
fn completing_with_required_steps_unticked_needs_a_reason_that_reopening_clears() {
    for engine in engines() {
        let s = Org::new(engine.clone());
        let task = s.create(false, Some(vec!["Tests pass"])).key;
        let ac = s.add(&task, "Login works", Some(ChecklistSection::Acceptance));
        let _optional = s.add(&task, "Refactor later", None);

        for resolution in [None, Some("fixed"), Some("Completed")] {
            let refused = s.transition(
                &s.owner,
                &task,
                Transition {
                    resolution,
                    ..done()
                },
            );
            let error = refused.unwrap_err();
            assert_eq!(error.code, ErrorCode::ChecklistIncomplete, "{resolution:?}");
            assert!(
                error.message.contains("[acceptance] Login works"),
                "{}",
                error.message
            );
            assert!(
                error.message.contains("[dod] Tests pass"),
                "{}",
                error.message
            );
        }
        for (reason, status, expect) in [
            ("   ", TaskStatus::Done, ErrorCode::InvalidRequest),
            ("why", TaskStatus::InProgress, ErrorCode::InvalidRequest),
        ] {
            let change = Transition {
                status,
                reason: Some(reason),
                ..done()
            };
            assert_eq!(code(s.transition(&s.owner, &task, change)), expect);
        }
        let long = "r".repeat(1001);
        let change = Transition {
            reason: Some(&long),
            ..done()
        };
        assert_eq!(
            code(s.transition(&s.owner, &task, change)),
            ErrorCode::TooLarge
        );
        assert_eq!(s.inspect(&task).task.status, TaskStatus::Todo);

        let change = Transition {
            reason: Some("  Waived by the owner  "),
            ..done()
        };
        let closed = s.transition(&s.bob, &task, change).expect("done anyway");
        let record = closed
            .details
            .and_then(|d| d.done_anyway)
            .expect("recorded");
        assert_eq!(record.reason, "Waived by the owner");
        let unticked: Vec<String> = s
            .inspect(&task)
            .checklist
            .iter()
            .filter(|st| st.required && !st.done)
            .map(|st| st.id.clone())
            .collect();
        assert_eq!(record.steps, unticked);
        assert!(record.steps.contains(&ac));
        let event = s.inspect(&task).history.last().unwrap().change.clone();
        assert_eq!(event["type"], "status_changed");
        assert_eq!(event["done_anyway"]["reason"], "Waived by the owner");
        assert_eq!(event["done_anyway"]["steps"].as_array().unwrap().len(), 2);

        let s = if engine.is_sqlite() {
            let s = s.rebuilt();
            let kept = s.inspect(&task).task.details.and_then(|d| d.done_anyway);
            assert_eq!(
                kept.map(|d| d.reason).as_deref(),
                Some("Waived by the owner")
            );
            s
        } else {
            s
        };
        reopen_and_tick(&s, &task);

        // Cancelling, and closing as anything but completed or fixed, is never gated.
        for (status, resolution) in [
            (TaskStatus::Cancelled, None),
            (TaskStatus::Done, Some("wont_do")),
        ] {
            let other = s.create(false, Some(vec!["Never ticked"])).key;
            let view = s
                .transition(
                    &s.owner,
                    &other,
                    Transition {
                        status,
                        resolution,
                        ..done()
                    },
                )
                .expect("not gated");
            assert!(view.details.and_then(|d| d.done_anyway).is_none());
        }
    }
}

/// Reopening clears the record; ticking every required step completes without a reason.
fn reopen_and_tick(s: &Org, task: &str) {
    let reopened = s
        .transition(
            &s.owner,
            task,
            Transition {
                status: TaskStatus::Todo,
                ..done()
            },
        )
        .expect("reopened");
    let details = reopened.details.expect("details");
    assert!(details.done_anyway.is_none());
    assert_eq!(details.reopen_count, 1);
    for step in s.inspect(task).checklist {
        if step.required {
            s.check(task, &step.id);
        }
    }
    let view = s.transition(&s.owner, task, done()).expect("complete");
    assert_eq!(view.status, TaskStatus::Done);
    assert!(view.details.and_then(|d| d.done_anyway).is_none());
}

#[test]
fn the_final_summary_is_attributed_and_only_a_close_or_its_own_call_sets_it() {
    for engine in engines() {
        let s = Org::new(engine.clone());
        let task = s.create(false, None).key;
        let open = Transition {
            status: TaskStatus::InProgress,
            summary: Some("early"),
            ..done()
        };
        assert_eq!(
            code(s.transition(&s.owner, &task, open)),
            ErrorCode::InvalidRequest
        );
        // Standalone `tasks.final_summary` also refuses open tasks.
        let rev0 = s.inspect(&task).task.revision;
        assert_eq!(
            code(s.summarize(&s.owner, &task, "too early", &uuid(), rev0)),
            ErrorCode::InvalidRequest
        );
        let blank = Transition {
            summary: Some("  "),
            ..done()
        };
        assert_eq!(
            code(s.transition(&s.owner, &task, blank)),
            ErrorCode::InvalidRequest
        );
        for bad in ["\0hello", "bad\x1b[31m"] {
            let ctrl = Transition {
                summary: Some(bad),
                ..done()
            };
            assert_eq!(
                code(s.transition(&s.owner, &task, ctrl)),
                ErrorCode::InvalidRequest
            );
        }

        let change = Transition {
            summary: Some(" Shipped the ledger. "),
            ..done()
        };
        let closed = s.transition(&s.bob, &task, change).expect("closed");
        let summary = closed.final_summary.clone().expect("summary");
        assert_eq!(summary.text, "Shipped the ledger.");
        assert_eq!(summary.by_kind, "employee");
        assert_eq!(summary.by_id, s.bob.as_str());
        assert_eq!(Some(summary.at.clone()), closed.updated_at);
        let event = s.inspect(&task).history.last().unwrap().change.clone();
        assert_eq!(event["final_summary"], "Shipped the ledger.");

        // Comments, watches, checklist changes and other edits leave it as it is.
        s.store
            .run(TaskCommentOp {
                actor: s.carol.as_str().to_owned(),
                params: TasksCommentParams {
                    command_id: uuid(),
                    task: task.clone(),
                    comment_id: uuid(),
                    body_markdown: "Nice work".into(),
                    attachment_ids: vec![],
                },
                prepared_files: Default::default(),
                authority: TaskWriteAuthority::Unrestricted,
            })
            .unwrap()
            .expect("commented");
        s.store
            .run(TaskWatchOp {
                actor: s.carol.as_str().to_owned(),
                params: TasksWatchParams {
                    command_id: uuid(),
                    task: task.clone(),
                },
                authority: TaskWriteAuthority::Unrestricted,
            })
            .unwrap()
            .expect("watched");
        let step = s.add(&task, "Follow-up", None);
        s.check(&task, &step);
        let revision = s.inspect(&task).task.revision;
        let update: TasksUpdateParams = serde_json::from_value(serde_json::json!({
            "command_id": uuid(),
            "task": task,
            "expected_revision": revision,
            "title": "Ship the ledger v2",
        }))
        .unwrap();
        s.store
            .run(TaskUpdateOp {
                actor: s.carol.as_str().to_owned(),
                params: update,
                prepared_files: Default::default(),
                authority: TaskWriteAuthority::Unrestricted,
            })
            .unwrap()
            .expect("updated");
        assert_eq!(s.inspect(&task).task.final_summary, Some(summary.clone()));

        // Its own call replaces it, attributed to the caller, at the expected revision.
        let revision = s.inspect(&task).task.revision;
        let stale = s.summarize(&s.carol, &task, "late", &uuid(), revision - 1);
        assert_eq!(code(stale), ErrorCode::Conflict);
        let blank = s.summarize(&s.carol, &task, " ", &uuid(), revision);
        assert_eq!(code(blank), ErrorCode::InvalidRequest);
        for bad in ["\0hello", "bad\x1b[31m"] {
            assert_eq!(
                code(s.summarize(&s.carol, &task, bad, &uuid(), revision)),
                ErrorCode::InvalidRequest
            );
        }
        let huge = "é".repeat(TASK_FINAL_SUMMARY_MAX_CHARS + 1);
        assert_eq!(
            code(s.summarize(&s.carol, &task, &huge, &uuid(), revision)),
            ErrorCode::TooLarge
        );
        let exact_utf8 = "é".repeat(TASK_FINAL_SUMMARY_MAX_CHARS);
        let at_limit = s
            .summarize(&s.carol, &task, &exact_utf8, &uuid(), revision)
            .expect("8000 multibyte UTF-8 scalar values fit");
        let revision = at_limit.revision;
        let command = uuid();
        let rewritten = s
            .summarize(
                &s.carol,
                &task,
                "Shipped; the export moved to T-A9.",
                &command,
                revision,
            )
            .expect("summarized");
        let replayed = s
            .summarize(
                &s.carol,
                &task,
                "Shipped; the export moved to T-A9.",
                &command,
                revision,
            )
            .expect("replayed");
        assert_eq!(replayed.final_summary, rewritten.final_summary);
        let summary = rewritten.final_summary.expect("summary");
        assert_eq!(summary.text, "Shipped; the export moved to T-A9.");
        assert_eq!(summary.by_id, s.carol.as_str());
        assert_eq!(rewritten.revision, revision + 1);
        let reused = s.summarize(&s.carol, &task, "other text", &command, revision);
        assert_eq!(code(reused), ErrorCode::OperationReused);

        if engine.is_sqlite() {
            let before = s.inspect(&task).task;
            let s = s.rebuilt();
            assert_eq!(s.inspect(&task).task, before);
            // Reopening clears live `final_summary*` while preserving prior summaries in history.
            let reopened = s
                .transition(
                    &s.owner,
                    &task,
                    Transition {
                        status: TaskStatus::Todo,
                        ..done()
                    },
                )
                .expect("reopened");
            assert!(reopened.final_summary.is_none());
            assert!(s
                .inspect(&task)
                .history
                .iter()
                .any(|e| e.change["final_summary"] == "Shipped; the export moved to T-A9."));
        } else {
            let reopened = s
                .transition(
                    &s.owner,
                    &task,
                    Transition {
                        status: TaskStatus::Todo,
                        ..done()
                    },
                )
                .expect("reopened");
            assert!(reopened.final_summary.is_none());
        }
    }
}

#[test]
fn done_anyway_reason_rejects_control_characters_on_both_engines() {
    for engine in engines() {
        let s = Org::new(engine.clone());
        let task = s.create(false, Some(vec!["Required"])).key;
        for bad in ["ok\0tail", "ok\x1b[31m", "line1\nline2", "line1\rline2"] {
            let change = Transition {
                reason: Some(bad),
                ..done()
            };
            assert_eq!(
                code(s.transition(&s.owner, &task, change)),
                ErrorCode::InvalidRequest,
                "{engine:?} {bad:?}"
            );
        }
    }
}

struct MutateTeam {
    team: Id,
    lead: Option<Id>,
    archive: bool,
}

impl StoreOp for MutateTeam {
    type Out = Result<(), StoreError>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        store
            .submit(None, true, move |uow| {
                use org_office::organization_family::records::{
                    encode_team, team_path, TeamStatus,
                };
                uow.next(Stream::Organization)?;
                let mut team = uow
                    .view()
                    .team(&self.team)?
                    .ok_or_else(|| Refusal::invalid("no such team"))?;
                let chain = uow
                    .view()
                    .team_chain(&self.team)?
                    .ok_or_else(|| Refusal::invalid("no team chain"))?;
                if let Some(lead) = self.lead.clone() {
                    team.lead_employee_id = Some(lead);
                }
                if self.archive {
                    team.status = TeamStatus::Archived;
                }
                let path = team_path(&chain).map_err(|_| Refusal::invalid("bad team path"))?;
                Ok(Change::new(Ack::Durable)
                    .put(path, encode_team(&team))
                    .reply(b"{}".to_vec()))
            })
            .map(|_| ())
    }
}

struct ArchiveProject(Id);

impl StoreOp for ArchiveProject {
    type Out = Result<(), StoreError>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        store
            .submit(None, true, move |uow| {
                use org_office::projects::records::{encode_project, project_path};
                uow.next(Stream::Organization)?;
                let mut project = uow
                    .view()
                    .project(&self.0)?
                    .ok_or_else(|| Refusal::invalid("no such project"))?;
                project.archived = true;
                Ok(Change::new(Ack::Durable)
                    .put(project_path(&project.id), encode_project(&project))
                    .reply(b"{}".to_vec()))
            })
            .map(|_| ())
    }
}

struct FirstAgentId;

impl ReadOp for FirstAgentId {
    type Out = Option<Id>;

    fn read<S: org_office::tasks::ports::TasksRead>(
        self,
        read: &S,
    ) -> Result<Self::Out, StoreError> {
        use org_office::organization_family::records::EmployeeKind;
        Ok(read
            .employees()?
            .into_iter()
            .find(|e| e.kind == EmployeeKind::Agent)
            .map(|e| e.id))
    }
}

#[test]
fn project_default_dod_guards_against_agents_archived_ancestors_and_archived_projects() {
    for engine in engines() {
        let s = Org::new(engine.clone());
        let agent = s.store.read(FirstAgentId).unwrap().0.expect("agent in org");
        let team_b = id(301);

        // 1. Even if an AI agent is listed as an ancestor team's lead, it is refused.
        s.store
            .run(MutateTeam {
                team: s.team_a.clone(),
                lead: Some(agent.clone()),
                archive: false,
            })
            .unwrap();
        refused_unrecorded(
            s.project_default(&agent, Some(vec!["Agent tried"])),
            PROJECT_DOD_DENIED,
        );

        // 2. Archiving ancestor team B revokes Bob's stale ancestor-lead authority.
        completed(s.project_default(&s.bob, Some(vec!["Active lead ok"])));
        s.store
            .run(MutateTeam {
                team: team_b,
                lead: None,
                archive: true,
            })
            .unwrap();
        refused_unrecorded(
            s.project_default(&s.bob, Some(vec!["Archived ancestor"])),
            PROJECT_DOD_DENIED,
        );

        // 3. Archiving the project refuses even the owner.
        s.store.run(ArchiveProject(s.project.clone())).unwrap();
        let reply = s
            .project_default(&s.owner, Some(vec!["After archive"]))
            .unwrap();
        assert_eq!(reply.outcome, Outcome::Rejected);
        let text = String::from_utf8_lossy(&reply.bytes);
        assert!(text.contains("archived"), "{text}");

        // 4. A satellite office identity is refused on both org and project default DoD setters.
        let mut sat_here = s.here.clone();
        sat_here.office_id = id(999);
        let sat_org = s.store.run(SetOrganizationDefaultDodOp {
            operation_id: Id::parse(&uuid()).unwrap(),
            here: sat_here.clone(),
            owner: s.owner.clone(),
            by: Actor::Employee(s.owner.clone()),
            command: SetOrganizationDefaultDod {
                expected_revision: 1,
                default_dod: Some(vec!["Sat".into()]),
            },
        });
        refused_unrecorded(sat_org, "only HQ changes the organization's structure");
        let sat_proj = s.store.run(SetProjectDefaultDodOp {
            operation_id: Id::parse(&uuid()).unwrap(),
            here: sat_here,
            owner: s.owner.clone(),
            by: Actor::Employee(s.owner.clone()),
            command: SetProjectDefaultDod {
                project_id: s.project.clone(),
                expected_revision: 1,
                default_dod: Some(vec!["Sat".into()]),
            },
        });
        refused_unrecorded(sat_proj, "only HQ changes the organization's structure");
    }
}
