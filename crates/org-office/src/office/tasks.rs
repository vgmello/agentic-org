//! High-level unified tasks facade on [`Office`] (`TASK-49`, `TASK-77`).

use org_proto::tasks::{
    TaskDependenciesResult, TaskInspectResult, TasksAddDependencyParams, TasksAssignParams,
    TasksChecklistAddParams, TasksChecklistCheckParams, TasksChecklistEditParams,
    TasksChecklistMoveParams, TasksChecklistRemoveParams, TasksChecklistResult, TasksCommentParams,
    TasksCommentResult, TasksCommentsResult, TasksCreateParams, TasksHistoryResult,
    TasksLabelsParams, TasksLabelsResult, TasksListParams, TasksListResult,
    TasksRemoveDependencyParams, TasksTransitionParams, TasksUnwatchParams, TasksUpdateParams,
    TasksWatchParams,
};
use org_proto::ApiError;

use crate::error::OfficeError;
use crate::office::{storage, Office};
use crate::store::open::task_ops::{
    TaskAddDependencyOp, TaskAssignOp, TaskChecklistAddOp, TaskChecklistCheckOp,
    TaskChecklistEditOp, TaskChecklistMoveOp, TaskChecklistRemoveOp, TaskCommentOp,
    TaskCommentsRead, TaskCreateOp, TaskDependenciesRead, TaskHistoryRead, TaskInspectRead,
    TaskLabelsRead, TaskListRead, TaskRemoveDependencyOp, TaskTransitionOp, TaskUnwatchOp,
    TaskUpdateOp, TaskWatchOp,
};
use crate::tasks::features::TaskWriteOutcome;

impl Office {
    pub fn task_create(
        &self,
        actor: &str,
        params: TasksCreateParams,
    ) -> Result<Result<TaskWriteOutcome, ApiError>, OfficeError> {
        self.store
            .run(TaskCreateOp {
                actor: actor.to_owned(),
                here: self.identity.clone(),
                params,
            })
            .map_err(storage)
    }

    pub fn task_update(
        &self,
        actor: &str,
        params: TasksUpdateParams,
    ) -> Result<Result<TaskWriteOutcome, ApiError>, OfficeError> {
        self.store
            .run(TaskUpdateOp {
                actor: actor.to_owned(),
                params,
            })
            .map_err(storage)
    }

    pub fn task_transition(
        &self,
        actor: &str,
        params: TasksTransitionParams,
    ) -> Result<Result<TaskWriteOutcome, ApiError>, OfficeError> {
        self.store
            .run(TaskTransitionOp {
                actor: actor.to_owned(),
                params,
            })
            .map_err(storage)
    }

    pub fn task_assign(
        &self,
        actor: &str,
        params: TasksAssignParams,
    ) -> Result<Result<TaskWriteOutcome, ApiError>, OfficeError> {
        self.store
            .run(TaskAssignOp {
                actor: actor.to_owned(),
                params,
            })
            .map_err(storage)
    }

    pub fn task_comment(
        &self,
        actor: &str,
        params: TasksCommentParams,
    ) -> Result<Result<(TasksCommentResult, bool), ApiError>, OfficeError> {
        self.store
            .run(TaskCommentOp {
                actor: actor.to_owned(),
                params,
            })
            .map_err(storage)
    }

    pub fn task_watch(
        &self,
        actor: &str,
        params: TasksWatchParams,
    ) -> Result<Result<TaskWriteOutcome, ApiError>, OfficeError> {
        self.store
            .run(TaskWatchOp {
                actor: actor.to_owned(),
                params,
            })
            .map_err(storage)
    }

    pub fn task_unwatch(
        &self,
        actor: &str,
        params: TasksUnwatchParams,
    ) -> Result<Result<TaskWriteOutcome, ApiError>, OfficeError> {
        self.store
            .run(TaskUnwatchOp {
                actor: actor.to_owned(),
                params,
            })
            .map_err(storage)
    }

    pub fn task_add_dependency(
        &self,
        actor: &str,
        params: TasksAddDependencyParams,
    ) -> Result<Result<TaskWriteOutcome, ApiError>, OfficeError> {
        self.store
            .run(TaskAddDependencyOp {
                actor: actor.to_owned(),
                params,
            })
            .map_err(storage)
    }

