//! Admitting a selected HQ create: the administrator, the plan and its pending reservation.

use org_proto::{
    selected_bootstrap::{PlanV1, PreparedV1},
    ApiError, ErrorCode,
};

use crate::{
    host_secrets::{Name, Value},
    hostdb::{accounts::Account, provisioning::Reservation},
};

use super::requests::{already_exists, validate_pending_binding};
use super::{
    generate_recovery_code, lookup_error, not_administrator, storage, unavailable, Pending,
    Provisioning,
};

impl Provisioning {
    pub(super) fn verify_owner(&self, r: &Reservation) -> Result<(), ApiError> {
        self.db
            .verify_selected_owner(&r.prepared.organization_id, &r.account, &r.owner_member)
            .map_err(storage)
    }

    /// Admission: the only step that holds the gate. A known operation is read before the
    /// gate, so a poll never waits for another create's admission. A new one is validated,
    /// its name and slug are checked, its recovery code is staged in host custody and its
    /// reservation is committed, in that order; a refusal before the reservation leaves no
    /// custody entry behind.
    pub(super) fn admit(
        &self,
        account: &Account,
        operation_id: &str,
        plan: &PlanV1,
        fp: [u8; 32],
    ) -> Result<Reservation, ApiError> {
        if !account.admin() || account.revoked_at_ms().is_some() {
            return Err(not_administrator());
        }
        let known = || {
            let wall = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(storage)?
                .as_millis();
            self.db
                .provisioning(
                    account,
                    operation_id,
                    &fp,
                    i64::try_from(wall).map_err(storage)?,
                )
                .map_err(lookup_error)
        };
        if let Some(r) = known()? {
            return Ok(r);
        }
        let _gate = self.gate.lock().map_err(storage)?;
        if let Some(r) = known()? {
            return Ok(r);
        }
        if let Some((field, rule)) = plan.invalid_field() {
            let mut error = ApiError::new(
                ErrorCode::InvalidRequest,
                "invalid selected provisioning plan",
            );
            error.field = Some(field.into());
            error.rule = Some(rule.into());
            return Err(error);
        }
        let handle = Name::parse(&format!("recovery.{operation_id}")).map_err(storage)?;
        // A staged code without a reservation is an admission the host lost before its
        // commit. It is continued with its own ids when it is this same request.
        let staged = match self.custody.load(&handle).map_err(storage)? {
            Some(bytes) => {
                let pending: Pending = serde_json::from_slice(bytes.expose()).map_err(storage)?;
                validate_pending_binding(&pending, account, operation_id, plan, &fp)?;
                Some(pending)
            }
            None => None,
        };
        let taken = |fresh: bool| -> Option<ApiError> {
            let taken = self.db.provisioning_taken(&plan.name, &plan.slug);
            if fresh || matches!(taken, Ok(Some(_))) {
                // No reservation will ever name this entry: remove it.
                let _ = self.custody.delete(&handle);
            }
            match taken {
                Ok(Some(taken)) => Some(already_exists(taken)),
                Ok(None) => None,
                Err(_) => Some(unavailable()),
            }
        };
        if let Some(found) = self
            .db
            .provisioning_taken(&plan.name, &plan.slug)
            .map_err(storage)?
        {
            if staged.is_some() {
                self.custody.delete(&handle).map_err(storage)?;
            }
            return Err(already_exists(found));
        }
        let fresh = staged.is_none();
        let pending = match staged {
            Some(pending) => pending,
            None => match self.new_pending(account, operation_id, plan, fp, &handle) {
                Ok(pending) => pending,
                Err(error) => {
                    // The put may have been published before it failed.
                    let _ = self.custody.delete(&handle);
                    return Err(error);
                }
            },
        };
        #[cfg(test)]
        match self
            .reservation_fault
            .swap(0, std::sync::atomic::Ordering::AcqRel)
        {
            1 => return Err(taken(fresh).unwrap_or_else(unavailable)),
            2 => return Err(unavailable()),
            _ => {}
        }
        #[cfg(test)]
        if let Some(revoke) = self.revoke_hook.lock().unwrap().take() {
            let id = crate::accounts::identity::AccountId::parse(account.account_id())
                .map_err(storage)?;
            revoke(&id);
        }
        self.db
            .reserve_provisioning(account, &pending.reservation)
            .map_err(|error| {
                if matches!(error, crate::hostdb::Error::NotAdministrator) {
                    // The account lost its authority after the first check: no reservation
                    // will ever name this entry.
                    let _ = self.custody.delete(&handle);
                    return not_administrator();
                }
                taken(fresh).unwrap_or_else(unavailable)
            })
    }

    /// The account that reserved a create, while it is still an instance administrator.
    pub(super) fn administrator(&self, account: &str) -> Result<Option<Account>, ApiError> {
        let id = crate::accounts::identity::AccountId::parse(account).map_err(storage)?;
        Ok(self
            .db
            .account_authority(&id)
            .map_err(storage)?
            .filter(|a| a.admin() && a.revoked_at_ms().is_none()))
    }

    pub(super) fn new_pending(
        &self,
        account: &Account,
        operation_id: &str,
        plan: &PlanV1,
        fp: [u8; 32],
        handle: &Name,
    ) -> Result<Pending, ApiError> {
        // The creator's person id is the member id the account is linked to.
        let owner_member = crate::ids::uuid_v7().map_err(storage)?;
        let prepared = PreparedV1 {
            creator: org_proto::selected_bootstrap::CreatorV1::from_login(
                owner_member.clone(),
                account.login(),
            ),
            plan: plan.clone(),
            organization_id: crate::ids::uuid_v7().map_err(storage)?,
            office_id: crate::ids::uuid_v7().map_err(storage)?,
            repository_id: crate::ids::uuid_v7().map_err(storage)?,
            home_team_id: crate::ids::uuid_v7().map_err(storage)?,
            hr_employee_id: crate::ids::uuid_v7().map_err(storage)?,
        };
        if self
            .keys
            .load(&prepared.office_id)
            .map_err(storage)?
            .is_some()
        {
            return Err(unavailable());
        }
        let reservation = Reservation {
            operation_id: operation_id.into(),
            account: account.account_id().into(),
            fingerprint: fp,
            prepared,
            owner_member,
            recovery_handle: handle.as_str().into(),
            key_fingerprint: None,
            linked: false,
        };
        let code = generate_recovery_code()?;
        let pending = Pending {
            version: 1,
            reservation,
            code,
            import_source: None,
        };
        let bytes = Value::new(serde_json::to_vec(&pending).map_err(storage)?).map_err(storage)?;
        self.custody.put(handle, &bytes).map_err(storage)?;
        // Re-read after a possibly durable publication; no worker
        // starts from an unverified in-memory credential copy.
        let actual = self
            .custody
            .load(handle)
            .map_err(storage)?
            .ok_or_else(unavailable)?;
        if actual.expose() != bytes.expose() {
            return Err(unavailable());
        }
        Ok(pending)
    }
}
