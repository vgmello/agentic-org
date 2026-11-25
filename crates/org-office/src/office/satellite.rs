//! Satellite office provisioning, local TLS identity persistence, D25 one-command
//! re-enrollment after R1 replacement HQ import, and replicated `state/` application
//! (`TASK-52` / `S1`, `ARCH-02`).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use org_pki::{
    cert::{cert_fingerprint_sha256, verify_office_cert, OfficeCertRole},
    wrap::{self, WrapBinding, WrapPurpose},
    SigningKey, Zeroizing,
};
use org_store::{journal_dir, Actor, AnchorFacts, DurablePath, Files, Id, Opening, Outcome};
use org_sync_proto::{Digest, Epoch, OfficeId};
use serde::{Deserialize, Serialize};

use super::{
    check_roots,
    folder::{
        check_named_by, check_not_a_link, exists, lock_folder, plain_folder, refuse_older_build,
    },
    open_companion_stores, storage, Custody, DataRoots, Office, Options, LOCK_WAIT,
};
use crate::{
    error::OfficeError,
    key::Key,
    model::{NewOffice, OfficeInfo, OfficeRole},
    office_file,
    organization::features::offices::ApplyReplicatedFiles,
    private_fs::{private_dir, private_dir_synced, tighten_dir, write_private_atomic},
    store::open::{open_store, ApplyReplicatedFilesOp},
};

pub(crate) const SATELLITE_INTENT_FILE: &str = "satellite-join.v1.json";
const INTENT_LIMIT: u64 = 64 * 1024;
const INTENT_VERSION: u32 = 1;

/// Durable satellite join metadata frozen in `<home>/offices/<office-id>/satellite-join.v1.json`.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SatelliteJoinIntent {
    pub version: u32,
    pub instance: String,
    pub journal_incarnation: String,
    pub writer_epoch: u64,
    pub machine_id: String,
    pub operation: String,
    pub office_id: String,
    pub office_name: String,
    pub org_id: String,
    pub org_name: String,
    pub hq_office_id: String,
    pub hq_addresses: Vec<String>,
    pub epoch: u64,
    pub ca_fingerprint_hex: String,
    pub created_at: String,
    pub durable_root: PathBuf,
    pub runtime_root: PathBuf,
}

impl SatelliteJoinIntent {
    pub(crate) fn info(&self) -> OfficeInfo {
        OfficeInfo {
            office_id: self.office_id.clone(),
            office_name: self.office_name.clone(),
            role: OfficeRole::Satellite,
            org_id: self.org_id.clone(),
            org_name: self.org_name.clone(),
            created_at: self.created_at.clone(),
        }
    }

    pub(crate) fn roots(&self) -> DataRoots {
        DataRoots {
            durable: self.durable_root.clone(),
            runtime: self.runtime_root.clone(),
        }
    }
}

/// Persisted satellite federation binding under `durable/local/offices/<office_id>/tls/federation.v1.json`.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SatelliteFederationConfig {
    pub version: u32,
    pub org_id: String,
    pub local_office_id: String,
    pub hq_office_id: String,
    pub hq_addresses: Vec<String>,
    pub epoch: u64,
    pub ca_fingerprint_hex: String,
    pub cert_serial_hex: String,
}

/// One locally committed native messaging journal entry ready for cross-office relay publication.
#[derive(Clone, Debug)]
pub struct PendingMessageJournalEntry {
    pub ordinal: u64,
    pub conversation_id: String,
    pub message_id: String,
    pub files: Vec<(String, Option<String>)>,
    /// Target offices determined by recipient placement (for DMs/GroupDMs) or channel membership
    /// (`TASK-52` P3-#1 / P4-#2).
    pub destination_offices: Vec<String>,
    /// Native message expiry timestamp in Unix ms (e.g. non-required ask expiry), preserved in
    /// transport deadlines (`TASK-52` P4-#9).
    pub expires_at_ms: Option<u64>,
}

/// Structured answer summary returned when an inbound relayed reply closes a local ask whose
/// asker is an active agent owned by this office (`TASK-52` P6-#2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayedAskAnswer {
    pub ask_id: String,
    pub asker_id: String,
    pub question_summary: String,
    pub answer_summary: String,
}

/// Structured withdrawal summary returned when an inbound relayed withdrawal closes a local ask
/// whose asker is an active agent owned by this office (`TASK-52` P13-#4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayedAskWithdrawal {
    pub ask_id: String,
    pub asker_id: String,
}

/// Exact wake action derived for one local active agent recipient of a relayed message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayedRecipientWake {
    pub employee_id: String,
    pub kind: String,
}

/// Summary of a durably committed inbound relayed native message so the destination office can
/// derive and deduplicate local agent wakes, resume suspended required askers, and notify local
/// waiters/live streams (`TASK-52` P4-#8 / P6-#2, `TASK-117`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RelayedMessageOutcome {
    pub message_id: String,
    pub conversation_id: String,
    pub sender_id: String,
    pub sender_name: String,
    pub sender_is_person: bool,
    pub is_dm: bool,
    #[serde(default)]
    pub inserted: bool,
    #[serde(default)]
    pub conversation_kind: Option<org_proto::chat::ConversationKind>,
    pub summary: String,
    pub local_agent_recipients: Vec<String>,
    #[serde(default)]
    pub local_recipient_wakes: Vec<RelayedRecipientWake>,
    #[serde(default)]
    pub local_delivered_headers: Vec<org_proto::msg::MessageHeader>,
    #[serde(default)]
    pub answered_ask: Option<RelayedAskAnswer>,
    #[serde(default)]
    pub withdrawn_ask: Option<RelayedAskWithdrawal>,
    #[serde(default)]
    pub closed_ask_id: Option<String>,
    #[serde(default)]
    pub closed_ask_reason: Option<String>,
    #[serde(default)]
    pub handoff_accepted: bool,
    #[serde(skip)]
    pub delivered_message: Option<crate::messaging::records::MessageRecord>,
}

/// Batch returned by [`Office::collect_pending_message_journal_entries`].
#[derive(Clone, Debug)]
pub struct PendingMessageJournalBatch {
    pub incarnation: String,
    pub last_ordinal: u64,
    pub entries: Vec<PendingMessageJournalEntry>,
}

/// Loaded satellite mTLS credentials (never logged).
pub struct SatelliteTlsIdentity {
    pub ca_cert_der: Vec<u8>,
    pub office_cert_der: Vec<u8>,
    pub office_key_der: Zeroizing<Vec<u8>>,
    pub federation: SatelliteFederationConfig,
}

impl std::fmt::Debug for SatelliteTlsIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SatelliteTlsIdentity")
            .field("federation", &self.federation)
            .field("office_key_der", &"<redacted>")
            .finish()
    }
}

#[derive(Serialize, Deserialize)]
struct WrappedTlsKeyFile {
    v: u32,
    wrapped_b64: String,
}

/// Single atomic commit file for a satellite's active TLS + HQ binding generation (`TASK-52` F13).
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AtomicSatelliteIdentityFile {
    v: u32,
    federation: SatelliteFederationConfig,
    ca_cert_der_b64: String,
    office_cert_der_b64: String,
    wrapped_key_b64: String,
}

pub(crate) fn read_active_identity_binding(
    durable_root: &Path,
    office_id: &str,
) -> Result<Option<SatelliteFederationConfig>, OfficeError> {
    let path = tls_dir(durable_root, office_id).join("federation-identity.v1.json");
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(&path).map_err(OfficeError::storage)?;
    let parsed: AtomicSatelliteIdentityFile = serde_json::from_slice(&bytes)
        .map_err(|_| OfficeError::Invalid("malformed atomic satellite identity file"))?;
    if parsed.v != 1 {
        return Err(OfficeError::Invalid(
            "unsupported satellite identity version",
        ));
    }
    Ok(Some(parsed.federation))
}

/// Inputs to provision a newly enrolled satellite office on disk.
pub struct SatelliteProvisionSpec {
    pub new: NewOffice,
    pub operation_id: String,
    pub hq_office_id: String,
    pub hq_addresses: Vec<String>,
    pub epoch: u64,
    pub ca_cert_der: Vec<u8>,
    pub office_cert_der: Vec<u8>,
    pub office_key_der: Zeroizing<Vec<u8>>,
}

/// Inputs to re-enroll an existing satellite onto a replacement HQ (D25).
pub struct SatelliteReEnrollSpec {
    pub org_id: String,
    pub new_hq_office_id: String,
    pub new_hq_addresses: Vec<String>,
    pub new_epoch: u64,
    pub new_ca_cert_der: Vec<u8>,
    pub new_office_cert_der: Vec<u8>,
    pub new_office_key_der: Zeroizing<Vec<u8>>,
}

pub(crate) fn read_intent(folder: &Path) -> Result<Option<SatelliteJoinIntent>, OfficeError> {
    let Some(bytes) = org_store::read_one(folder, SATELLITE_INTENT_FILE, INTENT_LIMIT)
        .map_err(OfficeError::storage)?
    else {
        return Ok(None);
    };
    let intent: SatelliteJoinIntent = serde_json::from_slice(&bytes)
        .map_err(|_| OfficeError::Invalid("malformed satellite join intent"))?;
    if intent.version != INTENT_VERSION {
        return Err(OfficeError::Invalid(
            "unsupported satellite join intent version",
        ));
    }
    Ok(Some(intent))
}

pub(crate) fn write_intent(
    folder: &Path,
    intent: &SatelliteJoinIntent,
    replace: bool,
) -> Result<(), OfficeError> {
    let bytes = serde_json::to_vec_pretty(intent).map_err(OfficeError::storage)?;
    if bytes.len() as u64 > INTENT_LIMIT {
        return Err(OfficeError::Invalid("satellite join intent too large"));
    }
    org_store::publish_one(folder, SATELLITE_INTENT_FILE, &bytes, !replace)
        .map_err(OfficeError::storage)
}

pub(crate) fn tls_dir(durable_root: &Path, office_id: &str) -> PathBuf {
    durable_root
        .join("local")
        .join("offices")
        .join(office_id)
        .join("tls")
}

pub(crate) fn persist_satellite_tls(
    durable_root: &Path,
    office_id: &str,
    secrets_key: &Key,
    federation: &SatelliteFederationConfig,
    ca_cert_der: &[u8],
    office_cert_der: &[u8],
    office_key_der: &[u8],
) -> Result<(), OfficeError> {
    let dir = tls_dir(durable_root, office_id);
    private_dir_synced(durable_root, &dir).map_err(OfficeError::storage)?;
    let wrapped = wrap::wrap(
        secrets_key.expose_bytes(),
        &WrapBinding {
            organization_id: &federation.org_id,
            office_id,
            purpose: WrapPurpose::OfficeTlsKey,
        },
        office_key_der,
    )
    .map_err(|_| OfficeError::Invalid("failed wrapping satellite TLS key"))?;
    let wrapped_b64 = STANDARD.encode(&wrapped);
    let atomic_identity = AtomicSatelliteIdentityFile {
        v: 1,
        federation: federation.clone(),
        ca_cert_der_b64: STANDARD.encode(ca_cert_der),
        office_cert_der_b64: STANDARD.encode(office_cert_der),
        wrapped_key_b64: wrapped_b64.clone(),
    };
    let identity_bytes =
        serde_json::to_vec_pretty(&atomic_identity).map_err(OfficeError::storage)?;
    // Single atomic commit point for a satellite's complete TLS + HQ binding generation (`TASK-52` F13).
    write_private_atomic(&dir.join("federation-identity.v1.json"), &identity_bytes)
        .map_err(OfficeError::storage)?;
    let key_file = WrappedTlsKeyFile { v: 1, wrapped_b64 };
    let key_bytes = serde_json::to_vec_pretty(&key_file).map_err(OfficeError::storage)?;
    let fed_bytes = serde_json::to_vec_pretty(federation).map_err(OfficeError::storage)?;
    write_private_atomic(&dir.join("org-ca.v1.der"), ca_cert_der).map_err(OfficeError::storage)?;
    write_private_atomic(&dir.join("office-cert.v1.der"), office_cert_der)
        .map_err(OfficeError::storage)?;
    write_private_atomic(&dir.join("office-key.v1.json"), &key_bytes)
        .map_err(OfficeError::storage)?;
    write_private_atomic(&dir.join("federation.v1.json"), &fed_bytes)
        .map_err(OfficeError::storage)?;
    Ok(())
}

fn validate_satellite_cert_and_key(
    org_id: &str,
    office_id: &str,
    ca_cert_der: &[u8],
    office_cert_der: &[u8],
    office_key_der: &[u8],
    now_unix: i64,
) -> Result<String, OfficeError> {
    let _ = SigningKey::from_local_der(office_key_der)
        .map_err(|_| OfficeError::Invalid("invalid satellite TLS private key"))?;
    let verified = verify_office_cert(
        ca_cert_der,
        office_cert_der,
        org_id,
        Some(OfficeCertRole::Satellite),
        &Default::default(),
        now_unix,
    )
    .map_err(|_| OfficeError::Invalid("satellite certificate failed Org CA verification"))?;
    if verified.office_id != office_id {
        return Err(OfficeError::Invalid(
            "satellite certificate office_id does not match local office",
        ));
    }
    Ok(verified.serial_hex)
}

impl Office {
    /// Provisions an enrolled satellite office on disk with its pinned Org CA, `ClientAuth`-only
    /// certificate, wrapped TLS private key, initial replication store, and `office.json` written last.
    pub fn provision_satellite(
        folder: impl Into<PathBuf>,
        spec: SatelliteProvisionSpec,
        key: Key,
        roots: DataRoots,
        custody: Custody,
    ) -> Result<Office, OfficeError> {
        Self::provision_satellite_with(
            folder.into(),
            spec,
            key,
            roots,
            custody,
            Options::production(),
            true,
        )
    }

    /// Provisions or resumes a satellite office WITHOUT publishing `office.json` yet so `join_and_serve`
    /// can finish initial snapshot installation before marking the office joined (`TASK-52` H-4).
    pub fn provision_satellite_pending(
        folder: impl Into<PathBuf>,
        spec: SatelliteProvisionSpec,
        key: Key,
        roots: DataRoots,
        custody: Custody,
    ) -> Result<Office, OfficeError> {
        Self::provision_satellite_with(
            folder.into(),
            spec,
            key,
            roots,
            custody,
            Options::production(),
            false,
        )
    }

    /// Publishes `office.json` after initial snapshot installation succeeds (`TASK-52` H-4).
    pub fn publish_satellite_office_file(&self) -> Result<(), OfficeError> {
        if self.info.role != OfficeRole::Satellite {
            return Err(OfficeError::Invalid(
                "only satellite offices publish deferred office.json",
            ));
        }
        let office_bytes = office_file::encode(&self.info)?;
        org_store::publish_one(&self.folder, office_file::FILE_NAME, &office_bytes, true)
            .map_err(OfficeError::storage)
    }

    pub(crate) fn provision_satellite_with(
        folder: PathBuf,
        spec: SatelliteProvisionSpec,
        key: Key,
        roots: DataRoots,
        custody: Custody,
        options: Options,
        publish_office_file: bool,
    ) -> Result<Office, OfficeError> {
        provision_satellite_impl(
            folder,
            spec,
            key,
            roots,
            custody,
            options,
            publish_office_file,
        )
    }

    /// Re-enrolls an existing satellite onto a replacement HQ after R1 Git import (D25),
    /// replacing pinned CA/cert/key/HQ addresses and resetting the replication checkpoint to
    /// `(new_epoch, 0)` with `initial_snapshot_required = true` while keeping the same `office_id`.
    pub fn re_enroll_satellite(&self, spec: SatelliteReEnrollSpec) -> Result<(), OfficeError> {
        re_enroll_satellite_impl(self, spec)
    }

    /// Reads this satellite's persisted federation configuration, preferring the single atomic
    /// `federation-identity.v1.json` generation file (`TASK-52` F13).
    pub fn satellite_federation_config(
        &self,
    ) -> Result<Option<SatelliteFederationConfig>, OfficeError> {
        if let Some(fed) = read_active_identity_binding(&self.durable, &self.info.office_id)? {
            return Ok(Some(fed));
        }
        let path = tls_dir(&self.durable, &self.info.office_id).join("federation.v1.json");
        if !path.is_file() {
            return Ok(None);
        }
        let bytes = fs::read(&path).map_err(OfficeError::storage)?;
        let cfg: SatelliteFederationConfig = serde_json::from_slice(&bytes)
            .map_err(|_| OfficeError::Invalid("malformed satellite federation config"))?;
        Ok(Some(cfg))
    }

    /// Loads and unwraps this satellite's pinned Org CA, leaf cert, and TLS private key from its
    /// single atomic `federation-identity.v1.json` generation file (with fallback for pre-upgrade dirs).
    pub fn load_satellite_tls_identity(&self) -> Result<SatelliteTlsIdentity, OfficeError> {
        if self.info.role != OfficeRole::Satellite {
            return Err(OfficeError::Invalid("not a satellite office"));
        }
        let secrets_key = self
            .secrets_key
            .as_ref()
            .ok_or(OfficeError::Invalid("office secrets key required"))?;
        let dir = tls_dir(&self.durable, &self.info.office_id);
        let atomic_path = dir.join("federation-identity.v1.json");
        let (ca_cert_der, office_cert_der, wrapped_b64, federation) = if atomic_path.is_file() {
            let bytes = fs::read(&atomic_path).map_err(OfficeError::storage)?;
            let parsed: AtomicSatelliteIdentityFile = serde_json::from_slice(&bytes)
                .map_err(|_| OfficeError::Invalid("malformed atomic satellite identity"))?;
            let ca = STANDARD
                .decode(&parsed.ca_cert_der_b64)
                .map_err(|_| OfficeError::Invalid("malformed ca_cert_der_b64"))?;
            let cert = STANDARD
                .decode(&parsed.office_cert_der_b64)
                .map_err(|_| OfficeError::Invalid("malformed office_cert_der_b64"))?;
            (ca, cert, parsed.wrapped_key_b64, parsed.federation)
        } else {
            let ca = fs::read(dir.join("org-ca.v1.der")).map_err(OfficeError::storage)?;
            let cert = fs::read(dir.join("office-cert.v1.der")).map_err(OfficeError::storage)?;
            let key_bytes =
                fs::read(dir.join("office-key.v1.json")).map_err(OfficeError::storage)?;
            let key_file: WrappedTlsKeyFile = serde_json::from_slice(&key_bytes)
                .map_err(|_| OfficeError::Invalid("malformed wrapped satellite TLS key"))?;
            let fed = self
                .satellite_federation_config()?
                .ok_or(OfficeError::Invalid("missing satellite federation config"))?;
            (ca, cert, key_file.wrapped_b64, fed)
        };
        let wrapped = STANDARD
            .decode(&wrapped_b64)
            .map_err(|_| OfficeError::Invalid("malformed wrapped TLS key base64"))?;
        let office_key_der = wrap::unwrap(
            secrets_key.expose_bytes(),
            &WrapBinding {
                organization_id: &self.info.org_id,
                office_id: &self.info.office_id,
                purpose: WrapPurpose::OfficeTlsKey,
            },
            &wrapped,
        )
        .map_err(|_| OfficeError::Invalid("failed unwrapping satellite TLS key"))?;
        Ok(SatelliteTlsIdentity {
            ca_cert_der,
            office_cert_der,
            office_key_der,
            federation,
        })
    }

    /// Applies an HQ-authoritative batch of `state/` files onto this satellite's `org-store`.
    pub fn apply_replicated_state_files(
        &self,
        operation_id: &str,
        files: Vec<(String, Option<Vec<u8>>)>,
    ) -> Result<(), OfficeError> {
        if self.info.role != OfficeRole::Satellite {
            return Err(OfficeError::Invalid(
                "only satellites apply replicated state batches",
            ));
        }
        self.apply_replicated_files_inner(operation_id, files)
    }

    /// Applies a validated satellite-originated work file batch strictly under projector-owned
    /// work families (`tasks` or `messaging`), refusing HQ-owned organization/office/project paths
    /// and any unowned paths (`TASK-52` P2-1).
    pub fn apply_replicated_work_files(
        &self,
        operation_id: &str,
        files: Vec<(String, Option<Vec<u8>>)>,
    ) -> Result<(), OfficeError> {
        if self.info.role != OfficeRole::Hq {
            return Err(OfficeError::Invalid(
                "only HQ ingests satellite work batches",
            ));
        }
        validate_work_file_paths(&files)?;
        self.apply_replicated_files_inner(operation_id, files)
    }

    /// Validates a satellite's typed work batch against HQ's organization snapshot (actor/team
    /// authority, monotonic task revision, immutable events) and commits it atomically with
    /// `origin` high-water under `federation_io_lock` (`TASK-52` H-7).
    pub fn ingest_validated_satellite_work(
        &self,
        origin: &org_sync::work::Origin,
        seq: org_sync_proto::SourceSeq,
        expected_record_id: &str,
        operation_id: &str,
        files: Vec<(String, Option<Vec<u8>>)>,
    ) -> Result<u64, OfficeError> {
        if self.info.role != OfficeRole::Hq {
            return Err(OfficeError::Invalid(
                "only HQ ingests satellite work batches",
            ));
        }
        let _guard = self
            .federation_io_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let prev_hw = self.replication.remote_work_high_water(origin)?;
        if seq.get() <= prev_hw {
            return Ok(prev_hw);
        }
        if seq.get() != prev_hw.saturating_add(1) {
            return Err(OfficeError::Invalid("non-contiguous satellite work seq"));
        }
        validate_work_file_paths(&files)?;
        validate_typed_work_files_against_hq(
            self,
            origin.office.as_str(),
            expected_record_id,
            &files,
        )?;
        self.apply_replicated_files_inner(operation_id, files)?;
        self.replication.record_remote_work_seq(origin, seq)?;
        Ok(prev_hw)
    }

    /// Validates and atomically applies an inbound relayed native message (with optional new
    /// conversation and structured ask answer) to this office's messaging store, synthesizing
    /// recipient-state rows ONLY for recipients owned by this office (`TASK-52` P3-#2/#3, P4-#1..#4/#8/#9).
    pub fn apply_relayed_messaging_files(
        &self,
        origin_office: &str,
        message_id: &str,
        operation_id: &str,
        files: Vec<(String, Option<Vec<u8>>)>,
    ) -> Result<RelayedMessageOutcome, OfficeError> {
        apply_validated_relayed_message(self, origin_office, message_id, operation_id, files)
    }

