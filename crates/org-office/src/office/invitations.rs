//! HQ Org-CA TLS identity, satellite office invitation lifecycle, 3-wrong-codes cancellation,
//! certificate issuance, and satellite removal / certificate revocation (`TASK-52` / `doc-67`).

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use org_pki::{
    cert::{
        build_org_ca_cert, cert_fingerprint_hex, issue_office_cert,
        issue_satellite_cert_from_pubkey, IssueOfficeCertParams, IssueSatelliteCertParams,
        OfficeCertRole, DEFAULT_OFFICE_CERT_VALIDITY_DAYS,
    },
    invitation::{
        compute_check_code, normalize_check_code, verify_exporter_proof, InvitationToken,
        NewInvitationParams,
    },
    wrap::{self, WrapBinding, WrapPurpose},
    SigningKey, Zeroizing,
};
use org_proto::{ApiError, ErrorCode};
use org_store::{Actor, Id, Outcome};
use serde::{Deserialize, Serialize};

use super::{satellite::tls_dir, storage, Office};
use crate::{
    error::OfficeError,
    model::OfficeRole,
    organization::features::offices::{RegisterSatelliteOffice, SatelliteRegistered},
    private_fs::{private_dir_synced, write_private_atomic},
    store::open::RegisterSatelliteOfficeOp,
};

const MAX_WRONG_CHECK_CODE_ATTEMPTS: u32 = 3;
const DEFAULT_INVITATION_TTL_SECS: u64 = 30 * 60;
const MAX_INVITATION_TTL_SECS: u64 = 7 * 24 * 3600;

/// HQ's active Organization CA certificate and HQ TLS server leaf certificate/key.
pub struct HqTlsIdentity {
    pub ca_cert_der: Vec<u8>,
    pub ca_fingerprint_hex: String,
    pub hq_cert_der: Vec<u8>,
    pub hq_key_der: Zeroizing<Vec<u8>>,
    pub epoch: u64,
}

