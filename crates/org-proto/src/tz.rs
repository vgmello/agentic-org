//! Pure-`std` IANA timezone validation and DST-aware calendar period bounds (TZif v1/v2/v3,
//! `TASK-53`, `FC-008`, `doc-92`, `doc-51`).
//!
//! Reads the OS-authoritative IANA tzdata under `/usr/share/zoneinfo/` (present on macOS and
//! Linux), with built-in `UTC`/`Etc/UTC` support and strict path-traversal rejection.

use std::fs;
use std::path::Path;

use crate::rfc3339::{format_rfc3339_ms, parse_rfc3339_ms};

const ZONEINFO_ROOT: &str = "/usr/share/zoneinfo";
const MAX_TZIF_BYTES: usize = 256 * 1024;
const SECONDS_PER_DAY: i64 = 86_400;

/// Validated IANA timezone transition table parsed from TZif.
#[derive(Debug, Clone)]
struct TzData {
    transitions: Vec<i64>,
    type_indices: Vec<u8>,
    offsets: Vec<i32>,
    posix: Option<PosixTz>,
}

#[derive(Debug, Clone)]
struct PosixTz {
    std_offset: i32,
    dst: Option<PosixDst>,
}

#[derive(Debug, Clone)]
struct PosixDst {
    dst_offset: i32,
    start: PosixRule,
    end: PosixRule,
}

#[derive(Debug, Clone)]
struct PosixRule {
    month: i64,
    week: i64,
    weekday: i64,
    time_secs: i64,
}

/// Validates an IANA timezone name syntax (rejecting traversal, empty, or overlong names).
pub fn is_valid_zone_name_syntax(zone: &str) -> bool {
    if zone.is_empty() || zone.len() > 128 || zone.starts_with('/') || zone.ends_with('/') {
        return false;
    }
    if zone.contains("..") || zone.contains("//") {
        return false;
    }
    zone.bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'_' | b'-' | b'+'))
}

fn load_tz(zone: &str) -> Option<TzData> {
    if !is_valid_zone_name_syntax(zone) {
        return None;
    }
    if zone == "UTC" || zone == "Etc/UTC" {
        return Some(TzData {
            transitions: Vec::new(),
            type_indices: Vec::new(),
            offsets: vec![0],
            posix: None,
        });
    }
    let path = Path::new(ZONEINFO_ROOT).join(zone);
    let meta = fs::metadata(&path).ok()?;
    if !meta.is_file() || meta.len() as usize > MAX_TZIF_BYTES {
        return None;
    }
    let bytes = fs::read(&path).ok()?;
    parse_tzif(&bytes)
}

fn read_be_u32(b: &[u8], off: usize) -> Option<u32> {
    let slice: [u8; 4] = b.get(off..off + 4)?.try_into().ok()?;
    Some(u32::from_be_bytes(slice))
}

fn read_be_i32(b: &[u8], off: usize) -> Option<i32> {
    let slice: [u8; 4] = b.get(off..off + 4)?.try_into().ok()?;
    Some(i32::from_be_bytes(slice))
}

fn read_be_i64(b: &[u8], off: usize) -> Option<i64> {
    let slice: [u8; 8] = b.get(off..off + 8)?.try_into().ok()?;
    Some(i64::from_be_bytes(slice))
}