    fn apply_replicated_files_inner(
        &self,
        operation_id: &str,
        files: Vec<(String, Option<Vec<u8>>)>,
    ) -> Result<(), OfficeError> {
        let op_id = Id::parse(operation_id)
            .map_err(|_| OfficeError::Invalid("invalid replication operation_id"))?;
        self.replication.record_replicated_op_id(operation_id)?;
        let reply = self
            .store
            .run(ApplyReplicatedFilesOp {
                operation_id: op_id,
                actor: Actor::System,
                command: ApplyReplicatedFiles { files },
            })
            .map_err(storage)?;
        match reply.outcome {
            Outcome::Completed => {
                self.store.sync().map_err(storage)?;
                Ok(())
            }
            Outcome::Rejected => Err(OfficeError::Invalid("replicated state batch rejected")),
        }
    }

    /// Publishes any custody-confirmed, un-replicated HQ `org-store` journal entries under
    /// `state/` as typed `org-sync` `OrgEvent` deltas using the durable `ARCH-03` consumer
    /// `"org-sync-hq"`, atomically recording `(journal_incarnation, ordinal)` in one transaction
    /// and splitting oversized multi-file entries within `MAX_EVENT_PAYLOAD_BYTES` (`TASK-52` H-12/M-16).
    pub fn sync_org_journal_to_replication(&self) -> Result<usize, OfficeError> {
        if self.info.role != OfficeRole::Hq {
            return Ok(0);
        }
        let _guard = self
            .federation_io_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let confirmed = self.store.status().confirmed;
        sync_org_journal_locked_up_to(self, confirmed)
    }

    /// Feeds locally accepted satellite task journal entries into this satellite's durable
    /// `org-sync` work outbox via the `ARCH-03` consumer `"org-sync-work-outbox"`, skipping entries
    /// produced by inbound HQ replication applies (`TASK-52` H-6).
    pub fn sync_satellite_work_outbox(&self) -> Result<usize, OfficeError> {
        if self.info.role != OfficeRole::Satellite {
            return Ok(0);
        }
        let _guard = self
            .federation_io_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        sync_satellite_work_outbox_locked(self)
    }

    /// Captures HQ's checkpoint and `state/` snapshot at the exact same `org-store` materialization
    /// boundary so concurrent writers can never interleave between checkpoint and walk (`TASK-52` H-11).
    pub fn collect_authoritative_state_snapshot_consistent(
        &self,
    ) -> Result<ConsistentStateSnapshot, OfficeError> {
        let _guard = self
            .federation_io_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut captured = None;
        let mut capture_err: Option<OfficeError> = None;
        self.store
            .with_consistent_materialized(|mat| {
                let res = (|| -> Result<ConsistentStateSnapshot, OfficeError> {
                    let _ = sync_org_journal_locked_up_to(self, mat)?;
                    let checkpoint = self.replication.org_checkpoint()?;
                    let files = walk_state_files(&self.durable)?;
                    Ok(ConsistentStateSnapshot { checkpoint, files })
                })();
                match res {
                    Ok(snap) => captured = Some(snap),
                    Err(e) => capture_err = Some(e),
                }
                Ok(())
            })
            .map_err(storage)?;
        if let Some(err) = capture_err {
            return Err(err);
        }
        captured.ok_or(OfficeError::Invalid("snapshot capture missing result"))
    }

    /// Lists all current `state/` paths on this office (used by satellites during D25 snapshot
    /// installation to delete stale files absent from a replacement HQ, `TASK-52` P2-8).
    pub fn list_local_state_paths(&self) -> Result<Vec<String>, OfficeError> {
        self.store.sync().map_err(storage)?;
        let files = Files::open(&self.durable).map_err(OfficeError::storage)?;
        let state_prefix = DurablePath::parse("state").map_err(storage)?;
        let mut out = Vec::new();
        for path in files.walk(&state_prefix).map_err(OfficeError::storage)? {
            let s = path.as_str();
            if s.starts_with("state/") {
                out.push(s.to_owned());
            }
        }
        out.sort();
        Ok(out)
    }

    /// Collects ONLY `conversation.json` and the single `messages/<message_id>.json` file for
    /// bounded cross-office message relay (never copying historical messages, `TASK-52` #9/#10).
    pub fn collect_single_message_relay_files(
        &self,
        conversation_id: &str,
        message_id: &str,
    ) -> Result<Vec<(String, Option<String>)>, OfficeError> {
        self.store.sync().map_err(storage)?;
        let files = Files::open(&self.durable).map_err(OfficeError::storage)?;
        let conv_path = DurablePath::parse(&format!(
            "messaging/conversations/{conversation_id}/conversation.json"
        ))
        .map_err(storage)?;
        let msg_path = DurablePath::parse(&format!(
            "messaging/conversations/{conversation_id}/messages/{message_id}.json"
        ))
        .map_err(storage)?;
        let mut out = Vec::with_capacity(2);
        if let Some(bytes) = files.read(&conv_path).map_err(OfficeError::storage)? {
            out.push((conv_path.as_str().to_owned(), Some(STANDARD.encode(bytes))));
        }
        if let Some(bytes) = files.read(&msg_path).map_err(OfficeError::storage)? {
            out.push((msg_path.as_str().to_owned(), Some(STANDARD.encode(bytes))));
        }
        Ok(out)
    }

    /// Scans newly committed native messaging journal entries via the `ARCH-03` consumer
    /// `"org-sync-message-relay"`, returning bounded per-message relay files and recipient-routed
    /// destination offices for each locally committed message (`TASK-52` #10 / P3-#1 / P3-#4).
    pub fn collect_pending_message_journal_entries(
        &self,
    ) -> Result<PendingMessageJournalBatch, OfficeError> {
        let _guard = self
            .federation_io_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let wm = self.register_replication_consumer("org-sync-message-relay")?;
        let confirmed = self.store.status().confirmed;
        let incarnation = self.identity.journal_incarnation.as_str().to_owned();
        let entries = self.store.entries_after_contiguous(wm).map_err(storage)?;
        let ctx = load_placement_context_locked(self)?;
        let mut out = Vec::new();
        let mut last_ord = wm;
        for entry in entries {
            let ord = entry.ordinal();
            if ord > confirmed {
                break;
            }
            let from_repl = is_replicated_journal_entry(&entry, self.replication.as_ref());
            if from_repl {
                // Even when a journal entry came from `replication.apply_message` or
                // `replication.apply_origin_close` (e.g. HQ owner withdrawal accepted at the
                // ask's origin), if this office is the ask's origin and just committed an
                // authoritative `"origin-close"`, publish that close durably to participants
                // (`TASK-52` P11-#3 / P13-#3).
                if entry.header.operation.as_ref().is_some_and(|op| {
                    op.kind == "replication.apply_message"
                        || op.kind == "replication.apply_origin_close"
                }) && !append_origin_close_answer_relays(
                    self, &ctx, ord, &entry, true, &mut out,
                )? {
                    break;
                }
            } else if !append_message_relay_entries(self, &ctx, ord, &entry, &mut out)? {
                // Conversation record not yet readable from committed generation; keep watermark
                // strictly before this entry so it is retried safely (`TASK-52` P3-#4).
                break;
            }
            last_ord = last_ord.max(ord);
        }
        Ok(PendingMessageJournalBatch {
            incarnation,
            last_ordinal: last_ord,
            entries: out,
        })
    }

    /// Reconciles an entire incoming snapshot against committed local satellite-owned tasks and
    /// pending local outbox edits BEFORE chunking into <= 8 MiB batches (`TASK-52` P11-#2 / P12-#1):
    /// - Uses strict `local.revision > incoming.revision` so an interrupted snapshot resuming at
    ///   equal revision still applies any missing sidecars.
    /// - Scans all incoming `task.json` entries BEFORE removing `pending_work_paths` and protects
    ///   every task (and all its sidecars) touched by `pending_work_paths`.
    pub fn filter_stale_snapshot_work_files(
        &self,
        mut files: ReplicatedFileBatch,
        pending_work_paths: &std::collections::BTreeSet<String>,
    ) -> Result<ReplicatedFileBatch, OfficeError> {
        if self.info.role != crate::model::OfficeRole::Satellite {
            return Ok(files);
        }
        let mut protected_tasks = collect_task_ids_from_paths(pending_work_paths);
        for (path, opt_bytes) in &files {
            let Some(bytes) = opt_bytes else {
                continue;
            };
            let Ok(dp) = DurablePath::parse(path) else {
                continue;
            };
            let Some(crate::tasks::records::Kind::Task(tid)) = crate::tasks::records::kind_of(&dp)
            else {
                continue;
            };
            let incoming = serde_json::from_slice::<crate::tasks::records::TaskRecord>(bytes)
                .map_err(|_| OfficeError::Invalid("malformed TaskRecord in snapshot"))?;
            match self.task_inspect(tid.as_str())? {
                Ok(local) => {
                    if local.task.office_code == incoming.office_code
                        && local.task.revision > incoming.meta.revision
                    {
                        protected_tasks.insert(tid);
                    }
                }
                Err(api_err) if api_err.code == org_proto::ErrorCode::NotFound => {}
                Err(_) => {
                    return Err(OfficeError::Invalid(
                        "failed inspecting local task during snapshot reconciliation",
                    ));
                }
            }
        }
        if protected_tasks.is_empty() && pending_work_paths.is_empty() {
            return Ok(files);
        }
        files.retain(|(path, opt_bytes)| {
            if pending_work_paths.contains(path) {
                return false;
            }
            if opt_bytes.is_none() {
                return true;
            }
            let Ok(dp) = DurablePath::parse(path) else {
                return true;
            };
            match task_id_of_work_path(&dp) {
                Some(tid) => !protected_tasks.contains(&tid),
                None => true,
            }
        });
        Ok(files)
    }

    /// Filters out stale self-echoes of this satellite's own single-writer task counter, tasks,
    /// and sidecars from an HQ `OrgEvent` so a satellite creating task N+1 before pulling HQ's echo
    /// of task N never stalls its own org stream on SQLite's monotonic counter trigger (`TASK-52` P8-#1).
    pub fn filter_self_echoed_org_event_files(
        &self,
        files: ReplicatedFileBatch,
    ) -> Result<ReplicatedFileBatch, OfficeError> {
        if self.info.role != crate::model::OfficeRole::Satellite {
            return Ok(files);
        }
        let (codes, _) = self.store.read(ReadOfficeCodesOp).map_err(storage)?;
        let local_codes: std::collections::BTreeSet<String> = codes
            .into_iter()
            .filter(|c| c.office_id == self.identity.office_id)
            .map(|c| c.code)
            .collect();
        if local_codes.is_empty() {
            return Ok(files);
        }
        let durable_files = Files::open(&self.durable).map_err(OfficeError::storage)?;
        let mut skipped_tasks: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::new();
        let mut out = Vec::with_capacity(files.len());
        // Pass 1: identify stale echoed local tasks & counters.
        for (path, opt_bytes) in files {
            let Ok(dp) = DurablePath::parse(&path) else {
                out.push((path, opt_bytes));
                continue;
            };
            match crate::tasks::records::kind_of(&dp) {
                Some(crate::tasks::records::Kind::OfficeTaskCounter(ref code))
                    if local_codes.contains(code) =>
                {
                    if should_skip_echoed_counter(&durable_files, &dp, opt_bytes.as_deref()) {
                        continue;
                    }
                    out.push((path, opt_bytes));
                }
                Some(crate::tasks::records::Kind::Task(ref tid)) => {
                    if should_skip_echoed_task(self, &local_codes, tid, opt_bytes.as_deref()) {
                        skipped_tasks.insert(tid.as_str().to_owned());
                        continue;
                    }
                    out.push((path, opt_bytes));
                }
                _ => out.push((path, opt_bytes)),
            }
        }
        // Pass 2: drop already-present sidecars for skipped local tasks.
        out.retain(|(path, opt_bytes)| {
            let Ok(dp) = DurablePath::parse(path) else {
                return true;
            };
            let sidecar_tid = match crate::tasks::records::kind_of(&dp) {
                Some(
                    crate::tasks::records::Kind::Checklist(tid)
                    | crate::tasks::records::Kind::Comment(tid, _)
                    | crate::tasks::records::Kind::Watchers(tid)
                    | crate::tasks::records::Kind::Links(tid)
                    | crate::tasks::records::Kind::Event(tid, _),
                ) => tid,
                _ => return true,
            };
            if opt_bytes.is_some()
                && skipped_tasks.contains(sidecar_tid.as_str())
                && durable_files.read(&dp).ok().flatten().is_some()
            {
                return false;
            }
            true
        });
        Ok(out)
    }

    /// Advances the `"org-sync-message-relay"` journal consumer watermark after durable publication.
    pub fn advance_message_relay_consumer(&self, ordinal: u64) -> Result<(), OfficeError> {
        self.advance_replication_consumer("org-sync-message-relay", ordinal)
    }

