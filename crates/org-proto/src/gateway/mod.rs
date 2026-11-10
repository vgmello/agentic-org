//! The browser's `/api/v1` contract, independent of the local socket API version.
//!
//! These are wire types, not a running HTTP adapter or an authorization service. The host
//! owns accounts/sessions, the worker owns domain authorization, and the gateway owns the
//! cookie/proof mapping. See `docs/design/gateway-api-v1.md`. Shared conformance fixtures live
//! in `crates/org-proto/test-vectors/v2/gateway-*.json`; they never enter production UI imports.

use serde::{Deserialize, Serialize};

mod outcome;
mod session;
mod stream;
mod views;

pub use outcome::{
    CommandEnvelope, CreateState, GatewayError, GatewayErrorCode, MutationOutcome, OperationId,
    OperationLookup, ReadResponse, ResourceAccess,
};
pub use session::{
    AccountView, OrganizationLink, SecretText, SessionEstablished, SessionProof, SessionView,
    SignInParams,
};
pub use stream::{
    EventFrame, Feed, PollResponse, ResumeCursor, ResyncReason, SnapshotResponse, StreamRequest,
    Subscription, SubscriptionCursor,
};
pub use views::{
    CancelResult, CancelState, Capabilities, Capability, GatewayStatus, InstanceView, ListenScheme,
    OfficeRestartResult, OfficeView, OrganizationView, RecoveryState, SessionReason, TlsStatus,
};

/// HTTP API and contract revision. Revisions are additive; breaking changes need a new API.
pub const API_VERSION: u32 = 1;
pub const CONTRACT_REVISION: u32 = 1;
pub const SESSION_PROOF_HEADER: &str = "X-Org-Session-Proof";

/// Maximum integer represented exactly by JavaScript. Times are Unix milliseconds, not strings.
pub const MAX_SAFE_INTEGER: u64 = (1_u64 << 53) - 1;

/// Public, constant metadata. Never add instance, account or organization information here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    pub api_version: u32,
    pub contract_revision: u32,
}

impl Default for Version {
    fn default() -> Self {
        Self {
            api_version: API_VERSION,
            contract_revision: CONTRACT_REVISION,
        }
    }
}

impl Version {
    /// A higher revision is additive; a lower required revision or another API needs update.
    pub fn check_required(&self, required: Self) -> Result<(), GatewayError> {
        if self.api_version == required.api_version
            && self.contract_revision >= required.contract_revision
        {
            Ok(())
        } else {
            Err(GatewayError::new(
                GatewayErrorCode::UnsupportedVersion,
                "Update this page to continue.",
            ))
        }
    }
}

/// A nonnegative JavaScript-safe millisecond value. Server time uses the Unix epoch;
/// retry delays are durations. JSON's mathematically integral `1.0` also means one millisecond.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct Milliseconds(u64);

impl Milliseconds {
    pub fn new(value: u64) -> Result<Self, &'static str> {
        (value <= MAX_SAFE_INTEGER)
            .then_some(Self(value))
            .ok_or("server time is not a JavaScript-safe integer")
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for Milliseconds {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl serde::de::Visitor<'_> for Visitor {
            type Value = Milliseconds;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a nonnegative JavaScript-safe integer of milliseconds")
            }
            fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
                Milliseconds::new(value).map_err(E::custom)
            }
            fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Self::Value, E> {
                self.visit_u64(
                    u64::try_from(value).map_err(|_| E::custom("negative milliseconds"))?,
                )
            }
            fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Self::Value, E> {
                if value.is_finite()
                    && value >= 0.0
                    && value <= MAX_SAFE_INTEGER as f64
                    && value.fract() == 0.0
                {
                    self.visit_u64(value as u64)
                } else {
                    Err(E::custom("milliseconds must be a nonnegative safe integer"))
                }
            }
        }
        d.deserialize_any(Visitor)
    }
}

/// Decode an incoming HTTP request object directly, with sanitized errors. The adapter
/// applies finite body/time/rate limits first and wipes credential input buffers. This is
/// ingress validation, never evidence that an already dispatched operation was rejected.
pub fn decode_request<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, GatewayError> {
    if body.iter().copied().find(|b| !b.is_ascii_whitespace()) != Some(b'{') {
        return Err(GatewayError::new(
            GatewayErrorCode::InvalidRequest,
            "Expected a JSON object.",
        ));
    }
    serde_json::from_slice(body)
        .map_err(|_| GatewayError::new(GatewayErrorCode::InvalidRequest, "Invalid request shape."))
}

