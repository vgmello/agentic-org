//! Engine-independent `StoreOp` and `ReadOp` wrappers for Git repositories, project-repository
//! links, and per-task repository branch recording (`TASK-51`, `C7`).

use std::collections::BTreeSet;

use org_proto::{ApiError, ErrorCode};
use org_store::{Actor, Id, Operation, Outcome, Refusal, Sha256, Store, StoreError};
use serde_json::json;

use crate::organization::ports::OrganizationRead;
use crate::organization::records::{Employee, Repository};
use crate::projects::features::{resolve_repository, AttachProjectRepository, RegisterRepository};
use crate::projects::records::Project;
use crate::tasks::features::{
    decode_refusal, parse_uuid_v7, require_active_employee, require_task, rewrite_task,
    storage_refusal,
};
use crate::tasks::ports::TasksRead;
use crate::tasks::records::{TaskRecord, TaskRepositoryRecord};
use crate::time::format_rfc3339_ms;

use super::operations::ProjectOp;
use super::{OfficeEngine, ReadOp, StoreOp};

pub type RegisterRepositoryOp = ProjectOp<RegisterRepository>;
pub type AttachProjectRepositoryOp = ProjectOp<AttachProjectRepository>;

/// Reads all registered repositories in `repositories.yaml` order.
pub struct RepositoriesList;

impl ReadOp for RepositoriesList {
    type Out = Vec<Repository>;

    fn read<S: TasksRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        Ok(read.repositories()?)
    }
}

/// Resolves one repository by UUIDv7 ID or canonical key.
pub struct RepositoryResolve(pub String);

impl ReadOp for RepositoryResolve {
    type Out = Option<Repository>;

    fn read<S: TasksRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        let repos = read.repositories()?;
        Ok(resolve_repository(&repos, &self.0).cloned())
    }
}

/// Snapshot of everything the office Git service needs to prepare a worktree or push a branch.
#[derive(Debug, Clone)]
pub struct TaskGitContext {
    pub task: TaskRecord,
    pub project: Option<Project>,
    pub caller: Employee,
    pub caller_team_chain: Vec<Id>,
    /// Target workspace owner (`task.assignee_id` if assigned, else `caller.id`).
    pub workspace_owner_id: Id,
    /// Team chain for `workspace_owner_id`'s `home_team_id` (`doc-35` §What lives where).
    pub workspace_owner_team_chain: Vec<Id>,
    /// Whether `caller` belongs to the task's owning, assigned, or project-contributing team.
    pub caller_in_task_team: bool,
    pub repositories: Vec<Repository>,
    /// All `integration_branch` names across every project in the organization, plus `main`/`master`.
    pub forbidden_branches: Vec<String>,
}

/// Loads [`TaskGitContext`] in one consistent snapshot read.
pub struct TaskGitContextRead {
    pub caller_id: String,
    pub task_ref: String,
}

impl ReadOp for TaskGitContextRead {
    type Out = Result<TaskGitContext, ApiError>;

    fn read<S: TasksRead>(self, read: &S) -> Result<Self::Out, StoreError> {
        let caller = match require_active_employee(read, &self.caller_id) {
            Ok(emp) => emp,
            Err(r) => return Ok(Err(decode_refusal(&r))),
        };
        let task = match require_task(read, &self.task_ref) {
            Ok(t) => t,
            Err(r) => return Ok(Err(decode_refusal(&r))),
        };
        let project = match &task.project_id {
            Some(pid) => read.project(pid)?,
            None => None,
        };
        let caller_team_chain = read
            .team_chain(&caller.home_team_id)?
            .unwrap_or_else(|| vec![caller.home_team_id.clone()]);

        let (workspace_owner_id, workspace_owner_team_chain) = match &task.assignee_id {
            Some(assignee_id) => {
                let assignee = read.employee(assignee_id)?;
                let chain = match assignee {
                    Some(emp) => read
                        .team_chain(&emp.home_team_id)?
                        .unwrap_or_else(|| vec![emp.home_team_id]),
                    None => caller_team_chain.clone(),
                };
                (assignee_id.clone(), chain)
            }
            None => (caller.id.clone(), caller_team_chain.clone()),
        };

        let caller_in_task_team = caller.home_team_id == task.team_id
            || task.assigned_team_id.as_ref() == Some(&caller.home_team_id)
            || project
                .as_ref()
                .is_some_and(|p| p.contributing_team_ids.contains(&caller.home_team_id));

        let repositories = read.repositories()?;
        let mut forbidden: BTreeSet<String> = BTreeSet::new();
        forbidden.insert("main".into());
        forbidden.insert("master".into());
        for p in read.projects()? {
            for link in p.repositories {
                forbidden.insert(link.integration_branch);
            }
        }
        Ok(Ok(TaskGitContext {
            task,
            project,
            caller,
            caller_team_chain,
            workspace_owner_id,
            workspace_owner_team_chain,
            caller_in_task_team,
            repositories,
            forbidden_branches: forbidden.into_iter().collect(),
        }))
    }
}