impl std::fmt::Debug for HqTlsIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HqTlsIdentity")
            .field("ca_fingerprint_hex", &self.ca_fingerprint_hex)
            .field("epoch", &self.epoch)
            .field("hq_key_der", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvitationStatus {
    Pending,
    AwaitingApproval,
    Approved,
    Declined,
    Cancelled,
    Expired,
    Revoked,
}

/// Public summary of a satellite invitation (never includes secret token material).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvitationSummaryView {
    pub invitation_id: String,
    pub office_id: String,
    pub office_name: String,
    pub status: InvitationStatus,
    pub check_code: Option<String>,
    pub wrong_code_attempts: u32,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
    pub issued_cert_serial_hex: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateInvitationParams {
    pub office_name: String,
    pub hq_addresses: Vec<String>,
    pub ttl_secs: Option<u64>,
    /// Optional explicit satellite `office_id` (used for D25 re-enrollment of an existing satellite).
    pub existing_office_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateInvitationResult {
    pub invitation: InvitationSummaryView,
    pub token: Zeroizing<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JoinRequestParams {
    pub invitation_id: String,
    pub request_id: String,
    pub office_id: String,
    pub joining_pub_key: [u8; 65],
    pub pop_signature_der: Vec<u8>,
    pub exporter_binding: [u8; 32],
    pub exporter_proof_der: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JoinRequestAccepted {
    Pending {
        invitation_id: String,
        office_id: String,
        check_code: String,
    },
    AlreadyApproved {
        cert_der: Vec<u8>,
        ca_cert_der: Vec<u8>,
        hq_office_id: String,
        epoch: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovedSatelliteCert {
    pub invitation: InvitationSummaryView,
    pub cert_der: Vec<u8>,
    pub ca_cert_der: Vec<u8>,
    pub serial_hex: String,
    pub epoch: u64,
    pub registered: SatelliteRegistered,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InvitationPollStatus {
    Pending,
    Approved {
        cert_der: Vec<u8>,
        ca_cert_der: Vec<u8>,
        hq_office_id: String,
        epoch: u64,
    },
    Terminal {
        code: String,
        message: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SatelliteRemovalResult {
    pub office_id: String,
    pub revoked_serials: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct StoredInvitation {
    invitation_id: String,
    office_id: String,
    office_name: String,
    hq_addresses: Vec<String>,
    invitation_pub_key_b64: String,
    status: InvitationStatus,
    created_at_ms: u64,
    expires_at_ms: u64,
    #[serde(default)]
    request_id: Option<String>,
    joining_pub_key_b64: Option<String>,
    pop_signature_b64: Option<String>,
    check_code: Option<String>,
    wrong_code_attempts: u32,
    issued_cert_der_b64: Option<String>,
    issued_cert_serial_hex: Option<String>,
}

#[derive(Default, Clone, Serialize, Deserialize)]
struct InvitationsStateFile {
    v: u32,
    invitations: Vec<StoredInvitation>,
}

#[derive(Default, Clone, Serialize, Deserialize)]
struct RevocationsFile {
    v: u32,
    revoked_serials: BTreeSet<String>,
    /// Authoritative set of removed satellite office IDs so HQ immediately reclaims task write
    /// authority for their codes when a satellite is removed (`TASK-52` P6-#9).
    #[serde(default)]
    removed_offices: BTreeSet<String>,
}

#[derive(Serialize, Deserialize)]
struct WrappedHqTlsFile {
    v: u32,
    epoch: u64,
    wrapped_key_b64: String,
    #[serde(default)]
    cert_der_b64: Option<String>,
    #[serde(default)]
    san_hosts: Vec<String>,
}

impl StoredInvitation {
    fn view(&self) -> InvitationSummaryView {
        InvitationSummaryView {
            invitation_id: self.invitation_id.clone(),
            office_id: self.office_id.clone(),
            office_name: self.office_name.clone(),
            status: self.status,
            check_code: self.check_code.clone(),
            wrong_code_attempts: self.wrong_code_attempts,
            created_at_ms: self.created_at_ms,
            expires_at_ms: self.expires_at_ms,
            issued_cert_serial_hex: self.issued_cert_serial_hex.clone(),
        }
    }
}

fn invitations_dir(office: &Office) -> PathBuf {
    office
        .durable
        .join("local")
        .join("offices")
        .join(&office.info.office_id)
        .join("invitations")
}

fn load_invitations(office: &Office) -> Result<InvitationsStateFile, OfficeError> {
    let path = invitations_dir(office).join("state.v1.json");
    if !path.is_file() {
        return Ok(InvitationsStateFile {
            v: 1,
            invitations: Vec::new(),
        });
    }
    let bytes = fs::read(&path).map_err(OfficeError::storage)?;
    serde_json::from_slice(&bytes)
        .map_err(|_| OfficeError::Invalid("malformed HQ invitations state"))
}

fn save_invitations(office: &Office, state: &InvitationsStateFile) -> Result<(), OfficeError> {
    let dir = invitations_dir(office);
    private_dir_synced(&office.durable, &dir).map_err(OfficeError::storage)?;
    let bytes = serde_json::to_vec_pretty(state).map_err(OfficeError::storage)?;
    write_private_atomic(&dir.join("state.v1.json"), &bytes).map_err(OfficeError::storage)
}

fn load_revocations(office: &Office) -> Result<RevocationsFile, OfficeError> {
    let _rev_guard = office
        .revocation_lock
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let dir = tls_dir(&office.durable, &office.info.office_id);
    let path = dir.join("revocations.v1.json");
    match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| OfficeError::Invalid("malformed HQ revocations file")),
        Err(err)
            if err.kind() == std::io::ErrorKind::NotFound
                && !dir.join("org-ca.v1.der").exists() =>
        {
            Ok(RevocationsFile {
                v: 1,
                revoked_serials: BTreeSet::new(),
                removed_offices: BTreeSet::new(),
            })
        }
        Err(err) => Err(OfficeError::storage(err)),
    }
}

fn save_revocations(office: &Office, rev: &RevocationsFile) -> Result<(), OfficeError> {
    let _rev_guard = office
        .revocation_lock
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let dir = tls_dir(&office.durable, &office.info.office_id);
    private_dir_synced(&office.durable, &dir).map_err(OfficeError::storage)?;
    let bytes = serde_json::to_vec_pretty(rev).map_err(OfficeError::storage)?;
    write_private_atomic(&dir.join("revocations.v1.json"), &bytes).map_err(OfficeError::storage)
}

fn try_load_existing_hq_tls(
    office: &Office,
    secret_bytes: &[u8; 32],
    dir: &std::path::Path,
    requested_hosts: &[String],
    merged_hosts: &mut Vec<String>,
) -> Result<Option<HqTlsIdentity>, OfficeError> {
    let ca_path = dir.join("org-ca.v1.der");
    let hq_cert_path = dir.join("hq-cert.v1.der");
    let hq_key_path = dir.join("hq-key.v1.json");
    if !ca_path.is_file() || !hq_key_path.is_file() {
        return Ok(None);
    }
    let ca_cert_der = fs::read(&ca_path).map_err(OfficeError::storage)?;
    let meta: WrappedHqTlsFile =
        serde_json::from_slice(&fs::read(&hq_key_path).map_err(OfficeError::storage)?)
            .map_err(|_| OfficeError::Invalid("malformed HQ TLS key metadata"))?;
    let hq_cert_der = if let Some(b64) = &meta.cert_der_b64 {
        STANDARD
            .decode(b64)
            .map_err(|_| OfficeError::Invalid("malformed HQ TLS cert base64"))?
    } else if hq_cert_path.is_file() {
        fs::read(&hq_cert_path).map_err(OfficeError::storage)?
    } else {
        Vec::new()
    };
    let covers_all = !hq_cert_der.is_empty()
        && requested_hosts
            .iter()
            .all(|h| meta.san_hosts.iter().any(|existing| existing == h));
    if covers_all {
        let wrapped = STANDARD
            .decode(&meta.wrapped_key_b64)
            .map_err(|_| OfficeError::Invalid("malformed HQ TLS key base64"))?;
        let hq_key_der = wrap::unwrap(
            secret_bytes,
            &WrapBinding {
                organization_id: &office.info.org_id,
                office_id: &office.info.office_id,
                purpose: WrapPurpose::OfficeTlsKey,
            },
            &wrapped,
        )
        .map_err(|_| OfficeError::Invalid("failed unwrapping HQ TLS key"))?;
        let ca_fingerprint_hex = cert_fingerprint_hex(&ca_cert_der);
        return Ok(Some(HqTlsIdentity {
            ca_cert_der,
            ca_fingerprint_hex,
            hq_cert_der,
            hq_key_der,
            epoch: meta.epoch,
        }));
    }
    for existing in meta.san_hosts {
        if !merged_hosts.iter().any(|h| h == &existing) {
            merged_hosts.push(existing);
        }
    }
    Ok(None)
}

fn ensure_hq_tls_identity_locked(
    office: &Office,
    san_hosts: &[String],
) -> Result<HqTlsIdentity, OfficeError> {
    if office.info.role != OfficeRole::Hq {
        return Err(OfficeError::Invalid("only HQ holds an Org CA TLS identity"));
    }
    let secrets = office
        .secrets_key
        .as_ref()
        .ok_or(OfficeError::Invalid("office secrets key required"))?;
    let dir = tls_dir(&office.durable, &office.info.office_id);
    private_dir_synced(&office.durable, &dir).map_err(OfficeError::storage)?;

    let ca_path = dir.join("org-ca.v1.der");
    let hq_cert_path = dir.join("hq-cert.v1.der");
    let hq_key_path = dir.join("hq-key.v1.json");
    let rev_path = dir.join("revocations.v1.json");
    let epoch = office.replication.org_checkpoint()?.position.epoch.get();
    let requested_hosts = extract_san_hosts(san_hosts);
    let ca_existed = ca_path.is_file();
    // Fail closed if an established Org CA is missing its revocation store (`TASK-52` H-5);
    // only initialize an empty revocation store during initial Org CA bootstrap.
    if ca_existed && !rev_path.is_file() {
        return Err(OfficeError::Invalid(
            "established Org CA is missing revocations.v1.json; refusing federation",
        ));
    }
    if !ca_existed && !rev_path.is_file() {
        save_revocations(
            office,
            &RevocationsFile {
                v: 1,
                revoked_serials: BTreeSet::new(),
                removed_offices: BTreeSet::new(),
            },
        )?;
    }

    let mut merged_hosts = requested_hosts.clone();
    if let Some(loaded) = try_load_existing_hq_tls(
        office,
        secrets.expose_bytes(),
        &dir,
        &requested_hosts,
        &mut merged_hosts,
    )? {
        return Ok(loaded);
    }

    let ca_key = unwrap_hq_ca_key(office)?;
    let now_unix = (office.monotonic_now_ms() / 1000) as i64;
    let ca_cert_der = if ca_path.is_file() {
        fs::read(&ca_path).map_err(OfficeError::storage)?
    } else {
        let built = build_org_ca_cert(&ca_key, &office.info.org_id, now_unix)
            .map_err(|_| OfficeError::Invalid("failed building Org CA certificate"))?;
        write_private_atomic(&ca_path, &built.cert_der).map_err(OfficeError::storage)?;
        built.cert_der
    };
    let hq_tls_key =
        SigningKey::generate().map_err(|_| OfficeError::Invalid("failed generating HQ TLS key"))?;
    let issued = issue_office_cert(
        &ca_key,
        &ca_cert_der,
        &IssueOfficeCertParams {
            org_id: &office.info.org_id,
            office_id: &office.info.office_id,
            role: OfficeCertRole::Hq { epoch },
            subject_key_der_pkcs8: hq_tls_key.local_der(),
            san_hosts: &merged_hosts,
            now_unix,
            validity_days: DEFAULT_OFFICE_CERT_VALIDITY_DAYS,
        },
    )
    .map_err(|_| OfficeError::Invalid("failed issuing HQ TLS certificate"))?;

    let wrapped = wrap::wrap(
        secrets.expose_bytes(),
        &WrapBinding {
            organization_id: &office.info.org_id,
            office_id: &office.info.office_id,
            purpose: WrapPurpose::OfficeTlsKey,
        },
        hq_tls_key.local_der(),
    )
    .map_err(|_| OfficeError::Invalid("failed wrapping HQ TLS key"))?;
    // Publish cert + wrapped key + SAN metadata together in ONE atomic rename so an
    // interrupted SAN rotation can never pair a new cert with an old key (`TASK-52` P3-#10).
    let key_meta = WrappedHqTlsFile {
        v: 1,
        epoch,
        wrapped_key_b64: STANDARD.encode(&wrapped),
        cert_der_b64: Some(STANDARD.encode(&issued.cert_der)),
        san_hosts: merged_hosts,
    };
    write_private_atomic(
        &hq_key_path,
        &serde_json::to_vec_pretty(&key_meta).map_err(OfficeError::storage)?,
    )
    .map_err(OfficeError::storage)?;
    let _ = write_private_atomic(&hq_cert_path, &issued.cert_der);

    Ok(HqTlsIdentity {
        ca_fingerprint_hex: cert_fingerprint_hex(&ca_cert_der),
        ca_cert_der,
        hq_cert_der: issued.cert_der,
        hq_key_der: Zeroizing::new(hq_tls_key.local_der().to_vec()),
        epoch,
    })
}

fn require_hq_owner(office: &Office, actor_member_id: &str) -> Result<(), ApiError> {
    if office.info.role != OfficeRole::Hq {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "satellite invitation management is only available on HQ",
        ));
    }
    if actor_member_id != office.owner.as_str() {
        return Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "only the organization owner may manage satellite offices",
        ));
    }
    Ok(())
}

fn unwrap_hq_ca_key(office: &Office) -> Result<SigningKey, OfficeError> {
    let secrets = office.secrets_key.as_ref().ok_or(OfficeError::Invalid(
        "office secrets key required for Org CA",
    ))?;
    let ca = office.ca()?;
    let raw = wrap::unwrap(
        secrets.expose_bytes(),
        &WrapBinding {
            organization_id: &office.info.org_id,
            office_id: &office.info.office_id,
            purpose: WrapPurpose::OrganizationCa,
        },
        &ca.key.wrapped_ca_key,
    )
    .map_err(|_| OfficeError::Invalid("failed unwrapping Org CA private key"))?;
    SigningKey::from_local_der(&raw).map_err(|_| OfficeError::Invalid("invalid Org CA private key"))
}

fn extract_san_hosts(hq_addresses: &[String]) -> Vec<String> {
    let mut hosts = vec![
        "localhost".to_owned(),
        "127.0.0.1".to_owned(),
        "::1".to_owned(),
    ];
    for addr in hq_addresses {
        let stripped = addr
            .strip_prefix("https://")
            .or_else(|| addr.strip_prefix("http://"))
            .or_else(|| addr.strip_prefix("tls://"))
            .unwrap_or(addr);
        let authority = stripped.split('/').next().unwrap_or("");
        let host = if let Some(rest) = authority.strip_prefix('[') {
            rest.split(']').next().unwrap_or("")
        } else {
            authority.split(':').next().unwrap_or("")
        };
        if !host.is_empty() && !hosts.iter().any(|h| h == host) {
            hosts.push(host.to_owned());
        }
    }
    hosts
}

impl Office {
    /// Ensures HQ's self-signed Org CA X.509 certificate and HQ TLS server leaf cert/key exist
    /// under `durable/local/offices/<hq_office_id>/tls/` and returns them for federation mTLS.
    pub fn ensure_hq_tls_identity(
        &self,
        san_hosts: &[String],
    ) -> Result<HqTlsIdentity, OfficeError> {
        let _guard = self
            .federation_io_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        ensure_hq_tls_identity_locked(self, san_hosts)
    }

    /// Creates a one-time satellite invitation token on HQ (owner-only). HQ stores only the
    /// public half of the one-time invitation key (`doc-67`).
    pub fn create_satellite_invitation(
        &self,
        actor_member_id: &str,
        params: CreateInvitationParams,
    ) -> Result<Result<CreateInvitationResult, ApiError>, OfficeError> {
        if let Err(err) = require_hq_owner(self, actor_member_id) {
            return Ok(Err(err));
        }
        let office_name = params.office_name.trim().to_owned();
        if office_name.is_empty() || params.hq_addresses.is_empty() {
            return Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "office_name and at least one hq_address are required",
            )));
        }
        let office_id = match params.existing_office_id {
            Some(id) => {
                if !org_proto::is_uuid_v7(&id) || id == self.info.office_id {
                    return Ok(Err(ApiError::new(
                        ErrorCode::InvalidRequest,
                        "existing_office_id must be a valid non-HQ UUIDv7",
                    )));
                }
                if is_satellite_office_removed(self, &id)? {
                    return Ok(Err(ApiError::new(
                        ErrorCode::Conflict,
                        "removed office ID cannot be re-enrolled; create a new office",
                    )));
                }
                id
            }
            None => self.next_uuid_v7(),
        };
        let ttl_secs = params
            .ttl_secs
            .unwrap_or(DEFAULT_INVITATION_TTL_SECS)
            .clamp(60, MAX_INVITATION_TTL_SECS);
        let now_ms = self.monotonic_now_ms();
        let expires_at_ms = now_ms.saturating_add(ttl_secs.saturating_mul(1000));
        let hq_tls = self.ensure_hq_tls_identity(&params.hq_addresses)?;
        let inv_key = SigningKey::generate()
            .map_err(|_| OfficeError::Invalid("failed generating invitation key"))?;
        let invitation_id = self.next_uuid_v7();
        let token = InvitationToken::new(
            NewInvitationParams {
                invitation_id: invitation_id.clone(),
                org_id: self.info.org_id.clone(),
                org_name: self.info.org_name.clone(),
                office_id: office_id.clone(),
                office_name: office_name.clone(),
                hq_office_id: self.info.office_id.clone(),
                hq_addresses: params.hq_addresses.clone(),
                ca_cert_der: hq_tls.ca_cert_der,
                expires_at_ms,
            },
            &inv_key,
        )
        .map_err(|_| OfficeError::Invalid("failed creating invitation token"))?;
        let encoded = token
            .encode()
            .map_err(|_| OfficeError::Invalid("failed encoding invitation token"))?;

        let stored = StoredInvitation {
            invitation_id,
            office_id,
            office_name,
            hq_addresses: params.hq_addresses,
            invitation_pub_key_b64: STANDARD.encode(inv_key.public_key()),
            status: InvitationStatus::Pending,
            created_at_ms: now_ms,
            expires_at_ms,
            request_id: None,
            joining_pub_key_b64: None,
            pop_signature_b64: None,
            check_code: None,
            wrong_code_attempts: 0,
            issued_cert_der_b64: None,
            issued_cert_serial_hex: None,
        };
        let view = stored.view();
        let _guard = self
            .federation_io_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut state = load_invitations(self)?;
        state.invitations.push(stored);
        save_invitations(self, &state)?;
        Ok(Ok(CreateInvitationResult {
            invitation: view,
            token: encoded,
        }))
    }

    /// Registers an incoming TLS-exporter-authenticated satellite join request against a pending
    /// invitation on HQ and computes the deterministic 8-char Crockford check code (`doc-67`).
    ///
    /// Persists `(request_id, joining_pub_key)` so a second peer cannot overwrite a pending
    /// key, and if TLS drops after HQ approval, reconnecting with the same `(request_id,
    /// joining_pub_key)` + fresh exporter proof replays the issued certificate idempotently.
    pub fn register_satellite_join_request(
        &self,
        req: JoinRequestParams,
    ) -> Result<Result<JoinRequestAccepted, ApiError>, OfficeError> {
        if self.info.role != OfficeRole::Hq {
            return Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "only HQ accepts satellite join requests",
            )));
        }
        if req.request_id.trim().is_empty() {
            return Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "request_id is required",
            )));
        }
        let _guard = self
            .federation_io_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let now_ms = self.monotonic_now_ms();
        let mut state = load_invitations(self)?;
        let Some(inv) = state
            .invitations
            .iter_mut()
            .find(|i| i.invitation_id == req.invitation_id)
        else {
            return Ok(Err(ApiError::new(
                ErrorCode::NotFound,
                "invitation not found",
            )));
        };
        if inv.office_id != req.office_id {
            return Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "office_id does not match invitation",
            )));
        }
        if let Some(err) = verify_join_request_proof(inv, &req)? {
            return Ok(Err(err));
        }
        if is_satellite_office_removed_locked(self, &inv.office_id)? {
            return Ok(Err(ApiError::new(
                ErrorCode::Conflict,
                "office certificate was revoked",
            )));
        }
        let incoming_pub_b64 = STANDARD.encode(req.joining_pub_key);
        if inv.status == InvitationStatus::Approved {
            return replay_approved_join(self, inv, &req.request_id, &incoming_pub_b64);
        }
        if now_ms > inv.expires_at_ms
            && matches!(
                inv.status,
                InvitationStatus::Pending | InvitationStatus::AwaitingApproval
            )
        {
            inv.status = InvitationStatus::Expired;
            save_invitations(self, &state)?;
            return Ok(Err(ApiError::new(
                ErrorCode::Expired,
                "invitation has expired",
            )));
        }
        if !matches!(
            inv.status,
            InvitationStatus::Pending | InvitationStatus::AwaitingApproval
        ) {
            return Ok(Err(ApiError::new(
                ErrorCode::Conflict,
                "invitation is no longer active",
            )));
        }
        if inv.status == InvitationStatus::AwaitingApproval {
            return reuse_pending_join(inv, &req, &incoming_pub_b64);
        }
        let check_code = compute_check_code(&req.invitation_id, &req.joining_pub_key);
        inv.request_id = Some(req.request_id);
        inv.joining_pub_key_b64 = Some(incoming_pub_b64);
        inv.pop_signature_b64 = Some(STANDARD.encode(&req.pop_signature_der));
        inv.check_code = Some(check_code.clone());
        inv.status = InvitationStatus::AwaitingApproval;
        save_invitations(self, &state)?;
        Ok(Ok(JoinRequestAccepted::Pending {
            invitation_id: req.invitation_id,
            office_id: req.office_id,
            check_code,
        }))
    }

    /// Approves a satellite join request after verifying the human-entered Crockford check code.
    /// Three wrong check codes automatically cancel the invitation (`doc-67`).
    pub fn approve_satellite_invitation(
        &self,
        actor_member_id: &str,
        invitation_id: &str,
        entered_check_code: &str,
    ) -> Result<Result<ApprovedSatelliteCert, ApiError>, OfficeError> {
        if let Err(err) = require_hq_owner(self, actor_member_id) {
            return Ok(Err(err));
        }
        let _guard = self
            .federation_io_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let now_ms = self.monotonic_now_ms();
        let mut state = load_invitations(self)?;
        let Some(idx) = state
            .invitations
            .iter()
            .position(|i| i.invitation_id == invitation_id)
        else {
            return Ok(Err(ApiError::new(
                ErrorCode::NotFound,
                "invitation not found",
            )));
        };
        if is_satellite_office_removed_locked(self, &state.invitations[idx].office_id)? {
            return Ok(Err(ApiError::new(
                ErrorCode::Conflict,
                "removed office ID cannot be re-enrolled; create a new office",
            )));
        }
        if state.invitations[idx].status == InvitationStatus::Approved {
            return idempotent_approved_retry(self, &state.invitations[idx], entered_check_code);
        }
        if let Some(err) =
            check_approval_preconditions(self, &mut state, idx, now_ms, entered_check_code)?
        {
            return Ok(Err(err));
        }
        issue_and_persist_approved_cert(self, &mut state, idx, invitation_id, now_ms)
    }

    fn register_satellite_office_in_store(
        &self,
        office_id: &str,
        office_name: &str,
    ) -> Result<SatelliteRegistered, OfficeError> {
        let sat_id = Id::parse(office_id)
            .map_err(|_| OfficeError::Invalid("invalid satellite office_id"))?;
        let op_id = Id::parse(&self.next_uuid_v7())
            .map_err(|_| OfficeError::Invalid("generated operation id"))?;
        let op = RegisterSatelliteOfficeOp {
            operation_id: op_id,
            here: self.identity.clone(),
            owner: self.owner.clone(),
            by: Actor::Employee(self.owner.clone()),
            command: RegisterSatelliteOffice {
                office_id: sat_id,
                office_name: office_name.to_owned(),
            },
        };
        let reply = self.store.run(op).map_err(storage)?;
        if reply.outcome != Outcome::Completed {
            return Err(OfficeError::Invalid(
                "satellite office registration rejected",
            ));
        }
        self.store.sync().map_err(storage)?;
        serde_json::from_slice(&reply.bytes)
            .map_err(|_| OfficeError::Invalid("malformed satellite registration reply"))
    }

    pub fn decline_satellite_invitation(
        &self,
        actor_member_id: &str,
        invitation_id: &str,
    ) -> Result<Result<InvitationSummaryView, ApiError>, OfficeError> {
        self.transition_invitation(actor_member_id, invitation_id, InvitationStatus::Declined)
    }

    pub fn cancel_satellite_invitation(
        &self,
        actor_member_id: &str,
        invitation_id: &str,
    ) -> Result<Result<InvitationSummaryView, ApiError>, OfficeError> {
        self.transition_invitation(actor_member_id, invitation_id, InvitationStatus::Cancelled)
    }

    fn transition_invitation(
        &self,
        actor_member_id: &str,
        invitation_id: &str,
        target: InvitationStatus,
    ) -> Result<Result<InvitationSummaryView, ApiError>, OfficeError> {
        if let Err(err) = require_hq_owner(self, actor_member_id) {
            return Ok(Err(err));
        }
        let _guard = self
            .federation_io_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut state = load_invitations(self)?;
        let Some(inv) = state
            .invitations
            .iter_mut()
            .find(|i| i.invitation_id == invitation_id)
        else {
            return Ok(Err(ApiError::new(
                ErrorCode::NotFound,
                "invitation not found",
            )));
        };
        if !matches!(
            inv.status,
            InvitationStatus::Pending | InvitationStatus::AwaitingApproval
        ) {
            return Ok(Err(ApiError::new(
                ErrorCode::Conflict,
                "invitation is already terminal",
            )));
        }
        inv.status = target;
        let view = inv.view();
        save_invitations(self, &state)?;
        Ok(Ok(view))
    }

    pub fn list_satellite_invitations(
        &self,
        actor_member_id: &str,
    ) -> Result<Result<Vec<InvitationSummaryView>, ApiError>, OfficeError> {
        if let Err(err) = require_hq_owner(self, actor_member_id) {
            return Ok(Err(err));
        }
        let _guard = self
            .federation_io_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let now_ms = self.monotonic_now_ms();
        let mut state = load_invitations(self)?;
        let mut changed = false;
        for inv in &mut state.invitations {
            if now_ms > inv.expires_at_ms
                && matches!(
                    inv.status,
                    InvitationStatus::Pending | InvitationStatus::AwaitingApproval
                )
            {
                inv.status = InvitationStatus::Expired;
                changed = true;
            }
        }
        if changed {
            save_invitations(self, &state)?;
        }
        Ok(Ok(state.invitations.iter().map(|i| i.view()).collect()))
    }

    /// Polls the current state of an enrollment invitation (used by HQ's federation listener).
    pub fn poll_satellite_invitation(
        &self,
        invitation_id: &str,
    ) -> Result<InvitationPollStatus, OfficeError> {
        let _guard = self
            .federation_io_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let now_ms = self.monotonic_now_ms();
        let state = load_invitations(self)?;
        let Some(inv) = state
            .invitations
            .iter()
            .find(|i| i.invitation_id == invitation_id)
        else {
            return Ok(InvitationPollStatus::Terminal {
                code: "not_found".into(),
                message: "invitation not found".into(),
            });
        };
        if is_satellite_office_removed_locked(self, &inv.office_id)? {
            return Ok(InvitationPollStatus::Terminal {
                code: "revoked".into(),
                message: "office certificate was revoked".into(),
            });
        }
        if now_ms > inv.expires_at_ms
            && matches!(
                inv.status,
                InvitationStatus::Pending | InvitationStatus::AwaitingApproval
            )
        {
            return Ok(InvitationPollStatus::Terminal {
                code: "expired".into(),
                message: "invitation expired".into(),
            });
        }
        match inv.status {
            InvitationStatus::Pending | InvitationStatus::AwaitingApproval => {
                Ok(InvitationPollStatus::Pending)
            }
            InvitationStatus::Approved => {
                let cert_der = STANDARD
                    .decode(inv.issued_cert_der_b64.as_deref().unwrap_or_default())
                    .map_err(|_| OfficeError::Invalid("stored issued cert corrupt"))?;
                let hq_tls = ensure_hq_tls_identity_locked(self, &inv.hq_addresses)?;
                Ok(InvitationPollStatus::Approved {
                    cert_der,
                    ca_cert_der: hq_tls.ca_cert_der,
                    hq_office_id: self.info.office_id.clone(),
                    epoch: hq_tls.epoch,
                })
            }
            InvitationStatus::Declined => Ok(InvitationPollStatus::Terminal {
                code: "declined".into(),
                message: "invitation was declined at HQ".into(),
            }),
            InvitationStatus::Cancelled => Ok(InvitationPollStatus::Terminal {
                code: "cancelled".into(),
                message: "invitation was cancelled at HQ".into(),
            }),
            InvitationStatus::Expired => Ok(InvitationPollStatus::Terminal {
                code: "expired".into(),
                message: "invitation expired".into(),
            }),
            InvitationStatus::Revoked => Ok(InvitationPollStatus::Terminal {
                code: "revoked".into(),
                message: "office certificate was revoked".into(),
            }),
        }
    }

    /// Revokes all issued certificates for `satellite_office_id` on HQ so it is immediately
    /// refused at the mTLS handshake and federation gate (`doc-67`).
    pub fn remove_satellite_office(
        &self,
        actor_member_id: &str,
        satellite_office_id: &str,
    ) -> Result<Result<SatelliteRemovalResult, ApiError>, OfficeError> {
        if let Err(err) = require_hq_owner(self, actor_member_id) {
            return Ok(Err(err));
        }
        if !org_proto::is_uuid_v7(satellite_office_id) || satellite_office_id == self.info.office_id
        {
            return Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "cannot remove HQ or invalid satellite office id",
            )));
        }
        let removed_id = satellite_office_id.to_owned();
        // Signal active sessions first so session loops stop starting new fenced work, then take
        // the exclusive per-peer activity barrier BEFORE publishing `revocations.v1.json` so every
        // already-admitted origin close or outbound send for this office completes before HQ
        // takeover or reconciliation can observe removal without blocking unrelated peers
        // (`TASK-52` P20-#2 / P25-#2).
        signal_peer_session_stops(self, &removed_id, false);
        let peer_fence = peer_activity_fence_for(self, &removed_id);
        let _barrier = peer_fence.write().unwrap_or_else(|e| e.into_inner());
        let (result, inv_save_err) = {
            let _guard = self
                .federation_io_lock
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let mut state = load_invitations(self)?;
            let mut rev = load_revocations(self)?;
            let mut newly_revoked = Vec::new();
            for inv in &mut state.invitations {
                if inv.office_id == satellite_office_id {
                    if let Some(serial) = &inv.issued_cert_serial_hex {
                        if rev.revoked_serials.insert(serial.clone()) {
                            newly_revoked.push(serial.clone());
                        }
                    }
                    inv.status = InvitationStatus::Revoked;
                }
            }
            rev.removed_offices.insert(removed_id.clone());
            save_revocations(self, &rev)?;
            // Revocation is now durable: save invitations WITHOUT skipping post-revocation relay
            // purge or live-session invalidation if `save_invitations` fails (`TASK-52` P20-#3).
            let inv_save_err = save_invitations(self, &state).err();
            let _ = self
                .replication
                .purge_relays_for_office(satellite_office_id);
            self.promoted_removed_offices
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&removed_id);
            (
                SatelliteRemovalResult {
                    office_id: removed_id.clone(),
                    revoked_serials: newly_revoked,
                },
                inv_save_err,
            )
        };
        // Still holding `_barrier`, invalidate any session that registered before the barrier.
        signal_peer_session_stops(self, &removed_id, true);
        drop(_barrier);
        cleanup_peer_activity_fence_if_unused(self, &removed_id, &peer_fence);
        // Promote any provisional HQ withdrawals for asks originated by the removed satellite
        // one journal commit at a time (`TASK-52` P15-#3 / P16-#2 / P16-#3).
        let _ = promote_pending_removed_origin_withdrawals(self);
        if let Some(err) = inv_save_err {
            return Err(err);
        }
        Ok(Ok(result))
    }

    pub fn revoked_cert_serials(&self) -> Result<BTreeSet<String>, OfficeError> {
        Ok(load_revocations(self)?.revoked_serials)
    }

    pub fn is_cert_serial_revoked(&self, serial_hex: &str) -> Result<bool, OfficeError> {
        Ok(load_revocations(self)?.revoked_serials.contains(serial_hex))
    }
}

