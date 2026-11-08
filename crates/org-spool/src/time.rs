//! ISO-8601 UTC timestamp formatting and duration parsing (`std`-only).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Format a `SystemTime` as `YYYY-MM-DDTHH:MM:SS.mmmZ` in UTC.
pub fn format_iso8601_utc(time: SystemTime) -> String {
    let dur = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    format_epoch_ms_iso8601(dur.as_millis() as u64)
}

/// Format epoch milliseconds as `YYYY-MM-DDTHH:MM:SS.mmmZ` using Howard Hinnant's civil algorithm.
pub fn format_epoch_ms_iso8601(epoch_ms: u64) -> String {
    let total_secs = (epoch_ms / 1000) as i64;
    let millis = epoch_ms % 1000;

    let days = total_secs.div_euclid(86_400);
    let secs_of_day = total_secs.rem_euclid(86_400);

    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;

    // Civil date from days since 1970-01-01
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = if m <= 2 { y + 1 } else { y };

    format!("{year:04}-{m:02}-{d:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

/// Validate that a timestamp string has canonical 24-char `YYYY-MM-DDTHH:MM:SS.mmmZ` UTC format.
pub fn is_valid_iso8601_utc(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 24 {
        return false;
    }
    let digit_indices = [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18, 20, 21, 22];
    if !digit_indices.iter().all(|&i| b[i].is_ascii_digit()) {
        return false;
    }
    b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && b[19] == b'.'
        && b[23] == b'Z'
}

/// Parse a human duration string such as `"120s"`, `"500ms"`, `"30m"`, `"2h"`, `"1d"`, or plain seconds `"60"`.
pub fn parse_duration_arg(raw: &str) -> Result<Duration, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err("duration must not be empty".into());
    }
    if let Some(ms_str) = s.strip_suffix("ms") {
        let val = ms_str
            .trim()
            .parse::<u64>()
            .map_err(|_| format!("invalid duration '{raw}'"))?;
        return Ok(Duration::from_millis(val));
    }
    if let Some(sec_str) = s.strip_suffix('s') {
        let val = sec_str
            .trim()
            .parse::<f64>()
            .map_err(|_| format!("invalid duration '{raw}'"))?;
        if val < 0.0 || !val.is_finite() {
            return Err(format!("invalid duration '{raw}'"));
        }
        return Ok(Duration::from_millis((val * 1000.0) as u64));
    }
    if let Some(min_str) = s.strip_suffix('m') {
        let val = min_str
            .trim()
            .parse::<u64>()
            .map_err(|_| format!("invalid duration '{raw}'"))?;
        return Ok(Duration::from_secs(val.saturating_mul(60)));
    }
    if let Some(hr_str) = s.strip_suffix('h') {
        let val = hr_str
            .trim()
            .parse::<u64>()
            .map_err(|_| format!("invalid duration '{raw}'"))?;
        return Ok(Duration::from_secs(val.saturating_mul(3600)));
    }
    if let Some(day_str) = s.strip_suffix('d') {
        let val = day_str
            .trim()
            .parse::<u64>()
            .map_err(|_| format!("invalid duration '{raw}'"))?;
        return Ok(Duration::from_secs(val.saturating_mul(86_400)));
    }
    let secs = s
        .parse::<u64>()
        .map_err(|_| format!("invalid duration '{raw}' (use e.g. 60s, 30m, 2h, or 500ms)"))?;
    Ok(Duration::from_secs(secs))
}
