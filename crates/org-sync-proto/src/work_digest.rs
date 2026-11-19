//! Frozen Work v1 preimages, shared by native codecs and replication policy.
use crate::{Codec, Digest, InvalidValue, OfficeId, OriginIncarnation, Position, SourceSeq};
use sha2::{Digest as _, Sha256};

fn encoded_lengths(record: u128, payload: u128) -> Result<(u32, u64), InvalidValue> {
    Ok((
        u32::try_from(record).map_err(|_| InvalidValue)?,
        u64::try_from(payload).map_err(|_| InvalidValue)?,
    ))
}

/// SHA-256(domain NUL, length-prefixed record ID, generation, version, length-prefixed
/// codec ID, codec version, u64 payload length, frozen native event bytes). All integers
/// are big endian. Source office/incarnation/sequence are deliberately absent.
pub fn work_record_digest_v1(
    record_id: &str,
    generation: u64,
    version: u64,
    codec: &Codec,
    payload: &[u8],
) -> Result<Digest, InvalidValue> {
    let (record_len, payload_len) =
        encoded_lengths(record_id.len() as u128, payload.len() as u128)?;
    let mut hash = Sha256::new();
    hash.update(b"agentic-org/work-record/v1\0");
    hash.update(record_len.to_be_bytes());
    hash.update(record_id.as_bytes());
    hash.update(generation.to_be_bytes());
    hash.update(version.to_be_bytes());
    hash.update((codec.id().len() as u32).to_be_bytes());
    hash.update(codec.id().as_bytes());
    hash.update(codec.version().to_be_bytes());
    hash.update(payload_len.to_be_bytes());
    hash.update(payload);
    Ok(Digest::from_bytes(hash.finalize().into()))
}

/// SHA-256(domain NUL, u32-length-prefixed office and incarnation, source sequence,
/// record digest, required Org epoch/sequence, created time). Integers are big endian.
pub fn work_source_digest_v1(
    office: &OfficeId,
    incarnation: &OriginIncarnation,
    seq: SourceSeq,
    record_digest: Digest,
    required_org: Position,
    created_ms: u64,
) -> Digest {
    let mut hash = Sha256::new();
    hash.update(b"agentic-org/work-change/v1\0");
    hash.update((office.as_str().len() as u32).to_be_bytes());
    hash.update(office.as_str().as_bytes());
    hash.update((incarnation.as_str().len() as u32).to_be_bytes());
    hash.update(incarnation.as_str().as_bytes());
    hash.update(seq.get().to_be_bytes());
    hash.update(record_digest.as_bytes());
    hash.update(required_org.epoch.get().to_be_bytes());
    hash.update(required_org.seq.to_be_bytes());
    hash.update(created_ms.to_be_bytes());
    Digest::from_bytes(hash.finalize().into())
}

#[cfg(test)]
mod tests;