/// Reconciles every permanently removed satellite office on HQ:
/// 1. Purges any lingering queued relays for each removed office under `federation_io_lock`
///    (retrying if a crash or storage error interrupted purge after `save_revocations`, `TASK-52` P17-#3).
/// 2. Promotes unfinalized provisional HQ withdrawals for asks originated by removed offices,
///    committing ONE withdrawal per journal entry (`TASK-52` P16-#2 / P16-#3).
pub fn promote_pending_removed_origin_withdrawals(office: &Office) -> Result<usize, OfficeError> {
    if office.info.role != OfficeRole::Hq {
        return Ok(0);
    }
    let removed = removed_satellite_offices(office)?;
    if removed.is_empty() {
        return Ok(0);
    }
    let pending: BTreeSet<String> = {
        let done = office
            .promoted_removed_offices
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        removed.difference(&done).cloned().collect()
    };
    if pending.is_empty() {
        return Ok(0);
    }
    // Re-purge every pending removed office under `federation_io_lock` first, propagating errors
    // so a crash or transient write failure never strands non-expiring relays (`TASK-52` P17-#3).
    {
        let _guard = office
            .federation_io_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        for oid in &pending {
            office.replication.purge_relays_for_office(oid)?;
        }
    }
    let mut promoted = 0usize;
    loop {
        let did_one = office
            .store
            .run(PromoteOneRemovedOriginWithdrawalOp {
                removed_offices: pending.clone(),
            })
            .map_err(OfficeError::storage)?;
        if !did_one {
            break;
        }
        promoted += 1;
    }
    let mut done = office
        .promoted_removed_offices
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for oid in pending {
        done.insert(oid);
    }
    Ok(promoted)
}

