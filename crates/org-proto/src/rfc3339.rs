//! Canonical RFC 3339 UTC timestamp parsing and formatting (`YYYY-MM-DDTHH:MM:SS.mmmZ`), `std` only.

fn days_in_month(year: i64, month: i64) -> i64 {
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn ascii_digits(b: &[u8], from: usize, len: usize) -> Option<i64> {
    let part = b.get(from..from + len)?;
    part.iter()
        .all(u8::is_ascii_digit)
        .then(|| part.iter().fold(0i64, |n, d| n * 10 + i64::from(d - b'0')))
}

fn parse_fraction_ms(b: &[u8], at: usize) -> Option<(i64, usize)> {
    if b.get(at) != Some(&b'.') {
        return Some((0, at));
    }
    let start = at + 1;
    let mut end = start;
    while b.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    if end == start {
        return None;
    }
    let mut millis = 0i64;
    for (place, digit) in b[start..end].iter().take(3).enumerate() {
        millis += i64::from(digit - b'0') * [100, 10, 1][place];
    }
    Some((millis, end))
}

fn parse_tz_offset_minutes(b: &[u8], at: usize) -> Option<i64> {
    match b.get(at..) {
        Some(b"Z" | b"z") => Some(0),
        Some([sign @ (b'+' | b'-'), rest @ ..]) if rest.len() == 5 && rest[2] == b':' => {
            let hh = ascii_digits(b, at + 1, 2)?;
            let mm = ascii_digits(b, at + 4, 2)?;
            if hh > 23 || mm > 59 {
                return None;
            }
            let minutes = hh * 60 + mm;
            Some(if *sign == b'+' { minutes } else { -minutes })
        }
        _ => None,
    }
}

/// Parses an RFC 3339 timestamp (`YYYY-MM-DDTHH:MM:SS[.frac](Z|±HH:MM)`) into Unix epoch ms.
pub fn parse_rfc3339_ms(text: &str) -> Option<u64> {
    let b = text.as_bytes();
    let (year, month, day) = (
        ascii_digits(b, 0, 4)?,
        ascii_digits(b, 5, 2)?,
        ascii_digits(b, 8, 2)?,
    );
    let (hour, minute, second) = (
        ascii_digits(b, 11, 2)?,
        ascii_digits(b, 14, 2)?,
        ascii_digits(b, 17, 2)?,
    );
    let valid_seps = b.get(4) == Some(&b'-')
        && b.get(7) == Some(&b'-')
        && matches!(b.get(10), Some(b'T' | b't'))
        && b.get(13) == Some(&b':')
        && b.get(16) == Some(&b':');
    let valid_range = (1..=12).contains(&month)
        && day >= 1
        && day <= days_in_month(year, month)
        && hour <= 23
        && minute <= 59
        && second <= 59;
    if !valid_seps || !valid_range {
        return None;
    }
    let (millis, after_frac) = parse_fraction_ms(b, 19)?;
    let offset_minutes = parse_tz_offset_minutes(b, after_frac)?;
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let seconds = days * 86_400 + hour * 3600 + minute * 60 + second - offset_minutes * 60;
    u64::try_from(seconds.checked_mul(1000)?.checked_add(millis)?).ok()
}

/// Formats Unix epoch milliseconds as canonical `YYYY-MM-DDTHH:MM:SS.mmmZ`.
pub fn format_rfc3339_ms(epoch_ms: u64) -> Option<String> {
    let total_secs = i64::try_from(epoch_ms / 1000).ok()?;
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
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + i64::from(month <= 2);
    if !(0..=9999).contains(&year) {
        return None;
    }
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z"
    ))
}

/// Canonicalizes any valid RFC 3339 timestamp to `YYYY-MM-DDTHH:MM:SS.mmmZ` UTC.
pub fn canonicalize_rfc3339_utc(text: &str) -> Option<String> {
    format_rfc3339_ms(parse_rfc3339_ms(text)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_offsets_missing_ms_and_variable_fractions() {
        assert_eq!(
            canonicalize_rfc3339_utc("2026-11-23T09:00:00Z").as_deref(),
            Some("2026-11-23T09:00:00.000Z")
        );
        assert_eq!(
            canonicalize_rfc3339_utc("2026-11-23T11:00:00+02:00").as_deref(),
            Some("2026-11-23T09:00:00.000Z")
        );
        assert_eq!(
            canonicalize_rfc3339_utc("2026-11-23T06:00:00.5-03:00").as_deref(),
            Some("2026-11-23T09:00:00.500Z")
        );
        assert_eq!(
            canonicalize_rfc3339_utc("2026-11-23T09:00:00.123456Z").as_deref(),
            Some("2026-11-23T09:00:00.123Z")
        );
        assert_eq!(
            canonicalize_rfc3339_utc("2024-02-29T23:59:59.999Z").as_deref(),
            Some("2024-02-29T23:59:59.999Z")
        );
        assert_eq!(canonicalize_rfc3339_utc("2025-02-29T00:00:00Z"), None);
        assert_eq!(canonicalize_rfc3339_utc("tomorrow"), None);
        assert_eq!(canonicalize_rfc3339_utc("2026-11-23T09:00:00.Z"), None);
        assert_eq!(canonicalize_rfc3339_utc("2026-11-23T09:00:00+25:00"), None);
    }
}
