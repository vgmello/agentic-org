//! Engine-independent `StoreOp` and `ReadOp` wrappers for the unified tasks family (`TASK-49`, `TASK-77`).

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use org_proto::attachments::{AttachmentMeta, AttachmentView};
use org_proto::tasks::{
    TaskDependenciesResult, TaskInspectResult, TasksAddDependencyParams, TasksAssignParams,
    TasksChecklistAddParams, TasksChecklistCheckParams, TasksChecklistEditParams,
    TasksChecklistMoveParams, TasksChecklistRemoveParams, TasksChecklistResult, TasksCommentParams,
    TasksCommentResult, TasksCommentsResult, TasksCreateParams, TasksFinalSummaryParams,
    TasksHistoryResult, TasksLabelsParams, TasksLabelsResult, TasksListParams, TasksListResult,
    TasksRemoveDependencyParams, TasksTransitionParams, TasksUnwatchParams, TasksUpdateParams,
    TasksWatchParams,
};
use org_proto::{ApiError, ErrorCode};
use org_store::{
    Ack, Actor, Change, Id, OfficeIdentity, Operation, Outcome, Refusal, Reply, Sha256, Store,
    StoreError,
};
use serde::{Deserialize, Serialize};

use crate::attachments::records::{AttachmentRecord, MetaPatch};
use crate::tasks::features::{
    self, build_task_view, comment_to_wire, decode_refusal, step_to_wire, AddDependency,
    AssignTask, AttachToTask, ChecklistAdd, ChecklistCheck, ChecklistEdit, ChecklistMove,
    ChecklistRemove, CommentTask, CreateTask, PreparedTaskFiles, RemoveDependency, SetFinalSummary,
    TaskNumbering, TaskWriteOutcome, TransitionTask, UnwatchTask, UpdateTask,
    UpdateTaskAttachmentMeta, WatchTask,
};
use crate::tasks::ports::TasksRead;
use crate::tasks::records::{OfficeTaskCounterRecord, TaskCommentRecord};

use super::{OfficeEngine, ReadOp, StoreOp};