/// Atomically checks origin and destination revocation and enrollment under `federation_io_lock`
/// before admitting `item` into the durable relay store, preventing concurrent removal races from
/// admitting removed senders or recreating a removed office's queue and failing closed on
/// revocation read errors (`TASK-52` P16-#4 / P17-#4 / P18-#2).
pub fn admit_relay_item_if_active(
    office: &Office,
    item: crate::replication::StoredRelayItem,
) -> Result<RelayAdmission, OfficeError> {
    let _guard = office
        .federation_io_lock
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if is_satellite_office_removed_locked(office, &item.origin_office)? {
        return Ok(RelayAdmission::OriginRemoved);
    }
    if is_satellite_office_removed_locked(office, &item.destination_office)? {
        return Ok(RelayAdmission::DestinationRemoved);
    }
    let enrolled = office
        .enrolled_office_ids()?
        .iter()
        .any(|id| id == &item.destination_office);
    if !enrolled {
        return Ok(RelayAdmission::DestinationUnknown);
    }
    match office.replication.enqueue_relay_item(item) {
        Ok(()) => Ok(RelayAdmission::Admitted),
        Err(_) => Ok(RelayAdmission::QuotaExceeded),
    }
}

/// Atomically checks destination revocation under `federation_io_lock` before publishing one
/// destination's native journal relays, failing closed on revocation read errors (`TASK-52` P16-#4 / P17-#4).
pub fn publish_message_journal_relay_for_dest_checked(
    office: &Office,
    destination_office: &str,
    messages: Vec<org_sync::messages::Message>,
    relays: Vec<crate::replication::StoredRelayItem>,
    incarnation: &str,
    ordinal: u64,
    now_ms: u64,
) -> Result<usize, OfficeError> {
    let _guard = office
        .federation_io_lock
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if is_satellite_office_removed_locked(office, destination_office)? {
        return Ok(0);
    }
    office.replication.publish_message_journal_relay_for_dest(
        destination_office,
        messages,
        relays,
        incarnation,
        ordinal,
        now_ms,
    )
}

