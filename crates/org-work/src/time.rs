//! RFC 3339 UTC timestamps with millisecond precision (`2026-11-08T02:10:00.000Z`), `std` only.

/// Formats Unix milliseconds (Howard Hinnant's civil-from-days).
pub fn format_ms(epoch_ms: u64) -> String {
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

/// Parses exactly the form [`format_ms`] writes, at or after 1970. `None` for anything else.
pub fn parse_ms(text: &str) -> Option<u64> {
    let b = text.as_bytes();
    if b.len() != 24
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
        || b[19] != b'.'
        || b[23] != b'Z'
    {
        return None;
    }
    let num = |from: usize, to: usize| -> Option<i64> {
        b[from..to].iter().try_fold(0i64, |acc, &c| {
            c.is_ascii_digit().then(|| acc * 10 + i64::from(c - b'0'))
        })
    };
    let (year, month, day) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (hour, minute, second, millis) = (num(11, 13)?, num(14, 16)?, num(17, 19)?, num(20, 23)?);
    if !(1..=12).contains(&month)
        || day < 1
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    // Hinnant's days-from-civil.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400 + hour * 3600 + minute * 60 + second;
    u64::try_from(secs * 1000 + millis).ok()
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => 31,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_and_parses_known_instants() {
        for (ms, text) in [
            (0, "1970-01-01T00:00:00.000Z"),
            (951_782_400_123, "2000-02-29T00:00:00.123Z"),
            (1_762_567_890_007, "2025-11-08T02:11:30.007Z"),
            (4_102_444_799_999, "2099-12-31T23:59:59.999Z"),
        ] {
            assert_eq!(format_ms(ms), text);
            assert_eq!(parse_ms(text), Some(ms));
        }
    }

    #[test]
    fn round_trips_every_day_for_years() {
        let mut ms = 1_700_000_000_000u64;
        for _ in 0..3_000 {
            assert_eq!(parse_ms(&format_ms(ms)), Some(ms));
            ms += 86_400_000 + 3_723_001;
        }
    }

    #[test]
    fn refuses_other_shapes() {
        for text in [
            "2026-11-08T02:10:00Z",
            "2026-11-08 02:10:00.000Z",
            "2026-02-30T00:00:00.000Z",
            "2026-13-01T00:00:00.000Z",
            "2026-11-08T24:00:00.000Z",
            "2026-11-08T02:10:00.000+",
            "+026-11-08T02:10:00.000Z",
        ] {
            assert_eq!(parse_ms(text), None, "{text}");
        }
    }
}
