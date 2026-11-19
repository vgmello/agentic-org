//! Private writer-custody control over a host-transferred connected descriptor (version 2).
//! This is not a task route. Only the admitted host may supply the identity and the roots.
//!
//! The host keeps, outside the instance root, one record per office: the writer
//! [`Identity`] and the last journal [`Head`] the office confirmed. The office's store
//! (`org_store::WriterAuthority`) asks for that head once at opening ([`Request::Checkpoint`])
//! and confirms its flushed head once per group flush ([`Request::Confirm`]). The host applies
//! [`confirms`] to each confirmation. The types carry exactly what the store's
//! `OfficeIdentity` and `JournalHead` hold, in text form: every ID is canonical lowercase
//! text and every hash lowercase hex, so the worker converts them without reformatting.
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, Read, Write};
use std::path::{Component, Path};

pub const VERSION: u32 = 2;
pub const MAX_BYTES: usize = 16 * 1024;

pub fn read<T: serde::de::DeserializeOwned>(reader: &mut impl BufRead) -> io::Result<Option<T>> {
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take((MAX_BYTES + 1) as u64)
        .read_until(b'\n', &mut bytes)?;
    if bytes.is_empty() {
        return Ok(None);
    }
    if bytes.pop() != Some(b'\n') {
        return Err(io::Error::other("invalid writer control frame"));
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| io::Error::other("invalid writer control frame"))
}
pub fn write(writer: &mut impl Write, message: &impl Serialize) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(message)
        .map_err(|_| io::Error::other("invalid writer control frame"))?;
    if bytes.len() > MAX_BYTES {
        return Err(io::Error::other("writer control frame too large"));
    }
    bytes.push(b'\n');
    writer.write_all(&bytes)?;
    writer.flush()
}

fn is_lower_hex(text: &str, len: usize) -> bool {
    text.len() == len
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// One writer of one journal incarnation of one office: the store's `OfficeIdentity`.
/// The host mints it once, at the protected reservation, and never changes it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    /// The registered instance: canonical lowercase UUIDv7.
    pub instance_id: String,
    /// Canonical lowercase UUIDv7.
    pub organization_id: String,
    /// Canonical lowercase UUIDv7.
    pub office_id: String,
    /// The journal incarnation this writer appends to: canonical lowercase UUIDv7.
    pub journal_incarnation: String,
    /// At least 1. A new epoch is a new writer; the host never changes it in place.
    pub writer_epoch: u64,
    /// The host's device-local custody identity: 64 lowercase hex digits.
    pub machine_id: String,
}

impl Identity {
    /// Every field in its contract form: valid store IDs, never reformatted.
    pub fn check(&self) -> bool {
        [
            &self.instance_id,
            &self.organization_id,
            &self.office_id,
            &self.journal_incarnation,
        ]
        .iter()
        .all(|id| org_proto::is_uuid_v7(id))
            && self.writer_epoch >= 1
            && is_lower_hex(&self.machine_id, 64)
    }
}

/// How far each stream's positions have been taken: the store's `HighWaters`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HighWaters {
    pub organization: u64,
    pub work: u64,
    pub message_content: u64,
    pub message_state: u64,
}

impl HighWaters {
    /// Whether no stream here is lower than in `stored`.
    pub fn covers(&self, stored: &Self) -> bool {
        self.organization >= stored.organization
            && self.work >= stored.work
            && self.message_content >= stored.message_content
            && self.message_state >= stored.message_state
    }
}

/// A flushed journal position: the store's `JournalHead`. Ordinal zero is the anchor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Head {
    pub ordinal: u64,
    /// The SHA-256 of the entry (or anchor) at `ordinal`: 64 lowercase hex digits.
    pub sha256: String,
    pub high_waters: HighWaters,
}

impl Head {
    pub fn check(&self) -> bool {
        is_lower_hex(&self.sha256, 64)
    }
}

/// The custody rule for one confirmation (ruling F4), given the head the host stored:
///
/// - `before` is exactly the stored head (none before the first confirmation);
/// - with no stored head, `after` is the anchor (ordinal 0);
/// - otherwise `after.ordinal` is greater than the stored ordinal, or equal with the
///   same head (the same hash, so the same high-waters);
/// - no stream's high-water is lower than the stored one.
///
/// One confirmation may advance many ordinals: the office sends one per group flush.
/// Identity is checked separately, against the stored identity.
pub fn confirms(stored: Option<&Head>, before: Option<&Head>, after: &Head) -> bool {
    if before != stored || !after.check() {
        return false;
    }
    match stored {
        None => after.ordinal == 0,
        Some(stored) => {
            (after.ordinal > stored.ordinal || after == stored)
                && after.high_waters.covers(&stored.high_waters)
        }
    }
}

/// An absolute path in its one written form: plain components only (no `.`, `..`, empty
/// component or trailing separator), no NUL, and not `/`.
fn plain_absolute(text: &str) -> bool {
    let path = Path::new(text);
    path.is_absolute()
        && !text.contains('\0')
        && path.parent().is_some()
        && path
            .components()
            .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
        && path
            .components()
            .collect::<std::path::PathBuf>()
            .as_os_str()
            == path.as_os_str()
}

/// What the host sends first on the control channel. The roots are the organization's
/// data roots, `<instance-root>/<organization-id>/durable` and `.../runtime`; they travel
/// only here, never on a launch line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Admission {
    pub version: u32,
    pub identity: Identity,
    pub durable_root: String,
    pub runtime_root: String,
    pub owner_account: String,
    /// Protected security principal; not an employee profile.
    pub owner_member: String,
}

impl Admission {
    /// The admission's own rules: this version, a valid identity, two separate plain
    /// absolute roots (neither inside the other), an owner account in the host's form (a
    /// ULID) and a valid owner member.
    pub fn check(&self) -> bool {
        let (durable, runtime) = (Path::new(&self.durable_root), Path::new(&self.runtime_root));
        self.version == VERSION
            && self.identity.check()
            && plain_absolute(&self.durable_root)
            && plain_absolute(&self.runtime_root)
            && !durable.starts_with(runtime)
            && !runtime.starts_with(durable)
            && org_proto::ulid_time_ms(&self.owner_account).is_some()
            && org_proto::is_valid_member_id(&self.owner_member)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Checkpoint {
        identity: Identity,
    },
    Confirm {
        identity: Identity,
        before: Option<Head>,
        after: Head,
    },
    Owner {
        identity: Identity,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum Response {
    Checkpoint { head: Option<Head> },
    Confirmed,
    Owner { account: String, member: String },
    Denied,
}

#[cfg(test)]
mod tests;
