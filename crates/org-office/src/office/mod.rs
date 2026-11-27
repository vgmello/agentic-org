//! The office on its store (`docs/design/office-store.md` §4, ADR-0021): one folder that
//! holds the office's identity and its lock, the organization's data under its data roots,
//! written through one acceptance journal, and a projection that can be deleted and
//! rebuilt.
//!
//! ```text
//! <home>/offices/<office-id>/
//!   office.lock            the office lock: exclusive, taken before anything is read
//!   selected-writer.required   the host's writer-custody marker, there before a create
//!   hq-creation.v1.json    the creation intent (version 3), frozen first
//!   office.json            written last: the office is finished
//! <data-root>/<org-id>/durable/   the journal and the organization's files
//! <data-root>/<org-id>/runtime/<office-id>/office.db   the projection
//! ```
//!
//! An office made by the old databases (`org.db`, `office.db`) is refused, never migrated.
//! Nothing here deletes anything under `durable/`.

mod accounting;
mod attachments;
mod budgets;
mod chief_of_staff;
mod companions;
mod creation;
mod dod;
mod folder;
mod git;
mod hiring;
pub(crate) mod import;
mod intent;
mod invitations;
mod messaging;
mod messaging_catchup;
mod opening;
mod org;
mod private_notes;
mod processes;
mod project_docs;
mod providers;
pub(crate) mod recorded;
mod reviews;
mod satellite;
mod search;
mod tasks;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
#[cfg(test)]
mod tests;

use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use org_store::{Clock, Files, Id, OfficeIdentity, StoreError, SystemClock, WriterAuthority};

pub use self::chief_of_staff::{CallerAuthority, DelegatedHireScope};
use self::companions::{open_companion_stores, CompanionOpenSpec};
pub use self::invitations::{
    ack_message_relay_and_enqueue_receipt_checked, admit_relay_item_if_active, is_peer_revoked,
    promote_pending_removed_origin_withdrawals, publish_message_journal_relay_for_dest_checked,
    register_peer_session_stop, run_if_peer_active, ApprovedSatelliteCert, CreateInvitationParams,
    CreateInvitationResult, HqTlsIdentity, InvitationPollStatus, InvitationStatus,
    InvitationSummaryView, JoinRequestAccepted, JoinRequestParams, PeerSessionGuard,
    RelayAdmission, SatelliteRemovalResult,
};
pub use self::providers::OfficePermitSpec;
pub(crate) use self::satellite::check_replicated_hq_owner;
#[cfg(any(test, feature = "testkit"))]
pub mod satellite_testkit {
    pub use super::satellite::{
        read_replicated_hq_owner, remove_replicated_hq_owner, write_replicated_hq_owner,
    };
}
pub use self::satellite::{
    classify_replicated_snapshot_path, export_authoritative_state_snapshot_pages,
    is_immutable_state_path, relocated_task_paths, resolve_employee_office,
    strip_org_core_employee_refs_typed, task_id_for_work_path, ConsistentStateSnapshot,
    ExportedSnapshotPages, OrgDeltaPayload, PendingMessageJournalBatch, PendingMessageJournalEntry,
    RelayedAskAnswer, RelayedAskWithdrawal, RelayedMessageOutcome, RelayedRecipientWake,
    ReplicatedFileBatch, SatelliteFederationConfig, SatelliteProvisionSpec, SatelliteReEnrollSpec,
    SatelliteTlsIdentity, SnapshotExportProgress, SnapshotPathClass,
};
#[cfg(any(test, feature = "testkit"))]
pub use self::testkit::{TestOffice, TestSatellite};
use crate::{
    error::OfficeError,
    hq_creation::{LocalRecord, PublicRecord},
    key::Key,
    model::OfficeInfo,
    office_local::{read_ca, CaRecords},
    store::{
        backend::OfficeStorageBackend,
        open::{EngineChoice, OfficeStanding, OfficeStore, StoreConfig},
    },
};
pub use folder::LOCK_WAIT;

/// An organization's two data roots, both absolute: `<data-root>/<org-id>/durable` and
/// `<data-root>/<org-id>/runtime`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DataRoots {
    pub durable: PathBuf,
    pub runtime: PathBuf,
}

/// Who may write this office's journal: the identity the host reserved for it, and the
/// host's writer custody, which confirms every flush.
#[derive(Clone)]
pub struct Custody {
    pub identity: OfficeIdentity,
    pub authority: Arc<dyn WriterAuthority>,
}