    /// Collects the native durable `messaging/conversations/<conversation_id>/...` files for
    /// cross-office message relay so the recipient office commits the real native conversation and
    /// message into its `org-store` inbox (`TASK-52` H-13).
    pub fn collect_conversation_messaging_files(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<(String, Option<String>)>, OfficeError> {
        self.store.sync().map_err(storage)?;
        let files = Files::open(&self.durable).map_err(OfficeError::storage)?;
        let prefix = DurablePath::parse(&format!("messaging/conversations/{conversation_id}"))
            .map_err(storage)?;
        let mut out = Vec::new();
        for path in files.walk(&prefix).map_err(OfficeError::storage)? {
            if crate::messaging::records::kind_of(&path).is_some() {
                if let Some(bytes) = files.read(&path).map_err(OfficeError::storage)? {
                    out.push((path.as_str().to_owned(), Some(STANDARD.encode(bytes))));
                }
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }

    /// Collects all current authoritative `state/` files from HQ's durable tree for initial
    /// snapshot streaming to a joining satellite (`TASK-52`).
    pub fn collect_authoritative_state_snapshot(
        &self,
    ) -> Result<Vec<(String, Vec<u8>)>, OfficeError> {
        self.store.sync().map_err(storage)?;
        let files = Files::open(&self.durable).map_err(OfficeError::storage)?;
        let state_prefix = DurablePath::parse("state").map_err(storage)?;
        let mut entries = Vec::new();
        for path in files.walk(&state_prefix).map_err(OfficeError::storage)? {
            let s = path.as_str();
            if !s.starts_with("state/") {
                continue;
            }
            if let Some(bytes) = files.read(&path).map_err(OfficeError::storage)? {
                entries.push((s.to_owned(), bytes.to_vec()));
            }
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(entries)
    }
}

pub(crate) fn validate_satellite_open_binding(
    intent: &SatelliteJoinIntent,
    info: &OfficeInfo,
    instance: &str,
    roots: &DataRoots,
    custody: &Custody,
) -> Result<(Id, Id, String), OfficeError> {
    let here = &custody.identity;
    let bound = intent.info() == *info
        && intent.instance == instance
        && intent.roots() == *roots
        && here.instance_id.as_str() == instance
        && here.organization_id.as_str() == info.org_id
        && here.office_id.as_str() == info.office_id;
    if !bound {
        return Err(OfficeError::Invalid(
            "satellite office roots or identity do not match its join intent",
        ));
    }
    let owner = Id::parse(&intent.office_id)
        .map_err(|_| OfficeError::Invalid("invalid satellite office_id"))?;
    let creation = Id::parse(&intent.operation)
        .map_err(|_| OfficeError::Invalid("invalid satellite join operation"))?;
    Ok((owner, creation, intent.operation.clone()))
}

fn validate_satellite_provision_spec(
    spec: &SatelliteProvisionSpec,
    here: &org_store::OfficeIdentity,
) -> Result<(), OfficeError> {
    let ok = spec.new.role == OfficeRole::Satellite
        && org_proto::is_uuid_v7(&spec.new.office_id)
        && org_proto::is_uuid_v7(&spec.new.org_id)
        && org_proto::is_uuid_v7(&spec.hq_office_id)
        && org_proto::is_uuid_v7(&spec.operation_id)
        && !spec.hq_addresses.is_empty()
        && here.office_id.as_str() == spec.new.office_id
        && here.organization_id.as_str() == spec.new.org_id;
    if !ok {
        return Err(OfficeError::Invalid(
            "invalid satellite provisioning parameters",
        ));
    }
    Ok(())
}

fn provision_satellite_impl(
    folder: PathBuf,
    spec: SatelliteProvisionSpec,
    key: Key,
    roots: DataRoots,
    custody: Custody,
    options: Options,
    publish_office_file: bool,
) -> Result<Office, OfficeError> {
    check_roots(&roots)?;
    let here = &custody.identity;
    validate_satellite_provision_spec(&spec, here)?;
    let now_unix = (options.clock.now().0 / 1000) as i64;
    let serial_hex = validate_satellite_cert_and_key(
        &spec.new.org_id,
        &spec.new.office_id,
        &spec.ca_cert_der,
        &spec.office_cert_der,
        &spec.office_key_der,
        now_unix,
    )?;
    let folder = plain_folder(folder)?;
    check_named_by(&folder, &spec.new.office_id)?;
    check_not_a_link(&folder)?;
    private_dir(&folder).map_err(OfficeError::storage)?;
    tighten_dir(&folder).map_err(OfficeError::storage)?;
    let lock = lock_folder(&folder, LOCK_WAIT)?;
    refuse_older_build(&folder)?;

    let ca_fp = cert_fingerprint_sha256(&spec.ca_cert_der);
    let ca_fp_hex = org_pki::cert::cert_fingerprint_hex(&spec.ca_cert_der);
    let intent = SatelliteJoinIntent {
        version: INTENT_VERSION,
        instance: here.instance_id.to_string(),
        journal_incarnation: here.journal_incarnation.to_string(),
        writer_epoch: here.writer_epoch,
        machine_id: here.machine_id.to_string(),
        operation: spec.operation_id.clone(),
        office_id: spec.new.office_id.clone(),
        office_name: spec.new.office_name.trim().to_owned(),
        org_id: spec.new.org_id.clone(),
        org_name: spec.new.org_name.trim().to_owned(),
        hq_office_id: spec.hq_office_id.clone(),
        hq_addresses: spec.hq_addresses.clone(),
        epoch: spec.epoch.max(1),
        ca_fingerprint_hex: ca_fp_hex.clone(),
        created_at: crate::organization::records::time_text(options.clock.now()),
        durable_root: roots.durable.clone(),
        runtime_root: roots.runtime.clone(),
    };
    let info = intent.info();
    let office_bytes = office_file::encode(&info)?;
    let federation = SatelliteFederationConfig {
        version: 1,
        org_id: info.org_id.clone(),
        local_office_id: info.office_id.clone(),
        hq_office_id: spec.hq_office_id.clone(),
        hq_addresses: spec.hq_addresses.clone(),
        epoch: spec.epoch.max(1),
        ca_fingerprint_hex: ca_fp_hex,
        cert_serial_hex: serial_hex,
    };
    persist_satellite_tls(
        &roots.durable,
        &info.office_id,
        &key,
        &federation,
        &spec.ca_cert_der,
        &spec.office_cert_der,
        &spec.office_key_der,
    )?;
    write_intent(&folder, &intent, true)?;

    let anchor = journal_dir(&roots.durable, here).join("anchor.json");
    let mut cfg = options.config(&roots, &custody);
    if !exists(&anchor)? {
        cfg.opening = Opening::Create(AnchorFacts::default());
    }
    let store = open_store(&cfg).map_err(storage)?;
    let companions = open_companion_stores(super::CompanionOpenSpec {
        engine: options.engine,
        roots: &roots,
        info: &info,
        identity: &custody.identity,
        hq_office_id: &spec.hq_office_id,
        ca_digest: Digest::from_bytes(ca_fp),
        epoch: spec.epoch.max(1),
        initial_snapshot_required: true,
    })?;
    let creation =
        Id::parse(&intent.operation).map_err(|_| OfficeError::Invalid("invalid operation id"))?;
    let owner =
        Id::parse(&intent.office_id).map_err(|_| OfficeError::Invalid("invalid office id"))?;
    let clock_high_ms = AtomicU64::new(options.clock.now().0);
    let office = Office {
        folder,
        info,
        durable: roots.durable,
        runtime_root: roots.runtime,
        identity: custody.identity,
        owner,
        creation,
        creation_command: intent.operation,
        secrets_key: Some(key),
        clock: options.clock,
        clock_high_ms,
        runtime: crate::messaging::runtime::OfficeRuntime::new(),
        private_notes: companions.private_notes,
        accounting: companions.accounting,
        budgets: companions.budgets,
        budget_op_lock: std::sync::Mutex::new(()),
        federation_io_lock: std::sync::Mutex::new(()),
        revocation_lock: std::sync::Mutex::new(()),
        snapshot_export_lock: std::sync::Mutex::new(()),
        promoted_removed_offices: std::sync::Mutex::new(std::collections::BTreeSet::new()),
        peer_activity_fences: std::sync::Mutex::new(std::collections::BTreeMap::new()),
        peer_session_stops: std::sync::Mutex::new(std::collections::BTreeMap::new()),
        providers: companions.providers,
        replication: companions.replication,
        store,
        _lock: lock,
        #[cfg(any(test, feature = "testkit"))]
        _test_root: None,
    };
    if publish_office_file {
        org_store::publish_one(&office.folder, office_file::FILE_NAME, &office_bytes, true)
            .map_err(OfficeError::storage)?;
    }
    // Eagerly register satellite journal consumers BEFORE serving any requests directly from
    // `provision_satellite` so early writes cannot be dropped on segment roll (`TASK-52` P6-#6).
    let _ = office.register_replication_consumer("org-sync-work-outbox")?;
    let _ = office.register_replication_consumer("org-sync-message-relay")?;
    Ok(office)
}

fn re_enroll_satellite_impl(
    office: &Office,
    spec: SatelliteReEnrollSpec,
) -> Result<(), OfficeError> {
    if office.info.role != OfficeRole::Satellite {
        return Err(OfficeError::Invalid(
            "only satellite offices may re-enroll onto a replacement HQ",
        ));
    }
    if spec.org_id != office.info.org_id
        || !org_proto::is_uuid_v7(&spec.new_hq_office_id)
        || spec.new_hq_addresses.is_empty()
    {
        return Err(OfficeError::Invalid(
            "invalid satellite re-enrollment parameters",
        ));
    }
    let secrets_key = office.secrets_key.as_ref().ok_or(OfficeError::Invalid(
        "office secrets key required to re-enroll",
    ))?;
    let now_unix = (office.monotonic_now_ms() / 1000) as i64;
    let serial_hex = validate_satellite_cert_and_key(
        &office.info.org_id,
        &office.info.office_id,
        &spec.new_ca_cert_der,
        &spec.new_office_cert_der,
        &spec.new_office_key_der,
        now_unix,
    )?;
    let ca_fp = cert_fingerprint_sha256(&spec.new_ca_cert_der);
    let ca_fp_hex = org_pki::cert::cert_fingerprint_hex(&spec.new_ca_cert_der);
    let new_epoch = spec.new_epoch.max(1);
    let federation = SatelliteFederationConfig {
        version: 1,
        org_id: office.info.org_id.clone(),
        local_office_id: office.info.office_id.clone(),
        hq_office_id: spec.new_hq_office_id.clone(),
        hq_addresses: spec.new_hq_addresses.clone(),
        epoch: new_epoch,
        ca_fingerprint_hex: ca_fp_hex.clone(),
        cert_serial_hex: serial_hex,
    };
    let new_hq = OfficeId::new(&spec.new_hq_office_id)
        .map_err(|_| OfficeError::Invalid("new_hq_office_id"))?;
    let ep = Epoch::new(new_epoch).map_err(|_| OfficeError::Invalid("new_epoch"))?;
    // Durably persist `initial_snapshot_required = true` BEFORE switching trust so a crash mid-
    // re-enrollment can never restart against a partial snapshot without gating admission (`TASK-52` P8-#4).
    office
        .replication
        .reset_for_re_enrollment(new_hq, Digest::from_bytes(ca_fp), ep)?;
    persist_satellite_tls(
        &office.durable,
        &office.info.office_id,
        secrets_key,
        &federation,
        &spec.new_ca_cert_der,
        &spec.new_office_cert_der,
        &spec.new_office_key_der,
    )?;
    if let Some(mut intent) = read_intent(&office.folder)? {
        intent.hq_office_id = spec.new_hq_office_id.clone();
        intent.hq_addresses = spec.new_hq_addresses;
        intent.epoch = new_epoch;
        intent.ca_fingerprint_hex = ca_fp_hex;
        write_intent(&office.folder, &intent, true)?;
    }
    Ok(())
}

fn sync_org_journal_locked_up_to(office: &Office, max_ordinal: u64) -> Result<usize, OfficeError> {
    let wm = office.register_replication_consumer("org-sync-hq")?;
    let entries = office.store.entries_after_contiguous(wm).map_err(storage)?;
    if entries.is_empty() {
        return Ok(0);
    }
    let incarnation = office.identity.journal_incarnation.as_str().to_owned();
    let codec = org_sync_proto::Codec::new("org.native.changes", 1)
        .map_err(|_| OfficeError::Invalid("codec"))?;
    let mut published = 0usize;
    let mut last_ordinal = wm;
    for entry in entries {
        let ord = entry.ordinal();
        if ord > max_ordinal {
            break;
        }
        let files: Vec<(String, Option<String>)> = entry
            .versions()
            .filter(|fv| is_hq_org_event_version(fv.path, fv.bytes))
            .map(|fv| {
                (
                    fv.path.as_str().to_owned(),
                    fv.bytes.map(|b| STANDARD.encode(b)),
                )
            })
            .collect();
        if !files.is_empty() {
            let payloads = chunk_org_delta_payloads(files)?;
            published += office.replication.publish_hq_journal_events(
                codec.clone(),
                payloads,
                &incarnation,
                ord,
            )?;
        }
        last_ordinal = last_ordinal.max(ord);
    }
    if last_ordinal > wm {
        office.advance_replication_consumer("org-sync-hq", last_ordinal)?;
    }
    Ok(published)
}

/// Checkpoint-consistent snapshot of HQ's authoritative `state/` files (`TASK-52` P2-8).
#[derive(Clone, Debug)]
pub struct ConsistentStateSnapshot {
    pub checkpoint: org_sync::organization::OrgCheckpoint,
    pub files: Vec<(String, Vec<u8>)>,
}

/// Typed `state/` file delta payload carried inside HQ-published `org-sync` `OrgEvent`s
/// (`TASK-52` F2) and satellite-originated `WorkChange` payloads (`TASK-52` F3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrgDeltaPayload {
    pub v: u32,
    /// `(durable_path_under_state, Some(base64_bytes) for put / None for delete)`.
    pub files: Vec<(String, Option<String>)>,
}

/// Decoded batch of `(state_path, Some(bytes) for put / None for delete)`.
pub type ReplicatedFileBatch = Vec<(String, Option<Vec<u8>>)>;

impl OrgDeltaPayload {
    pub fn decode_files(&self) -> Result<ReplicatedFileBatch, OfficeError> {
        if self.v != 1 {
            return Err(OfficeError::Invalid(
                "unsupported org delta payload version",
            ));
        }
        let mut out = Vec::with_capacity(self.files.len());
        for (path, b64) in &self.files {
            if (!path.starts_with("state/") && !path.starts_with("messaging/"))
                || path.contains("..")
            {
                return Err(OfficeError::Invalid("invalid org delta file path"));
            }
            let bytes = match b64 {
                Some(s) => Some(
                    STANDARD
                        .decode(s)
                        .map_err(|_| OfficeError::Invalid("invalid org delta file base64"))?,
                ),
                None => None,
            };
            out.push((path.clone(), bytes));
        }
        Ok(out)
    }
}

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

struct PlacementContext {
    hq_office_id: String,
    enrolled: std::collections::BTreeSet<String>,
    removed_offices: std::collections::BTreeSet<String>,
    employee_offices: std::collections::BTreeMap<String, String>,
}

struct ReadPlacementContextOp;

impl crate::store::open::StoreOp for ReadPlacementContextOp {
    type Out = Result<PlacementContext, org_store::StoreError>;

    fn run<E: crate::store::open::OfficeEngine>(self, store: &org_store::Store<E>) -> Self::Out {
        let (ctx, _) = store.read(|snap| {
            let offices = crate::organization::ports::OrganizationRead::offices(snap)
                .map_err(org_store::StoreError::Engine)?;
            let hq_office_id = offices
                .iter()
                .find(|o| o.role == crate::organization::records::OfficeRole::Hq)
                .map(|o| o.id.as_str().to_owned())
                .unwrap_or_default();
            let enrolled: std::collections::BTreeSet<String> =
                offices.iter().map(|o| o.id.as_str().to_owned()).collect();
            let hires = crate::organization::ports::OrganizationRead::hire_requests(snap)
                .map_err(org_store::StoreError::Engine)?;
            let mut employee_offices = std::collections::BTreeMap::new();
            for req in hires {
                if req.state == crate::organization::records::HireState::Hired {
                    if let (Some(emp_id), Some(oid)) = (req.employee_id, req.proposal.office_id) {
                        if enrolled.contains(&oid) {
                            employee_offices.insert(emp_id.as_str().to_owned(), oid);
                        }
                    }
                }
            }
            Ok(PlacementContext {
                hq_office_id,
                enrolled,
                removed_offices: std::collections::BTreeSet::new(),
                employee_offices,
            })
        })?;
        Ok(ctx)
    }
}

fn load_placement_context_locked(office: &Office) -> Result<PlacementContext, OfficeError> {
    let mut ctx = office.store.run(ReadPlacementContextOp).map_err(storage)?;
    if ctx.hq_office_id.is_empty() {
        ctx.hq_office_id = office
            .replication
            .org_checkpoint()
            .map(|cp| cp.hq_office.as_str().to_owned())
            .unwrap_or_else(|_| office.info.office_id.clone());
    }
    let removed = super::invitations::removed_satellite_offices_locked(office)?;
    for oid in &removed {
        ctx.enrolled.remove(oid);
    }
    ctx.removed_offices = removed;
    Ok(ctx)
}

fn load_placement_context(office: &Office) -> Result<PlacementContext, OfficeError> {
    let _guard = office
        .federation_io_lock
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    load_placement_context_locked(office)
}

/// Resolves the enrolled office where `employee_id` is placed (explicit hired agent placement,
/// falling back to HQ for persons and unplaced agents, `TASK-52` P3-#1 / P3-#11).
pub fn resolve_employee_office(office: &Office, employee_id: &str) -> Result<String, OfficeError> {
    let ctx = load_placement_context(office)?;
    Ok(ctx
        .employee_offices
        .get(employee_id)
        .cloned()
        .unwrap_or(ctx.hq_office_id))
}

/// Returns whether `office` has local write authority for a task numbered with `task_office_code`
/// (`TASK-52` P3-#9 / P4-#6 / P5-#7): satellites may only modify their own assigned codes, while HQ
/// may modify its own codes AND retired/orphaned codes (such as R1/D25 recovered tasks from a lost
/// HQ), refusing only tasks owned by an active satellite's current code.
pub(crate) fn has_local_task_write_authority(
    office: &Office,
    task_office_code: &str,
) -> Result<bool, OfficeError> {
    let (codes, _) = office.store.read(ReadOfficeCodesOp).map_err(storage)?;
    if office.info.role == OfficeRole::Satellite {
        return Ok(codes
            .iter()
            .any(|c| c.office_id.as_str() == office.info.office_id && c.code == task_office_code));
    }
    let enrolled = office.enrolled_office_ids().unwrap_or_default();
    let removed = super::invitations::removed_satellite_offices(office)?;
    let owned_by_active_satellite = codes.iter().any(|c| {
        c.code == task_office_code
            && c.retired_at.is_none()
            && c.office_id.as_str() != office.info.office_id
            && enrolled.iter().any(|id| id == c.office_id.as_str())
            && !removed.contains(c.office_id.as_str())
    });
    Ok(!owned_by_active_satellite)
}

struct ReadConversationOp {
    conversation_id: Id,
}

impl crate::store::open::StoreOp for ReadConversationOp {
    type Out = Result<Option<crate::messaging::records::ConversationRecord>, org_store::StoreError>;

    fn run<E: crate::store::open::OfficeEngine>(self, store: &org_store::Store<E>) -> Self::Out {
        let (conv, _) = store.read(|snap| {
            crate::messaging::ports::MessagingRead::conversation(snap, &self.conversation_id)
                .map_err(org_store::StoreError::Engine)
        })?;
        Ok(conv)
    }
}

struct ReadConversationMembersOp {
    conv: crate::messaging::records::ConversationRecord,
    owner: Id,
}

impl crate::store::open::StoreOp for ReadConversationMembersOp {
    type Out = Result<Vec<Id>, org_store::StoreError>;

    fn run<E: crate::store::open::OfficeEngine>(self, store: &org_store::Store<E>) -> Self::Out {
        let (members, _) = store.read(|snap| {
            crate::messaging::features::effective_auto_members(snap, &self.conv, &self.owner)
                .map_err(org_store::StoreError::Engine)
        })?;
        Ok(members)
    }
}

fn message_expires_at_ms(msg: &crate::messaging::records::MessageRecord) -> Option<u64> {
    if let Some(exp) = msg.expires_at.as_deref() {
        return crate::parse_rfc3339_ms(exp);
    }
    msg.ttl_ms.and_then(|ttl| {
        crate::parse_rfc3339_ms(&msg.created_at).map(|created| created.saturating_add(ttl))
    })
}

fn route_message_destinations(
    office: &Office,
    ctx: &PlacementContext,
    conv: &crate::messaging::records::ConversationRecord,
    msg: &crate::messaging::records::MessageRecord,
) -> Result<Vec<String>, OfficeError> {
    let members = office
        .store
        .run(ReadConversationMembersOp {
            conv: conv.clone(),
            owner: office.owner.clone(),
        })
        .map_err(storage)?;
    let local_office_id = office.info.office_id.as_str();
    let sender = msg.sender_employee_id.as_ref();
    let targets: std::collections::BTreeSet<String> = members
        .into_iter()
        .filter(|emp| Some(emp) != sender)
        .map(|emp| {
            ctx.employee_offices
                .get(emp.as_str())
                .cloned()
                .unwrap_or_else(|| ctx.hq_office_id.clone())
        })
        .filter(|dest| {
            dest != local_office_id && (ctx.enrolled.contains(dest) || dest == &ctx.hq_office_id)
        })
        .collect();
    Ok(targets.into_iter().collect())
}

/// Extracts bounded message relay entries from one journal entry, reading the conversation from
/// the same committed journal entry or committed `Store` projection (never an unflushed file),
/// including any structured ask answer committed in the same transaction (`TASK-52` P3-#4 / P4-#4/#9).
fn append_message_relay_entries(
    office: &Office,
    ctx: &PlacementContext,
    ordinal: u64,
    entry: &org_store::Entry,
    out: &mut Vec<PendingMessageJournalEntry>,
) -> Result<bool, OfficeError> {
    let mut entry_convs: std::collections::BTreeMap<Id, Vec<u8>> =
        std::collections::BTreeMap::new();
    let mut entry_answers: std::collections::BTreeMap<(Id, String), (String, Vec<u8>)> =
        std::collections::BTreeMap::new();
    for fv in entry.versions() {
        let Some(bytes) = fv.bytes else {
            continue;
        };
        match crate::messaging::records::kind_of(fv.path) {
            Some(crate::messaging::records::Kind::Conversation(cid)) => {
                entry_convs.insert(cid, bytes.to_vec());
            }
            Some(crate::messaging::records::Kind::ParticipantAnswer(cid, _, ask_id)) => {
                entry_answers.insert((cid, ask_id), (fv.path.as_str().to_owned(), bytes.to_vec()));
            }
            _ => {}
        }
    }
    // Publish origin-authoritative ask closes and recoveries FIRST so any dependent reply message
    // in the same journal entry never fails foreign-key checks on a newly added/rejoined office
    // before the recovered ask is committed (`TASK-52` P14-#3 / P15-#2).
    if !append_origin_close_answer_relays(office, ctx, ordinal, entry, false, out)? {
        return Ok(false);
    }
    let mut relayed_conv_ids: std::collections::BTreeSet<Id> = std::collections::BTreeSet::new();
    for fv in entry.versions() {
        let Some(crate::messaging::records::Kind::Message(cid, mid)) =
            crate::messaging::records::kind_of(fv.path)
        else {
            continue;
        };
        let Some(msg_bytes) = fv.bytes else {
            continue;
        };
        relayed_conv_ids.insert(cid.clone());
        let conv_bytes = if let Some(bytes) = entry_convs.get(&cid) {
            bytes.clone()
        } else if let Some(conv) = office
            .store
            .run(ReadConversationOp {
                conversation_id: cid.clone(),
            })
            .map_err(storage)?
        {
            crate::messaging::records::encode_json(&conv)
        } else {
            return Ok(false);
        };
        let conv: crate::messaging::records::ConversationRecord =
            serde_json::from_slice(&conv_bytes)
                .map_err(|_| OfficeError::Invalid("malformed conversation in journal"))?;
        let msg: crate::messaging::records::MessageRecord = serde_json::from_slice(msg_bytes)
            .map_err(|_| OfficeError::Invalid("malformed message in journal"))?;
        let destination_offices = route_message_destinations(office, ctx, &conv, &msg)?;
        let conv_path = format!("messaging/conversations/{cid}/conversation.json");
        let conv_entry = (conv_path, Some(STANDARD.encode(&conv_bytes)));
        let msg_entry = (
            fv.path.as_str().to_owned(),
            Some(STANDARD.encode(msg_bytes)),
        );
        let ans_entry = msg.reply_to_message_id.as_ref().and_then(|reply_to| {
            entry_answers
                .get(&(cid.clone(), reply_to.clone()))
                .map(|(ans_path, ans_bytes)| (ans_path.clone(), Some(STANDARD.encode(ans_bytes))))
        });
        let mut combined = vec![conv_entry.clone(), msg_entry.clone()];
        if let Some(ref ans) = ans_entry {
            combined.push(ans.clone());
        }
        let encoded_len = serde_json::to_vec(&OrgDeltaPayload {
            v: 1,
            files: combined.clone(),
        })
        .map_or(usize::MAX, |b| b.len());
        if encoded_len > org_sync::messages::MAX_PAYLOAD_BYTES {
            // Deliver `conversation.json` separately FIRST when bundling with `message.json`
            // exceeds `MAX_PAYLOAD_BYTES` (e.g. a 1,000-member channel) so a satellite seeing
            // the channel for the first time still receives the conversation (`TASK-52` P6-#5).
            out.push(PendingMessageJournalEntry {
                ordinal,
                conversation_id: cid.as_str().to_owned(),
                message_id: format!("conv-{cid}-r{}", conv.revision),
                files: vec![conv_entry],
                destination_offices: destination_offices.clone(),
                expires_at_ms: message_expires_at_ms(&msg),
            });
            let mut msg_only = vec![msg_entry];
            if let Some(ans) = ans_entry {
                msg_only.push(ans);
            }
            out.push(PendingMessageJournalEntry {
                ordinal,
                conversation_id: cid.as_str().to_owned(),
                message_id: mid,
                files: msg_only,
                destination_offices,
                expires_at_ms: message_expires_at_ms(&msg),
            });
        } else {
            out.push(PendingMessageJournalEntry {
                ordinal,
                conversation_id: cid.as_str().to_owned(),
                message_id: mid,
                files: combined,
                destination_offices,
                expires_at_ms: message_expires_at_ms(&msg),
            });
        }
    }
    // Replicate HQ-authored channel creations and membership revisions immediately even before
    // any message is sent in the channel (`TASK-52` P9-#8).
    if office.info.role == crate::model::OfficeRole::Hq {
        append_hq_channel_only_relays(office, ctx, ordinal, &entry_convs, &relayed_conv_ids, out)?;
    }
    Ok(true)
}

fn task_id_of_work_path(dp: &DurablePath) -> Option<Id> {
    match crate::tasks::records::kind_of(dp) {
        Some(
            crate::tasks::records::Kind::Task(tid)
            | crate::tasks::records::Kind::Checklist(tid)
            | crate::tasks::records::Kind::Comment(tid, _)
            | crate::tasks::records::Kind::Watchers(tid)
            | crate::tasks::records::Kind::Links(tid)
            | crate::tasks::records::Kind::Event(tid, _),
        ) => Some(tid),
        _ => None,
    }
}

fn collect_task_ids_from_paths(
    paths: &std::collections::BTreeSet<String>,
) -> std::collections::BTreeSet<Id> {
    paths
        .iter()
        .filter_map(|p| DurablePath::parse(p).ok())
        .filter_map(|dp| task_id_of_work_path(&dp))
        .collect()
}

/// Returns the owning task ID string if `path` is a task record or task sidecar under `state/work/`.
pub fn task_id_for_work_path(path: &str) -> Option<String> {
    DurablePath::parse(path)
        .ok()
        .and_then(|dp| task_id_of_work_path(&dp))
        .map(|id| id.as_str().to_owned())
}

/// Returns `Ok(Some(task_id))` when `office` is a satellite that already holds a strictly newer
/// local revision for this incoming snapshot `task.json`, failing closed on storage or non-`NotFound`
/// lookup errors so newer sidecars can never be overwritten (`TASK-52` P12-#1 / P21-#2 / P27-#1).
pub fn is_stale_snapshot_task_entry(
    office: &Office,
    path: &str,
    bytes: &[u8],
) -> Result<Option<String>, OfficeError> {
    if office.info.role != crate::model::OfficeRole::Satellite {
        return Ok(None);
    }
    let dp = DurablePath::parse(path).map_err(storage)?;
    let Some(crate::tasks::records::Kind::Task(tid)) = crate::tasks::records::kind_of(&dp) else {
        return Ok(None);
    };
    let incoming: crate::tasks::records::TaskRecord = serde_json::from_slice(bytes)
        .map_err(|_| OfficeError::Invalid("malformed TaskRecord in snapshot"))?;
    match office.task_inspect(tid.as_str())? {
        Ok(local) => {
            if local.task.office_code == incoming.office_code
                && local.task.revision > incoming.meta.revision
            {
                Ok(Some(tid.as_str().to_owned()))
            } else {
                Ok(None)
            }
        }
        Err(api_err) if api_err.code == org_proto::ErrorCode::NotFound => Ok(None),
        Err(_) => Err(OfficeError::Invalid(
            "failed inspecting local task during snapshot indexing",
        )),
    }
}

/// Progress and disk-budget events emitted by [`export_authoritative_state_snapshot_pages`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotExportProgress {
    /// Periodic cooperative cancellation / keepalive check while scanning files.
    Heartbeat,
    /// Requests reservation of `bytes` in the caller's staging budget before writing a page.
    ReservePageBytes(u64),
}

/// Metadata returned by [`export_authoritative_state_snapshot_pages`] after streaming a consistent
/// HQ snapshot into bounded JSON page files on disk.
pub struct ExportedSnapshotPages {
    pub checkpoint: org_sync::organization::OrgCheckpoint,
    pub total_pages: u32,
    pub total_bytes: u64,
    pub manifest_sha256_hex: String,
}

fn write_ephemeral_private_file(path: &Path, buf: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        opts.mode(0o600);
    }
    let mut file = opts.open(path)?;
    file.write_all(buf)
}

fn flush_exported_snapshot_page(
    staging_dir: &Path,
    page_idx: u32,
    max_pages: u32,
    buf: &[u8],
    total_bytes: &mut u64,
    manifest_inputs: &mut Vec<u8>,
    on_progress: &mut impl FnMut(SnapshotExportProgress) -> bool,
) -> Result<(), OfficeError> {
    if page_idx >= max_pages {
        return Err(OfficeError::Invalid("snapshot exceeds maximum page count"));
    }
    let len = buf.len() as u64;
    if !on_progress(SnapshotExportProgress::ReservePageBytes(len)) {
        return Err(OfficeError::Invalid("snapshot staging budget exceeded"));
    }
    *total_bytes = total_bytes.saturating_add(len);
    manifest_inputs.extend_from_slice(org_pki::cert::cert_fingerprint_hex(buf).as_bytes());
    let path = staging_dir.join(format!("page-{page_idx:06}.json"));
    // Ephemeral `runtime/` staging files are rebuilt on retry and removed on startup/drop, so
    // private `0o600` unfsynced writes avoid per-page disk-sync stalls (`TASK-52` P22-#2).
    write_ephemeral_private_file(&path, buf).map_err(OfficeError::storage)
}

fn check_export_heartbeat(
    seen: &mut usize,
    on_progress: &mut impl FnMut(SnapshotExportProgress) -> bool,
) -> Result<(), OfficeError> {
    *seen = seen.saturating_add(1);
    if (*seen).is_multiple_of(32) && !on_progress(SnapshotExportProgress::Heartbeat) {
        return Err(OfficeError::Invalid("snapshot export cancelled"));
    }
    Ok(())
}

fn probe_real_dir_chain(root: &Path, components: &[&str]) -> Result<Option<PathBuf>, OfficeError> {
    // Validate `root` (`durable/`) itself first via `symlink_metadata` so a missing or
    // symlinked durable root fails closed instead of treating missing children as empty
    // (`TASK-52` P30-#1).
    let root_meta = std::fs::symlink_metadata(root).map_err(OfficeError::storage)?;
    if !root_meta.is_dir() {
        return Err(OfficeError::Invalid("durable root is not a real directory"));
    }
    let mut cur = root.to_path_buf();
    for comp in components {
        cur.push(comp);
        match std::fs::symlink_metadata(&cur) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => {
                return Err(OfficeError::Invalid(
                    "durable snapshot directory component is not a real directory",
                ));
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(OfficeError::storage(err)),
        }
    }
    Ok(Some(cur))
}

fn walk_state_paths_cancellable(
    durable: &Path,
    on_progress: &mut impl FnMut(SnapshotExportProgress) -> bool,
) -> Result<Vec<DurablePath>, OfficeError> {
    // Use fallible component-wise `symlink_metadata` so I/O, permission, or symlink errors fail
    // closed rather than publishing an empty snapshot (`TASK-52` P26-#2 / P29-#1).
    let Some(state_root) = probe_real_dir_chain(durable, &["state"])? else {
        return Ok(Vec::new());
    };
    let state_prefix = DurablePath::parse("state").map_err(storage)?;
    let mut paths = Vec::new();
    let mut stack = vec![(state_root, state_prefix)];
    let mut seen = 0usize;
    while let Some((dir, prefix)) = stack.pop() {
        check_export_heartbeat(&mut seen, on_progress)?;
        let entries = std::fs::read_dir(&dir).map_err(OfficeError::storage)?;
        for entry in entries {
            check_export_heartbeat(&mut seen, on_progress)?;
            let entry = entry.map_err(OfficeError::storage)?;
            let ft = entry.file_type().map_err(OfficeError::storage)?;
            let name = entry.file_name();
            let name_str = name
                .to_str()
                .ok_or(OfficeError::Invalid("non-UTF-8 entry under state/"))?;
            if ft.is_file() && name_str.starts_with(".store-tmp-") {
                continue;
            }
            // Build `DurablePath` from validated filesystem components (matching `Files::walk`)
            // without rewriting separators, and enforce `DurablePath::MAX_BYTES` so HQ never
            // exports a path that `DurablePath::parse` rejects (`TASK-52` P27-#2 / P28-#2).
            let dp = prefix.join(name_str).map_err(storage)?;
            let dp = DurablePath::parse(dp.as_str()).map_err(storage)?;
            let full = entry.path();
            if ft.is_dir() {
                stack.push((full, dp));
            } else if ft.is_file() {
                paths.push(dp);
            } else {
                return Err(OfficeError::Invalid(
                    "unexpected non-file entry under state/",
                ));
            }
        }
    }
    Ok(paths)
}

fn append_channel_snapshot_paths_cancellable(
    durable: &Path,
    files: &Files,
    paths: &mut Vec<DurablePath>,
    on_progress: &mut impl FnMut(SnapshotExportProgress) -> bool,
) -> Result<(), OfficeError> {
    // Validate BOTH `durable/messaging` and `durable/messaging/conversations` as real
    // directories via `symlink_metadata` before accepting absence (`TASK-52` P28-#1 / P29-#1).
    let Some(convs_root) = probe_real_dir_chain(durable, &["messaging", "conversations"])? else {
        return Ok(());
    };
    let entries = std::fs::read_dir(&convs_root).map_err(OfficeError::storage)?;
    let convs_prefix = DurablePath::parse("messaging/conversations").map_err(storage)?;
    let mut seen = 0usize;
    for entry in entries {
        // Check cancellation on every conversation directory without descending into
        // `messages/` or `answers/` history, rejecting symlinks/unexpected entries and
        // propagating every I/O or decode error (`TASK-52` P24-#2 / P25-#1 / P28-#1 / P28-#2).
        check_export_heartbeat(&mut seen, on_progress)?;
        let entry = entry.map_err(OfficeError::storage)?;
        let ft = entry.file_type().map_err(OfficeError::storage)?;
        let cid = entry.file_name();
        let cid_str = cid
            .to_str()
            .ok_or(OfficeError::Invalid("non-UTF-8 conversation directory"))?;
        if ft.is_file() && cid_str.starts_with(".store-tmp-") {
            continue;
        }
        if !ft.is_dir() {
            return Err(OfficeError::Invalid(
                "unexpected non-directory entry under messaging/conversations",
            ));
        }
        let dp = convs_prefix
            .join(cid_str)
            .and_then(|d| d.join("conversation.json"))
            .map_err(storage)?;
        let dp = DurablePath::parse(dp.as_str()).map_err(storage)?;
        if !on_progress(SnapshotExportProgress::Heartbeat) {
            return Err(OfficeError::Invalid("snapshot export cancelled"));
        }
        let Some(bytes) = files.read(&dp).map_err(OfficeError::storage)? else {
            continue;
        };
        let conv: crate::messaging::records::ConversationRecord = serde_json::from_slice(&bytes)
            .map_err(|_| OfficeError::Invalid("malformed conversation.json in snapshot export"))?;
        if conv.kind == org_proto::chat::ConversationKind::Channel {
            paths.push(dp);
        }
    }
    Ok(())
}

fn collect_authoritative_snapshot_paths(
    durable: &Path,
    files: &Files,
    on_progress: &mut impl FnMut(SnapshotExportProgress) -> bool,
) -> Result<Vec<DurablePath>, OfficeError> {
    let mut paths = walk_state_paths_cancellable(durable, on_progress)?;
    append_channel_snapshot_paths_cancellable(durable, files, &mut paths, on_progress)?;
    if !on_progress(SnapshotExportProgress::Heartbeat) {
        return Err(OfficeError::Invalid("snapshot export cancelled"));
    }
    paths.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    Ok(paths)
}

fn acquire_snapshot_export_lock_cancellable<'a>(
    office: &'a Office,
    on_progress: &mut impl FnMut(SnapshotExportProgress) -> bool,
) -> Result<std::sync::MutexGuard<'a, ()>, OfficeError> {
    loop {
        if !on_progress(SnapshotExportProgress::Heartbeat) {
            return Err(OfficeError::Invalid("snapshot export cancelled"));
        }
        match office.snapshot_export_lock.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(std::sync::TryLockError::Poisoned(p)) => return Ok(p.into_inner()),
            Err(std::sync::TryLockError::WouldBlock) => {
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
        }
    }
}

