//! REC-DEL-1 one-shot public escrow exchange. No secret/key holder is serializable
//! here. Peers authenticate the transferred descriptor and compare frozen binding
//! and SHA-256 themselves; this crate has no crypto or domain dependency.
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::io::{self, BufRead, Read, Write};

pub const MAX_FRAME_BYTES: usize = 16 * 1024;
pub const MAX_PACKAGE_BYTES: usize = 4475;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub instance_id: String,
    pub office_id: String,
    pub organization_id: String,
    pub operation_id: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicRecord {
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
pub fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid recovery public record")
}
impl Request {
    pub fn check(&self) -> io::Result<()> {
        if self.version != 1
            || !crate::is_plain_id(&self.instance_id)
            || !crate::is_storage_id(&self.office_id)
            || !crate::is_storage_id(&self.organization_id)
            || self.instance_id.len() > 255
            || self.office_id.len() > 255
            || self.organization_id.len() > 255
            || org_proto::ulid_time_ms(&self.operation_id).is_none()
        {
            return Err(invalid());
        }
        Ok(())
    }
}
impl PublicRecord {
    pub fn binding(&self) -> Request {
        Request {
            version: self.version,
            instance_id: self.instance_id.clone(),
            office_id: self.office_id.clone(),
            organization_id: self.organization_id.clone(),
            operation_id: self.operation_id.clone(),
        }
    }
    /// Structural envelope validation only. Digest and expected identities must
    /// also match the authenticated protected provisioning context at consumers.
    pub fn package(&self) -> io::Result<Vec<u8>> {
        self.binding().check()?;
        if self.escrow_version != 1 || !hex_of_len(&self.package_sha256, 32) {
            return Err(invalid());
        }
        let key = decode_hex(&self.public_key, 65)?;
        if key.len() != 65 || key[0] != 4 {
            return Err(invalid());
        }
        let bytes = decode_hex(&self.package_hex, MAX_PACKAGE_BYTES)?;
        validate_package(&bytes, self.organization_id.as_bytes(), &key)?;
        Ok(bytes)
    }
}

fn validate_package(bytes: &[u8], org: &[u8], key: &[u8]) -> io::Result<()> {
    if bytes.len() < 10 || &bytes[..8] != b"AGORGCA1" {
        return Err(invalid());
    }
    let org_len = usize::from(u16::from_be_bytes([bytes[8], bytes[9]]));
    let header = 108usize.checked_add(org_len).ok_or_else(invalid)?;
    if org_len == 0
        || org_len > 255
        || bytes.len() < header
        || bytes.get(10..10 + org_len) != Some(org)
        || bytes[10 + org_len] != 1
        || bytes.get(39 + org_len..104 + org_len) != Some(key)
    {
        return Err(invalid());
    }
    validate_ciphertext_length(bytes, org_len, header)
}

fn validate_ciphertext_length(bytes: &[u8], org_len: usize, header: usize) -> io::Result<()> {
    let length = u32::from_be_bytes(
        bytes[104 + org_len..header]
            .try_into()
            .map_err(|_| invalid())?,
    ) as usize;
    if !(17..=4112).contains(&length) || bytes.len() != header + length {
        return Err(invalid());
    }
    Ok(())
}
fn hex_of_len(s: &str, len: usize) -> bool {
    s.len() == len * 2
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(out, "{byte:02x}").expect("string write");
    }
    out
}
fn decode_hex(s: &str, max: usize) -> io::Result<Vec<u8>> {
    if s.is_empty()
        || s.len() > max * 2
        || !s.len().is_multiple_of(2)
        || !hex_of_len(s, s.len() / 2)
    {
        return Err(invalid());
    }
    fn digit(b: u8) -> u8 {
        if b <= b'9' {
            b - b'0'
        } else {
            b - b'a' + 10
        }
    }
    Ok(s.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| (digit(p[0]) << 4) | digit(p[1]))
        .collect())
}
fn read<T: DeserializeOwned>(reader: &mut impl BufRead) -> io::Result<T> {
    let mut bytes = Vec::with_capacity(MAX_FRAME_BYTES);
    reader
        .take((MAX_FRAME_BYTES + 1) as u64)
        .read_until(b'\n', &mut bytes)?;
    if bytes.len() > MAX_FRAME_BYTES || bytes.last() != Some(&b'\n') || bytes.first() != Some(&b'{')
    {
        return Err(invalid());
    }
    let mut extra = [0];
    if reader.read(&mut extra)? != 0 {
        return Err(invalid());
    }
    serde_json::from_slice(&bytes).map_err(|_| invalid())
}
fn write(writer: &mut impl Write, value: &impl Serialize) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| invalid())?;
    bytes.push(b'\n');
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(invalid());
    }
    writer.write_all(&bytes)?;
    writer.flush()
}
pub fn read_request(reader: &mut impl BufRead) -> io::Result<Request> {
    let request: Request = read(reader)?;
    request.check()?;
    Ok(request)
}
pub fn write_request(writer: &mut impl Write, request: &Request) -> io::Result<()> {
    request.check()?;
    write(writer, request)
}
pub fn read_record(reader: &mut impl BufRead) -> io::Result<PublicRecord> {
    let record: PublicRecord = read(reader)?;
    record.package()?;
    Ok(record)
}
pub fn write_record(writer: &mut impl Write, record: &PublicRecord) -> io::Result<()> {
    record.package()?;
    write(writer, record)
}

#[cfg(test)]
mod tests;