/// Roots that are absolute and plain: no `.` or `..`, and two different folders.
pub(crate) fn check_roots(roots: &DataRoots) -> Result<(), OfficeError> {
    let plain = |path: &Path| {
        path.is_absolute()
            && path.to_str().is_some()
            && path.components().all(|c| {
                matches!(
                    c,
                    std::path::Component::RootDir | std::path::Component::Normal(_)
                )
            })
    };
    if !plain(&roots.durable) || !plain(&roots.runtime) || roots.durable == roots.runtime {
        return Err(OfficeError::Invalid(
            "the data roots are not two plain absolute folders",
        ));
    }
    // The host makes both, owner-only, when it reserves the office's custody.
    for root in [&roots.durable, &roots.runtime] {
        match std::fs::symlink_metadata(root) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return Err(OfficeError::Invalid("a data root is not a folder")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(OfficeError::Invalid("a data root is not there"));
            }
            Err(e) => return Err(OfficeError::storage(e)),
        }
    }
    Ok(())
}

/// The office's local TLS folder: `<durable>/local/offices/<office-id>/tls`.
pub(crate) fn tls_dir(durable_root: &Path, office_id: &str) -> PathBuf {
    durable_root
        .join("local")
        .join("offices")
        .join(office_id)
        .join("tls")
}

