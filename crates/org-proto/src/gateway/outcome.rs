use std::fmt;
use std::marker::PhantomData;

use serde::de::{DeserializeOwned, Error as _};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use super::{OperationKind, Scope, SessionReason};

/// Struct derives otherwise accept JSON arrays as positional structs. HTTP records are maps.
struct Object<T>(T);

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Object<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Visitor<T>(PhantomData<T>);
        impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Visitor<T> {
            type Value = Object<T>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON object")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                map: M,
            ) -> Result<Self::Value, M::Error> {
                T::deserialize(serde::de::value::MapAccessDeserializer::new(map)).map(Object)
            }
        }
        d.deserialize_map(Visitor(PhantomData))
    }
}

/// The one end-to-end ID. Admission age/skew and durable dedup remain the owner's job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct OperationId(String);

impl OperationId {
    pub fn parse(id: String) -> Result<Self, &'static str> {
        crate::ulid_time_ms(&id)
            .map(|_| Self(id))
            .ok_or("operation_id must be a canonical ULID")
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for OperationId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Self::parse(String::deserialize(d)?).map_err(D::Error::custom)
    }
}

/// Typed mutation input. Expected versions remain in `params`; identity is never duplicated.
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct CommandEnvelope<P> {
    pub operation_id: OperationId,
    pub scope: Scope,
    pub params: P,
}

impl<P> fmt::Debug for CommandEnvelope<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandEnvelope")
            .field("operation_id", &self.operation_id)
            .field("scope", &self.scope)
            .finish_non_exhaustive()
    }
}

fn check_business_params(params: &Value) -> Result<(), &'static str> {
    let object = params.as_object().ok_or("params must be an object")?;
    for field in ["operation_id", "command_id", "change_id", "id"] {
        if object.contains_key(field) {
            return Err("the operation ID belongs only in the envelope");
        }
    }
    for field in ["home", "clear_home", "app_token"] {
        if object.contains_key(field) {
            return Err("retired credential or managed-home input");
        }
    }
    if let Some(profile) = object.get("profile") {
        let profile = profile.as_object().ok_or("profile must be an object")?;
        if profile.contains_key("home") {
            return Err("home is managed by the office");
        }
    }
    Ok(())
}

impl<'de, P: DeserializeOwned> Deserialize<'de> for CommandEnvelope<P> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            operation_id: OperationId,
            scope: Scope,
            params: Value,
        }
        let raw = Object::<Raw>::deserialize(d)?.0;
        check_business_params(&raw.params).map_err(D::Error::custom)?;
        let params = serde_json::from_value(raw.params)
            .map_err(|_| D::Error::custom("invalid typed business params"))?;
        Ok(Self {
            operation_id: raw.operation_id,
            scope: raw.scope,
            params,
        })
    }
}

impl<P: Serialize> CommandEnvelope<P> {
    /// Reconstruct the owner's native params without minting an ID or altering versions.
    /// The adapter must then decode the *specific typed call*; this is not a dispatch API.
    /// Credential/session inputs never go through this business-payload `Value` path.
    pub fn native_params(&self, kind: OperationKind) -> Result<Value, &'static str> {
        let mut params = serde_json::to_value(&self.params).map_err(|_| "invalid params")?;
        check_business_params(&params)?;
        let field = match kind {
            OperationKind::Host => "operation_id",
            OperationKind::Org => "command_id",
            OperationKind::Work => "change_id",
            OperationKind::Msg => "id",
        };
        params[field] = Value::String(self.operation_id.0.clone());
        Ok(params)
    }
}

/// Browser error vocabulary. New X-02 codes are additive here before the socket API cutover.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum GatewayErrorCode {
    InvalidRequest,
    UnknownMethod,
    InvalidName,
    AlreadyExists,
    StorageError,
    Cycle,
    NotEmpty,
    NotAMember,
    OwnerProtected,
    NotFound,
    Conflict,
    SnapshotRequired,
    Expired,
    ClockSkew,
    UnknownRecipient,
    NotAuthorized,
    Unavailable,
    NotReady,
    Timeout,
    TooLarge,
    ChecklistIncomplete,
    UnsupportedVersion,
    OperationReused,
    /// Always a confirmed pre-dispatch refusal. Distinct from domain error codes.
    RateLimited,
    Unknown,
    /// A newer refusal code is still a refusal; retain its exact spelling.
    #[serde(untagged)]
    Other(String),
}

/// Sanitized human error, with optional nonsecret retry/session hints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GatewayError {
    pub code: GatewayErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<super::Milliseconds>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_reason: Option<SessionReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<OperationId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<super::CancelState>,
}

