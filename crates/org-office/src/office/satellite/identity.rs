//! The satellite's join intent (`satellite-join.v1.json`) and its persisted TLS identity:
//! the wrapped private key, pinned Org CA, office certificate and HQ federation binding.

use std::fs;
use std::path::{Path, PathBuf};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use org_pki::{
    cert::{cert_fingerprint_sha256, verify_office_cert, OfficeCertRole},
    wrap::{self, WrapBinding, WrapPurpose},
    SigningKey, Zeroizing,
};
use org_sync_proto::Digest;
use serde::{Deserialize, Serialize};

use crate::office::{tls_dir, DataRoots, Office};
use crate::{
    error::OfficeError,
    key::Key,
    model::{OfficeInfo, OfficeRole},
    private_fs::{private_dir_synced, write_private_atomic},
};

const SATELLITE_INTENT_FILE: &str = "satellite-join.v1.json";

const INTENT_LIMIT: u64 = 64 * 1024;

pub(super) const INTENT_VERSION: u32 = 1;

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

const REPLICATED_HQ_OWNER_FILE: &str = "hq-owner.v1.json";
const REPLICATED_HQ_OWNER_LIMIT: u64 = 4 * 1024;

/// Durable HQ-authoritative owner employee ID received by a satellite on mTLS `Hello` (`TASK-141`).
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplicatedHqOwnerFile {
    v: u32,
    org_id: String,
    hq_office_id: String,
    owner_member_id: String,
}

fn replicated_hq_owner_dir(durable_root: &Path, office_id: &str) -> PathBuf {
    durable_root.join("local").join("offices").join(office_id)
}

/// The replicated HQ owner file of office `office_id`, parsed and checked for everything that
/// does not depend on which HQ the satellite is bound to: its version, organization, a UUIDv7
/// HQ office id, and an owner id that names neither office. `None` when there is none.
fn load_replicated_hq_owner(
    durable_root: &Path,
    office_id: &str,
    org_id: &str,
) -> Result<Option<ReplicatedHqOwnerFile>, OfficeError> {
    let dir = replicated_hq_owner_dir(durable_root, office_id);
    match fs::symlink_metadata(&dir) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => {
            return Err(OfficeError::Invalid(
                "satellite local office directory is not a folder",
            ));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(OfficeError::storage(e)),
    }
    let Some(bytes) =
        org_store::read_one(&dir, REPLICATED_HQ_OWNER_FILE, REPLICATED_HQ_OWNER_LIMIT)
            .map_err(OfficeError::storage)?
    else {
        return Ok(None);
    };
    let parsed: ReplicatedHqOwnerFile = serde_json::from_slice(&bytes)
        .map_err(|_| OfficeError::Invalid("malformed replicated HQ owner file"))?;
    if parsed.v != 1 {
        return Err(OfficeError::Invalid(
            "unsupported replicated HQ owner file version",
        ));
    }
    if parsed.org_id != org_id {
        return Err(OfficeError::Invalid(
            "replicated HQ owner file belongs to another organization",
        ));
    }
    if !org_proto::is_uuid_v7(&parsed.hq_office_id)
        || !org_proto::is_valid_member_id(&parsed.owner_member_id)
        || parsed.owner_member_id == office_id
        || parsed.owner_member_id == parsed.hq_office_id
        || org_store::Id::parse(&parsed.owner_member_id).is_err()
    {
        return Err(OfficeError::Invalid("invalid replicated HQ owner id"));
    }
    Ok(Some(parsed))
}

pub fn read_replicated_hq_owner(
    durable_root: &Path,
    office_id: &str,
    org_id: &str,
    hq_office_id: &str,
) -> Result<Option<org_store::Id>, OfficeError> {
    let Some(parsed) = load_replicated_hq_owner(durable_root, office_id, org_id)? else {
        return Ok(None);
    };
    if parsed.hq_office_id != hq_office_id {
        return Ok(None);
    }
    org_store::Id::parse(&parsed.owner_member_id)
        .map(Some)
        .map_err(|_| OfficeError::Invalid("invalid replicated HQ owner id"))
}

/// Checks a restored copy's replicated HQ owner file, when there is one, without knowing the
/// HQ the satellite is bound to (`restore --verify`, `TASK-104`): it must parse and validate.
pub(crate) fn check_replicated_hq_owner(
    durable_root: &Path,
    office_id: &str,
    org_id: &str,
) -> Result<(), OfficeError> {
    load_replicated_hq_owner(durable_root, office_id, org_id).map(|_| ())
}

pub fn write_replicated_hq_owner(
    durable_root: &Path,
    office_id: &str,
    org_id: &str,
    hq_office_id: &str,
    owner_member_id: &org_store::Id,
) -> Result<(), OfficeError> {
    let dir = replicated_hq_owner_dir(durable_root, office_id);
    private_dir_synced(durable_root, &dir).map_err(OfficeError::storage)?;
    let record = ReplicatedHqOwnerFile {
        v: 1,
        org_id: org_id.to_owned(),
        hq_office_id: hq_office_id.to_owned(),
        owner_member_id: owner_member_id.as_str().to_owned(),
    };
    let bytes = serde_json::to_vec_pretty(&record).map_err(OfficeError::storage)?;
    org_store::publish_one(&dir, REPLICATED_HQ_OWNER_FILE, &bytes, false)
        .map_err(OfficeError::storage)
}

pub fn remove_replicated_hq_owner(durable_root: &Path, office_id: &str) -> Result<(), OfficeError> {
    let dir = replicated_hq_owner_dir(durable_root, office_id);
    let path = dir.join(REPLICATED_HQ_OWNER_FILE);
    match fs::remove_file(&path) {
        Ok(()) => crate::private_fs::sync_dir(&dir).map_err(OfficeError::storage),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(OfficeError::storage(e)),
    }
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

pub(super) fn write_intent(
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

pub(super) fn persist_satellite_tls(
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

pub(super) fn validate_satellite_cert_and_key(
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