fn stream_snapshot_paths_to_pages(
    durable: &Path,
    staging_dir: &Path,
    max_page_bytes: usize,
    max_pages: u32,
    on_progress: &mut impl FnMut(SnapshotExportProgress) -> bool,
) -> Result<(u32, u64, String), OfficeError> {
    let files = Files::open(durable).map_err(OfficeError::storage)?;
    let paths = collect_authoritative_snapshot_paths(durable, &files, on_progress)?;
    let mut page_idx = 0u32;
    let mut total_bytes = 0u64;
    let mut manifest_inputs = Vec::new();
    let mut cur = vec![b'['];
    for (file_idx, path) in paths.into_iter().enumerate() {
        if file_idx % 64 == 0 && !on_progress(SnapshotExportProgress::Heartbeat) {
            return Err(OfficeError::Invalid("snapshot export cancelled"));
        }
        let Some(bytes) = files.read(&path).map_err(OfficeError::storage)? else {
            return Err(OfficeError::Invalid("snapshot file vanished during export"));
        };
        let entry = serde_json::to_vec(&(path.as_str(), STANDARD.encode(bytes)))
            .map_err(OfficeError::storage)?;
        if entry.len().saturating_add(2) > max_page_bytes {
            return Err(OfficeError::Invalid(
                "snapshot file exceeds shared page limit",
            ));
        }
        let sep = usize::from(cur.len() > 1);
        if cur.len().saturating_add(sep + entry.len() + 1) > max_page_bytes {
            cur.push(b']');
            flush_exported_snapshot_page(
                staging_dir,
                page_idx,
                max_pages,
                &cur,
                &mut total_bytes,
                &mut manifest_inputs,
                on_progress,
            )?;
            page_idx = page_idx.saturating_add(1);
            cur = vec![b'['];
        }
        if cur.len() > 1 {
            cur.push(b',');
        }
        cur.extend_from_slice(&entry);
    }
    cur.push(b']');
    flush_exported_snapshot_page(
        staging_dir,
        page_idx,
        max_pages,
        &cur,
        &mut total_bytes,
        &mut manifest_inputs,
        on_progress,
    )?;
    let total_pages = page_idx.saturating_add(1);
    let manifest_sha256_hex = org_pki::cert::cert_fingerprint_hex(&manifest_inputs);
    Ok((total_pages, total_bytes, manifest_sha256_hex))
}

/// Captures HQ's checkpoint and `state/` + channel files at ONE consistent `org-store`
/// materialization boundary, streaming files directly into bounded JSON page files
/// (`page-000000.json`, ...) under `staging_dir` so peak HQ RAM stays bounded to one page and
/// total staged disk bytes obey `on_progress` (`TASK-52` P21-#2 / P22-#2 / P24-#1 / P24-#2).
pub fn export_authoritative_state_snapshot_pages(
    office: &Office,
    staging_dir: &Path,
    max_page_bytes: usize,
    max_pages: u32,
    mut on_progress: impl FnMut(SnapshotExportProgress) -> bool,
) -> Result<ExportedSnapshotPages, OfficeError> {
    crate::private_fs::private_dir(staging_dir).map_err(OfficeError::storage)?;
    // Serialize concurrent exports BEFORE acquiring `federation_io_lock` or waiting on
    // `with_consistent_materialized`, polling cooperatively so queued exports cancel promptly on
    // shutdown without holding `federation_io_lock` (`TASK-52` P24-#1).
    let _export_guard = acquire_snapshot_export_lock_cancellable(office, &mut on_progress)?;
    let mut fed_guard = Some(
        office
            .federation_io_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner()),
    );
    let mut captured: Option<ExportedSnapshotPages> = None;
    let mut capture_err: Option<OfficeError> = None;
    office
        .store
        .with_consistent_materialized(|mat| {
            let res = (|| -> Result<ExportedSnapshotPages, OfficeError> {
                let _ = sync_org_journal_locked_up_to(office, mat)?;
                let checkpoint = office.replication.org_checkpoint()?;
                drop(fed_guard.take());
                let (total_pages, total_bytes, manifest_sha256_hex) =
                    stream_snapshot_paths_to_pages(
                        &office.durable,
                        staging_dir,
                        max_page_bytes,
                        max_pages,
                        &mut on_progress,
                    )?;
                Ok(ExportedSnapshotPages {
                    checkpoint,
                    total_pages,
                    total_bytes,
                    manifest_sha256_hex,
                })
            })();
            match res {
                Ok(out) => captured = Some(out),
                Err(err) => capture_err = Some(err),
            }
            Ok(())
        })
        .map_err(storage)?;
    if let Some(err) = capture_err {
        return Err(err);
    }
    captured.ok_or(OfficeError::Invalid("snapshot export missing result"))
}

struct ReadMessageOp {
    ask_id: String,
}

impl crate::store::open::StoreOp for ReadMessageOp {
    type Out = Result<Option<crate::messaging::records::MessageRecord>, org_store::StoreError>;

    fn run<E: crate::store::open::OfficeEngine>(self, store: &org_store::Store<E>) -> Self::Out {
        let (msg, _) = store.read(|snap| {
            crate::messaging::ports::MessagingRead::message(snap, &self.ask_id)
                .map_err(org_store::StoreError::Engine)
        })?;
        Ok(msg)
    }
}

fn delta_fits_message_limit(files: &[(String, Option<String>)]) -> bool {
    serde_json::to_vec(&OrgDeltaPayload {
        v: 1,
        files: files.to_vec(),
    })
    .is_ok_and(|b| b.len() <= org_sync::messages::MAX_PAYLOAD_BYTES)
}

type EncodedRelayFiles = Vec<(String, Option<String>)>;

/// Builds bounded, ordered non-expiring `close-*` deliveries for an origin-authoritative ask
/// close: prefers one single bundle `[conv, ask, answer]`, falls back to `[ask, answer]`, and
/// splits oversized ask+answer combinations into ordered `close-conv-*` -> `close-ask-*` ->
/// `close-*` entries so a large valid ask can never wedge a newly added office (`TASK-52` P13-#2 / P14-#2).
fn build_origin_close_entries(
    cid: &Id,
    conv: &crate::messaging::records::ConversationRecord,
    ask_msg: &crate::messaging::records::MessageRecord,
    ans_path: &str,
    ans_bytes: &[u8],
    ask_id: &str,
) -> Vec<(String, EncodedRelayFiles)> {
    let ans_entry = (ans_path.to_owned(), Some(STANDARD.encode(ans_bytes)));
    let ask_entry = (
        format!("messaging/conversations/{cid}/messages/{}.json", ask_msg.id),
        Some(STANDARD.encode(crate::messaging::records::encode_json(ask_msg))),
    );
    let conv_entry = (
        format!("messaging/conversations/{cid}/conversation.json"),
        Some(STANDARD.encode(crate::messaging::records::encode_json(conv))),
    );
    let with_conv = vec![conv_entry.clone(), ask_entry.clone(), ans_entry.clone()];
    if delta_fits_message_limit(&with_conv) {
        return vec![(format!("close-{ask_id}"), with_conv)];
    }
    let with_ask = vec![ask_entry.clone(), ans_entry.clone()];
    if delta_fits_message_limit(&with_ask) {
        return vec![(format!("close-{ask_id}"), with_ask)];
    }
    let mut entries = Vec::new();
    let conv_and_ask = vec![conv_entry.clone(), ask_entry.clone()];
    if delta_fits_message_limit(&conv_and_ask) {
        entries.push((format!("close-ask-{ask_id}"), conv_and_ask));
    } else {
        entries.push((format!("close-conv-{ask_id}"), vec![conv_entry]));
        entries.push((format!("close-ask-{ask_id}"), vec![ask_entry]));
    }
    entries.push((format!("close-{ask_id}"), vec![ans_entry]));
    entries
}

struct OriginCloseCandidate<'a> {
    ordinal: u64,
    cid: &'a Id,
    ask_id: &'a str,
    ans_path: &'a str,
    bytes: &'a [u8],
    origin_only: bool,
}