/// Atomically acknowledges a destination message relay and enqueues any forwarded receipt ONLY
/// if the receipt's target origin office has not been permanently removed, failing closed on
/// revocation read errors (`TASK-52` P16-#4 / P17-#4).
pub fn ack_message_relay_and_enqueue_receipt_checked(
    office: &Office,
    destination_office: &str,
    origin_office: &str,
    message_id: &str,
    fingerprint_hex: &str,
    receipt_item: Option<crate::replication::StoredRelayItem>,
) -> Result<bool, OfficeError> {
    let _guard = office
        .federation_io_lock
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let receipt_item = match receipt_item {
        Some(r) if is_satellite_office_removed_locked(office, &r.destination_office)? => None,
        other => other,
    };
    office.replication.ack_message_relay_and_enqueue_receipt(
        destination_office,
        origin_office,
        message_id,
        fingerprint_hex,
        receipt_item,
    )
}

/// RAII guard unregistering a live HQ peer session's stop flag on drop (`TASK-52` P19-#2).
pub struct PeerSessionGuard<'a> {
    office: &'a Office,
    peer_office_id: String,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for PeerSessionGuard<'_> {
    fn drop(&mut self) {
        let mut stops = self
            .office
            .peer_session_stops
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(flags) = stops.get_mut(&self.peer_office_id) {
            flags.retain(|f| !std::sync::Arc::ptr_eq(f, &self.stop));
            if flags.is_empty() {
                stops.remove(&self.peer_office_id);
            }
        }
    }
}

