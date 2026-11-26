//! Satellite office provisioning, local TLS identity persistence, D25 one-command
//! re-enrollment after R1 replacement HQ import, and replicated `state/` application
//! (`TASK-52` / `S1`, `ARCH-02`).

mod enrollment;
mod filters;
mod identity;
mod placement;
mod relay;
mod replication;
mod snapshot;
mod work_ingest;

use org_store::DurablePath;

pub(crate) use enrollment::validate_satellite_open_binding;
pub use enrollment::{SatelliteProvisionSpec, SatelliteReEnrollSpec};
pub use filters::{
    classify_replicated_snapshot_path, is_stale_snapshot_task_entry,
    strip_org_core_employee_refs_typed, task_id_for_work_path, SnapshotPathClass,
};
pub(crate) use identity::{read_active_identity_binding, read_ca_digest_for_open, read_intent};
pub use identity::{SatelliteFederationConfig, SatelliteTlsIdentity};
pub(crate) use placement::has_local_task_write_authority;
pub use placement::resolve_employee_office;
pub use relay::{
    PendingMessageJournalBatch, PendingMessageJournalEntry, RelayedAskAnswer, RelayedAskWithdrawal,
    RelayedMessageOutcome, RelayedRecipientWake,
};
pub(crate) use replication::reconcile_satellite_replication_on_open;
pub use replication::{ConsistentStateSnapshot, OrgDeltaPayload};
pub use snapshot::{
    export_authoritative_state_snapshot_pages, ExportedSnapshotPages, SnapshotExportProgress,
};

/// Decoded batch of `(state_path, Some(bytes) for put / None for delete)`.
pub type ReplicatedFileBatch = Vec<(String, Option<Vec<u8>>)>;

/// Returns true when `path` is an append-only immutable task or messaging file that must NEVER be
/// deleted during snapshot reconciliation (`TASK-52` H-9).
pub fn is_immutable_state_path(path: &str) -> bool {
    let Ok(dp) = DurablePath::parse(path) else {
        return false;
    };
    matches!(
        crate::tasks::records::kind_of(&dp),
        Some(crate::tasks::records::Kind::Event(_, _) | crate::tasks::records::Kind::Comment(_, _))
    ) || matches!(
        crate::messaging::records::kind_of(&dp),
        Some(crate::messaging::records::Kind::Message(_, _))
    )
}

#[cfg(test)]
mod replicated_kind_tests;
