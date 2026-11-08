//! Client-made ids: ULIDs (§2.3), and the short id derived from one (§1.1).

/// Crockford's Base32 alphabet, upper case, as ULIDs are written.
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// The length of a ULID.
pub const ULID_LEN: usize = 26;

/// The length of a short id (§1.1).
pub const SHORT_ID_LEN: usize = 6;

/// The Unix milliseconds a ULID was made at, or `None` if `id` isn't a ULID in its canonical
/// form (upper case, 26 characters, the first `0` to `7`): the shared rule of `org-proto`, so
/// a `change_id` and a `command_id` are judged alike.
pub use org_proto::ulid_time_ms;

/// Whether `id` is a ULID in its canonical form (see [`ulid_time_ms`]).
pub fn is_ulid(id: &str) -> bool {
    ulid_time_ms(id).is_some()
}

/// An id's short form: its last 6 characters, lower case (`3k9f2x`).
pub fn short_id(id: &str) -> String {
    let chars: Vec<char> = id.chars().collect();
    let start = chars.len().saturating_sub(SHORT_ID_LEN);
    chars[start..].iter().collect::<String>().to_lowercase()
}

/// A ULID made at `ms` whose random part is `n` (deterministic, for tests and fakes).
pub fn ulid_from_parts(ms: u64, n: u128) -> String {
    let mut value = (u128::from(ms & 0xFFFF_FFFF_FFFF) << 80) | (n & ((1u128 << 80) - 1));
    let mut out = [0u8; ULID_LEN];
    for slot in out.iter_mut().rev() {
        *slot = CROCKFORD[(value & 0x1F) as usize];
        value >>= 5;
    }
    String::from_utf8(out.to_vec()).expect("Crockford alphabet is ASCII")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_time_of_canonical_ulids_only() {
        assert_eq!(
            ulid_time_ms("01ARZ3NDEKTSV4RRFFQ69G5FAV"),
            Some(1_469_922_850_259)
        );
        assert_eq!(ulid_time_ms(&ulid_from_parts(42, 7)), Some(42));
        assert_eq!(
            ulid_time_ms(&ulid_from_parts(0xFFFF_FFFF_FFFF, u128::MAX)),
            Some(0xFFFF_FFFF_FFFF)
        );
        // Lower case, a letter outside the alphabet, the wrong length, an overflow.
        assert_eq!(ulid_time_ms("01arz3ndektsv4rrffq69g5fav"), None);
        assert_eq!(ulid_time_ms("01ARZ3NDEKTSV4RRFFQ69G5FAU"), None);
        assert_eq!(ulid_time_ms("01ARZ3NDEKTSV4RRFFQ69G5FA"), None);
        assert_eq!(ulid_time_ms("81ARZ3NDEKTSV4RRFFQ69G5FAV"), None);
        assert_eq!(ulid_time_ms(""), None);
    }

    #[test]
    fn short_id_is_the_last_six_lower_case() {
        assert_eq!(short_id("01ARZ3NDEKTSV4RRFFQ69G5FAV"), "9g5fav");
        assert_eq!(short_id("abc"), "abc");
    }
}