#[derive(Serialize, Deserialize)]
struct TaskCommitReply {
    task_id: String,
    #[serde(default)]
    newly_assigned_agent: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct CommentCommitReply {
    task_id: String,
    comment: TaskCommentRecord,
    #[serde(default)]
    attachments: Vec<AttachmentMeta>,
}

fn task_op<P: Serialize>(
    kind: &'static str,
    command_id: &str,
    actor: &str,
    params: &P,
) -> Option<Operation> {
    let id = Id::parse(command_id).ok()?;
    let actor_val = Id::parse(actor)
        .map(Actor::Employee)
        .unwrap_or(Actor::System);
    let fields = serde_json::to_vec(params).unwrap_or_default();
    Some(Operation {
        id,
        kind,
        actor: actor_val,
        fingerprint: Sha256::of_parts(&[kind.as_bytes(), b"\0", &fields]),
    })
}

fn with_reply<T: Serialize>(mut change: Change, payload: &T) -> Change {
    change.reply = serde_json::to_vec(payload).unwrap_or_default().into();
    change
}

fn submit_task<E: OfficeEngine>(
    store: &Store<E>,
    op: Option<Operation>,
    decide: impl FnMut(&mut org_store::UnitOfWork<'_, E>) -> Result<Change, Refusal> + Send + 'static,
) -> Result<Result<(Reply, bool), ApiError>, StoreError> {
    let executed = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&executed);
    let mut decide = decide;
    match store.submit(op, false, move |uow| {
        flag.store(true, Ordering::Relaxed);
        decide(uow)
    }) {
        Ok(reply) => Ok(Ok((reply, !executed.load(Ordering::Relaxed)))),
        Err(StoreError::Conflict) => Ok(Err(ApiError::new(
            ErrorCode::OperationReused,
            "command_id reused with different request",
        ))),
        Err(other) => Err(other),
    }
}

fn finish_view<E: OfficeEngine>(
    store: &Store<E>,
    reply: Reply,
    replayed: bool,
) -> Result<Result<TaskWriteOutcome, ApiError>, StoreError> {
    match reply.outcome {
        Outcome::Rejected => Ok(Err(decode_refusal(&Refusal::Rejected(reply.bytes)))),
        Outcome::Completed => {
            let committed: TaskCommitReply = serde_json::from_slice(&reply.bytes)
                .map_err(|_| StoreError::invalid("corrupt task commit reply"))?;
            let task_id = Id::parse(&committed.task_id)
                .map_err(|_| StoreError::invalid("invalid committed task id"))?;
            let agent = if replayed {
                None
            } else {
                committed
                    .newly_assigned_agent
                    .as_deref()
                    .and_then(|s| Id::parse(s).ok())
            };
            let (out, _) = store.read(|snap| {
                let Some(t) = snap.task(&task_id)? else {
                    return Ok(Err(ApiError::new(
                        ErrorCode::StorageError,
                        "committed task missing from projection",
                    )));
                };
                match build_task_view(snap, &t) {
                    Ok(view) => Ok(Ok(TaskWriteOutcome {
                        task: view,
                        newly_assigned_agent: agent,
                        replayed,
                    })),
                    Err(r) => Ok(Err(decode_refusal(&r))),
                }
            })?;
            Ok(out)
        }
    }
}

fn finish_checklist<E: OfficeEngine>(
    store: &Store<E>,
    reply: Reply,
    replayed: bool,
) -> Result<Result<(TasksChecklistResult, bool), ApiError>, StoreError> {
    match reply.outcome {
        Outcome::Rejected => Ok(Err(decode_refusal(&Refusal::Rejected(reply.bytes)))),
        Outcome::Completed => {
            let committed: TaskCommitReply = serde_json::from_slice(&reply.bytes)
                .map_err(|_| StoreError::invalid("corrupt checklist commit reply"))?;
            let task_id = Id::parse(&committed.task_id)
                .map_err(|_| StoreError::invalid("invalid committed task id"))?;
            let (out, _) = store.read(|snap| {
                let Some(t) = snap.task(&task_id)? else {
                    return Ok(Err(ApiError::new(
                        ErrorCode::StorageError,
                        "committed task missing from projection",
                    )));
                };
                let task = match build_task_view(snap, &t) {
                    Ok(v) => v,
                    Err(r) => return Ok(Err(decode_refusal(&r))),
                };
                let checklist = snap
                    .checklist_steps(&task_id)?
                    .iter()
                    .map(step_to_wire)
                    .collect();
                Ok(Ok((TasksChecklistResult { task, checklist }, replayed)))
            })?;
            Ok(out)
        }
    }
}

pub struct TaskCreateOp {
    pub actor: String,
    pub here: OfficeIdentity,
    pub params: TasksCreateParams,
    /// [`super::OfficeStore::task_numbering`]: the store's task numbering (`ADR-0019`).
    pub numbering: Arc<TaskNumbering>,
    /// Validated, blob-promoted files to bind on create (`TASK-98.4`).
    pub prepared_files: PreparedTaskFiles,
}

impl StoreOp for TaskCreateOp {
    type Out = Result<Result<TaskWriteOutcome, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let op = task_op(
            "tasks.create",
            &self.params.command_id,
            &self.actor,
            &self.params,
        );
        let numbering = Arc::clone(&self.numbering);
        // The counter the last decision left, and whether it was answered `durable`.
        let issued: Arc<Mutex<Option<(OfficeTaskCounterRecord, bool)>>> = Arc::default();
        let decided = Arc::clone(&issued);
        let cmd = CreateTask {
            actor_member_id: self.actor,
            params: self.params,
            numbering: self.numbering,
            prepared_files: self.prepared_files,
        };
        let (reply, replayed) = match submit_task(store, op, move |uow| {
            let created = cmd.decide(uow, &self.here)?;
            let durable = created.change.ack == Ack::Durable;
            *decided.lock().unwrap_or_else(|p| p.into_inner()) = Some((created.counter, durable));
            Ok(with_reply(
                created.change,
                &TaskCommitReply {
                    task_id: created.task_id.as_str().to_owned(),
                    newly_assigned_agent: created.assigned_agent.map(|a| a.as_str().to_owned()),
                },
            ))
        })? {
            Ok(pair) => pair,
            Err(api_err) => return Ok(Err(api_err)),
        };
        // A `durable` create that ran here and completed is flushed with its counter, so the
        // creates inside that reservation may be answered `committed` (`ADR-0019`).
        if !replayed && reply.outcome == Outcome::Completed {
            let issued = issued.lock().unwrap_or_else(|p| p.into_inner()).take();
            if let Some((counter, true)) = issued {
                numbering.flushed(&counter);
            }
        }
        finish_view(store, reply, replayed)
    }
}