/// Registers a live HQ session's stop flag so `remove_satellite_office` can immediately
/// invalidate active sessions for that satellite before returning (`TASK-52` P19-#2).
pub fn register_peer_session_stop<'a>(
    office: &'a Office,
    peer_office_id: &str,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> PeerSessionGuard<'a> {
    let mut stops = office
        .peer_session_stops
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    stops
        .entry(peer_office_id.to_owned())
        .or_default()
        .push(std::sync::Arc::clone(&stop));
    PeerSessionGuard {
        office,
        peer_office_id: peer_office_id.to_owned(),
        stop,
    }
}

fn signal_peer_session_stops(office: &Office, peer_office_id: &str, remove_entry: bool) {
    let mut stops = office
        .peer_session_stops
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(flags) = stops.get(peer_office_id) {
        for flag in flags {
            flag.store(true, std::sync::atomic::Ordering::Release);
        }
    }
    if remove_entry {
        stops.remove(peer_office_id);
    }
}

/// Checks fail-closed whether `peer_office_id` (or optional `cert_serial_hex`) has been revoked
/// or permanently removed in `revocations.v1.json`, synchronizing only on the leaf
/// `revocation_lock` so keepalives never block on `federation_io_lock` (`TASK-52` P24-#1).
pub fn is_peer_revoked(
    office: &Office,
    peer_office_id: &str,
    cert_serial_hex: Option<&str>,
) -> Result<bool, OfficeError> {
    let rev = load_revocations(office)?;
    Ok(rev.removed_offices.contains(peer_office_id)
        || cert_serial_hex.is_some_and(|s| rev.revoked_serials.contains(s)))
}

fn peer_activity_fence_for(
    office: &Office,
    peer_office_id: &str,
) -> std::sync::Arc<std::sync::RwLock<()>> {
    let mut fences = office
        .peer_activity_fences
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    std::sync::Arc::clone(
        fences
            .entry(peer_office_id.to_owned())
            .or_insert_with(|| std::sync::Arc::new(std::sync::RwLock::new(()))),
    )
}

