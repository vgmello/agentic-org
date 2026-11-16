//! Additive selected task contract, independent of legacy `task.assign` and
//! `gateway::CommandEnvelope`. Decode directly from ingress bytes, never `Value`.
//! Defining this contract does not advertise an implemented runtime capability.

use std::{fmt, marker::PhantomData};

use serde::{
    de::{self, MapAccess, Visitor},
    Deserialize, Deserializer, Serialize, Serializer,
};
use serde_json::value::RawValue;

use crate::gateway::OperationId;
pub mod adoption;
pub mod read;
pub mod staging;

pub const CAPABILITY: &str = "selected_tasks_v1";
pub const SET_ASSIGNMENT: &str = <SetAssignment as crate::Call>::METHOD;
pub const MAX_CHUNK_BYTES: usize = 256 * 1024;

/// Canonical lowercase UUIDv7 entity text. Does not establish existence or
/// authority and never reformats an imported opaque identity.
pub fn is_uuid_v7(id: &str) -> bool {
    let bytes = id.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => *b == b'-',
            14 => *b == b'7',
            19 => matches!(b, b'8' | b'9' | b'a' | b'b'),
            _ => b.is_ascii_digit() || (b'a'..=b'f').contains(b),
        })
}

pub struct SetAssignment;
impl crate::Call for SetAssignment {
    const METHOD: &'static str = "task.set_assignment";
    type Params = NativeAssignment;
    type Result = AssignmentResult<i64>;
}

/// The selected decoder preserves the original receipt's conflict state. The
/// method and params remain the existing work operation lookup contract.
pub struct Lookup;
impl crate::Call for Lookup {
    const METHOD: &'static str = "operation.get";
    type Params = crate::OperationGetParams;
    type Result = LookupOutcome<i64>;
}

/// The authorized assignment state observed at rejection, retained with the
/// original receipt. Browser adapters replace native i64 revisions with strings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurrentAssignment<R> {
    pub organization_id: String,
    pub office_id: String,
    pub task_id: String,
    pub revision: R,
    pub assigned_team_id: Option<String>,
    pub assignee_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
#[serde(bound(deserialize = "R: Deserialize<'de>"))]
pub enum AssignmentResult<R> {
    Completed {
        organization_id: String,
        office_id: String,
        task_id: String,
        revision: R,
        assigned_team_id: Option<String>,
        assignee_id: Option<String>,
        event_sha256: String,
    },
    Rejected {
        code: String,
        // Additive coverage: earlier retained receipts have no observed state.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        current: Option<CurrentAssignment<R>>,
    },
}

/// Selected adapter for the existing work-operation lookup. A legacy ApiError
/// decoder may ignore current; the selected adapter must retain it.
#[derive(Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LookupOutcome<R> {
    Committed { result: AssignmentResult<R> },
    Rejected { refusal: AssignmentRefusal<R> },
}
#[derive(Serialize, Deserialize)]
#[serde(bound(deserialize = "R: Deserialize<'de>"))]
pub struct AssignmentRefusal<R> {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<CurrentAssignment<R>>,
}

/// Lossless positive i64 entity revision. Operation IDs remain independent ULIDs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct Revision(i64);

impl Revision {
    pub fn new(value: i64) -> Result<Self, DecodeError> {
        if value <= 0 {
            Err(DecodeError)
        } else {
            Ok(Self(value))
        }
    }

    pub fn get(self) -> i64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for Revision {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Self::new(i64::deserialize(d)?).map_err(|_| de::Error::custom("invalid selected revision"))
    }
}

/// HTTP revisions are decimal strings; JavaScript Number never carries them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BrowserRevision(pub Revision);

impl Serialize for BrowserRevision {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0.get().to_string())
    }
}

impl<'de> Deserialize<'de> for BrowserRevision {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        if !text
            .as_bytes()
            .first()
            .is_some_and(|b| matches!(b, b'1'..=b'9'))
            || !text.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(de::Error::custom("invalid decimal revision string"));
        }
        let value = text
            .parse::<i64>()
            .ok()
            .and_then(|n| Revision::new(n).ok())
            .ok_or_else(|| de::Error::custom("invalid decimal revision string"))?;
        Ok(Self(value))
    }
}

impl AssignmentResult<i64> {
    pub fn into_browser(self) -> Result<AssignmentResult<BrowserRevision>, DecodeError> {
        Ok(match self {
            Self::Completed {
                organization_id,
                office_id,
                task_id,
                revision,
                assigned_team_id,
                assignee_id,
                event_sha256,
            } => AssignmentResult::Completed {
                organization_id,
                office_id,
                task_id,
                revision: BrowserRevision(Revision::new(revision)?),
                assigned_team_id,
                assignee_id,
                event_sha256,
            },
            Self::Rejected { code, current } => AssignmentResult::Rejected {
                code,
                current: current.map(CurrentAssignment::into_browser).transpose()?,
            },
        })
    }
}
impl CurrentAssignment<i64> {
    fn into_browser(self) -> Result<CurrentAssignment<BrowserRevision>, DecodeError> {
        Ok(CurrentAssignment {
            organization_id: self.organization_id,
            office_id: self.office_id,
            task_id: self.task_id,
            revision: BrowserRevision(Revision::new(self.revision)?),
            assigned_team_id: self.assigned_team_id,
            assignee_id: self.assignee_id,
        })
    }
}
impl LookupOutcome<i64> {
    pub fn into_browser(self) -> Result<LookupOutcome<BrowserRevision>, DecodeError> {
        Ok(match self {
            Self::Committed { result } => LookupOutcome::Committed {
                result: result.into_browser()?,
            },
            Self::Rejected { refusal } => LookupOutcome::Rejected {
                refusal: AssignmentRefusal {
                    code: refusal.code,
                    message: refusal.message,
                    current: refusal
                        .current
                        .map(CurrentAssignment::into_browser)
                        .transpose()?,
                },
            },
        })
    }
}

