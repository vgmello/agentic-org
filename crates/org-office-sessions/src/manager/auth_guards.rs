//! Caller authorization and agent admission guards shared by every session operation.

use org_office::organization_family::records::{
    BuiltinKey, Employee, EmployeeKind, EmployeeStatus,
};
use org_proto::{ApiError, ErrorCode};

use crate::credentials::rfc3339_now;

use super::SessionManager;

impl SessionManager {
    /// Recognizes the organization owner on both HQ (host-authoritative owner) and satellites
    /// (HQ-replicated owner via `provenance::is_verified_owner`), while never treating a
    /// satellite's own local `office_id` as the organization owner.
    pub(crate) fn is_owner_actor(&self, member_id: &str) -> bool {
        crate::provenance::is_verified_owner(&self.office, member_id)
            || (self.office.info().role == org_office::OfficeRole::Hq
                && member_id == self.office.owner_member_id())
    }

    pub(super) fn authorize_owner_only(&self, member_id: &str) -> Result<(), ApiError> {
        self.authorize_owner_only_inner(member_id, false)
    }

    pub(crate) fn authorize_owner_only_inner(
        &self,
        member_id: &str,
        hq_attested_owner: bool,
    ) -> Result<(), ApiError> {
        if hq_attested_owner || self.is_owner_actor(member_id) {
            return Ok(());
        }
        Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "only the organization owner may perform this operation",
        ))
    }

    pub(crate) fn authorize_owner_or_person_inner(
        &self,
        member_id: &str,
        hq_attested_owner: bool,
    ) -> Result<(), ApiError> {
        if hq_attested_owner || self.is_owner_actor(member_id) {
            return Ok(());
        }
        let found = self
            .office
            .load_employee(member_id)
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "could not verify caller"))?;
        if found
            .is_some_and(|e| e.kind == EmployeeKind::Person && e.status == EmployeeStatus::Active)
        {
            return Ok(());
        }
        Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "only an active person or the owner may manage sessions",
        ))
    }

    /// Tess (the Chief of Staff) serves every caller in partitions of her own, so anyone but
    /// Tess herself may inspect, list, read or stop her sessions only as the owner: any other
    /// active person would see the owner's replies and transcripts, or stop the owner's turn
    /// (`TASK-80.1`). Every other target is left to the caller's other checks.
    pub(crate) fn authorize_chief_of_staff_target_inner(
        &self,
        member_id: &str,
        hq_attested_owner: bool,
        target_id: &str,
    ) -> Result<(), ApiError> {
        if target_id == member_id {
            return Ok(());
        }
        let is_chief_of_staff = self
            .office
            .load_employee(target_id)
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "could not read employee"))?
            .is_some_and(|e| e.builtin_key == Some(BuiltinKey::ChiefOfStaff));
        if is_chief_of_staff {
            return self.authorize_owner_only_inner(member_id, hq_attested_owner);
        }
        Ok(())
    }

    pub(crate) fn require_agent(&self, employee_id: &str) -> Result<Employee, ApiError> {
        let emp = self
            .office
            .load_employee(employee_id)
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "could not read employee"))?
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "agent not found"))?;
        if emp.kind != EmployeeKind::Agent {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "target employee is not an agent",
            ));
        }
        Ok(emp)
    }

    pub(crate) fn require_active_agent(&self, employee_id: &str) -> Result<Employee, ApiError> {
        if self
            .office
            .sync_status()
            .is_ok_and(|s| s.initial_snapshot_required)
        {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "satellite office is awaiting a complete HQ snapshot before running agents",
            ));
        }
        let emp = self.require_agent(employee_id)?;
        if emp.status != EmployeeStatus::Active {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "only active agents can start a session",
            ));
        }
        // Enforce local office placement at the shared admission boundary for both manual
        // `start_session` and autonomous wakes (`TASK-52` P7-#6).
        if self.office.is_multi_office().unwrap_or(false) {
            let owner_office = org_office::resolve_employee_office(&self.office, employee_id)
                .map_err(|_| {
                    ApiError::new(
                        ErrorCode::StorageError,
                        "could not resolve agent home office",
                    )
                })?;
            if owner_office != self.office.info().office_id {
                return Err(ApiError::new(
                    ErrorCode::NotAuthorized,
                    format!(
                        "agent is placed on office `{owner_office}` and cannot run on `{}`",
                        self.office.info().office_id
                    ),
                ));
            }
        }
        Ok(emp)
    }

    pub(super) fn check_not_recovery_required(&self, employee_id: &str) -> Result<(), ApiError> {
        if let Ok(Some(record)) = self.store.read_active_turn(employee_id) {
            if record.recovery_required {
                return Err(ApiError::new(
                    ErrorCode::Conflict,
                    "agent session requires manual recovery before starting",
                ));
            }
        }
        Ok(())
    }

    pub(super) fn clear_persisted_stall_only(&self, employee_id: &str) {
        let _ = self.store.update_active_turn(employee_id, |persisted| {
            if persisted.stall_locked {
                persisted.consecutive_stalls = 0;
                persisted.stall_locked = false;
                persisted.updated_at = rfc3339_now();
            }
        });
    }

    pub(super) fn check_not_blocked(&self, employee_id: &str) -> Result<(), ApiError> {
        if let Ok(Some(record)) = self.store.read_active_turn(employee_id) {
            if record.stall_locked {
                return Err(ApiError::new(
                    ErrorCode::Conflict,
                    "agent session is stall-locked after 3 consecutive stalls; clear the stall lock first",
                ));
            }
            if record.recovery_required {
                return Err(ApiError::new(
                    ErrorCode::Conflict,
                    "agent session requires manual recovery before starting",
                ));
            }
        }
        Ok(())
    }
}
