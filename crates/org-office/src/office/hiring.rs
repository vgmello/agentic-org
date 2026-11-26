//! Office-level dispatch and private-home provisioning for C2 HR hiring, role catalog,
//! team limits, `may_hire`, and agent lifecycle (`TASK-41` / C2, `ADR-0029`).

mod convert;
mod dispatch;
#[cfg(test)]
mod wire_tests;

use std::path::PathBuf;

use org_proto::{ApiError, ErrorCode};
use org_store::Id;

use self::convert::{decode_reply, map_store_error};
use super::{storage, Office};
use crate::error::OfficeError;
use crate::organization::{features::HIRE_DENIED, records};
use crate::store::open::{
    AgentExecutionById, CheckHrMessageAccess, EmployeeById, EmployeesList, ModelProfileById,
    ProviderConnectionById, RoleById,
};

impl Office {
    /// Returns the path of an agent's private home directory on this office
    /// (`<durable>/local/offices/<office_id>/agents/<employee_id>/home`).
    /// Never created before hire confirmation (`TASK-41` AC #2 / `HIRE-8`).
    pub fn agent_private_home_path(&self, employee_id: &str) -> PathBuf {
        self.durable
            .join("local")
            .join("offices")
            .join(self.identity.office_id.as_str())
            .join("agents")
            .join(employee_id)
            .join("home")
    }

    /// Ensures the agent's private `0700` home directory exists and returns its path.
    pub fn ensure_private_home(&self, employee_id: &str) -> Result<PathBuf, OfficeError> {
        self.ensure_agent_private_home(employee_id)?;
        Ok(self.agent_private_home_path(employee_id))
    }

    /// Reads one employee record by canonical UUIDv7 ID.
    pub fn load_employee(
        &self,
        employee_id: &str,
    ) -> Result<Option<records::Employee>, OfficeError> {
        let Ok(id) = Id::parse(employee_id) else {
            return Ok(None);
        };
        Ok(self.store.read(EmployeeById(id)).map_err(storage)?.0)
    }

    /// Lists all employees registered in the organization.
    pub fn list_employees(&self) -> Result<Vec<records::Employee>, OfficeError> {
        Ok(self.store.read(EmployeesList).map_err(storage)?.0)
    }

    /// Reads one role record by canonical UUIDv7 ID.
    pub fn load_role(&self, role_id: &str) -> Result<Option<records::Role>, OfficeError> {
        let Ok(id) = Id::parse(role_id) else {
            return Ok(None);
        };
        Ok(self.store.read(RoleById(id)).map_err(storage)?.0)
    }

    /// Reads one agent's execution configuration by canonical employee UUIDv7 ID.
    pub fn load_agent_execution(
        &self,
        employee_id: &str,
    ) -> Result<Option<records::AgentExecutionConfig>, OfficeError> {
        let Ok(id) = Id::parse(employee_id) else {
            return Ok(None);
        };
        Ok(self.store.read(AgentExecutionById(id)).map_err(storage)?.0)
    }

    /// Reads one provider connection by canonical UUIDv7 ID.
    pub fn load_provider_connection(
        &self,
        connection_id: &str,
    ) -> Result<Option<records::ProviderConnection>, OfficeError> {
        let Ok(id) = Id::parse(connection_id) else {
            return Ok(None);
        };
        Ok(self
            .store
            .read(ProviderConnectionById(id))
            .map_err(storage)?
            .0)
    }

    /// Reads one model profile by canonical UUIDv7 ID.
    pub fn load_model_profile(
        &self,
        profile_id: &str,
    ) -> Result<Option<records::ModelProfile>, OfficeError> {
        let Ok(id) = Id::parse(profile_id) else {
            return Ok(None);
        };
        Ok(self.store.read(ModelProfileById(id)).map_err(storage)?.0)
    }

    /// Reads the organization's channel watching model profile setting (`ADR-0024` d13, `WATCH-5`).
    pub fn watching_model_setting(
        &self,
    ) -> Result<Option<records::WatchingModelSetting>, OfficeError> {
        Ok(self
            .store
            .read(crate::store::open::WatchingModelRead)
            .map_err(storage)?
            .0)
    }

