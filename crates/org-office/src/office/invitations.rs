//! HQ Org-CA TLS identity, satellite office invitation lifecycle, 3-wrong-codes cancellation,
//! certificate issuance, and satellite removal / certificate revocation (`TASK-52` / `doc-67`).

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use org_pki::Zeroizing;
use org_proto::{ApiError, ErrorCode};
use serde::{Deserialize, Serialize};

use super::{satellite::tls_dir, Office};
use crate::{
    error::OfficeError,
    model::OfficeRole,
    organization::features::offices::SatelliteRegistered,
    private_fs::{private_dir_synced, write_private_atomic},
};

mod invite;
mod join;
mod peers;
mod removal;
mod tls;

pub use peers::{
    ack_message_relay_and_enqueue_receipt_checked, admit_relay_item_if_active, is_peer_revoked,
    publish_message_journal_relay_for_dest_checked, register_peer_session_stop, run_if_peer_active,
    PeerSessionGuard, RelayAdmission,
};
pub use removal::promote_pending_removed_origin_withdrawals;
pub(crate) use removal::{
    is_satellite_office_removed_locked, removed_satellite_offices, removed_satellite_offices_locked,
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
        } else if authority.parse::<std::net::IpAddr>().is_ok() {
            authority
        } else {
            authority.split(':').next().unwrap_or("")
        };
        if !host.is_empty() && !hosts.iter().any(|h| h == host) {
            hosts.push(host.to_owned());
        }
    }
    hosts
}

#[cfg(test)]
mod san_host_tests {
    use super::extract_san_hosts;

    #[test]
    fn extract_san_hosts_preserves_bare_and_bracketed_ipv6_alongside_dns_and_ipv4() {
        let hosts = extract_san_hosts(&[
            "2001:db8::1".to_owned(),
            "[2001:db8::2]:7443".to_owned(),
            "10.0.0.5:7443".to_owned(),
            "https://hq.example.com:7443/federation".to_owned(),
            "2001:db8::1".to_owned(),
        ]);
        assert!(hosts.contains(&"2001:db8::1".to_owned()), "{hosts:?}");
        assert!(hosts.contains(&"2001:db8::2".to_owned()), "{hosts:?}");
        assert!(hosts.contains(&"10.0.0.5".to_owned()), "{hosts:?}");
        assert!(hosts.contains(&"hq.example.com".to_owned()), "{hosts:?}");
        assert!(!hosts.contains(&"2001".to_owned()), "{hosts:?}");
    }
}
