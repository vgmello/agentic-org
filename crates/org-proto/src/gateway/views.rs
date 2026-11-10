use serde::de::Error as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Milliseconds, OperationId, Scope};
use crate::{OfficeInfo, OfficeRole, OfficeStatus, OfficeStatusReason, Organization};

macro_rules! view_enum {
    ($name:ident { $($variant:ident),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        #[non_exhaustive]
        pub enum $name { $($variant,)+ #[serde(other)] Unknown }
    };
}

view_enum!(GatewayStatus { Ok, Degraded });
view_enum!(ListenScheme {
    Https,
    HttpLoopback
});
view_enum!(TlsStatus {
    Ok,
    Missing,
    Expiring
});
view_enum!(RecoveryState {
    Pending,
    Delivered,
    Acknowledged
});
view_enum!(SessionReason {
    SessionEnded,
    HqRestarted
});
view_enum!(CancelState {
    Abandoned,
    Linked,
    InProgress
});

/// The minimum protected instance status. No paths, sockets, keys or certificate material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceView {
    pub instance_id: String,
    pub name: String,
    pub server_version: String,
    pub gateway_status: GatewayStatus,
    pub listen_scheme: ListenScheme,
    pub tls: TlsStatus,
    pub server_time_ms: Milliseconds,
}

/// Public office status deliberately excludes `OfficeInfo.endpoint`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeView {
    pub id: String,
    pub organization_id: String,
    pub name: String,
    pub role: OfficeRole,
    pub host: String,
    pub status: OfficeStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_reason: Option<OfficeStatusReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_restart_at: Option<String>,
}

impl From<OfficeInfo> for OfficeView {
    fn from(office: OfficeInfo) -> Self {
        Self {
            id: office.id,
            organization_id: office.organization_id,
            name: office.name,
            role: office.role,
            host: office.host,
            status: office.status,
            status_reason: office.status_reason,
            next_restart_at: office.next_restart_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OrganizationView {
    pub id: String,
    pub name: String,
    pub created_at: String,
    /// State only. The recovery code must never enter the browser contract.
    pub recovery: RecoveryState,
}

impl OrganizationView {
    pub fn from_organization(organization: Organization, recovery: RecoveryState) -> Self {
        Self {
            id: organization.id,
            name: organization.name,
            created_at: organization.created_at,
            recovery,
        }
    }
}

/// Known host-control fields can never become additive public result fields. This guard
/// applies only to create/restart projections, not domain records such as repository paths.
fn check_public_projection(value: &Value) -> Result<(), &'static str> {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                if matches!(
                    key.as_str(),
                    "endpoint"
                        | "socket"
                        | "path"
                        | "db_key"
                        | "recovery_code"
                        | "owner_grant"
                        | "host_control"
                ) || key.ends_with("_path")
                {
                    return Err("host-control fields are not public result data");
                }
                check_public_projection(value)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                check_public_projection(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn public_object<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Value, D::Error> {
    let value = Value::deserialize(d)?;
    if !value.is_object() {
        return Err(D::Error::custom("public result must be an object"));
    }
    check_public_projection(&value).map_err(D::Error::custom)?;
    Ok(value)
}

impl<'de> Deserialize<'de> for OrganizationView {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            id: String,
            name: String,
            created_at: String,
            recovery: RecoveryState,
        }
        let raw: Raw = serde_json::from_value(public_object(d)?)
            .map_err(|_| D::Error::custom("invalid public organization view"))?;
        Ok(Self {
            id: raw.id,
            name: raw.name,
            created_at: raw.created_at,
            recovery: raw.recovery,
        })
    }
}

/// Restart acceptance/completion says nothing about health. Read the offices list/feed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct OfficeRestartResult {}

impl<'de> Deserialize<'de> for OfficeRestartResult {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        public_object(d)?;
        Ok(Self {})
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    pub name: String,
    pub scope: Scope,
    #[serde(default)]
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl Capability {
    /// Callers supply names their client actually implements. A newer server cannot make
    /// an unknown client operation executable merely by advertising it.
    pub fn is_available(&self, known_names: &[&str]) -> bool {
        self.available && known_names.contains(&self.name.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub api_version: u32,
    pub contract_revision: u32,
    pub instance_id: String,
    pub operations: Vec<Capability>,
    pub feeds: Vec<Capability>,
}

/// Targeted cancellation of the original create. No new ID or fingerprint is submitted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelResult {
    pub operation_id: OperationId,
    pub state: CancelState,
}

impl CancelResult {
    pub fn http_status(&self) -> Option<u16> {
        match self.state {
            CancelState::Abandoned | CancelState::Linked => Some(200),
            CancelState::InProgress => Some(409),
            CancelState::Unknown => None,
        }
    }
}