fn cleanup_peer_activity_fence_if_unused(
    office: &Office,
    peer_office_id: &str,
    fence: &std::sync::Arc<std::sync::RwLock<()>>,
) {
    let mut fences = office
        .peer_activity_fences
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if fences.get(peer_office_id).is_some_and(|cur| {
        std::sync::Arc::ptr_eq(cur, fence) && std::sync::Arc::strong_count(cur) == 2
    }) {
        fences.remove(peer_office_id);
    }
}

/// Executes `f` under `peer_office_id`'s per-peer activity fence ONLY if `peer_office_id` (and
/// optional `cert_serial_hex`) is not revoked/removed in `revocations.v1.json`, failing closed on
/// revocation read errors (`TASK-52` P19-#1 / P19-#2 / P25-#2).
pub fn run_if_peer_active<T>(
    office: &Office,
    peer_office_id: &str,
    cert_serial_hex: Option<&str>,
    f: impl FnOnce() -> T,
) -> Result<Option<T>, OfficeError> {
    let peer_fence = peer_activity_fence_for(office, peer_office_id);
    let _fence = peer_fence.read().unwrap_or_else(|e| e.into_inner());
    if is_peer_revoked(office, peer_office_id, cert_serial_hex)? {
        return Ok(None);
    }
    Ok(Some(f()))
}

/// Result of an atomic revocation-checked relay admission (`TASK-52` P16-#4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelayAdmission {
    Admitted,
    OriginRemoved,
    DestinationRemoved,
    DestinationUnknown,
    QuotaExceeded,
}

/// Promotes AT MOST ONE unfinalized provisional HQ withdrawal for asks originated by any office in
/// `removed_offices` to `reason: Some("origin-close")` per journal commit so every promoted entry
/// stays well within per-destination relay quotas and restarts resume cleanly (`TASK-52` P16-#2/#3).
struct PromoteOneRemovedOriginWithdrawalOp {
    removed_offices: BTreeSet<String>,
}

impl crate::store::open::StoreOp for PromoteOneRemovedOriginWithdrawalOp {
    type Out = Result<bool, org_store::StoreError>;

    fn run<E: crate::store::open::OfficeEngine>(self, store: &org_store::Store<E>) -> Self::Out {
        use crate::messaging::ports::MessagingRead;
        use crate::messaging::records::{encode_json, participant_answer_path};
        use org_store::{Ack, Change, Refusal};
        let removed = self.removed_offices;
        let promoted = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let slot = std::sync::Arc::clone(&promoted);
        store.submit(None, false, move |uow| {
            let convs = uow
                .view()
                .conversations()
                .map_err(|_| Refusal::invalid("read conversations"))?;
            for conv in convs {
                let msgs = uow
                    .view()
                    .messages_in_conversation(&conv.id)
                    .map_err(|_| Refusal::invalid("read conversation messages"))?;
                for msg in msgs {
                    if msg.kind != "request" || !removed.contains(msg.origin_office_id.as_str()) {
                        continue;
                    }
                    let Some(mut ans) = uow
                        .view()
                        .answer_for_message(&msg.id)
                        .map_err(|_| Refusal::invalid("read ask answer"))?
                    else {
                        continue;
                    };
                    let is_provisional_withdrawal =
                        crate::messaging::asks::decode_ask_answer_file(&ans)
                            .1
                            .is_some()
                            && ans.reason.as_deref() != Some("origin-close");
                    if !is_provisional_withdrawal {
                        continue;
                    }
                    ans.reason = Some("origin-close".to_owned());
                    ans.revision = ans.revision.saturating_add(1);
                    let path = participant_answer_path(
                        &ans.conversation_id,
                        &ans.employee_id,
                        &ans.message_id,
                    );
                    slot.store(true, std::sync::atomic::Ordering::Release);
                    return Ok(Change::new(Ack::Durable).put(path, encode_json(&ans)));
                }
            }
            Ok(Change::new(Ack::Durable))
        })?;
        Ok(promoted.load(std::sync::atomic::Ordering::Acquire))
    }
}

pub(crate) fn is_satellite_office_removed(
    office: &Office,
    office_id: &str,
) -> Result<bool, OfficeError> {
    let _guard = office
        .federation_io_lock
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    is_satellite_office_removed_locked(office, office_id)
}

/// Checks `removed_offices` fail-closed when the caller already holds `office.federation_io_lock`.
pub(crate) fn is_satellite_office_removed_locked(
    office: &Office,
    office_id: &str,
) -> Result<bool, OfficeError> {
    Ok(load_revocations(office)?
        .removed_offices
        .contains(office_id))
}

/// Loads all permanently removed satellite office IDs fail-closed when `office.federation_io_lock` is held.
pub(crate) fn removed_satellite_offices_locked(
    office: &Office,
) -> Result<BTreeSet<String>, OfficeError> {
    Ok(load_revocations(office)?.removed_offices)
}

/// Loads all permanently removed satellite office IDs fail-closed, acquiring `office.federation_io_lock`.
pub(crate) fn removed_satellite_offices(office: &Office) -> Result<BTreeSet<String>, OfficeError> {
    let _guard = office
        .federation_io_lock
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    removed_satellite_offices_locked(office)
}

fn verify_join_request_proof(
    inv: &StoredInvitation,
    req: &JoinRequestParams,
) -> Result<Option<ApiError>, OfficeError> {
    let pub_bytes = STANDARD
        .decode(&inv.invitation_pub_key_b64)
        .map_err(|_| OfficeError::Invalid("stored invitation pubkey corrupt"))?;
    let pub_arr: [u8; 65] = pub_bytes
        .try_into()
        .map_err(|_| OfficeError::Invalid("stored invitation pubkey length"))?;
    if verify_exporter_proof(
        &pub_arr,
        &req.exporter_binding,
        &req.invitation_id,
        &req.office_id,
        &req.joining_pub_key,
        &req.exporter_proof_der,
    )
    .is_err()
    {
        return Ok(Some(ApiError::new(
            ErrorCode::NotAuthorized,
            "invitation exporter possession proof verification failed",
        )));
    }
    Ok(None)
}

fn replay_approved_join(
    office: &Office,
    inv: &StoredInvitation,
    request_id: &str,
    incoming_pub_b64: &str,
) -> Result<Result<JoinRequestAccepted, ApiError>, OfficeError> {
    let same_binding = inv.request_id.as_deref() == Some(request_id)
        && inv.joining_pub_key_b64.as_deref() == Some(incoming_pub_b64);
    if !same_binding {
        return Ok(Err(ApiError::new(
            ErrorCode::Conflict,
            "invitation already approved for a different joining key",
        )));
    }
    let cert_der = STANDARD
        .decode(inv.issued_cert_der_b64.as_deref().unwrap_or_default())
        .map_err(|_| OfficeError::Invalid("stored issued cert corrupt"))?;
    let hq_tls = ensure_hq_tls_identity_locked(office, &inv.hq_addresses)?;
    Ok(Ok(JoinRequestAccepted::AlreadyApproved {
        cert_der,
        ca_cert_der: hq_tls.ca_cert_der,
        hq_office_id: office.info.office_id.clone(),
        epoch: hq_tls.epoch,
    }))
}