/// Emits durable journal-backed `close-{ask_id}` relays for any origin-owned ask close
/// (`reason == "origin-close"`) or withdrawal (`TASK-52` P11-#3 / P11-#5 / P13-#2 / P13-#3).
fn append_origin_close_answer_relays(
    office: &Office,
    ctx: &PlacementContext,
    ordinal: u64,
    entry: &org_store::Entry,
    origin_only: bool,
    out: &mut Vec<PendingMessageJournalEntry>,
) -> Result<bool, OfficeError> {
    for fv in entry.versions() {
        let Some(crate::messaging::records::Kind::ParticipantAnswer(cid, _, ask_id)) =
            crate::messaging::records::kind_of(fv.path)
        else {
            continue;
        };
        let Some(bytes) = fv.bytes else {
            continue;
        };
        let cand = OriginCloseCandidate {
            ordinal,
            cid: &cid,
            ask_id: &ask_id,
            ans_path: fv.path.as_str(),
            bytes,
            origin_only,
        };
        if !append_one_origin_close_relay(office, ctx, &cand, out)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn append_handoff_accept_relay(
    office: &Office,
    ctx: &PlacementContext,
    cand: &OriginCloseCandidate<'_>,
    conv: &crate::messaging::records::ConversationRecord,
    ask_msg: &crate::messaging::records::MessageRecord,
    ans: &crate::messaging::records::ParticipantAnswerFile,
    out: &mut Vec<PendingMessageJournalEntry>,
) -> Result<bool, OfficeError> {
    if cand.origin_only || ask_msg.kind != "handoff" {
        return Ok(true);
    }
    let dests = route_origin_close_destinations(office, ctx, conv, office.info.office_id.as_str())?;
    if !dests.is_empty() {
        out.push(PendingMessageJournalEntry {
            ordinal: cand.ordinal,
            conversation_id: cand.cid.as_str().to_owned(),
            message_id: format!("accept-{}-{}", cand.ask_id, ans.employee_id),
            files: vec![(cand.ans_path.to_owned(), Some(STANDARD.encode(cand.bytes)))],
            destination_offices: dests,
            expires_at_ms: None,
        });
    }
    Ok(true)
}

fn push_origin_close_entries(
    office: &Office,
    ctx: &PlacementContext,
    cand: &OriginCloseCandidate<'_>,
    conv: &crate::messaging::records::ConversationRecord,
    ask_msg: &crate::messaging::records::MessageRecord,
    out: &mut Vec<PendingMessageJournalEntry>,
) -> Result<(), OfficeError> {
    let dests = route_origin_close_destinations(office, ctx, conv, office.info.office_id.as_str())?;
    if dests.is_empty() {
        return Ok(());
    }
    for (message_id, files) in build_origin_close_entries(
        cand.cid,
        conv,
        ask_msg,
        cand.ans_path,
        cand.bytes,
        cand.ask_id,
    ) {
        out.push(PendingMessageJournalEntry {
            ordinal: cand.ordinal,
            conversation_id: cand.cid.as_str().to_owned(),
            message_id,
            files,
            destination_offices: dests.clone(),
            expires_at_ms: None,
        });
    }
    Ok(())
}

fn append_one_origin_close_relay(
    office: &Office,
    ctx: &PlacementContext,
    cand: &OriginCloseCandidate<'_>,
    out: &mut Vec<PendingMessageJournalEntry>,
) -> Result<bool, OfficeError> {
    let Ok(ans) =
        serde_json::from_slice::<crate::messaging::records::ParticipantAnswerFile>(cand.bytes)
    else {
        return Ok(true);
    };
    let is_committed_authoritative = ans.reason.as_deref() == Some("origin-close");
    let is_withdrawal = crate::messaging::asks::decode_ask_answer_file(&ans)
        .1
        .is_some();
    let is_handoff_accept = ans.answer.is_none() && ans.accepted_by.is_some();
    if !is_committed_authoritative && !is_withdrawal && !is_handoff_accept {
        return Ok(true);
    }
    let Some(conv) = office
        .store
        .run(ReadConversationOp {
            conversation_id: cand.cid.clone(),
        })
        .map_err(storage)?
    else {
        return Ok(false);
    };
    let Some(ask_msg) = office
        .store
        .run(ReadMessageOp {
            ask_id: cand.ask_id.to_owned(),
        })
        .map_err(storage)?
    else {
        return Ok(false);
    };
    if is_handoff_accept {
        return append_handoff_accept_relay(office, ctx, cand, &conv, &ask_msg, &ans, out);
    }
    let is_ask_origin = ask_msg.origin_office_id.as_str() == office.info.office_id.as_str();
    let hq_finalizes_removed = office.info.role == crate::model::OfficeRole::Hq
        && is_committed_authoritative
        && ctx
            .removed_offices
            .contains(ask_msg.origin_office_id.as_str());
    if cand.origin_only && !is_ask_origin {
        return Ok(true);
    }
    if is_ask_origin || hq_finalizes_removed {
        push_origin_close_entries(office, ctx, cand, &conv, &ask_msg, out)?;
        return Ok(true);
    }
    let origin = ask_msg.origin_office_id.as_str();
    if ctx.enrolled.contains(origin) || origin == ctx.hq_office_id {
        out.push(PendingMessageJournalEntry {
            ordinal: cand.ordinal,
            conversation_id: cand.cid.as_str().to_owned(),
            message_id: format!("close-{}", cand.ask_id),
            files: vec![(cand.ans_path.to_owned(), Some(STANDARD.encode(cand.bytes)))],
            destination_offices: vec![origin.to_owned()],
            expires_at_ms: None,
        });
    }
    Ok(true)
}

fn route_origin_close_destinations(
    office: &Office,
    ctx: &PlacementContext,
    conv: &crate::messaging::records::ConversationRecord,
    local_id: &str,
) -> Result<Vec<String>, OfficeError> {
    let members = office
        .store
        .run(ReadConversationMembersOp {
            conv: conv.clone(),
            owner: office.owner.clone(),
        })
        .map_err(storage)?;
    let dests: std::collections::BTreeSet<String> = members
        .into_iter()
        .map(|emp| {
            ctx.employee_offices
                .get(emp.as_str())
                .cloned()
                .unwrap_or_else(|| ctx.hq_office_id.clone())
        })
        .filter(|oid| oid != local_id && (ctx.enrolled.contains(oid) || oid == &ctx.hq_office_id))
        .collect();
    Ok(dests.into_iter().collect())
}

fn append_hq_channel_only_relays(
    office: &Office,
    ctx: &PlacementContext,
    ordinal: u64,
    entry_convs: &std::collections::BTreeMap<Id, Vec<u8>>,
    relayed_conv_ids: &std::collections::BTreeSet<Id>,
    out: &mut Vec<PendingMessageJournalEntry>,
) -> Result<(), OfficeError> {
    let local_id = office.info.office_id.as_str();
    let remote_offices: Vec<String> = ctx
        .enrolled
        .iter()
        .filter(|oid| oid.as_str() != local_id)
        .cloned()
        .collect();
    if remote_offices.is_empty() {
        return Ok(());
    }
    for (cid, conv_bytes) in entry_convs {
        if relayed_conv_ids.contains(cid) {
            continue;
        }
        let Ok(conv) =
            serde_json::from_slice::<crate::messaging::records::ConversationRecord>(conv_bytes)
        else {
            continue;
        };
        if conv.kind != org_proto::chat::ConversationKind::Channel {
            continue;
        }
        let conv_path = format!("messaging/conversations/{cid}/conversation.json");
        out.push(PendingMessageJournalEntry {
            ordinal,
            conversation_id: cid.as_str().to_owned(),
            message_id: format!("conv-{cid}-r{}", conv.revision),
            files: vec![(conv_path, Some(STANDARD.encode(conv_bytes)))],
            destination_offices: remote_offices.clone(),
            expires_at_ms: None,
        });
    }
    Ok(())
}

struct ApplyRelayedMessageOp {
    operation_id: Id,
    here: org_store::OfficeIdentity,
    owner: Id,
    origin_office: String,
    now_ms: u64,
    ctx: PlacementContext,
    incoming_conv: Option<crate::messaging::records::ConversationRecord>,
    incoming_answer: Option<(Id, String, crate::messaging::records::ParticipantAnswerFile)>,
    msg: crate::messaging::records::MessageRecord,
}

impl crate::store::open::StoreOp for ApplyRelayedMessageOp {
    type Out = Result<org_store::Reply, org_store::StoreError>;

    fn run<E: crate::store::open::OfficeEngine>(self, store: &org_store::Store<E>) -> Self::Out {
        let op = org_store::Operation {
            id: self.operation_id.clone(),
            kind: "replication.apply_message",
            actor: Actor::System,
            fingerprint: org_store::Sha256::of_parts(&[
                self.msg.conversation_id.as_str().as_bytes(),
                b"\0",
                self.msg.id.as_bytes(),
                b"\0",
                self.msg.content_sha256.as_bytes(),
            ]),
        };
        store.submit(Some(op), false, move |uow| {
            decide_apply_relayed_message(uow, &self)
        })
    }
}

struct RelayedRecipientCtx<'a> {
    here: &'a org_store::OfficeIdentity,
    owner: &'a Id,
    placement: &'a PlacementContext,
    employees: &'a [crate::organization::records::Employee],
    conv: &'a crate::messaging::records::ConversationRecord,
    active_members: &'a [Id],
    msg: &'a crate::messaging::records::MessageRecord,
}

fn validate_relayed_sender(
    employees: &[crate::organization::records::Employee],
    placement: &PlacementContext,
    origin_office: &str,
    msg: &crate::messaging::records::MessageRecord,
) -> Result<(String, String, bool), org_store::Refusal> {
    use crate::organization::records::{EmployeeKind, EmployeeStatus};
    if msg.sender_kind == crate::messaging::records::SenderKind::Office {
        // Bind office-system sender identity strictly to the authenticated origin office (`TASK-52` P5-#5).
        if msg.sender_employee_id.is_some()
            || msg.sender_office_id.as_ref().map(Id::as_str) != Some(origin_office)
        {
            return Err(org_store::Refusal::invalid(
                "office-system sender_office_id does not match authenticated origin office",
            ));
        }
        return Ok((
            origin_office.to_owned(),
            msg.sender_display_name.clone(),
            false,
        ));
    }
    let Some(sid) = msg.sender_employee_id.as_ref() else {
        return Err(org_store::Refusal::invalid(
            "employee message missing sender_employee_id",
        ));
    };
    let Some(emp) = employees
        .iter()
        .find(|e| e.id == *sid && e.status == EmployeeStatus::Active)
    else {
        return Err(org_store::Refusal::invalid(
            "relayed message sender is not an active employee",
        ));
    };
    // Bind agent authors strictly to their placed office so satellite A cannot spoof an agent
    // placed on satellite B or HQ (`TASK-52` P4-#1).
    if emp.kind == EmployeeKind::Agent {
        let placed = placement
            .employee_offices
            .get(emp.id.as_str())
            .map(String::as_str)
            .unwrap_or(placement.hq_office_id.as_str());
        if placed != origin_office {
            return Err(org_store::Refusal::invalid(
                "relayed agent sender is not placed at authenticated origin office",
            ));
        }
    }
    Ok((
        emp.id.as_str().to_owned(),
        emp.name.clone(),
        emp.kind == EmployeeKind::Person,
    ))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ConvWriteMode {
    None,
    Create,
    Update,
}

/// Validates the immutable identity of a deterministic automatic channel (`#general`, `#team-*`,
/// `#proj-*`) against the receiver's committed organization snapshot and derives its mutable
/// channel name from the receiver's current org/team/project record so a team rename while a
/// satellite is offline never strands its first automatic-channel message (`TASK-52` P12-#3 / P13-#5).
fn canonicalize_deterministic_auto_channel(
    view: &impl crate::messaging::ports::MessagingRead,
    conv: &crate::messaging::records::ConversationRecord,
) -> Option<crate::messaging::records::ConversationRecord> {
    use crate::messaging::records::derive_conversation_id;
    use crate::organization::records::TeamStatus;
    use org_proto::chat::{AutoScope, ConversationKind};
    if conv.kind != ConversationKind::Channel
        || conv.archived
        || !conv.memberships.is_empty()
        || !conv.pins.is_empty()
    {
        return None;
    }
    let org = view.organization().ok()??;
    if conv.organization_id != org.id {
        return None;
    }
    let canonical_name = match conv.auto_scope {
        Some(AutoScope::Organization)
            if conv.id == derive_conversation_id("auto:org", org.id.as_str())
                && conv.team_id.is_none()
                && conv.project_id.is_none() =>
        {
            "general".to_owned()
        }
        Some(AutoScope::Team) => {
            let tid = conv.team_id.as_ref()?;
            if conv.project_id.is_some()
                || conv.id != derive_conversation_id("auto:team", tid.as_str())
            {
                return None;
            }
            let teams = view.teams().ok()?;
            let team = teams
                .iter()
                .find(|t| t.id == *tid && t.status == TeamStatus::Active)?;
            crate::messaging::features::slugify_channel(&team.name)
        }
        Some(AutoScope::Project) => {
            let pid = conv.project_id.as_ref()?;
            if conv.team_id.is_some()
                || conv.id != derive_conversation_id("auto:project", pid.as_str())
            {
                return None;
            }
            let projects = view.projects().ok()?;
            let project = projects.iter().find(|p| p.id == *pid && !p.archived)?;
            project.slug.clone()
        }
        _ => return None,
    };
    let mut canonical = conv.clone();
    canonical.name = Some(canonical_name);
    Some(canonical)
}

fn resolve_relayed_conversation(
    view: &impl crate::messaging::ports::MessagingRead,
    owner: &Id,
    origin_office: &str,
    hq_office_id: &str,
    incoming: Option<&crate::messaging::records::ConversationRecord>,
    msg: &crate::messaging::records::MessageRecord,
) -> Result<
    (
        crate::messaging::records::ConversationRecord,
        ConvWriteMode,
        Vec<Id>,
    ),
    org_store::Refusal,
> {
    let existing = view
        .conversation(&msg.conversation_id)
        .map_err(|_| org_store::Refusal::invalid("read conversation"))?;
    let (conv, write_mode) = match (existing, incoming) {
        (Some(local), Some(remote)) if local.kind != remote.kind => {
            return Err(org_store::Refusal::invalid(
                "relayed conversation kind mismatch",
            ));
        }
        // Adopt HQ-authoritative newer conversation revisions (e.g. channel membership changes,
        // `TASK-52` P5-#4) while preventing non-HQ peers from overwriting existing conversations.
        (Some(local), Some(remote))
            if origin_office == hq_office_id && remote.revision > local.revision =>
        {
            (remote.clone(), ConvWriteMode::Update)
        }
        (Some(local), _) => (local, ConvWriteMode::None),
        (None, Some(remote)) => {
            // Enforce single HQ channel ownership for custom channels while allowing satellites
            // to materialize deterministic auto-channels with canonical receiver-derived names
            // (`TASK-52` P8-#6 / P12-#3 / P13-#5).
            if remote.kind == org_proto::chat::ConversationKind::Channel
                && origin_office != hq_office_id
            {
                let Some(canonical) = canonicalize_deterministic_auto_channel(view, remote) else {
                    return Err(org_store::Refusal::invalid(
                        "only HQ may create relayed custom channels",
                    ));
                };
                (canonical, ConvWriteMode::Create)
            } else {
                (remote.clone(), ConvWriteMode::Create)
            }
        }
        (None, None) => {
            return Err(org_store::Refusal::invalid(
                "conversation missing for relayed message",
            ));
        }
    };
    // Reuse native team/project/org/explicit membership rules (`TASK-52` P4-#2).
    let active_members = crate::messaging::features::effective_auto_members(view, &conv, owner)
        .map_err(|_| org_store::Refusal::invalid("resolve conversation members"))?;
    if msg
        .sender_employee_id
        .as_ref()
        .is_some_and(|sid| !active_members.contains(sid))
    {
        return Err(org_store::Refusal::invalid(
            "relayed message sender is not an active conversation member",
        ));
    }
    Ok((conv, write_mode, active_members))
}

struct RelayedRecipientDeliveries {
    local_agent_recipients: Vec<String>,
    local_recipient_wakes: Vec<RelayedRecipientWake>,
    local_delivered_headers: Vec<org_proto::msg::MessageHeader>,
}

fn relayed_wake_kind_for_message(
    conv_kind: org_proto::chat::ConversationKind,
    msg: &crate::messaging::records::MessageRecord,
    target_id: &str,
    delivered_because: org_proto::chat::DeliveredBecause,
    is_mentioned: bool,
    open_question_asker_id: Option<&str>,
) -> Option<&'static str> {
    use org_proto::chat::{ConversationKind, DeliveredBecause};
    if msg.kind == "announcement" {
        return None;
    }
    if msg.kind == org_proto::msg::KIND_REQUEST {
        return Some("ask");
    }
    if msg.kind == "handoff"
        && (delivered_because == DeliveredBecause::Addressed
            || matches!(conv_kind, ConversationKind::Dm | ConversationKind::GroupDm))
    {
        return Some("handoff");
    }
    if open_question_asker_id == Some(target_id) {
        return Some("ask_answer");
    }
    if delivered_because == DeliveredBecause::Mention || is_mentioned {
        return Some("mention");
    }
    if matches!(conv_kind, ConversationKind::Dm | ConversationKind::GroupDm) {
        return Some("direct_message");
    }
    None
}

fn attach_relayed_recipient_states(
    view: &impl crate::messaging::ports::MessagingRead,
    ctx: &RelayedRecipientCtx<'_>,
    mut change: org_store::Change,
) -> Result<(org_store::Change, RelayedRecipientDeliveries), org_store::Refusal> {
    use crate::messaging::features::{
        build_mention_candidates, collect_recipient_reasons, to_wire_header,
        upsert_participant_snapshot,
    };
    use crate::messaging::mentions::extract_mentions;
    use crate::messaging::records::RecipientStateRecord;
    use crate::organization::records::EmployeeKind;

    let sender_id = ctx.msg.sender_employee_id.as_ref().unwrap_or(ctx.owner);
    let candidates = build_mention_candidates(ctx.employees, ctx.active_members, ctx.owner);
    let scan = extract_mentions(&ctx.msg.body_markdown, &candidates);
    let reasons = collect_recipient_reasons(
        view,
        ctx.conv,
        sender_id,
        ctx.active_members,
        &scan.resolved,
        ctx.msg.thread_root_id.as_deref(),
        &ctx.msg.kind,
        &ctx.msg.metadata,
    )?;
    let open_question_asker_id = ctx
        .msg
        .thread_root_id
        .as_deref()
        .and_then(|root_id| view.message(root_id).ok().flatten())
        .filter(|root| {
            root.kind == org_proto::msg::KIND_REQUEST
                && crate::messaging::asks::is_open_ask_card(view, root)
        })
        .and_then(|root| root.sender_employee_id.map(|id| id.as_str().to_owned()));
    let local_seq = view
        .message_local_seq(&ctx.msg.id)
        .ok()
        .flatten()
        .unwrap_or_else(|| {
            view.max_local_seq()
                .ok()
                .map_or(ctx.msg.origin_sequence, |s| s.saturating_add(1))
        });
    let mut local_agent_recipients = Vec::new();
    let mut local_recipient_wakes = Vec::new();
    let mut local_delivered_headers = Vec::new();
    for (emp_id, because) in reasons {
        // Commit inbox recipient state ONLY at the single office owning that recipient so multiple
        // offices never create diverging inbox copies for the same employee (`TASK-52` P4-#3).
        let owner_office = ctx
            .placement
            .employee_offices
            .get(emp_id.as_str())
            .map(String::as_str)
            .unwrap_or(ctx.placement.hq_office_id.as_str());
        if owner_office != ctx.here.office_id.as_str() {
            continue;
        }
        let emp = ctx.employees.iter().find(|e| e.id == emp_id);
        let display_name = emp.map(|e| e.name.clone());
        if emp.is_some_and(|e| e.kind == EmployeeKind::Agent) {
            let is_mentioned = scan
                .resolved
                .iter()
                .any(|m| m.employee_id == emp_id.as_str());
            if let Some(wake_kind) = relayed_wake_kind_for_message(
                ctx.conv.kind,
                ctx.msg,
                emp_id.as_str(),
                because,
                is_mentioned,
                open_question_asker_id.as_deref(),
            ) {
                local_agent_recipients.push(emp_id.as_str().to_owned());
                local_recipient_wakes.push(RelayedRecipientWake {
                    employee_id: emp_id.as_str().to_owned(),
                    kind: wake_kind.to_owned(),
                });
            }
        }
        let state = RecipientStateRecord {
            organization_id: ctx.here.organization_id.clone(),
            message_id: ctx.msg.id.clone(),
            conversation_id: ctx.conv.id.clone(),
            employee_id: emp_id.clone(),
            recipient_display_name: display_name,
            state: org_proto::msg::MessageLifecycleState::Unread,
            triage_generation: 0,
            snoozed_until: None,
            snooze_after_task: false,
            snooze_note: None,
            triaged_at: None,
            read_at: None,
            updated_at: ctx.msg.created_at.clone(),
            reason: None,
            delivered_because: because,
            related_message_ids: Vec::new(),
            expired_waiting_person: false,
            owner_office_id: ctx.here.office_id.clone(),
            origin_incarnation: ctx.msg.origin_incarnation.clone(),
            origin_sequence: ctx.msg.origin_sequence,
            org_epoch: ctx.msg.org_epoch,
            org_sequence: ctx.msg.org_sequence,
        };
        local_delivered_headers.push(to_wire_header(ctx.msg, Some(&state), local_seq));
        change = upsert_participant_snapshot(
            view,
            &ctx.here.organization_id,
            &ctx.conv.id,
            &emp_id,
            state,
            change,
        )?;
    }
    Ok((
        change,
        RelayedRecipientDeliveries {
            local_agent_recipients,
            local_recipient_wakes,
            local_delivered_headers,
        },
    ))
}

fn attach_relayed_ask_answer(
    view: &impl crate::messaging::ports::MessagingRead,
    rctx: &RelayedRecipientCtx<'_>,
    incoming_answer: Option<&(Id, String, crate::messaging::records::ParticipantAnswerFile)>,
    mut change: org_store::Change,
) -> Result<(org_store::Change, Option<RelayedAskAnswer>, Option<String>), org_store::Refusal> {
    let Some((emp_id, ask_id, answer_file)) = incoming_answer else {
        return Ok((change, None, None));
    };
    let Some(ask_msg) = view
        .message(ask_id)
        .map_err(|_| org_store::Refusal::invalid("read ask message"))?
    else {
        return Ok((change, None, None));
    };
    if ask_msg.conversation_id != rctx.conv.id || ask_msg.kind != "request" {
        return Err(org_store::Refusal::invalid(
            "relayed answer does not target a request in this conversation",
        ));
    }
    let Some(card) = crate::messaging::asks::parse_ask_card(&ask_msg) else {
        return Err(org_store::Refusal::invalid(
            "relayed answer targets message without ask card",
        ));
    };
    let Some(responder) = rctx.employees.iter().find(|e| e.id == *emp_id) else {
        return Err(org_store::Refusal::invalid("relayed ask responder missing"));
    };
    // Reuse native approval, self-answer, addressee, and typed answer-set rules (`TASK-52` P6-#3).
    if !rctx.active_members.contains(emp_id) && emp_id != rctx.owner {
        return Err(org_store::Refusal::invalid(
            "relayed ask responder is not a conversation member",
        ));
    }
    if card.approval && responder.kind != crate::organization::records::EmployeeKind::Person {
        return Err(org_store::Refusal::invalid(
            "only a person may answer an approval",
        ));
    }
    if ask_msg.sender_employee_id.as_ref() == Some(emp_id) {
        return Err(org_store::Refusal::invalid(
            "the asker cannot answer their own ask",
        ));
    }
    if card
        .addressed_to
        .as_deref()
        .is_some_and(|t| t != emp_id.as_str())
    {
        return Err(org_store::Refusal::invalid(
            "only the addressed recipient may answer this ask",
        ));
    }
    let (Some(decoded_ans), _) = crate::messaging::asks::decode_ask_answer_file(answer_file) else {
        return Err(org_store::Refusal::invalid("malformed relayed ask answer"));
    };
    let validated =
        crate::messaging::asks::validate_answer_set(&card, decoded_ans.answers.clone())?;
    let existing_canonical = view
        .answer_for_message(ask_id)
        .map_err(|_| org_store::Refusal::invalid("read existing ask answer"))?;
    let is_ask_origin = ask_msg.origin_office_id == rctx.here.office_id;
    let (skip_write, emit_resume) = evaluate_relayed_ask_close(
        is_ask_origin,
        existing_canonical.as_ref(),
        emp_id,
        answer_file,
    );
    let resume_info = emit_resume
        .then(|| build_local_ask_answer_resume(rctx, &ask_msg, &card, &validated))
        .flatten();
    let closed_ask_id = emit_resume.then(|| ask_id.clone());
    if skip_write {
        return Ok((change, resume_info, closed_ask_id));
    }
    let mut stored_answer = answer_file.clone();
    if is_ask_origin {
        stored_answer.reason = Some("origin-close".to_owned());
    }
    let encoded_answer = crate::messaging::records::encode_json(&stored_answer);
    let path = crate::messaging::records::participant_answer_path(&rctx.conv.id, emp_id, ask_id);
    change = change.put(path, encoded_answer);
    let (next_change, _) = crate::messaging::asks::mark_ask_recipients_done(
        view,
        rctx.here,
        &ask_msg,
        ask_msg.org_sequence,
        &ask_msg.created_at,
        change,
    )?;
    Ok((next_change, resume_info, closed_ask_id))
}

/// Evaluates `(skip_write, emit_resume)` for a relayed ask answer:
/// - On the ask's origin office (`is_ask_origin`), enforces strict first-close consistency so
///   display and resume never diverge (`TASK-52` P9-#6).
/// - On non-origin offices, preserves every valid responder's submission and converges onto the
///   deterministic `(accepted_at, employee_id)` canonical winner (`TASK-52` P7-#5 / P8-#9).
fn evaluate_relayed_ask_close(
    is_ask_origin: bool,
    existing_canonical: Option<&crate::messaging::records::ParticipantAnswerFile>,
    emp_id: &Id,
    answer_file: &crate::messaging::records::ParticipantAnswerFile,
) -> (bool, bool) {
    let Some(existing) = existing_canonical else {
        return (false, true);
    };
    let exact_replay = existing.employee_id == *emp_id && existing.answer == answer_file.answer;
    if is_ask_origin {
        return (true, exact_replay);
    }
    let responder_stored = existing.employee_id == *emp_id;
    let emit_resume = is_canonical_relayed_ask_winner(Some(existing), emp_id, answer_file);
    (responder_stored, emit_resume)
}

/// Deterministic `(accepted_at, employee_id)` canonical winner comparison so concurrent offline
/// answers converge to the identical canonical answer across all offices (`TASK-52` P7-#5 / P8-#9).
fn is_canonical_relayed_ask_winner(
    existing_canonical: Option<&crate::messaging::records::ParticipantAnswerFile>,
    emp_id: &Id,
    answer_file: &crate::messaging::records::ParticipantAnswerFile,
) -> bool {
    let Some(existing) = existing_canonical else {
        return true;
    };
    let incoming_key = (
        answer_file.accepted_at.as_deref().unwrap_or(""),
        emp_id.as_str(),
    );
    let existing_key = (
        existing.accepted_at.as_deref().unwrap_or(""),
        existing.employee_id.as_str(),
    );
    incoming_key < existing_key
        || (existing.employee_id == *emp_id && existing.answer == answer_file.answer)
}

fn build_local_ask_answer_resume(
    rctx: &RelayedRecipientCtx<'_>,
    ask_msg: &crate::messaging::records::MessageRecord,
    card: &crate::messaging::asks::AskCard,
    validated: &[org_proto::ask::QuestionAnswer],
) -> Option<RelayedAskAnswer> {
    let asker_id = ask_msg.sender_employee_id.as_ref()?;
    let owner_office = rctx
        .placement
        .employee_offices
        .get(asker_id.as_str())
        .map(String::as_str)
        .unwrap_or(rctx.placement.hq_office_id.as_str());
    if owner_office != rctx.here.office_id.as_str() {
        return None;
    }
    let is_agent = rctx
        .employees
        .iter()
        .any(|e| e.id == *asker_id && e.kind == crate::organization::records::EmployeeKind::Agent);
    if !is_agent {
        return None;
    }
    let question_summary = card
        .questions
        .iter()
        .map(|q| q.prompt.as_str())
        .collect::<Vec<_>>()
        .join(" / ");
    let answer_summary = crate::messaging::asks::render_answer_summary(card, validated);
    Some(RelayedAskAnswer {
        ask_id: ask_msg.id.clone(),
        asker_id: asker_id.as_str().to_owned(),
        question_summary,
        answer_summary,
    })
}

fn decide_apply_relayed_message<E: crate::store::open::OfficeEngine>(
    uow: &mut org_store::UnitOfWork<'_, E>,
    req: &ApplyRelayedMessageOp,
) -> Result<org_store::Change, org_store::Refusal> {
    use crate::messaging::ports::MessagingRead;
    use crate::messaging::records::{conversation_path, encode_json, message_path};
    use crate::organization::ports::OrganizationRead;
    use org_store::{Ack, Change, Refusal};

    if message_expires_at_ms(&req.msg).is_some_and(|exp| exp <= req.now_ms) {
        return Err(Refusal::invalid("relayed message already expired"));
    }
    let view = uow.view();
    let employees = view
        .employees()
        .map_err(|_| Refusal::invalid("read employees"))?;
    let (sender_id, sender_name, sender_is_person) =
        validate_relayed_sender(&employees, &req.ctx, &req.origin_office, &req.msg)?;
    let (conv, write_mode, active_members) = resolve_relayed_conversation(
        view,
        &req.owner,
        &req.origin_office,
        &req.ctx.hq_office_id,
        req.incoming_conv.as_ref(),
        &req.msg,
    )?;
    let rctx = RelayedRecipientCtx {
        here: &req.here,
        owner: &req.owner,
        placement: &req.ctx,
        employees: &employees,
        conv: &conv,
        active_members: &active_members,
        msg: &req.msg,
    };
    if let Some(existing_msg) = view
        .message(&req.msg.id)
        .map_err(|_| Refusal::invalid("read existing message"))?
    {
        if existing_msg.content_sha256 != req.msg.content_sha256 {
            return Err(Refusal::invalid("relayed message content_sha256 conflict"));
        }
        let (_, answered_ask, closed_ask_id) = attach_relayed_ask_answer(
            view,
            &rctx,
            req.incoming_answer.as_ref(),
            Change::new(Ack::Durable),
        )?;
        let (_, delivered) =
            attach_relayed_recipient_states(view, &rctx, Change::new(Ack::Durable))?;
        let outcome = RelayedMessageOutcome {
            message_id: req.msg.id.clone(),
            conversation_id: conv.id.as_str().to_owned(),
            sender_id,
            sender_name,
            sender_is_person,
            is_dm: conv.kind == org_proto::chat::ConversationKind::Dm,
            inserted: false,
            conversation_kind: Some(conv.kind),
            summary: relayed_wake_summary(&req.msg.body_markdown, &req.msg.id),
            local_agent_recipients: delivered.local_agent_recipients,
            local_recipient_wakes: delivered.local_recipient_wakes,
            local_delivered_headers: delivered.local_delivered_headers,
            answered_ask,
            withdrawn_ask: None,
            closed_ask_reason: closed_ask_id.as_ref().map(|_| "answered".to_owned()),
            closed_ask_id,
            handoff_accepted: false,
            delivered_message: None,
        };
        let reply = serde_json::to_vec(&outcome).unwrap_or_default();
        return Ok(Change::new(Ack::Durable).reply(reply));
    }
    let mut change = Change::new(Ack::Durable);
    match write_mode {
        ConvWriteMode::Create => {
            change = change.create(conversation_path(&conv.id), encode_json(&conv));
        }
        ConvWriteMode::Update => {
            change = change.put(conversation_path(&conv.id), encode_json(&conv));
        }
        ConvWriteMode::None => {}
    }
    change = change.create(message_path(&conv.id, &req.msg.id), encode_json(&req.msg));
    let (change, answered_ask, closed_ask_id) =
        attach_relayed_ask_answer(view, &rctx, req.incoming_answer.as_ref(), change)?;
    let (change, delivered) = attach_relayed_recipient_states(view, &rctx, change)?;
    let outcome = RelayedMessageOutcome {
        message_id: req.msg.id.clone(),
        conversation_id: conv.id.as_str().to_owned(),
        sender_id,
        sender_name,
        sender_is_person,
        is_dm: conv.kind == org_proto::chat::ConversationKind::Dm,
        inserted: true,
        conversation_kind: Some(conv.kind),
        summary: relayed_wake_summary(&req.msg.body_markdown, &req.msg.id),
        local_agent_recipients: delivered.local_agent_recipients,
        local_recipient_wakes: delivered.local_recipient_wakes,
        local_delivered_headers: delivered.local_delivered_headers,
        answered_ask,
        withdrawn_ask: None,
        closed_ask_reason: closed_ask_id.as_ref().map(|_| "answered".to_owned()),
        closed_ask_id,
        handoff_accepted: false,
        delivered_message: None,
    };
    let reply = serde_json::to_vec(&outcome).unwrap_or_default();
    Ok(change.reply(reply))
}

const MAX_RELAYED_WAKE_SUMMARY_CHARS: usize = 4096;

fn relayed_wake_summary(body: &str, message_id: &str) -> String {
    let mut chars = body.chars();
    let prefix: String = chars
        .by_ref()
        .take(MAX_RELAYED_WAKE_SUMMARY_CHARS)
        .collect();
    if chars.next().is_some() {
        format!(
            "{prefix}\n...[truncated; run `agent-os msg read --id {message_id}` for the full body]"
        )
    } else {
        prefix
    }
}

struct ApplyRelayedConversationOnlyOp {
    operation_id: Id,
    origin_office: String,
    ctx: PlacementContext,
    conv: crate::messaging::records::ConversationRecord,
}

impl crate::store::open::StoreOp for ApplyRelayedConversationOnlyOp {
    type Out = Result<org_store::Reply, org_store::StoreError>;

    fn run<E: crate::store::open::OfficeEngine>(self, store: &org_store::Store<E>) -> Self::Out {
        use crate::messaging::ports::MessagingRead;
        use crate::messaging::records::{conversation_path, encode_json};
        use org_store::{Ack, Change, Refusal};
        let op = org_store::Operation {
            id: self.operation_id,
            kind: "replication.apply_conversation",
            actor: Actor::System,
            fingerprint: org_store::Sha256::of_parts(&[
                self.conv.id.as_str().as_bytes(),
                b"\0",
                &self.conv.revision.to_le_bytes(),
            ]),
        };
        let origin = self.origin_office;
        let ctx = self.ctx;
        let conv = self.conv;
        store.submit(Some(op), false, move |uow| {
            let existing = uow
                .view()
                .conversation(&conv.id)
                .map_err(|_| Refusal::invalid("read conversation"))?;
            let is_channel = conv.kind == org_proto::chat::ConversationKind::Channel;
            let origin_authorized_dm = !is_channel
                && conv.memberships.iter().any(|p| {
                    p.left_at.is_none()
                        && ctx
                            .employee_offices
                            .get(p.employee_id.as_str())
                            .map(String::as_str)
                            .unwrap_or(ctx.hq_office_id.as_str())
                            == origin.as_str()
                });
            let change = match existing {
                Some(local) if local.kind != conv.kind => {
                    return Err(Refusal::invalid("conversation kind mismatch"));
                }
                Some(local)
                    if is_channel
                        && origin == ctx.hq_office_id
                        && conv.revision > local.revision =>
                {
                    Change::new(Ack::Durable).put(conversation_path(&conv.id), encode_json(&conv))
                }
                Some(_) => Change::new(Ack::Durable),
                None if is_channel && origin == ctx.hq_office_id => Change::new(Ack::Durable)
                    .create(conversation_path(&conv.id), encode_json(&conv)),
                None if is_channel => {
                    let Some(canonical) =
                        canonicalize_deterministic_auto_channel(uow.view(), &conv)
                    else {
                        return Err(Refusal::invalid(
                            "origin office lacks authority to create relayed custom channel",
                        ));
                    };
                    Change::new(Ack::Durable)
                        .create(conversation_path(&canonical.id), encode_json(&canonical))
                }
                None if origin_authorized_dm => Change::new(Ack::Durable)
                    .create(conversation_path(&conv.id), encode_json(&conv)),
                None => {
                    return Err(Refusal::invalid(
                        "origin office lacks authority to create relayed conversation",
                    ));
                }
            };
            Ok(change)
        })
    }
}

struct ParsedRelayedMessage {
    conv: Option<crate::messaging::records::ConversationRecord>,
    answer: Option<(Id, String, crate::messaging::records::ParticipantAnswerFile)>,
    msg: crate::messaging::records::MessageRecord,
}

struct ParsedOriginAskClose {
    conv: Option<crate::messaging::records::ConversationRecord>,
    ask_msg: Option<crate::messaging::records::MessageRecord>,
    cid: Id,
    emp_id: Id,
    ask_id: String,
    answer: crate::messaging::records::ParticipantAnswerFile,
}

struct ParsedOriginAskRecovery {
    conv: Option<crate::messaging::records::ConversationRecord>,
    cid: Id,
    ask_msg: crate::messaging::records::MessageRecord,
}

struct ParsedHandoffAccept {
    cid: Id,
    emp_id: Id,
    message_id: String,
    answer: crate::messaging::records::ParticipantAnswerFile,
}

enum ParsedRelayedDelta {
    ConversationOnly(Box<crate::messaging::records::ConversationRecord>),
    OriginAskRecovery(Box<ParsedOriginAskRecovery>),
    OriginAskClose(Box<ParsedOriginAskClose>),
    HandoffAccept(Box<ParsedHandoffAccept>),
    Message(Box<ParsedRelayedMessage>),
}

#[derive(Serialize, Deserialize)]
struct StagedAskRecovery {
    conv: Option<crate::messaging::records::ConversationRecord>,
    ask_msg: crate::messaging::records::MessageRecord,
}

fn close_recovery_stage_path(office: &Office, origin_office: &str, ask_id: &str) -> PathBuf {
    let key =
        org_store::Sha256::of_parts(&[origin_office.as_bytes(), b"\0", ask_id.as_bytes()]).to_hex();
    // Store staged split-recovery asks in durable office-local storage (same durability scope as
    // `replication/state.v1.json`) so rebuilding `runtime/` never strands an acknowledged
    // `close-ask-*` delivery (`TASK-52` P16-#1).
    office
        .durable
        .join("local")
        .join("offices")
        .join(&office.info.office_id)
        .join("close-recovery")
        .join(format!("{key}.json"))
}

fn stage_close_recovery(
    office: &Office,
    origin_office: &str,
    ask_id: &str,
    staged: &StagedAskRecovery,
) -> Result<(), OfficeError> {
    let _guard = office
        .federation_io_lock
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let path = close_recovery_stage_path(office, origin_office, ask_id);
    if let Some(parent) = path.parent() {
        private_dir_synced(&office.durable, parent).map_err(OfficeError::storage)?;
    }
    let bytes = serde_json::to_vec(staged).map_err(OfficeError::storage)?;
    write_private_atomic(&path, &bytes).map_err(OfficeError::storage)
}

fn load_staged_close_recovery(
    office: &Office,
    origin_office: &str,
    ask_id: &str,
) -> Option<StagedAskRecovery> {
    let _guard = office
        .federation_io_lock
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let path = close_recovery_stage_path(office, origin_office, ask_id);
    let raw = std::fs::read(&path).ok()?;
    serde_json::from_slice(&raw).ok()
}

fn remove_staged_close_recovery(office: &Office, origin_office: &str, ask_id: &str) {
    let _guard = office
        .federation_io_lock
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let path = close_recovery_stage_path(office, origin_office, ask_id);
    let _ = std::fs::remove_file(path);
}

/// Read-only entitlement check for a split `close-ask-*` recovery delivery: returns `Ok(true)` if
/// the ask is already committed locally with matching digest, `Ok(false)` if the local office is
/// entitled and should stage the ask until `close-*` arrives, or `Err` if unentitled/invalid
/// (`TASK-52` P15-#1).
struct CheckAskRecoveryEntitlementOp {
    here: org_store::OfficeIdentity,
    owner: Id,
    ctx: PlacementContext,
    origin_office: String,
    incoming_conv: Option<crate::messaging::records::ConversationRecord>,
    cid: Id,
    ask_msg: crate::messaging::records::MessageRecord,
}

impl crate::store::open::StoreOp for CheckAskRecoveryEntitlementOp {
    type Out = Result<Result<bool, org_store::Refusal>, org_store::StoreError>;

    fn run<E: crate::store::open::OfficeEngine>(self, store: &org_store::Store<E>) -> Self::Out {
        let (res, _) = store.read(|snap| Ok(check_ask_recovery_entitlement(snap, &self)))?;
        Ok(res)
    }
}

fn check_ask_recovery_entitlement(
    view: &impl crate::messaging::ports::MessagingRead,
    req: &CheckAskRecoveryEntitlementOp,
) -> Result<bool, org_store::Refusal> {
    use org_store::Refusal;
    if let Some(existing) = view
        .message(&req.ask_msg.id)
        .map_err(|_| Refusal::invalid("read ask message"))?
    {
        if existing.content_sha256 != req.ask_msg.content_sha256 {
            return Err(Refusal::invalid("recovered ask digest conflict"));
        }
        return Ok(true);
    }
    let valid_origin = req.ask_msg.origin_office_id.as_str() == req.origin_office.as_str()
        || req.origin_office == req.ctx.hq_office_id;
    if req.ask_msg.conversation_id != req.cid || req.ask_msg.kind != "request" || !valid_origin {
        return Err(Refusal::invalid("invalid ask recovery payload"));
    }
    let (conv, _) = resolve_conv_for_close_recovery(
        view,
        &req.cid,
        &req.origin_office,
        &req.ctx.hq_office_id,
        req.incoming_conv.as_ref(),
    )?;
    ensure_local_office_entitled(view, &conv, &req.owner, &req.ctx, &req.here.office_id)?;
    Ok(false)
}

fn ensure_local_office_entitled(
    view: &impl crate::messaging::ports::MessagingRead,
    conv: &crate::messaging::records::ConversationRecord,
    owner: &Id,
    ctx: &PlacementContext,
    local_office_id: &Id,
) -> Result<(), org_store::Refusal> {
    let members = crate::messaging::features::effective_auto_members(view, conv, owner)
        .map_err(|_| org_store::Refusal::invalid("resolve conversation members"))?;
    let local_entitled = members.iter().any(|emp| {
        ctx.employee_offices
            .get(emp.as_str())
            .map(String::as_str)
            .unwrap_or(ctx.hq_office_id.as_str())
            == local_office_id.as_str()
    });
    if local_entitled {
        Ok(())
    } else {
        Err(org_store::Refusal::invalid(
            "office is not entitled to this conversation",
        ))
    }
}

struct ApplyRelayedOriginCloseOp {
    operation_id: Id,
    here: org_store::OfficeIdentity,
    owner: Id,
    ctx: PlacementContext,
    origin_office: String,
    incoming_conv: Option<crate::messaging::records::ConversationRecord>,
    incoming_ask: Option<crate::messaging::records::MessageRecord>,
    cid: Id,
    emp_id: Id,
    ask_id: String,
    answer: crate::messaging::records::ParticipantAnswerFile,
}

impl crate::store::open::StoreOp for ApplyRelayedOriginCloseOp {
    type Out = Result<org_store::Reply, org_store::StoreError>;

    fn run<E: crate::store::open::OfficeEngine>(self, store: &org_store::Store<E>) -> Self::Out {
        let op = org_store::Operation {
            id: self.operation_id.clone(),
            kind: "replication.apply_origin_close",
            actor: Actor::System,
            fingerprint: org_store::Sha256::of_parts(&[
                self.cid.as_str().as_bytes(),
                b"\0",
                self.ask_id.as_bytes(),
                b"\0",
                self.emp_id.as_str().as_bytes(),
            ]),
        };
        store.submit(Some(op), false, move |uow| {
            decide_apply_relayed_origin_close(uow.view(), &self)
        })
    }
}

fn resolve_origin_close_ask(
    view: &impl crate::messaging::ports::MessagingRead,
    req: &ApplyRelayedOriginCloseOp,
) -> Result<
    (
        crate::messaging::records::MessageRecord,
        Option<crate::messaging::records::ConversationRecord>,
        bool,
    ),
    org_store::Refusal,
> {
    use org_store::Refusal;
    if let Some(local_ask) = view
        .message(&req.ask_id)
        .map_err(|_| Refusal::invalid("read ask message"))?
    {
        if local_ask.conversation_id != req.cid || local_ask.kind != "request" {
            return Err(Refusal::invalid("origin-close ask record mismatch"));
        }
        if req
            .incoming_ask
            .as_ref()
            .is_some_and(|inc| inc.content_sha256 != local_ask.content_sha256)
        {
            return Err(Refusal::invalid("origin-close ask digest conflict"));
        }
        return Ok((local_ask, None, false));
    }
    let Some(inc_ask) = req.incoming_ask.as_ref() else {
        return Err(Refusal::invalid("ask not present on this office"));
    };
    let valid_origin = inc_ask.origin_office_id.as_str() == req.origin_office.as_str()
        || req.origin_office == req.ctx.hq_office_id;
    if inc_ask.id != req.ask_id
        || inc_ask.conversation_id != req.cid
        || inc_ask.kind != "request"
        || !valid_origin
    {
        return Err(Refusal::invalid("invalid bundled origin-close ask"));
    }
    let (conv, conv_to_create) = resolve_conv_for_close_recovery(
        view,
        &req.cid,
        &req.origin_office,
        &req.ctx.hq_office_id,
        req.incoming_conv.as_ref(),
    )?;
    ensure_local_office_entitled(view, &conv, &req.owner, &req.ctx, &req.here.office_id)?;
    Ok((inc_ask.clone(), conv_to_create, true))
}

fn resolve_conv_for_close_recovery(
    view: &impl crate::messaging::ports::MessagingRead,
    cid: &Id,
    origin_office: &str,
    hq_office_id: &str,
    incoming_conv: Option<&crate::messaging::records::ConversationRecord>,
) -> Result<
    (
        crate::messaging::records::ConversationRecord,
        Option<crate::messaging::records::ConversationRecord>,
    ),
    org_store::Refusal,
> {
    use org_store::Refusal;
    if let Some(local_conv) = view
        .conversation(cid)
        .map_err(|_| Refusal::invalid("read conversation"))?
    {
        return Ok((local_conv, None));
    }
    let Some(remote) = incoming_conv else {
        return Err(Refusal::invalid(
            "conversation missing for relayed ask close",
        ));
    };
    let is_channel = remote.kind == org_proto::chat::ConversationKind::Channel;
    if !is_channel || origin_office == hq_office_id {
        return Ok((remote.clone(), Some(remote.clone())));
    }
    let Some(canonical) = canonicalize_deterministic_auto_channel(view, remote) else {
        return Err(Refusal::invalid(
            "only HQ may create relayed custom channels",
        ));
    };
    Ok((canonical.clone(), Some(canonical)))
}

fn build_local_ask_withdrawal_reply(
    view: &impl crate::messaging::ports::MessagingRead,
    req: &ApplyRelayedOriginCloseOp,
    ask_msg: &crate::messaging::records::MessageRecord,
    is_withdrawal: bool,
) -> Vec<u8> {
    let withdrawn_ask = if is_withdrawal {
        ask_msg.sender_employee_id.as_ref().and_then(|asker_id| {
            let owner_office = req
                .ctx
                .employee_offices
                .get(asker_id.as_str())
                .map(String::as_str)
                .unwrap_or(req.ctx.hq_office_id.as_str());
            if owner_office != req.here.office_id.as_str() {
                return None;
            }
            let is_agent = view.employees().ok().is_some_and(|emps| {
                emps.iter().any(|e| {
                    e.id == *asker_id && e.kind == crate::organization::records::EmployeeKind::Agent
                })
            });
            is_agent.then(|| RelayedAskWithdrawal {
                ask_id: req.ask_id.clone(),
                asker_id: asker_id.as_str().to_owned(),
            })
        })
    } else {
        None
    };
    let outcome = RelayedMessageOutcome {
        conversation_id: req.cid.as_str().to_owned(),
        sender_id: req.emp_id.as_str().to_owned(),
        withdrawn_ask,
        closed_ask_id: Some(req.ask_id.clone()),
        closed_ask_reason: Some(
            if is_withdrawal {
                "withdrawn"
            } else {
                "answered"
            }
            .to_owned(),
        ),
        ..Default::default()
    };
    serde_json::to_vec(&outcome).unwrap_or_default()
}

fn decide_apply_relayed_origin_close(
    view: &impl crate::messaging::ports::MessagingRead,
    req: &ApplyRelayedOriginCloseOp,
) -> Result<org_store::Change, org_store::Refusal> {
    use crate::messaging::records::{
        conversation_path, encode_json, message_path, participant_answer_path,
    };
    use org_store::{Ack, Change, Refusal};
    let (ask_msg, conv_to_create, create_ask) = resolve_origin_close_ask(view, req)?;
    let is_withdrawal = crate::messaging::asks::decode_ask_answer_file(&req.answer)
        .1
        .is_some();
    let from_origin = ask_msg.origin_office_id.as_str() == req.origin_office.as_str();
    let from_hq_owner_withdrawal =
        is_withdrawal && req.origin_office == req.ctx.hq_office_id && req.emp_id == req.owner;
    if !from_origin && !from_hq_owner_withdrawal {
        return Err(Refusal::invalid(
            "origin-close answer must originate from the ask's origin office or HQ owner withdrawal",
        ));
    }
    // At the ask's origin office, preserve an already-committed local close if a provisional HQ
    // withdrawal races behind it; on non-origin participants, HQ owner withdrawals are only ever
    // fanned out when the origin office was removed and therefore override provisional answers
    // (`TASK-52` P13-#3 / P14-#5).
    if !from_origin && ask_msg.origin_office_id == req.here.office_id {
        if let Some(existing) = view
            .answer_for_message(&req.ask_id)
            .map_err(|_| Refusal::invalid("read existing ask answer"))?
        {
            let (a, w) = crate::messaging::asks::decode_ask_answer_file(&existing);
            if a.is_some() || w.is_some() {
                let same_withdrawal = w.is_some() && existing.employee_id == req.emp_id;
                let reply = build_local_ask_withdrawal_reply(view, req, &ask_msg, same_withdrawal);
                return Ok(Change::new(Ack::Durable).reply(reply));
            }
        }
    }
    let mut change = Change::new(Ack::Durable);
    if let Some(conv_rec) = conv_to_create {
        change = change.create(conversation_path(&conv_rec.id), encode_json(&conv_rec));
    }
    if create_ask {
        change = change.create(message_path(&req.cid, &ask_msg.id), encode_json(&ask_msg));
    }
    let path = participant_answer_path(&req.cid, &req.emp_id, &req.ask_id);
    change = change.put(path, encode_json(&req.answer));
    if !create_ask {
        let (next_change, _) = crate::messaging::asks::mark_ask_recipients_done(
            view,
            &req.here,
            &ask_msg,
            ask_msg.org_sequence,
            &ask_msg.created_at,
            change,
        )?;
        change = next_change;
    }
    let reply = build_local_ask_withdrawal_reply(view, req, &ask_msg, is_withdrawal);
    Ok(change.reply(reply))
}

fn validate_relayed_answer_binding(
    msg_cid: &Id,
    msg: &crate::messaging::records::MessageRecord,
    parsed_answer: Option<(
        Id,
        Id,
        String,
        crate::messaging::records::ParticipantAnswerFile,
    )>,
) -> Result<Option<(Id, String, crate::messaging::records::ParticipantAnswerFile)>, OfficeError> {
    let Some((ans_cid, emp, ask_id, ans)) = parsed_answer else {
        return Ok(None);
    };
    let (decoded_ans, _) = crate::messaging::asks::decode_ask_answer_file(&ans);
    let responder_matches = msg.sender_employee_id.as_ref() == Some(&emp)
        && decoded_ans.as_ref().is_some_and(|a| a.by == emp.as_str());
    if ans_cid != *msg_cid
        || msg.reply_to_message_id.as_deref() != Some(ask_id.as_str())
        || !responder_matches
    {
        return Err(OfficeError::Invalid(
            "relayed answer does not match authenticated reply message",
        ));
    }
    Ok(Some((emp, ask_id, ans)))
}

fn parse_validated_relayed_delta(
    org_id: &Id,
    origin_office: &str,
    hq_office_id: &str,
    is_ask_recovery: bool,
    files: Vec<(String, Option<Vec<u8>>)>,
) -> Result<ParsedRelayedDelta, OfficeError> {
    use crate::messaging::records::{
        kind_of, ConversationRecord, Kind, MessageRecord, ParticipantAnswerFile,
    };

    let mut parsed_conv: Option<(Id, ConversationRecord)> = None;
    let mut parsed_msg: Option<(Id, String, MessageRecord)> = None;
    let mut parsed_answer: Option<(Id, Id, String, ParticipantAnswerFile)> = None;
    for (path, opt_bytes) in files {
        let Some(bytes) = opt_bytes else {
            return Err(OfficeError::Invalid(
                "relayed message cannot delete messaging files",
            ));
        };
        let dp = DurablePath::parse(&path)
            .map_err(|_| OfficeError::Invalid("invalid relayed messaging path"))?;
        match kind_of(&dp) {
            Some(Kind::Conversation(cid)) => {
                let conv: ConversationRecord = serde_json::from_slice(&bytes)
                    .map_err(|_| OfficeError::Invalid("malformed ConversationRecord"))?;
                if parsed_conv.is_some() || conv.organization_id != *org_id || conv.id != cid {
                    return Err(OfficeError::Invalid("ConversationRecord validation failed"));
                }
                parsed_conv = Some((cid, conv));
            }
            Some(Kind::Message(cid, mid)) => {
                let msg: MessageRecord = serde_json::from_slice(&bytes)
                    .map_err(|_| OfficeError::Invalid("malformed MessageRecord"))?;
                if parsed_msg.is_some()
                    || msg.organization_id != *org_id
                    || msg.conversation_id != cid
                    || msg.id != mid
                    || msg.content_sha256 != msg.compute_content_sha256()
                {
                    return Err(OfficeError::Invalid("MessageRecord validation failed"));
                }
                parsed_msg = Some((cid, mid, msg));
            }
            Some(Kind::ParticipantAnswer(cid, emp, ask_id)) => {
                let ans: ParticipantAnswerFile = serde_json::from_slice(&bytes)
                    .map_err(|_| OfficeError::Invalid("malformed ParticipantAnswerFile"))?;
                if parsed_answer.is_some()
                    || ans.organization_id != *org_id
                    || ans.conversation_id != cid
                    || ans.employee_id != emp
                    || ans.message_id != ask_id
                {
                    return Err(OfficeError::Invalid(
                        "ParticipantAnswerFile validation failed",
                    ));
                }
                parsed_answer = Some((cid, emp, ask_id, ans));
            }
            _ => {
                return Err(OfficeError::Invalid(
                    "relayed delta may only contain conversation, message, and optional ask answer",
                ));
            }
        }
    }
    assemble_parsed_relayed_delta(
        origin_office,
        hq_office_id,
        is_ask_recovery,
        parsed_conv,
        parsed_msg,
        parsed_answer,
    )
}

fn assemble_message_less_delta(
    parsed_conv: Option<(Id, crate::messaging::records::ConversationRecord)>,
    parsed_answer: Option<(
        Id,
        Id,
        String,
        crate::messaging::records::ParticipantAnswerFile,
    )>,
) -> Result<ParsedRelayedDelta, OfficeError> {
    match (parsed_conv, parsed_answer) {
        (Some((_, conv)), None) => Ok(ParsedRelayedDelta::ConversationOnly(Box::new(conv))),
        (None, Some((cid, emp, msg_id, ans)))
            if ans.answer.is_none() && ans.accepted_by.as_ref() == Some(&emp) =>
        {
            Ok(ParsedRelayedDelta::HandoffAccept(Box::new(
                ParsedHandoffAccept {
                    cid,
                    emp_id: emp,
                    message_id: msg_id,
                    answer: ans,
                },
            )))
        }
        (None, Some((cid, emp, ask_id, mut ans)))
            if ans.answer.is_some()
                && (ans.reason.as_deref() == Some("origin-close")
                    || crate::messaging::asks::decode_ask_answer_file(&ans)
                        .1
                        .is_some()) =>
        {
            ans.reason = Some("origin-close".to_owned());
            Ok(ParsedRelayedDelta::OriginAskClose(Box::new(
                ParsedOriginAskClose {
                    conv: None,
                    ask_msg: None,
                    cid,
                    emp_id: emp,
                    ask_id,
                    answer: ans,
                },
            )))
        }
        _ => Err(OfficeError::Invalid("relayed delta missing MessageRecord")),
    }
}

fn assemble_parsed_relayed_delta(
    origin_office: &str,
    hq_office_id: &str,
    is_ask_recovery: bool,
    parsed_conv: Option<(Id, crate::messaging::records::ConversationRecord)>,
    parsed_msg: Option<(Id, String, crate::messaging::records::MessageRecord)>,
    parsed_answer: Option<(
        Id,
        Id,
        String,
        crate::messaging::records::ParticipantAnswerFile,
    )>,
) -> Result<ParsedRelayedDelta, OfficeError> {
    let Some((msg_cid, msg_id, msg)) = parsed_msg else {
        return assemble_message_less_delta(parsed_conv, parsed_answer);
    };
    let conv = match parsed_conv {
        Some((conv_cid, c)) if conv_cid == msg_cid => Some(c),
        Some(_) => return Err(OfficeError::Invalid("conversation_id mismatch")),
        None => None,
    };
    let close_origin_ok =
        msg.origin_office_id.as_str() == origin_office || origin_office == hq_office_id;
    let Some((ans_cid, emp, ask_id, mut ans)) = parsed_answer else {
        if is_ask_recovery && msg.kind == "request" && close_origin_ok {
            return Ok(ParsedRelayedDelta::OriginAskRecovery(Box::new(
                ParsedOriginAskRecovery {
                    conv,
                    cid: msg_cid,
                    ask_msg: msg,
                },
            )));
        }
        if msg.origin_office_id.as_str() != origin_office {
            return Err(OfficeError::Invalid(
                "MessageRecord origin_office_id mismatch",
            ));
        }
        return Ok(ParsedRelayedDelta::Message(Box::new(
            ParsedRelayedMessage {
                conv,
                answer: None,
                msg,
            },
        )));
    };
    let is_origin_close_bundle = ans_cid == msg_cid
        && msg_id == ask_id
        && msg.kind == "request"
        && close_origin_ok
        && (ans.reason.as_deref() == Some("origin-close")
            || crate::messaging::asks::decode_ask_answer_file(&ans)
                .1
                .is_some());
    if is_origin_close_bundle {
        ans.reason = Some("origin-close".to_owned());
        return Ok(ParsedRelayedDelta::OriginAskClose(Box::new(
            ParsedOriginAskClose {
                conv,
                ask_msg: Some(msg),
                cid: ans_cid,
                emp_id: emp,
                ask_id,
                answer: ans,
            },
        )));
    }
    if msg.origin_office_id.as_str() != origin_office {
        return Err(OfficeError::Invalid(
            "MessageRecord origin_office_id mismatch",
        ));
    }
    let answer =
        validate_relayed_answer_binding(&msg_cid, &msg, Some((ans_cid, emp, ask_id, ans)))?;
    Ok(ParsedRelayedDelta::Message(Box::new(
        ParsedRelayedMessage { conv, answer, msg },
    )))
}

fn apply_relayed_origin_close(
    office: &Office,
    origin_office: &str,
    op_id: Id,
    ctx: PlacementContext,
    parsed: ParsedOriginAskClose,
) -> Result<RelayedMessageOutcome, OfficeError> {
    let ParsedOriginAskClose {
        mut conv,
        mut ask_msg,
        cid,
        emp_id,
        ask_id,
        answer,
    } = parsed;
    let mut used_staged = false;
    if ask_msg.is_none() {
        if let Some(staged) = load_staged_close_recovery(office, origin_office, &ask_id) {
            if conv.is_none() {
                conv = staged.conv;
            }
            ask_msg = Some(staged.ask_msg);
            used_staged = true;
        }
    }
    let reply = office
        .store
        .run(ApplyRelayedOriginCloseOp {
            operation_id: op_id,
            here: office.identity.clone(),
            owner: office.owner.clone(),
            ctx,
            origin_office: origin_office.to_owned(),
            incoming_conv: conv,
            incoming_ask: ask_msg,
            cid,
            emp_id,
            ask_id: ask_id.clone(),
            answer,
        })
        .map_err(storage)?;
    match reply.outcome {
        Outcome::Completed => {
            if used_staged {
                remove_staged_close_recovery(office, origin_office, &ask_id);
            }
            if reply.bytes.is_empty() {
                Ok(RelayedMessageOutcome::default())
            } else {
                serde_json::from_slice(&reply.bytes)
                    .map_err(|_| OfficeError::Invalid("corrupt relayed origin ask close reply"))
            }
        }
        Outcome::Rejected => Err(OfficeError::Invalid(
            "relayed origin ask close rejected by validation",
        )),
    }
}

struct ApplyRelayedHandoffAcceptOp {
    operation_id: Id,
    owner: Id,
    ctx: PlacementContext,
    origin_office: String,
    cid: Id,
    emp_id: Id,
    message_id: String,
    answer: crate::messaging::records::ParticipantAnswerFile,
}

impl crate::store::open::StoreOp for ApplyRelayedHandoffAcceptOp {
    type Out = Result<org_store::Reply, org_store::StoreError>;

    fn run<E: crate::store::open::OfficeEngine>(self, store: &org_store::Store<E>) -> Self::Out {
        let op = org_store::Operation {
            id: self.operation_id.clone(),
            kind: "replication.apply_handoff_accept",
            actor: Actor::System,
            fingerprint: org_store::Sha256::of_parts(&[
                self.cid.as_str().as_bytes(),
                b"\0",
                self.message_id.as_bytes(),
                b"\0",
                self.emp_id.as_str().as_bytes(),
            ]),
        };
        store.submit(Some(op), false, move |uow| {
            decide_apply_relayed_handoff_accept(uow.view(), &self)
        })
    }
}

fn validate_relayed_handoff_accept_target(
    view: &impl crate::messaging::ports::MessagingRead,
    req: &ApplyRelayedHandoffAcceptOp,
    employees: &[crate::organization::records::Employee],
    conv: &crate::messaging::records::ConversationRecord,
    active_members: &[Id],
) -> Result<(), org_store::Refusal> {
    use crate::messaging::features::{build_mention_candidates, collect_recipient_reasons};
    use crate::messaging::mentions::extract_mentions;
    use org_store::Refusal;

    if !req
        .answer
        .accepted_at
        .as_deref()
        .is_some_and(|ts| !ts.trim().is_empty())
    {
        return Err(Refusal::invalid(
            "relayed handoff accept missing accepted_at",
        ));
    }
    if let Some(reason) = req.answer.reason.as_deref() {
        if org_proto::msg::json_string_len(reason) > org_proto::msg::MAX_NOTE_BYTES {
            return Err(Refusal::invalid(
                "relayed handoff accept reason exceeds size limit",
            ));
        }
    }
    let Some(msg) = view
        .message(&req.message_id)
        .map_err(|_| Refusal::invalid("read handoff message"))?
    else {
        return Err(Refusal::invalid(
            "handoff message not present on this office",
        ));
    };
    if msg.conversation_id != req.cid || msg.kind != "handoff" {
        return Err(Refusal::invalid("relayed handoff accept target mismatch"));
    }
    if msg.sender_employee_id.as_ref() == Some(&req.emp_id) {
        return Err(Refusal::invalid("handoff sender cannot accept own handoff"));
    }
    let sender_id = msg.sender_employee_id.as_ref().unwrap_or(&req.owner);
    let candidates = build_mention_candidates(employees, active_members, &req.owner);
    let scan = extract_mentions(&msg.body_markdown, &candidates);
    let reasons = collect_recipient_reasons(
        view,
        conv,
        sender_id,
        active_members,
        &scan.resolved,
        msg.thread_root_id.as_deref(),
        &msg.kind,
        &msg.metadata,
    )?;
    if !reasons.contains_key(&req.emp_id) {
        return Err(Refusal::invalid(
            "relayed handoff accepter is not an addressed recipient",
        ));
    }
    Ok(())
}

fn decide_apply_relayed_handoff_accept(
    view: &impl crate::messaging::ports::MessagingRead,
    req: &ApplyRelayedHandoffAcceptOp,
) -> Result<org_store::Change, org_store::Refusal> {
    use crate::messaging::records::{encode_json, participant_answer_path};
    use crate::organization::records::EmployeeStatus;
    use org_store::{Ack, Change, Refusal};

    let employees = view
        .employees()
        .map_err(|_| Refusal::invalid("read employees"))?;
    let Some(emp) = employees
        .iter()
        .find(|e| e.id == req.emp_id && e.status == EmployeeStatus::Active)
    else {
        return Err(Refusal::invalid("relayed handoff accepter is not active"));
    };
    // Enforce origin-office placement for ALL employees (Agents at their placed office; Persons
    // strictly at HQ per `doc-90`) so a satellite can never forge a Person/owner acceptance.
    let placed = req
        .ctx
        .employee_offices
        .get(emp.id.as_str())
        .map(String::as_str)
        .unwrap_or(req.ctx.hq_office_id.as_str());
    if placed != req.origin_office.as_str() {
        return Err(Refusal::invalid(
            "relayed handoff accepter is not placed at authenticated origin office",
        ));
    }
    let Some(conv) = view
        .conversation(&req.cid)
        .map_err(|_| Refusal::invalid("read conversation"))?
    else {
        return Err(Refusal::invalid("conversation missing for handoff accept"));
    };
    let active_members =
        crate::messaging::features::effective_auto_members(view, &conv, &req.owner)
            .map_err(|_| Refusal::invalid("resolve conversation members"))?;
    if !active_members.contains(&req.emp_id) && req.emp_id != req.owner {
        return Err(Refusal::invalid(
            "relayed handoff accepter is not an active member",
        ));
    }
    validate_relayed_handoff_accept_target(view, req, &employees, &conv, &active_members)?;
    let outcome = RelayedMessageOutcome {
        conversation_id: req.cid.as_str().to_owned(),
        sender_id: req.emp_id.as_str().to_owned(),
        handoff_accepted: true,
        ..Default::default()
    };
    let reply = serde_json::to_vec(&outcome).unwrap_or_default();
    if let Some(existing) = view
        .answer_for_message(&req.message_id)
        .map_err(|_| Refusal::invalid("read existing handoff accept"))?
    {
        if existing.accepted_by.as_ref() == Some(&req.emp_id)
            || !is_canonical_relayed_ask_winner(Some(&existing), &req.emp_id, &req.answer)
        {
            return Ok(Change::new(Ack::Durable).reply(reply));
        }
    }
    let path = participant_answer_path(&req.cid, &req.emp_id, &req.message_id);
    Ok(Change::new(Ack::Durable)
        .put(path, encode_json(&req.answer))
        .reply(reply))
}

fn apply_relayed_handoff_accept(
    office: &Office,
    origin_office: &str,
    op_id: Id,
    ctx: PlacementContext,
    parsed: ParsedHandoffAccept,
) -> Result<RelayedMessageOutcome, OfficeError> {
    let reply = office
        .store
        .run(ApplyRelayedHandoffAcceptOp {
            operation_id: op_id,
            owner: office.owner.clone(),
            ctx,
            origin_office: origin_office.to_owned(),
            cid: parsed.cid,
            emp_id: parsed.emp_id,
            message_id: parsed.message_id,
            answer: parsed.answer,
        })
        .map_err(storage)?;
    match reply.outcome {
        Outcome::Completed => {
            if reply.bytes.is_empty() {
                Ok(RelayedMessageOutcome::default())
            } else {
                serde_json::from_slice(&reply.bytes)
                    .map_err(|_| OfficeError::Invalid("corrupt relayed handoff accept reply"))
            }
        }
        Outcome::Rejected => Err(OfficeError::Invalid(
            "relayed handoff accept rejected by validation",
        )),
    }
}

fn apply_validated_relayed_message(
    office: &Office,
    origin_office: &str,
    message_id: &str,
    operation_id: &str,
    files: Vec<(String, Option<Vec<u8>>)>,
) -> Result<RelayedMessageOutcome, OfficeError> {
    let ctx = load_placement_context(office)?;
    let parsed = parse_validated_relayed_delta(
        &office.identity.organization_id,
        origin_office,
        &ctx.hq_office_id,
        message_id.starts_with("close-ask-"),
        files,
    )?;
    let op_id = Id::parse(operation_id)
        .map_err(|_| OfficeError::Invalid("invalid replication operation_id"))?;
    office.replication.record_replicated_op_id(operation_id)?;
    let ParsedRelayedMessage { conv, answer, msg } = match parsed {
        ParsedRelayedDelta::ConversationOnly(conv) => {
            let reply = office
                .store
                .run(ApplyRelayedConversationOnlyOp {
                    operation_id: op_id,
                    origin_office: origin_office.to_owned(),
                    ctx,
                    conv: *conv,
                })
                .map_err(storage)?;
            return match reply.outcome {
                Outcome::Completed => Ok(RelayedMessageOutcome::default()),
                Outcome::Rejected => Err(OfficeError::Invalid(
                    "relayed conversation rejected by validation",
                )),
            };
        }
        ParsedRelayedDelta::OriginAskRecovery(boxed) => {
            let ParsedOriginAskRecovery { conv, cid, ask_msg } = *boxed;
            let already_present = office
                .store
                .run(CheckAskRecoveryEntitlementOp {
                    here: office.identity.clone(),
                    owner: office.owner.clone(),
                    ctx,
                    origin_office: origin_office.to_owned(),
                    incoming_conv: conv.clone(),
                    cid,
                    ask_msg: ask_msg.clone(),
                })
                .map_err(storage)?
                .map_err(|_| OfficeError::Invalid("relayed ask recovery rejected by validation"))?;
            if !already_present {
                let staged = StagedAskRecovery { conv, ask_msg };
                stage_close_recovery(office, origin_office, &staged.ask_msg.id, &staged)?;
            }
            return Ok(RelayedMessageOutcome::default());
        }
        ParsedRelayedDelta::OriginAskClose(boxed) => {
            return apply_relayed_origin_close(office, origin_office, op_id, ctx, *boxed);
        }
        ParsedRelayedDelta::HandoffAccept(boxed) => {
            return apply_relayed_handoff_accept(office, origin_office, op_id, ctx, *boxed);
        }
        ParsedRelayedDelta::Message(boxed) => *boxed,
    };
    let now_ms = office.monotonic_now_ms();
    if message_expires_at_ms(&msg).is_some_and(|exp| exp <= now_ms) {
        return Err(OfficeError::Invalid("relayed message already expired"));
    }
    let delivered_msg_for_outcome = msg.clone();
    let reply = office
        .store
        .run(ApplyRelayedMessageOp {
            operation_id: op_id,
            here: office.identity.clone(),
            owner: office.owner.clone(),
            origin_office: origin_office.to_owned(),
            now_ms,
            ctx,
            incoming_conv: conv,
            incoming_answer: answer,
            msg,
        })
        .map_err(storage)?;
    match reply.outcome {
        Outcome::Completed => {
            let mut outcome: RelayedMessageOutcome = serde_json::from_slice(&reply.bytes)
                .map_err(|_| OfficeError::Invalid("corrupt relayed message commit reply"))?;
            if outcome.inserted {
                outcome.delivered_message = Some(delivered_msg_for_outcome);
            }
            Ok(outcome)
        }
        Outcome::Rejected => Err(OfficeError::Invalid(
            "relayed message rejected by native messaging validation",
        )),
    }
}

fn is_hq_org_event_version(path: &DurablePath, bytes: Option<&[u8]>) -> bool {
    if path.as_str().starts_with("state/") {
        return true;
    }
    matches!(
        crate::messaging::records::kind_of(path),
        Some(crate::messaging::records::Kind::Conversation(_))
    ) && bytes.is_some_and(|raw| {
        serde_json::from_slice::<crate::messaging::records::ConversationRecord>(raw)
            .is_ok_and(|c| c.kind == org_proto::chat::ConversationKind::Channel)
    })
}

fn walk_state_files(durable: &Path) -> Result<Vec<(String, Vec<u8>)>, OfficeError> {
    let files = Files::open(durable).map_err(OfficeError::storage)?;
    let state_prefix = DurablePath::parse("state").map_err(storage)?;
    let mut entries = Vec::new();
    for path in files.walk(&state_prefix).map_err(OfficeError::storage)? {
        let s = path.as_str();
        if !s.starts_with("state/") {
            continue;
        }
        if let Some(bytes) = files.read(&path).map_err(OfficeError::storage)? {
            entries.push((s.to_owned(), bytes.to_vec()));
        }
    }
    append_channel_snapshot_files(&files, &mut entries)?;
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(entries)
}

/// Appends HQ's authoritative `ConversationKind::Channel` definitions to snapshots so newly
/// enrolled or long-offline satellites immediately see all current channels (`TASK-52` P10-#7 / P25-#1).
fn append_channel_snapshot_files(
    files: &Files,
    entries: &mut Vec<(String, Vec<u8>)>,
) -> Result<(), OfficeError> {
    let msg_prefix = DurablePath::parse("messaging/conversations").map_err(storage)?;
    let msg_paths = files.walk(&msg_prefix).map_err(OfficeError::storage)?;
    for path in msg_paths {
        let is_conv = matches!(
            crate::messaging::records::kind_of(&path),
            Some(crate::messaging::records::Kind::Conversation(_))
        );
        if !is_conv {
            continue;
        }
        let Some(bytes) = files.read(&path).map_err(OfficeError::storage)? else {
            return Err(OfficeError::Invalid("snapshot file vanished during export"));
        };
        let conv: crate::messaging::records::ConversationRecord = serde_json::from_slice(&bytes)
            .map_err(|_| OfficeError::Invalid("malformed conversation.json in snapshot"))?;
        if conv.kind == org_proto::chat::ConversationKind::Channel {
            entries.push((path.as_str().to_owned(), bytes.to_vec()));
        }
    }
    Ok(())
}

fn validate_work_file_paths(files: &[(String, Option<Vec<u8>>)]) -> Result<(), OfficeError> {
    for (path, bytes) in files {
        let dp = DurablePath::parse(path)
            .map_err(|_| OfficeError::Invalid("invalid replicated work path"))?;
        let task_kind = crate::tasks::records::kind_of(&dp);
        let msg_kind = crate::messaging::records::kind_of(&dp);
        if task_kind.is_none() && msg_kind.is_none() {
            return Err(OfficeError::Invalid(
                "satellite work change may only touch tasks or messaging records",
            ));
        }
        if bytes.is_none() && is_immutable_state_path(path) {
            return Err(OfficeError::Invalid(
                "cannot delete immutable task or messaging history file",
            ));
        }
    }
    Ok(())
}

struct ReadOfficeCodesOp;

impl crate::store::open::ReadOp for ReadOfficeCodesOp {
    type Out = Vec<crate::organization::records::OfficeCode>;

    fn read<S: crate::tasks::ports::TasksRead>(
        self,
        snapshot: &S,
    ) -> Result<Self::Out, org_store::StoreError> {
        crate::organization::ports::OrganizationRead::office_codes(snapshot)
            .map_err(org_store::StoreError::Engine)
    }
}

fn validate_incoming_task_record(
    office: &Office,
    tid: &Id,
    opt_bytes: Option<&[u8]>,
    expected_record_id: &str,
    origin_codes: &std::collections::BTreeSet<String>,
    team_ids: &std::collections::BTreeSet<&str>,
) -> Result<String, OfficeError> {
    let Some(bytes) = opt_bytes else {
        return Err(OfficeError::Invalid(
            "satellite work change cannot delete task records",
        ));
    };
    let rec: crate::tasks::records::TaskRecord = serde_json::from_slice(bytes)
        .map_err(|_| OfficeError::Invalid("malformed TaskRecord in satellite work"))?;
    if rec.id != *tid {
        return Err(OfficeError::Invalid("TaskRecord id does not match path"));
    }
    if org_proto::is_uuid_v7(expected_record_id) && rec.id.as_str() != expected_record_id {
        return Err(OfficeError::Invalid(
            "TaskRecord id does not match WorkChange record_id",
        ));
    }
    if !origin_codes.contains(&rec.office_code) {
        return Err(OfficeError::Invalid(
            "satellite cannot write task belonging to another office",
        ));
    }
    if !team_ids.contains(rec.team_id.as_str()) {
        return Err(OfficeError::Invalid(
            "TaskRecord references unknown organization team",
        ));
    }
    let actor_id = rec
        .meta
        .updated_by_id
        .as_ref()
        .or(rec.meta.created_by_id.as_ref())
        .ok_or(OfficeError::Invalid(
            "TaskRecord missing writer attribution",
        ))?;
    let actor_active = office
        .load_employee(actor_id.as_str())?
        .is_some_and(|e| e.status == crate::organization::records::EmployeeStatus::Active);
    if !actor_active {
        return Err(OfficeError::Invalid(
            "TaskRecord writer is not an active organization member",
        ));
    }
    if let Ok(Ok(existing)) = office.task_inspect(rec.id.as_str()) {
        if !origin_codes.contains(&existing.task.office_code) {
            return Err(OfficeError::Invalid(
                "satellite cannot overwrite task owned by another office",
            ));
        }
        if rec.meta.revision < existing.task.revision {
            return Err(OfficeError::Invalid(
                "stale TaskRecord revision rejected by HQ",
            ));
        }
    }
    Ok(rec.office_code)
}

fn validate_typed_work_files_against_hq(
    office: &Office,
    origin_office: &str,
    expected_record_id: &str,
    files: &[(String, Option<Vec<u8>>)],
) -> Result<(), OfficeError> {
    // Caller (`ingest_validated_satellite_work`) already holds `office.federation_io_lock`.
    if super::invitations::is_satellite_office_removed_locked(office, origin_office)? {
        return Err(OfficeError::Invalid(
            "removed satellite office no longer holds task write authority",
        ));
    }
    let snap = office.org_snapshot()?;
    let team_ids: std::collections::BTreeSet<&str> =
        snap.teams.iter().map(|t| t.id.as_str()).collect();
    let (codes, _) = office.store.read(ReadOfficeCodesOp).map_err(storage)?;
    let origin_codes: std::collections::BTreeSet<String> = codes
        .into_iter()
        .filter(|c| c.office_id.as_str() == origin_office)
        .map(|c| c.code)
        .collect();
    if origin_codes.is_empty() {
        return Err(OfficeError::Invalid(
            "origin satellite office has no assigned office code on HQ",
        ));
    }
    let mut incoming_task_codes: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    let mut deferred_sidecars: Vec<Id> = Vec::new();
    // Pass 1: validate counters and Task records before sidecars regardless of file order
    // (e.g. `links.json` sorts before `task.json` when creating a task with `depends_on`, `TASK-52` P8-#2).
    for (path, opt_bytes) in files {
        let dp = DurablePath::parse(path).map_err(|_| OfficeError::Invalid("invalid path"))?;
        let Some(kind) = crate::tasks::records::kind_of(&dp) else {
            return Err(OfficeError::Invalid(
                "satellite work change must only contain task records",
            ));
        };
        match kind {
            crate::tasks::records::Kind::OfficeTaskCounter(code) => {
                if opt_bytes.is_none() || !origin_codes.contains(&code) {
                    return Err(OfficeError::Invalid(
                        "satellite may only update its own office task counter",
                    ));
                }
            }
            crate::tasks::records::Kind::Task(tid) => {
                let code = validate_incoming_task_record(
                    office,
                    &tid,
                    opt_bytes.as_deref(),
                    expected_record_id,
                    &origin_codes,
                    &team_ids,
                )?;
                incoming_task_codes.insert(tid.as_str().to_owned(), code);
            }
            crate::tasks::records::Kind::Checklist(tid)
            | crate::tasks::records::Kind::Comment(tid, _)
            | crate::tasks::records::Kind::Watchers(tid)
            | crate::tasks::records::Kind::Links(tid)
            | crate::tasks::records::Kind::Event(tid, _) => {
                deferred_sidecars.push(tid);
            }
        }
    }
    // Pass 2: validate task sidecars now that every incoming `Task` record has been registered.
    for tid in deferred_sidecars {
        if org_proto::is_uuid_v7(expected_record_id) && tid.as_str() != expected_record_id {
            return Err(OfficeError::Invalid(
                "task sidecar does not match WorkChange record_id",
            ));
        }
        if !incoming_task_codes.contains_key(tid.as_str()) {
            let existing = office
                .task_inspect(tid.as_str())
                .ok()
                .and_then(|r| r.ok())
                .ok_or(OfficeError::Invalid(
                    "task sidecar references unknown task on HQ",
                ))?;
            if !origin_codes.contains(&existing.task.office_code) {
                return Err(OfficeError::Invalid(
                    "satellite cannot mutate sidecar of another office's task",
                ));
            }
        }
    }
    Ok(())
}

/// Splits a batch of encoded `(path, Option<b64>)` entries into one or more `OrgDeltaPayload`
/// JSON byte slices each strictly within `org_sync::organization::MAX_PAYLOAD_BYTES` (`TASK-52` H-12).
fn chunk_org_delta_payloads(
    files: Vec<(String, Option<String>)>,
) -> Result<Vec<std::sync::Arc<[u8]>>, OfficeError> {
    let max_bytes = org_sync::organization::MAX_PAYLOAD_BYTES;
    let mut chunks = Vec::new();
    let mut current: Vec<(String, Option<String>)> = Vec::new();
    for item in files {
        current.push(item);
        let encoded = serde_json::to_vec(&OrgDeltaPayload {
            v: 1,
            files: current.clone(),
        })
        .map_err(OfficeError::storage)?;
        if encoded.len() <= max_bytes {
            continue;
        }
        let popped = current.pop().expect("just pushed");
        if !current.is_empty() {
            let flush = serde_json::to_vec(&OrgDeltaPayload {
                v: 1,
                files: std::mem::take(&mut current),
            })
            .map_err(OfficeError::storage)?;
            chunks.push(std::sync::Arc::from(flush));
        }
        let single = serde_json::to_vec(&OrgDeltaPayload {
            v: 1,
            files: vec![popped.clone()],
        })
        .map_err(OfficeError::storage)?;
        if single.len() > max_bytes {
            return Err(OfficeError::Invalid(
                "single replicated file exceeds org-sync event payload limit",
            ));
        }
        current.push(popped);
    }
    if !current.is_empty() {
        let flush = serde_json::to_vec(&OrgDeltaPayload {
            v: 1,
            files: current,
        })
        .map_err(OfficeError::storage)?;
        chunks.push(std::sync::Arc::from(flush));
    }
    Ok(chunks)
}

fn sync_satellite_work_outbox_locked(office: &Office) -> Result<usize, OfficeError> {
    let wm = office.register_replication_consumer("org-sync-work-outbox")?;
    let confirmed = office.store.status().confirmed;
    let entries = office.store.entries_after_contiguous(wm).map_err(storage)?;
    if entries.is_empty() {
        return Ok(0);
    }
    let incarnation = office.identity.journal_incarnation.as_str().to_owned();
    let codec = org_sync_proto::Codec::new("org.native.changes", 1)
        .map_err(|_| OfficeError::Invalid("codec"))?;
    let mut enqueued = 0usize;
    let mut last_ordinal = wm;
    for entry in entries {
        let ord = entry.ordinal();
        if ord > confirmed {
            break;
        }
        let from_replication = is_replicated_journal_entry(&entry, office.replication.as_ref());
        if !from_replication {
            let files: Vec<(String, Option<String>)> = entry
                .versions()
                .filter(|fv| crate::tasks::records::kind_of(fv.path).is_some())
                .map(|fv| {
                    (
                        fv.path.as_str().to_owned(),
                        fv.bytes.map(|b| STANDARD.encode(b)),
                    )
                })
                .collect();
            if !files.is_empty() {
                let record_id =
                    extract_primary_task_id(&files).unwrap_or_else(|| format!("task-ord-{ord}"));
                let payloads = chunk_org_delta_payloads(files)?;
                let created_ms = office.monotonic_now_ms();
                enqueued += office.replication.publish_satellite_work_entries(
                    codec.clone(),
                    &record_id,
                    payloads,
                    &incarnation,
                    ord,
                    created_ms,
                )?;
            }
        }
        last_ordinal = last_ordinal.max(ord);
    }
    if last_ordinal > wm {
        office.advance_replication_consumer("org-sync-work-outbox", last_ordinal)?;
    }
    Ok(enqueued)
}

/// Canonical dependency classification for replicated snapshot files derived directly from
/// owning domain record parsers (`TASK-52` P8-#5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotPathClass {
    OrgCoreStructure,
    OrgCoreEmployee,
    ProvidersAndHires,
    ModelProfilesAndProjects,
    DependentExecutionDocsAndBudgets,
    WorkCore,
    WorkSidecar,
}

