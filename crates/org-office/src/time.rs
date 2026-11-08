//! RFC 3339 UTC timestamps with millisecond precision (`YYYY-MM-DDTHH:MM:SS.mmmZ`), `std` only.

use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn now_rfc3339() -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    format_epoch_ms(ms)
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
}
