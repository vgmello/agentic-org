//! High-level repository, project-repository, and task Git context facade on [`Office`]
//! (`TASK-51`, `C7`, `doc-35`).

use std::path::Path;

use org_proto::git::{ProjectRepositoryLinkWire, RepositoryKindWire};
use org_proto::{ApiError, ErrorCode};
use org_store::{Actor, Id, Outcome, StoreError};

use crate::error::OfficeError;
use crate::git;
use crate::office::{storage, Office};
use crate::organization::features::{OrgRejection, RejectCode};
use crate::organization::records::{is_stable_id, Repository};
use crate::projects::features::{resolve_repository, AttachProjectRepository, RegisterRepository};
use crate::store::open::{
    AttachProjectRepositoryOp, ProjectInspectByRef, RecordTaskRepoBranchOp, RegisterRepositoryOp,
    RepositoriesList, RepositoryResolve, TaskGitContext, TaskGitContextRead,
};

/// One listed repository with optional per-project `integration_branch`.
#[derive(Debug, Clone)]
pub struct ListedRepository {
    pub record: Repository,
    pub integration_branch: Option<String>,
}

fn rejection_to_api(rej: OrgRejection) -> ApiError {
    let code = match rej.code {
        RejectCode::NotFound => ErrorCode::NotFound,
        RejectCode::InvalidRequest => ErrorCode::InvalidRequest,
        RejectCode::InvalidName => ErrorCode::InvalidName,
        RejectCode::OwnerProtected => ErrorCode::OwnerProtected,
        RejectCode::Cycle => ErrorCode::Cycle,
        RejectCode::NotEmpty => ErrorCode::NotEmpty,
        RejectCode::Conflict => ErrorCode::Conflict,
        RejectCode::AlreadyExists => ErrorCode::AlreadyExists,
    };
    ApiError::new(code, rej.message)
}

fn decode_org_reply(bytes: &[u8]) -> ApiError {
    serde_json::from_slice::<OrgRejection>(bytes)
        .map(rejection_to_api)
        .unwrap_or_else(|_| {
            ApiError::new(ErrorCode::InvalidRequest, "repository operation refused")
        })
}

impl Office {
    /// The organization's runtime data root (`<data-root>/<org-id>/runtime`).
    pub fn runtime_root(&self) -> &Path {
        &self.runtime_root
    }