    /// Sets or clears the organization's channel watching model profile setting (owner-only,
    /// idempotent on `command_id`; `ADR-0024` d8/d13, `WATCH-5`).
    pub fn set_watching_model_setting(
        &self,
        actor: &str,
        command_id: &str,
        model_profile_id: Option<&str>,
    ) -> Result<Result<org_proto::chat::ChatWatchingModelResult, ApiError>, OfficeError> {
        let Ok(actor_id) = Id::parse(actor) else {
            return Ok(Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "invalid actor id",
            )));
        };
        let Ok(cmd_id) = Id::parse(command_id) else {
            return Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "command_id must be a canonical UUIDv7",
            )));
        };
        let profile_id = match model_profile_id {
            Some(raw) => match Id::parse(raw) {
                Ok(id) => Some(id),
                Err(_) => {
                    return Ok(Err(ApiError::new(
                        ErrorCode::InvalidRequest,
                        "model_profile_id must be a canonical UUIDv7",
                    )));
                }
            },
            None => None,
        };
        let op = crate::store::open::StructureOp {
            command: crate::organization::features::SetWatchingModel {
                model_profile_id: profile_id,
            },
            operation_id: cmd_id,
            by: org_store::Actor::Employee(actor_id),
            owner: self.owner.clone(),
            here: self.identity.clone(),
        };
        match self.store.run(op) {
            Ok(reply) => match decode_reply::<crate::organization::features::Changed>(reply) {
                Ok(_) => self.watching_model_view().map(Ok),
                Err(err) => Ok(Err(err)),
            },
            Err(e) => Ok(Err(map_store_error(e))),
        }
    }

    /// Builds the wire view for `chat.watching_model_get` / `chat.watching_model_set`.
    pub fn watching_model_view(
        &self,
    ) -> Result<org_proto::chat::ChatWatchingModelResult, OfficeError> {
        let setting = self.watching_model_setting()?;
        let revision = setting.as_ref().map_or(0, |s| s.meta.revision);
        let model_profile_id = setting
            .and_then(|s| s.model_profile_id)
            .map(|id| id.as_str().to_owned());
        let model_profile_name = match model_profile_id.as_deref() {
            Some(pid) => self.load_model_profile(pid)?.map(|p| p.name),
            None => None,
        };
        Ok(org_proto::chat::ChatWatchingModelResult {
            enabled: model_profile_id.is_some(),
            model_profile_id,
            model_profile_name,
            revision,
        })
    }

    fn ensure_agent_private_home(&self, employee_id: &str) -> Result<(), OfficeError> {
        let base = self
            .durable
            .join("local")
            .join("offices")
            .join(self.identity.office_id.as_str())
            .join("agents")
            .join(employee_id);
        let home = base.join("home");
        for dir in [
            self.durable.join("local"),
            self.durable.join("local").join("offices"),
            self.durable
                .join("local")
                .join("offices")
                .join(self.identity.office_id.as_str()),
            self.durable
                .join("local")
                .join("offices")
                .join(self.identity.office_id.as_str())
                .join("agents"),
            base,
            home,
        ] {
            match org_platform::fs::create_owner_only_dir(&dir) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(OfficeError::storage(e)),
            }
        }
        Ok(())
    }

    /// Checks whether `sender` may address `target` in messaging (ADR-0029 decision 3 / HIRE-2):
    /// anyone who may hire nowhere is refused when targeting `@hr` / the HR lead.
    pub fn check_hr_message_allowed(
        &self,
        sender: &str,
        target: &str,
    ) -> Result<Result<(), ApiError>, OfficeError> {
        let Ok(sender_id) = Id::parse(sender) else {
            return Ok(Err(ApiError::new(ErrorCode::NotAuthorized, HIRE_DENIED)));
        };
        let (allowed, _) = self
            .store
            .read(CheckHrMessageAccess {
                owner: self.owner.clone(),
                sender: sender_id,
                target: target.to_owned(),
            })
            .map_err(storage)?;
        if allowed {
            Ok(Ok(()))
        } else {
            Ok(Err(ApiError::new(ErrorCode::NotAuthorized, HIRE_DENIED)))
        }
    }
}
