//! The office-local family's two files under `local/offices/<office-id>/`, and their codecs.
//!
//! - `security/ca-key.v1.json`: the HQ's CA key record. The CA private key, wrapped by the
//!   host-held office secrets key (ADR-0016), with what binds it to the organization's
//!   escrow package. Written atomically to local security storage outside the acceptance
//!   journal (TASK-10 / H-06).
//! - `security/ca-key.v1.staging`: temporary staging record during HQ creation, promoted to
//!   `ca-key.v1.json` once Entry 1 commits and removed on completion or aborted pre-append.
//! - `clean-stop`: the marker an orderly stop leaves. Never written through the journal.
//!
//! Both are one JSON object with a fixed key order, lowercase hex, no whitespace and a
//! trailing newline (data contract document 01). A file in any other form is refused,
//! so one record has one spelling and the exact bytes are its contract.

use std::fmt;

use org_store::{DurablePath, Id, Sha256};
use serde::Deserialize;

use crate::hq_creation::{MAX_WRAPPED_CA_KEY, MIN_WRAPPED_CA_KEY};

/// The only schema version of the CA key record.
const CA_KEY_SCHEMA_VERSION: u64 = 1;
/// The escrow format the record's package is in (ADR-0007, version 1).
const ESCROW_VERSION: u64 = 1;

/// A file of this family that is not in its one form.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Malformed(pub &'static str);

impl fmt::Display for Malformed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for Malformed {}

fn path(text: String) -> DurablePath {
    DurablePath::parse(&text).expect("an ID is one safe path component")
}

/// The folder of `office`'s records.
pub fn folder(office: &Id) -> DurablePath {
    path(format!("local/offices/{office}"))
}

/// The CA key record of `office`.
pub fn ca_key_path(office: &Id) -> DurablePath {
    path(format!("local/offices/{office}/security/ca-key.v1.json"))
}

/// The transient staging CA key record written outside the acceptance journal during HQ creation.
pub fn ca_key_staging_path(office: &Id) -> DurablePath {
    path(format!("local/offices/{office}/security/ca-key.v1.staging"))
}

/// The clean-stop marker of `office`.
pub fn clean_stop_path(office: &Id) -> DurablePath {
    path(format!("local/offices/{office}/clean-stop"))
}

/// Which file of `office`'s a path is; `None` when it is not this family's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    CaKey,
    CaKeyStaging,
    CleanStop,
}

pub fn kind_of(path: &DurablePath, office: &Id) -> Option<Kind> {
    if *path == ca_key_path(office) {
        return Some(Kind::CaKey);
    }
    if *path == ca_key_staging_path(office) {
        return Some(Kind::CaKeyStaging);
    }
    (*path == clean_stop_path(office)).then_some(Kind::CleanStop)
}

// ---- security/ca-key.v1.json ----

/// The HQ's CA key record. `wrapped_ca_key` is an `org_pki::wrap` record, opaque here: the
/// CA private key never reaches the store in any other form.
#[derive(Clone, PartialEq, Eq)]
pub struct CaKey {
    pub organization_id: Id,
    pub office_id: Id,
    /// The store operation that created the organization: a UUIDv7.
    pub operation_id: Id,
    /// The CA's public key, uncompressed SEC1 (ADR-0007).
    pub public_key: [u8; 65],
    /// The SHA-256 of the escrow package, `state/security/ca-escrow.v1.bin`.
    pub package_sha256: Sha256,
    pub wrapped_ca_key: Vec<u8>,
}

impl fmt::Debug for CaKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CaKey")
            .field("organization_id", &self.organization_id)
            .field("office_id", &self.office_id)
            .field("operation_id", &self.operation_id)
            .field("package_sha256", &self.package_sha256)
            .finish_non_exhaustive()
    }
}

impl CaKey {
    /// The rules one record can break.
    pub fn check(&self) -> Result<(), Malformed> {
        let ids = [&self.organization_id, &self.office_id, &self.operation_id];
        if !ids.iter().all(|id| org_proto::is_uuid_v7(id.as_str())) {
            return Err(Malformed("every ID is a UUIDv7 in lowercase text"));
        }
        if self.public_key[0] != 4 {
            return Err(Malformed("the public key is not an uncompressed SEC1 key"));
        }
        let wrapped = MIN_WRAPPED_CA_KEY..=MAX_WRAPPED_CA_KEY;
        if !wrapped.contains(&self.wrapped_ca_key.len()) {
            return Err(Malformed(
                "the wrapped CA key is not a wrapped key's length",
            ));
        }
        Ok(())
    }
}

