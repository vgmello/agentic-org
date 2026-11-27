//! The organization's default definition of done (`TASK-88`): the step texts a new task
//! copies into its checklist's `dod` section when neither it nor its project names one. The
//! owner sets it at HQ, at the organization record's revision; `organization.yaml` carries
//! it to every office, so a satellite copies it offline.

use org_docs::MAX_FRONT_MATTER_BYTES;
use org_store::{Actor, Change, Engine, Id, OfficeIdentity, Refusal, UnitOfWork};
use serde::Serialize;

use super::shared::{admit, check_revision, checked_dod_items, reject, touched};
use super::{changed, Feature, Op, RejectCode};
use crate::organization::{ports::TeamDependents, records};

/// Replaces the organization's default definition of done (`None` clears it).
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct SetOrganizationDefaultDod {
    pub expected_revision: u64,
    pub default_dod: Option<Vec<String>>,
}

impl Feature for SetOrganizationDefaultDod {
    const OP: Op = Op::SetOrganizationDefaultDod;

    fn decide<E: Engine>(
        &self,
        uow: &mut UnitOfWork<'_, E>,
        here: &OfficeIdentity,
        owner: &Id,
        by: &Actor,
    ) -> Result<Change, Refusal>
    where
        E::Tx: TeamDependents,
    {
        let default_dod = checked_dod_items(self.default_dod.as_deref())?;
        let admitted = admit(uow, here, owner, by)?;
        let mut organization = admitted.organization;
        check_revision(organization.meta.revision, self.expected_revision)?;
        organization.default_dod = default_dod;
        organization.meta = touched(&organization.meta, &admitted.by, admitted.now);
        let bytes = records::encode_organization(&organization);
        if bytes.len() > MAX_FRONT_MATTER_BYTES {
            return reject(
                RejectCode::InvalidRequest,
                "the default definition of done does not fit the organization record",
            );
        }
        let reply = changed(
            &organization.id,
            organization.meta.revision,
            admitted.generation,
        );
        Ok(Change::new(Op::SetOrganizationDefaultDod.ack())
            .put(records::organization_path(), bytes)
            .reply(reply))
    }
}
