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
use org_proto::{ApiError, ErrorCode};

use crate::error::OfficeError;
use crate::model::OfficeRole;
use crate::office::{storage, Office};
use crate::store::open::task_ops::{
    TaskAddDependencyOp, TaskAssignOp, TaskChecklistAddOp, TaskChecklistCheckOp,
    TaskChecklistEditOp, TaskChecklistMoveOp, TaskChecklistRemoveOp, TaskCommentOp,
    TaskCommentsRead, TaskCreateOp, TaskDependenciesRead, TaskHistoryRead, TaskInspectRead,
    TaskLabelsRead, TaskListRead, TaskRemoveDependencyOp, TaskTransitionOp, TaskUnwatchOp,
    TaskUpdateOp, TaskWatchOp,
};
use crate::tasks::features::TaskWriteOutcome;

/// Enforces single-office task writer authority BEFORE committing a task mutation (`TASK-52` P3-#9 /
/// P4-#6 / P5-#7):
/// - A satellite may only locally mutate tasks whose `office_code` belongs to that satellite.
/// - HQ may locally mutate tasks owned by HQ OR recovered tasks whose `office_code` was retired
///   (e.g. R1/D25 lost-HQ replacement), while refusing edits to tasks owned by an active satellite.
pub(super) fn ensure_local_task_write_authority(
    office: &Office,
    task_ref: &str,
) -> Result<Result<(), ApiError>, OfficeError> {
    if office.info.role == OfficeRole::Hq && !office.is_multi_office()? {
        return Ok(Ok(()));
    }
    let inspect = match office.task_inspect(task_ref)? {
        Ok(res) => res,
        // Let the underlying command return canonical `NotFound` / `InvalidRequest`;
        // fail closed on `StorageError` or any other non-lookup error (`TASK-192`).
        Err(api_err)
            if matches!(
                api_err.code,
                ErrorCode::NotFound | ErrorCode::InvalidRequest
            ) =>
        {
            return Ok(Ok(()));
        }
        Err(api_err) => return Ok(Err(api_err)),
    };
    if office.has_local_task_write_authority_for_code(&inspect.task.office_code)? {
        return Ok(Ok(()));
    }
    Ok(Err(ApiError::new(
        ErrorCode::NotAuthorized,
        format!(
            "office {} cannot locally modify task {} owned by office code {}; submit edits to the owning office",
            office.info.office_id, inspect.task.key, inspect.task.office_code
        ),
    )))
}

impl Office {
    /// Returns whether this office holds local write authority for a task numbered with
    /// `task_office_code` (`TASK-52`, `TASK-192`).
    pub fn has_local_task_write_authority_for_code(
        &self,
        task_office_code: &str,
    ) -> Result<bool, OfficeError> {
        if self.info.role == OfficeRole::Hq && !self.is_multi_office()? {
            return Ok(true);
        }
        crate::office::satellite::has_local_task_write_authority(self, task_office_code)
    }

    pub fn task_create(
        &self,
        actor: &str,
        params: TasksCreateParams,
    ) -> Result<Result<TaskWriteOutcome, ApiError>, OfficeError> {
        let res = self
            .store
            .run(TaskCreateOp {
                actor: actor.to_owned(),
                here: self.identity.clone(),
                params,
            })
            .map_err(storage)?;
        if res.is_ok() {
            let _ = self.sync_satellite_work_outbox();
        }
        Ok(res)
    }

    pub fn task_update(
        &self,
        actor: &str,
        params: TasksUpdateParams,
    ) -> Result<Result<TaskWriteOutcome, ApiError>, OfficeError> {
        if let Err(err) = ensure_local_task_write_authority(self, &params.task)? {
            return Ok(Err(err));
        }
        let res = self
            .store
            .run(TaskUpdateOp {
                actor: actor.to_owned(),
                params,
            })
            .map_err(storage)?;
        if res.is_ok() {
            let _ = self.sync_satellite_work_outbox();
        }
        Ok(res)
    }

    pub fn task_transition(
        &self,
        actor: &str,
        params: TasksTransitionParams,
    ) -> Result<Result<TaskWriteOutcome, ApiError>, OfficeError> {
        if let Err(err) = ensure_local_task_write_authority(self, &params.task)? {
            return Ok(Err(err));
        }
        let res = self
            .store
            .run(TaskTransitionOp {
                actor: actor.to_owned(),
                params,
            })
            .map_err(storage)?;
        if res.is_ok() {
            let _ = self.sync_satellite_work_outbox();
        }
        Ok(res)
    }

    pub fn task_assign(
        &self,
        actor: &str,
        params: TasksAssignParams,
    ) -> Result<Result<TaskWriteOutcome, ApiError>, OfficeError> {
        if let Err(err) = ensure_local_task_write_authority(self, &params.task)? {
            return Ok(Err(err));
        }
        let res = self
            .store
            .run(TaskAssignOp {
                actor: actor.to_owned(),
                params,
            })
            .map_err(storage)?;
        if res.is_ok() {
            let _ = self.sync_satellite_work_outbox();
        }
        Ok(res)
    }

    pub fn task_comment(
        &self,
        actor: &str,
        params: TasksCommentParams,
    ) -> Result<Result<(TasksCommentResult, bool), ApiError>, OfficeError> {
        if let Err(err) = ensure_local_task_write_authority(self, &params.task)? {
            return Ok(Err(err));
        }
        let res = self
            .store
            .run(TaskCommentOp {
                actor: actor.to_owned(),
                params,
            })
            .map_err(storage)?;
        if res.is_ok() {
            let _ = self.sync_satellite_work_outbox();
        }
        Ok(res)
    }

