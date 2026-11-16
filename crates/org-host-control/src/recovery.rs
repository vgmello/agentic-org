//! Native REC-DEL-1 nonsecret reads. Showing a code remains ops::RecoveryRelease.
use crate::SessionHandle;
use org_proto::{Call, EventKind};
use serde::{Deserialize, Serialize};

pub struct Status;
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusParams {
    pub session: SessionHandle,
    pub org_id: String,
    /// Exclusive canonical decimal delivery ordinal. Each page includes all
    /// actions for at most 32 ordinals, without splitting an ACK from its show.
    #[serde(default)]
    pub after_ordinal: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryState {
    Pending,
    Delivered,
    AckPending,
    Acknowledged,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Audit {
    pub ordinal: String,
    pub action: String,
    pub account: String,
    pub at_ms: i64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusResult {
    pub version: u32,
    pub org_id: String,
    pub state: DeliveryState,
    pub delivery_ordinal: String,
    pub first_shown_ms: Option<i64>,
    pub last_redelivered_ms: Option<i64>,
    pub redelivery_count: String,
    pub acknowledged_ms: Option<i64>,
    pub acknowledged_by: Option<String>,
    pub audit: Vec<Audit>,
    pub next_after_ordinal: Option<String>,
    pub hq_loss_recovery_supported: bool,
}
impl Call for Status {
    const METHOD: &'static str = "recovery.status";
    type Params = StatusParams;
    type Result = StatusResult;
}
pub struct Package;
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageParams {
    pub session: SessionHandle,
    pub org_id: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageResult {
    pub version: u32,
    pub instance_id: String,
    pub office_id: String,
    pub organization_id: String,
    pub operation_id: String,
    pub escrow_version: u32,
    pub public_key: String,
    pub package_sha256: String,
    pub package_hex: String,
}
impl Call for Package {
    const METHOD: &'static str = "recovery.package";
    type Params = PackageParams;
    type Result = PackageResult;
}
pub struct Changed;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangedData {
    pub version: u32,
    pub org_id: String,
    pub recovery: DeliveryState,
    pub delivery_ordinal: String,
    pub first_shown_ms: Option<i64>,
    pub last_redelivered_ms: Option<i64>,
    pub redelivery_count: String,
    pub acknowledged_ms: Option<i64>,
}
impl EventKind for Changed {
    const NAME: &'static str = "organization.changed";
    type Data = ChangedData;
}
