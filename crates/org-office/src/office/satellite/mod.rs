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

pub(crate) use enrollment::validate_satellite_open_binding;
pub use enrollment::{SatelliteProvisionSpec, SatelliteReEnrollSpec};
pub use filters::{
    classify_replicated_snapshot_path, is_stale_snapshot_task_entry,
    strip_org_core_employee_refs_typed, task_id_for_work_path, SnapshotPathClass,
};
pub(crate) use identity::{
    read_active_identity_binding, read_ca_digest_for_open, read_intent, tls_dir,
};
pub use identity::{SatelliteFederationConfig, SatelliteTlsIdentity};
pub(crate) use placement::has_local_task_write_authority;
pub use placement::resolve_employee_office;
pub use relay::{
    PendingMessageJournalEntry, RelayedAskAnswer, RelayedAskWithdrawal, RelayedMessageOutcome,
    RelayedRecipientWake,
};
pub(crate) use replication::reconcile_satellite_replication_on_open;
pub use replication::{is_immutable_state_path, OrgDeltaPayload};
pub use snapshot::{
    export_authoritative_state_snapshot_pages, ExportedSnapshotPages, SnapshotExportProgress,
};

#[cfg(test)]
mod replicated_kind_tests;