    pub fn task_watch(
        &self,
        actor: &str,
        params: TasksWatchParams,
    ) -> Result<Result<TaskWriteOutcome, ApiError>, OfficeError> {
        if let Err(err) = ensure_local_task_write_authority(self, &params.task)? {
            return Ok(Err(err));
        }
        let res = self
            .store
            .run(TaskWatchOp {
                actor: actor.to_owned(),
                params,
            })
            .map_err(storage)?;
        if res.is_ok() {
            let _ = self.sync_satellite_work_outbox();
        }
        Ok(res)
    }

    pub fn task_unwatch(
        &self,
        actor: &str,
        params: TasksUnwatchParams,
    ) -> Result<Result<TaskWriteOutcome, ApiError>, OfficeError> {
        if let Err(err) = ensure_local_task_write_authority(self, &params.task)? {
            return Ok(Err(err));
        }
        let res = self
            .store
            .run(TaskUnwatchOp {
                actor: actor.to_owned(),
                params,
            })
            .map_err(storage)?;
        if res.is_ok() {
            let _ = self.sync_satellite_work_outbox();
        }
        Ok(res)
    }

    pub fn task_add_dependency(
        &self,
        actor: &str,
        params: TasksAddDependencyParams,
    ) -> Result<Result<TaskWriteOutcome, ApiError>, OfficeError> {
        if let Err(err) = ensure_local_task_write_authority(self, &params.task)? {
            return Ok(Err(err));
        }
        let res = self
            .store
            .run(TaskAddDependencyOp {
                actor: actor.to_owned(),
                params,
            })
            .map_err(storage)?;
        if res.is_ok() {
            let _ = self.sync_satellite_work_outbox();
        }
        Ok(res)
    }

    pub fn task_remove_dependency(
        &self,
        actor: &str,
        params: TasksRemoveDependencyParams,
    ) -> Result<Result<TaskWriteOutcome, ApiError>, OfficeError> {
        if let Err(err) = ensure_local_task_write_authority(self, &params.task)? {
            return Ok(Err(err));
        }
        let res = self
            .store
            .run(TaskRemoveDependencyOp {
                actor: actor.to_owned(),
                params,
            })
            .map_err(storage)?;
        if res.is_ok() {
            let _ = self.sync_satellite_work_outbox();
        }
        Ok(res)
    }

    pub fn task_checklist_add(
        &self,
        actor: &str,
        params: TasksChecklistAddParams,
    ) -> Result<Result<(TasksChecklistResult, bool), ApiError>, OfficeError> {
        if let Err(err) = ensure_local_task_write_authority(self, &params.task)? {
            return Ok(Err(err));
        }
        let res = self
            .store
            .run(TaskChecklistAddOp {
                actor: actor.to_owned(),
                params,
            })
            .map_err(storage)?;
        if res.is_ok() {
            let _ = self.sync_satellite_work_outbox();
        }
        Ok(res)
    }

    pub fn task_checklist_check(
        &self,
        actor: &str,
        params: TasksChecklistCheckParams,
    ) -> Result<Result<(TasksChecklistResult, bool), ApiError>, OfficeError> {
        if let Err(err) = ensure_local_task_write_authority(self, &params.task)? {
            return Ok(Err(err));
        }
        let res = self
            .store
            .run(TaskChecklistCheckOp {
                actor: actor.to_owned(),
                params,
            })
            .map_err(storage)?;
        if res.is_ok() {
            let _ = self.sync_satellite_work_outbox();
        }
        Ok(res)
    }

    pub fn task_checklist_edit(
        &self,
        actor: &str,
        params: TasksChecklistEditParams,
    ) -> Result<Result<(TasksChecklistResult, bool), ApiError>, OfficeError> {
        if let Err(err) = ensure_local_task_write_authority(self, &params.task)? {
            return Ok(Err(err));
        }
        let res = self
            .store
            .run(TaskChecklistEditOp {
                actor: actor.to_owned(),
                params,
            })
            .map_err(storage)?;
        if res.is_ok() {
            let _ = self.sync_satellite_work_outbox();
        }
        Ok(res)
    }

    pub fn task_checklist_remove(
        &self,
        actor: &str,
        params: TasksChecklistRemoveParams,
    ) -> Result<Result<(TasksChecklistResult, bool), ApiError>, OfficeError> {
        if let Err(err) = ensure_local_task_write_authority(self, &params.task)? {
            return Ok(Err(err));
        }
        let res = self
            .store
            .run(TaskChecklistRemoveOp {
                actor: actor.to_owned(),
                params,
            })
            .map_err(storage)?;
        if res.is_ok() {
            let _ = self.sync_satellite_work_outbox();
        }
        Ok(res)
    }

    pub fn task_checklist_move(
        &self,
        actor: &str,
        params: TasksChecklistMoveParams,
    ) -> Result<Result<(TasksChecklistResult, bool), ApiError>, OfficeError> {
        if let Err(err) = ensure_local_task_write_authority(self, &params.task)? {
            return Ok(Err(err));
        }
        let res = self
            .store
            .run(TaskChecklistMoveOp {
                actor: actor.to_owned(),
                params,
            })
            .map_err(storage)?;
        if res.is_ok() {
            let _ = self.sync_satellite_work_outbox();
        }
        Ok(res)
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