fn parse_tzif_block(b: &[u8], off: usize, is_64: bool) -> Option<(TzData, usize)> {
    if b.get(off..off + 4) != Some(b"TZif") {
        return None;
    }
    let isutcnt = read_be_u32(b, off + 20)? as usize;
    let isstdcnt = read_be_u32(b, off + 24)? as usize;
    let leapcnt = read_be_u32(b, off + 28)? as usize;
    let timecnt = read_be_u32(b, off + 32)? as usize;
    let typecnt = read_be_u32(b, off + 36)? as usize;
    let charcnt = read_be_u32(b, off + 40)? as usize;
    if typecnt == 0 {
        return None;
    }
    let time_size = if is_64 { 8 } else { 4 };
    let mut pos = off + 44;
    let mut transitions = Vec::with_capacity(timecnt);
    for _ in 0..timecnt {
        let t = if is_64 {
            read_be_i64(b, pos)?
        } else {
            i64::from(read_be_i32(b, pos)?)
        };
        transitions.push(t);
        pos += time_size;
    }
    let type_indices = b.get(pos..pos + timecnt)?.to_vec();
    pos += timecnt;
    let mut offsets = Vec::with_capacity(typecnt);
    for _ in 0..typecnt {
        offsets.push(read_be_i32(b, pos)?);
        pos += 6;
    }
    let leap_entry = if is_64 { 12 } else { 8 };
    let end = pos
        .checked_add(charcnt)?
        .checked_add(leapcnt.checked_mul(leap_entry)?)?
        .checked_add(isstdcnt)?
        .checked_add(isutcnt)?;
    if end > b.len() {
        return None;
    }
    Some((
        TzData {
            transitions,
            type_indices,
            offsets,
            posix: None,
        },
        end,
    ))
}

fn parse_tzif(bytes: &[u8]) -> Option<TzData> {
    if bytes.len() < 44 || &bytes[0..4] != b"TZif" {
        return None;
    }
    let version = bytes[4];
    let (v1, v1_end) = parse_tzif_block(bytes, 0, false)?;
    if matches!(version, b'2' | b'3') && bytes.get(v1_end..v1_end + 4) == Some(b"TZif") {
        let (mut v2, v2_end) = parse_tzif_block(bytes, v1_end, true)?;
        if let Some(footer) = bytes
            .get(v2_end..)
            .and_then(|s| std::str::from_utf8(s).ok())
        {
            let rule_str = footer.trim();
            if !rule_str.is_empty() {
                v2.posix = parse_posix_tz(rule_str);
            }
        }
        return Some(v2);
    }
    Some(v1)
}

fn parse_posix_tz(raw: &str) -> Option<PosixTz> {
    let (_, rest) = take_tz_abbr(raw)?;
    let (std_west, rest) = parse_posix_offset(rest)?;
    let std_offset = -std_west;
    if rest.is_empty() {
        return Some(PosixTz {
            std_offset,
            dst: None,
        });
    }
    let (_, rest) = take_tz_abbr(rest)?;
    let (dst_offset, rest) = if rest.starts_with(',') || rest.is_empty() {
        (std_offset + 3600, rest)
    } else {
        let (dst_west, r) = parse_posix_offset(rest)?;
        (-dst_west, r)
    };
    let rest = rest.strip_prefix(',')?;
    let (start_s, end_s) = rest.split_once(',')?;
    let start = parse_posix_m_rule(start_s)?;
    let end = parse_posix_m_rule(end_s)?;
    Some(PosixTz {
        std_offset,
        dst: Some(PosixDst {
            dst_offset,
            start,
            end,
        }),
    })
}

fn take_tz_abbr(s: &str) -> Option<(&str, &str)> {
    if let Some(rest) = s.strip_prefix('<') {
        let end = rest.find('>')?;
        return Some((&rest[..end], &rest[end + 1..]));
    }
    let end = s
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(s.len());
    (end >= 3).then(|| (&s[..end], &s[end..]))
}

fn parse_posix_offset(s: &str) -> Option<(i32, &str)> {
    let (sign, rest) = if let Some(r) = s.strip_prefix('-') {
        (-1, r)
    } else if let Some(r) = s.strip_prefix('+') {
        (1, r)
    } else {
        (1, s)
    };
    let end = rest
        .find(|c: char| !c.is_ascii_digit() && c != ':')
        .unwrap_or(rest.len());
    let parts: Vec<i32> = rest[..end]
        .split(':')
        .map(|p| p.parse::<i32>().ok())
        .collect::<Option<_>>()?;
    let h = *parts.first()?;
    let m = *parts.get(1).unwrap_or(&0);
    let sec = *parts.get(2).unwrap_or(&0);
    Some((sign * (h * 3600 + m * 60 + sec), &rest[end..]))
}

