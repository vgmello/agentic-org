//! Openers for the office's isolated companion stores (private task notes, `L2`
//! accounting, `L3` budgets, `L4` provider pools and replication) on the chosen engine.

use std::{path::Path, sync::Arc};

use super::super::sqlite::{
    accounting::SqliteAccountingStore,
    budgets::SqliteBudgetStore,
    private_notes::SqlitePrivateNotesStore,
    providers::SqliteProviderPoolStore,
    replication::{OpenReplicationParams, SqliteReplicationStore},
};
use super::EngineChoice;
use crate::{
    accounting::AccountingStore,
    budgets::BudgetStore,
    error::OfficeError,
    private_notes::{PrivateNotesStore, SatelliteRefusedPrivateNotesStore},
    providers::ProviderPoolStore,
    replication::ReplicationStore,
};

/// Opens the isolated HQ-authoritative per-author private task notes store (`TASK-50`, `L8`,
/// `ADR-0016`, `doc-79`, `TASK-97` ARCH-01, `TASK-52` ARCH-02). On satellites (`is_hq == false`),
/// returns a fail-closed stub without creating `<durable-root>/private/task-notes.db`.
pub(crate) fn open_private_notes(
    engine: EngineChoice,
    durable_root: &Path,
    is_hq: bool,
) -> Result<Arc<dyn PrivateNotesStore>, OfficeError> {
    if !is_hq {
        return Ok(Arc::new(SatelliteRefusedPrivateNotesStore));
    }
    match engine {
        EngineChoice::Sqlite => Ok(Arc::new(SqlitePrivateNotesStore::open(durable_root)?)),
        #[cfg(any(test, feature = "testkit"))]
        EngineChoice::Memory => Ok(Arc::new(
            crate::private_notes::memory::MemoryPrivateNotesStore::new(),
        )),
    }
}

/// Opens the isolated `L2` usage & cost accounting store (`TASK-53`, `doc-87`, `ADR-0016`)
/// backed by `<durable-root>/accounting/` and `<runtime-root>/<office-id>/protected/accounting.db`.
pub(crate) fn open_accounting(
    engine: EngineChoice,
    durable_root: &Path,
    runtime_root: &Path,
    office_id: &str,
    org_id: &str,
) -> Result<Arc<dyn AccountingStore>, OfficeError> {
    match engine {
        EngineChoice::Sqlite => Ok(Arc::new(SqliteAccountingStore::open(
            durable_root,
            runtime_root,
            office_id,
            org_id,
        )?)),
        #[cfg(any(test, feature = "testkit"))]
        EngineChoice::Memory => Ok(Arc::new(
            crate::accounting::memory::MemoryAccountingStore::new(),
        )),
    }
}

/// Opens the isolated `L3` budgets & holds store (`TASK-53`, `doc-92`, `ADR-0016`)
/// backed by `<durable-root>/budgets/` and `<runtime-root>/<office-id>/protected/budgets.db`.
/// On satellites (`is_hq == false`), wraps the store in `SatelliteReadOnlyBudgetStore` (`TASK-52` M-20).
pub(crate) fn open_budgets(
    engine: EngineChoice,
    durable_root: &Path,
    runtime_root: &Path,
    office_id: &str,
    org_id: &str,
    is_hq: bool,
) -> Result<Arc<dyn BudgetStore>, OfficeError> {
    match engine {
        EngineChoice::Sqlite => {
            let store = SqliteBudgetStore::open(durable_root, runtime_root, office_id, org_id)?;
            if is_hq {
                Ok(Arc::new(store))
            } else {
                Ok(Arc::new(crate::budgets::SatelliteReadOnlyBudgetStore::new(
                    store,
                )))
            }
        }
        #[cfg(any(test, feature = "testkit"))]
        EngineChoice::Memory => {
            let store = crate::budgets::memory::MemoryBudgetStore::new();
            if is_hq {
                Ok(Arc::new(store))
            } else {
                Ok(Arc::new(crate::budgets::SatelliteReadOnlyBudgetStore::new(
                    store,
                )))
            }
        }
    }
}

/// Opens the isolated `L4` provider bindings, discovery cache, and dual-authority quota permit
/// store (`TASK-54`, `doc-88`, `doc-89`, `TASK-2`, `TASK-52` ARCH-02).
pub(crate) fn open_providers(
    engine: EngineChoice,
    durable_root: &Path,
    runtime_root: &Path,
    office_id: &str,
    org_id: &str,
    is_hq: bool,
) -> Result<Arc<dyn ProviderPoolStore>, OfficeError> {
    match engine {
        EngineChoice::Sqlite => Ok(Arc::new(SqliteProviderPoolStore::open(
            durable_root,
            runtime_root,
            office_id,
            org_id,
            is_hq,
        )?)),
        #[cfg(any(test, feature = "testkit"))]
        EngineChoice::Memory => Ok(Arc::new(
            crate::providers::memory::MemoryProviderPoolStore::new(),
        )),
    }
}

pub(crate) struct ReplicationOpenSpec<'a> {
    pub engine: EngineChoice,
    pub durable_root: &'a Path,
    pub runtime_root: &'a Path,
    pub org_id: &'a str,
    pub local_office: &'a str,
    pub hq_office: &'a str,
    pub ca_digest: org_sync_proto::Digest,
    pub epoch: u64,
    pub incarnation: &'a str,
    pub initial_snapshot_required: bool,
}

/// Opens the engine-independent replication store (`TASK-52`, `FC-145`, `ADR-0018` `ENG-3`).
pub(crate) fn open_replication(
    spec: ReplicationOpenSpec<'_>,
) -> Result<Arc<dyn ReplicationStore>, OfficeError> {
    let org_id =
        org_sync_proto::OrgId::new(spec.org_id).map_err(|_| OfficeError::Invalid("org_id"))?;
    let local_office = org_sync_proto::OfficeId::new(spec.local_office)
        .map_err(|_| OfficeError::Invalid("local_office"))?;
    let hq_office = org_sync_proto::OfficeId::new(spec.hq_office)
        .map_err(|_| OfficeError::Invalid("hq_office"))?;
    let epoch =
        org_sync_proto::Epoch::new(spec.epoch.max(1)).map_err(|_| OfficeError::Invalid("epoch"))?;
    let incarnation = org_sync_proto::OriginIncarnation::new(spec.incarnation)
        .map_err(|_| OfficeError::Invalid("incarnation"))?;
    match spec.engine {
        EngineChoice::Sqlite => Ok(Arc::new(SqliteReplicationStore::open(
            OpenReplicationParams {
                durable_root: spec.durable_root,
                runtime_root: spec.runtime_root,
                org_id,
                local_office,
                hq_office,
                ca: spec.ca_digest,
                epoch,
                incarnation,
                initial_snapshot_required: spec.initial_snapshot_required,
            },
        )?)),
        #[cfg(any(test, feature = "testkit"))]
        EngineChoice::Memory => Ok(Arc::new(
            crate::replication::memory::MemoryReplicationStore::new(
                org_id,
                local_office,
                hq_office,
                spec.ca_digest,
                epoch,
                incarnation,
                spec.initial_snapshot_required,
            ),
        )),
    }
}