/// What the office composes its store from besides the request: production uses SQLite
/// and the wall clock; tests choose.
#[derive(Clone)]
pub struct Options {
    pub engine: EngineChoice,
    pub clock: Arc<dyn Clock>,
    /// Optional explicit storage backend overriding the installed process default.
    pub backend: Option<Arc<dyn OfficeStorageBackend>>,
    #[cfg(any(test, feature = "testkit"))]
    pub testing: crate::store::open::Testing,
    /// Runs once a creation is durable and its files are written, before the final checks
    /// and `office.json`: a test spoils the disk there.
    #[cfg(any(test, feature = "testkit"))]
    pub before_final_checks: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl Options {
    pub(crate) fn production() -> Self {
        Self {
            engine: EngineChoice::Sqlite,
            clock: Arc::new(SystemClock),
            backend: None,
            #[cfg(any(test, feature = "testkit"))]
            testing: Default::default(),
            #[cfg(any(test, feature = "testkit"))]
            before_final_checks: None,
        }
    }

    pub(crate) fn with_backend(backend: Arc<dyn OfficeStorageBackend>) -> Self {
        Self {
            backend: Some(backend),
            ..Self::production()
        }
    }

    fn config(&self, roots: &DataRoots, custody: &Custody) -> StoreConfig {
        let mut config = StoreConfig::new(
            self.engine,
            roots.durable.clone(),
            roots.runtime.clone(),
            custody.identity.clone(),
            Arc::clone(&custody.authority),
            Arc::clone(&self.clock),
        );
        config.backend = self.backend.clone();
        #[cfg(any(test, feature = "testkit"))]
        {
            config.testing = self.testing.clone();
        }
        config
    }
}

/// An HQ office, open on its store. Safe to share across threads.
pub struct Office {
    folder: PathBuf,
    info: OfficeInfo,
    durable: PathBuf,
    runtime_root: PathBuf,
    identity: OfficeIdentity,
    /// The member whose account owns the organization: the creator (on HQ) or the satellite's
    /// own `office_id` for local custody (on satellites).
    owner: Id,
    /// On satellites, the `(hq_office_id, owner_member_id)` pair received over mTLS `Hello`
    /// and persisted in `hq-owner.v1.json` (`TASK-141`); always `None` on HQ.
    replicated_owner: std::sync::RwLock<Option<(String, Id)>>,
    /// The store operation that created the organization.
    creation: Id,
    /// The creation request's canonical lowercase UUIDv7 command ID as the host sent it.
    creation_command: String,
    secrets_key: Option<Key>,
    clock: Arc<dyn Clock>,
    /// Monotonic high-water mark of `clock` (ms), so wall-clock rollbacks never reopen expired windows.
    clock_high_ms: AtomicU64,
    /// In-memory runtime presence and self-reported status lines (`status.set`, `who`).
    runtime: crate::messaging::runtime::OfficeRuntime,
    /// Isolated HQ-authoritative per-author private task notes store (`durable/private/task-notes.db`).
    private_notes: Arc<dyn crate::private_notes::PrivateNotesStore>,
    /// The private notes' disposable search index (HQ only; `TASK-91`).
    private_search: Arc<dyn crate::search::ports::PrivateNotesSearch>,
    /// Attachment bytes, staged uploads and HQ's human-private space (`TASK-98.1`).
    attachments: crate::attachments::AttachmentStores,
    /// Isolated `L2` usage & cost accounting store (`protected/accounting.db`).
    accounting: Arc<dyn crate::accounting::AccountingStore>,
    /// Isolated `L3` budget policies, periods, notices, and holds (`protected/budgets.db`).
    budgets: Arc<dyn crate::budgets::BudgetStore>,
    /// Serializes read-modify-write budget operations across mutations, usage reevaluations,
    /// and turn-completion hold transitions so concurrent calls cannot clobber hold state or
    /// race task cancellation against budget clears (`TASK-97` / Pass 6).
    budget_op_lock: std::sync::Mutex<()>,
    /// Serializes HQ invitation transitions, certificate issuance, revocation writes, and atomic
    /// relay admission (`TASK-52` F8 / P16-#4).
    federation_io_lock: std::sync::Mutex<()>,
    /// Leaf lock protecting reads/writes of `revocations.v1.json` so revocation checks and
    /// snapshot preparation keepalives never block on `federation_io_lock` or store
    /// materialization (`TASK-52` P24-#1).
    revocation_lock: std::sync::Mutex<()>,
    /// Serializes concurrent HQ snapshot exports BEFORE `federation_io_lock` / materialization
    /// so queued exports never hold `federation_io_lock` while waiting (`TASK-52` P24-#1).
    snapshot_export_lock: std::sync::Mutex<()>,
    /// Tracks removed satellite office IDs whose provisional HQ withdrawals have been promoted
    /// in this process lifetime (empty on restart so crash-interrupted promotions always resume,
    /// `TASK-52` P16-#3).
    promoted_removed_offices: std::sync::Mutex<std::collections::BTreeSet<String>>,
    /// Per-peer epoch barriers (`read` for active peer mutations/sends, `write` on
    /// `remove_satellite_office`) scoped to each peer office ID so removing one office never
    /// blocks another office's keepalives or deliveries (`TASK-52` P19-#1 / P19-#2 / P25-#2).
    peer_activity_fences:
        std::sync::Mutex<std::collections::BTreeMap<String, std::sync::Arc<std::sync::RwLock<()>>>>,
    /// Live HQ session stop flags keyed by peer office ID, signaled immediately on removal
    /// (`TASK-52` P19-#2).
    peer_session_stops: std::sync::Mutex<
        std::collections::BTreeMap<String, Vec<std::sync::Arc<std::sync::atomic::AtomicBool>>>,
    >,
    /// Isolated `L4` provider bindings, discovery observations, permits, and cooldowns (`protected/providers.db`).
    providers: Arc<dyn crate::providers::ProviderPoolStore>,
    /// Engine-independent replication store (`TASK-52`, `FC-145`, `ADR-0018` `ENG-3`).
    replication: Arc<dyn crate::replication::ReplicationStore>,
    // The store closes before the lock is released: fields drop in order.
    store: OfficeStore,
    _lock: File,
    #[cfg(any(test, feature = "testkit"))]
    _test_root: Option<tempfile::TempDir>,
}

impl std::fmt::Debug for Office {
    /// What `office.json` records; never a key.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Office")
            .field("folder", &self.folder)
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

/// The one refusal of an HQ whose records disagree: it is never repaired by generating
/// anything again.
pub(crate) fn recovery() -> OfficeError {
    OfficeError::Invalid("HQ provisioning requires recovery")
}

/// A store failure, its path kept in the cause for the log and out of `Display`.
pub(crate) fn storage(error: StoreError) -> OfficeError {
    OfficeError::storage(error)
}

impl Office {
    /// Returns when every accepted entry is flushed and confirmed: a test reading what the
    /// office publishes after a `committed` write (a task create inside its reserved block,
    /// a comment) waits for it here (`ADR-0014`, `ADR-0019`).
    #[cfg(any(test, feature = "testkit"))]
    pub fn sync_store(&self) -> Result<(), OfficeError> {
        self.store.sync().map_err(storage)
    }

    /// Asks the office store to flush and confirm its buffered journal entries now instead of
    /// after the group-flush interval, without blocking the caller (`TASK-140`). Federation
    /// calls it when a cross-office write is waiting on the journal (a sync wake, a pending
    /// relay pipeline) so the entry the relay publisher reads is confirmed in milliseconds.
    pub fn request_store_flush(&self) {
        self.store.request_flush();
    }

    /// The HQ's CA records, checked against the creation (M1): both files there, the
    /// package the key record names, the same public key, organization and operation.
    fn ca(&self) -> Result<CaRecords, OfficeError> {
        let files = Files::open(&self.durable).map_err(OfficeError::storage)?;
        let records = read_ca(&files, &self.identity.office_id).map_err(|_| recovery())?;
        let records = records.ok_or_else(recovery)?;
        let here = &self.identity;
        records
            .check(&here.organization_id, &here.office_id, &self.creation)
            .map_err(|_| recovery())?;
        Ok(records)
    }

