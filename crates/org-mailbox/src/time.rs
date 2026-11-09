use std::time::{SystemTime, UNIX_EPOCH};

/// Wall time for persisted UTC timestamps. Blocking waits use Instant separately.
pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
}

#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|duration| u64::try_from(duration.as_millis()).ok())
            .unwrap_or(0)
    }
}

/// Canonical RFC3339 UTC, millisecond precision, years 1970..=9999.
pub fn format_utc(ms: u64) -> Option<String> {
    if ms > 253_402_300_799_999 {
        return None;
    }
    let secs = (ms / 1000) as i64;
    let days = secs / 86_400;
    let within = secs % 86_400;
    // Howard Hinnant's civil-from-days algorithm, also used by org-work.
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        within / 3600,
        (within % 3600) / 60,
        within % 60,
        ms % 1000
    ))
}

pub fn parse_utc(text: &str) -> Option<u64> {
    let bytes = text.as_bytes();
    if bytes.len() != 24
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'.'
        || bytes[23] != b'Z'
    {
        return None;
    }
    let number = |from: usize, to: usize| -> Option<i64> {
        bytes[from..to].iter().try_fold(0, |value, &byte| {
            byte.is_ascii_digit()
                .then_some(value * 10 + i64::from(byte.wrapping_sub(b'0')))
        })
    };
    let (year, month, day) = (number(0, 4)?, number(5, 7)?, number(8, 10)?);
    let (hour, minute, second, millis) = (
        number(11, 13)?,
        number(14, 16)?,
        number(17, 19)?,
        number(20, 23)?,
    );
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => return None,
    };
    if year < 1970 || day < 1 || day > days_in_month || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from((days * 86_400 + hour * 3600 + minute * 60 + second) * 1000 + millis).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_boundaries_and_leap_days() {
        for (ms, text) in [
            (0, "1970-01-01T00:00:00.000Z"),
            (951_782_400_123, "2000-02-29T00:00:00.123Z"),
            (253_402_300_799_999, "9999-12-31T23:59:59.999Z"),
        ] {
            assert_eq!(format_utc(ms).as_deref(), Some(text));
            assert_eq!(parse_utc(text), Some(ms));
        }
        assert_eq!(format_utc(u64::MAX), None);
        for invalid in [
            "1900-01-01T00:00:00.000Z",
            "2026-02-29T00:00:00.000Z",
            "2026-11-08T00:00:00Z",
            "2026-11-08T24:00:00.000Z",
        ] {
            assert_eq!(parse_utc(invalid), None);
        }
    }
}
