//! Wire RPC contracts for HQ satellite office invitations, certificate revocation / removal,
//! and multi-office replication status (`TASK-52`, `doc-67`, `FC-020`, `FC-024`, `FC-145`).

use serde::{Deserialize, Serialize};

use crate::Call;

/// Wire summary of a satellite office invitation (never carries secret token material).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireInvitationSummary {
    pub invitation_id: String,
    pub office_id: String,
    pub office_name: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check_code: Option<String>,
    pub wrong_code_attempts: u32,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issued_cert_serial_hex: Option<String>,
}

/// `office.invitation.create`: creates a one-time satellite invitation on HQ (owner-only).
pub struct OfficeInvitationCreate;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeInvitationCreateParams {
    pub office_name: String,
    pub hq_addresses: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub existing_office_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeInvitationCreateResult {
    pub invitation: WireInvitationSummary,
    /// One-time secret token (`agorg-inv1.<payload>`); shown once, never stored on HQ.
    pub token: String,
}

impl Call for OfficeInvitationCreate {
    const METHOD: &'static str = "office.invitation.create";
    type Params = OfficeInvitationCreateParams;
    type Result = OfficeInvitationCreateResult;
}

/// `office.invitation.list`: lists satellite invitations on HQ (owner-only).
pub struct OfficeInvitationList;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeInvitationListParams {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeInvitationListResult {
    pub invitations: Vec<WireInvitationSummary>,
}

impl Call for OfficeInvitationList {
    const METHOD: &'static str = "office.invitation.list";
    type Params = OfficeInvitationListParams;
    type Result = OfficeInvitationListResult;
}

/// `office.invitation.approve`: verifies the human-entered check code on HQ, signs the
/// satellite's `ClientAuth`-only certificate, and registers the satellite office in org state.
pub struct OfficeInvitationApprove;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeInvitationApproveParams {
    pub invitation_id: String,
    pub check_code: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeInvitationApproveResult {
    pub invitation: WireInvitationSummary,
    pub office_id: String,
    pub slug: String,
    pub office_code: String,
    pub cert_serial_hex: String,
}

impl Call for OfficeInvitationApprove {
    const METHOD: &'static str = "office.invitation.approve";
    type Params = OfficeInvitationApproveParams;
    type Result = OfficeInvitationApproveResult;
}

/// `office.invitation.decline`: declines an active invitation on HQ.
pub struct OfficeInvitationDecline;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeInvitationActionParams {
    pub invitation_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeInvitationActionResult {
    pub invitation: WireInvitationSummary,
}

impl Call for OfficeInvitationDecline {
    const METHOD: &'static str = "office.invitation.decline";
    type Params = OfficeInvitationActionParams;
    type Result = OfficeInvitationActionResult;
}

/// `office.invitation.cancel`: cancels an active invitation on HQ.
pub struct OfficeInvitationCancel;

impl Call for OfficeInvitationCancel {
    const METHOD: &'static str = "office.invitation.cancel";
    type Params = OfficeInvitationActionParams;
    type Result = OfficeInvitationActionResult;
}

/// `office.remove`: revokes a satellite office's issued certificates on HQ (owner-only).
pub struct OfficeRemove;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeRemoveParams {
    pub office_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeRemoveResult {
    pub office_id: String,
    pub revoked_serials: Vec<String>,
}

impl Call for OfficeRemove {
    const METHOD: &'static str = "office.remove";
    type Params = OfficeRemoveParams;
    type Result = OfficeRemoveResult;
}

/// `office.sync_status`: returns live replication and federation status for this office.
pub struct OfficeSyncStatus;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeSyncStatusParams {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeSyncStatusResult {
    pub org_id: String,
    pub local_office: String,
    pub hq_office: String,
    pub epoch: u64,
    pub seq: u64,
    pub highest_epoch: u64,
    pub protocol_level: u32,
    pub restore_pending: bool,
    pub initial_snapshot_required: bool,
    pub pending_work_count: usize,
    pub pending_message_count: usize,
}

impl Call for OfficeSyncStatus {
    const METHOD: &'static str = "office.sync_status";
    type Params = OfficeSyncStatusParams;
    type Result = OfficeSyncStatusResult;
}
