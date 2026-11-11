//! Count-only, session-free instance diagnostics on host.sock (X-02 §6.2).
use std::collections::BTreeMap;

use org_proto::Call;
use serde::{de, Deserialize, Deserializer, Serialize};

pub struct InstanceStatus;
#[derive(Debug, Default, Serialize)]
pub struct InstanceStatusParams {}
impl<'de> Deserialize<'de> for InstanceStatusParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let fields = BTreeMap::<String, de::IgnoredAny>::deserialize(d)?;
        if !fields.is_empty() {
            return Err(de::Error::custom("instance.status takes no fields"));
        }
        Ok(InstanceStatusParams {})
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum InstanceMode {
    Bootstrap,
    Service,
    #[serde(other, skip_serializing)]
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum GatewayState {
    NotConfigured,
    Starting,
    Running,
    Failed,
    #[serde(other, skip_serializing)]
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayStatus {
    pub state: GatewayState,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeCounts {
    pub total: u64,
    pub active: u64,
    pub failed: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceStatusResult {
    pub instance_id: String,
    pub mode: InstanceMode,
    pub gateway: GatewayStatus,
    pub offices: OfficeCounts,
}
impl Call for InstanceStatus {
    const METHOD: &'static str = "instance.status";
    type Params = InstanceStatusParams;
    type Result = InstanceStatusResult;
}
#[cfg(test)]
mod tests;