pub struct TaskUpdateOp {
    pub actor: String,
    pub params: TasksUpdateParams,
    /// Validated, blob-promoted files for `params.add_attachments` (`TASK-98.4`).
    pub prepared_files: PreparedTaskFiles,
}

impl StoreOp for TaskUpdateOp {
    type Out = Result<Result<TaskWriteOutcome, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let op = task_op(
            "tasks.update",
            &self.params.command_id,
            &self.actor,
            &self.params,
        );
        let cmd = UpdateTask {
            actor_member_id: self.actor,
            params: self.params,
            prepared_files: self.prepared_files,
        };
        let (reply, replayed) = match submit_task(store, op, move |uow| {
            let (change, id) = cmd.decide(uow)?;
            Ok(with_reply(
                change,
                &TaskCommitReply {
                    task_id: id.as_str().to_owned(),
                    newly_assigned_agent: None,
                },
            ))
        })? {
            Ok(pair) => pair,
            Err(api_err) => return Ok(Err(api_err)),
        };
        finish_view(store, reply, replayed)
    }
}

pub struct TaskTransitionOp {
    pub actor: String,
    pub params: TasksTransitionParams,
}

impl StoreOp for TaskTransitionOp {
    type Out = Result<Result<TaskWriteOutcome, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let op = task_op(
            "tasks.transition",
            &self.params.command_id,
            &self.actor,
            &self.params,
        );
        let cmd = TransitionTask {
            actor_member_id: self.actor,
            params: self.params,
        };
        let (reply, replayed) = match submit_task(store, op, move |uow| {
            let (change, id) = cmd.decide(uow)?;
            Ok(with_reply(
                change,
                &TaskCommitReply {
                    task_id: id.as_str().to_owned(),
                    newly_assigned_agent: None,
                },
            ))
        })? {
            Ok(pair) => pair,
            Err(api_err) => return Ok(Err(api_err)),
        };
        finish_view(store, reply, replayed)
    }
}

pub struct TaskAssignOp {
    pub actor: String,
    pub params: TasksAssignParams,
}

impl StoreOp for TaskAssignOp {
    type Out = Result<Result<TaskWriteOutcome, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let op = task_op(
            "tasks.assign",
            &self.params.command_id,
            &self.actor,
            &self.params,
        );
        let cmd = AssignTask {
            actor_member_id: self.actor,
            params: self.params,
        };
        let (reply, replayed) = match submit_task(store, op, move |uow| {
            let (change, id, agent) = cmd.decide(uow)?;
            Ok(with_reply(
                change,
                &TaskCommitReply {
                    task_id: id.as_str().to_owned(),
                    newly_assigned_agent: agent.map(|a| a.as_str().to_owned()),
                },
            ))
        })? {
            Ok(pair) => pair,
            Err(api_err) => return Ok(Err(api_err)),
        };
        finish_view(store, reply, replayed)
    }
}

pub struct TaskCommentOp {
    pub actor: String,
    pub params: TasksCommentParams,
    /// Validated, blob-promoted files bound to this comment (`TASK-98.4`).
    pub prepared_files: PreparedTaskFiles,
}

impl StoreOp for TaskCommentOp {
    type Out = Result<Result<(TasksCommentResult, bool), ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let op = task_op(
            "tasks.comment",
            &self.params.command_id,
            &self.actor,
            &self.params,
        );
        let cmd = CommentTask {
            actor_member_id: self.actor,
            params: self.params,
            prepared_files: self.prepared_files,
        };
        let (reply, replayed) = match submit_task(store, op, move |uow| {
            let (change, id, comment, attachments) = cmd.decide(uow)?;
            let meta: Vec<AttachmentMeta> =
                attachments.iter().map(AttachmentRecord::to_meta).collect();
            Ok(with_reply(
                change,
                &CommentCommitReply {
                    task_id: id.as_str().to_owned(),
                    comment,
                    attachments: meta,
                },
            ))
        })? {
            Ok(pair) => pair,
            Err(api_err) => return Ok(Err(api_err)),
        };
        match reply.outcome {
            Outcome::Rejected => Ok(Err(decode_refusal(&Refusal::Rejected(reply.bytes)))),
            Outcome::Completed => {
                let committed: CommentCommitReply = serde_json::from_slice(&reply.bytes)
                    .map_err(|_| StoreError::invalid("corrupt comment commit reply"))?;
                let task_id = Id::parse(&committed.task_id)
                    .map_err(|_| StoreError::invalid("invalid committed task id"))?;
                let (out, _) = store.read(|snap| {
                    let Some(t) = snap.task(&task_id)? else {
                        return Ok(Err(ApiError::new(
                            ErrorCode::StorageError,
                            "committed task missing from projection",
                        )));
                    };
                    match build_task_view(snap, &t) {
                        Ok(task) => Ok(Ok((
                            TasksCommentResult {
                                task,
                                comment: comment_to_wire(&committed.comment, committed.attachments),
                            },
                            replayed,
                        ))),
                        Err(r) => Ok(Err(decode_refusal(&r))),
                    }
                })?;
                Ok(out)
            }
        }
    }
}

