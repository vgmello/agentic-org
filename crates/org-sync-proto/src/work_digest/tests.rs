use super::*;
use crate::Epoch;

fn bytes(hex: &str) -> Vec<u8> {
    assert!(hex.len().is_multiple_of(2));
    hex.as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

#[test]
fn independent_python_work_record_and_source_known_answers() {
    let rows: serde_json::Value =
        serde_json::from_str(include_str!("../../test-vectors/work-digest-v1.json")).unwrap();
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    for row in rows {
        let codec = Codec::new(
            row["codec"].as_str().unwrap(),
            row["codec_version"].as_u64().unwrap().try_into().unwrap(),
        )
        .unwrap();
        let record = work_record_digest_v1(
            row["record"].as_str().unwrap(),
            row["generation"].as_u64().unwrap(),
            row["version"].as_u64().unwrap(),
            &codec,
            &bytes(row["payload_hex"].as_str().unwrap()),
        )
        .unwrap();
        let expected = bytes(row["record_digest_hex"].as_str().unwrap());
        assert_eq!(record.as_bytes().as_slice(), expected);
        assert_eq!(
            Sha256::digest(bytes(row["record_preimage_hex"].as_str().unwrap())).as_slice(),
            expected
        );
        let source = work_source_digest_v1(
            &OfficeId::new(row["office"].as_str().unwrap()).unwrap(),
            &OriginIncarnation::new(row["incarnation"].as_str().unwrap()).unwrap(),
            SourceSeq::new(row["seq"].as_u64().unwrap()).unwrap(),
            record,
            Position {
                epoch: Epoch::new(row["epoch"].as_u64().unwrap()).unwrap(),
                seq: row["org_seq"].as_u64().unwrap(),
            },
            row["created"].as_u64().unwrap(),
        );
        let expected = bytes(row["source_digest_hex"].as_str().unwrap());
        assert_eq!(source.as_bytes().as_slice(), expected);
        assert_eq!(
            Sha256::digest(bytes(row["source_preimage_hex"].as_str().unwrap())).as_slice(),
            expected
        );
    }
}

#[test]
fn work_hash_bounds_lengths_without_allocating_oversize_inputs() {
    let codec = Codec::new("org.native.changes", 1).unwrap();
    assert_eq!(encoded_lengths(0, 0), Ok((0, 0)));
    assert_eq!(
        encoded_lengths(u128::from(u32::MAX), u128::from(u64::MAX)),
        Ok((u32::MAX, u64::MAX))
    );
    assert_eq!(
        encoded_lengths(u128::from(u32::MAX) + 1, 1),
        Err(InvalidValue)
    );
    assert_eq!(
        encoded_lengths(1, u128::from(u64::MAX) + 1),
        Err(InvalidValue)
    );
    // Hash helpers frame bytes; queue policy owns record-ID and version rules.
    assert!(work_record_digest_v1("", 0, 0, &codec, b"").is_ok());
    assert!(work_record_digest_v1(&"x".repeat(129), 1, 1, &codec, b"event").is_ok());
}

#[test]
fn every_record_field_is_bound_and_independent_of_source_identity() {
    let codec = Codec::new("org.native.changes", 1).unwrap();
    let base = work_record_digest_v1("TASK1", 1, 2, &codec, b"event").unwrap();
    for (id, generation, version, codec, payload) in [
        ("TASK2", 1, 2, codec.clone(), b"event".as_slice()),
        ("TASK1", 2, 2, codec.clone(), b"event".as_slice()),
        ("TASK1", 1, 3, codec.clone(), b"event".as_slice()),
        (
            "TASK1",
            1,
            2,
            Codec::new("org.native.snapshot", 1).unwrap(),
            b"event".as_slice(),
        ),
        (
            "TASK1",
            1,
            2,
            Codec::new("org.native.changes", 2).unwrap(),
            b"event".as_slice(),
        ),
        ("TASK1", 1, 2, codec.clone(), b"changed".as_slice()),
    ] {
        assert_ne!(
            work_record_digest_v1(id, generation, version, &codec, payload).unwrap(),
            base
        );
    }
    let position = Position {
        epoch: Epoch::new(1).unwrap(),
        seq: 4,
    };
    let first = work_source_digest_v1(
        &OfficeId::new("A").unwrap(),
        &OriginIncarnation::new("BC").unwrap(),
        SourceSeq::new(1).unwrap(),
        base,
        position,
        100,
    );
    let second = work_source_digest_v1(
        &OfficeId::new("AB").unwrap(),
        &OriginIncarnation::new("C").unwrap(),
        SourceSeq::new(1).unwrap(),
        base,
        position,
        100,
    );
    assert_ne!(first, second); // equal concatenation, different length-prefixed fields
    assert_eq!(
        work_record_digest_v1("TASK1", 1, 2, &codec, b"event").unwrap(),
        base
    );
}

#[test]
fn every_source_field_is_bound_to_the_record_digest() {
    let office = OfficeId::new("OFF1").unwrap();
    let incarnation = OriginIncarnation::new("INC1").unwrap();
    let seq = SourceSeq::new(1).unwrap();
    let record = Digest::from_bytes([7; 32]);
    let position = Position {
        epoch: Epoch::new(1).unwrap(),
        seq: 4,
    };
    let base = work_source_digest_v1(&office, &incarnation, seq, record, position, 100);
    for case in 0..7 {
        let mut o = office.clone();
        let mut i = incarnation.clone();
        let mut s = seq;
        let mut r = record;
        let mut p = position;
        let mut created = 100;
        match case {
            0 => o = OfficeId::new("OTHER").unwrap(),
            1 => i = OriginIncarnation::new("INC2").unwrap(),
            2 => s = SourceSeq::new(2).unwrap(),
            3 => r = Digest::ZERO,
            4 => p.epoch = Epoch::new(2).unwrap(),
            5 => p.seq += 1,
            _ => created += 1,
        }
        assert_ne!(work_source_digest_v1(&o, &i, s, r, p, created), base);
    }
}
