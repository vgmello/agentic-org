//! Browser session layer (A3 B5; web-api-v1 §4, ADR-0013). The web process mints
//! the cookie and proof, sends only their digests to the host, and keeps an
//! in-memory cache of validated sessions keyed by the cookie digest. It stores
//! nothing on disk; the host owns every session record.
pub mod cache;
pub mod cookie;

pub use cache::SessionCache;
pub use cookie::{BrowserPair, CookieName, PresentedPair};

/// Unix milliseconds as RFC 3339 UTC with milliseconds (web-api-v1 §3
/// `idle_expires_at`), e.g. `2026-12-10T12:00:00.000Z`.
pub fn rfc3339_millis(ms: u64) -> String {
    let (secs, millis) = (ms / 1000, ms % 1000);
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // Howard Hinnant's civil-from-days, as the host's clock writes it.
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
    use super::rfc3339_millis;

    #[test]
    fn idle_expiry_is_rfc3339_utc_with_milliseconds() {
        assert_eq!(rfc3339_millis(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            rfc3339_millis(1_794_312_000_000),
            "2026-11-10T12:00:00.000Z"
        );
        assert_eq!(
            rfc3339_millis(1_709_164_800_250),
            "2024-02-29T00:00:00.250Z"
        );
        assert_eq!(rfc3339_millis(951_868_799_999), "2000-02-29T23:59:59.999Z");
    }
}
