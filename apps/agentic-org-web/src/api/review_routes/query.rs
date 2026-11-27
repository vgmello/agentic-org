//! Path validators, query-string parsers, and response post-filters for the browser
//! `/api/v1/organizations/{org}/reviews*` routes (`TASK-92.2`).

use org_proto::reviews::{
    ReviewDiffParams, ReviewDiffResult, ReviewInspectParams, ReviewInspectSelection,
    ReviewListParams, ReviewStatus, ReviewTargetKind, REVIEW_DIFF_MAX_CONTEXT_LINES,
    REVIEW_LIST_MAX_LIMIT, REVIEW_PATH_MAX_LEN,
};
use serde_json::Value;

use super::super::{invalid, not_found};
use crate::service::ApiReply;

const MAX_REF_BYTES: usize = 128;

pub(super) struct InspectQuery {
    pub(super) params: ReviewInspectParams,
    pub(super) round: Option<u32>,
}

pub(super) struct DiffQuery {
    pub(super) params: ReviewDiffParams,
    pub(super) start_line: Option<u32>,
    pub(super) end_line: Option<u32>,
}

pub(super) fn valid_org_id(id: &str) -> Result<(), ApiReply> {
    org_office_launch::is_storage_id(id)
        .then_some(())
        .ok_or_else(invalid)
}

/// Validates a review, project, task, repository, or team reference segment: non-blank, bounded
/// to 128 bytes, free of control characters, path separators, or traversal markers.
pub(super) fn valid_ref(raw: &str) -> Result<(), ApiReply> {
    let ok = !raw.trim().is_empty()
        && raw.len() <= MAX_REF_BYTES
        && raw != "."
        && !raw.contains("..")
        && !raw.contains('/')
        && !raw.contains('\\')
        && !raw.chars().any(char::is_control);
    ok.then_some(()).ok_or_else(invalid)
}

pub(super) fn valid_uuid(raw: &str) -> Result<(), ApiReply> {
    org_proto::is_uuid_v7(raw).then_some(()).ok_or_else(invalid)
}

pub(super) fn valid_optional_ref(raw: Option<&str>) -> Result<(), ApiReply> {
    match raw {
        Some(v) => valid_ref(v),
        None => Ok(()),
    }
}

pub(super) fn valid_single_line_token(raw: &str, max_bytes: usize) -> Result<(), ApiReply> {
    let ok = !raw.trim().is_empty() && raw.len() <= max_bytes && !raw.chars().any(char::is_control);
    ok.then_some(()).ok_or_else(invalid)
}

pub(super) fn valid_optional_token(raw: Option<&str>, max_bytes: usize) -> Result<(), ApiReply> {
    match raw {
        Some(v) => valid_single_line_token(v, max_bytes),
        None => Ok(()),
    }
}

/// Validates a repository-relative diff path filter (matching `FindingAnchor::check` rules).
pub(super) fn valid_diff_path(path: &str) -> Result<(), ApiReply> {
    let bad = path.is_empty()
        || path.len() > REVIEW_PATH_MAX_LEN
        || path.starts_with(['/', '-', ':'])
        || path.chars().any(|c| c.is_control() || c == '\\')
        || path
            .split('/')
            .any(|c| c.is_empty() || c == "." || c == ".." || c.eq_ignore_ascii_case(".git"));
    (!bad).then_some(()).ok_or_else(invalid)
}

fn set_once<T>(slot: &mut Option<T>, value: T) -> Result<(), ApiReply> {
    if slot.is_some() {
        return Err(invalid());
    }
    *slot = Some(value);
    Ok(())
}

fn decode_query_value(raw: &str) -> Result<String, ApiReply> {
    let mut out = Vec::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
        } else if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err(invalid());
            }
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).map_err(|_| invalid())?;
            let b = u8::from_str_radix(hex, 16).map_err(|_| invalid())?;
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let decoded = String::from_utf8(out).map_err(|_| invalid())?;
    if decoded.chars().any(char::is_control) {
        return Err(invalid());
    }
    Ok(decoded)
}

fn parse_positive_u32(raw: &str) -> Result<u32, ApiReply> {
    let n = raw.parse::<u32>().map_err(|_| invalid())?;
    (n >= 1).then_some(n).ok_or_else(invalid)
}

pub(super) fn parse_list_query(query: Option<&str>) -> Result<ReviewListParams, ApiReply> {
    let mut params = ReviewListParams::default();
    let Some(q) = query.filter(|s| !s.is_empty()) else {
        return Ok(params);
    };
    for pair in q.split('&') {
        let (k, raw_v) = pair.split_once('=').ok_or_else(invalid)?;
        let v = decode_query_value(raw_v)?;
        apply_list_param(&mut params, k, v)?;
    }
    Ok(params)
}