/// Records or updates one `(repository_id, branch, integrated_commit)` entry on `task.md`
/// (projected into `task_repositories`) alongside an immutable `task_events` entry.
pub struct RecordTaskRepoBranchOp {
    pub actor: String,
    pub command_id: String,
    pub task_id: Id,
    pub repository_id: Id,
    pub branch: String,
    pub integrated_commit: Option<String>,
    /// When `Some(office_id)`, atomically verifies inside the store `UnitOfWork` that the target
    /// task's `office_code` belongs to `office_id` (eliminating TOCTOU on satellites, `TASK-192`).
    pub enforce_satellite_office_id: Option<Id>,
}

fn rejected_api_error(code: ErrorCode, message: impl Into<String>) -> Refusal {
    Refusal::rejected(serde_json::to_vec(&ApiError::new(code, message)).unwrap())
}

fn verify_satellite_branch_authority(
    view: &impl OrganizationRead,
    sat_office_id: &Id,
    task: &TaskRecord,
) -> Result<(), Refusal> {
    let codes = view.office_codes().map_err(storage_refusal)?;
    let owns_code = codes
        .iter()
        .any(|c| c.office_id == *sat_office_id && c.code == task.office_code);
    if owns_code {
        return Ok(());
    }
    Err(rejected_api_error(
        ErrorCode::NotAuthorized,
        format!(
            "office {} cannot locally modify task {} owned by office code {}; submit edits to the owning office",
            sat_office_id.as_str(),
            task.key(),
            task.office_code
        ),
    ))
}

fn upsert_task_repository_link(
    task: &mut TaskRecord,
    repository_id: &Id,
    branch: &str,
    integrated_commit: &Option<String>,
) -> Result<(), Refusal> {
    if let Some(existing) = task
        .repositories
        .iter_mut()
        .find(|r| r.repository_id == *repository_id)
    {
        if existing.integrated_commit.is_some() && integrated_commit.is_none() {
            return Err(rejected_api_error(
                ErrorCode::Conflict,
                "task branch is already recorded as integrated",
            ));
        }
        existing.branch = Some(branch.to_owned());
        if let Some(ref sha) = integrated_commit {
            existing.integrated_commit = Some(sha.clone());
        }
        return Ok(());
    }
    task.repositories.push(TaskRepositoryRecord {
        repository_id: repository_id.clone(),
        branch: Some(branch.to_owned()),
        integrated_commit: integrated_commit.clone(),
    });
    task.repositories
        .sort_by(|a, b| a.repository_id.cmp(&b.repository_id));
    Ok(())
}

impl StoreOp for RecordTaskRepoBranchOp {
    type Out = Result<Result<(), ApiError>, StoreError>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let cmd_id = match parse_uuid_v7(&self.command_id, "command_id") {
            Ok(id) => id,
            Err(r) => return Ok(Err(decode_refusal(&r))),
        };
        let actor_val = Id::parse(&self.actor)
            .map(Actor::Employee)
            .unwrap_or(Actor::System);
        let fingerprint_bytes = serde_json::to_vec(&json!({
            "task_id": self.task_id.as_str(),
            "repository_id": self.repository_id.as_str(),
            "branch": &self.branch,
            "integrated_commit": &self.integrated_commit,
        }))
        .unwrap_or_default();
        let op = Operation {
            id: cmd_id,
            kind: "tasks.record_repository_branch",
            actor: actor_val,
            fingerprint: Sha256::of_parts(&[
                b"tasks.record_repository_branch\0",
                &fingerprint_bytes,
            ]),
        };

        let Self {
            actor,
            command_id,
            task_id,
            repository_id,
            branch,
            integrated_commit,
            enforce_satellite_office_id,
        } = self;

        let reply = match store.submit(Some(op), false, move |uow| {
            let emp = require_active_employee(uow.view(), &actor)?;
            let mut task = uow
                .view()
                .task(&task_id)
                .map_err(storage_refusal)?
                .ok_or_else(|| {
                    rejected_api_error(ErrorCode::NotFound, format!("task `{task_id}` not found"))
                })?;
            if let Some(ref sat_office_id) = enforce_satellite_office_id {
                verify_satellite_branch_authority(uow.view(), sat_office_id, &task)?;
            }
            let repos = uow.view().repositories().map_err(storage_refusal)?;
            if !repos.iter().any(|r| r.id == repository_id) {
                return Err(rejected_api_error(
                    ErrorCode::NotFound,
                    format!("repository `{repository_id}` not found"),
                ));
            }
            upsert_task_repository_link(&mut task, &repository_id, &branch, &integrated_commit)?;
            let at = format_rfc3339_ms(uow.now().0);
            task.meta.touch(&emp.id, &at);
            rewrite_task(
                uow.view(),
                &task,
                &emp.id,
                &at,
                &command_id,
                json!({
                    "type": "task_repository_branch_recorded",
                    "repository_id": repository_id.as_str(),
                    "branch": &branch,
                    "integrated_commit": &integrated_commit,
                }),
            )
        }) {
            Ok(reply) => reply,
            Err(StoreError::Conflict) => {
                return Ok(Err(ApiError::new(
                    ErrorCode::OperationReused,
                    "command_id reused with different request",
                )));
            }
            Err(e) => return Err(e),
        };

        match reply.outcome {
            Outcome::Completed => Ok(Ok(())),
            Outcome::Rejected => Ok(Err(decode_refusal(&Refusal::Rejected(reply.bytes)))),
        }
    }
}