/// Binds one attachment to a task via `attachment.attach` (`TASK-98.4`).
pub struct TaskAttachOp {
    pub operation: Operation,
    pub actor: String,
    pub command_id: String,
    pub record: AttachmentRecord,
}

impl StoreOp for TaskAttachOp {
    type Out = Result<Result<AttachmentView, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let cmd = AttachToTask {
            actor_member_id: self.actor,
            command_id: self.command_id,
            record: self.record,
        };
        let (reply, _) = match submit_task(store, Some(self.operation), move |uow| {
            let (change, view) = cmd.decide(uow)?;
            Ok(with_reply(change, &view))
        })? {
            Ok(pair) => pair,
            Err(api_err) => return Ok(Err(api_err)),
        };
        if reply.outcome == Outcome::Rejected {
            return Ok(Err(decode_refusal(&Refusal::Rejected(reply.bytes))));
        }
        serde_json::from_slice::<AttachmentView>(&reply.bytes)
            .map(Ok)
            .map_err(|e| StoreError::Invalid(format!("corrupt task attach reply: {e}")))
    }
}

/// Updates metadata on a task-bound attachment via `attachment.update_meta` (`TASK-98.4`).
pub struct TaskUpdateAttachmentMetaOp {
    pub operation: Operation,
    pub actor: String,
    pub command_id: String,
    pub attachment_id: Id,
    pub patch: MetaPatch,
}

impl StoreOp for TaskUpdateAttachmentMetaOp {
    type Out = Result<Result<AttachmentView, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let cmd = UpdateTaskAttachmentMeta {
            actor_member_id: self.actor,
            command_id: self.command_id,
            attachment_id: self.attachment_id,
            patch: self.patch,
        };
        let (reply, _) = match submit_task(store, Some(self.operation), move |uow| {
            let (change, view) = cmd.decide(uow)?;
            Ok(with_reply(change, &view))
        })? {
            Ok(pair) => pair,
            Err(api_err) => return Ok(Err(api_err)),
        };
        if reply.outcome == Outcome::Rejected {
            return Ok(Err(decode_refusal(&Refusal::Rejected(reply.bytes))));
        }
        serde_json::from_slice::<AttachmentView>(&reply.bytes)
            .map(Ok)
            .map_err(|e| StoreError::Invalid(format!("corrupt task attach meta reply: {e}")))
    }
}

macro_rules! simple_task_write_op {
    ($op_name:ident, $method:literal, $params_ty:ty, $cmd_ty:ident) => {
        pub struct $op_name {
            pub actor: String,
            pub params: $params_ty,
        }

        impl StoreOp for $op_name {
            type Out = Result<Result<TaskWriteOutcome, ApiError>, StoreError>;
            fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
                let op = task_op($method, &self.params.command_id, &self.actor, &self.params);
                let cmd = $cmd_ty {
                    actor_member_id: self.actor,
                    params: self.params,
                };
                let (reply, replayed) = match submit_task(store, op, move |uow| {
                    let (change, id) = cmd.decide(uow)?;
                    Ok(with_reply(
                        change,
                        &TaskCommitReply {
                            task_id: id.as_str().to_owned(),
                            newly_assigned_agent: None,
                        },
                    ))
                })? {
                    Ok(pair) => pair,
                    Err(api_err) => return Ok(Err(api_err)),
                };
                finish_view(store, reply, replayed)
            }
        }
    };
}