impl<'de> Deserialize<'de> for GatewayError {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            code: GatewayErrorCode,
            message: String,
            #[serde(default)]
            retry_after_ms: Option<super::Milliseconds>,
            #[serde(default)]
            session_reason: Option<SessionReason>,
            #[serde(default)]
            operation_id: Option<OperationId>,
            #[serde(default)]
            state: Option<super::CancelState>,
        }
        let raw = Object::<Raw>::deserialize(d)?.0;
        if raw.code == GatewayErrorCode::RateLimited && raw.retry_after_ms.is_none() {
            return Err(D::Error::custom("rate_limited requires retry_after_ms"));
        }
        Ok(Self {
            code: raw.code,
            message: raw.message,
            retry_after_ms: raw.retry_after_ms,
            session_reason: raw.session_reason,
            operation_id: raw.operation_id,
            state: raw.state,
        })
    }
}

impl GatewayError {
    /// Cancellation keeps the original create ID; in-progress is never a successful result.
    pub fn create_in_progress(operation_id: OperationId) -> Self {
        let mut error = Self::new(
            GatewayErrorCode::Conflict,
            "The create is still in progress; try again.",
        );
        error.operation_id = Some(operation_id);
        error.state = Some(super::CancelState::InProgress);
        error
    }

    pub fn new(code: GatewayErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retry_after_ms: None,
            session_reason: None,
            operation_id: None,
            state: None,
        }
    }

    /// The error's class, for confirmed refusals only. A transport timeout after possible
    /// dispatch is an *unknown outcome*, not this error. `None` means unsupported.
    pub fn http_status(&self, authenticated: bool) -> Option<u16> {
        use GatewayErrorCode::*;
        Some(match self.code {
            InvalidRequest | UnknownMethod | InvalidName | Cycle | NotEmpty | NotAMember
            | OwnerProtected | ChecklistIncomplete | ClockSkew | UnknownRecipient => 400,
            NotAuthorized if authenticated => 403,
            NotAuthorized => 401,
            NotFound => 404,
            Conflict | AlreadyExists | OperationReused | SnapshotRequired => 409,
            Expired => 410,
            TooLarge => 413,
            UnsupportedVersion => 426,
            RateLimited => 429,
            NotReady | Unavailable | StorageError | Timeout => 503,
            Unknown | Other(_) => return None,
        })
    }

    /// Adapt an access decision already made by the owning authority. Hidden and missing
    /// intentionally have one shape; this helper does not decide access or inspect storage.
    pub fn for_resource_access(access: ResourceAccess) -> Self {
        match access {
            ResourceAccess::Missing | ResourceAccess::Hidden => {
                Self::new(GatewayErrorCode::NotFound, "Not found.")
            }
            ResourceAccess::ReadOnly => {
                Self::new(GatewayErrorCode::NotAuthorized, "Not authorized.")
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceAccess {
    Missing,
    Hidden,
    ReadOnly,
}

impl From<crate::ApiError> for GatewayError {
    /// The adapter logs an unexpected `not_bound` on a host-made channel as an invariant
    /// failure before converting it. Browser callers never bind and never see that code.
    fn from(error: crate::ApiError) -> Self {
        if error.code == crate::ErrorCode::NotBound {
            return Self::new(
                GatewayErrorCode::Unavailable,
                "This operation is unavailable.",
            );
        }
        let code = serde_json::to_value(error.code)
            .and_then(serde_json::from_value)
            .unwrap_or(GatewayErrorCode::Unknown);
        Self::new(code, error.message)
    }
}

/// Only organization.create has these journal states. Other host operations are ordinary
/// completed/rejected outcomes; abandoned creation is final without an outcome at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CreateState {
    Reserved,
    Linked,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationLookup<T> {
    Outcome {
        kind: OperationKind,
        state: Option<CreateState>,
        outcome: MutationOutcome<T>,
    },
    Abandoned {
        operation_id: OperationId,
    },
    Unsupported {
        operation_id: OperationId,
        kind: OperationKind,
    },
}

impl<T> OperationLookup<T> {
    pub fn is_final(&self) -> bool {
        matches!(
            self,
            Self::Abandoned { .. }
                | Self::Unsupported { .. }
                | Self::Outcome {
                    outcome: MutationOutcome::Completed { .. }
                        | MutationOutcome::Rejected { .. }
                        | MutationOutcome::Unsupported { .. },
                    ..
                }
        )
    }
}

impl<T: Serialize> Serialize for OperationLookup<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if matches!(
            self,
            Self::Outcome {
                outcome: MutationOutcome::Unknown { .. },
                ..
            }
        ) {
            return Err(<S::Error as serde::ser::Error>::custom(
                "unknown is not a stored lookup outcome",
            ));
        }
        // Stream the typed result directly, preserving its bytes/field order exactly as
        // in the mutation response. A Value intermediary would reorder result fields.
        let mut wire = serializer.serialize_map(None)?;
        match self {
            Self::Outcome {
                kind,
                state,
                outcome,
            } => {
                wire.serialize_entry("kind", kind)?;
                if let Some(state) = state {
                    wire.serialize_entry("state", state)?;
                }
                let (operation_id, name) = match outcome {
                    MutationOutcome::Accepted { operation_id } => (operation_id, "accepted"),
                    MutationOutcome::Completed { operation_id, .. } => (operation_id, "completed"),
                    MutationOutcome::Rejected { operation_id, .. } => (operation_id, "rejected"),
                    MutationOutcome::Unknown { operation_id } => (operation_id, "unknown"),
                    MutationOutcome::Unsupported { operation_id } => (operation_id, "unsupported"),
                };
                wire.serialize_entry("operation_id", operation_id)?;
                wire.serialize_entry("outcome", name)?;
                match outcome {
                    MutationOutcome::Completed { result, .. } => {
                        wire.serialize_entry("result", result)?
                    }
                    MutationOutcome::Rejected { error, .. } => {
                        wire.serialize_entry("error", error)?
                    }
                    _ => {}
                }
            }
            Self::Abandoned { operation_id } => {
                wire.serialize_entry("operation_id", operation_id)?;
                wire.serialize_entry("kind", "host")?;
                wire.serialize_entry("state", "abandoned")?;
            }
            Self::Unsupported { operation_id, kind } => {
                wire.serialize_entry("operation_id", operation_id)?;
                wire.serialize_entry("kind", kind)?;
                wire.serialize_entry("outcome", "unsupported")?;
            }
        }
        wire.end()
    }
}

impl<'de, T: DeserializeOwned> Deserialize<'de> for OperationLookup<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut wire = Object::<serde_json::Map<String, Value>>::deserialize(d)?.0;
        if wire.get("outcome").and_then(Value::as_str) == Some("unknown") {
            return Err(D::Error::custom("unknown is not a stored lookup outcome"));
        }
        let operation_id: OperationId =
            serde_json::from_value(wire.get("operation_id").cloned().unwrap_or(Value::Null))
                .map_err(|_| D::Error::custom("lookup requires an operation ID"))?;
        let kind: OperationKind =
            serde_json::from_value(wire.remove("kind").unwrap_or(Value::Null))
                .map_err(|_| D::Error::custom("lookup requires its ledger kind"))?;
        let state = wire.remove("state");
        if state.is_some() && kind != OperationKind::Host {
            return Err(D::Error::custom("only host creates have journal states"));
        }
        if state.as_ref() == Some(&Value::String("abandoned".into())) {
            if ["outcome", "result", "error"]
                .iter()
                .any(|key| wire.contains_key(*key))
            {
                return Err(D::Error::custom(
                    "abandoned creation has no outcome or payload",
                ));
            }
            return Ok(Self::Abandoned { operation_id });
        }
        let state: Option<CreateState> = state
            .map(serde_json::from_value)
            .transpose()
            .map_err(|_| D::Error::custom("invalid create journal state"))?;
        if state == Some(CreateState::Unknown) {
            return Ok(Self::Unsupported { operation_id, kind });
        }
        let outcome = serde_json::from_value::<MutationOutcome<T>>(Value::Object(wire))
            .map_err(|_| D::Error::custom("invalid lookup outcome"))?;
        if matches!(outcome, MutationOutcome::Unsupported { .. }) {
            return Ok(Self::Unsupported { operation_id, kind });
        }
        match (&state, &outcome) {
            (Some(CreateState::Reserved), MutationOutcome::Accepted { .. })
            | (Some(CreateState::Linked), MutationOutcome::Completed { .. }) => {}
            (None, MutationOutcome::Accepted { .. }) if kind == OperationKind::Host => {
                return Err(D::Error::custom(
                    "accepted host create requires reserved state",
                ));
            }
            (None, _) => {}
            _ => return Err(D::Error::custom("journal state and outcome disagree")),
        }
        Ok(Self::Outcome {
            kind,
            state,
            outcome,
        })
    }
}