/// Clears employee attribution on typed canonical `state/repositories.yaml` records for the
/// neutral pre-pass of > 8 MiB org-core snapshots (`TASK-52` P11-#1).
pub fn strip_org_core_employee_refs_typed(path: &str, bytes: &[u8]) -> Option<Vec<u8>> {
    if path != "state/repositories.yaml" {
        return None;
    }
    let mut repos = crate::organization::records::decode_repositories(bytes).ok()?;
    for repo in &mut repos {
        repo.meta.created_by = crate::organization::records::By::System;
        repo.meta.updated_by = crate::organization::records::By::System;
    }
    Some(crate::organization::records::encode_repositories(&repos))
}

/// Classifies a replicated `state/` path strictly using canonical record parsers so large
/// snapshots never misclassify files such as `execution.md` or `processes/*.md` (`TASK-52` P8-#5).
pub fn classify_replicated_snapshot_path(path: &str) -> SnapshotPathClass {
    let Ok(dp) = DurablePath::parse(path) else {
        return SnapshotPathClass::DependentExecutionDocsAndBudgets;
    };
    if let Some(tk) = crate::tasks::records::kind_of(&dp) {
        return match tk {
            crate::tasks::records::Kind::OfficeTaskCounter(_)
            | crate::tasks::records::Kind::Task(_) => SnapshotPathClass::WorkCore,
            _ => SnapshotPathClass::WorkSidecar,
        };
    }
    if crate::processes::records::parse_path(&dp).is_some() {
        return SnapshotPathClass::DependentExecutionDocsAndBudgets;
    }
    if crate::projects::records::kind_of(&dp).is_some() {
        return SnapshotPathClass::ModelProfilesAndProjects;
    }
    if let Some(ok) = crate::organization::records::kind_of(&dp) {
        use crate::organization::records::Kind as OrgKind;
        return match ok {
            OrgKind::Employee(_, _, _) => SnapshotPathClass::OrgCoreEmployee,
            OrgKind::ProviderConnection(_) | OrgKind::HireRequest(_, _) => {
                SnapshotPathClass::ProvidersAndHires
            }
            OrgKind::ModelProfile(_) => SnapshotPathClass::ModelProfilesAndProjects,
            OrgKind::AgentExecution(_, _) | OrgKind::WatchingModel => {
                SnapshotPathClass::DependentExecutionDocsAndBudgets
            }
            _ => SnapshotPathClass::OrgCoreStructure,
        };
    }
    if matches!(
        crate::messaging::records::kind_of(&dp),
        Some(crate::messaging::records::Kind::Conversation(_))
    ) || path.starts_with("state/budgets/")
    {
        return SnapshotPathClass::DependentExecutionDocsAndBudgets;
    }
    SnapshotPathClass::OrgCoreStructure
}

