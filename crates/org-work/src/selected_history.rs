//! Supported codec-2 event headers, checksum validation and producer encoding.
//!
//! Verification here establishes JSON/header integrity, not actor authority,
//! historical coverage, public classification, blob durability or acceptance.
//! Preserve original bytes independently, including unsupported codecs in staging.

use std::{collections::BTreeMap, fmt};

use crate::{
    selected_codec::{self as codec, Json, NumberToken, RESOURCE_PROFILE},
    TaskHistoryHeader,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission {
    /// Actual known actor/time required for a new accepted change.
    New,
    /// Genuine unknown actor/time allowed; no attribution is fabricated.
    Import,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Codec(codec::Error),
    UnsupportedCodec,
    UnsupportedProfile,
    MissingHeader,
    InvalidHeader,
    IdentityMismatch,
    InvalidAssignment,
    InvalidSnapshot,
    ChecksumMismatch,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "selected event: {self:?}")
    }
}
impl std::error::Error for Error {}
impl From<codec::Error> for Error {
    fn from(e: codec::Error) -> Self {
        Self::Codec(e)
    }
}

/// Identity extracted by the adapter from the actual canonical task/path registry.
/// The filename must be the full decimal revision component, including `.json`.
pub struct ExpectedIdentity<'a> {
    pub organization_id: &'a str,
    pub task_id: &'a str,
    pub filename: &'a str,
}

/// A supported, checksum-verified envelope. All unknown content remains in `value`.
/// External blob references still require authorized length/hash/durability checks.
#[derive(Clone, Debug)]
pub struct VerifiedEvent {
    value: Json,
    revision: i64,
}

impl VerifiedEvent {
    pub fn value(&self) -> &Json {
        &self.value
    }
    pub fn revision(&self) -> i64 {
        self.revision
    }

    /// Suitable input to the separate coverage checker after actual file identity
    /// verification. This method does not itself establish coverage/final state.
    pub fn coverage_header(&self) -> TaskHistoryHeader<'_> {
        let text = |key| {
            self.value
                .get(key)
                .and_then(Json::as_str)
                .expect("validated event header")
        };
        TaskHistoryHeader {
            organization_id: text("organization_id"),
            task_id: text("task_id"),
            revision: self.revision as u64,
            filename_revision: self.revision as u64,
            change_type: self
                .value
                .get("change")
                .and_then(|c| c.get("type"))
                .and_then(Json::as_str)
                .expect("validated event tag"),
            payload_sha256: text("payload_sha256"),
        }
    }
}

/// Verify supported-codec headers and actual hash, then bind to canonical identity.
/// The caller retains input bytes on every failure; unknown codecs are not recoded.
pub fn verify(
    bytes: &[u8],
    expected: ExpectedIdentity<'_>,
    admission: Admission,
) -> Result<VerifiedEvent, Error> {
    let value = codec::parse(bytes)?;
    let revision = validate(&value, admission)?;
    if expected.organization_id.is_empty()
        || expected.task_id.is_empty()
        || value.get("organization_id").and_then(Json::as_str) != Some(expected.organization_id)
        || value.get("task_id").and_then(Json::as_str) != Some(expected.task_id)
        || expected.filename != format!("{revision}.json")
    {
        return Err(Error::IdentityMismatch);
    }
    let stored = value
        .get("payload_sha256")
        .and_then(Json::as_str)
        .ok_or(Error::MissingHeader)?;
    if !is_digest(stored) || codec::payload_sha256(&value)? != stored {
        return Err(Error::ChecksumMismatch);
    }
    // Input size and normalized output size are independently bounded.
    codec::canonical(&value)?;
    Ok(VerifiedEvent { value, revision })
}

/// Producer fields. Actor/time must come from authenticated writer context, not
/// user params. Optional IDs and empty source metadata are explicitly materialized.
pub struct EventFields {
    pub organization_id: String,
    pub task_id: String,
    pub revision: i64,
    pub occurred_at: Option<String>,
    pub actor_kind: String,
    pub actor_id: Option<String>,
    pub operation_id: Option<String>,
    pub change: Json,
    pub source_metadata: BTreeMap<String, Json>,
    pub extensions: BTreeMap<String, Json>,
}

