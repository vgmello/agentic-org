//! `@handle` mention extraction (`doc-90`, `ADR-0024`, `WATCH-1/2/9`).
//!
//! Rules from `doc-90` §File/SQL mapping:
//! - Outside inline code (`` `...` ``, respecting multi-backtick spans) and fenced code blocks (`` ```...``` ``).
//! - An `@` that starts the text or follows a character that is neither a letter nor a digit.
//! - Exact case-insensitive match against an active employee handle after stripping trailing
//!   `.`, `-`, or `_` punctuation (never prefix-stripping into a different word such as `@adam` -> `ada`).
//! - `@here` mentions nobody; unresolved `@tokens` are reported in `unmatched_mentions`.

use std::collections::{BTreeMap, BTreeSet};

use org_proto::chat::MentionedMember;
use org_store::Id;

/// Candidate member eligible to be mentioned in a conversation.
#[derive(Debug, Clone)]
pub struct MentionCandidate {
    pub employee_id: Id,
    pub handle: String,
    pub display_name: String,
}

/// Result of scanning a message body for `@handle` mentions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MentionScan {
    pub resolved: Vec<MentionedMember>,
    pub unmatched: Vec<String>,
}

/// Extracts resolved `@handle` mentions and unmatched `@tokens` from `body`.
pub fn extract_mentions(body: &str, candidates: &[MentionCandidate]) -> MentionScan {
    let mut by_lower: BTreeMap<String, &MentionCandidate> = BTreeMap::new();
    for c in candidates {
        by_lower.entry(c.handle.to_lowercase()).or_insert(c);
    }
    let tokens = scan_mention_tokens(body);
    let mut seen_ids = BTreeSet::new();
    let mut seen_unmatched = BTreeSet::new();
    let mut scan = MentionScan::default();

    for token in tokens {
        let lower = token.to_lowercase();
        if lower == "here" {
            continue;
        }
        if let Some(candidate) = by_lower.get(&lower) {
            if seen_ids.insert(candidate.employee_id.clone()) {
                scan.resolved.push(MentionedMember {
                    employee_id: candidate.employee_id.as_str().to_owned(),
                    handle: candidate.handle.clone(),
                    display_name: candidate.display_name.clone(),
                });
            }
        } else if seen_unmatched.insert(lower) {
            scan.unmatched.push(token);
        }
    }
    scan
}

fn is_handle_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '-' | '_' | '.')
}

fn count_backticks(s: &str) -> usize {
    s.bytes().take_while(|&b| b == b'`').count()
}

fn consume_mention_at(body: &str, start: usize, out: &mut Vec<String>) -> usize {
    let mut end = start;
    for next in body[start..].chars() {
        if is_handle_char(next) {
            end += next.len_utf8();
        } else {
            break;
        }
    }
    let raw = body[start..end].trim_end_matches(['.', '-', '_']);
    if !raw.is_empty() {
        out.push(raw.to_owned());
    }
    end
}

fn update_code_state(
    body: &str,
    i: usize,
    in_fence: &mut bool,
    inline_ticks: &mut usize,
) -> Option<usize> {
    if *inline_ticks == 0 && body[i..].starts_with("```") {
        let run = count_backticks(&body[i..]);
        *in_fence = !*in_fence;
        return Some(run);
    }
    if !*in_fence && body.as_bytes()[i] == b'`' {
        let run = count_backticks(&body[i..]);
        if *inline_ticks == 0 {
            *inline_ticks = run;
        } else if *inline_ticks == run {
            *inline_ticks = 0;
        }
        return Some(run);
    }
    None
}

/// Scans `@token` occurrences outside CommonMark inline code spans and fenced code blocks.
fn scan_mention_tokens(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    let mut in_fence = false;
    let mut inline_ticks: usize = 0;
    let mut prev_char: Option<char> = None;

    while i < body.len() {
        if let Some(run) = update_code_state(body, i, &mut in_fence, &mut inline_ticks) {
            i += run;
            prev_char = Some('`');
            continue;
        }
        let ch = body[i..].chars().next().expect("valid UTF-8 char");
        let ch_len = ch.len_utf8();
        let is_mention_start = !in_fence
            && inline_ticks == 0
            && ch == '@'
            && prev_char.is_none_or(|p| !p.is_alphanumeric());
        if is_mention_start {
            let end = consume_mention_at(body, i + ch_len, &mut out);
            prev_char = body[i..end].chars().next_back();
            i = end;
            continue;
        }
        prev_char = Some(ch);
        i += ch_len;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u8) -> Id {
        Id::parse(&format!("0190a5d2-7c3e-7a01-8b2c-3d4e5f6071{n:02x}")).unwrap()
    }

    #[test]
    fn extracts_mentions_ignoring_code_emails_here_prefixes_and_trailing_punctuation() {
        let candidates = vec![
            MentionCandidate {
                employee_id: id(1),
                handle: "ada".into(),
                display_name: "Ada".into(),
            },
            MentionCandidate {
                employee_id: id(2),
                handle: "ada-dev".into(),
                display_name: "Ada Dev".into(),
            },
        ];
        let body = "Hey @ada-dev. Please check `@ada`, ``@ada``, and ```\n@ada\n``` \
                    plus user@ada.com, @here, @Ada_, @adam, @ada-ghost, and @ghost!";
        let scan = extract_mentions(body, &candidates);
        assert_eq!(scan.resolved.len(), 2);
        assert_eq!(scan.resolved[0].handle, "ada-dev");
        assert_eq!(scan.resolved[1].handle, "ada");
        assert_eq!(scan.unmatched, vec!["adam", "ada-ghost", "ghost"]);
    }
}