/// Validate a raw, still encoded path segment before the HTTP framework decodes/routes it.
/// Decode at most once afterward; encoded separators must never become routing separators.
/// The client separately segment-encodes IDs and treats an ID separator as a client bug.
pub fn validate_raw_path_segment(segment: &str) -> Result<(), GatewayError> {
    let bytes = segment.as_bytes();
    if segment.is_empty()
        || [".", "..", "%2e", ".%2e", "%2e.", "%2e%2e"]
            .iter()
            .any(|dot| segment.eq_ignore_ascii_case(dot))
        || bytes.iter().any(|b| matches!(b, b'/' | b'\\'))
        || bytes
            .windows(3)
            .any(|part| matches!(part, [b'%', b'2', b'f' | b'F'] | [b'%', b'5', b'c' | b'C']))
    {
        Err(GatewayError::new(
            GatewayErrorCode::InvalidRequest,
            "Invalid path segment.",
        ))
    } else {
        Ok(())
    }
}

/// Explicit browser target. The authenticated pair supplies the actor, never these fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Scope {
    Instance {
        instance_id: String,
    },
    Organization {
        instance_id: String,
        org_id: String,
    },
    Office {
        instance_id: String,
        org_id: String,
        office_id: String,
    },
}

impl Scope {
    pub fn instance_id(&self) -> &str {
        match self {
            Self::Instance { instance_id }
            | Self::Organization { instance_id, .. }
            | Self::Office { instance_id, .. } => instance_id,
        }
    }

    /// Check the typed route target before forwarding. This does not authorize the caller.
    pub fn check_target(&self, route_target: &Self) -> Result<(), GatewayError> {
        if self == route_target {
            Ok(())
        } else {
            Err(GatewayError::new(
                GatewayErrorCode::InvalidRequest,
                "The request scope does not match its route.",
            ))
        }
    }
}

/// The independently stored operation-ID namespaces. The ID alone cannot select a ledger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    Host,
    Org,
    Work,
    Msg,
}

pub type OrganizationCreateRequest = CommandEnvelope<crate::OrganizationCreateParams>;
pub type OfficeRestartRequest = CommandEnvelope<crate::Empty>;
pub type OrganizationCreateOutcome = MutationOutcome<OrganizationView>;
pub type OrganizationCreateLookup = OperationLookup<OrganizationView>;
pub type OfficeRestartOutcome = MutationOutcome<OfficeRestartResult>;
pub type OfficeRestartLookup = OperationLookup<OfficeRestartResult>;

/// Dedicated cancellation has no body ID or business payload; the URL names the original ID.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CancelRequest {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrganizationList {
    pub organizations: Vec<OrganizationView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeList {
    pub offices: Vec<OfficeView>,
}

/// Frozen typed HTTP operations. This inventory is not an unrestricted dispatch endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Operation {
    pub name: &'static str,
    pub method: &'static str,
    pub route: &'static str,
}

pub const OPERATIONS: &[Operation] = &[
    Operation {
        name: "version",
        method: "GET",
        route: "/api/v1/version",
    },
    Operation {
        name: "session.sign_in",
        method: "POST",
        route: "/api/v1/session/sign-in",
    },
    Operation {
        name: "session.get",
        method: "GET",
        route: "/api/v1/session",
    },
    Operation {
        name: "session.refresh",
        method: "POST",
        route: "/api/v1/session/refresh",
    },
    Operation {
        name: "session.sign_out",
        method: "POST",
        route: "/api/v1/session/sign-out",
    },
    Operation {
        name: "capabilities",
        method: "GET",
        route: "/api/v1/capabilities",
    },
    Operation {
        name: "instance.get",
        method: "GET",
        route: "/api/v1/instance",
    },
    Operation {
        name: "organization.list",
        method: "GET",
        route: "/api/v1/organizations",
    },
    Operation {
        name: "organization.create",
        method: "POST",
        route: "/api/v1/organizations",
    },
    Operation {
        name: "organization.cancel",
        method: "POST",
        route: "/api/v1/organizations/operations/{id}/cancel",
    },
    Operation {
        name: "office.list",
        method: "GET",
        route: "/api/v1/organizations/{org}/offices",
    },
    Operation {
        name: "org.snapshot",
        method: "GET",
        route: "/api/v1/organizations/{org}/snapshot",
    },
    Operation {
        name: "office.restart",
        method: "POST",
        route: "/api/v1/organizations/{org}/offices/{office}/restart",
    },
    Operation {
        name: "operation.get",
        method: "GET",
        route: "/api/v1/operations/{kind}/{id}",
    },
    Operation {
        name: "events",
        method: "POST",
        route: "/api/v1/events",
    },
    Operation {
        name: "events.poll",
        method: "POST",
        route: "/api/v1/events/poll",
    },
];

#[cfg(test)]
mod tests;