fn apply_list_param(params: &mut ReviewListParams, key: &str, v: String) -> Result<(), ApiReply> {
    match key {
        "status" => set_once(
            &mut params.status,
            ReviewStatus::parse(&v).ok_or_else(invalid)?,
        ),
        "project_id" => {
            valid_ref(&v)?;
            set_once(&mut params.project_id, v)
        }
        "task_id" => {
            valid_ref(&v)?;
            set_once(&mut params.task_id, v)
        }
        "reviewer_id" => {
            valid_uuid(&v)?;
            set_once(&mut params.reviewer_id, v)
        }
        "author_id" | "created_by" => {
            valid_uuid(&v)?;
            set_once(&mut params.created_by, v)
        }
        "kind" | "target_kind" => set_once(
            &mut params.target_kind,
            ReviewTargetKind::parse(&v).ok_or_else(invalid)?,
        ),
        "limit" => {
            let n = parse_positive_u32(&v)?;
            if n > REVIEW_LIST_MAX_LIMIT {
                return Err(invalid());
            }
            set_once(&mut params.limit, n)
        }
        "cursor" | "offset" => {
            let n = v.parse::<u64>().map_err(|_| invalid())?;
            set_once(&mut params.offset, n)
        }
        _ => Err(invalid()),
    }
}

pub(super) fn parse_inspect_query(
    review: String,
    query: Option<&str>,
) -> Result<InspectQuery, ApiReply> {
    let mut fields: Option<Vec<String>> = None;
    let mut round: Option<u32> = None;
    if let Some(q) = query.filter(|s| !s.is_empty()) {
        for pair in q.split('&') {
            let (k, raw_v) = pair.split_once('=').ok_or_else(invalid)?;
            let v = decode_query_value(raw_v)?;
            match k {
                "fields" => {
                    let list: Vec<String> = v.split(',').map(|s| s.trim().to_owned()).collect();
                    ReviewInspectSelection::try_from_fields(Some(&list)).map_err(|_| invalid())?;
                    set_once(&mut fields, list)?;
                }
                "round" => set_once(&mut round, parse_positive_u32(&v)?)?,
                _ => return Err(invalid()),
            }
        }
    }
    Ok(InspectQuery {
        params: ReviewInspectParams { review, fields },
        round,
    })
}

#[derive(Default)]
struct DiffQueryAcc {
    round: Option<u32>,
    since_round: Option<u32>,
    path: Option<String>,
    start_line: Option<u32>,
    end_line: Option<u32>,
    stat_only: Option<bool>,
    context_lines: Option<u32>,
}

pub(super) fn parse_diff_query(review: String, query: Option<&str>) -> Result<DiffQuery, ApiReply> {
    let mut acc = DiffQueryAcc::default();
    if let Some(q) = query.filter(|s| !s.is_empty()) {
        for pair in q.split('&') {
            let (k, raw_v) = pair.split_once('=').ok_or_else(invalid)?;
            let v = decode_query_value(raw_v)?;
            apply_diff_param(&mut acc, k, v)?;
        }
    }
    validate_diff_acc(&acc)?;
    Ok(DiffQuery {
        params: ReviewDiffParams {
            review,
            round: acc.round,
            since_round: acc.since_round,
            path: acc.path,
            stat_only: acc.stat_only.unwrap_or(false),
            context_lines: acc.context_lines,
        },
        start_line: acc.start_line,
        end_line: acc.end_line,
    })
}

fn apply_diff_param(acc: &mut DiffQueryAcc, key: &str, v: String) -> Result<(), ApiReply> {
    match key {
        "round" => set_once(&mut acc.round, parse_positive_u32(&v)?),
        "compare_round" | "since_round" => set_once(&mut acc.since_round, parse_positive_u32(&v)?),
        "path" => {
            valid_diff_path(&v)?;
            set_once(&mut acc.path, v)
        }
        "start_line" => set_once(&mut acc.start_line, parse_positive_u32(&v)?),
        "end_line" => set_once(&mut acc.end_line, parse_positive_u32(&v)?),
        "stat_only" => set_once(&mut acc.stat_only, parse_bool(&v)?),
        "context_lines" => {
            let n = v.parse::<u32>().map_err(|_| invalid())?;
            if n > REVIEW_DIFF_MAX_CONTEXT_LINES {
                return Err(invalid());
            }
            set_once(&mut acc.context_lines, n)
        }
        _ => Err(invalid()),
    }
}

