use crate::{Digest, Epoch, OfficeId, OrgId, Position};

/// Canonical bytes signed for a v1 organization authority record. The signature covers
/// the pinned identities/CA, new authority and complete restored event position/digest.
/// IDs are length-prefixed UTF-8; integers are big endian; digests have fixed width.
/// Signature bytes are excluded. Validated IDs bound this allocation to fewer than 256 bytes.
pub fn authority_record_preimage_v1(
    org: &OrgId,
    hq: &OfficeId,
    ca: Digest,
    epoch: Epoch,
    restored_position: Position,
    restored_digest: Digest,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(256);
    bytes.extend_from_slice(b"agentic-org/org-authority/v1\0");
    for id in [org.as_str(), hq.as_str()] {
        let len = u32::try_from(id.len()).expect("validated IDs are at most 64 bytes");
        bytes.extend_from_slice(&len.to_be_bytes());
        bytes.extend_from_slice(id.as_bytes());
    }
    bytes.extend_from_slice(ca.as_bytes());
    bytes.extend_from_slice(&epoch.get().to_be_bytes());
    bytes.extend_from_slice(&restored_position.epoch.get().to_be_bytes());
    bytes.extend_from_slice(&restored_position.seq.to_be_bytes());
    bytes.extend_from_slice(restored_digest.as_bytes());
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest as _, Sha256};

    fn bytes(hex: &str) -> Vec<u8> {
        hex.as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    #[test]
    fn independent_python_authority_preimage_binds_the_entire_restore_base() {
        let row: serde_json::Value =
            serde_json::from_str(include_str!("../test-vectors/authority-record-v1.json")).unwrap();
        let org = OrgId::new(row["org_id"].as_str().unwrap()).unwrap();
        let hq = OfficeId::new(row["hq_office_id"].as_str().unwrap()).unwrap();
        let ca = Digest::from_bytes(bytes(row["ca_hex"].as_str().unwrap()).try_into().unwrap());
        let epoch = Epoch::new(row["epoch"].as_u64().unwrap()).unwrap();
        let restored = Position {
            epoch: Epoch::new(row["restored_epoch"].as_u64().unwrap()).unwrap(),
            seq: row["restored_seq"].as_u64().unwrap(),
        };
        let digest = Digest::from_bytes(
            bytes(row["restored_digest_hex"].as_str().unwrap())
                .try_into()
                .unwrap(),
        );
        let actual = authority_record_preimage_v1(&org, &hq, ca, epoch, restored, digest);
        assert_eq!(actual, bytes(row["preimage_hex"].as_str().unwrap()));
        assert_eq!(
            &Sha256::digest(&actual)[..],
            bytes(row["sha256_hex"].as_str().unwrap())
        );
        for changed in [
            authority_record_preimage_v1(
                &OrgId::new("OTHER").unwrap(),
                &hq,
                ca,
                epoch,
                restored,
                digest,
            ),
            authority_record_preimage_v1(
                &org,
                &OfficeId::new("OTHER").unwrap(),
                ca,
                epoch,
                restored,
                digest,
            ),
            authority_record_preimage_v1(&org, &hq, Digest::ZERO, epoch, restored, digest),
            authority_record_preimage_v1(
                &org,
                &hq,
                ca,
                epoch.checked_next().unwrap(),
                restored,
                digest,
            ),
            authority_record_preimage_v1(
                &org,
                &hq,
                ca,
                epoch,
                Position {
                    epoch: restored.epoch.checked_next().unwrap(),
                    ..restored
                },
                digest,
            ),
            authority_record_preimage_v1(
                &org,
                &hq,
                ca,
                epoch,
                Position {
                    seq: restored.seq + 1,
                    ..restored
                },
                digest,
            ),
            authority_record_preimage_v1(&org, &hq, ca, epoch, restored, Digest::ZERO),
        ] {
            assert_ne!(actual, changed);
        }
    }
}
