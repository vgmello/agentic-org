//! Private selected-writer control over a host-transferred connected descriptor.
//! This is not a task route. Only the admitted host may supply the binding/roots.
use org_proto::work::{JournalPosition, WriterIdentity};
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, Read, Write};

pub const VERSION: u32 = 1;
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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Admission {
    pub version: u32,
    pub identity: WriterIdentity,
    pub protected_root: String,
    pub public_root: String,
    pub owner_account: String,
    /// Protected security principal; not an employee profile.
    pub owner_member: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Checkpoint {
        identity: WriterIdentity,
    },
    Confirm {
        identity: WriterIdentity,
        before: Option<JournalPosition>,
        after: JournalPosition,
    },
    Owner {
        identity: WriterIdentity,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum Response {
    Checkpoint { position: Option<JournalPosition> },
    Confirmed,
    Owner { account: String, member: String },
    Denied,
}