// Track presence separately from Option: even `error:null` is a second payload key.
#[derive(Default)]
enum Field<T> {
    #[default]
    Missing,
    Present(T),
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Field<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        T::deserialize(d).map(Self::Present)
    }
}

/// Read/session envelope. Decode credential results directly into their secret-bearing type,
/// never into a Value/untagged/flattened intermediate. Unknown additive fields are ignored.
pub enum ReadResponse<T> {
    Result(T),
    Error(GatewayError),
}

impl<T> fmt::Debug for ReadResponse<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Result(_) => f.write_str("ReadResponse::Result(<payload>)"),
            Self::Error(error) => f.debug_tuple("ReadResponse::Error").field(error).finish(),
        }
    }
}

impl<T: Serialize> Serialize for ReadResponse<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(1))?;
        match self {
            Self::Result(value) => map.serialize_entry("result", value)?,
            Self::Error(error) => map.serialize_entry("error", error)?,
        }
        map.end()
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for ReadResponse<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(bound(deserialize = "T: Deserialize<'de>"))]
        struct Raw<T> {
            #[serde(default)]
            result: Field<Object<T>>,
            #[serde(default)]
            error: Field<Object<GatewayError>>,
        }
        let raw = Object::<Raw<T>>::deserialize(d)?.0;
        match (raw.result, raw.error) {
            (Field::Present(Object(value)), Field::Missing) => Ok(Self::Result(value)),
            (Field::Missing, Field::Present(Object(error))) => Ok(Self::Error(error)),
            _ => Err(D::Error::custom(
                "exactly one of result and error is required",
            )),
        }
    }
}

