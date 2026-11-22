//! Registering shared organization repositories (`state/repositories.yaml`) and attaching
//! repositories to projects with a verified `integration_branch` (`state/projects/<id>/project.yaml`,
//! `TASK-51`, `C7`, `doc-35`).

use org_store::{Actor, Change, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};
use serde::Serialize;

use crate::organization::{
    features::{
        changed,
        shared::{admit, id_in_use, reject, touched},
        stable, Op, RejectCode,
    },
    ports::OrganizationRead,
    records::{
        self as org_records, encode_repositories, is_branch, is_line, is_remote_url,
        repositories_path, Meta, Repository, RepositoryKind,
    },
};
use crate::projects::{
    ports::ProjectsRead,
    records::{encode_project, project_path, ProjectRepositoryLink},
};

use super::ProjectFeature;

fn valid_repo_host(h: &str) -> bool {
    !h.is_empty() && !h.starts_with('-') && !h.contains(['/', '?', '#', '@', '\\'])
}

fn valid_repo_user(u: &str) -> bool {
    !u.is_empty() && !u.starts_with('-') && !u.contains([':', '/', '?', '#', '@', '\\'])
}

fn valid_ssh_scheme_repo_url(rest: &str) -> bool {
    let Some((authority, repo_path)) = rest.split_once('/') else {
        return false;
    };
    if authority.is_empty() || repo_path.is_empty() || repo_path.starts_with('-') {
        return false;
    }
    let host_port = match authority.split_once('@') {
        Some((userinfo, host_part)) if valid_repo_user(userinfo) => host_part,
        Some(_) => return false,
        None => authority,
    };
    let host = host_port.split_once(':').map_or(host_port, |(h, _)| h);
    valid_repo_host(host)
}

fn valid_scp_repo_url(url: &str) -> bool {
    if url.contains("://") || url.contains("::") {
        return false;
    }
    let Some((user_host, repo_path)) = url.split_once(':') else {
        return false;
    };
    if user_host.contains(['/', '?', '#', '\\'])
        || repo_path.is_empty()
        || repo_path.starts_with('-')
    {
        return false;
    }
    let Some((user, host)) = user_host.split_once('@') else {
        return false;
    };
    valid_repo_user(user) && valid_repo_host(host)
}

/// Enforces the `doc-35` transport allowlist (`https://` and SSH only, no embedded password,
/// no option-injection `-` prefixes, no local-path SCP confusion).
pub fn is_allowed_remote_repo_url(raw: &str) -> bool {
    let url = raw.trim();
    if url.is_empty()
        || url.len() > org_records::MAX_URL_BYTES
        || url.starts_with('-')
        || url.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return false;
    }
    if let Some(rest) = url.strip_prefix("https://") {
        return valid_repo_host(rest.split('/').next().unwrap_or(""));
    }
    if let Some(rest) = url.strip_prefix("ssh://") {
        return valid_ssh_scheme_repo_url(rest);
    }
    valid_scp_repo_url(url)
}

/// Registers a shared remote or local-only repository in `state/repositories.yaml` (owner-only).
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct RegisterRepository {
    pub id: Id,
    pub key: Option<String>,
    /// `"remote"` or `"local"`.
    pub kind: String,
    pub remote_url: Option<String>,
}

impl ProjectFeature for RegisterRepository {
    const OP: Op = Op::RegisterRepository;