/// Encode an explicitly new schema-2 event; never use this to rewrite a retained
/// schema-1 event at its identity. Import may wrap original lossless objects.
pub fn encode(fields: EventFields, admission: Admission) -> Result<Vec<u8>, Error> {
    let mut map = fields.extensions;
    let nullable = |id: Option<String>| id.map(Json::String).unwrap_or(Json::Null);
    for (key, value) in [
        ("schema_version", Json::Number(NumberToken::new("2")?)),
        (
            "resource_profile",
            Json::String(RESOURCE_PROFILE.to_owned()),
        ),
        ("organization_id", Json::String(fields.organization_id)),
        ("task_kind", Json::String("task".into())),
        ("task_id", Json::String(fields.task_id)),
        (
            "revision",
            Json::Number(NumberToken::new(&fields.revision.to_string())?),
        ),
        ("occurred_at", nullable(fields.occurred_at)),
        ("actor_kind", Json::String(fields.actor_kind)),
        ("actor_id", nullable(fields.actor_id)),
        ("operation_id", nullable(fields.operation_id)),
        ("change", fields.change),
        ("source_metadata", Json::Object(fields.source_metadata)),
    ] {
        if map.insert(key.to_owned(), value).is_some() {
            return Err(Error::InvalidHeader);
        }
    }
    if map.contains_key("payload_sha256") {
        return Err(Error::InvalidHeader);
    }
    let mut value = Json::Object(map);
    validate(&value, admission)?;
    let checksum = codec::payload_sha256(&value)?;
    if let Json::Object(map) = &mut value {
        map.insert("payload_sha256".into(), Json::String(checksum));
    }
    Ok(codec::canonical(&value)?)
}

fn validate(value: &Json, admission: Admission) -> Result<i64, Error> {
    let required = |key| value.get(key).ok_or(Error::MissingHeader);
    let text = |key| {
        required(key)?
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or(Error::InvalidHeader)
    };
    let schema = required("schema_version")?;
    if !matches!(schema, Json::Number(n) if n.as_str() == "2") {
        return Err(Error::UnsupportedCodec);
    }
    if text("resource_profile")? != RESOURCE_PROFILE {
        return Err(Error::UnsupportedProfile);
    }
    text("organization_id")?;
    text("task_id")?;
    if text("task_kind")? != "task" {
        return Err(Error::InvalidHeader);
    }
    let revision = required("revision")?
        .positive_i64()
        .ok_or(Error::InvalidHeader)?;
    validate_attribution(value, admission)?;
    validate_change(required("change")?, revision)?;
    Ok(revision)
}

fn validate_attribution(value: &Json, admission: Admission) -> Result<(), Error> {
    let required = |key| value.get(key).ok_or(Error::MissingHeader);
    let text = |key| {
        required(key)?
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or(Error::InvalidHeader)
    };
    let actor = text("actor_kind")?;
    if !matches!(actor, "employee" | "owner" | "system" | "unknown")
        || (admission == Admission::New && actor == "unknown")
    {
        return Err(Error::InvalidHeader);
    }
    match (actor, required("actor_id")?) {
        ("employee", Json::String(s)) if !s.is_empty() => {}
        ("owner" | "system" | "unknown", Json::Null) => {}
        _ => return Err(Error::InvalidHeader),
    }
    match required("occurred_at")? {
        Json::String(s) if utc_timestamp(s) => {}
        Json::Null if admission == Admission::Import => {}
        _ => return Err(Error::InvalidHeader),
    }
    match required("operation_id")? {
        Json::String(s) if !s.is_empty() => {}
        Json::Null => {}
        _ => return Err(Error::InvalidHeader),
    }
    if !matches!(required("source_metadata")?, Json::Object(_)) {
        return Err(Error::InvalidHeader);
    }
    Ok(())
}