fn is_replicated_op_kind(kind: &str) -> bool {
    matches!(
        kind,
        "organization.apply_replicated_files"
            | "replication.apply_message"
            | "replication.apply_conversation"
            | "replication.apply_origin_close"
            | "replication.apply_handoff_accept"
    )
}

/// Determines provenance directly from durable journal operation kind (with bounded-ring fallback),
/// preventing long catch-ups (>2,048 ops) from ever re-exporting replicated HQ work (`TASK-52` P8-#3).
fn is_replicated_journal_entry(
    entry: &org_store::Entry,
    replication: &dyn crate::replication::ReplicationStore,
) -> bool {
    entry.header.operation.as_ref().is_some_and(|op| {
        is_replicated_op_kind(op.kind.as_str())
            || replication
                .is_replicated_op_id(op.id.as_str())
                .unwrap_or(false)
    })
}

fn should_skip_echoed_counter(
    durable_files: &Files,
    dp: &DurablePath,
    incoming_bytes: Option<&[u8]>,
) -> bool {
    let Some(in_raw) = incoming_bytes else {
        return false;
    };
    let Ok(in_val) = serde_json::from_slice::<serde_json::Value>(in_raw) else {
        return false;
    };
    let Some(in_num) = in_val
        .get("last_number")
        .and_then(serde_json::Value::as_u64)
    else {
        return false;
    };
    let Ok(Some(local_raw)) = durable_files.read(dp) else {
        return false;
    };
    serde_json::from_slice::<serde_json::Value>(&local_raw)
        .ok()
        .and_then(|v| v.get("last_number").and_then(serde_json::Value::as_u64))
        .is_some_and(|local_num| local_num >= in_num)
}