/// Durable acceptance is not completion. `Unknown` retains the ID for lookup. `Unsupported`
/// is a client-side state for a future variant; servers do not deliberately produce it.
/// Mutation results contain business data, never password/proof/session material.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum MutationOutcome<T> {
    Accepted {
        operation_id: OperationId,
    },
    Completed {
        operation_id: OperationId,
        result: T,
    },
    Rejected {
        operation_id: OperationId,
        error: GatewayError,
    },
    Unknown {
        operation_id: OperationId,
    },
    Unsupported {
        operation_id: OperationId,
    },
}

impl<T> fmt::Debug for MutationOutcome<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (name, operation_id) = match self {
            Self::Accepted { operation_id } => ("Accepted", operation_id),
            Self::Completed { operation_id, .. } => ("Completed", operation_id),
            Self::Rejected { operation_id, .. } => ("Rejected", operation_id),
            Self::Unknown { operation_id } => ("Unknown", operation_id),
            Self::Unsupported { operation_id } => ("Unsupported", operation_id),
        };
        f.debug_struct(name)
            .field("operation_id", operation_id)
            .finish_non_exhaustive()
    }
}

impl<T> MutationOutcome<T> {
    /// Unsupported future outcomes stop polling for an update; they never mean success.
    pub fn is_final(&self) -> bool {
        matches!(
            self,
            Self::Completed { .. } | Self::Rejected { .. } | Self::Unsupported { .. }
        )
    }

    pub fn http_status(&self, authenticated: bool) -> Option<u16> {
        match self {
            Self::Accepted { .. } => Some(202),
            Self::Completed { .. } => Some(200),
            Self::Rejected { error, .. } => error.http_status(authenticated),
            Self::Unknown { .. } => Some(504),
            Self::Unsupported { .. } => None,
        }
    }
}

impl<'de, T: DeserializeOwned> Deserialize<'de> for MutationOutcome<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            outcome: String,
            operation_id: OperationId,
            #[serde(default)]
            result: Field<Value>,
            #[serde(default)]
            error: Field<Value>,
        }
        let Raw {
            outcome,
            operation_id,
            result,
            error,
        } = Object::<Raw>::deserialize(d)?.0;
        match (outcome.as_str(), result, error) {
            ("accepted", Field::Missing, Field::Missing) => Ok(Self::Accepted { operation_id }),
            ("unknown", Field::Missing, Field::Missing) => Ok(Self::Unknown { operation_id }),
            ("completed", Field::Present(value), Field::Missing) => {
                if !value.is_object() {
                    return Err(D::Error::custom("completed result must be an object"));
                }
                let result = serde_json::from_value(value)
                    .map_err(|_| D::Error::custom("invalid completed result"))?;
                Ok(Self::Completed {
                    operation_id,
                    result,
                })
            }
            ("rejected", Field::Missing, Field::Present(value)) => {
                if !value.is_object() {
                    return Err(D::Error::custom("rejection error must be an object"));
                }
                let error: GatewayError = serde_json::from_value(value)
                    .map_err(|_| D::Error::custom("invalid rejection error"))?;
                if error.code == GatewayErrorCode::Unknown {
                    Ok(Self::Unknown { operation_id })
                } else {
                    Ok(Self::Rejected {
                        operation_id,
                        error,
                    })
                }
            }
            ("accepted" | "completed" | "rejected" | "unknown", _, _) => {
                Err(D::Error::custom("outcome and payload keys disagree"))
            }
            _ => Ok(Self::Unsupported { operation_id }),
        }
    }
}
