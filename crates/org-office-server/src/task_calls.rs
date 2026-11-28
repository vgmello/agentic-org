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
    OrganizationDefaultDodGet, OrganizationDefaultDodSet, ProjectDefaultDodSet,
    TaskInspectSelection, TasksAddDependency, TasksAssign, TasksChecklistAdd, TasksChecklistCheck,
    TasksChecklistEdit, TasksChecklistMove, TasksChecklistRemove, TasksComment, TasksComments,
    TasksCreate, TasksDependencies, TasksFinalSummary, TasksHistory, TasksInspect, TasksLabels,
    TasksList, TasksMutationResult, TasksRemoveDependency, TasksTransition, TasksUnwatch,
    TasksUpdate, TasksWatch,
};
use org_proto::{ApiError, Call, ErrorCode, Outcome, Request, Response};

use crate::live::LiveEvents;

mod search_filter;

fn is_task_core_method(request: &Request) -> bool {
    request.is::<TasksCreate>()
        || request.is::<TasksList>()
        || request.is::<TasksInspect>()
        || request.is::<TasksUpdate>()
        || request.is::<TasksTransition>()
        || request.is::<TasksAssign>()
        || request.is::<TasksFinalSummary>()
}

/// The default definitions of done new tasks copy (`TASK-88`).
fn is_task_dod_method(request: &Request) -> bool {
    request.is::<OrganizationDefaultDodGet>()
        || request.is::<OrganizationDefaultDodSet>()
        || request.is::<ProjectDefaultDodSet>()
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
        || is_task_dod_method(request)
}

mod routing_labels;

