//! Satellite provisioning on disk and D25 one-command re-enrollment onto a replacement HQ.

use std::path::PathBuf;
use std::sync::atomic::AtomicU64;

use org_pki::{cert::cert_fingerprint_sha256, Zeroizing};
use org_store::{journal_dir, AnchorFacts, Id, Opening};
use org_sync_proto::{Digest, Epoch, OfficeId};

use crate::office::{
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
    private_fs::{private_dir, tighten_dir},
    store::open::open_store,
};

use super::identity::{
    persist_satellite_tls, read_intent, validate_satellite_cert_and_key, write_intent,
    SatelliteFederationConfig, SatelliteJoinIntent, INTENT_VERSION,
};

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

    #[cfg(any(test, feature = "testkit"))]
    pub fn provision_satellite_with(
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

    #[cfg(not(any(test, feature = "testkit")))]
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
    let companions = open_companion_stores(crate::office::CompanionOpenSpec {
        engine: options.engine,
        backend: options.backend.as_ref(),
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
        replicated_owner: std::sync::RwLock::new(None),
        creation,
        creation_command: intent.operation,
        secrets_key: Some(key),
        clock: options.clock,
        clock_high_ms,
        runtime: crate::messaging::runtime::OfficeRuntime::new(),
        private_notes: companions.private_notes,
        private_search: companions.private_search,
        attachments: companions.attachments,
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
    {
        let _guard = office
            .federation_io_lock
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        super::remove_replicated_hq_owner(&office.durable, &office.info.office_id)?;
        *office
            .replicated_owner
            .write()
            .unwrap_or_else(|p| p.into_inner()) = None;
    }
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
