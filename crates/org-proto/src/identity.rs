//! Canonical identity syntax; validation grants no authority.

/// Whether `id` is a canonical lowercase RFC 9562 UUIDv7 string.
pub fn is_uuid_v7(id: &str) -> bool {
    let bytes = id.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => *b == b'-',
            14 => *b == b'7',
            19 => matches!(b, b'8' | b'9' | b'a' | b'b'),
            _ => b.is_ascii_digit() || (b'a'..=b'f').contains(b),
        })
}

/// Extracts the 48-bit Unix millisecond timestamp from a canonical lowercase UUIDv7.
pub fn uuid_v7_time_ms(id: &str) -> Option<u64> {
    if !is_uuid_v7(id) {
        return None;
    }
    let bytes = id.as_bytes();
    let mut ms: u64 = 0;
    for &i in &[0, 1, 2, 3, 4, 5, 6, 7, 9, 10, 11, 12] {
        let nibble = match bytes[i] {
            b'0'..=b'9' => (bytes[i] - b'0') as u64,
            b'a'..=b'f' => (bytes[i] - b'a' + 10) as u64,
            _ => return None,
        };
        ms = (ms << 4) | nibble;
    }
    Some(ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_uuid_v7_unix_ms_timestamp() {
        assert_eq!(
            uuid_v7_time_ms("00000000-0000-7000-8000-000000000000"),
            Some(0)
        );
        assert_eq!(
            uuid_v7_time_ms("ffffffff-ffff-7fff-bfff-ffffffffffff"),
            Some(0xffff_ffff_ffff)
        );
        // 0x01563e3ab5d3 == 1469922850259 ms
        assert_eq!(
            uuid_v7_time_ms("01563e3a-b5d3-7d67-9931-87bee64c0af5"),
            Some(1_469_922_850_259)
        );
        assert_eq!(uuid_v7_time_ms("01ARZ3NDEKTSV4RRFFQ69G5FAV"), None);
        assert_eq!(
            uuid_v7_time_ms("01563E3A-B5D3-7D67-9931-87BEE64C0AF5"),
            None
        );
    }
}
