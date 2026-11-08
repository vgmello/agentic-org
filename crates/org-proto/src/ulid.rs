//! Reading the time out of a client-made ULID, the one rule every office uses for a
//! `command_id` ([`crate::org`]) and a `change_id` ([`crate::work`]).

/// Crockford's base32 alphabet, upper case, as ULIDs are written.
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// The length of a ULID, in characters.
const ULID_CHARS: usize = 26;

fn digit(c: u8) -> Option<u64> {
    CROCKFORD.iter().position(|&d| d == c).map(|p| p as u64)
}

/// The Unix milliseconds a ULID was made at, or `None` if `id` isn't a ULID in its canonical
/// form: exactly 26 **upper-case** Crockford base32 characters (no `I`, `L`, `O` or `U`) whose
/// value fits 128 bits, so the first character is `0` to `7`. Strict on purpose: lower case and
/// the look-alike letters a lenient decoder would accept are refused, so one ULID has one
/// spelling (ids are keys, compared as strings).
pub fn ulid_time_ms(id: &str) -> Option<u64> {
    let bytes = id.as_bytes();
    if bytes.len() != ULID_CHARS || !bytes.iter().all(|&b| digit(b).is_some()) {
        return None;
    }
    if digit(bytes[0])? > 7 {
        return None;
    }
    // The first 10 characters are 50 bits; with the first at most 7, the time is 48 bits.
    bytes[..10]
        .iter()
        .try_fold(0u64, |acc, &b| Some((acc << 5) | digit(b)?))
}

#[cfg(test)]
mod tests {
    use super::ulid_time_ms;

    #[test]
    fn reads_the_time_of_a_canonical_ulid() {
        assert_eq!(
            ulid_time_ms("01ARZ3NDEKTSV4RRFFQ69G5FAV"),
            Some(1_469_922_850_259)
        );
        assert_eq!(ulid_time_ms("00000000000000000000000000"), Some(0));
        assert_eq!(ulid_time_ms("0000000001ZZZZZZZZZZZZZZZZ"), Some(1));
        assert_eq!(
            ulid_time_ms("7ZZZZZZZZZZZZZZZZZZZZZZZZZ"),
            Some(0xFFFF_FFFF_FFFF)
        );
    }

    #[test]
    fn refuses_anything_but_the_canonical_form() {
        for bad in [
            "01arz3ndektsv4rrffq69g5fav",  // lower case
            "01ARZ3NDEKTSV4RRFFQ69G5FAv",  // one lower-case letter
            "01ARZ3NDEKTSV4RRFFQ69G5FAU",  // U is not in the alphabet
            "01ARZ3NDEKTSV4RRFFQ69G5FAI",  // nor I
            "01ARZ3NDEKTSV4RRFFQ69G5FAL",  // nor L
            "01ARZ3NDEKTSV4RRFFQ69G5FAO",  // nor O
            "01ARZ3NDEKTSV4RRFFQ69G5FA",   // 25 characters
            "01ARZ3NDEKTSV4RRFFQ69G5FAVV", // 27
            "81ARZ3NDEKTSV4RRFFQ69G5FAV",  // more than 128 bits
            " 01ARZ3NDEKTSV4RRFFQ69G5FA",  // a space
            "01ARZ3NDEKTSV4RRFFQ69G5FÀ",   // 26 bytes, not 26 characters
            "",
        ] {
            assert_eq!(ulid_time_ms(bad), None, "{bad:?}");
        }
    }
}
