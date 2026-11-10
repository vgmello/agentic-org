//! Plain-text status policy and its presence projection, shared by all members.
use crate::time;
use org_proto::directory::{
    MemberPresence, MemberPresenceChangedData, StatusSetParams, StatusSetResult,
    DEFAULT_STATUS_TTL_MS, MAX_STATUS_TTL_MS, STATUS_MAX_BYTES, STATUS_MAX_CHARS,
};
use org_proto::{ApiError, ErrorCode};

/// The line `params` ask for, checked: `Ok(None)` clears, `Ok(Some((text, ttl_ms)))` sets.
pub(crate) fn check(params: &StatusSetParams) -> Result<Option<(String, u64)>, ApiError> {
    let ttl = params.ttl_ms.unwrap_or(DEFAULT_STATUS_TTL_MS);
    if ttl > MAX_STATUS_TTL_MS {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "the status lasts too long",
        ));
    }
    // A line that lasts no time is no line.
    if ttl == 0 {
        return Ok(None);
    }
    let Some(text) = params.text.as_deref() else {
        return Ok(None);
    };
    // Unicode whitespace around the line goes too (it is trimmed, then must be one line).
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    if text.len() > STATUS_MAX_BYTES || text.chars().count() > STATUS_MAX_CHARS {
        return Err(ApiError::new(ErrorCode::TooLarge, "the status is too long"));
    }
    if text
        .chars()
        .any(|c| c.is_control() || is_invisible_or_break(c))
    {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "the status is one line of plain text",
        ));
    }
    Ok(Some((text.to_owned(), ttl)))
}

/// What `char::is_control` lets through but a one-line plain-text status must not hold: the
/// Unicode line and paragraph separators (line breaks), and the characters that hide text or
/// reorder it: the zero-width space, bidirectional embeddings, overrides and isolates, the word
/// joiner and invisible operators, the deprecated formatting characters, the byte order mark, the
/// invisible fillers, the tag block (the usual way to hide instructions in text another agent
/// reads) and the variation selectors supplement. The zero-width
/// joiner and non-joiner and the left/right marks stay: emoji sequences and Persian, Indic and
/// mixed-direction text need them.
fn is_invisible_or_break(c: char) -> bool {
    matches!(
        c,
        '\u{2028}'
            | '\u{2029}'
            | '\u{200b}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206f}'
            | '\u{feff}'
            | '\u{180e}'
            | '\u{fff9}'..='\u{fffb}'
            // The fillers that render as nothing.
            | '\u{115f}'
            | '\u{1160}'
            | '\u{3164}'
            | '\u{ffa0}'
            // The tag block and the variation selectors supplement (a run of them hides bytes
            // behind one emoji).
            | '\u{e0000}'..='\u{e007f}'
            | '\u{e0100}'..='\u{e01ef}'
    )
}

/// The `member.presence_changed` data for a member whose status line was just set or cleared
/// (`set` is what `status.set` answered). Presence itself is not reported yet, so the member
/// reads as offline beside its line. `last_seen_at` is now: the member just called the office, so
/// it was alive a moment ago (`who` has no last-seen time to compare with yet).
pub fn presence_after_status(member_id: &str, set: &StatusSetResult) -> MemberPresenceChangedData {
    MemberPresenceChangedData {
        member_id: member_id.to_owned(),
        presence: MemberPresence::Offline,
        last_seen_at: time::now_rfc3339(),
        status: set.status.clone(),
        status_expires_at: set.expires_at.clone(),
        current_task_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn text(text: &str) -> StatusSetParams {
        StatusSetParams {
            text: Some(text.to_owned()),
            ttl_ms: None,
        }
    }
    fn code(params: &StatusSetParams) -> ErrorCode {
        check(params).unwrap_err().code
    }

    #[test]
    fn a_line_is_trimmed_and_an_empty_one_clears() {
        assert_eq!(
            check(&text("  Reviewing  ")).unwrap(),
            Some(("Reviewing".to_owned(), DEFAULT_STATUS_TTL_MS))
        );
        for clear in ["", "   ", "\u{a0}\t "] {
            assert_eq!(check(&text(clear)).unwrap(), None, "{clear:?}");
        }
        assert_eq!(check(&StatusSetParams::default()).unwrap(), None);
    }

    #[test]
    fn the_limits_are_chars_and_bytes_and_a_line_is_one_line() {
        assert!(check(&text(&"x".repeat(STATUS_MAX_CHARS))).is_ok());
        assert_eq!(
            code(&text(&"x".repeat(STATUS_MAX_CHARS + 1))),
            ErrorCode::TooLarge
        );
        // 140 characters but over 256 bytes.
        assert_eq!(
            code(&text(&"é".repeat(STATUS_MAX_CHARS))),
            ErrorCode::TooLarge
        );
        for bad in [
            "two\nlines",
            "tab\there",
            "bell\u{7}",
            "nul\0",
            "esc\u{1b}[0m",
            "line\u{2028}break",
            "para\u{2029}graph",
            "hidden\u{200b}gap",
            "flip\u{202e}ed",
            "bom\u{feff}",
            "joiner\u{2060}",
            "invisible\u{2064}times",
            "isolate\u{2066}x",
            "isolate\u{2069}",
            "deprecated\u{206a}",
            "deprecated\u{206f}",
            "embed\u{202a}",
            "tag\u{e0041}",
            "tag\u{e007f}",
            "selector\u{e0100}",
            "selector\u{e01ef}",
            "mongolian\u{180e}",
            "annotation\u{fff9}",
            "annotation\u{fffb}",
            "filler\u{115f}",
            "filler\u{1160}",
            "filler\u{3164}",
            "filler\u{ffa0}",
        ] {
            assert_eq!(code(&text(bad)), ErrorCode::InvalidRequest, "{bad:?}");
        }
    }

    #[test]
    fn a_ttl_of_nothing_clears_and_ordinary_unicode_is_kept() {
        let zero = StatusSetParams {
            text: Some("x".into()),
            ttl_ms: Some(0),
        };
        assert_eq!(check(&zero).unwrap(), None);
        for fine in [
            "Réviewing — naïve ✓",
            "日本語のステータス",
            "emoji 🚧 ok",
            // Sequences that need the joiners and marks.
            "👩\u{200d}💻 pairing",
            "می\u{200c}خواهم",
            "a\u{200e}b\u{200f}c",
        ] {
            assert!(check(&text(fine)).unwrap().is_some(), "{fine}");
        }
    }

    #[test]
    fn the_ttl_defaults_and_is_capped() {
        let with = |ttl| StatusSetParams {
            text: Some("x".into()),
            ttl_ms: Some(ttl),
        };
        assert_eq!(check(&with(1)).unwrap().unwrap().1, 1);
        assert_eq!(
            check(&with(MAX_STATUS_TTL_MS)).unwrap().unwrap().1,
            MAX_STATUS_TTL_MS
        );
        assert_eq!(
            code(&with(MAX_STATUS_TTL_MS + 1)),
            ErrorCode::InvalidRequest
        );
        // The ttl is checked even when the line clears.
        let clearing = StatusSetParams {
            text: None,
            ttl_ms: Some(MAX_STATUS_TTL_MS + 1),
        };
        assert_eq!(code(&clearing), ErrorCode::InvalidRequest);
    }
}
