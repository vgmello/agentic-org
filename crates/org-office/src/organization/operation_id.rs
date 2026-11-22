//! The store operation ID of a wire command ID.
//!
//! The native protocol's command IDs are client-made uppercase Crockford ULIDs
//! (`org_proto::org`, "Command ids and retries"; `HqCreationV1.operation_id`). Everything
//! on disk names an operation by a UUIDv7 in lowercase text (data contract document 06): the
//! receipt, the journal entry's operation and the CA key record. This is the one mapping
//! between the two, used for every command an office accepts from the wire, so the same
//! ULID always names the same store operation. The ID slice removes it once the wire
//! carries UUIDv7 itself.
//!
//! The layout keeps the ULID's 48-bit timestamp as the UUID's `unix_ts_ms` and sets the
//! version and variant bits; the ULID's 80 random bits r79..r0 fill `rand_a` (r79..r68) and
//! `rand_b` (r67..r6), and r5..r0 are dropped:
//!
//! ```text
//! UUIDv7 = unix_ts_ms (48) ‖ 0b0111 ‖ r79..r68 (12) ‖ 0b10 ‖ r67..r6 (62)
//! ```
//!
//! Two ULIDs that differ only in their last six random bits name one operation. A client
//! that reuses a timestamp and 74 random bits with another command is answered as for a
//! reused command ID: the store matches the receipt on the command's fingerprint.

use std::fmt;

use org_store::Id;

/// A command ID that is not a ULID in its canonical form.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NotAWireUlid;

impl fmt::Display for NotAWireUlid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a command ID is a ULID: 26 uppercase Crockford base32 characters")
    }
}

impl std::error::Error for NotAWireUlid {}

/// Crockford's base32 alphabet, upper case, as ULIDs are written.
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// The store operation ID of the wire command ID `ulid`. Refused unless `ulid` is a ULID in
/// its canonical form ([`org_proto::ulid_time_ms`]): one ULID has one spelling.
pub fn from_wire_ulid(ulid: &str) -> Result<Id, NotAWireUlid> {
    if org_proto::ulid_time_ms(ulid).is_none() {
        return Err(NotAWireUlid);
    }
    // 26 digits of 5 bits; the first is at most 7, so the value fits 128 bits.
    let mut value: u128 = 0;
    for byte in ulid.bytes() {
        let digit = CROCKFORD
            .iter()
            .position(|&d| d == byte)
            .ok_or(NotAWireUlid)?;
        value = (value << 5) | digit as u128;
    }
    let timestamp = value >> 80;
    let random = value & ((1u128 << 80) - 1);
    let rand_a = random >> 68;
    let rand_b = (random >> 6) & ((1u128 << 62) - 1);
    let uuid = (timestamp << 80) | (0x7 << 76) | (rand_a << 64) | (0b10 << 62) | rand_b;
    let hex = format!("{uuid:032x}");
    let text = format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    );
    debug_assert!(org_proto::is_uuid_v7(&text));
    Id::parse(&text).map_err(|_| NotAWireUlid)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Worked out by hand from the layout in the module docs, and checked with an
    /// independent implementation (Python's `uuid`): version 7, RFC 9562 variant.
    const GOLDEN: [(&str, &str); 6] = [
        // Every bit zero: only the version and variant bits are set.
        (
            "00000000000000000000000000",
            "00000000-0000-7000-8000-000000000000",
        ),
        // Every bit one: the dropped six bits do not show.
        (
            "7ZZZZZZZZZZZZZZZZZZZZZZZZZ",
            "ffffffff-ffff-7fff-bfff-ffffffffffff",
        ),
        // A timestamp with an all-zero random part.
        (
            "01ARZ3NDEK0000000000000000",
            "01563e3a-b5d3-7000-8000-000000000000",
        ),
        // A zero timestamp with an all-ones random part.
        (
            "0000000000ZZZZZZZZZZZZZZZZ",
            "00000000-0000-7fff-bfff-ffffffffffff",
        ),
        // The ULID specification's example.
        (
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "01563e3a-b5d3-7d67-9931-87bee64c0af5",
        ),
        (
            "01HZY8M6V2Q3D4E5F6G7H8J9K0",
            "018ffc8a-1b62-7b8d-a91c-579a078a2499",
        ),
    ];

    #[test]
    fn a_wire_ulid_maps_to_the_pinned_uuidv7() {
        for (ulid, uuid) in GOLDEN {
            let mapped = from_wire_ulid(ulid).expect("a ULID");
            assert_eq!(mapped.as_str(), uuid, "{ulid}");
            assert!(org_proto::is_uuid_v7(mapped.as_str()), "{ulid}");
            // The same ULID always maps to the same ID.
            assert_eq!(from_wire_ulid(ulid), Ok(mapped), "{ulid}");
        }
    }

    #[test]
    fn the_timestamp_is_kept_and_only_the_last_six_random_bits_are_dropped() {
        let ulid = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        let mapped = from_wire_ulid(ulid).unwrap();
        let time = org_proto::ulid_time_ms(ulid).unwrap();
        let first: String = mapped.as_str().replace('-', "")[..12].to_owned();
        assert_eq!(u64::from_str_radix(&first, 16), Ok(time));
        // The last character holds five random bits, the one before it the sixth (its
        // lowest bit): changing them gives the same ID; the seventh bit does not.
        let same = ["01ARZ3NDEKTSV4RRFFQ69G5FA0", "01ARZ3NDEKTSV4RRFFQ69G5FAZ"];
        let same_ids: Vec<Id> = same.iter().map(|u| from_wire_ulid(u).unwrap()).collect();
        assert!(same_ids.iter().all(|id| *id == mapped), "{same_ids:?}");
        assert_eq!(
            from_wire_ulid("01ARZ3NDEKTSV4RRFFQ69G5FBV"),
            Ok(mapped.clone())
        );
        assert_ne!(from_wire_ulid("01ARZ3NDEKTSV4RRFFQ69G5FCV"), Ok(mapped));
    }

    #[test]
    fn anything_but_a_canonical_ulid_is_refused() {
        for bad in [
            "",
            "01arz3ndektsv4rrffq69g5fav",
            "01ARZ3NDEKTSV4RRFFQ69G5FAU",
            "01ARZ3NDEKTSV4RRFFQ69G5FA",
            "01ARZ3NDEKTSV4RRFFQ69G5FAVX",
            "81ARZ3NDEKTSV4RRFFQ69G5FAV",
            "01563e3a-b5d3-7d67-9931-87bee64c0af5",
        ] {
            assert_eq!(from_wire_ulid(bad), Err(NotAWireUlid), "{bad:?}");
        }
    }
}