fn reuse_pending_join(
    inv: &StoredInvitation,
    req: &JoinRequestParams,
    incoming_pub_b64: &str,
) -> Result<Result<JoinRequestAccepted, ApiError>, OfficeError> {
    let same_binding = inv.request_id.as_deref() == Some(req.request_id.as_str())
        && inv.joining_pub_key_b64.as_deref() == Some(incoming_pub_b64);
    if !same_binding {
        return Ok(Err(ApiError::new(
            ErrorCode::Conflict,
            "invitation already bound to a pending join request",
        )));
    }
    let check_code = inv
        .check_code
        .clone()
        .unwrap_or_else(|| compute_check_code(&req.invitation_id, &req.joining_pub_key));
    Ok(Ok(JoinRequestAccepted::Pending {
        invitation_id: req.invitation_id.clone(),
        office_id: req.office_id.clone(),
        check_code,
    }))
}

fn idempotent_approved_retry(
    office: &Office,
    inv: &StoredInvitation,
    entered_check_code: &str,
) -> Result<Result<ApprovedSatelliteCert, ApiError>, OfficeError> {
    // Preserve terminal `Approved` state on retry even after TTL (`TASK-52` M-17).
    let expected = inv.check_code.as_deref().unwrap_or_default();
    if normalize_check_code(entered_check_code).as_deref() == Some(expected) {
        if let (Some(cert_b64), Some(serial_hex)) = (
            inv.issued_cert_der_b64.as_deref(),
            inv.issued_cert_serial_hex.clone(),
        ) {
            let cert_der = STANDARD
                .decode(cert_b64)
                .map_err(|_| OfficeError::Invalid("stored issued cert corrupt"))?;
            let hq_tls = ensure_hq_tls_identity_locked(office, &inv.hq_addresses)?;
            let registered =
                office.register_satellite_office_in_store(&inv.office_id, &inv.office_name)?;
            return Ok(Ok(ApprovedSatelliteCert {
                invitation: inv.view(),
                cert_der,
                ca_cert_der: hq_tls.ca_cert_der,
                serial_hex,
                epoch: hq_tls.epoch,
                registered,
            }));
        }
    }
    Ok(Err(ApiError::new(
        ErrorCode::Conflict,
        "invitation is already approved",
    )))
}

fn check_approval_preconditions(
    office: &Office,
    state: &mut InvitationsStateFile,
    idx: usize,
    now_ms: u64,
    entered_check_code: &str,
) -> Result<Option<ApiError>, OfficeError> {
    // Caller (`approve_satellite_invitation`) already holds `federation_io_lock`.
    if is_satellite_office_removed_locked(office, &state.invitations[idx].office_id)? {
        return Ok(Some(ApiError::new(
            ErrorCode::Conflict,
            "removed office ID cannot be re-enrolled; create a new office",
        )));
    }
    if matches!(
        state.invitations[idx].status,
        InvitationStatus::Pending | InvitationStatus::AwaitingApproval
    ) && now_ms > state.invitations[idx].expires_at_ms
    {
        state.invitations[idx].status = InvitationStatus::Expired;
        save_invitations(office, state)?;
        return Ok(Some(ApiError::new(
            ErrorCode::Expired,
            "invitation has expired",
        )));
    }
    if state.invitations[idx].status != InvitationStatus::AwaitingApproval {
        return Ok(Some(ApiError::new(
            ErrorCode::Conflict,
            "invitation is not awaiting approval",
        )));
    }
    let expected = state.invitations[idx]
        .check_code
        .clone()
        .unwrap_or_default();
    if normalize_check_code(entered_check_code).as_deref() != Some(expected.as_str()) {
        state.invitations[idx].wrong_code_attempts =
            state.invitations[idx].wrong_code_attempts.saturating_add(1);
        let attempts = state.invitations[idx].wrong_code_attempts;
        if attempts >= MAX_WRONG_CHECK_CODE_ATTEMPTS {
            state.invitations[idx].status = InvitationStatus::Cancelled;
        }
        save_invitations(office, state)?;
        return Ok(Some(ApiError::new(
            ErrorCode::InvalidRequest,
            if attempts >= MAX_WRONG_CHECK_CODE_ATTEMPTS {
                "check code mismatch; invitation cancelled after 3 wrong attempts"
            } else {
                "check code mismatch"
            },
        )));
    }
    Ok(None)
}

fn issue_and_persist_approved_cert(
    office: &Office,
    state: &mut InvitationsStateFile,
    idx: usize,
    invitation_id: &str,
    now_ms: u64,
) -> Result<Result<ApprovedSatelliteCert, ApiError>, OfficeError> {
    let joining_pub_vec = STANDARD
        .decode(
            state.invitations[idx]
                .joining_pub_key_b64
                .as_deref()
                .unwrap_or_default(),
        )
        .map_err(|_| OfficeError::Invalid("stored joining pubkey corrupt"))?;
    let joining_pub: [u8; 65] = joining_pub_vec
        .try_into()
        .map_err(|_| OfficeError::Invalid("stored joining pubkey length"))?;
    let pop_sig = STANDARD
        .decode(
            state.invitations[idx]
                .pop_signature_b64
                .as_deref()
                .unwrap_or_default(),
        )
        .map_err(|_| OfficeError::Invalid("stored pop signature corrupt"))?;

    let hq_tls = ensure_hq_tls_identity_locked(office, &state.invitations[idx].hq_addresses)?;
    let ca_key = unwrap_hq_ca_key(office)?;
    let now_unix = (now_ms / 1000) as i64;
    let issued = issue_satellite_cert_from_pubkey(
        &ca_key,
        &hq_tls.ca_cert_der,
        &IssueSatelliteCertParams {
            org_id: &office.info.org_id,
            office_id: &state.invitations[idx].office_id,
            invitation_id,
            subject_pub_key_uncompressed: &joining_pub,
            pop_signature_der: &pop_sig,
            now_unix,
            validity_days: DEFAULT_OFFICE_CERT_VALIDITY_DAYS,
        },
    )
    .map_err(|_| OfficeError::Invalid("satellite key proof-of-possession or signing failed"))?;

    let registered = office.register_satellite_office_in_store(
        &state.invitations[idx].office_id,
        &state.invitations[idx].office_name,
    )?;
    // Activate HQ durable journal consumers only once a satellite is actually enrolled so an
    // unused/expired invitation never pins a standalone HQ's journal (`TASK-52` P7-#8).
    let _ = office.register_replication_consumer("org-sync-hq")?;
    let _ = office.register_replication_consumer("org-sync-message-relay")?;

    state.invitations[idx].status = InvitationStatus::Approved;
    state.invitations[idx].issued_cert_der_b64 = Some(STANDARD.encode(&issued.cert_der));
    state.invitations[idx].issued_cert_serial_hex = Some(issued.serial_hex.clone());
    let view = state.invitations[idx].view();
    save_invitations(office, state)?;

    Ok(Ok(ApprovedSatelliteCert {
        invitation: view,
        cert_der: issued.cert_der,
        ca_cert_der: hq_tls.ca_cert_der,
        serial_hex: issued.serial_hex,
        epoch: hq_tls.epoch,
        registered,
    }))
}