/// Required nullable ID. Unlike `Option`, missing JSON fields are not null.
/// Opaque nonempty strings are preserved without normalization or path inference.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct NullableId(pub Option<String>);

impl<'de> Deserialize<'de> for NullableId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct IdVisitor;
        impl Visitor<'_> for IdVisitor {
            type Value = NullableId;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an explicit null or nonempty opaque ID")
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(NullableId(None))
            }
            fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
                if s.is_empty() {
                    return Err(E::custom("empty selected ID"));
                }
                Ok(NullableId(Some(s.to_owned())))
            }
            fn visit_string<E: de::Error>(self, s: String) -> Result<Self::Value, E> {
                if s.is_empty() {
                    return Err(E::custom("empty selected ID"));
                }
                Ok(NullableId(Some(s)))
            }
        }
        // Missing-field deserializers reject deserialize_any; explicit JSON null
        // visits unit. Delegating to Option would silently accept absent fields.
        d.deserialize_any(IdVisitor)
    }
}

/// The complete pair with a native integer or browser string revision adapter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AssignmentCommand<R> {
    pub expected_revision: R,
    pub assigned_team_id: NullableId,
    pub assignee_id: NullableId,
}

impl<'de, R: Deserialize<'de>> Deserialize<'de> for AssignmentCommand<R> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Raw<R> {
            expected_revision: R,
            assigned_team_id: NullableId,
            assignee_id: NullableId,
        }
        let raw = object::<D, Raw<R>>(d)?;
        Ok(Self {
            expected_revision: raw.expected_revision,
            assigned_team_id: raw.assigned_team_id,
            assignee_id: raw.assignee_id,
        })
    }
}

impl AssignmentCommand<BrowserRevision> {
    pub fn into_native(self) -> AssignmentCommand<Revision> {
        AssignmentCommand {
            expected_revision: self.expected_revision.0,
            assigned_team_id: self.assigned_team_id,
            assignee_id: self.assignee_id,
        }
    }
}

/// Scope identity is validated against authenticated/registry context by the
/// adapter; these strings do not grant access or nominate a writer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OfficeScope {
    pub kind: OfficeKind,
    pub instance_id: String,
    pub org_id: String,
    pub office_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OfficeKind {
    Office,
}

impl<'de> Deserialize<'de> for OfficeScope {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Raw {
            kind: OfficeKind,
            instance_id: String,
            org_id: String,
            office_id: String,
        }
        let raw = object::<D, Raw>(d)?;
        if [&raw.instance_id, &raw.org_id, &raw.office_id]
            .iter()
            .any(|s| s.is_empty())
        {
            return Err(de::Error::custom("empty selected scope"));
        }
        Ok(Self {
            kind: raw.kind,
            instance_id: raw.instance_id,
            org_id: raw.org_id,
            office_id: raw.office_id,
        })
    }
}

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct HttpAssignment {
    pub operation_id: OperationId,
    pub scope: OfficeScope,
    pub params: AssignmentCommand<BrowserRevision>,
}

impl<'de> Deserialize<'de> for HttpAssignment {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Raw {
            operation_id: OperationId,
            scope: OfficeScope,
            params: AssignmentCommand<BrowserRevision>,
        }
        let raw = object::<D, Raw>(d)?;
        Ok(Self {
            operation_id: raw.operation_id,
            scope: raw.scope,
            params: raw.params,
        })
    }
}

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct NativeAssignment {
    pub change_id: OperationId,
    pub task_id: String,
    pub command: AssignmentCommand<Revision>,
}

impl<'de> Deserialize<'de> for NativeAssignment {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Raw {
            change_id: OperationId,
            task_id: String,
            command: AssignmentCommand<Revision>,
        }
        let raw = object::<D, Raw>(d)?;
        if raw.task_id.is_empty() {
            return Err(de::Error::custom("empty selected task ID"));
        }
        Ok(Self {
            change_id: raw.change_id,
            task_id: raw.task_id,
            command: raw.command,
        })
    }
}

/// Preserve a present null principal so the authorization layer refuses it
/// instead of mistaking it for an absent principal. Raw secrets have no Debug.
#[derive(Default)]
pub struct RawPrincipal(Option<Box<RawValue>>);

