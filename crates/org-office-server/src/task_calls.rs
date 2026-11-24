//! Unified v1 tasks RPC dispatcher on `org-office-server` (`TASK-49` B4/B5 + `TASK-77` B4b).
//!
//! - Routes all `tasks.*` calls to [`Office`].
//! - Triggers [`SessionManager::on_task_assigned`] (`AVAIL-2`, `ADR-0028`) when a task is
//!   assigned to an active agent.
//! - Keeps every handler short and engine-independent.

use std::sync::Arc;

use org_office::organization_family::records::EmployeeKind;
use org_office::tasks::features::TaskWriteOutcome;
use org_office::Office;
use org_proto::private_notes::{
    TasksNotesAdd, TasksNotesGet, TasksNotesList, TasksNotesPublish, TasksNotesRemove,
    TasksNotesUpdate,
};
use org_proto::tasks::{
    TasksAddDependency, TasksAssign, TasksChecklistAdd, TasksChecklistCheck, TasksChecklistEdit,
    TasksChecklistMove, TasksChecklistRemove, TasksComment, TasksComments, TasksCreate,
    TasksDependencies, TasksHistory, TasksInspect, TasksLabels, TasksList, TasksMutationResult,
    TasksRemoveDependency, TasksTransition, TasksUnwatch, TasksUpdate, TasksWatch,
};
use org_proto::{ApiError, Call, ErrorCode, Request, Response};

use crate::live::LiveEvents;

fn is_task_core_method(request: &Request) -> bool {
    request.is::<TasksCreate>()
        || request.is::<TasksList>()
        || request.is::<TasksInspect>()
        || request.is::<TasksUpdate>()
        || request.is::<TasksTransition>()
        || request.is::<TasksAssign>()
}

fn is_task_collab_method(request: &Request) -> bool {
    request.is::<TasksComment>()
        || request.is::<TasksComments>()
        || request.is::<TasksWatch>()
        || request.is::<TasksUnwatch>()
        || request.is::<TasksHistory>()
        || request.is::<TasksDependencies>()
        || request.is::<TasksAddDependency>()
        || request.is::<TasksRemoveDependency>()
        || request.is::<TasksLabels>()
}

fn is_task_checklist_method(request: &Request) -> bool {
    request.is::<TasksChecklistAdd>()
        || request.is::<TasksChecklistCheck>()
        || request.is::<TasksChecklistEdit>()
        || request.is::<TasksChecklistRemove>()
        || request.is::<TasksChecklistMove>()
}

fn is_task_notes_method(request: &Request) -> bool {
    request.is::<TasksNotesAdd>()
        || request.is::<TasksNotesList>()
        || request.is::<TasksNotesGet>()
        || request.is::<TasksNotesUpdate>()
        || request.is::<TasksNotesRemove>()
        || request.is::<TasksNotesPublish>()
}

/// Whether `request` belongs to the unified v1 `tasks.*` RPC family.
pub(crate) fn is_task_method(request: &Request) -> bool {
    is_task_core_method(request)
        || is_task_collab_method(request)
        || is_task_checklist_method(request)
        || is_task_notes_method(request)
}

/// Live task RPC service composed over [`Office`], [`LiveEvents`], and optional [`SessionManager`].
pub(crate) struct TaskService {
    office: Arc<Office>,
    live: Arc<LiveEvents>,
    sessions: Option<Arc<org_office_sessions::SessionManager>>,
}

impl TaskService {
    pub(crate) fn new(
        office: Arc<Office>,
        live: Arc<LiveEvents>,
        sessions: Option<Arc<org_office_sessions::SessionManager>>,
    ) -> Self {
        Self {
            office,
            live,
            sessions,
        }
    }