    /// Registers a remote or local repository in `state/repositories.yaml` (owner-only).
    pub fn register_repository(
        &self,
        member: &str,
        command_id: &str,
        repository_id: &str,
        key: Option<String>,
        kind: RepositoryKindWire,
        remote_url: Option<String>,
    ) -> Result<Result<Repository, ApiError>, OfficeError> {
        if member != self.owner.as_str() {
            return Ok(Err(ApiError::new(
                ErrorCode::NotAuthorized,
                crate::organization::features::NOT_AUTHORIZED,
            )));
        }
        if !is_stable_id(command_id) {
            return Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "command_id must be a canonical lowercase UUIDv7",
            )));
        }
        if !is_stable_id(repository_id) {
            return Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "repository_id must be a canonical lowercase UUIDv7",
            )));
        }
        let op_id = Id::parse(command_id)
            .map_err(|_| OfficeError::Invalid("validated command_id failed Id::parse"))?;
        let repo_id = Id::parse(repository_id)
            .map_err(|_| OfficeError::Invalid("validated repository_id failed Id::parse"))?;
        let actor_id = Id::parse(member)
            .map_err(|_| OfficeError::Invalid("validated owner member failed Id::parse"))?;
        let op = RegisterRepositoryOp {
            operation_id: op_id,
            here: self.identity.clone(),
            owner: self.owner.clone(),
            by: Actor::Employee(actor_id),
            command: RegisterRepository {
                id: repo_id.clone(),
                key,
                kind: kind.as_str().to_owned(),
                remote_url,
            },
        };
        let reply = match self.store.run(op) {
            Ok(reply) => reply,
            Err(StoreError::Conflict) => {
                return Ok(Err(ApiError::new(
                    ErrorCode::OperationReused,
                    "command_id reused with different request",
                )));
            }
            Err(e) => return Err(storage(e)),
        };
        if reply.outcome == Outcome::Rejected {
            return Ok(Err(decode_org_reply(&reply.bytes)));
        }
        let found = self
            .resolve_repository(repo_id.as_str())?
            .ok_or_else(|| OfficeError::Invalid("registered repository missing from projection"))?;
        Ok(Ok(found))
    }

    /// Attaches a registered repository to a project with a verified `integration_branch`
    /// (owner-only; caller is responsible for verifying the branch via hardened `git ls-remote`).
    pub fn attach_project_repository(
        &self,
        member: &str,
        command_id: &str,
        project_ref: &str,
        repository_ref: &str,
        integration_branch: String,
    ) -> Result<Result<ProjectRepositoryLinkWire, ApiError>, OfficeError> {
        if member != self.owner.as_str() {
            return Ok(Err(ApiError::new(
                ErrorCode::NotAuthorized,
                crate::organization::features::NOT_AUTHORIZED,
            )));
        }
        if !is_stable_id(command_id) {
            return Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "command_id must be a canonical lowercase UUIDv7",
            )));
        }
        let op_id = Id::parse(command_id)
            .map_err(|_| OfficeError::Invalid("validated command_id failed Id::parse"))?;
        let actor_id = Id::parse(member)
            .map_err(|_| OfficeError::Invalid("validated owner member failed Id::parse"))?;

        let (project, _) = self
            .store
            .read(ProjectInspectByRef(project_ref.to_owned()))
            .map_err(storage)?;
        let Some(project) = project else {
            return Ok(Err(ApiError::new(ErrorCode::NotFound, "project not found")));
        };
        let Some(repo) = self.resolve_repository(repository_ref)? else {
            return Ok(Err(ApiError::new(
                ErrorCode::NotFound,
                "repository not found",
            )));
        };

        let op = AttachProjectRepositoryOp {
            operation_id: op_id,
            here: self.identity.clone(),
            owner: self.owner.clone(),
            by: Actor::Employee(actor_id),
            command: AttachProjectRepository {
                project_id: project.id.clone(),
                repository_id: repo.id.clone(),
                integration_branch: integration_branch.clone(),
            },
        };
        let reply = match self.store.run(op) {
            Ok(reply) => reply,
            Err(StoreError::Conflict) => {
                return Ok(Err(ApiError::new(
                    ErrorCode::OperationReused,
                    "command_id reused with different request",
                )));
            }
            Err(e) => return Err(storage(e)),
        };
        if reply.outcome == Outcome::Rejected {
            return Ok(Err(decode_org_reply(&reply.bytes)));
        }
        let (updated_proj, _) = self
            .store
            .read(ProjectInspectByRef(project.id.as_str().to_owned()))
            .map_err(storage)?;
        let rev = updated_proj.map_or(project.meta.revision + 1, |p| p.meta.revision);
        Ok(Ok(ProjectRepositoryLinkWire {
            project_id: project.id.as_str().to_owned(),
            project_key: project.key(),
            repository_id: repo.id.as_str().to_owned(),
            repository_key: repo.key,
            integration_branch: integration_branch.trim().to_owned(),
            revision: rev,
        }))
    }

    /// Lists organization repositories, optionally filtered to those attached to `project_ref`.
    pub fn list_repositories(
        &self,
        project_ref: Option<&str>,
    ) -> Result<Result<Vec<ListedRepository>, ApiError>, OfficeError> {
        let (repos, _) = self.store.read(RepositoriesList).map_err(storage)?;
        let Some(pref) = project_ref else {
            return Ok(Ok(repos
                .into_iter()
                .map(|record| ListedRepository {
                    record,
                    integration_branch: None,
                })
                .collect()));
        };
        let (project, _) = self
            .store
            .read(ProjectInspectByRef(pref.to_owned()))
            .map_err(storage)?;
        let Some(project) = project else {
            return Ok(Err(ApiError::new(ErrorCode::NotFound, "project not found")));
        };
        let mut out = Vec::new();
        for link in &project.repositories {
            if let Some(repo) = resolve_repository(&repos, link.repository_id.as_str()) {
                out.push(ListedRepository {
                    record: repo.clone(),
                    integration_branch: Some(link.integration_branch.clone()),
                });
            }
        }
        Ok(Ok(out))
    }

    /// Resolves a repository by UUIDv7 ID or canonical key.
    pub fn resolve_repository(&self, reference: &str) -> Result<Option<Repository>, OfficeError> {
        let (found, _) = self
            .store
            .read(RepositoryResolve(reference.to_owned()))
            .map_err(storage)?;
        Ok(found)
    }

    /// Loads task + project + team chain + repositories + forbidden integration branches in one
    /// consistent snapshot read for worktree preparation or push authorization.
    pub fn task_git_context(
        &self,
        caller_id: &str,
        task_ref: &str,
    ) -> Result<Result<TaskGitContext, ApiError>, OfficeError> {
        let (res, _) = self
            .store
            .read(TaskGitContextRead {
                caller_id: caller_id.to_owned(),
                task_ref: task_ref.to_owned(),
            })
            .map_err(storage)?;
        Ok(res)
    }

    /// Records a task's Git branch (and optional integrated commit) durably in `task.json`
    /// (`task_repositories` projection + immutable `task_events`).
    pub fn record_task_repository_branch(
        &self,
        actor: &str,
        command_id: &str,
        task_id: &Id,
        repository_id: &Id,
        branch: String,
        integrated_commit: Option<String>,
    ) -> Result<Result<(), ApiError>, OfficeError> {
        self.store
            .run(RecordTaskRepoBranchOp {
                actor: actor.to_owned(),
                command_id: command_id.to_owned(),
                task_id: task_id.clone(),
                repository_id: repository_id.clone(),
                branch,
                integrated_commit,
            })
            .map_err(storage)
    }

    /// Stores the office-local `origin_path` for a `kind: local` repository.
    pub fn write_local_repo_origin(
        &self,
        repository_id: &Id,
        origin_path: &str,
    ) -> Result<(), OfficeError> {
        git::write_local_repo_origin(
            &self.durable,
            &self.identity.office_id,
            repository_id,
            origin_path,
        )
        .map_err(OfficeError::storage)
    }

    /// Reads the office-local `origin_path` for a `kind: local` repository, if bound here.
    pub fn read_local_repo_origin(
        &self,
        repository_id: &Id,
    ) -> Result<Option<String>, OfficeError> {
        git::read_local_repo_origin(&self.durable, &self.identity.office_id, repository_id)
            .map_err(OfficeError::storage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::open::EngineChoice;
    use crate::TestOffice;
    use org_proto::org::ProjectCreateParams;
    use org_proto::tasks::TasksCreateParams;

    fn run_parity_suite(engine: EngineChoice) {
        let office = Office::for_test(TestOffice { engine });
        let owner = office.owner_member_id().to_owned();
        let root_team = office.org_snapshot().unwrap().teams[0].id.clone();
        let uuid = |n: u64| format!("018bce83-1200-7000-8000-{n:012x}");

        // Domain-level URL allowlist refuses file://, http://, ext::, and leading `-`.
        for (idx, bad) in [
            "file:///tmp/repo.git",
            "http://example.com/repo.git",
            "ext::sh -c evil",
            "ssh://-oProxyCommand=evil/repo",
        ]
        .into_iter()
        .enumerate()
        {
            let err = office
                .register_repository(
                    &owner,
                    &uuid(100 + idx as u64),
                    &uuid(200 + idx as u64),
                    Some("bad".into()),
                    RepositoryKindWire::Remote,
                    Some(bad.into()),
                )
                .unwrap()
                .unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidRequest, "must refuse {bad}");
        }

        // Register valid remote repository.
        let repo_id = uuid(10);
        let repo = office
            .register_repository(
                &owner,
                &uuid(11),
                &repo_id,
                Some("backend".into()),
                RepositoryKindWire::Remote,
                Some("https://github.com/acme/backend.git".into()),
            )
            .unwrap()
            .unwrap();
        assert_eq!(repo.key.as_deref(), Some("backend"));

        // Create project and attach repository.
        let project_id = uuid(20);
        let proj_params: ProjectCreateParams = serde_json::from_value(serde_json::json!({
            "command_id": uuid(21),
            "project_id": project_id,
            "name": "Alpha",
            "slug": "alpha",
            "owner_team_id": root_team,
            "contributing_team_ids": [],
        }))
        .unwrap();
        office
            .submit_org(&owner, crate::rules::OrgCommand::ProjectCreate(proj_params))
            .unwrap()
            .unwrap();
        let link = office
            .attach_project_repository(&owner, &uuid(22), "alpha", "backend", "main".into())
            .unwrap()
            .unwrap();
        assert_eq!(link.integration_branch, "main");

        // Create task and record task repository branch.
        let task_id = uuid(30);
        let task = office
            .task_create(
                &owner,
                TasksCreateParams {
                    command_id: uuid(31),
                    task_id: task_id.clone(),
                    team_id: Some(root_team),
                    assigned_team_id: None,
                    project_id: Some(project_id),
                    task_type: Some("task".into()),
                    work_type: None,
                    title: "Git parity test".into(),
                    body_markdown: String::new(),
                    assignee_id: Some(owner.clone()),
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
            .unwrap()
            .unwrap();

        office
            .record_task_repository_branch(
                &owner,
                &uuid(32),
                &Id::parse(&task.task.id).unwrap(),
                &Id::parse(&repo_id).unwrap(),
                "task/T-A1-git-parity-test".into(),
                None,
            )
            .unwrap()
            .unwrap();

        let ctx = office
            .task_git_context(&owner, &task.task.key)
            .unwrap()
            .unwrap();
        assert_eq!(ctx.task.repositories.len(), 1);
        assert_eq!(
            ctx.task.repositories[0].branch.as_deref(),
            Some("task/T-A1-git-parity-test")
        );
        assert_eq!(ctx.project.unwrap().repositories.len(), 1);
    }

    #[test]
    fn memory_and_sqlite_project_and_task_repository_parity() {
        run_parity_suite(EngineChoice::Memory);
        run_parity_suite(EngineChoice::Sqlite);
    }
}