fn parse_posix_m_rule(s: &str) -> Option<PosixRule> {
    let (date_part, time_part) = match s.split_once('/') {
        Some((d, t)) => (d, Some(t)),
        None => (s, None),
    };
    let m_body = date_part.strip_prefix('M')?;
    let mut nums = m_body.split('.');
    let month: i64 = nums.next()?.parse().ok()?;
    let week: i64 = nums.next()?.parse().ok()?;
    let weekday: i64 = nums.next()?.parse().ok()?;
    let time_secs = match time_part {
        Some(t) => i64::from(parse_posix_offset(t)?.0),
        None => 7200,
    };
    Some(PosixRule {
        month,
        week,
        weekday,
        time_secs,
    })
}

fn posix_rule_local_secs(year: i64, r: &PosixRule) -> i64 {
    let first_day = days_from_civil(year, r.month, 1);
    // 1970-01-01 was Thursday (4 with Sun=0).
    let first_wday = (first_day + 4).rem_euclid(7);
    let offset = (r.weekday - first_wday).rem_euclid(7);
    let mut day = 1 + offset + (r.week - 1) * 7;
    let next_first = if r.month == 12 {
        days_from_civil(year + 1, 1, 1)
    } else {
        days_from_civil(year, r.month + 1, 1)
    };
    let month_len = next_first - first_day;
    while day > month_len {
        day -= 7;
    }
    days_from_civil(year, r.month, day) * SECONDS_PER_DAY + r.time_secs
}

impl PosixTz {
    fn offset_at_utc(&self, utc_secs: i64) -> i32 {
        let Some(ref dst) = self.dst else {
            return self.std_offset;
        };
        let (year, _, _) =
            civil_from_days((utc_secs + i64::from(self.std_offset)).div_euclid(SECONDS_PER_DAY));
        let start_utc = posix_rule_local_secs(year, &dst.start) - i64::from(self.std_offset);
        let end_utc = posix_rule_local_secs(year, &dst.end) - i64::from(dst.dst_offset);
        let in_dst = if start_utc < end_utc {
            utc_secs >= start_utc && utc_secs < end_utc
        } else {
            utc_secs >= start_utc || utc_secs < end_utc
        };
        if in_dst {
            dst.dst_offset
        } else {
            self.std_offset
        }
    }
}

impl TzData {
    fn offset_at_utc(&self, utc_secs: i64) -> i32 {
        if let (Some(ref posix), Some(&last)) = (&self.posix, self.transitions.last()) {
            if utc_secs >= last {
                return posix.offset_at_utc(utc_secs);
            }
        }
        if self.transitions.is_empty() {
            return self
                .posix
                .as_ref()
                .map_or(self.offsets[0], |p| p.offset_at_utc(utc_secs));
        }
        let idx = match self.transitions.binary_search(&utc_secs) {
            Ok(i) => i,
            Err(0) => return self.offsets[0],
            Err(i) => i - 1,
        };
        let type_idx = usize::from(*self.type_indices.get(idx).unwrap_or(&0));
        *self.offsets.get(type_idx).unwrap_or(&self.offsets[0])
    }

    /// Resolves a local civil timestamp (seconds since 1970-01-01T00:00:00 local) to the
    /// exact earliest UTC epoch second (handling spring-forward gaps and both negative/positive
    /// offset fall-back overlaps).
    fn local_to_utc(&self, local_secs: i64) -> i64 {
        let probes = [local_secs - 72_000, local_secs, local_secs + 72_000];
        let mut best_valid: Option<i64> = None;
        let mut fallback: Option<i64> = None;
        for probe in probes {
            let off = i64::from(self.offset_at_utc(probe));
            let cand = local_secs - off;
            let actual = i64::from(self.offset_at_utc(cand));
            if actual == off {
                best_valid = Some(best_valid.map_or(cand, |b| b.min(cand)));
            } else {
                let refined = local_secs - actual;
                if i64::from(self.offset_at_utc(refined)) == actual {
                    best_valid = Some(best_valid.map_or(refined, |b| b.min(refined)));
                } else {
                    let gap = cand.max(refined);
                    fallback = Some(fallback.map_or(gap, |f| f.max(gap)));
                }
            }
        }
        best_valid.or(fallback).unwrap_or(local_secs)
    }
}

