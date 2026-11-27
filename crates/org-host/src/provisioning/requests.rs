//! The organization calls: create, lookup and list, and the views they answer with.

use std::sync::Arc;

use org_host_control::{
    ops::RecoveryState,
    organization::{CreatingOrganization, ListResult, OrganizationView},
    HostOpResult,
};
use org_proto::{selected_bootstrap::PlanV1, ApiError, ErrorCode};

use crate::hostdb::{
    accounts::Account,
    provisioning::{Reservation, Taken},
};

use super::{storage, unavailable, Pending, Provisioning};

/// What a signed-in browser asks: create, look up, list.
impl Provisioning {
    /// `organization.create`: admit a new create or find the one this id names, then report.
    pub(crate) fn create(
        self: &Arc<Self>,
        account: &Account,
        operation_id: &str,
        plan: &PlanV1,
        fp: [u8; 32],
    ) -> Result<HostOpResult<OrganizationView>, ApiError> {
        let r = self.admit(account, operation_id, plan, fp)?;
        self.progress(&r)
    }

    /// `operation.lookup`: one of the account's own creates. Never admits a new one.
    pub(crate) fn lookup(
        self: &Arc<Self>,
        account: &Account,
        operation_id: &str,
    ) -> Result<HostOpResult<OrganizationView>, ApiError> {
        let r = self
            .db
            .provisioning_lookup(account.account_id(), operation_id)
            .map_err(storage)?
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "no such operation"))?;
        self.progress(&r)
    }

    /// `organization.list`: the organizations the account is linked to, and its creates
    /// still in flight.
    pub(crate) fn list(&self, account: &Account) -> Result<ListResult, ApiError> {
        let hq = self.hq.get().ok_or_else(unavailable)?;
        self.web.snapshot(|| {
            let mut organizations = Vec::new();
            for link in account.links() {
                let Some(r) = self
                    .db
                    .provisioning_linked_to(link.org_id())
                    .map_err(storage)?
                else {
                    continue;
                };
                // One organization this host cannot describe does not hide the others.
                let Some(organization) = hq.organization(&r.prepared.organization_id) else {
                    crate::host_log::log("a linked organization is missing from the catalog");
                    continue;
                };
                organizations.push(self.view(&r, organization, link.member_id())?);
            }
            let creating = self
                .db
                .provisioning_of(account.account_id())
                .map_err(storage)?
                .into_iter()
                .filter(|r| !r.linked)
                .map(|r| CreatingOrganization {
                    operation_id: r.operation_id,
                    name: r.prepared.plan.name,
                    slug: r.prepared.plan.slug,
                })
                .collect();
            Ok(ListResult {
                organizations,
                creating,
            })
        })
    }

    /// Reads the create's state without waiting for any admission or creation. The read is
    /// ordered with the broadcast, so `account.changed` is queued before any `completed`.
    fn progress(
        self: &Arc<Self>,
        r: &Reservation,
    ) -> Result<HostOpResult<OrganizationView>, ApiError> {
        let hq = self.hq.get().ok_or_else(unavailable)?;
        let current = self
            .web
            .snapshot(|| self.db.provisioning_lookup(&r.account, &r.operation_id))
            .map_err(storage)?
            .ok_or_else(unavailable)?;
        if current.linked {
            self.verify_owner(&current)?;
            return Ok(HostOpResult::Completed {
                result: self.view(
                    &current,
                    hq.organization(&current.prepared.organization_id)
                        .ok_or_else(unavailable)?,
                    &current.owner_member,
                )?,
            });
        }
        // A create whose account is no longer an administrator is never created from
        // ([`Self::drive`] refuses it), so a lookup starts no thread for it: it answers the
        // stored state. The name and slug stay reserved until `organization.cancel` exists
        // (FC-188).
        if self.administrator(&current.account)?.is_some() {
            self.ensure_driving(&current)?;
        }
        Ok(HostOpResult::Accepted)
    }

    pub(super) fn view(
        &self,
        r: &Reservation,
        organization: org_proto::Organization,
        employee_id: &str,
    ) -> Result<OrganizationView, ApiError> {
        use crate::hostdb::recovery::State;
        let recovery = match self
            .recovery
            .recovery_state(&r.operation_id)
            .map_err(storage)?
        {
            None | Some(State::Pending) => RecoveryState::Pending,
            Some(State::Delivered | State::AckPending) => RecoveryState::Delivered,
            Some(State::Acknowledged) => RecoveryState::Acknowledged,
        };
        Ok(OrganizationView {
            id: organization.id,
            name: organization.name,
            slug: r.prepared.plan.slug.clone(),
            created_at: organization.created_at,
            employee_id: employee_id.to_owned(),
            offices: organization
                .offices
                .into_iter()
                .map(|mut office| {
                    office.endpoint = None;
                    office
                })
                .collect(),
            recovery,
        })
    }
}

pub(super) fn validate_pending_binding(
    pending: &Pending,
    account: &Account,
    operation_id: &str,
    plan: &PlanV1,
    fp: &[u8; 32],
) -> Result<(), ApiError> {
    if pending.version != 1
        || pending.reservation.operation_id != operation_id
        || pending.reservation.account != account.account_id()
        || &pending.reservation.fingerprint != fp
        || &pending.reservation.prepared.plan != plan
    {
        return Err(ApiError::new(
            ErrorCode::OperationReused,
            "operation binding differs",
        ));
    }
    Ok(())
}

pub(super) fn already_exists(taken: Taken) -> ApiError {
    let (field, message) = match taken {
        Taken::Name => ("name", "an organization with that name already exists"),
        Taken::Slug => ("slug", "an organization with that slug already exists"),
    };
    let mut error = ApiError::new(ErrorCode::AlreadyExists, message);
    error.field = Some(field.into());
    error.rule = Some("taken".into());
    error
}