fn validate_change(change: &Json, revision: i64) -> Result<(), Error> {
    let tag = change
        .get("type")
        .and_then(Json::as_str)
        .filter(|s| !s.is_empty())
        .ok_or(Error::InvalidHeader)?;
    match tag {
        "assignment_changed" => {
            validate_assignment_change(change)?;
        }
        "created" | "history_baseline" => {
            if tag == "created" && revision != 1 {
                return Err(Error::InvalidHeader);
            }
            if !matches!(change.get("known_facts"), Some(Json::Object(_))) {
                return Err(Error::InvalidSnapshot);
            }
            validate_snapshot(
                change
                    .get("public_snapshot")
                    .ok_or(Error::InvalidSnapshot)?,
            )?;
        }
        // Future changes are retained, hashed and rendered explicitly as unknown.
        _ => {}
    }
    Ok(())
}

fn validate_assignment_change(change: &Json) -> Result<(), Error> {
    for key in ["from", "to"] {
        let pair = change.get(key).ok_or(Error::InvalidAssignment)?;
        for key in ["assigned_team_id", "assignee_id"] {
            match pair.get(key) {
                Some(Json::Null) => {}
                Some(Json::String(s)) if !s.is_empty() => {}
                _ => return Err(Error::InvalidAssignment),
            }
        }
    }
    Ok(())
}

fn is_digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
}

fn validate_snapshot(value: &Json) -> Result<(), Error> {
    if value.get("encoding").and_then(Json::as_str) != Some("utf-8-markdown") {
        return Err(Error::InvalidSnapshot);
    }
    if let Some(text) = value.get("text") {
        if !matches!(text, Json::String(_))
            || ["sha256", "byte_length", "resource_profile"]
                .iter()
                .any(|key| value.get(key).is_some())
        {
            return Err(Error::InvalidSnapshot);
        }
    } else {
        let digest = value
            .get("sha256")
            .and_then(Json::as_str)
            .ok_or(Error::InvalidSnapshot)?;
        let length = value.get("byte_length").ok_or(Error::InvalidSnapshot)?;
        let length = match length {
            Json::Number(n) if n.as_str() == "0" => 0,
            n => n.positive_i64().ok_or(Error::InvalidSnapshot)?,
        };
        if !is_digest(digest)
            || length as u64 > codec::MAX_BLOB_BYTES
            || value.get("resource_profile").and_then(Json::as_str) != Some(RESOURCE_PROFILE)
        {
            return Err(Error::InvalidSnapshot);
        }
    }
    Ok(())
}

// Accept quoted UTC ISO timestamps, retaining fractional digits exactly. Newly
// generated events use format_ms; imports need not invent millisecond precision.
/// Check the selected UTC timestamp profile without changing fractional digits.
pub fn utc_timestamp(s: &str) -> bool {
    let Some(core) = s.strip_suffix('Z') else {
        return false;
    };
    let (seconds, fraction) = core
        .split_once('.')
        .map_or((core, None), |(a, b)| (a, Some(b)));
    if fraction.is_some_and(|s| s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit())) {
        return false;
    }
    let Some([year, month, day, hour, minute, second]) = timestamp_fields(seconds) else {
        return false;
    };
    let days = match month {
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => return false,
    };
    day >= 1 && day <= days && hour < 24 && minute < 60 && second < 60
}

fn timestamp_fields(seconds: &str) -> Option<[u32; 6]> {
    if seconds.len() != 19 || !seconds.is_ascii() {
        return None;
    }
    let b = seconds.as_bytes();
    if b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let number = |a: usize, end: usize| {
        b[a..end].iter().try_fold(0u32, |n, &c| {
            c.is_ascii_digit().then(|| n * 10 + u32::from(c - b'0'))
        })
    };
    Some([
        number(0, 4)?,
        number(5, 7)?,
        number(8, 10)?,
        number(11, 13)?,
        number(14, 16)?,
        number(17, 19)?,
    ])
}

#[cfg(test)]
mod tests;
