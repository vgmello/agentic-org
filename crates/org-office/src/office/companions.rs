//! The companion stores an office opens beside its `org-store` journal: private notes (and
//! their search index), accounting, budgets, provider pools and replication.

use std::sync::Arc;

use org_store::OfficeIdentity;

use crate::{error::OfficeError, model::OfficeInfo, store::open::EngineChoice};

use super::DataRoots;

pub(super) struct CompanionStores {
    pub private_notes: Arc<dyn crate::private_notes::PrivateNotesStore>,
    pub private_search: Arc<dyn crate::search::ports::PrivateNotesSearch>,
    pub attachments: crate::attachments::AttachmentStores,
    pub accounting: Arc<dyn crate::accounting::AccountingStore>,
    pub budgets: Arc<dyn crate::budgets::BudgetStore>,
    pub providers: Arc<dyn crate::providers::ProviderPoolStore>,
    pub replication: Arc<dyn crate::replication::ReplicationStore>,
}

pub(super) struct CompanionOpenSpec<'a> {
    pub engine: EngineChoice,
    pub roots: &'a DataRoots,
    pub info: &'a OfficeInfo,
    pub identity: &'a OfficeIdentity,
    pub hq_office_id: &'a str,
    pub ca_digest: org_sync_proto::Digest,
    pub epoch: u64,
    pub initial_snapshot_required: bool,
}

pub(super) fn open_companion_stores(
    spec: CompanionOpenSpec<'_>,
) -> Result<CompanionStores, OfficeError> {
    let is_hq = spec.info.role == crate::model::OfficeRole::Hq;
    let private = crate::store::open::open_private_notes(
        spec.engine,
        &spec.roots.durable,
        &spec.roots.runtime,
        &spec.info.office_id,
        &spec.info.org_id,
        is_hq,
    )?;
    Ok(CompanionStores {
        private_notes: private.store,
        private_search: private.search,
        attachments: crate::store::open::open_attachments(
            spec.engine,
            &spec.roots.durable,
            &spec.info.office_id,
            is_hq,
        )?,
        accounting: crate::store::open::open_accounting(
            spec.engine,
            &spec.roots.durable,
            &spec.roots.runtime,
            &spec.info.office_id,
            &spec.info.org_id,
        )?,
        budgets: crate::store::open::open_budgets(
            spec.engine,
            &spec.roots.durable,
            &spec.roots.runtime,
            &spec.info.office_id,
            &spec.info.org_id,
            is_hq,
        )?,
        providers: crate::store::open::open_providers(
            spec.engine,
            &spec.roots.durable,
            &spec.roots.runtime,
            &spec.info.office_id,
            &spec.info.org_id,
            is_hq,
        )?,
        replication: crate::store::open::open_replication(
            crate::store::open::ReplicationOpenSpec {
                engine: spec.engine,
                durable_root: &spec.roots.durable,
                runtime_root: &spec.roots.runtime,
                org_id: &spec.info.org_id,
                local_office: &spec.info.office_id,
                hq_office: spec.hq_office_id,
                ca_digest: spec.ca_digest,
                epoch: spec.epoch,
                incarnation: spec.identity.journal_incarnation.as_str(),
                initial_snapshot_required: spec.initial_snapshot_required,
            },
        )?,
    })
}
