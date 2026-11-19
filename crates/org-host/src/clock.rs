//! Timestamps as the contract writes them: RFC 3339 UTC with milliseconds.

use std::time::{SystemTime, UNIX_EPOCH};

/// `at` as RFC 3339 UTC with milliseconds, e.g. `2026-11-08T02:10:01.000Z`.
pub(crate) fn rfc3339_millis(at: SystemTime) -> String {
    let since = at.duration_since(UNIX_EPOCH).unwrap_or_default();
    let (secs, millis) = (since.as_secs(), since.subsec_millis());
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // Howard Hinnant's civil-from-days.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn formats_known_instants() {
        assert_eq!(rfc3339_millis(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        // 2026-11-08T02:10:01.250Z
        let at = UNIX_EPOCH + Duration::from_millis(1_794_103_801_250);
        assert_eq!(rfc3339_millis(at), "2026-11-08T02:10:01.250Z");
        // A leap day.
        let at = UNIX_EPOCH + Duration::from_secs(1_709_164_800);
        assert_eq!(rfc3339_millis(at), "2024-02-29T00:00:00.000Z");
    }
}