/// Returns `true` iff `zone` is a valid, installed IANA timezone name (e.g., `"UTC"`,
/// `"America/New_York"`, `"Europe/London"`).
pub fn validate_iana_timezone(zone: &str) -> bool {
    load_tz(zone).is_some()
}

/// Returns the UTC offset in seconds for `zone` at `epoch_secs`.
pub fn local_offset_seconds(zone: &str, epoch_secs: i64) -> Option<i32> {
    Some(load_tz(zone)?.offset_at_utc(epoch_secs))
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
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
    (year, month, day)
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn parse_ymd(s: &str) -> Option<(i64, i64, i64)> {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let y: i64 = s[0..4].parse().ok()?;
    let m: i64 = s[5..7].parse().ok()?;
    let d: i64 = s[8..10].parse().ok()?;
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let max_d = match m {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => return None,
    };
    if d < 1 || d > max_d {
        return None;
    }
    Some((y, m, d))
}

/// Computes the local calendar day key (`YYYY-MM-DD`) and half-open UTC bounds `[starts_at_ms, ends_at_ms)`
/// for `epoch_ms` in IANA timezone `zone`, honoring DST (23h / 24h / 25h local days).
pub fn local_day_bounds_utc(zone: &str, epoch_ms: u64) -> Option<(String, u64, u64)> {
    let tz = load_tz(zone)?;
    let utc_secs = i64::try_from(epoch_ms / 1000).ok()?;
    let local_secs = utc_secs + i64::from(tz.offset_at_utc(utc_secs));
    let local_days = local_secs.div_euclid(SECONDS_PER_DAY);
    let (y, m, d) = civil_from_days(local_days);
    let start_utc = tz.local_to_utc(local_days * SECONDS_PER_DAY);
    let end_utc = tz.local_to_utc((local_days + 1) * SECONDS_PER_DAY);
    Some((
        format!("{y:04}-{m:02}-{d:02}"),
        u64::try_from(start_utc * 1000).ok()?,
        u64::try_from(end_utc * 1000).ok()?,
    ))
}

/// Computes the Monday-anchored local calendar week `[starts_at_ms, ends_at_ms)` for `epoch_ms`
/// in IANA timezone `zone`, honoring DST transitions across the 7-day window (`doc-51` §3).
pub fn local_week_bounds_utc(zone: &str, epoch_ms: u64) -> Option<(String, u64, u64)> {
    let tz = load_tz(zone)?;
    let utc_secs = i64::try_from(epoch_ms / 1000).ok()?;
    let local_secs = utc_secs + i64::from(tz.offset_at_utc(utc_secs));
    let local_days = local_secs.div_euclid(SECONDS_PER_DAY);
    // 1970-01-01 was Thursday (offset 3 from Monday).
    let days_since_monday = (local_days + 3).rem_euclid(7);
    let monday_days = local_days - days_since_monday;
    let (y, m, d) = civil_from_days(monday_days);
    let start_utc = tz.local_to_utc(monday_days * SECONDS_PER_DAY);
    let end_utc = tz.local_to_utc((monday_days + 7) * SECONDS_PER_DAY);
    Some((
        format!("{y:04}-{m:02}-{d:02}"),
        u64::try_from(start_utc * 1000).ok()?,
        u64::try_from(end_utc * 1000).ok()?,
    ))
}

/// Computes the local calendar month key (`YYYY-MM`) and half-open UTC bounds `[starts_at_ms, ends_at_ms)`
/// for `epoch_ms` in IANA timezone `zone`, honoring DST transitions.
pub fn local_month_bounds_utc(zone: &str, epoch_ms: u64) -> Option<(String, u64, u64)> {
    let tz = load_tz(zone)?;
    let utc_secs = i64::try_from(epoch_ms / 1000).ok()?;
    let local_secs = utc_secs + i64::from(tz.offset_at_utc(utc_secs));
    let (y, m, _) = civil_from_days(local_secs.div_euclid(SECONDS_PER_DAY));
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    let start_utc = tz.local_to_utc(days_from_civil(y, m, 1) * SECONDS_PER_DAY);
    let end_utc = tz.local_to_utc(days_from_civil(ny, nm, 1) * SECONDS_PER_DAY);
    Some((
        format!("{y:04}-{m:02}"),
        u64::try_from(start_utc * 1000).ok()?,
        u64::try_from(end_utc * 1000).ok()?,
    ))
}

/// Converts inclusive whole-date range `[from_ymd, to_ymd]` (or `[from_ymd, now_ms]` when `to_ymd`
/// is `None`) in IANA timezone `zone` into half-open UTC bounds `[start_ms, end_ms)` (`doc-51`).
pub fn date_range_to_utc_bounds(
    zone: &str,
    from_ymd: &str,
    to_ymd: Option<&str>,
    now_ms: u64,
) -> Option<(u64, u64)> {
    let tz = load_tz(zone)?;
    let (fy, fm, fd) = parse_ymd(from_ymd)?;
    let start_utc = tz.local_to_utc(days_from_civil(fy, fm, fd) * SECONDS_PER_DAY);
    let start_ms = u64::try_from(start_utc * 1000).ok()?;
    let end_ms = match to_ymd {
        Some(to) => {
            let (ty, tm, td) = parse_ymd(to)?;
            let end_day = days_from_civil(ty, tm, td) + 1;
            u64::try_from(tz.local_to_utc(end_day * SECONDS_PER_DAY) * 1000).ok()?
        }
        None => now_ms,
    };
    if end_ms < start_ms {
        return None;
    }
    Some((start_ms, end_ms))
}

/// Formats `[start_ms, end_ms)` into canonical RFC 3339 UTC strings.
pub fn format_utc_bounds(start_ms: u64, end_ms: u64) -> Option<(String, String)> {
    Some((format_rfc3339_ms(start_ms)?, format_rfc3339_ms(end_ms)?))
}

/// Parses an RFC 3339 string and returns the local `YYYY-MM-DD` date key in `zone`.
pub fn local_date_key_for_rfc3339(zone: &str, rfc3339: &str) -> Option<String> {
    let ms = parse_rfc3339_ms(rfc3339)?;
    Some(local_day_bounds_utc(zone, ms)?.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_iana_names_and_rejects_traversal() {
        assert!(validate_iana_timezone("UTC"));
        assert!(validate_iana_timezone("Etc/UTC"));
        assert!(validate_iana_timezone("America/New_York"));
        assert!(!validate_iana_timezone(""));
        assert!(!validate_iana_timezone("../etc/passwd"));
        assert!(!validate_iana_timezone("/UTC"));
        assert!(!validate_iana_timezone("Mars/Olympus"));
    }

    #[test]
    fn dst_spring_forward_and_fall_back_day_bounds_in_new_york() {
        // 2026-03-08 is US DST spring-forward (23-hour day in America/New_York).
        let spring_noon = parse_rfc3339_ms("2026-03-08T16:00:00.000Z").unwrap();
        let (key, start, end) = local_day_bounds_utc("America/New_York", spring_noon).unwrap();
        assert_eq!(key, "2026-03-08");
        assert_eq!(end - start, 23 * 3600 * 1000, "spring-forward day is 23h");
        assert_eq!(
            format_rfc3339_ms(start).as_deref(),
            Some("2026-03-08T05:00:00.000Z")
        );
        assert_eq!(
            format_rfc3339_ms(end).as_deref(),
            Some("2026-03-09T04:00:00.000Z")
        );

        // 2026-11-01 is US DST fall-back (25-hour day in America/New_York).
        let fall_noon = parse_rfc3339_ms("2026-11-01T17:00:00.000Z").unwrap();
        let (key2, start2, end2) = local_day_bounds_utc("America/New_York", fall_noon).unwrap();
        assert_eq!(key2, "2026-11-01");
        assert_eq!(end2 - start2, 25 * 3600 * 1000, "fall-back day is 25h");
        assert_eq!(
            format_rfc3339_ms(start2).as_deref(),
            Some("2026-11-01T04:00:00.000Z")
        );
        assert_eq!(
            format_rfc3339_ms(end2).as_deref(),
            Some("2026-11-02T05:00:00.000Z")
        );
    }

    #[test]
    fn month_and_date_range_bounds_follow_selected_zone() {
        let mid_nov = parse_rfc3339_ms("2026-11-15T12:00:00.000Z").unwrap();
        let (mkey, ms, me) = local_month_bounds_utc("America/New_York", mid_nov).unwrap();
        assert_eq!(mkey, "2026-11");
        assert_eq!(
            format_rfc3339_ms(ms).as_deref(),
            Some("2026-11-01T04:00:00.000Z")
        );
        assert_eq!(
            format_rfc3339_ms(me).as_deref(),
            Some("2026-12-01T05:00:00.000Z")
        );

        let (rs, re) = date_range_to_utc_bounds(
            "America/New_York",
            "2026-11-01",
            Some("2026-11-01"),
            mid_nov,
        )
        .unwrap();
        assert_eq!(rs, ms);
        assert_eq!(re - rs, 25 * 3600 * 1000);
    }

    #[test]
    fn week_bounds_dst_posix_footer_and_positive_midnight_overlap() {
        // Week containing 2026-03-08 (Sunday spring-forward in America/New_York) is 167 hours.
        let sun = parse_rfc3339_ms("2026-03-08T16:00:00.000Z").unwrap();
        let (wkey, ws, we) = local_week_bounds_utc("America/New_York", sun).unwrap();
        assert_eq!(wkey, "2026-03-02");
        assert_eq!(we - ws, (7 * 24 - 1) * 3600 * 1000);
        assert_eq!(
            format_rfc3339_ms(ws).as_deref(),
            Some("2026-03-02T05:00:00.000Z")
        );
        assert_eq!(
            format_rfc3339_ms(we).as_deref(),
            Some("2026-03-09T04:00:00.000Z")
        );

        // Post-2038 POSIX TZ footer in America/New_York: 2045-07-04 is EDT (-04:00), 2045-01-15 is EST (-05:00).
        let july_2045 = parse_rfc3339_ms("2045-07-04T12:00:00.000Z").unwrap() / 1000;
        let jan_2045 = parse_rfc3339_ms("2045-01-15T12:00:00.000Z").unwrap() / 1000;
        assert_eq!(
            local_offset_seconds("America/New_York", july_2045 as i64),
            Some(-4 * 3600)
        );
        assert_eq!(
            local_offset_seconds("America/New_York", jan_2045 as i64),
            Some(-5 * 3600)
        );

        // Positive-offset midnight fall-back (+03:00 -> +02:00 at local 00:00): earliest UTC instant is selected.
        let midnight_local = days_from_civil(2026, 10, 25) * SECONDS_PER_DAY;
        let transition_utc = midnight_local - 2 * 3600;
        let synthetic = TzData {
            transitions: vec![transition_utc],
            type_indices: vec![1],
            offsets: vec![3 * 3600, 2 * 3600],
            posix: None,
        };
        assert_eq!(
            synthetic.local_to_utc(midnight_local),
            midnight_local - 3 * 3600
        );
    }
}