    pub(crate) fn dispatch(&self, member: Option<&str>, request: &Request) -> Option<Response> {
        if !is_task_method(request) {
            return None;
        }
        let Some(member) = member else {
            return Some(Response::error(
                request.id,
                ApiError::new(ErrorCode::NotBound, "connection is not bound"),
            ));
        };
        // Refuse data RPCs on a satellite awaiting a complete HQ snapshot (`TASK-52` P9-#3).
        if self
            .office
            .sync_status()
            .is_ok_and(|s| s.initial_snapshot_required)
        {
            return Some(Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::Conflict,
                    "satellite office is awaiting a complete HQ snapshot",
                ),
            ));
        }
        self.dispatch_core(member, request)
            .or_else(|| self.dispatch_collab(member, request))
            .or_else(|| self.dispatch_checklist(member, request))
            .or_else(|| self.dispatch_notes(member, request))
    }

    fn dispatch_core(&self, member: &str, request: &Request) -> Option<Response> {
        if request.is::<TasksCreate>() {
            return Some(self.run_write::<TasksCreate>(member, request, |p| {
                self.office.task_create(member, p)
            }));
        }
        if request.is::<TasksList>() {
            return Some(run_call::<TasksList>(request, |p| self.office.task_list(p)));
        }
        if request.is::<TasksInspect>() {
            return Some(run_call::<TasksInspect>(request, |p| {
                self.office.task_inspect(&p.task)
            }));
        }
        if request.is::<TasksUpdate>() {
            return Some(self.run_write::<TasksUpdate>(member, request, |p| {
                self.office.task_update(member, p)
            }));
        }
        if request.is::<TasksTransition>() {
            return Some(self.run_write::<TasksTransition>(member, request, |p| {
                self.office.task_transition(member, p)
            }));
        }
        if request.is::<TasksAssign>() {
            return Some(self.run_write::<TasksAssign>(member, request, |p| {
                self.office.task_assign(member, p)
            }));
        }
        None
    }

    fn run_notifying<C: Call>(
        &self,
        request: &Request,
        run: impl FnOnce(
            C::Params,
        )
            -> Result<Result<(C::Result, bool), ApiError>, org_office::OfficeError>,
    ) -> Response {
        run_call::<C>(request, |p| match run(p)? {
            Ok((res, replayed)) => {
                if !replayed {
                    self.live.record_mailbox_change();
                }
                Ok(Ok(res))
            }
            Err(api_err) => Ok(Err(api_err)),
        })
    }

    fn dispatch_collab(&self, member: &str, request: &Request) -> Option<Response> {
        if request.is::<TasksComment>() {
            return Some(
                self.run_notifying::<TasksComment>(request, |p| {
                    self.office.task_comment(member, p)
                }),
            );
        }
        if request.is::<TasksComments>() {
            return Some(run_call::<TasksComments>(request, |p| {
                self.office.task_comments(&p.task)
            }));
        }
        if request.is::<TasksWatch>() {
            return Some(
                self.run_write::<TasksWatch>(member, request, |p| {
                    self.office.task_watch(member, p)
                }),
            );
        }
        if request.is::<TasksUnwatch>() {
            return Some(self.run_write::<TasksUnwatch>(member, request, |p| {
                self.office.task_unwatch(member, p)
            }));
        }
        if request.is::<TasksHistory>() {
            return Some(run_call::<TasksHistory>(request, |p| {
                self.office.task_history(&p.task)
            }));
        }
        if request.is::<TasksDependencies>() {
            return Some(run_call::<TasksDependencies>(request, |p| {
                self.office.task_dependencies(&p.task)
            }));
        }
        if request.is::<TasksAddDependency>() {
            return Some(self.run_write::<TasksAddDependency>(member, request, |p| {
                self.office.task_add_dependency(member, p)
            }));
        }
        if request.is::<TasksRemoveDependency>() {
            return Some(
                self.run_write::<TasksRemoveDependency>(member, request, |p| {
                    self.office.task_remove_dependency(member, p)
                }),
            );
        }
        if request.is::<TasksLabels>() {
            return Some(run_call::<TasksLabels>(request, |p| {
                self.office.task_labels(p)
            }));
        }
        None
    }

    fn dispatch_checklist(&self, member: &str, request: &Request) -> Option<Response> {
        if request.is::<TasksChecklistAdd>() {
            return Some(self.run_notifying::<TasksChecklistAdd>(request, |p| {
                self.office.task_checklist_add(member, p)
            }));
        }
        if request.is::<TasksChecklistCheck>() {
            return Some(self.run_notifying::<TasksChecklistCheck>(request, |p| {
                self.office.task_checklist_check(member, p)
            }));
        }
        if request.is::<TasksChecklistEdit>() {
            return Some(self.run_notifying::<TasksChecklistEdit>(request, |p| {
                self.office.task_checklist_edit(member, p)
            }));
        }
        if request.is::<TasksChecklistRemove>() {
            return Some(self.run_notifying::<TasksChecklistRemove>(request, |p| {
                self.office.task_checklist_remove(member, p)
            }));
        }
        if request.is::<TasksChecklistMove>() {
            return Some(self.run_notifying::<TasksChecklistMove>(request, |p| {
                self.office.task_checklist_move(member, p)
            }));
        }
        None
    }

    /// Private task notes (`TASK-50`, `L8`, `ADR-0031`). Add/update/remove NEVER emit shared
    /// `LiveEvents` or wake agents (`PN-02`); only explicit `publish` emits a shared comment.
    fn dispatch_notes(&self, member: &str, request: &Request) -> Option<Response> {
        if request.is::<TasksNotesAdd>() {
            return Some(run_call::<TasksNotesAdd>(request, |p| {
                self.office.task_note_add(member, p)
            }));
        }
        if request.is::<TasksNotesList>() {
            return Some(run_call::<TasksNotesList>(request, |p| {
                self.office.task_note_list(member, p)
            }));
        }
        if request.is::<TasksNotesGet>() {
            return Some(run_call::<TasksNotesGet>(request, |p| {
                self.office.task_note_get(member, p)
            }));
        }
        if request.is::<TasksNotesUpdate>() {
            return Some(run_call::<TasksNotesUpdate>(request, |p| {
                self.office.task_note_update(member, p)
            }));
        }
        if request.is::<TasksNotesRemove>() {
            return Some(run_call::<TasksNotesRemove>(request, |p| {
                self.office.task_note_remove(member, p)
            }));
        }
        if request.is::<TasksNotesPublish>() {
            return Some(self.run_notifying::<TasksNotesPublish>(request, |p| {
                self.office.task_note_publish(member, p)
            }));
        }
        None
    }

    fn run_write<C: Call<Result = TasksMutationResult>>(
        &self,
        actor: &str,
        request: &Request,
        run: impl FnOnce(
            C::Params,
        ) -> Result<Result<TaskWriteOutcome, ApiError>, org_office::OfficeError>,
    ) -> Response {
        let Ok(params) = request.params::<C>() else {
            return invalid_params(request.id, C::METHOD);
        };
        match run(params) {
            Ok(Ok(outcome)) => {
                if !outcome.replayed {
                    self.live.record_mailbox_change();
                    self.maybe_wake_assigned_agent(actor, &outcome);
                }
                ok_reply::<C>(request.id, &TasksMutationResult { task: outcome.task })
            }
            Ok(Err(refusal)) => Response::error(request.id, refusal),
            Err(error) => {
                crate::logged(C::METHOD, &error);
                storage_err(request.id)
            }
        }
    }

    fn maybe_wake_assigned_agent(&self, actor: &str, outcome: &TaskWriteOutcome) {
        let Some(agent_id) = outcome.newly_assigned_agent.as_ref() else {
            return;
        };
        let Some(sessions) = self.sessions.as_ref() else {
            return;
        };
        let (actor_name, actor_is_person) = self
            .office
            .load_employee(actor)
            .ok()
            .flatten()
            .map(|e| (e.name, e.kind == EmployeeKind::Person))
            .unwrap_or_else(|| (actor.to_owned(), false));
        let _ = sessions.on_task_assigned(
            agent_id.as_str(),
            actor,
            &actor_name,
            actor_is_person,
            &outcome.task.key,
            &outcome.task.title,
        );
    }
}