    pub fn task_remove_dependency(
        &self,
        actor: &str,
        params: TasksRemoveDependencyParams,
    ) -> Result<Result<TaskWriteOutcome, ApiError>, OfficeError> {
        self.store
            .run(TaskRemoveDependencyOp {
                actor: actor.to_owned(),
                params,
            })
            .map_err(storage)
    }

    pub fn task_checklist_add(
        &self,
        actor: &str,
        params: TasksChecklistAddParams,
    ) -> Result<Result<(TasksChecklistResult, bool), ApiError>, OfficeError> {
        self.store
            .run(TaskChecklistAddOp {
                actor: actor.to_owned(),
                params,
            })
            .map_err(storage)
    }

    pub fn task_checklist_check(
        &self,
        actor: &str,
        params: TasksChecklistCheckParams,
    ) -> Result<Result<(TasksChecklistResult, bool), ApiError>, OfficeError> {
        self.store
            .run(TaskChecklistCheckOp {
                actor: actor.to_owned(),
                params,
            })
            .map_err(storage)
    }

    pub fn task_checklist_edit(
        &self,
        actor: &str,
        params: TasksChecklistEditParams,
    ) -> Result<Result<(TasksChecklistResult, bool), ApiError>, OfficeError> {
        self.store
            .run(TaskChecklistEditOp {
                actor: actor.to_owned(),
                params,
            })
            .map_err(storage)
    }

    pub fn task_checklist_remove(
        &self,
        actor: &str,
        params: TasksChecklistRemoveParams,
    ) -> Result<Result<(TasksChecklistResult, bool), ApiError>, OfficeError> {
        self.store
            .run(TaskChecklistRemoveOp {
                actor: actor.to_owned(),
                params,
            })
            .map_err(storage)
    }

    pub fn task_checklist_move(
        &self,
        actor: &str,
        params: TasksChecklistMoveParams,
    ) -> Result<Result<(TasksChecklistResult, bool), ApiError>, OfficeError> {
        self.store
            .run(TaskChecklistMoveOp {
                actor: actor.to_owned(),
                params,
            })
            .map_err(storage)
    }

    pub fn task_list(
        &self,
        params: TasksListParams,
    ) -> Result<Result<TasksListResult, ApiError>, OfficeError> {
        let (res, _) = self.store.read(TaskListRead { params }).map_err(storage)?;
        Ok(res)
    }

    pub fn task_inspect(
        &self,
        task: &str,
    ) -> Result<Result<TaskInspectResult, ApiError>, OfficeError> {
        let (res, _) = self
            .store
            .read(TaskInspectRead {
                task: task.to_owned(),
            })
            .map_err(storage)?;
        Ok(res)
    }

    pub fn task_dependencies(
        &self,
        task: &str,
    ) -> Result<Result<TaskDependenciesResult, ApiError>, OfficeError> {
        let (res, _) = self
            .store
            .read(TaskDependenciesRead {
                task: task.to_owned(),
            })
            .map_err(storage)?;
        Ok(res)
    }

    pub fn task_comments(
        &self,
        task: &str,
    ) -> Result<Result<TasksCommentsResult, ApiError>, OfficeError> {
        let (res, _) = self
            .store
            .read(TaskCommentsRead {
                task: task.to_owned(),
            })
            .map_err(storage)?;
        Ok(res)
    }

    pub fn task_history(
        &self,
        task: &str,
    ) -> Result<Result<TasksHistoryResult, ApiError>, OfficeError> {
        let (res, _) = self
            .store
            .read(TaskHistoryRead {
                task: task.to_owned(),
            })
            .map_err(storage)?;
        Ok(res)
    }

    pub fn task_labels(
        &self,
        params: TasksLabelsParams,
    ) -> Result<Result<TasksLabelsResult, ApiError>, OfficeError> {
        let (res, _) = self
            .store
            .read(TaskLabelsRead { params })
            .map_err(storage)?;
        Ok(res)
    }
}
