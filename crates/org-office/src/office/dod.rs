//! Default definitions of done on [`Office`] (`TASK-88`): the organization's (owner only) and
//! a project's (the owner, or the lead of the project's owning team or a team above it). Both
//! are set at HQ; every office's projection has them, so a task created anywhere, offline
//! too, copies the one that applies.

use org_proto::projects::ProjectRecord;
use org_proto::tasks::{
    normalize_dod_items, OrganizationDefaultDodResult, OrganizationDefaultDodSetParams,
    ProjectDefaultDodSetParams,
};
use org_proto::{ApiError, ErrorCode};
use org_store::{Actor, Id, Outcome, Reply, StoreError};

use crate::error::OfficeError;
use crate::model::OfficeRole;
use crate::office::{storage, Office};
use crate::organization::features::{OrgRejection, SetOrganizationDefaultDod, NOT_AUTHORIZED};
use crate::projects::features::{SetProjectDefaultDod, PROJECT_DOD_DENIED};
use crate::store::open::{
    OrganizationDefaultDodRead, ProjectInspectByRef, SetOrganizationDefaultDodOp,
    SetProjectDefaultDodOp,
};
use crate::structure::wire::rejection_code;

/// The operation ID and the actor of a default-DoD change, or why they are refused.
fn header(actor: &str, command_id: &str) -> Result<(Id, Actor), ApiError> {
    let invalid = |what: &str| ApiError::new(ErrorCode::InvalidRequest, what);
    if !org_proto::is_uuid_v7(command_id) {
        return Err(invalid("command_id must be a canonical lowercase UUIDv7"));
    }
    let operation_id = Id::parse(command_id).map_err(|_| invalid("invalid command_id"))?;
    let actor = Id::parse(actor)
        .map_err(|_| ApiError::new(ErrorCode::NotAuthorized, "caller is not an employee"))?;
    Ok((operation_id, Actor::Employee(actor)))
}

/// The items normalized, or why they are refused (`None` stays `None`).
fn items(default_dod: Option<&[String]>) -> Result<Option<Vec<String>>, ApiError> {
    default_dod.map(normalize_dod_items).transpose()
}

/// A structure change's answer: done, or the refusal the caller gets.
fn answer(answer: Result<Reply, StoreError>) -> Result<Result<(), ApiError>, OfficeError> {
    let reply = match answer {
        Ok(reply) => reply,
        Err(StoreError::Conflict) => {
            let message = "command_id was used before with other params";
            return Ok(Err(ApiError::new(ErrorCode::OperationReused, message)));
        }
        Err(StoreError::Invalid(reason)) => {
            let code = if reason == NOT_AUTHORIZED || reason == PROJECT_DOD_DENIED {
                ErrorCode::NotAuthorized
            } else {
                ErrorCode::InvalidRequest
            };
            return Ok(Err(ApiError::new(code, reason)));
        }
        Err(error) => return Err(storage(error)),
    };
    match reply.outcome {
        Outcome::Completed => Ok(Ok(())),
        Outcome::Rejected => {
            let rejected: OrgRejection = serde_json::from_slice(&reply.bytes)
                .map_err(|_| storage(StoreError::Internal("an unreadable refusal".into())))?;
            Ok(Err(ApiError::new(
                rejection_code(rejected.code),
                rejected.message,
            )))
        }
    }
}

impl Office {
    fn hq_only(&self) -> Result<(), ApiError> {
        if self.info.role == OfficeRole::Hq {
            return Ok(());
        }
        Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "default definitions of done are set at HQ",
        ))
    }

    /// The organization's default definition of done.
    pub fn organization_default_dod(
        &self,
    ) -> Result<Result<OrganizationDefaultDodResult, ApiError>, OfficeError> {
        let (found, _) = self
            .store
            .read(OrganizationDefaultDodRead)
            .map_err(storage)?;
        Ok(found
            .map(|(default_dod, revision)| OrganizationDefaultDodResult {
                default_dod,
                revision,
            })
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "there is no organization yet")))
    }

    /// Replaces the organization's default definition of done: the owner, at HQ.
    pub fn organization_set_default_dod(
        &self,
        actor: &str,
        params: OrganizationDefaultDodSetParams,
    ) -> Result<Result<OrganizationDefaultDodResult, ApiError>, OfficeError> {
        let prepared = self.hq_only().and_then(|()| {
            let (operation_id, by) = header(actor, &params.command_id)?;
            Ok((operation_id, by, items(params.default_dod.as_deref())?))
        });
        let (operation_id, by, default_dod) = match prepared {
            Ok(prepared) => prepared,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let op = SetOrganizationDefaultDodOp {
            operation_id,
            here: self.identity.clone(),
            owner: self.owner.clone(),
            by,
            command: SetOrganizationDefaultDod {
                expected_revision: params.expected_revision,
                default_dod,
            },
        };
        if let Err(refusal) = answer(self.store.run(op))? {
            return Ok(Err(refusal));
        }
        self.organization_default_dod()
    }

    /// Replaces a project's default definition of done: the owner, or the lead of the
    /// project's owning team or of a team above it, at HQ.
    pub fn project_set_default_dod(
        &self,
        actor: &str,
        params: ProjectDefaultDodSetParams,
    ) -> Result<Result<ProjectRecord, ApiError>, OfficeError> {
        let prepared = self.hq_only().and_then(|()| {
            let (operation_id, by) = header(actor, &params.command_id)?;
            Ok((operation_id, by, items(params.default_dod.as_deref())?))
        });
        let (operation_id, by, default_dod) = match prepared {
            Ok(prepared) => prepared,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let not_found = || {
            let message = format!("project `{}` not found", params.project);
            ApiError::new(ErrorCode::NotFound, message)
        };
        let (project, _) = self
            .store
            .read(ProjectInspectByRef(params.project.clone()))
            .map_err(storage)?;
        let Some(project) = project else {
            return Ok(Err(not_found()));
        };
        let op = SetProjectDefaultDodOp {
            operation_id,
            here: self.identity.clone(),
            owner: self.owner.clone(),
            by,
            command: SetProjectDefaultDod {
                project_id: project.id.clone(),
                expected_revision: params.expected_revision,
                default_dod,
            },
        };
        if let Err(refusal) = answer(self.store.run(op))? {
            return Ok(Err(refusal));
        }
        let (project, _) = self
            .store
            .read(ProjectInspectByRef(project.id.as_str().to_owned()))
            .map_err(storage)?;
        Ok(project.map(|p| p.to_wire()).ok_or_else(not_found))
    }
}