fn hex(bytes: &[u8]) -> String {
    use fmt::Write;
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(text, "{byte:02x}").expect("writing to a string");
    }
    text
}

/// Lowercase hex only, an even number of digits.
fn unhex(text: &str) -> Option<Vec<u8>> {
    let digit = |byte: u8| match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    };
    let (pairs, rest) = text.as_bytes().as_chunks::<2>();
    if !rest.is_empty() {
        return None;
    }
    let decode = |pair: &[u8; 2]| Some(digit(pair[0])? << 4 | digit(pair[1])?);
    pairs.iter().map(decode).collect()
}

pub fn encode_ca_key(record: &CaKey) -> Vec<u8> {
    format!(
        concat!(
            r#"{{"schema_version":{},"organization_id":"{}","office_id":"{}","#,
            r#""operation_id":"{}","escrow_version":{},"public_key":"{}","#,
            r#""package_sha256":"{}","wrapped_ca_key":"{}"}}"#,
            "\n"
        ),
        CA_KEY_SCHEMA_VERSION,
        record.organization_id,
        record.office_id,
        record.operation_id,
        ESCROW_VERSION,
        hex(&record.public_key),
        record.package_sha256.to_hex(),
        hex(&record.wrapped_ca_key),
    )
    .into_bytes()
}

/// The record's fields as the file holds them.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CaKeyFields {
    schema_version: u64,
    organization_id: String,
    office_id: String,
    operation_id: String,
    escrow_version: u64,
    public_key: String,
    package_sha256: String,
    wrapped_ca_key: String,
}

pub fn decode_ca_key(bytes: &[u8]) -> Result<CaKey, Malformed> {
    let fields: CaKeyFields =
        serde_json::from_slice(bytes).map_err(|_| Malformed("not a CA key record"))?;
    if fields.schema_version != CA_KEY_SCHEMA_VERSION {
        return Err(Malformed("schema_version is not 1"));
    }
    if fields.escrow_version != ESCROW_VERSION {
        return Err(Malformed("escrow_version is not 1"));
    }
    let id = |text: &str| Id::parse(text).map_err(|_| Malformed("an ID is not an ID"));
    let public_key = unhex(&fields.public_key).and_then(|key| key.try_into().ok());
    let public_key = public_key.ok_or(Malformed("public_key is not 65 bytes in hex"))?;
    let package_sha256 = Sha256::from_hex(&fields.package_sha256)
        .ok_or(Malformed("package_sha256 is not a SHA-256 in hex"))?;
    let wrapped_ca_key =
        unhex(&fields.wrapped_ca_key).ok_or(Malformed("wrapped_ca_key is not hex"))?;
    let record = CaKey {
        organization_id: id(&fields.organization_id)?,
        office_id: id(&fields.office_id)?,
        operation_id: id(&fields.operation_id)?,
        public_key,
        package_sha256,
        wrapped_ca_key,
    };
    record.check()?;
    // Key order, spacing, case and the final newline: the one form.
    if encode_ca_key(&record) != bytes {
        return Err(Malformed("not in the record's one form"));
    }
    Ok(record)
}

// ---- clean-stop ----

/// Where the journal stood when the office stopped in order.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CleanStop {
    pub journal_incarnation: Id,
    /// The ordinal of the journal's head, flushed, materialized and confirmed.
    pub head_ordinal: u64,
}

pub fn encode_clean_stop(record: &CleanStop) -> Vec<u8> {
    format!(
        "{{\"journal_incarnation\":\"{}\",\"head_ordinal\":{}}}\n",
        record.journal_incarnation, record.head_ordinal
    )
    .into_bytes()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CleanStopFields {
    journal_incarnation: String,
    head_ordinal: u64,
}

pub fn decode_clean_stop(bytes: &[u8]) -> Result<CleanStop, Malformed> {
    let fields: CleanStopFields =
        serde_json::from_slice(bytes).map_err(|_| Malformed("not a clean-stop marker"))?;
    let journal_incarnation = Id::parse(&fields.journal_incarnation)
        .map_err(|_| Malformed("journal_incarnation is not an ID"))?;
    let record = CleanStop {
        journal_incarnation,
        head_ordinal: fields.head_ordinal,
    };
    if encode_clean_stop(&record) != bytes {
        return Err(Malformed("not in the marker's one form"));
    }
    Ok(record)
}

#[cfg(test)]
mod tests;
