use crate::{Codec, Digest, InvalidValue, OrgId, Position};
use sha2::{Digest as _, Sha256};

/// Rowan's frozen event-digest v1 preimage (decision01M73A6MT76KQYF6VXGCSMGBY8).
/// Integers and length prefixes are big endian; no JSON canonicalization is involved.
/// Across an epoch change, previous is the restored HQ's actual last event digest.
/// This function does no allocation proportional to payload length.
pub fn event_digest_v1(
    org: &OrgId,
    position: Position,
    previous: Digest,
    codec: &Codec,
    payload: &[u8],
) -> Result<Digest, InvalidValue> {
    let payload_len = u32::try_from(payload.len()).map_err(|_| InvalidValue)?;
    let mut hash = Sha256::new();
    hash.update(b"agentic-org/org-event/v1\0");
    hash.update((org.as_str().len() as u32).to_be_bytes());
    hash.update(org.as_str().as_bytes());
    hash.update(position.epoch.get().to_be_bytes());
    hash.update(position.seq.to_be_bytes());
    hash.update(previous.as_bytes());
    hash.update((codec.id().len() as u32).to_be_bytes());
    hash.update(codec.id().as_bytes());
    hash.update(codec.version().to_be_bytes());
    hash.update(payload_len.to_be_bytes());
    hash.update(payload);
    Ok(Digest::from_bytes(hash.finalize().into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bytes(hex: &str) -> Vec<u8> {
        assert!(hex.len().is_multiple_of(2));
        hex.as_bytes()
            .chunks(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }
    #[test]
    fn independent_python_known_answer_first_event_and_epoch_change_vectors() {
        let vectors: serde_json::Value =
            serde_json::from_str(include_str!("../test-vectors/event-digest-v1.json")).unwrap();
        let rows = vectors.as_array().unwrap();
        assert_eq!(rows.len(), 3);
        for row in rows {
            let org = OrgId::new(row["org_id"].as_str().unwrap()).unwrap();
            let position = Position {
                epoch: crate::Epoch::new(row["epoch"].as_u64().unwrap()).unwrap(),
                seq: row["seq"].as_u64().unwrap(),
            };
            let previous = Digest::from_bytes(
                bytes(row["previous_hex"].as_str().unwrap())
                    .try_into()
                    .unwrap(),
            );
            let codec = Codec::new(
                row["codec_id"].as_str().unwrap(),
                row["codec_version"].as_u64().unwrap() as u32,
            )
            .unwrap();
            let payload = bytes(row["payload_hex"].as_str().unwrap());
            let actual = event_digest_v1(&org, position, previous, &codec, &payload).unwrap();
            assert_eq!(
                actual.as_bytes().as_slice(),
                bytes(row["digest_hex"].as_str().unwrap()),
                "{}",
                row["name"]
            );
        }
    }
}