fn parse_bool(raw: &str) -> Result<bool, ApiReply> {
    match raw {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(invalid()),
    }
}

fn validate_diff_acc(acc: &DiffQueryAcc) -> Result<(), ApiReply> {
    let bad_rounds = matches!((acc.round, acc.since_round), (Some(r), Some(s)) if s >= r);
    let bad_lines = matches!((acc.start_line, acc.end_line), (Some(s), Some(e)) if e < s);
    let stat_with_lines =
        acc.stat_only.unwrap_or(false) && (acc.start_line.is_some() || acc.end_line.is_some());
    if bad_rounds || bad_lines || stat_with_lines {
        return Err(invalid());
    }
    Ok(())
}

/// Filters a serialized `review.inspect` reply (full or `--fields`-pruned) to one round.
pub(super) fn filter_inspect_round(val: &mut Value, round: u32) -> Result<(), ApiReply> {
    let Some(obj) = val.as_object_mut() else {
        return Ok(());
    };
    let target = u64::from(round);
    if let Some(rounds) = obj.get_mut("rounds").and_then(Value::as_array_mut) {
        rounds.retain(|r| r.get("round_number").and_then(Value::as_u64) == Some(target));
        if rounds.is_empty() {
            return Err(not_found());
        }
    } else if let Some(cur) = obj
        .get("review")
        .and_then(|r| r.get("current_round"))
        .and_then(Value::as_u64)
    {
        if target > cur {
            return Err(not_found());
        }
    }
    if let Some(findings) = obj.get_mut("findings").and_then(Value::as_array_mut) {
        findings.retain(|f| f.get("round_number").and_then(Value::as_u64) == Some(target));
    }
    Ok(())
}

/// Retains only unified-diff hunks whose new-file line range overlaps `[start_line, end_line]`.
pub(super) fn filter_diff_by_line_range(
    diff: &mut ReviewDiffResult,
    start_line: Option<u32>,
    end_line: Option<u32>,
) {
    if start_line.is_none() && end_line.is_none() || diff.unified_diff.is_empty() {
        return;
    }
    let lo = start_line.unwrap_or(1);
    let hi = end_line.unwrap_or(u32::MAX);
    let mut out = String::new();
    for block in split_diff_blocks(&diff.unified_diff) {
        append_matching_hunks(&mut out, block, lo, hi);
    }
    diff.unified_diff = out;
}

fn split_diff_blocks(unified: &str) -> Vec<&str> {
    let mut starts = Vec::new();
    let mut offset = 0usize;
    for line in unified.split_inclusive('\n') {
        if line.starts_with("diff --git ") {
            starts.push(offset);
        }
        offset += line.len();
    }
    if starts.is_empty() {
        return vec![unified];
    }
    let mut blocks = Vec::with_capacity(starts.len());
    for (idx, &start) in starts.iter().enumerate() {
        let end = starts.get(idx + 1).copied().unwrap_or(unified.len());
        blocks.push(&unified[start..end]);
    }
    blocks
}

fn append_matching_hunks(out: &mut String, block: &str, lo: u32, hi: u32) {
    let mut header = String::new();
    let mut hunks: Vec<(&str, String)> = Vec::new();
    let mut in_hunk = false;
    for line in block.split_inclusive('\n') {
        if line.starts_with("@@ ") {
            in_hunk = true;
            hunks.push((line, line.to_owned()));
        } else if in_hunk {
            if let Some((_, body)) = hunks.last_mut() {
                body.push_str(line);
            }
        } else {
            header.push_str(line);
        }
    }
    let matching: Vec<&str> = hunks
        .iter()
        .filter(|(hdr, _)| hunk_overlaps(hdr, lo, hi))
        .map(|(_, body)| body.as_str())
        .collect();
    if matching.is_empty() {
        return;
    }
    out.push_str(&header);
    for hunk in matching {
        out.push_str(hunk);
    }
}

fn hunk_overlaps(header_line: &str, lo: u32, hi: u32) -> bool {
    let Some(rest) = header_line.strip_prefix("@@ ") else {
        return false;
    };
    let Some(plus_part) = rest.split_whitespace().find(|tok| tok.starts_with('+')) else {
        return false;
    };
    let spec = &plus_part[1..];
    let (start_str, count_str) = spec.split_once(',').unwrap_or((spec, "1"));
    let Ok(start) = start_str.parse::<u32>() else {
        return false;
    };
    let Ok(count) = count_str.parse::<u32>() else {
        return false;
    };
    let end = if count == 0 {
        start
    } else {
        start.saturating_add(count - 1)
    };
    end >= lo && start <= hi
}