fn should_skip_echoed_task(
    office: &Office,
    local_codes: &std::collections::BTreeSet<String>,
    tid: &Id,
    incoming_bytes: Option<&[u8]>,
) -> bool {
    let Some(in_raw) = incoming_bytes else {
        return false;
    };
    let Ok(in_rec) = serde_json::from_slice::<crate::tasks::records::TaskRecord>(in_raw) else {
        return false;
    };
    if !local_codes.contains(&in_rec.office_code) {
        return false;
    }
    office
        .task_inspect(tid.as_str())
        .ok()
        .and_then(|r| r.ok())
        .is_some_and(|local| local.task.revision >= in_rec.meta.revision)
}

fn extract_primary_task_id(files: &[(String, Option<String>)]) -> Option<String> {
    for (path, _) in files {
        if let Ok(dp) = DurablePath::parse(path) {
            if let Some(crate::tasks::records::Kind::Task(id)) = crate::tasks::records::kind_of(&dp)
            {
                return Some(id.as_str().to_owned());
            }
        }
    }
    for (path, _) in files {
        if let Ok(dp) = DurablePath::parse(path) {
            if let Some(
                crate::tasks::records::Kind::Checklist(id)
                | crate::tasks::records::Kind::Watchers(id)
                | crate::tasks::records::Kind::Links(id)
                | crate::tasks::records::Kind::Comment(id, _)
                | crate::tasks::records::Kind::Event(id, _),
            ) = crate::tasks::records::kind_of(&dp)
            {
                return Some(id.as_str().to_owned());
            }
        }
    }
    None
}

pub(crate) fn reconcile_satellite_replication_on_open(
    replication: &dyn crate::replication::ReplicationStore,
    hq_office_id: &str,
    ca_digest: Digest,
    epoch: u64,
) -> Result<(), OfficeError> {
    let cp = replication.org_checkpoint()?;
    let pending_snapshot = replication
        .sync_status()
        .is_ok_and(|s| s.initial_snapshot_required);
    if cp.hq_office.as_str() != hq_office_id
        || cp.ca != ca_digest
        || cp.position.epoch.get() < epoch
    {
        if epoch < cp.highest_epoch.get() {
            return Err(OfficeError::Invalid(
                "satellite identity epoch regressed below replication checkpoint",
            ));
        }
        // If re-enrollment already persisted replacement HQ/CA pins (`initial_snapshot_required`)
        // and crashed before swapping TLS credentials at the same epoch, preserve the committed
        // replacement checkpoint pins so the old HQ cannot be re-trusted on restart (`TASK-52` P10-#2).
        if pending_snapshot && epoch == cp.highest_epoch.get() {
            return Ok(());
        }
        let new_hq =
            OfficeId::new(hq_office_id).map_err(|_| OfficeError::Invalid("hq_office_id"))?;
        let new_ep = Epoch::new(epoch.max(1)).map_err(|_| OfficeError::Invalid("epoch"))?;
        replication.reset_for_re_enrollment(new_hq, ca_digest, new_ep)?;
    }
    Ok(())
}

pub(crate) fn read_ca_digest_for_open(
    durable: &Path,
    info: &OfficeInfo,
    hq_office_id: &str,
) -> Digest {
    if info.role == OfficeRole::Satellite {
        let atomic_path = tls_dir(durable, &info.office_id).join("federation-identity.v1.json");
        if let Ok(bytes) = fs::read(&atomic_path) {
            if let Ok(parsed) = serde_json::from_slice::<AtomicSatelliteIdentityFile>(&bytes) {
                if let Ok(ca) = STANDARD.decode(&parsed.ca_cert_der_b64) {
                    return Digest::from_bytes(cert_fingerprint_sha256(&ca));
                }
            }
        }
        let ca_path = tls_dir(durable, &info.office_id).join("org-ca.v1.der");
        if let Ok(bytes) = fs::read(ca_path) {
            return Digest::from_bytes(cert_fingerprint_sha256(&bytes));
        }
        return Digest::ZERO;
    }
    let ca_path = tls_dir(durable, hq_office_id).join("org-ca.v1.der");
    if let Ok(bytes) = fs::read(ca_path) {
        return Digest::from_bytes(cert_fingerprint_sha256(&bytes));
    }
    Digest::ZERO
}

#[cfg(test)]
mod replicated_kind_tests {
    use super::is_replicated_op_kind;

    #[test]
    fn replicated_op_kinds_include_handoff_accept_without_ring_fallback() {
        for kind in [
            "organization.apply_replicated_files",
            "replication.apply_message",
            "replication.apply_conversation",
            "replication.apply_origin_close",
            "replication.apply_handoff_accept",
        ] {
            assert!(
                is_replicated_op_kind(kind),
                "missing replicated kind {kind}"
            );
        }
        assert!(!is_replicated_op_kind("messaging.accept_handoff"));
        assert!(!is_replicated_op_kind("messaging.send"));
    }
}