impl RawPrincipal {
    pub fn raw(&self) -> Option<&RawValue> {
        self.0.as_deref()
    }
}

impl<'de> Deserialize<'de> for RawPrincipal {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Box::<RawValue>::deserialize(d).map(|raw| Self(Some(raw)))
    }
}

/// Full native frame decoder, before legacy Request.params can collapse keys.
pub struct NativeAssignmentRequest {
    pub id: u64,
    pub principal: RawPrincipal,
    pub params: NativeAssignment,
}

/// Selected work-receipt lookup uses the existing method and operation ID.
/// Decode the entire frame before legacy `Value` routing erases duplicates.
pub struct NativeLookupRequest {
    pub id: u64,
    pub principal: RawPrincipal,
    pub operation_id: OperationId,
}

pub fn decode_native_lookup(bytes: &[u8]) -> Result<NativeLookupRequest, DecodeError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum Work {
        Work,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Params {
        kind: Work,
        id: OperationId,
    }
    impl<'de> Deserialize<'de> for LookupParams {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            let params = object::<D, Params>(d)?;
            let Work::Work = params.kind;
            Ok(Self(params.id))
        }
    }
    struct LookupParams(OperationId);
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Raw {
        id: u64,
        method: String,
        #[serde(default)]
        principal: RawPrincipal,
        params: LookupParams,
    }
    if bytes.len() > crate::MAX_FRAME_BYTES {
        return Err(DecodeError);
    }
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    let raw = object::<_, Raw>(&mut decoder).map_err(|_| DecodeError)?;
    decoder.end().map_err(|_| DecodeError)?;
    if raw.method != "operation.get" || raw.id == 0 || raw.id > 9_007_199_254_740_991 {
        return Err(DecodeError);
    }
    Ok(NativeLookupRequest {
        id: raw.id,
        principal: raw.principal,
        operation_id: raw.params.0,
    })
}

/// Sanitized ingress error. Do not expose raw serde errors or principal bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodeError;

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid selected task request")
    }
}
impl std::error::Error for DecodeError {}

pub fn decode_native_assignment(bytes: &[u8]) -> Result<NativeAssignmentRequest, DecodeError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Raw {
        id: u64,
        method: String,
        #[serde(default)]
        principal: RawPrincipal,
        params: NativeAssignment,
    }
    if bytes.len() > crate::MAX_FRAME_BYTES {
        return Err(DecodeError);
    }
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    let raw = object::<_, Raw>(&mut decoder).map_err(|_| DecodeError)?;
    decoder.end().map_err(|_| DecodeError)?;
    if raw.method != SET_ASSIGNMENT || raw.id == 0 || raw.id > 9_007_199_254_740_991 {
        return Err(DecodeError);
    }
    Ok(NativeAssignmentRequest {
        id: raw.id,
        principal: raw.principal,
        params: raw.params,
    })
}

pub fn decode_http_assignment(bytes: &[u8]) -> Result<HttpAssignment, DecodeError> {
    if bytes.len() > crate::MAX_FRAME_BYTES {
        return Err(DecodeError);
    }
    serde_json::from_slice(bytes).map_err(|_| DecodeError)
}

// Derived structs accept positional JSON arrays. The selected contract is
// object-only at every record boundary, with derive handling duplicates/unknowns.
fn object<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<T, D::Error> {
    struct ObjectVisitor<T>(PhantomData<T>);
    impl<'de, T: Deserialize<'de>> Visitor<'de> for ObjectVisitor<T> {
        type Value = T;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a JSON object")
        }
        fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<T, M::Error> {
            T::deserialize(de::value::MapAccessDeserializer::new(map))
        }
    }
    d.deserialize_map(ObjectVisitor(PhantomData))
}

/// Protected host admission identity. Never taken from a task command.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriterIdentity {
    pub instance_id: String,
    pub organization_id: String,
    pub office_id: String,
    pub journal_incarnation: String,
    pub writer_epoch: i64,
    pub machine_id: String,
}

/// Journal continuity, independent of the work-origin event allocator. Ordinal
/// zero names the immutable anchor, never a transaction or an event position.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalPosition {
    pub ordinal: i64,
    pub sha256: String,
    pub previous_sha256: Option<String>,
    pub origin_incarnation: String,
    pub origin_high_water: i64,
}

impl JournalPosition {
    pub fn valid(&self) -> bool {
        fn hash(s: &str) -> bool {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        }
        self.ordinal >= 0
            && hash(&self.sha256)
            && match &self.previous_sha256 {
                None => self.ordinal == 0,
                Some(s) => self.ordinal > 0 && hash(s),
            }
            && !self.origin_incarnation.is_empty()
            && self.origin_high_water >= 0
    }

    pub fn follows(&self, before: &Self) -> bool {
        self.valid()
            && before.valid()
            && before.ordinal.checked_add(1) == Some(self.ordinal)
            && self.previous_sha256.as_deref() == Some(&before.sha256)
            && self.origin_incarnation == before.origin_incarnation
            && self.origin_high_water >= before.origin_high_water
    }
}

#[cfg(test)]
mod tests;