simple_task_write_op!(TaskWatchOp, "tasks.watch", TasksWatchParams, WatchTask);
simple_task_write_op!(
    TaskFinalSummaryOp,
    "tasks.final_summary",
    TasksFinalSummaryParams,
    SetFinalSummary
);
simple_task_write_op!(
    TaskUnwatchOp,
    "tasks.unwatch",
    TasksUnwatchParams,
    UnwatchTask
);
simple_task_write_op!(
    TaskAddDependencyOp,
    "tasks.add_dependency",
    TasksAddDependencyParams,
    AddDependency
);
simple_task_write_op!(
    TaskRemoveDependencyOp,
    "tasks.remove_dependency",
    TasksRemoveDependencyParams,
    RemoveDependency
);

macro_rules! checklist_write_op {
    ($op_name:ident, $method:literal, $params_ty:ty, $cmd_ty:ident) => {
        pub struct $op_name {
            pub actor: String,
            pub params: $params_ty,
        }

        impl StoreOp for $op_name {
            type Out = Result<Result<(TasksChecklistResult, bool), ApiError>, StoreError>;
            fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
                let op = task_op($method, &self.params.command_id, &self.actor, &self.params);
                let cmd = $cmd_ty {
                    actor_member_id: self.actor,
                    params: self.params,
                };
                let (reply, replayed) = match submit_task(store, op, move |uow| {
                    let (change, id) = cmd.decide(uow)?;
                    Ok(with_reply(
                        change,
                        &TaskCommitReply {
                            task_id: id.as_str().to_owned(),
                            newly_assigned_agent: None,
                        },
                    ))
                })? {
                    Ok(pair) => pair,
                    Err(api_err) => return Ok(Err(api_err)),
                };
                finish_checklist(store, reply, replayed)
            }
        }
    };
}

checklist_write_op!(
    TaskChecklistAddOp,
    "tasks.checklist.add",
    TasksChecklistAddParams,
    ChecklistAdd
);
checklist_write_op!(
    TaskChecklistCheckOp,
    "tasks.checklist.check",
    TasksChecklistCheckParams,
    ChecklistCheck
);
checklist_write_op!(
    TaskChecklistEditOp,
    "tasks.checklist.edit",
    TasksChecklistEditParams,
    ChecklistEdit
);
checklist_write_op!(
    TaskChecklistRemoveOp,
    "tasks.checklist.remove",
    TasksChecklistRemoveParams,
    ChecklistRemove
);
checklist_write_op!(
    TaskChecklistMoveOp,
    "tasks.checklist.move",
    TasksChecklistMoveParams,
    ChecklistMove
);

// --- ReadOps ---

pub struct TaskListRead {
    pub params: TasksListParams,
}

impl ReadOp for TaskListRead {
    type Out = Result<TasksListResult, ApiError>;
    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        Ok(features::list_tasks(snapshot, &self.params).map_err(|r| decode_refusal(&r)))
    }
}

pub struct TaskInspectRead {
    pub task: String,
}

impl ReadOp for TaskInspectRead {
    type Out = Result<TaskInspectResult, ApiError>;
    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        Ok(features::inspect_task(snapshot, &self.task).map_err(|r| decode_refusal(&r)))
    }
}

pub struct TaskDependenciesRead {
    pub task: String,
}

impl ReadOp for TaskDependenciesRead {
    type Out = Result<TaskDependenciesResult, ApiError>;
    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        Ok(features::task_dependencies(snapshot, &self.task).map_err(|r| decode_refusal(&r)))
    }
}

pub struct TaskCommentsRead {
    pub task: String,
}

impl ReadOp for TaskCommentsRead {
    type Out = Result<TasksCommentsResult, ApiError>;
    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        Ok(features::list_comments(snapshot, &self.task).map_err(|r| decode_refusal(&r)))
    }
}

pub struct TaskHistoryRead {
    pub task: String,
}

impl ReadOp for TaskHistoryRead {
    type Out = Result<TasksHistoryResult, ApiError>;
    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        Ok(features::list_history(snapshot, &self.task).map_err(|r| decode_refusal(&r)))
    }
}

pub struct TaskLabelsRead {
    pub params: TasksLabelsParams,
}

impl ReadOp for TaskLabelsRead {
    type Out = Result<TasksLabelsResult, ApiError>;
    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        Ok(features::list_labels(snapshot, &self.params).map_err(|r| decode_refusal(&r)))
    }
}