    /// Removes an unfinished create: a folder with neither `office.json` nor a creation
    /// intent, nor the host's custody marker or a file it stages one in. One with any of
    /// them is `OfficeExists`; nothing under the data roots is ever touched. A folder that
    /// is not there is fine; one that holds anything an office does not make is `Invalid`.
    pub fn discard(folder: impl Into<PathBuf>, office_id: &str) -> Result<(), OfficeError> {
        creation::discard(folder.into(), office_id)
    }

    /// The orderly stop: the store closes (flushed, confirmed, written, its log folded in)
    /// and leaves the clean-stop marker, then the office lock is released.
    pub fn close(self) -> Result<(), OfficeError> {
        let Office { store, _lock, .. } = self;
        store.close().map_err(storage)
    }

    /// Why the store stopped accepting, if it has: a fixed text and the store's reason,
    /// which names the path. The office then serves nothing more and should exit.
    pub fn fatal(&self) -> Option<String> {
        let fatal = self.store.fatal()?;
        Some(format!("the office store stopped: {fatal}"))
    }

    /// Whether the stop before this opening was orderly.
    pub fn stopped_cleanly(&self) -> bool {
        self.store.stopped_cleanly()
    }

    fn monotonic_now_ms(&self) -> u64 {
        let now = self.clock.now().0;
        self.clock_high_ms
            .fetch_max(now, Ordering::Relaxed)
            .max(now)
    }

    /// Whether this office is its organization's HQ, as the projection holds it.
    pub fn is_hq(&self) -> Result<bool, OfficeError> {
        let read = OfficeStanding {
            office: self.identity.office_id.clone(),
        };
        Ok(self.store.read(read).map_err(storage)?.0.hq)
    }

    /// The HQ's creation record with the wrapped CA key, read from its file and checked.
    /// Satellite offices hold no organization CA and return `Ok(None)`.
    pub fn hq_creation_record(&self) -> Result<Option<LocalRecord>, OfficeError> {
        if self.info.role != crate::model::OfficeRole::Hq {
            return Ok(None);
        }
        let records = self.ca()?;
        let key = records.key;
        LocalRecord::new(
            key.organization_id.to_string(),
            key.operation_id.to_string(),
            key.public_key,
            records.package,
            key.wrapped_ca_key,
        )
        .map(Some)
    }

    /// The canonical lowercase UUIDv7 command ID of the request that created this HQ,
    /// kept in the creation intent and matching the store operation ID.
    pub fn creation_command_id(&self) -> &str {
        &self.creation_command
    }

    /// The HQ's public creation record: the escrow package and what binds it. It carries
    /// no private key in any form. Satellite offices hold no organization CA and return `Ok(None)`.
    pub fn hq_creation_public_record(&self) -> Result<Option<PublicRecord>, OfficeError> {
        if self.info.role != crate::model::OfficeRole::Hq {
            return Ok(None);
        }
        let records = self.ca()?;
        Ok(Some(PublicRecord {
            version: 1,
            organization_id: records.key.organization_id.to_string(),
            operation_id: records.key.operation_id.to_string(),
            public_key: records.key.public_key,
            package: records.package,
        }))
    }

    /// The office secrets key the host handed over, if any. Only for wrapping and
    /// unwrapping the office's private keys; never log, persist or send it.
    pub fn secrets_key(&self) -> Option<&Key> {
        self.secrets_key.as_ref()
    }

    /// What `office.json` records.
    pub fn info(&self) -> &OfficeInfo {
        &self.info
    }

    /// The office's folder.
    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// The organization's durable data root (`<data-root>/<org-id>/durable`).
    pub fn durable_root(&self) -> &Path {
        &self.durable
    }

    /// The canonical UUIDv7 member ID of the organization's current owner (or local satellite
    /// custody ID on a satellite office).
    pub fn owner_member_id(&self) -> &str {
        self.owner.as_str()
    }

    /// The member ID of the organization's active owner as this office knows it: on HQ the
    /// host-authoritative owner, on a satellite the HQ-replicated active owner
    /// (`hq-owner.v1.json`) once a verified `Hello` recorded one, else the satellite's local
    /// custody ID (which names no person). The identity a `[verified: owner]` badge checks.
    pub fn effective_owner_member_id(&self) -> String {
        self.effective_owner_for_ops().as_str().to_owned()
    }

