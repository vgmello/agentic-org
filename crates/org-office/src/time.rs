//! RFC 3339 UTC timestamps with millisecond precision (`YYYY-MM-DDTHH:MM:SS.mmmZ`), `std` only.

use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn now_rfc3339() -> String {
    format_epoch_ms(now_ms())
}

/// Unix milliseconds now.
pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// How many days `month` (1 to 12) of `year` has, by the Gregorian leap rule.
fn days_in_month(year: i64, month: i64) -> i64 {
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Unix milliseconds of an RFC 3339 time: `YYYY-MM-DDTHH:MM:SS`, an optional fraction (any
/// length, truncated to milliseconds), then `Z` or a `+HH:MM` / `-HH:MM` offset. `None` for
/// anything else (or a time before 1970). Compare times with this, never as text: `...00Z` and
/// `...00.000Z` are the same instant, and an offset changes the instant.
pub(crate) fn parse_rfc3339_ms(text: &str) -> Option<u64> {
    let b = text.as_bytes();
    let digits = |from: usize, len: usize| -> Option<i64> {
        let part = b.get(from..from + len)?;
        part.iter()
            .all(u8::is_ascii_digit)
            .then(|| part.iter().fold(0i64, |n, d| n * 10 + i64::from(d - b'0')))
    };
    let (year, month, day) = (digits(0, 4)?, digits(5, 2)?, digits(8, 2)?);
    let (hour, minute, second) = (digits(11, 2)?, digits(14, 2)?, digits(17, 2)?);
    if b.get(4) != Some(&b'-')
        || b.get(7) != Some(&b'-')
        || !matches!(b.get(10), Some(b'T' | b't'))
        || b.get(13) != Some(&b':')
        || b.get(16) != Some(&b':')
        || !(1..=12).contains(&month)
        || day < 1
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let mut at = 19;
    let mut millis = 0i64;
    if b.get(at) == Some(&b'.') {
        let start = at + 1;
        let mut end = start;
        while b.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        if end == start {
            return None;
        }
        for (place, digit) in b[start..end].iter().take(3).enumerate() {
            millis += i64::from(digit - b'0') * [100, 10, 1][place];
        }
        at = end;
    }
    let offset_minutes = match b.get(at..) {
        Some(b"Z" | b"z") => 0,
        Some([sign @ (b'+' | b'-'), rest @ ..]) if rest.len() == 5 && rest[2] == b':' => {
            let hh = digits(at + 1, 2)?;
            let mm = digits(at + 4, 2)?;
            if hh > 23 || mm > 59 {
                return None;
            }
            let minutes = hh * 60 + mm;
            if *sign == b'+' {
                minutes
            } else {
                -minutes
            }
        }
        _ => return None,
    };
    // Days from the civil date (Howard Hinnant's algorithm, the inverse of `format_epoch_ms`).
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let seconds = days * 86_400 + hour * 3600 + minute * 60 + second - offset_minutes * 60;
    u64::try_from(seconds * 1000 + millis).ok()
}

/// Howard Hinnant's civil-from-days algorithm. The output format matches the agent message
/// envelope's timestamps.
pub(crate) fn format_epoch_ms(epoch_ms: u64) -> String {
    let total_secs = (epoch_ms / 1000) as i64;
    let millis = epoch_ms % 1000;

    let days = total_secs.div_euclid(86_400);
    let secs_of_day = total_secs.rem_euclid(86_400);
    let (hour, minute, second) = (
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60,
    );

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe as i64 + era * 400 + i64::from(month <= 2);

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_known_instants() {
        assert_eq!(format_epoch_ms(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(format_epoch_ms(951_782_400_123), "2000-02-29T00:00:00.123Z");
        assert_eq!(
            format_epoch_ms(1_762_567_890_007),
            "2025-11-08T02:11:30.007Z"
        );
    }

    #[test]
    fn now_has_canonical_shape() {
        let now = now_rfc3339();
        assert_eq!(now.len(), 24);
        assert!(now.ends_with('Z'));
    }

    #[test]
    fn parses_what_it_formats_and_reads_fractions_and_offsets() {
        for ms in [
            0,
            1,
            999,
            86_400_000,
            951_782_400_000,
            1_800_000_000_123,
            4_102_444_800_000,
        ] {
            assert_eq!(parse_rfc3339_ms(&format_epoch_ms(ms)), Some(ms), "{ms}");
        }
        // The same instant, written differently.
        let instant = Some(1_800_000_000_000);
        assert_eq!(parse_rfc3339_ms("2027-01-15T08:00:00Z"), instant);
        assert_eq!(parse_rfc3339_ms("2027-01-15T08:00:00.000Z"), instant);
        assert_eq!(parse_rfc3339_ms("2027-01-15T10:00:00+02:00"), instant);
        assert_eq!(parse_rfc3339_ms("2027-01-15T07:30:00-00:30"), instant);
        assert_eq!(parse_rfc3339_ms("2027-01-15t08:00:00z"), instant);
        // A longer fraction is cut to milliseconds, a shorter one is padded.
        assert_eq!(
            parse_rfc3339_ms("2027-01-15T08:00:00.1Z"),
            Some(1_800_000_000_100)
        );
        assert_eq!(
            parse_rfc3339_ms("2027-01-15T08:00:00.123456Z"),
            Some(1_800_000_000_123)
        );
        // Leap days.
        assert_eq!(
            parse_rfc3339_ms("2000-02-29T00:00:00Z"),
            Some(951_782_400_000)
        );
        assert_eq!(
            parse_rfc3339_ms("2028-02-29T00:00:00Z"),
            Some(1835395200000)
        );
        assert_eq!(
            parse_rfc3339_ms("2027-04-30T00:00:00Z"),
            Some(1809043200000)
        );
        assert_eq!(
            parse_rfc3339_ms("2400-02-29T00:00:00Z"),
            Some(13574563200000)
        ); // divisible by 400
    }

    #[test]
    fn refuses_what_is_not_an_rfc_3339_time() {
        for bad in [
            "",
            "yesterday",
            "2027-01-15",
            "2027-01-15 08:00:00Z",
            "2027-01-15T08:00:00",
            "2027-01-15T08:00:00.Z",
            "2027-01-15T08:00:00+0200",
            "2027-01-15T08:00:00+25:00",
            "2027-13-15T08:00:00Z",
            "2027-01-32T08:00:00Z",
            "2027-02-29T08:00:00Z", // 2027 is not a leap year
            "2027-02-30T08:00:00Z",
            "2027-04-31T08:00:00Z",
            "2100-02-29T08:00:00Z", // a century year that is not divisible by 400
            "2027-01-15T24:00:00Z",
            "2027-01-15T08:60:00Z",
            "2027-01-15T08:00:00Zjunk",
            "1969-12-31T23:59:59Z",
        ] {
            assert_eq!(parse_rfc3339_ms(bad), None, "{bad:?}");
        }
    }
}
