//! Ids the office makes itself, deterministically, so a retry or a replay of the same
//! decision commits the same message (an idempotent `commit_message`, `inserted: false`).

use sha2::{Digest, Sha256};

use org_proto::msg::MessageHeader;
use org_proto::ulid_time_ms;

use crate::parse_utc;

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// A canonical ULID (26 Crockford base32 characters): a 48-bit millisecond time (higher
/// bits dropped) and an 80-bit random part.
pub(crate) fn encode_ulid(time_ms: u64, random: [u8; 10]) -> String {
    let mut value = u128::from(time_ms & 0xFFFF_FFFF_FFFF) << 80;
    for (index, byte) in random.into_iter().enumerate() {
        value |= u128::from(byte) << (8 * (9 - index));
    }
    (0..26)
        .map(|index| char::from(CROCKFORD[((value >> (5 * (25 - index))) & 31) as usize]))
        .collect()
}

/// The id of the `delivery_failed` notice for `original`: its time part is the original's
/// deadline (`expires_at`; else the original id's own time), its random part the first 10
/// bytes of `sha256("delivery_failed:" + original id)`. One original, one notice id.
pub(crate) fn delivery_failed_id(original: &MessageHeader) -> String {
    let time_ms = original
        .expires_at
        .as_deref()
        .and_then(parse_utc)
        .or_else(|| ulid_time_ms(&original.id))
        .unwrap_or(0);
    let digest = Sha256::new()
        .chain_update(b"delivery_failed:")
        .chain_update(original.id.as_bytes())
        .finalize();
    let mut random = [0; 10];
    random.copy_from_slice(&digest[..10]);
    encode_ulid(time_ms, random)
}

#[cfg(test)]
mod tests {
    use super::*;
    use org_proto::msg::{MessageLifecycleState, MessagePriority};

    fn request(id: &str, expires_at: Option<&str>) -> MessageHeader {
        MessageHeader {
            id: id.into(),
            thread_id: id.into(),
            reply_to: None,
            from_member_id: "a".into(),
            from_display_name: "A".into(),
            to_member_id: "b".into(),
            to_display_name: "B".into(),
            kind: "request".into(),
            priority: MessagePriority::Urgent,
            urgent: true,
            subject: None,
            created_at: "2026-11-08T12:00:00.000Z".into(),
            expires_at: expires_at.map(Into::into),
            ttl_ms: Some(120_000),
            body_bytes: 2,
            meta_bytes: 0,
            local_seq: 1,
            state: MessageLifecycleState::Unread,
            triage_gen: 0,
            snoozed_until: None,
            snooze_after_task: false,
            snooze_note: None,
            read_at: None,
            expired_waiting_person: false,
        }
    }

    #[test]
    fn delivery_failed_id_is_deterministic_and_timed_at_the_deadline() {
        let deadline = "2026-11-08T12:02:00.000Z";
        let original = request("01JCMSG0000000000000000001", Some(deadline));
        let id = delivery_failed_id(&original);
        assert_eq!(id, delivery_failed_id(&original.clone()));
        assert_eq!(ulid_time_ms(&id), parse_utc(deadline));
        let digest = Sha256::digest(b"delivery_failed:01JCMSG0000000000000000001");
        let mut random = [0; 10];
        random.copy_from_slice(&digest[..10]);
        assert_eq!(id, encode_ulid(parse_utc(deadline).unwrap(), random));
        let other = request("01JCMSG0000000000000000002", Some(deadline));
        assert_ne!(delivery_failed_id(&other), id);
        // No deadline: the original id's own time.
        let undated = request("01ARZ3NDEKTSV4RRFFQ69G5FAV", None);
        assert_eq!(
            ulid_time_ms(&delivery_failed_id(&undated)),
            Some(1_469_922_850_259)
        );
    }

    #[test]
    fn encodes_a_canonical_ulid() {
        // The ULID spec's example: time 1469922850259, random part all ones -> "Z"s.
        let id = encode_ulid(1_469_922_850_259, [0xFF; 10]);
        assert_eq!(id, "01ARZ3NDEKZZZZZZZZZZZZZZZZ");
        assert_eq!(ulid_time_ms(&id), Some(1_469_922_850_259));
        assert_eq!(encode_ulid(0, [0; 10]), "00000000000000000000000000");
        assert_eq!(encode_ulid(1, [0; 10]), "00000000010000000000000000");
        let max = encode_ulid(0xFFFF_FFFF_FFFF, [0xFF; 10]);
        assert_eq!(max, "7ZZZZZZZZZZZZZZZZZZZZZZZZZ");
        assert_eq!(ulid_time_ms(&max), Some(0xFFFF_FFFF_FFFF));
        // The last random byte is the last 8 bits: 0x01 sets only the lowest bit.
        assert_eq!(
            encode_ulid(0, [0, 0, 0, 0, 0, 0, 0, 0, 0, 1]),
            "00000000000000000000000001"
        );
        assert_eq!(
            encode_ulid(0, [0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            "0000000000G000000000000000"
        );
    }
}
