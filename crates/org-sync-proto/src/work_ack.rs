//! Cumulative durable-copy receipt, distinct from delivery and transport credit.
use crate::{Digest, InvalidValue, OfficeId, OriginIncarnation};
use sha2::{Digest as _, Sha256};

pub const WORK_CREDIT_WINDOW: usize = 32;
pub fn work_rejection_code_valid(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 64
        && code
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkRejection {
    pub seq: u64,
    pub code: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkAck {
    pub origin_office_id: OfficeId,
    pub incarnation: OriginIncarnation,
    pub from_exclusive: u64,
    pub through_seq: u64,
    pub rejected: Vec<WorkRejection>,
}
impl WorkAck {
    pub fn validate(&self) -> Result<(), InvalidValue> {
        if self.from_exclusive >= self.through_seq || self.rejected.len() > WORK_CREDIT_WINDOW {
            return Err(InvalidValue);
        }
        let mut after = self.from_exclusive;
        for rejection in &self.rejected {
            if rejection.seq <= after
                || rejection.seq > self.through_seq
                || !work_rejection_code_valid(&rejection.code)
            {
                return Err(InvalidValue);
            }
            after = rejection.seq;
        }
        Ok(())
    }
}
/// Rowan's Work ACK v1 preimage. Every scalar is big endian; strings have u32 byte
/// lengths. Validated identifiers/codes/counts fit u32. No JSON normalization.
pub fn work_ack_proof_digest_v1(ack: &WorkAck) -> Result<Digest, InvalidValue> {
    ack.validate()?;
    let mut hash = Sha256::new();
    hash.update(b"agentic-org/work-ack/v1\0");
    for id in [ack.origin_office_id.as_str(), ack.incarnation.as_str()] {
        hash.update((id.len() as u32).to_be_bytes());
        hash.update(id.as_bytes());
    }
    hash.update(ack.from_exclusive.to_be_bytes());
    hash.update(ack.through_seq.to_be_bytes());
    hash.update((ack.rejected.len() as u32).to_be_bytes());
    for rejection in &ack.rejected {
        hash.update(rejection.seq.to_be_bytes());
        hash.update((rejection.code.len() as u32).to_be_bytes());
        hash.update(rejection.code.as_bytes());
    }
    Ok(Digest::from_bytes(hash.finalize().into()))
}

#[cfg(test)]
mod tests;