    /// Returns the owner ID for organization/directory/messaging/relay ops: on HQ this is always
    /// the host-authoritative `self.owner`; on a satellite this prefers the HQ-replicated active
    /// owner employee ID from mTLS `Hello` (`hq-owner.v1.json`) when present, else `self.owner`
    /// (`TASK-141`).
    pub(crate) fn effective_owner_for_ops(&self) -> Id {
        if self.info.role == crate::model::OfficeRole::Satellite {
            if let Some((_, id)) = self
                .replicated_owner
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .as_ref()
            {
                return id.clone();
            }
        }
        self.owner.clone()
    }

    /// Persists the HQ-authoritative active owner employee ID received on a verified mTLS `Hello`
    /// handshake for a satellite office (`TASK-141`).
    pub fn record_replicated_hq_owner(
        &self,
        hq_office_id: &str,
        owner_member_id: &str,
    ) -> Result<(), OfficeError> {
        if self.info.role != crate::model::OfficeRole::Satellite {
            return Err(OfficeError::Invalid(
                "only satellite offices record a replicated HQ owner",
            ));
        }
        if !org_proto::is_uuid_v7(hq_office_id)
            || !org_proto::is_valid_member_id(owner_member_id)
            || owner_member_id == self.info.office_id
            || owner_member_id == hq_office_id
        {
            return Err(OfficeError::Invalid("invalid replicated HQ owner id"));
        }
        let parsed = Id::parse(owner_member_id)
            .map_err(|_| OfficeError::Invalid("invalid replicated HQ owner id"))?;
        if self
            .replicated_owner
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .is_some_and(|(hq, id)| hq == hq_office_id && id == &parsed)
        {
            return Ok(());
        }
        let _guard = self
            .federation_io_lock
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if self
            .replicated_owner
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .is_some_and(|(hq, id)| hq == hq_office_id && id == &parsed)
        {
            return Ok(());
        }
        let cp = self.replication.org_checkpoint()?;
        if cp.hq_office.as_str() != hq_office_id {
            return Err(OfficeError::Invalid(
                "replicated HQ owner does not match active HQ checkpoint",
            ));
        }
        satellite::write_replicated_hq_owner(
            &self.durable,
            &self.info.office_id,
            &self.info.org_id,
            hq_office_id,
            &parsed,
        )?;
        *self
            .replicated_owner
            .write()
            .unwrap_or_else(|p| p.into_inner()) = Some((hq_office_id.to_owned(), parsed));
        Ok(())
    }

    /// Engine-independent replication store for `org-sync` state (`TASK-52` / `ENG-3`).
    pub fn replication(&self) -> &Arc<dyn crate::replication::ReplicationStore> {
        &self.replication
    }

    /// Live replication status snapshot for diagnostics and RPCs (`TASK-52`).
    pub fn sync_status(&self) -> Result<crate::replication::SyncStatusSnapshot, OfficeError> {
        self.replication.sync_status()
    }

    /// Registers a durable journal consumer watermark (`ARCH-03` / `TASK-52`).
    pub fn register_replication_consumer(&self, consumer_id: &str) -> Result<u64, OfficeError> {
        self.store.register_consumer(consumer_id).map_err(storage)
    }

    /// Advances a durable journal consumer watermark (`ARCH-03` / `TASK-52`).
    pub fn advance_replication_consumer(
        &self,
        consumer_id: &str,
        ordinal: u64,
    ) -> Result<(), OfficeError> {
        self.store
            .advance_consumer(consumer_id, ordinal)
            .map_err(storage)
    }

    /// Returns retained journal entries strictly after `ordinal` (`ARCH-03` / `TASK-52`).
    pub fn journal_entries_after(&self, ordinal: u64) -> Vec<Arc<org_store::Entry>> {
        self.store.entries_after(ordinal)
    }

    #[cfg(any(test, feature = "testkit"))]
    pub fn dump(&self) -> Vec<String> {
        self.store.dump()
    }
    #[cfg(any(test, feature = "testkit"))]
    pub fn store_for_test(&self) -> &OfficeStore {
        &self.store
    }
    #[cfg(any(test, feature = "testkit"))]
    pub fn accounting_for_test(&self) -> &Arc<dyn crate::accounting::AccountingStore> {
        &self.accounting
    }
    #[cfg(any(test, feature = "testkit"))]
    pub fn budgets_for_test(&self) -> &Arc<dyn crate::budgets::BudgetStore> {
        &self.budgets
    }
    #[cfg(any(test, feature = "testkit"))]
    pub fn private_notes_for_test(&self) -> &Arc<dyn crate::private_notes::PrivateNotesStore> {
        &self.private_notes
    }
    #[cfg(any(test, feature = "testkit"))]
    pub fn retain_test_root(&mut self, root: tempfile::TempDir) {
        self._test_root = Some(root);
    }
}
