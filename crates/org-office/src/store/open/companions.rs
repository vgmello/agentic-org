//! Openers for the office's isolated companion stores (private task notes, `L2`
//! accounting, `L3` budgets, `L4` provider pools and replication) on the chosen engine.
//!
//! Each opener first lays out its store's durable folders, whatever the engine
//! (`TASK-172`): the folders are the office's, the same on every engine (see
//! `store/layout.rs` for what their contents may still differ in).

use std::{path::Path, sync::Arc};

use super::super::layout;
use super::super::sqlite::{
    accounting::SqliteAccountingStore, budgets::SqliteBudgetStore,
    private_notes::SqlitePrivateNotesStore, providers::SqliteProviderPoolStore,
    replication::SqliteReplicationProjection, search::private::SqlitePrivateNotesSearch,
    user_space::SqliteUserSpaceStore,
};
use super::EngineChoice;
use crate::{
    accounting::AccountingStore,
    attachments::{
        blobs::{BlobStore, FsBlobStore},
        staging::StagingArea,
        user_space::{SatelliteRefusedUserSpace, UserSpaceStore},
        AttachmentStores,
    },
    budgets::BudgetStore,
    error::OfficeError,
    private_fs::private_dir_synced,
    private_notes::{PrivateNotesStore, SatelliteRefusedPrivateNotesStore},
    providers::ProviderPoolStore,
    replication::{
        durable::{DurableReplicationParams, DurableReplicationStore},
        ReplicationStore,
    },
    search::ports::{PrivateNotesSearch, SatelliteNoPrivateSearch},
};

/// Creates `folder`, one of the office's durable folders, below `durable_root`.
fn lay_out(durable_root: &Path, folder: &Path) -> Result<(), OfficeError> {
    private_dir_synced(durable_root, folder).map_err(OfficeError::storage)
}

/// The private notes store and its search index.
pub(crate) struct PrivateNotesOpened {
    pub store: Arc<dyn PrivateNotesStore>,
    pub search: Arc<dyn PrivateNotesSearch>,
}

/// Opens the isolated HQ-authoritative per-author private task notes store (`TASK-50`, `L8`,
/// `ADR-0016`, `doc-79`, `TASK-97` ARCH-01, `TASK-52` ARCH-02) and its disposable search index
/// (`TASK-91`, `runtime/<office-id>/protected/private/task-notes-search.db`, made anew). On
/// satellites (`is_hq == false`), returns fail-closed stubs without creating
/// `<durable-root>/private/task-notes.db` or any search index.
pub(crate) fn open_private_notes(
    engine: EngineChoice,
    durable_root: &Path,
    runtime_root: &Path,
    office_id: &str,
    org_id: &str,
    is_hq: bool,
) -> Result<PrivateNotesOpened, OfficeError> {
    if !is_hq {
        return Ok(PrivateNotesOpened {
            store: Arc::new(SatelliteRefusedPrivateNotesStore),
            search: Arc::new(SatelliteNoPrivateSearch),
        });
    }
    lay_out(durable_root, &layout::private_notes(durable_root))?;
    match engine {
        EngineChoice::Sqlite => {
            let store = SqlitePrivateNotesStore::open(durable_root)?;
            let search =
                SqlitePrivateNotesSearch::open(durable_root, runtime_root, office_id, org_id)?;
            Ok(PrivateNotesOpened {
                store: Arc::new(store),
                search: Arc::new(search),
            })
        }
        #[cfg(any(test, feature = "testkit"))]
        EngineChoice::Memory => {
            let store = Arc::new(crate::private_notes::memory::MemoryPrivateNotesStore::new());
            let search = crate::search::testkit::MemoryPrivateNotesSearch::new(Arc::clone(&store));
            Ok(PrivateNotesOpened {
                store,
                search: Arc::new(search),
            })
        }
    }
}

/// Opens the office's attachment stores (`TASK-98.1`, ADR-0037): shared blobs and the staging
/// area on every office, folders made lazily; on HQ also the private blobs and the
/// human-private space (`private/user-space.db`). A satellite gets the fail-closed private
/// space and never creates `<durable-root>/private/` (`ARCH-02`).
pub(crate) fn open_attachments(
    engine: EngineChoice,
    durable_root: &Path,
    office_id: &str,
    is_hq: bool,
) -> Result<AttachmentStores, OfficeError> {
    let shared: Arc<dyn BlobStore> = Arc::new(FsBlobStore::new(
        durable_root,
        layout::shared_blobs(durable_root),
    ));
    let staging = StagingArea::new(durable_root, office_id);
    if !is_hq {
        return Ok(AttachmentStores {
            shared,
            private: None,
            staging,
            user_space: Arc::new(SatelliteRefusedUserSpace),
        });
    }
    lay_out(durable_root, &layout::private_notes(durable_root))?;
    let user_space: Arc<dyn UserSpaceStore> = match engine {
        EngineChoice::Sqlite => Arc::new(SqliteUserSpaceStore::open(durable_root)?),
        #[cfg(any(test, feature = "testkit"))]
        EngineChoice::Memory => {
            Arc::new(crate::attachments::user_space::memory::MemoryUserSpaceStore::new())
        }
    };
    Ok(AttachmentStores {
        shared,
        private: Some(Arc::new(FsBlobStore::new(
            durable_root,
            layout::private_blobs(durable_root),
        ))),
        staging,
        user_space,
    })
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
    lay_out(durable_root, &layout::accounting(durable_root))?;
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
    lay_out(durable_root, &layout::budgets(durable_root))?;
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
    lay_out(
        durable_root,
        &layout::provider_bindings(durable_root, office_id),
    )?;
    lay_out(
        durable_root,
        &layout::execution_authority(durable_root, office_id, is_hq),
    )?;
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

/// Opens the engine-independent replication store (`TASK-52`, `FC-145`, `ADR-0018` `ENG-3`):
/// the same durable state for every engine (`TASK-172`), with the engine's projection.
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
    let durable = DurableReplicationParams {
        durable_root: spec.durable_root,
        org_id,
        local_office: local_office.clone(),
        hq_office,
        ca: spec.ca_digest,
        epoch,
        incarnation,
        initial_snapshot_required: spec.initial_snapshot_required,
    };
    let store = match spec.engine {
        EngineChoice::Sqlite => DurableReplicationStore::open(durable, |data| {
            SqliteReplicationProjection::open(spec.runtime_root, &local_office, data)
        })?,
        #[cfg(any(test, feature = "testkit"))]
        EngineChoice::Memory => DurableReplicationStore::open_unprojected(durable)?,
    };
    Ok(Arc::new(store))
}