fn run_call<C: Call>(
    request: &Request,
    run: impl FnOnce(C::Params) -> Result<Result<C::Result, ApiError>, org_office::OfficeError>,
) -> Response {
    let Ok(params) = request.params::<C>() else {
        return invalid_params(request.id, C::METHOD);
    };
    match run(params) {
        Ok(Ok(result)) => ok_reply::<C>(request.id, &result),
        Ok(Err(refusal)) => Response::error(request.id, refusal),
        Err(error) => {
            crate::logged(C::METHOD, &error);
            storage_err(request.id)
        }
    }
}

fn ok_reply<C: Call>(id: u64, result: &C::Result) -> Response {
    Response::ok::<C>(id, result).unwrap_or_else(|_| storage_err(id))
}

fn invalid_params(id: u64, method: &str) -> Response {
    Response::error(
        id,
        ApiError::new(
            ErrorCode::InvalidRequest,
            format!("the params of `{method}` are not valid"),
        ),
    )
}

fn storage_err(id: u64) -> Response {
    Response::error(
        id,
        ApiError::new(ErrorCode::StorageError, "the office could not answer"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use org_office::TestOffice;
    use org_proto::tasks::{
        LabelMatchMode, PatchField, TaskSortMode, TaskStatus, TasksAddDependencyParams,
        TasksAssignParams, TasksCreateParams, TasksInspectParams, TasksListParams,
        TasksTransitionParams,
    };
    use org_proto::Outcome;

    fn uuid(n: u64) -> String {
        let ms = org_office::organization_family::testkit::CREATION_AT.0;
        let mut bytes = [0u8; 16];
        bytes[0..6].copy_from_slice(&ms.to_be_bytes()[2..8]);
        bytes[6] = 0x70 | ((n >> 8) as u8 & 0x0f);
        bytes[7] = (n & 0xff) as u8;
        bytes[8] = 0x80;
        bytes[15] = (n & 0xff) as u8;
        format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
        )
    }

    #[test]
    fn task_service_dispatches_create_list_dependencies_and_transition() {
        let office = Arc::new(Office::for_test(TestOffice::default()));
        let owner = office.owner_member_id().to_owned();
        let root_team = office.org_snapshot().unwrap().teams[0].id.clone();
        let live = Arc::new(LiveEvents::new(Box::new(|| {})));
        let svc = TaskService::new(Arc::clone(&office), live, None);

        // Unbound caller rejected.
        let list_req = Request::new::<TasksList>(1, &TasksListParams::default()).unwrap();
        let unbound = svc.dispatch(None, &list_req).unwrap();
        assert!(matches!(unbound.outcome, Outcome::Error(e) if e.code == ErrorCode::NotBound));

        // Create two tasks with labels, files, and order.
        let r1 = Request::new::<TasksCreate>(
            2,
            &TasksCreateParams {
                command_id: uuid(10),
                task_id: uuid(11),
                team_id: Some(root_team.clone()),
                project_id: None,
                assigned_team_id: None,
                assignee_id: None,
                task_type: Some("task".into()),
                work_type: None,
                title: "First task".into(),
                body_markdown: String::new(),
                priority: None,
                effort: None,
                severity: None,
                rank_order: Some(10),
                labels: vec!["backend".into(), "security".into()],
                reference_files: vec!["docs/a.md".into()],
                modified_files: vec!["src/a.rs".into()],
                depends_on: vec![],
                details: None,
            },
        )
        .unwrap();
        let res1: TasksMutationResult = match svc.dispatch(Some(&owner), &r1).unwrap().outcome {
            Outcome::Result(v) => serde_json::from_value(v).unwrap(),
            Outcome::Error(e) => panic!("create 1 failed: {e:?}"),
        };
        assert_eq!(res1.task.key, "T-A1");

        let r2 = Request::new::<TasksCreate>(
            3,
            &TasksCreateParams {
                command_id: uuid(12),
                task_id: uuid(13),
                team_id: Some(root_team),
                project_id: None,
                assigned_team_id: None,
                assignee_id: None,
                task_type: Some("bug".into()),
                work_type: None,
                title: "Second bug".into(),
                body_markdown: String::new(),
                priority: None,
                effort: None,
                severity: Some("major".into()),
                rank_order: Some(1),
                labels: vec!["backend".into()],
                reference_files: vec![],
                modified_files: vec![],
                depends_on: vec![res1.task.key.clone()],
                details: None,
            },
        )
        .unwrap();
        let res2: TasksMutationResult = match svc.dispatch(Some(&owner), &r2).unwrap().outcome {
            Outcome::Result(v) => serde_json::from_value(v).unwrap(),
            Outcome::Error(e) => panic!("create 2 failed: {e:?}"),
        };
        assert_eq!(res2.task.key, "T-A2");
        assert_eq!(res2.task.depends_on, vec![res1.task.id.clone()]);

        // Cycle refused through RPC.
        let cyc = Request::new::<TasksAddDependency>(
            4,
            &TasksAddDependencyParams {
                command_id: uuid(14),
                task: res1.task.key.clone(),
                depends_on_task: res2.task.key.clone(),
            },
        )
        .unwrap();
        assert!(matches!(
            svc.dispatch(Some(&owner), &cyc).unwrap().outcome,
            Outcome::Error(e) if e.code == ErrorCode::Conflict
        ));

        // Ordered list returns T-A2 (rank 1) before T-A1 (rank 10), and label AND filter narrows.
        let list_ordered = Request::new::<TasksList>(
            5,
            &TasksListParams {
                labels: vec!["backend".into(), "security".into()],
                label_match: LabelMatchMode::All,
                sort: TaskSortMode::Order,
                ..Default::default()
            },
        )
        .unwrap();
        let listed: org_proto::tasks::TasksListResult =
            match svc.dispatch(Some(&owner), &list_ordered).unwrap().outcome {
                Outcome::Result(v) => serde_json::from_value(v).unwrap(),
                Outcome::Error(e) => panic!("list failed: {e:?}"),
            };
        assert_eq!(listed.tasks.len(), 1);
        assert_eq!(listed.tasks[0].key, "T-A1");

        // Assign + transition + inspect round-trip.
        let assign_req = Request::new::<TasksAssign>(
            6,
            &TasksAssignParams {
                command_id: uuid(15),
                task: res1.task.key.clone(),
                expected_revision: res1.task.revision,
                assigned_team_id: PatchField::Unchanged,
                assignee_id: PatchField::Set(Some(owner.clone())),
            },
        )
        .unwrap();
        let assigned: TasksMutationResult =
            match svc.dispatch(Some(&owner), &assign_req).unwrap().outcome {
                Outcome::Result(v) => serde_json::from_value(v).unwrap(),
                Outcome::Error(e) => panic!("assign failed: {e:?}"),
            };
        let tr_req = Request::new::<TasksTransition>(
            7,
            &TasksTransitionParams {
                command_id: uuid(16),
                task: res1.task.key.clone(),
                expected_revision: assigned.task.revision,
                status: TaskStatus::InProgress,
                blocked_reason: None,
                resolution: None,
                note: None,
            },
        )
        .unwrap();
        assert!(matches!(
            svc.dispatch(Some(&owner), &tr_req).unwrap().outcome,
            Outcome::Result(_)
        ));
        let insp_req = Request::new::<TasksInspect>(
            8,
            &TasksInspectParams {
                task: res1.task.key,
            },
        )
        .unwrap();
        let inspected: org_proto::tasks::TaskInspectResult =
            match svc.dispatch(Some(&owner), &insp_req).unwrap().outcome {
                Outcome::Result(v) => serde_json::from_value(v).unwrap(),
                Outcome::Error(e) => panic!("inspect failed: {e:?}"),
            };
        assert_eq!(inspected.task.status, TaskStatus::InProgress);
        assert_eq!(inspected.dependent_tasks.len(), 1);
        assert_eq!(inspected.history.len(), 3);
    }

    #[test]
    fn private_notes_never_emit_live_events_until_explicit_publish() {
        use org_proto::private_notes::{
            TasksNotesAddParams, TasksNotesPublishParams, TasksNotesUpdateParams,
        };
        use std::sync::atomic::{AtomicUsize, Ordering};
        let office = Arc::new(Office::for_test(TestOffice::default()));
        let owner = office.owner_member_id().to_owned();
        let root_team = office.org_snapshot().unwrap().teams[0].id.clone();
        let notifications = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&notifications);
        let live = Arc::new(LiveEvents::new(Box::new(move || {
            counter.fetch_add(1, Ordering::Relaxed);
        })));
        let svc = TaskService::new(Arc::clone(&office), live, None);

        let create_req = Request::new::<TasksCreate>(
            1,
            &TasksCreateParams {
                command_id: uuid(100),
                task_id: uuid(101),
                team_id: Some(root_team),
                project_id: None,
                assigned_team_id: None,
                assignee_id: None,
                task_type: Some("task".into()),
                work_type: None,
                title: "Private note host test".into(),
                body_markdown: String::new(),
                priority: None,
                effort: None,
                severity: None,
                rank_order: None,
                labels: vec![],
                reference_files: vec![],
                modified_files: vec![],
                depends_on: vec![],
                details: None,
            },
        )
        .unwrap();
        assert!(matches!(
            svc.dispatch(Some(&owner), &create_req).unwrap().outcome,
            Outcome::Result(_)
        ));
        let before = notifications.load(Ordering::Relaxed);

        let note_id = uuid(102);
        let add_req = Request::new::<TasksNotesAdd>(
            2,
            &TasksNotesAddParams {
                command_id: uuid(103),
                task: "T-A1".into(),
                note_id: note_id.clone(),
                body_markdown: "Private draft".into(),
            },
        )
        .unwrap();
        assert!(matches!(
            svc.dispatch(Some(&owner), &add_req).unwrap().outcome,
            Outcome::Result(_)
        ));
        let edit_req = Request::new::<TasksNotesUpdate>(
            3,
            &TasksNotesUpdateParams {
                command_id: uuid(104),
                note_id: note_id.clone(),
                expected_revision: 1,
                body_markdown: "Refined private draft".into(),
            },
        )
        .unwrap();
        assert!(matches!(
            svc.dispatch(Some(&owner), &edit_req).unwrap().outcome,
            Outcome::Result(_)
        ));
        assert_eq!(
            notifications.load(Ordering::Relaxed),
            before,
            "private note add/update must not emit shared LiveEvents"
        );

        let pub_req = Request::new::<TasksNotesPublish>(
            4,
            &TasksNotesPublishParams {
                command_id: uuid(105),
                note_id,
                expected_revision: 2,
                comment_id: uuid(106),
            },
        )
        .unwrap();
        assert!(matches!(
            svc.dispatch(Some(&owner), &pub_req).unwrap().outcome,
            Outcome::Result(_)
        ));
        assert_eq!(notifications.load(Ordering::Relaxed), before + 1);
        // Replay does not emit a duplicate LiveEvents notification.
        assert!(matches!(
            svc.dispatch(Some(&owner), &pub_req).unwrap().outcome,
            Outcome::Result(_)
        ));
        assert_eq!(notifications.load(Ordering::Relaxed), before + 1);
    }
}