/// Live task RPC service composed over [`Office`], [`LiveEvents`], and optional [`SessionManager`].
pub(crate) struct TaskService {
    office: Arc<Office>,
    live: Arc<LiveEvents>,
    sessions: Option<Arc<org_office_sessions::SessionManager>>,
    git: Option<Arc<crate::git_calls::GitService>>,
    sync_wake: Option<Arc<dyn Fn() + Send + Sync>>,
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
            git: None,
            sync_wake: None,
        }
    }

    pub(crate) fn with_git_service(mut self, git: Arc<crate::git_calls::GitService>) -> Self {
        self.git = Some(git);
        self
    }

    pub(crate) fn with_sync_wake(mut self, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        self.sync_wake = Some(wake);
        self
    }

    fn notify_sync(&self) {
        if let Some(wake) = &self.sync_wake {
            wake();
        }
    }

    #[cfg(test)]
    pub(crate) fn dispatch(&self, member: Option<&str>, request: &Request) -> Option<Response> {
        self.dispatch_with_grant(member, true, request)
    }

    pub(crate) fn dispatch_with_grant(
        &self,
        member: Option<&str>,
        via_grant: bool,
        request: &Request,
    ) -> Option<Response> {
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
        self.dispatch_core(member, via_grant, request)
            .or_else(|| self.dispatch_collab(member, request))
            .or_else(|| self.dispatch_checklist(member, request))
            .or_else(|| self.dispatch_notes(member, request))
            .or_else(|| self.dispatch_dod(member, request))
    }

    fn dispatch_core(&self, member: &str, via_grant: bool, request: &Request) -> Option<Response> {
        if request.is::<TasksCreate>() {
            return Some(self.run_write::<TasksCreate>(member, request, |p| {
                if let Err(err) = routing_labels::validate_create_routing_labels(
                    &self.office,
                    member,
                    via_grant,
                    &p.labels,
                ) {
                    return Ok(Err(err));
                }
                self.office.task_create(member, p)
            }));
        }
        if request.is::<TasksList>() {
            // Structured filters, ordering and paging stay on the office's query (`TASK-105`),
            // while non-empty `--query` routes through the FTS5 `search.query` index (`TASK-91`).
            return Some(run_call::<TasksList>(request, |p| {
                search_filter::list_tasks_with_query(&self.office, member, p)
            }));
        }
        if request.is::<TasksInspect>() {
            return Some(self.dispatch_inspect(request));
        }
        if request.is::<TasksUpdate>() {
            return Some(self.run_write::<TasksUpdate>(member, request, |p| {
                if let Err(err) = routing_labels::validate_update_routing_labels(
                    &self.office,
                    member,
                    via_grant,
                    &p,
                ) {
                    return Ok(Err(err));
                }
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
        if request.is::<TasksFinalSummary>() {
            return Some(self.run_write::<TasksFinalSummary>(member, request, |p| {
                self.office.task_final_summary(member, p)
            }));
        }
        None
    }

    /// Default definitions of done (`TASK-88`): a change wakes replication, so satellites
    /// copy the new default offline.
    fn dispatch_dod(&self, member: &str, request: &Request) -> Option<Response> {
        if request.is::<OrganizationDefaultDodGet>() {
            return Some(run_call::<OrganizationDefaultDodGet>(request, |_| {
                self.office.organization_default_dod()
            }));
        }
        if request.is::<OrganizationDefaultDodSet>() {
            return Some(run_call::<OrganizationDefaultDodSet>(request, |p| {
                self.synced(self.office.organization_set_default_dod(member, p))
            }));
        }
        if request.is::<ProjectDefaultDodSet>() {
            return Some(run_call::<ProjectDefaultDodSet>(request, |p| {
                self.synced(self.office.project_set_default_dod(member, p))
            }));
        }
        None
    }

    /// `answer`, after waking replication when it is a change made.
    fn synced<T>(
        &self,
        answer: Result<Result<T, ApiError>, org_office::OfficeError>,
    ) -> Result<Result<T, ApiError>, org_office::OfficeError> {
        if matches!(answer, Ok(Ok(_))) {
            self.notify_sync();
        }
        answer
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
                    self.notify_sync();
                }
                Ok(Ok(res))
            }
            Err(api_err) => Ok(Err(api_err)),
        })
    }

    fn dispatch_collab(&self, member: &str, request: &Request) -> Option<Response> {
        if request.is::<TasksComment>() {
            return Some(run_call::<TasksComment>(request, |p| {
                match self.office.task_comment(member, p)? {
                    Ok((res, replayed)) => {
                        if !replayed {
                            self.live.record_mailbox_change();
                            self.notify_sync();
                            if let Some(sessions) = self.sessions.as_ref() {
                                crate::wake_bridge::on_task_comment_committed(
                                    &self.office,
                                    sessions,
                                    member,
                                    &res,
                                );
                            }
                        }
                        Ok(Ok(res))
                    }
                    Err(api_err) => Ok(Err(api_err)),
                }
            }));
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
                    self.notify_sync();
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
        // Auto-pre-prepare any repository-linked task worktrees BEFORE waking Turn 1 so the
        // initial sandbox launch already holds exact doc-35 bare-cache grants (`TASK-135` AC #4).
        if let Some(git) = self.git.as_ref() {
            let _ = git.preprepare_task_worktrees_for_assignee(agent_id.as_str(), &outcome.task.id);
        }
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
        let attachment_ids = self
            .office
            .attachment_list(
                agent_id.as_str(),
                org_proto::attachments::AttachmentListParams {
                    target_kind: Some(org_proto::attachments::AttachmentTargetKind::Task),
                    target_id: Some(outcome.task.id.clone()),
                    scope: None,
                    limit: Some(org_proto::attachments::ATTACHMENT_LIST_MAX_LIMIT),
                    cursor: None,
                },
            )
            .ok()
            .and_then(Result::ok)
            .map(|res| {
                res.attachments
                    .into_iter()
                    .map(|att| att.id)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let turn_override =
            org_office_sessions::parse_task_routing_labels_or_default(&outcome.task.labels);
        let _ = sessions.on_task_assigned_with_overrides(
            agent_id.as_str(),
            actor,
            &actor_name,
            actor_is_person,
            &outcome.task.key,
            &outcome.task.title,
            attachment_ids,
            turn_override,
        );
    }

    fn dispatch_inspect(&self, request: &Request) -> Response {
        let Ok(params) = request.params::<TasksInspect>() else {
            return invalid_params(request.id, TasksInspect::METHOD);
        };
        let selection = match TaskInspectSelection::try_from_fields(params.fields.as_deref()) {
            Ok(sel) => sel,
            Err(msg) => {
                return Response::error(
                    request.id,
                    ApiError::new(org_proto::ErrorCode::InvalidRequest, msg),
                );
            }
        };
        match self
            .office
            .task_inspect_with_fields(&params.task, params.fields.clone())
        {
            Ok(Ok(result)) => match params.fields.as_deref() {
                None => ok_reply::<TasksInspect>(request.id, &result),
                Some(_) => match serde_json::to_value(&result) {
                    Ok(val) => {
                        let pruned = prune_inspect_value(val, selection);
                        Response {
                            id: request.id,
                            outcome: Outcome::Result(pruned),
                            waiting_messages: None,
                        }
                    }
                    Err(_) => storage_err(request.id),
                },
            },
            Ok(Err(refusal)) => Response::error(request.id, refusal),
            Err(error) => {
                crate::logged(TasksInspect::METHOD, &error);
                storage_err(request.id)
            }
        }
    }
}

fn prune_inspect_value(mut val: serde_json::Value, sel: TaskInspectSelection) -> serde_json::Value {
    let Some(obj) = val.as_object_mut() else {
        return val;
    };
    if !sel.body {
        if let Some(task) = obj
            .get_mut("task")
            .and_then(serde_json::Value::as_object_mut)
        {
            task.remove("body_markdown");
        }
    }
    let sections = [
        ("checklist", sel.checklist),
        ("comments", sel.comments),
        ("attachments", sel.attachments),
        ("watchers", sel.watchers),
        ("depends_on_tasks", sel.depends_on),
        ("dependent_tasks", sel.dependents),
        ("history", sel.history),
    ];
    for (key, keep) in sections {
        if !keep {
            obj.remove(key);
        }
    }
    val
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
mod tests;