    fn decide<E: Engine>(
        &self,
        uow: &mut UnitOfWork<'_, E>,
        here: &OfficeIdentity,
        owner: &Id,
        by: &Actor,
    ) -> Result<Change, Refusal>
    where
        E::Tx: ProjectsRead,
    {
        stable(&self.id, "the repository's ID")?;
        let admitted = admit(uow, here, owner, by)?;
        let view = uow.view();
        if id_in_use(view, &admitted.organization, &self.id)? {
            return reject(RejectCode::AlreadyExists, "the repository's ID is in use");
        }
        let kind = match self.kind.as_str() {
            "remote" => RepositoryKind::Remote,
            "local" => RepositoryKind::Local,
            _ => {
                return reject(
                    RejectCode::InvalidRequest,
                    "repository kind must be `remote` or `local`",
                );
            }
        };
        let key = match self.key.as_deref() {
            Some(raw) => {
                let trimmed = raw.trim();
                if !is_line(trimmed) {
                    return reject(
                        RejectCode::InvalidName,
                        "repository key must be a single line of text",
                    );
                }
                Some(trimmed.to_owned())
            }
            None => None,
        };
        let remote_url = match (kind, self.remote_url.as_deref()) {
            (RepositoryKind::Remote, Some(url)) => {
                let trimmed = url.trim();
                if !is_remote_url(trimmed) || !is_allowed_remote_repo_url(trimmed) {
                    return reject(
                        RejectCode::InvalidRequest,
                        "remote repository URL must be a valid https:// or ssh URL",
                    );
                }
                Some(trimmed.to_owned())
            }
            (RepositoryKind::Remote, None) => {
                return reject(
                    RejectCode::InvalidRequest,
                    "remote repository requires a remote_url",
                );
            }
            (RepositoryKind::Local, None) => None,
            (RepositoryKind::Local, Some(_)) => {
                return reject(
                    RejectCode::InvalidRequest,
                    "local repository must not specify remote_url",
                );
            }
        };

        let mut existing = view.repositories()?;
        if existing.iter().any(|r| r.id == self.id) {
            return reject(
                RejectCode::AlreadyExists,
                "a repository with that ID is already registered",
            );
        }
        if let Some(ref k) = key {
            if existing
                .iter()
                .any(|r| r.key.as_deref() == Some(k.as_str()))
            {
                return reject(
                    RejectCode::AlreadyExists,
                    "a repository with that key is already registered",
                );
            }
        }
        let record = Repository {
            id: self.id.clone(),
            key,
            kind,
            remote_url,
            meta: Meta::created(&admitted.by, admitted.now),
        };
        existing.push(record);
        let reply = changed(&self.id, 1, admitted.generation);
        Ok(Change::new(Op::RegisterRepository.ack())
            .put(repositories_path(), encode_repositories(&existing))
            .reply(reply))
    }
}

/// Attaches a registered repository to an active project with a verified `integration_branch`.
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct AttachProjectRepository {
    pub project_id: Id,
    pub repository_id: Id,
    pub integration_branch: String,
}

impl ProjectFeature for AttachProjectRepository {
    const OP: Op = Op::AttachProjectRepository;

    fn decide<E: Engine>(
        &self,
        uow: &mut UnitOfWork<'_, E>,
        here: &OfficeIdentity,
        owner: &Id,
        by: &Actor,
    ) -> Result<Change, Refusal>
    where
        E::Tx: ProjectsRead,
    {
        stable(&self.project_id, "the project's ID")?;
        stable(&self.repository_id, "the repository's ID")?;
        let admitted = admit(uow, here, owner, by)?;
        let view = uow.view();
        let Some(mut project) = view.project(&self.project_id)? else {
            return reject(RejectCode::NotFound, "no such project");
        };
        if project.archived {
            return reject(RejectCode::InvalidRequest, "the project is archived");
        }
        let repos = view.repositories()?;
        if !repos.iter().any(|r| r.id == self.repository_id) {
            return reject(RejectCode::NotFound, "no such repository");
        }
        let branch = self.integration_branch.trim();
        if !is_branch(branch) {
            return reject(
                RejectCode::InvalidRequest,
                "integration_branch is not a valid Git branch name",
            );
        }
        if let Some(existing) = project
            .repositories
            .iter_mut()
            .find(|l| l.repository_id == self.repository_id)
        {
            existing.integration_branch = branch.to_owned();
        } else {
            project.repositories.push(ProjectRepositoryLink {
                repository_id: self.repository_id.clone(),
                integration_branch: branch.to_owned(),
            });
            project
                .repositories
                .sort_by(|a, b| a.repository_id.cmp(&b.repository_id));
        }
        project.meta = touched(&project.meta, &admitted.by, admitted.now);
        let reply = changed(&project.id, project.meta.revision, admitted.generation);
        Ok(Change::new(Op::AttachProjectRepository.ack())
            .put(project_path(&project.id), encode_project(&project))
            .reply(reply))
    }
}

/// Resolves a repository by UUIDv7 ID or canonical key.
pub fn resolve_repository<'a>(repos: &'a [Repository], reference: &str) -> Option<&'a Repository> {
    let trimmed = reference.trim();
    if trimmed.is_empty() {
        return None;
    }
    if org_records::is_stable_id(trimmed) {
        if let Some(found) = repos.iter().find(|r| r.id.as_str() == trimmed) {
            return Some(found);
        }
    }
    repos
        .iter()
        .find(|r| r.key.as_deref().is_some_and(|k| k == trimmed))
}
