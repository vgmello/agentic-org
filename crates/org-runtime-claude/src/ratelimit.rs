//! `rate_limit_event` as [`RateLimit`]s.
//!
//! The shape is read from the CLI's own schema in 2.1.311 (**\[binary\]**; the spike's
//! fixtures scrub every value): `rate_limit_info{status, resetsAt, rateLimitType,
//! utilization, unifiedWindows{<window>{utilization, resetsAt, status}}}`, where `status` is
//! `allowed` | `allowed_warning` | `rejected`, `resetsAt` is in Unix seconds and
//! `utilization` is a fraction (the CLI shows it times 100).

use org_runtime::{LimitStatus, RateLimit, Window};
use serde_json::Value;

/// The windows a `rate_limit_event` reports: each of `unifiedWindows`, or, without them, the
/// top-level one. `None` if the event has no `rate_limit_info` object.
pub(crate) fn from_event(msg: &Value) -> Option<Vec<RateLimit>> {
    let info = msg.get("rate_limit_info")?.as_object()?;
    if let Some(windows) = info.get("unifiedWindows").and_then(Value::as_object) {
        if !windows.is_empty() {
            return Some(windows.iter().map(|(id, w)| limit(window(id), w)).collect());
        }
    }
    let id = info.get("rateLimitType").and_then(Value::as_str);
    let w = id.map_or(Window::Unknown, window);
    Some(vec![limit(w, &Value::Object(info.clone()))])
}

fn window(id: &str) -> Window {
    match id {
        "five_hour" => Window::FiveHour,
        "seven_day" => Window::SevenDay,
        other => Window::Other {
            id: other.to_owned(),
            mins: None,
        },
    }
}

fn limit(window: Window, w: &Value) -> RateLimit {
    RateLimit {
        window,
        used_fraction: w
            .get("utilization")
            .and_then(Value::as_f64)
            .filter(|f| f.is_finite()),
        resets_at: w.get("resetsAt").and_then(Value::as_i64).and_then(rfc3339),
        status: match w.get("status").and_then(Value::as_str) {
            Some("allowed") => LimitStatus::Ok,
            Some("allowed_warning") => LimitStatus::Warning,
            Some("rejected") => LimitStatus::Rejected,
            _ => LimitStatus::Unknown,
        },
    }
}

/// Unix seconds as RFC 3339 in UTC (`2026-11-08T05:00:00Z`), for years 1970 to 9999.
pub(crate) fn rfc3339(secs: i64) -> Option<String> {
    if !(0..=253_402_300_799).contains(&secs) {
        return None;
    }
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    Some(format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    ))
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to (year, month, day).
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn times() {
        assert_eq!(rfc3339(0).unwrap(), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(951_782_400).unwrap(), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339(1_794_114_000).unwrap(), "2026-11-08T05:00:00Z");
        assert_eq!(rfc3339(253_402_300_799).unwrap(), "9999-12-31T23:59:59Z");
        assert_eq!(rfc3339(-1), None);
        assert_eq!(rfc3339(253_402_300_800), None);
    }

    #[test]
    fn unified_windows() {
        let msg = json!({"type": "rate_limit_event", "rate_limit_info": {
        "status": "allowed_warning", "resetsAt": 1_794_114_000_i64, "rateLimitType": "five_hour",
        "unifiedWindows": {
            "five_hour": {"utilization": 0.82, "resetsAt": 1_794_114_000_i64, "status": "allowed_warning"},
            "seven_day": {"utilization": 0.3, "resetsAt": 1_794_114_000_i64},
            "seven_day_opus": {"utilization": 1.0, "resetsAt": 1_794_114_000_i64, "status": "rejected"},
        }}});
        let limits = from_event(&msg).unwrap();
        assert_eq!(limits.len(), 3);
        assert_eq!(
            limits[0],
            RateLimit {
                window: Window::FiveHour,
                used_fraction: Some(0.82),
                resets_at: Some("2026-11-08T05:00:00Z".into()),
                status: LimitStatus::Warning,
            }
        );
        assert_eq!(limits[1].window, Window::SevenDay);
        assert_eq!(limits[1].status, LimitStatus::Unknown);
        assert_eq!(
            limits[2].window,
            Window::Other {
                id: "seven_day_opus".into(),
                mins: None
            }
        );
        assert_eq!(limits[2].status, LimitStatus::Rejected);
    }

    #[test]
    fn top_level_only_and_scrubbed() {
        let msg = json!({"rate_limit_info": {"status": "rejected", "rateLimitType": "seven_day",
            "utilization": 1.0, "resetsAt": 0}});
        let limits = from_event(&msg).unwrap();
        assert_eq!(limits.len(), 1);
        assert_eq!(limits[0].window, Window::SevenDay);
        assert_eq!(limits[0].status, LimitStatus::Rejected);
        assert_eq!(limits[0].resets_at.as_deref(), Some("1970-01-01T00:00:00Z"));
        // The spike's fixtures scrub every value: the shape still reads, the values don't.
        let scrubbed = json!({"rate_limit_info": {"status": "<scrubbed>", "unifiedWindows": {
            "five_hour": {"utilization": "<scrubbed>", "resetsAt": "<scrubbed>", "status": "<scrubbed>"}}}});
        let limits = from_event(&scrubbed).unwrap();
        assert_eq!(
            limits,
            vec![RateLimit {
                window: Window::FiveHour,
                used_fraction: None,
                resets_at: None,
                status: LimitStatus::Unknown
            }]
        );
        assert_eq!(from_event(&json!({"type": "rate_limit_event"})), None);
        assert_eq!(from_event(&json!({"rate_limit_info": "<scrubbed>"})), None);
        // Wrong types anywhere read as unknown, never a panic.
        let wrong = json!({"rate_limit_info": {"status": 3, "unifiedWindows": {
            "five_hour": "<scrubbed>", "seven_day": {"utilization": true, "resetsAt": 1.5,
            "status": ["allowed"]}}}});
        let limits = from_event(&wrong).unwrap();
        assert_eq!(limits.len(), 2);
        for l in limits {
            assert_eq!(
                (l.used_fraction, l.resets_at, l.status),
                (None, None, LimitStatus::Unknown)
            );
        }
    }
}
